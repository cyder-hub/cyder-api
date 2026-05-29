use std::collections::HashMap;

use axum::{body::Bytes, http::HeaderMap};
use reqwest::{
    Url,
    header::{ACCEPT_ENCODING, AUTHORIZATION, CONTENT_LENGTH, HOST},
};
use serde_json::{Map, Value, json};

use crate::{
    proxy::{
        ProxyError,
        logging::LoggedBody,
        protocol_transform_error,
        runtime::{
            credential::{ProviderCredentials, apply_provider_request_auth_header},
            reasoning_content_repair::{
                ReasoningContentRepairDiagnostic, ReasoningContentRepairReport,
                ReasoningContentRepairRequest, ReasoningContentRepairResultKey,
                repair_openai_reasoning_content,
            },
            request_patch::{apply_request_patches, rebuild_gemini_url_query_from_snapshot},
            route_resolver::{ExecutionCandidate, ReasoningConfigSource},
            transport::ProxyResponseMode,
        },
        util::{determine_target_api_type, format_model_str},
        utility::{UtilityOperation, UtilityProtocol},
    },
    schema::enum_def::LlmApiType,
    service::{
        cache::types::{CacheModel, CacheProvider, RuntimeResolvedRequestPatch},
        runtime::{ReasoningContinuationScope, ReasoningContinuationStore},
        transform::{
            finalize_request_data, transform_request_data_with_diagnostics,
            unified::{
                UnifiedTransformDiagnostic, UnifiedTransformDiagnosticAction,
                UnifiedTransformDiagnosticKind, UnifiedTransformDiagnosticLossLevel,
            },
        },
    },
    utils::storage::RequestLogBundleQueryParam,
};
use cyder_tools::log::debug;

pub(in crate::proxy) struct MaterializedAttemptRequest {
    pub final_url: String,
    pub final_headers: HeaderMap,
    pub final_body: Bytes,
    pub llm_request_body_for_log: Option<LoggedBody>,
    pub transform_diagnostics: Vec<UnifiedTransformDiagnostic>,
    pub reasoning_repair_report: Option<ReasoningContentRepairReport>,
    pub model_str: String,
    pub response_mode: ProxyResponseMode,
    pub provider_api_key_id: i64,
}

struct PreparedGenerationRequest {
    final_url: String,
    final_headers: HeaderMap,
    final_body_value: Value,
    provider_api_key_id: i64,
}

#[derive(Debug)]
enum GenerationPrepareKind {
    Llm { path: &'static str },
    Gemini { is_stream: bool },
}

fn select_generation_prepare_kind(
    target_api_type: LlmApiType,
    is_stream: bool,
) -> Result<GenerationPrepareKind, ProxyError> {
    match target_api_type {
        LlmApiType::Openai | LlmApiType::GeminiOpenai => Ok(GenerationPrepareKind::Llm {
            path: "chat/completions",
        }),
        LlmApiType::Ollama => Ok(GenerationPrepareKind::Llm { path: "api/chat" }),
        LlmApiType::Gemini => Ok(GenerationPrepareKind::Gemini { is_stream }),
        _ => Err(ProxyError::InternalError(format!(
            "unsupported generation target api type: {:?}",
            target_api_type
        ))),
    }
}

fn build_gemini_headers(
    original_headers: &HeaderMap,
    provider: &CacheProvider,
    api_key: &str,
) -> Result<HeaderMap, ProxyError> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in original_headers.iter() {
        if name != HOST
            && name != CONTENT_LENGTH
            && name != ACCEPT_ENCODING
            && name != "x-api-key"
            && name != "x-goog-api-key"
            && name != AUTHORIZATION
        {
            headers.insert(name.clone(), value.clone());
        }
    }

    apply_provider_request_auth_header(&mut headers, provider, LlmApiType::Gemini, api_key)?;

    Ok(headers)
}

fn build_gemini_url(
    provider: &CacheProvider,
    real_model_name: &str,
    action: &str,
    params: &HashMap<String, String>,
    is_stream: bool,
) -> Result<Url, ProxyError> {
    let target_url_str = format!("{}/{}:{}", provider.endpoint, real_model_name, action);
    let mut url = Url::parse(&target_url_str)
        .map_err(|_| ProxyError::BadRequest("failed to parse target url".to_string()))?;

    for (k, v) in params {
        if k != "key" {
            url.query_pairs_mut().append_pair(k, v);
        }
    }

    if is_stream {
        url.query_pairs_mut().append_pair("alt", "sse");
    }

    Ok(url)
}

