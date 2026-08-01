use std::collections::HashMap;

use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue},
};
use reqwest::{
    Url,
    header::{ACCEPT_ENCODING, AUTHORIZATION, CONTENT_LENGTH, HOST},
};
use serde_json::{Map, Value, json};

use crate::{
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, protocol_transform_error,
        request_context::{ProxyRequestContext, X_CLIENT_REQUEST_ID, X_REQUEST_ID},
        runtime::{
            reasoning_content_repair::{
                ReasoningContentRepairRequest, repair_openai_reasoning_content,
            },
            request_patch::apply_request_patches,
            route_resolver::{ExecutionTarget, ReasoningConfigSource},
            transport::ProxyResponseMode,
        },
        util::{determine_upstream_protocol, format_model_str},
        utility::{UtilityOperation, UtilityProtocol},
    },
    schema::enum_def::{DownstreamProtocol, UpstreamProtocol},
    service::{
        cache::types::{CacheModel, CacheProvider, RuntimeResolvedRequestPatch},
        provider_credential::{ProviderCredential, apply_provider_request_auth_header},
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
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
) -> Result<GenerationPrepareKind, ProxyError> {
    match upstream_protocol {
        UpstreamProtocol::Openai => Ok(GenerationPrepareKind::Llm {
            path: "chat/completions",
        }),
        UpstreamProtocol::Ollama => Ok(GenerationPrepareKind::Llm { path: "api/chat" }),
        UpstreamProtocol::Gemini => Ok(GenerationPrepareKind::Gemini { is_stream }),
        _ => {
            let message =
                format!("unsupported generation upstream protocol: {upstream_protocol:?}");
            Err(ProxyError::gateway(
                ProxyErrorCode::UnsupportedCapabilityError,
                ExecutionStage::Capability,
                ResponseVisibility::NotVisible,
                Some(message.clone()),
                message,
            ))
        }
    }
}

fn build_gemini_headers(
    original_headers: &HeaderMap,
    provider: &CacheProvider,
    credential: &ProviderCredential,
) -> Result<HeaderMap, ProxyError> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in original_headers.iter() {
        if name != HOST
            && name != CONTENT_LENGTH
            && name != ACCEPT_ENCODING
            && name != "x-api-key"
            && name != "x-goog-api-key"
            && name != AUTHORIZATION
            && name != X_REQUEST_ID
            && name != X_CLIENT_REQUEST_ID
        {
            headers.insert(name.clone(), value.clone());
        }
    }

    apply_provider_request_auth_header(
        &mut headers,
        provider,
        UpstreamProtocol::Gemini,
        credential,
    )
    .map_err(|error| {
        ProxyError::gateway(
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Materialize,
            ResponseVisibility::NotVisible,
            None,
            error.to_string(),
        )
    })?;

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
    let mut url = Url::parse(&target_url_str).map_err(|error| {
        ProxyError::gateway(
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Materialize,
            ResponseVisibility::NotVisible,
            None,
            format!("failed to parse target url: {error}"),
        )
    })?;

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
    upstream_protocol: UpstreamProtocol,
    credential: &ProviderCredential,
) -> Result<HeaderMap, ProxyError> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in pre_headers.iter() {
        if name != HOST
            && name != CONTENT_LENGTH
            && name != ACCEPT_ENCODING
            && name != "x-api-key"
            && name != X_REQUEST_ID
            && name != X_CLIENT_REQUEST_ID
        {
            headers.insert(name.clone(), value.clone());
        }
    }
    apply_provider_request_auth_header(&mut headers, provider, upstream_protocol, credential)
        .map_err(|error| {
            ProxyError::gateway(
                ProxyErrorCode::ProviderConfigurationError,
                ExecutionStage::Materialize,
                ResponseVisibility::NotVisible,
                None,
                error.to_string(),
            )
        })?;
    Ok(headers)
}

pub(in crate::proxy) fn apply_gateway_request_identity(
    headers: &mut HeaderMap,
    request_context: &ProxyRequestContext,
) {
    headers.remove(&X_REQUEST_ID);
    headers.remove(&X_CLIENT_REQUEST_ID);
    headers.insert(
        &X_REQUEST_ID,
        HeaderValue::from_str(request_context.request_id.as_str())
            .expect("generated request id must be a valid header value"),
    );
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
    provider_credential: &ProviderCredential,
    path: &str,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let target_url = format!("{}/{}", provider.endpoint, path);
    let mut url = Url::parse(&target_url).map_err(|error| {
        ProxyError::gateway(
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Materialize,
            ResponseVisibility::NotVisible,
            None,
            format!("failed to parse target url: {error}"),
        )
    })?;
    let upstream_protocol = determine_upstream_protocol(provider);
    let mut headers = build_new_headers(
        original_headers,
        provider,
        upstream_protocol,
        provider_credential,
    )?;

    ensure_request_body_object(&mut data);
    if let Value::Object(obj) = &mut data {
        obj.insert("model".to_string(), json!(resolve_real_model_name(model)));
    }

    data = finalize_request_data(
        data,
        UpstreamProtocol::Openai,
        &provider.provider_type,
        path,
    );
    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;

    Ok((url.to_string(), headers, data, provider_credential.key_id()))
}

