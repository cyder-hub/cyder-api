use std::collections::HashMap;

use axum::{body::Bytes, http::HeaderMap};
use reqwest::{
    Url,
    header::{ACCEPT_ENCODING, AUTHORIZATION, CONTENT_LENGTH, HOST},
};
use serde_json::{Map, Value, json};

use crate::{
    proxy::{
        ProxyError, protocol_transform_error,
        runtime::{
            credential::{ProviderCredentials, apply_provider_request_auth_header},
            reasoning_content_repair::{
                ReasoningContentRepairRequest, repair_openai_reasoning_content,
            },
            request_patch::apply_request_patches,
            route_resolver::{ExecutionTarget, ReasoningConfigSource},
            transport::ProxyResponseMode,
        },
        util::{determine_target_api_type, format_model_str},
        utility::{UtilityOperation, UtilityProtocol},
    },
    schema::enum_def::LlmApiType,
    service::{
        cache::types::{CacheModel, CacheProvider, RuntimeResolvedRequestPatch},
        runtime::{ReasoningContinuationScope, ReasoningContinuationStore},
        transform::{finalize_request_data, transform_request_data},
    },
};
use cyder_tools::log::debug;

pub(in crate::proxy) struct MaterializedRequest {
    pub final_url: String,
    pub final_headers: HeaderMap,
    pub final_body: Bytes,
    pub model_str: String,
    pub response_mode: ProxyResponseMode,
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

fn target_has_explicit_reasoning_disabled(target: &ExecutionTarget) -> bool {
    matches!(
        target.reasoning_config_source,
        Some(ReasoningConfigSource::ModelDisabled)
    ) || matches!(
        target.reasoning_preset,
        Some(crate::database::reasoning_config::ReasoningPreset::Disabled)
    )
}

fn reasoning_continuation_scope(
    target: &ExecutionTarget,
    downstream_api_key_id: i64,
) -> ReasoningContinuationScope {
    ReasoningContinuationScope {
        api_key_id: downstream_api_key_id,
        provider_id: target.provider.id,
        model_id: target.model.id,
    }
}

async fn repair_generation_request_body(
    target: &ExecutionTarget,
    final_body_value: &mut Value,
    downstream_api_key_id: i64,
    target_api_type: LlmApiType,
    reasoning_continuation_store: &dyn ReasoningContinuationStore,
) -> Result<(), ProxyError> {
    repair_openai_reasoning_content(ReasoningContentRepairRequest {
        body: final_body_value,
        scope: reasoning_continuation_scope(target, downstream_api_key_id),
        store: reasoning_continuation_store,
        feature_enabled: target
            .runtime_features
            .openai_reasoning_content_repair_enabled,
        target_is_openai_compatible_generation: is_openai_compatible_generation_target(
            target_api_type,
        ),
        explicit_reasoning_disabled: target_has_explicit_reasoning_disabled(target),
        now_ms: chrono::Utc::now().timestamp_millis(),
    })
    .await
    .map_err(|err| ProxyError::InternalError(format!("reasoning content repair failed: {err}")))?;
    Ok(())
}

pub(in crate::proxy) async fn materialize_generation_request(
    target: &ExecutionTarget,
    mut data: Value,
    user_api_type: LlmApiType,
    is_stream: bool,
    original_headers: &HeaderMap,
    query_params: &HashMap<String, String>,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    downstream_api_key_id: i64,
    reasoning_continuation_store: &dyn ReasoningContinuationStore,
) -> Result<MaterializedRequest, ProxyError> {
    let target_api_type = target.llm_api_type;
    data = transform_request_data(data, user_api_type, target_api_type, is_stream);
    let prepared_request = prepare_generation_request(
        &target.provider,
        &target.model,
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
    let final_url = prepared_request.final_url;
    let mut final_body_value = prepared_request.final_body_value;
    repair_generation_request_body(
        target,
        &mut final_body_value,
        downstream_api_key_id,
        target_api_type,
        reasoning_continuation_store,
    )
    .await?;
    let final_body =
        Bytes::from(serde_json::to_vec(&final_body_value).map_err(|err| {
            protocol_transform_error("Failed to serialize final request body", err)
        })?);
    Ok(MaterializedRequest {
        final_url,
        final_headers: prepared_request.final_headers,
        final_body,
        model_str: format_model_str(&target.provider, &target.model),
        response_mode: ProxyResponseMode::Generation {
            api_type: user_api_type,
            target_api_type,
        },
    })
}

pub(in crate::proxy) async fn materialize_utility_request(
    target: &ExecutionTarget,
    operation: &UtilityOperation,
    data: Value,
    original_headers: &HeaderMap,
    query_params: &HashMap<String, String>,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
) -> Result<MaterializedRequest, ProxyError> {
    let (final_url, final_headers, final_body_value, provider_api_key_id) = match operation.protocol
    {
        UtilityProtocol::OpenaiCompatible => {
            prepare_llm_request(
                &target.provider,
                &target.model,
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
                &target.provider,
                &target.model,
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
    debug_assert_eq!(provider_api_key_id, provider_credentials.key_id);
    let final_body =
        Bytes::from(serde_json::to_vec(&final_body_value).map_err(|err| {
            protocol_transform_error("Failed to serialize final request body", err)
        })?);

    Ok(MaterializedRequest {
        final_url,
        final_headers,
        final_body,
        model_str: format_model_str(&target.provider, &target.model),
        response_mode: ProxyResponseMode::Utility {
            api_type: operation.api_type,
        },
    })
}