fn build_new_headers(
    pre_headers: &HeaderMap,
    provider: &CacheProvider,
    target_api_type: LlmApiType,
    api_key: &str,
) -> Result<HeaderMap, ProxyError> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in pre_headers.iter() {
        if name != HOST && name != CONTENT_LENGTH && name != ACCEPT_ENCODING && name != "x-api-key"
        {
            headers.insert(name.clone(), value.clone());
        }
    }
    apply_provider_request_auth_header(&mut headers, provider, target_api_type, api_key)?;
    Ok(headers)
}

fn resolve_real_model_name(model: &CacheModel) -> &str {
    model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name)
}

fn ensure_request_body_object(data: &mut Value) {
    if !matches!(data, Value::Object(_)) {
        *data = Value::Object(Map::new());
    }
}

async fn prepare_llm_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    path: &str,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let target_url = format!("{}/{}", provider.endpoint, path);
    let mut url = Url::parse(&target_url)
        .map_err(|_| ProxyError::BadRequest("failed to parse target url".to_string()))?;
    let target_api_type = determine_target_api_type(provider);
    let mut headers = build_new_headers(
        original_headers,
        provider,
        target_api_type,
        &provider_credentials.request_key,
    )?;

    ensure_request_body_object(&mut data);
    if let Value::Object(obj) = &mut data {
        obj.insert("model".to_string(), json!(resolve_real_model_name(model)));
    }

    data = finalize_request_data(data, LlmApiType::Openai, &provider.provider_type, path);
    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;

    Ok((url.to_string(), headers, data, provider_credentials.key_id))
}

async fn prepare_generation_request(
    provider: &CacheProvider,
    model: &CacheModel,
    data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    target_api_type: LlmApiType,
    is_stream: bool,
    params: &HashMap<String, String>,
) -> Result<PreparedGenerationRequest, ProxyError> {
    match select_generation_prepare_kind(target_api_type, is_stream)? {
        GenerationPrepareKind::Llm { path } => {
            let (final_url, final_headers, final_body_value, provider_api_key_id) =
                prepare_llm_request(
                    provider,
                    model,
                    data,
                    original_headers,
                    request_patches,
                    provider_credentials,
                    path,
                )
                .await?;
            Ok(PreparedGenerationRequest {
                final_url,
                final_headers,
                final_body_value,
                provider_api_key_id,
            })
        }
        GenerationPrepareKind::Gemini { is_stream } => {
            let (final_url, final_headers, final_body_value, provider_api_key_id) =
                prepare_gemini_llm_request(
                    provider,
                    model,
                    data,
                    original_headers,
                    request_patches,
                    provider_credentials,
                    is_stream,
                    params,
                )
                .await?;
            Ok(PreparedGenerationRequest {
                final_url,
                final_headers,
                final_body_value,
                provider_api_key_id,
            })
        }
    }
}

async fn prepare_simple_gemini_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    action: &str,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing simple Gemini request for provider: {}, model: {}, action: {}",
        provider.name, model.model_name, action
    );

    let real_model_name = resolve_real_model_name(model);
    let mut url = build_gemini_url(provider, real_model_name, action, params, false)?;
    let mut headers = build_gemini_headers(
        original_headers,
        provider,
        &provider_credentials.request_key,
    )?;
    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;

    Ok((url.to_string(), headers, data, provider_credentials.key_id))
}

async fn prepare_gemini_llm_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    is_stream: bool,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing Gemini LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let real_model_name = resolve_real_model_name(model);
    let action = if is_stream {
        "streamGenerateContent"
    } else {
        "generateContent"
    };
    let mut url = build_gemini_url(provider, real_model_name, action, params, is_stream)?;
    let mut headers = build_gemini_headers(
        original_headers,
        provider,
        &provider_credentials.request_key,
    )?;

    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;

    Ok((url.to_string(), headers, data, provider_credentials.key_id))
}

fn is_openai_compatible_generation_target(target_api_type: LlmApiType) -> bool {
    matches!(
        target_api_type,
        LlmApiType::Openai | LlmApiType::GeminiOpenai
    )
}

fn candidate_has_explicit_reasoning_disabled(candidate: &ExecutionCandidate) -> bool {
    matches!(
        candidate.reasoning_config_source,
        Some(ReasoningConfigSource::ModelDisabled)
    ) || matches!(
        candidate.reasoning_preset,
        Some(crate::database::reasoning_config::ReasoningPreset::Disabled)
    )
}