async fn prepare_generation_request(
    provider: &CacheProvider,
    model: &CacheModel,
    data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credential: &ProviderCredential,
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
    params: &HashMap<String, String>,
) -> Result<PreparedGenerationRequest, ProxyError> {
    match select_generation_prepare_kind(upstream_protocol, is_stream)? {
        GenerationPrepareKind::Llm { path } => {
            let (final_url, final_headers, final_body_value, provider_api_key_id) =
                prepare_llm_request(
                    provider,
                    model,
                    data,
                    original_headers,
                    request_patches,
                    provider_credential,
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
                    provider_credential,
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
    provider_credential: &ProviderCredential,
    action: &str,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing simple Gemini request for provider: {}, model: {}, action: {}",
        provider.name, model.model_name, action
    );

    let real_model_name = resolve_real_model_name(model);
    let mut url = build_gemini_url(provider, real_model_name, action, params, false)?;
    let mut headers = build_gemini_headers(original_headers, provider, provider_credential)?;
    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;

    Ok((url.to_string(), headers, data, provider_credential.key_id()))
}

async fn prepare_gemini_llm_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credential: &ProviderCredential,
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
    let mut headers = build_gemini_headers(original_headers, provider, provider_credential)?;

    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;

    Ok((url.to_string(), headers, data, provider_credential.key_id()))
}

fn is_openai_compatible_generation_target(upstream_protocol: UpstreamProtocol) -> bool {
    upstream_protocol == UpstreamProtocol::Openai
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
    upstream_protocol: UpstreamProtocol,
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
            upstream_protocol,
        ),
        explicit_reasoning_disabled: target_has_explicit_reasoning_disabled(target),
        now_ms: chrono::Utc::now().timestamp_millis(),
    })
    .await
    .map_err(|err| {
        protocol_transform_error(
            ExecutionStage::Transform,
            ResponseVisibility::NotVisible,
            "reasoning content repair failed",
            err,
        )
    })?;
    Ok(())
}

pub(in crate::proxy) async fn materialize_generation_request(
    target: &ExecutionTarget,
    mut data: Value,
    downstream_protocol: DownstreamProtocol,
    is_stream: bool,
    original_headers: &HeaderMap,
    query_params: &HashMap<String, String>,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credential: &ProviderCredential,
    downstream_api_key_id: i64,
    reasoning_continuation_store: &dyn ReasoningContinuationStore,
) -> Result<MaterializedRequest, ProxyError> {
    let upstream_protocol = target.upstream_protocol;
    data = transform_request_data(data, downstream_protocol, upstream_protocol, is_stream);
    let prepared_request = prepare_generation_request(
        &target.provider,
        &target.model,
        data,
        original_headers,
        request_patches,
        provider_credential,
        upstream_protocol,
        is_stream,
        query_params,
    )
    .await?;
    debug_assert_eq!(
        prepared_request.provider_api_key_id,
        provider_credential.key_id()
    );
    let final_url = prepared_request.final_url;
    let mut final_body_value = prepared_request.final_body_value;
    repair_generation_request_body(
        target,
        &mut final_body_value,
        downstream_api_key_id,
        upstream_protocol,
        reasoning_continuation_store,
    )
    .await?;
    let final_body = Bytes::from(serde_json::to_vec(&final_body_value).map_err(|err| {
        protocol_transform_error(
            ExecutionStage::Materialize,
            ResponseVisibility::NotVisible,
            "Failed to serialize final request body",
            err,
        )
    })?);
    Ok(MaterializedRequest {
        final_url,
        final_headers: prepared_request.final_headers,
        final_body,
        model_str: format_model_str(&target.provider, &target.model),
        response_mode: ProxyResponseMode::Generation {
            downstream_protocol,
            upstream_protocol,
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
    provider_credential: &ProviderCredential,
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
                provider_credential,
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
                provider_credential,
                &operation.downstream_path,
                query_params,
            )
            .await?
        }
    };
    debug_assert_eq!(provider_api_key_id, provider_credential.key_id());
    let final_body = Bytes::from(serde_json::to_vec(&final_body_value).map_err(|err| {
        protocol_transform_error(
            ExecutionStage::Materialize,
            ResponseVisibility::NotVisible,
            "Failed to serialize final request body",
            err,
        )
    })?);

    Ok(MaterializedRequest {
        final_url,
        final_headers,
        final_body,
        model_str: format_model_str(&target.provider, &target.model),
        response_mode: ProxyResponseMode::Utility {
            downstream_protocol: operation.downstream_protocol,
            upstream_protocol: target.upstream_protocol,
        },
    })
}
