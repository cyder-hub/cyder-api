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
            request_patch::apply_request_patches, route_resolver::ExecutionTarget,
            transport::ProxyResponseMode,
        },
        util::format_model_str,
        utility::{UtilityOperation, UtilityProtocol},
    },
    schema::enum_def::{DownstreamProtocol, UpstreamProtocol},
    service::{
        cache::types::{
            CacheModel, CacheProvider, CacheUpstreamSource, RuntimeResolvedRequestPatch,
        },
        provider_credential::{ProviderCredential, apply_provider_request_auth_header},
        transform::{finalize_request_data, transform_request_data},
        upstream_response::apply_upstream_accept_encoding,
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

fn build_gemini_headers(original_headers: &HeaderMap) -> Result<HeaderMap, ProxyError> {
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

    Ok(headers)
}

fn build_gemini_url(
    source: &CacheUpstreamSource,
    real_model_name: &str,
    action: &str,
    params: &HashMap<String, String>,
    is_stream: bool,
) -> Result<Url, ProxyError> {
    let target_url_str = format!("{}/{}:{}", source.endpoint, real_model_name, action);
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

fn build_new_headers(pre_headers: &HeaderMap) -> Result<HeaderMap, ProxyError> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in pre_headers.iter() {
        if name != HOST
            && name != CONTENT_LENGTH
            && name != ACCEPT_ENCODING
            && name != AUTHORIZATION
            && name != "x-api-key"
            && name != "x-goog-api-key"
            && name != X_REQUEST_ID
            && name != X_CLIENT_REQUEST_ID
        {
            headers.insert(name.clone(), value.clone());
        }
    }
    Ok(headers)
}

fn apply_provider_authentication(
    headers: &mut HeaderMap,
    source: &CacheUpstreamSource,
    upstream_protocol: UpstreamProtocol,
    credential: &ProviderCredential,
) -> Result<(), ProxyError> {
    apply_provider_request_auth_header(headers, source, upstream_protocol, credential).map_err(
        |error| {
            ProxyError::gateway(
                ProxyErrorCode::ProviderConfigurationError,
                ExecutionStage::Materialize,
                ResponseVisibility::NotVisible,
                None,
                error.to_string(),
            )
        },
    )
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
    source: &CacheUpstreamSource,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    provider_credential: &ProviderCredential,
    upstream_protocol: UpstreamProtocol,
    path: &str,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let target_url = format!("{}/{}", source.endpoint, path);
    let mut url = Url::parse(&target_url).map_err(|error| {
        ProxyError::gateway(
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Materialize,
            ResponseVisibility::NotVisible,
            None,
            format!("failed to parse target url: {error}"),
        )
    })?;
    let mut headers = build_new_headers(original_headers)?;

    ensure_request_body_object(&mut data);
    if let Value::Object(obj) = &mut data {
        obj.insert("model".to_string(), json!(resolve_real_model_name(model)));
    }

    data = finalize_request_data(data, upstream_protocol, &source.profile_type, path);
    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;
    apply_provider_authentication(&mut headers, source, upstream_protocol, provider_credential)?;

    Ok((url.to_string(), headers, data, provider_credential.key_id()))
}

async fn prepare_generation_request(
    provider: &CacheProvider,
    source: &CacheUpstreamSource,
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
                    source,
                    model,
                    data,
                    original_headers,
                    request_patches,
                    provider_credential,
                    upstream_protocol,
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
                    source,
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
    source: &CacheUpstreamSource,
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
    let mut url = build_gemini_url(source, real_model_name, action, params, false)?;
    let mut headers = build_gemini_headers(original_headers)?;
    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;
    apply_provider_authentication(
        &mut headers,
        source,
        UpstreamProtocol::Gemini,
        provider_credential,
    )?;

    Ok((url.to_string(), headers, data, provider_credential.key_id()))
}

async fn prepare_gemini_llm_request(
    provider: &CacheProvider,
    source: &CacheUpstreamSource,
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
    let mut url = build_gemini_url(source, real_model_name, action, params, is_stream)?;
    let mut headers = build_gemini_headers(original_headers)?;

    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;
    apply_provider_authentication(
        &mut headers,
        source,
        UpstreamProtocol::Gemini,
        provider_credential,
    )?;

    Ok((url.to_string(), headers, data, provider_credential.key_id()))
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
) -> Result<MaterializedRequest, ProxyError> {
    let upstream_protocol = target.upstream_protocol;
    data = transform_request_data(data, downstream_protocol, upstream_protocol, is_stream);
    let mut prepared_request = prepare_generation_request(
        &target.provider,
        &target.upstream_source,
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
    apply_upstream_accept_encoding(&mut prepared_request.final_headers, is_stream);
    debug_assert_eq!(
        prepared_request.provider_api_key_id,
        provider_credential.key_id()
    );
    let final_url = prepared_request.final_url;
    let final_body_value = prepared_request.final_body_value;
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
    let (final_url, mut final_headers, final_body_value, provider_api_key_id) =
        match operation.protocol {
            UtilityProtocol::OpenaiCompatible => {
                prepare_llm_request(
                    &target.provider,
                    &target.upstream_source,
                    &target.model,
                    data,
                    original_headers,
                    request_patches,
                    provider_credential,
                    target.upstream_protocol,
                    &operation.downstream_path,
                )
                .await?
            }
            UtilityProtocol::GeminiCompatible => {
                prepare_simple_gemini_request(
                    &target.provider,
                    &target.upstream_source,
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
    apply_upstream_accept_encoding(&mut final_headers, false);
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