fn reasoning_continuation_scope(
    candidate: &ExecutionCandidate,
    downstream_api_key_id: i64,
) -> ReasoningContinuationScope {
    ReasoningContinuationScope {
        api_key_id: downstream_api_key_id,
        provider_id: candidate.provider.id,
        model_id: candidate.model.id,
        route_id: candidate.route_id,
        route_name: candidate.route_name.clone(),
        candidate_position: candidate.candidate_position,
    }
}

async fn repair_generation_request_body(
    candidate: &ExecutionCandidate,
    final_body_value: &mut Value,
    downstream_api_key_id: i64,
    target_api_type: LlmApiType,
    reasoning_continuation_store: &dyn ReasoningContinuationStore,
) -> Result<Option<ReasoningContentRepairReport>, ProxyError> {
    let report = repair_openai_reasoning_content(ReasoningContentRepairRequest {
        body: final_body_value,
        scope: reasoning_continuation_scope(candidate, downstream_api_key_id),
        store: reasoning_continuation_store,
        feature_enabled: candidate
            .runtime_features
            .openai_reasoning_content_repair_enabled,
        target_is_openai_compatible_generation: is_openai_compatible_generation_target(
            target_api_type,
        ),
        explicit_reasoning_disabled: candidate_has_explicit_reasoning_disabled(candidate),
        now_ms: chrono::Utc::now().timestamp_millis(),
    })
    .await
    .map_err(|err| ProxyError::InternalError(format!("reasoning content repair failed: {err}")))?;

    if report.diagnostics.iter().all(|diagnostic| {
        matches!(
            diagnostic.result,
            ReasoningContentRepairResultKey::Disabled
                | ReasoningContentRepairResultKey::NotApplicable
        )
    }) {
        return Ok(Some(report));
    }

    Ok(Some(report))
}

fn reasoning_repair_transform_diagnostics(
    report: &ReasoningContentRepairReport,
) -> Vec<UnifiedTransformDiagnostic> {
    report
        .diagnostics
        .iter()
        .map(reasoning_repair_transform_diagnostic)
        .collect()
}

fn reasoning_repair_transform_diagnostic(
    diagnostic: &ReasoningContentRepairDiagnostic,
) -> UnifiedTransformDiagnostic {
    UnifiedTransformDiagnostic {
        type_: "runtime_feature_diagnostic".to_string(),
        diagnostic_kind: UnifiedTransformDiagnosticKind::CapabilityDowngrade,
        provider: "openai_compatible".to_string(),
        target_provider: "openai_compatible".to_string(),
        source: "cyder_runtime".to_string(),
        target: "upstream_request".to_string(),
        stream_id: None,
        stage: Some("request_repair".to_string()),
        loss_level: UnifiedTransformDiagnosticLossLevel::Lossless,
        action: UnifiedTransformDiagnosticAction::Send,
        semantic_unit: "reasoning_content".to_string(),
        reason: format!(
            "openai_reasoning_content_repair:{}",
            diagnostic.result.as_key()
        ),
        context: serde_json::to_string(diagnostic).ok(),
        raw_data_summary: None,
        recovery_hint: None,
    }
}

pub(in crate::proxy) async fn materialize_generation_attempt(
    candidate: &ExecutionCandidate,
    mut data: Value,
    user_api_type: LlmApiType,
    is_stream: bool,
    _original_request_value: &Value,
    original_headers: &HeaderMap,
    query_params: &HashMap<String, String>,
    replay_query_params: Option<&[RequestLogBundleQueryParam]>,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    downstream_api_key_id: i64,
    reasoning_continuation_store: &dyn ReasoningContinuationStore,
) -> Result<MaterializedAttemptRequest, ProxyError> {
    let target_api_type = candidate.llm_api_type;
    let transform_output =
        transform_request_data_with_diagnostics(data, user_api_type, target_api_type, is_stream);
    data = transform_output.value;
    let prepared_request = prepare_generation_request(
        &candidate.provider,
        &candidate.model,
        data,
        original_headers,
        request_patches,
        provider_credentials,
        target_api_type,
        is_stream,
        query_params,
    )
    .await?;
    debug_assert_eq!(
        prepared_request.provider_api_key_id,
        provider_credentials.key_id
    );
    let final_url = if target_api_type == LlmApiType::Gemini {
        match replay_query_params {
            Some(params) => rebuild_gemini_url_query_from_snapshot(
                &prepared_request.final_url,
                params,
                is_stream,
                request_patches,
            )?,
            None => prepared_request.final_url,
        }
    } else {
        prepared_request.final_url
    };
    let mut final_body_value = prepared_request.final_body_value;
    let reasoning_repair_report = repair_generation_request_body(
        candidate,
        &mut final_body_value,
        downstream_api_key_id,
        target_api_type,
        reasoning_continuation_store,
    )
    .await?;
    let mut transform_diagnostics = transform_output.diagnostics;
    if let Some(report) = reasoning_repair_report.as_ref() {
        transform_diagnostics.extend(reasoning_repair_transform_diagnostics(report));
    }
    let final_body =
        Bytes::from(serde_json::to_vec(&final_body_value).map_err(|err| {
            protocol_transform_error("Failed to serialize final request body", err)
        })?);
    Ok(MaterializedAttemptRequest {
        final_url,
        final_headers: prepared_request.final_headers,
        llm_request_body_for_log: Some(LoggedBody::from_bytes(final_body.clone())),
        transform_diagnostics,
        reasoning_repair_report,
        final_body,
        model_str: format_model_str(&candidate.provider, &candidate.model),
        response_mode: ProxyResponseMode::Generation {
            api_type: user_api_type,
            target_api_type,
        },
        provider_api_key_id: prepared_request.provider_api_key_id,
    })
}

pub(in crate::proxy) async fn materialize_utility_attempt(
    candidate: &ExecutionCandidate,
    operation: &UtilityOperation,
    data: Value,
    original_headers: &HeaderMap,
    query_params: &HashMap<String, String>,
    replay_query_params: Option<&[RequestLogBundleQueryParam]>,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
) -> Result<MaterializedAttemptRequest, ProxyError> {
    let (final_url, final_headers, final_body_value, provider_api_key_id) = match operation.protocol
    {
        UtilityProtocol::OpenaiCompatible => {
            prepare_llm_request(
                &candidate.provider,
                &candidate.model,
                data,
                original_headers,
                request_patches,
                provider_credentials,
                &operation.downstream_path,
            )
            .await?
        }
        UtilityProtocol::GeminiCompatible => {
            prepare_simple_gemini_request(
                &candidate.provider,
                &candidate.model,
                data,
                original_headers,
                request_patches,
                provider_credentials,
                &operation.downstream_path,
                query_params,
            )
            .await?
        }
    };
    let final_url = match (operation.protocol, replay_query_params) {
        (UtilityProtocol::GeminiCompatible, Some(params)) => {
            rebuild_gemini_url_query_from_snapshot(&final_url, params, false, request_patches)?
        }
        _ => final_url,
    };
    debug_assert_eq!(provider_api_key_id, provider_credentials.key_id);
    let final_body =
        Bytes::from(serde_json::to_vec(&final_body_value).map_err(|err| {
            protocol_transform_error("Failed to serialize final request body", err)
        })?);

    Ok(MaterializedAttemptRequest {
        final_url,
        final_headers,
        llm_request_body_for_log: Some(LoggedBody::from_bytes(final_body.clone())),
        transform_diagnostics: Vec::new(),
        reasoning_repair_report: None,
        final_body,
        model_str: format_model_str(&candidate.provider, &candidate.model),
        response_mode: ProxyResponseMode::Utility {
            api_type: operation.api_type,
        },
        provider_api_key_id,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::{
        HeaderMap, HeaderValue,
        header::{AUTHORIZATION, CONTENT_TYPE},
    };
    use serde_json::json;

    use super::*;
    use crate::{
        proxy::logging::LoggedBody,
        proxy::runtime::reasoning_content_repair::continuation_snapshot_from_parts,
        proxy::runtime::route_resolver::{
            CandidateRuntimeFeatures, ExecutionCandidate, RuntimeFeatureConfigSource,
        },
        schema::enum_def::{
            ProviderApiKeyMode, ProviderType, RequestPatchOperation, RequestPatchPlacement,
        },
        service::{
            cache::types::{CacheModel, CacheProvider, RequestPatchRuleOrigin, RequestPatchSource},
            runtime::{MemoryReasoningContinuationStore, ReasoningContinuationStore},
        },
        utils::storage::RequestLogBundleQueryParam,
    };

    fn provider(id: i64) -> Arc<CacheProvider> {
        Arc::new(CacheProvider {
            id,
            provider_key: format!("provider-{id}"),
            name: format!("Provider {id}"),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
        })
    }

    fn model(id: i64, supports_tools: bool, supports_image_input: bool) -> Arc<CacheModel> {
        Arc::new(CacheModel {
            id,
            provider_id: id,
            model_name: format!("model-{id}"),
            real_model_name: None,
            cost_catalog_id: None,
            supports_streaming: true,
            supports_tools,
            supports_reasoning: true,
            supports_image_input,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled: true,
        })
    }

    fn candidate(
        position: usize,
        supports_tools: bool,
        supports_image_input: bool,
    ) -> ExecutionCandidate {
        ExecutionCandidate {
            candidate_position: position,
            route_id: Some(1),
            route_name: Some("route".to_string()),
            route_candidate_priority: Some(position as i32),
            provider: provider(position as i64),
            model: model(position as i64, supports_tools, supports_image_input),
            llm_api_type: LlmApiType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            reasoning_config_id: None,
            reasoning_config_scope: None,
            reasoning_config_source: None,
            reasoning_config_preset_id: None,
            reasoning_family: None,
            reasoning_preset: None,
            reasoning_suffix: None,
            runtime_features: CandidateRuntimeFeatures {
                openai_reasoning_content_repair_enabled: false,
                openai_reasoning_content_repair_source: RuntimeFeatureConfigSource::DefaultFalse,
            },
        }
    }

    fn credentials() -> ProviderCredentials {
        ProviderCredentials {
            key_id: 42,
            request_key: "provider-secret".to_string(),
        }
    }

    fn store() -> MemoryReasoningContinuationStore {
        MemoryReasoningContinuationStore::default()
    }

    fn request_patch(target: &str, value: serde_json::Value) -> RuntimeResolvedRequestPatch {
        RuntimeResolvedRequestPatch {
            placement: RequestPatchPlacement::Body,
            target: target.to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some(serde_json::to_string(&value).unwrap()),
            source: RequestPatchSource::ProviderRule { rule_id: 77 },
            source_rule_id: Some(77),
            source_origin: Some(RequestPatchRuleOrigin::ProviderDirect),
            overridden_rule_ids: Vec::new(),
            overridden_sources: Vec::new(),
            description: None,
        }
    }

    fn enable_reasoning_repair(candidate: &mut ExecutionCandidate) {
        candidate
            .runtime_features
            .openai_reasoning_content_repair_enabled = true;
        candidate
            .runtime_features
            .openai_reasoning_content_repair_source = RuntimeFeatureConfigSource::ProviderDefault;
    }

    fn repair_tool_calls() -> serde_json::Value {
        json!([
            {
                "id": "call-weather",
                "type": "function",
                "function": { "name": "weather", "arguments": "{\"city\":\"Paris\"}" }
            }
        ])
    }

    async fn seed_reasoning_repair_store(
        store: &dyn ReasoningContinuationStore,
        candidate: &ExecutionCandidate,
        downstream_api_key_id: i64,
    ) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let snapshot = continuation_snapshot_from_parts(
            reasoning_continuation_scope(candidate, downstream_api_key_id),
            "MATERIALIZER_SECRET_REASONING",
            &repair_tool_calls(),
            now_ms,
        )
        .expect("snapshot parse should succeed")
        .expect("snapshot should exist");
        store
            .insert(snapshot, now_ms)
            .await
            .expect("repair store insert should succeed");
    }

    #[test]
    fn resolve_real_model_name_prefers_non_empty_real_name() {
        let aliased = CacheModel {
            real_model_name: Some("providers/acme/models/gpt-4.1".to_string()),
            ..(*model(1, true, true)).clone()
        };
        let empty_real_name = CacheModel {
            real_model_name: Some(String::new()),
            ..(*model(1, true, true)).clone()
        };
        let direct = CacheModel {
            real_model_name: None,
            ..(*model(1, true, true)).clone()
        };

        assert_eq!(
            resolve_real_model_name(&aliased),
            "providers/acme/models/gpt-4.1"
        );
        assert_eq!(resolve_real_model_name(&empty_real_name), "model-1");
        assert_eq!(resolve_real_model_name(&direct), "model-1");
    }

    #[test]
    fn select_generation_prepare_kind_maps_supported_generation_targets() {
        assert!(matches!(
            select_generation_prepare_kind(LlmApiType::Openai, false),
            Ok(GenerationPrepareKind::Llm {
                path: "chat/completions"
            })
        ));
        assert!(matches!(
            select_generation_prepare_kind(LlmApiType::GeminiOpenai, true),
            Ok(GenerationPrepareKind::Llm {
                path: "chat/completions"
            })
        ));
        assert!(matches!(
            select_generation_prepare_kind(LlmApiType::Ollama, false),
            Ok(GenerationPrepareKind::Llm { path: "api/chat" })
        ));
        assert!(matches!(
            select_generation_prepare_kind(LlmApiType::Gemini, true),
            Ok(GenerationPrepareKind::Gemini { is_stream: true })
        ));
    }

    #[test]
    fn select_generation_prepare_kind_rejects_non_generation_target() {
        let err = select_generation_prepare_kind(LlmApiType::Anthropic, false).unwrap_err();
        assert!(matches!(err, ProxyError::InternalError(_)));
        assert_eq!(
            err.to_string(),
            "[server_error] unsupported generation target api type: Anthropic"
        );
    }

    #[tokio::test]
    async fn materialize_openai_generation_attempt_prepares_chat_request() {
        let candidate = candidate(1, true, true);
        let store = store();
        let mut original_headers = HeaderMap::new();
        original_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        original_headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer user-key"));

        let materialized = materialize_generation_attempt(
            &candidate,
            json!({ "messages": [{ "role": "user", "content": "hello" }] }),
            LlmApiType::Openai,
            false,
            &json!({}),
            &original_headers,
            &HashMap::new(),
            None,
            &[],
            &credentials(),
            99,
            &store,
        )
        .await
        .unwrap();

        assert_eq!(
            materialized.final_url,
            "https://example.com/chat/completions"
        );
        assert_eq!(materialized.provider_api_key_id, 42);
        assert_eq!(
            materialized
                .final_headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer provider-secret")
        );
        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert_eq!(body["model"], "model-1");
        assert_eq!(body["messages"][0]["content"], "hello");
        assert!(matches!(
            materialized.response_mode,
            ProxyResponseMode::Generation {
                api_type: LlmApiType::Openai,
                target_api_type: LlmApiType::Openai
            }
        ));
    }

    #[tokio::test]
    async fn materialize_openai_generation_repair_updates_final_body_and_log_body() {
        let mut candidate = candidate(1, true, true);
        enable_reasoning_repair(&mut candidate);
        let store = store();
        let downstream_api_key_id = 99;
        seed_reasoning_repair_store(&store, &candidate, downstream_api_key_id).await;
        let mut original_headers = HeaderMap::new();
        original_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let materialized = materialize_generation_attempt(
            &candidate,
            json!({
                "messages": [
                    { "role": "user", "content": "weather" },
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": repair_tool_calls()
                    },
                    { "role": "tool", "tool_call_id": "call-weather", "content": "{}" }
                ]
            }),
            LlmApiType::Openai,
            false,
            &json!({}),
            &original_headers,
            &HashMap::new(),
            None,
            &[request_patch("/metadata/source", json!("patch-kept"))],
            &credentials(),
            downstream_api_key_id,
            &store,
        )
        .await
        .unwrap();

        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert_eq!(
            body["messages"][1]["reasoning_content"],
            "MATERIALIZER_SECRET_REASONING"
        );
        assert_eq!(body["metadata"]["source"], "patch-kept");
        let LoggedBody::InMemory { bytes, .. } = materialized
            .llm_request_body_for_log
            .as_ref()
            .expect("llm request body should be logged")
        else {
            panic!("llm request body should stay in memory in this test");
        };
        assert_eq!(bytes, &materialized.final_body);
        assert_eq!(
            materialized
                .reasoning_repair_report
                .as_ref()
                .expect("repair report should exist")
                .repaired_count,
            1
        );
        assert!(materialized.transform_diagnostics.iter().any(|diagnostic| {
            diagnostic.stage.as_deref() == Some("request_repair")
                && diagnostic
                    .reason
                    .contains("openai_reasoning_content_repair:matched")
        }));
        let diagnostics_json = serde_json::to_string(&materialized.transform_diagnostics)
            .expect("diagnostics should serialize");
        assert!(!diagnostics_json.contains("MATERIALIZER_SECRET_REASONING"));
    }

    #[tokio::test]
    async fn materialize_openai_generation_skips_repair_for_disabled_reasoning_preset() {
        let mut candidate = candidate(1, true, true);
        enable_reasoning_repair(&mut candidate);
        candidate.reasoning_preset =
            Some(crate::database::reasoning_config::ReasoningPreset::Disabled);
        let store = store();
        seed_reasoning_repair_store(&store, &candidate, 99).await;
        let original_headers = HeaderMap::new();

        let materialized = materialize_generation_attempt(
            &candidate,
            json!({
                "messages": [
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": repair_tool_calls()
                    }
                ]
            }),
            LlmApiType::Openai,
            false,
            &json!({}),
            &original_headers,
            &HashMap::new(),
            None,
            &[],
            &credentials(),
            99,
            &store,
        )
        .await
        .unwrap();

        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert!(body["messages"][0].get("reasoning_content").is_none());
        assert_eq!(
            materialized
                .reasoning_repair_report
                .as_ref()
                .expect("repair report should exist")
                .diagnostics[0]
                .result,
            ReasoningContentRepairResultKey::ExplicitReasoningDisabled
        );
    }

    #[tokio::test]
    async fn materialize_openai_generation_does_not_repair_across_candidate_scope() {
        let mut seed_candidate = candidate(1, true, true);
        enable_reasoning_repair(&mut seed_candidate);
        let store = store();
        let downstream_api_key_id = 99;
        seed_reasoning_repair_store(&store, &seed_candidate, downstream_api_key_id).await;

        let mut fallback_candidate = candidate(2, true, true);
        enable_reasoning_repair(&mut fallback_candidate);
        let materialized = materialize_generation_attempt(
            &fallback_candidate,
            json!({
                "messages": [
                    { "role": "user", "content": "weather" },
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": repair_tool_calls()
                    },
                    { "role": "tool", "tool_call_id": "call-weather", "content": "{}" }
                ]
            }),
            LlmApiType::Openai,
            false,
            &json!({}),
            &HeaderMap::new(),
            &HashMap::new(),
            None,
            &[],
            &credentials(),
            downstream_api_key_id,
            &store,
        )
        .await
        .unwrap();

        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert!(body["messages"][1].get("reasoning_content").is_none());
        let report = materialized
            .reasoning_repair_report
            .as_ref()
            .expect("repair report should exist");
        assert_eq!(report.repaired_count, 0);
        assert_eq!(
            report.diagnostics[0].result,
            ReasoningContentRepairResultKey::CacheMiss
        );
    }

    #[tokio::test]
    async fn materialize_gemini_generation_rebuilds_replay_query_flags() {
        let mut candidate = candidate(1, true, true);
        let store = store();
        candidate.provider = Arc::new(CacheProvider {
            endpoint: "https://example.com/v1beta/models".to_string(),
            provider_type: ProviderType::Gemini,
            ..(*candidate.provider).clone()
        });
        candidate.llm_api_type = LlmApiType::Gemini;
        let mut original_headers = HeaderMap::new();
        original_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        original_headers.insert("x-goog-api-key", HeaderValue::from_static("user-key"));
        let query_params = HashMap::from([
            ("foo".to_string(), "bar".to_string()),
            ("key".to_string(), "user-key".to_string()),
        ]);
        let replay_query_params = vec![
            RequestLogBundleQueryParam {
                name: "foo".to_string(),
                value: Some("bar".to_string()),
                value_present: true,
                encoded_name: None,
                encoded_value: None,
            },
            RequestLogBundleQueryParam {
                name: "key".to_string(),
                value: Some("user-key".to_string()),
                value_present: true,
                encoded_name: None,
                encoded_value: None,
            },
        ];

        let materialized = materialize_generation_attempt(
            &candidate,
            json!({ "contents": [{ "parts": [{ "text": "hello" }] }] }),
            LlmApiType::Gemini,
            true,
            &json!({}),
            &original_headers,
            &query_params,
            Some(&replay_query_params),
            &[],
            &credentials(),
            99,
            &store,
        )
        .await
        .unwrap();

        assert_eq!(
            materialized.final_url,
            "https://example.com/v1beta/models/model-1:streamGenerateContent?foo=bar&alt=sse"
        );
        assert_eq!(
            materialized
                .final_headers
                .get("x-goog-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("provider-secret")
        );
        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert_eq!(body["contents"][0]["parts"][0]["text"], "hello");
    }

    #[tokio::test]
    async fn materialize_ollama_generation_attempt_prepares_api_chat_request() {
        let mut candidate = candidate(1, true, true);
        let store = store();
        candidate.provider = Arc::new(CacheProvider {
            provider_type: ProviderType::Ollama,
            ..(*candidate.provider).clone()
        });
        candidate.llm_api_type = LlmApiType::Ollama;
        let mut original_headers = HeaderMap::new();
        original_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let materialized = materialize_generation_attempt(
            &candidate,
            json!({ "messages": [{ "role": "user", "content": "hello" }] }),
            LlmApiType::Ollama,
            false,
            &json!({}),
            &original_headers,
            &HashMap::new(),
            None,
            &[],
            &credentials(),
            99,
            &store,
        )
        .await
        .unwrap();

        assert_eq!(materialized.final_url, "https://example.com/api/chat");
        assert_eq!(
            materialized
                .final_headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer provider-secret")
        );
        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert_eq!(body["model"], "model-1");
        assert_eq!(body["messages"][0]["content"], "hello");
        assert!(matches!(
            materialized.response_mode,
            ProxyResponseMode::Generation {
                api_type: LlmApiType::Ollama,
                target_api_type: LlmApiType::Ollama
            }
        ));
    }

    #[tokio::test]
    async fn materialize_openai_utility_attempt_prepares_headers_uri_and_body_snapshot() {
        let candidate = candidate(1, true, true);
        let operation = UtilityOperation {
            name: "embeddings".to_string(),
            api_type: LlmApiType::Openai,
            protocol: UtilityProtocol::OpenaiCompatible,
            downstream_path: "embeddings".to_string(),
        };
        let mut original_headers = HeaderMap::new();
        original_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        original_headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer user-key"));

        let materialized = materialize_utility_attempt(
            &candidate,
            &operation,
            json!({ "input": "embed me" }),
            &original_headers,
            &HashMap::new(),
            None,
            &[],
            &credentials(),
        )
        .await
        .unwrap();

        assert_eq!(materialized.final_url, "https://example.com/embeddings");
        assert_eq!(materialized.provider_api_key_id, 42);
        assert_eq!(
            materialized
                .final_headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer provider-secret")
        );
        assert_eq!(
            materialized
                .final_headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert_eq!(body["model"], "model-1");
        assert_eq!(body["input"], "embed me");
        match materialized.llm_request_body_for_log.unwrap() {
            LoggedBody::InMemory { bytes, .. } => {
                assert_eq!(bytes, materialized.final_body);
            }
            LoggedBody::Spooled { .. } => panic!("small request body should stay in memory"),
        }
    }

    #[tokio::test]
    async fn materialize_gemini_utility_attempt_prepares_headers_uri_and_body_snapshot() {
        let mut candidate = candidate(1, true, true);
        candidate.provider = Arc::new(CacheProvider {
            endpoint: "https://example.com/v1beta/models".to_string(),
            provider_type: ProviderType::Gemini,
            ..(*candidate.provider).clone()
        });
        candidate.llm_api_type = LlmApiType::Gemini;
        let operation = UtilityOperation {
            name: "countTokens".to_string(),
            api_type: LlmApiType::Gemini,
            protocol: UtilityProtocol::GeminiCompatible,
            downstream_path: "countTokens".to_string(),
        };
        let mut original_headers = HeaderMap::new();
        original_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        original_headers.insert("x-goog-api-key", HeaderValue::from_static("user-key"));
        let query_params = HashMap::from([
            ("foo".to_string(), "bar".to_string()),
            ("key".to_string(), "user-key".to_string()),
        ]);

        let materialized = materialize_utility_attempt(
            &candidate,
            &operation,
            json!({ "contents": [{ "parts": [{ "text": "count this" }] }] }),
            &original_headers,
            &query_params,
            None,
            &[],
            &credentials(),
        )
        .await
        .unwrap();

        assert_eq!(
            materialized.final_url,
            "https://example.com/v1beta/models/model-1:countTokens?foo=bar"
        );
        assert_eq!(
            materialized
                .final_headers
                .get("x-goog-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("provider-secret")
        );
        assert_eq!(
            materialized
                .final_headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert_eq!(body["contents"][0]["parts"][0]["text"], "count this");
        match materialized.llm_request_body_for_log.unwrap() {
            LoggedBody::InMemory { bytes, .. } => {
                assert_eq!(bytes, materialized.final_body);
            }
            LoggedBody::Spooled { .. } => panic!("small request body should stay in memory"),
        }
    }
}
