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
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, classify_transform_failure,
        protocol_transform_error,
        request_context::{ProxyRequestContext, X_CLIENT_REQUEST_ID, X_REQUEST_ID},
        runtime::{
            request_patch::apply_request_patches, route_resolver::ExecutionTarget,
            transport::ProxyResponseMode,
        },
        util::format_model_str,
        utility::{
            UtilityOperation, UtilityProtocol, validate_embeddings_request,
            validate_gemini_count_tokens_request, validate_rerank_request,
        },
    },
    schema::enum_def::{DownstreamProtocol, UpstreamProtocol},
    service::{
        cache::types::{
            CacheModel, CacheProvider, CacheUpstreamSource, RuntimeResolvedRequestPatch,
        },
        provider_credential::{ProviderCredential, apply_provider_request_auth_header},
        provider_http::{
            ANTHROPIC_BETA_HEADER, ANTHROPIC_MESSAGES_OPERATION, ANTHROPIC_VERSION_HEADER,
            GeminiModelOperation, enforce_anthropic_version_header, gemini_operation_target_url,
            join_base_url_and_operation_path, sanitize_gemini_request_headers,
            validate_gemini_pre_auth_target,
        },
        transform::{
            TransformFailure, TransformFailureOrigin, TransformPhase, TransformReasonCode,
            TransformSemanticUnit, TransformSuccess, finalize_request_data, transform_request_data,
            validate_final_generation_request_for_downstream,
        },
        upstream_response::apply_upstream_accept_encoding,
    },
};

#[cfg(test)]
use crate::service::provider_http::ANTHROPIC_VERSION;
use cyder_tools::log::debug;

pub(in crate::proxy) struct MaterializedRequest {
    pub final_url: String,
    pub final_headers: HeaderMap,
    pub final_body: Bytes,
    pub model_str: String,
    pub response_mode: ProxyResponseMode,
}

pub(in crate::proxy) struct PreflightGenerationFailure {
    pub transform_failure: TransformFailure,
    pub proxy_error: ProxyError,
}

struct PreparedGenerationRequest {
    final_url: String,
    final_headers: HeaderMap,
    final_body_value: Value,
}

#[derive(Debug, PartialEq, Eq)]
enum GenerationPrepareKind {
    Llm { path: &'static str },
    Gemini { operation: GeminiModelOperation },
}

fn select_generation_prepare_kind(
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
) -> Result<GenerationPrepareKind, ProxyError> {
    match upstream_protocol {
        UpstreamProtocol::Openai => Ok(GenerationPrepareKind::Llm {
            path: "chat/completions",
        }),
        UpstreamProtocol::Responses => Ok(GenerationPrepareKind::Llm { path: "responses" }),
        UpstreamProtocol::Anthropic => Ok(GenerationPrepareKind::Llm {
            path: ANTHROPIC_MESSAGES_OPERATION,
        }),
        UpstreamProtocol::Gemini => Ok(GenerationPrepareKind::Gemini {
            operation: if is_stream {
                GeminiModelOperation::StreamGenerateContent
            } else {
                GeminiModelOperation::GenerateContent
            },
        }),
    }
}

fn build_gemini_operation_url(
    source: &CacheUpstreamSource,
    real_model_name: &str,
    operation: GeminiModelOperation,
) -> Result<Url, ProxyError> {
    gemini_operation_target_url(&source.base_url, real_model_name, operation).map_err(|error| {
        ProxyError::gateway(
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Materialize,
            ResponseVisibility::NotVisible,
            None,
            format!("failed to resolve Gemini target URL: {error}"),
        )
    })
}

fn build_new_headers(
    pre_headers: &HeaderMap,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
) -> Result<HeaderMap, ProxyError> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in pre_headers.iter() {
        if name != HOST
            && name != CONTENT_LENGTH
            && name != ACCEPT_ENCODING
            && name != AUTHORIZATION
            && name != "x-api-key"
            && name != "x-goog-api-key"
            && name != ANTHROPIC_VERSION_HEADER
            && (name != ANTHROPIC_BETA_HEADER
                || (downstream_protocol == DownstreamProtocol::Anthropic
                    && upstream_protocol == UpstreamProtocol::Anthropic))
            && name != X_REQUEST_ID
            && name != X_CLIENT_REQUEST_ID
        {
            headers.insert(name.clone(), value.clone());
        }
    }
    if upstream_protocol == UpstreamProtocol::Anthropic {
        enforce_anthropic_version_header(&mut headers);
    }
    Ok(headers)
}

pub(in crate::proxy) fn apply_provider_authentication(
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
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    path: &str,
    resolved_target_url: Option<&str>,
) -> Result<(String, HeaderMap, Value), ProxyError> {
    debug!(
        "Preparing LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let target_url = match resolved_target_url {
        Some(target_url) => target_url.to_string(),
        None => join_base_url_and_operation_path(&source.base_url, path).map_err(|error| {
            ProxyError::gateway(
                ProxyErrorCode::ProviderConfigurationError,
                ExecutionStage::Materialize,
                ResponseVisibility::NotVisible,
                None,
                format!("failed to resolve target URL: {error}"),
            )
        })?,
    };
    let mut url = Url::parse(&target_url).map_err(|error| {
        ProxyError::gateway(
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Materialize,
            ResponseVisibility::NotVisible,
            None,
            format!("failed to parse target url: {error}"),
        )
    })?;
    let mut headers = build_new_headers(original_headers, downstream_protocol, upstream_protocol)?;

    ensure_request_body_object(&mut data);
    if let Value::Object(obj) = &mut data {
        obj.insert("model".to_string(), json!(resolve_real_model_name(model)));
    }

    data = finalize_request_data(data, upstream_protocol, &source.profile_type, path);
    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;
    if upstream_protocol == UpstreamProtocol::Anthropic {
        enforce_anthropic_version_header(&mut headers);
    }

    Ok((url.to_string(), headers, data))
}

async fn prepare_generation_request(
    provider: &CacheProvider,
    source: &CacheUpstreamSource,
    model: &CacheModel,
    data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
    operation_url: Option<&str>,
) -> Result<PreparedGenerationRequest, ProxyError> {
    match select_generation_prepare_kind(upstream_protocol, is_stream)? {
        GenerationPrepareKind::Llm { path } => {
            let (final_url, final_headers, final_body_value) = prepare_llm_request(
                provider,
                source,
                model,
                data,
                original_headers,
                request_patches,
                downstream_protocol,
                upstream_protocol,
                path,
                operation_url,
            )
            .await?;
            Ok(PreparedGenerationRequest {
                final_url,
                final_headers,
                final_body_value,
            })
        }
        GenerationPrepareKind::Gemini { operation } => {
            let (final_url, final_headers, final_body_value) = prepare_gemini_generation_request(
                provider,
                source,
                model,
                data,
                original_headers,
                request_patches,
                operation,
            )
            .await?;
            Ok(PreparedGenerationRequest {
                final_url,
                final_headers,
                final_body_value,
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
    operation: GeminiModelOperation,
) -> Result<(String, HeaderMap, Value), ProxyError> {
    debug!(
        "Preparing simple Gemini request for provider: {}, model: {}, action: {}",
        provider.name,
        model.model_name,
        operation.action()
    );

    let real_model_name = resolve_real_model_name(model);
    let mut url = build_gemini_operation_url(source, real_model_name, operation)?;
    let mut headers = sanitize_gemini_request_headers(original_headers);
    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;
    validate_gemini_pre_auth_target(&url, &headers, operation).map_err(|error| {
        ProxyError::gateway(
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Patch,
            ResponseVisibility::NotVisible,
            None,
            format!("final Gemini target validation failed: {error}"),
        )
    })?;

    Ok((url.to_string(), headers, data))
}

async fn prepare_gemini_generation_request(
    provider: &CacheProvider,
    source: &CacheUpstreamSource,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    operation: GeminiModelOperation,
) -> Result<(String, HeaderMap, Value), ProxyError> {
    debug!(
        "Preparing Gemini LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let real_model_name = resolve_real_model_name(model);
    debug_assert!(matches!(
        operation,
        GeminiModelOperation::GenerateContent | GeminiModelOperation::StreamGenerateContent
    ));
    let mut url = build_gemini_operation_url(source, real_model_name, operation)?;
    let mut headers = sanitize_gemini_request_headers(original_headers);

    apply_request_patches(&mut data, &mut url, &mut headers, request_patches)?;
    validate_gemini_pre_auth_target(&url, &headers, operation).map_err(|error| {
        ProxyError::gateway(
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Patch,
            ResponseVisibility::NotVisible,
            None,
            format!("final Gemini target validation failed: {error}"),
        )
    })?;

    Ok((url.to_string(), headers, data))
}

pub(in crate::proxy) async fn materialize_generation_request(
    target: &ExecutionTarget,
    data: Value,
    downstream_protocol: DownstreamProtocol,
    is_stream: bool,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
    operation_url: Option<&str>,
) -> Result<MaterializedRequest, ProxyError> {
    let upstream_protocol = target.upstream_protocol;
    let mut prepared_request = prepare_generation_request(
        &target.provider,
        &target.upstream_source,
        &target.model,
        data,
        original_headers,
        request_patches,
        downstream_protocol,
        upstream_protocol,
        is_stream,
        operation_url,
    )
    .await?;
    if let Err(error) = validate_final_generation_request_for_downstream(
        &prepared_request.final_body_value,
        downstream_protocol,
        upstream_protocol,
        &target.upstream_source.profile_type,
    ) {
        return Err(ProxyError::gateway(
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Patch,
            ResponseVisibility::NotVisible,
            None,
            format!("final target Profile validation failed at {error}"),
        ));
    }
    apply_upstream_accept_encoding(&mut prepared_request.final_headers, is_stream);
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

pub(in crate::proxy) fn preflight_generation_request(
    target: &ExecutionTarget,
    data: Value,
    downstream_protocol: DownstreamProtocol,
    is_stream: bool,
) -> Result<TransformSuccess<Value>, PreflightGenerationFailure> {
    let mut transformed = transform_request_data(
        data,
        downstream_protocol,
        target.upstream_protocol,
        is_stream,
    )
    .map_err(|transform_failure| PreflightGenerationFailure {
        proxy_error: classify_transform_failure(&transform_failure, ResponseVisibility::NotVisible),
        transform_failure,
    })?;

    if matches!(
        target.upstream_protocol,
        UpstreamProtocol::Openai | UpstreamProtocol::Responses | UpstreamProtocol::Anthropic
    ) {
        if let Value::Object(object) = &mut transformed.value {
            object.insert(
                "model".to_string(),
                json!(resolve_real_model_name(&target.model)),
            );
            if target.upstream_protocol == UpstreamProtocol::Responses {
                object.insert("stream".to_string(), json!(is_stream));
            }
        }
    }

    if let Err(error) = validate_final_generation_request_for_downstream(
        &transformed.value,
        downstream_protocol,
        target.upstream_protocol,
        &target.upstream_source.profile_type,
    ) {
        let transform_failure = TransformFailure {
            origin: TransformFailureOrigin::DownstreamInput,
            phase: TransformPhase::RequestEncode,
            semantic_unit: TransformSemanticUnit::RequestEnvelope,
            reason_code: TransformReasonCode::InvalidProtocolShape,
            summary: transformed.summary.clone(),
        };
        return Err(PreflightGenerationFailure {
            proxy_error: ProxyError::gateway(
                ProxyErrorCode::InvalidRequestError,
                ExecutionStage::Parse,
                ResponseVisibility::NotVisible,
                None,
                format!("target Profile validation failed at {error}"),
            ),
            transform_failure,
        });
    }

    Ok(transformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::enum_def::UpstreamProfileType;

    fn gemini_source(base_url: &str, profile_type: UpstreamProfileType) -> CacheUpstreamSource {
        CacheUpstreamSource {
            id: 1,
            profile_type,
            base_url: base_url.to_string(),
            use_proxy: false,
            chat_completions_enabled: Some(false),
            chat_completions_path_override: None,
            embeddings_enabled: Some(false),
            embeddings_path_override: None,
            rerank_enabled: Some(false),
            rerank_path_override: None,
            is_enabled: true,
            is_default: true,
        }
    }

    #[test]
    fn gemini_generation_prepare_kind_owns_exact_typed_operation() {
        assert_eq!(
            select_generation_prepare_kind(UpstreamProtocol::Gemini, false)
                .expect("Gemini non-stream prepare kind"),
            GenerationPrepareKind::Gemini {
                operation: GeminiModelOperation::GenerateContent,
            }
        );
        assert_eq!(
            select_generation_prepare_kind(UpstreamProtocol::Gemini, true)
                .expect("Gemini stream prepare kind"),
            GenerationPrepareKind::Gemini {
                operation: GeminiModelOperation::StreamGenerateContent,
            }
        );
    }

    #[test]
    fn responses_generation_prepare_kind_uses_one_native_path_for_both_modes() {
        for is_stream in [false, true] {
            let kind = select_generation_prepare_kind(UpstreamProtocol::Responses, is_stream)
                .expect("Responses generation should be materializable");
            assert!(
                matches!(kind, GenerationPrepareKind::Llm { path: "responses" }),
                "is_stream={is_stream}"
            );
        }
    }

    #[test]
    fn anthropic_generation_prepare_kind_uses_messages_for_both_modes() {
        for is_stream in [false, true] {
            let kind = select_generation_prepare_kind(UpstreamProtocol::Anthropic, is_stream)
                .expect("Anthropic generation should be materializable");
            assert!(
                matches!(
                    kind,
                    GenerationPrepareKind::Llm {
                        path: ANTHROPIC_MESSAGES_OPERATION
                    }
                ),
                "is_stream={is_stream}"
            );
        }
    }

    #[test]
    fn anthropic_header_policy_fixes_version_and_scopes_client_beta_to_same_wire() {
        let mut original = HeaderMap::new();
        original.insert(
            ANTHROPIC_VERSION_HEADER,
            HeaderValue::from_static("2099-01-01"),
        );
        original.insert(
            ANTHROPIC_BETA_HEADER,
            HeaderValue::from_static("future-beta"),
        );
        original.insert(AUTHORIZATION, HeaderValue::from_static("Bearer downstream"));
        original.insert("x-api-key", HeaderValue::from_static("downstream-secret"));
        original.insert("x-client-feature", HeaderValue::from_static("preserved"));

        let same_wire = build_new_headers(
            &original,
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Anthropic,
        )
        .expect("same-wire headers");
        assert_eq!(
            same_wire
                .get(ANTHROPIC_VERSION_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some(ANTHROPIC_VERSION)
        );
        assert_eq!(
            same_wire
                .get(ANTHROPIC_BETA_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("future-beta")
        );
        assert_eq!(
            same_wire
                .get("x-client-feature")
                .and_then(|value| value.to_str().ok()),
            Some("preserved")
        );
        assert!(!same_wire.contains_key(AUTHORIZATION));
        assert!(!same_wire.contains_key("x-api-key"));
        assert_eq!(
            same_wire.get_all(ANTHROPIC_VERSION_HEADER).iter().count(),
            1
        );

        for downstream in [
            DownstreamProtocol::Openai,
            DownstreamProtocol::Responses,
            DownstreamProtocol::Gemini,
        ] {
            let cross_wire = build_new_headers(&original, downstream, UpstreamProtocol::Anthropic)
                .expect("cross-wire headers");
            assert_eq!(
                cross_wire
                    .get(ANTHROPIC_VERSION_HEADER)
                    .and_then(|value| value.to_str().ok()),
                Some(ANTHROPIC_VERSION),
                "{downstream:?}"
            );
            assert!(
                !cross_wire.contains_key(ANTHROPIC_BETA_HEADER),
                "{downstream:?}"
            );
        }

        let non_anthropic = build_new_headers(
            &original,
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Openai,
        )
        .expect("non-Anthropic target headers");
        assert!(!non_anthropic.contains_key(ANTHROPIC_VERSION_HEADER));
        assert!(!non_anthropic.contains_key(ANTHROPIC_BETA_HEADER));
    }

    #[test]
    fn anthropic_version_finalization_replaces_duplicate_or_mutated_values() {
        let mut headers = HeaderMap::new();
        headers.append(
            ANTHROPIC_VERSION_HEADER,
            HeaderValue::from_static("2020-01-01"),
        );
        headers.append(
            ANTHROPIC_VERSION_HEADER,
            HeaderValue::from_static("2099-01-01"),
        );

        enforce_anthropic_version_header(&mut headers);

        assert_eq!(
            headers
                .get(ANTHROPIC_VERSION_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some(ANTHROPIC_VERSION)
        );
        assert_eq!(headers.get_all(ANTHROPIC_VERSION_HEADER).iter().count(), 1);
    }

    #[test]
    fn gemini_runtime_url_uses_shared_typed_operation_builder_for_both_profiles() {
        let cases = [
            (
                gemini_source(
                    "https://generativelanguage.googleapis.com/v1beta/models/",
                    UpstreamProfileType::Gemini,
                ),
                GeminiModelOperation::GenerateContent,
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-fixture:generateContent",
            ),
            (
                gemini_source(
                    "https://region-aiplatform.googleapis.com/v1/projects/p/locations/r/publishers/google/models",
                    UpstreamProfileType::Vertex,
                ),
                GeminiModelOperation::StreamGenerateContent,
                "https://region-aiplatform.googleapis.com/v1/projects/p/locations/r/publishers/google/models/gemini-fixture:streamGenerateContent?alt=sse",
            ),
            (
                gemini_source(
                    "http://127.0.0.1:9000/proxy/models",
                    UpstreamProfileType::Gemini,
                ),
                GeminiModelOperation::CountTokens,
                "http://127.0.0.1:9000/proxy/models/gemini-fixture:countTokens",
            ),
        ];

        for (source, operation, expected) in cases {
            let url = build_gemini_operation_url(&source, "gemini-fixture", operation)
                .expect("typed Gemini URL");
            assert_eq!(url.as_str(), expected);
        }

        let invalid = gemini_source(
            "https://api.example/v1beta/models?key=sentinel-secret",
            UpstreamProfileType::Gemini,
        );
        let error = build_gemini_operation_url(
            &invalid,
            "models/sentinel-model",
            GeminiModelOperation::GenerateContent,
        )
        .expect_err("unsafe Gemini URL");
        assert!(!error.operator_message().contains("sentinel"));
        assert!(!error.operator_message().contains("https://"));
    }
}

pub(in crate::proxy) async fn materialize_utility_request(
    target: &ExecutionTarget,
    operation: &UtilityOperation,
    data: Value,
    original_headers: &HeaderMap,
    operation_url: Option<&str>,
) -> Result<MaterializedRequest, ProxyError> {
    match operation.upstream_operation() {
        Some(crate::service::upstream_profile::UpstreamOperation::Embeddings) => {
            validate_embeddings_request(&data, target.upstream_source.profile_type)?;
        }
        Some(crate::service::upstream_profile::UpstreamOperation::Rerank) => {
            validate_rerank_request(&data, target.upstream_source.profile_type)?;
        }
        Some(crate::service::upstream_profile::UpstreamOperation::ChatCompletions) | None => {}
    }
    if operation.protocol == UtilityProtocol::GeminiCompatible {
        validate_gemini_count_tokens_request(&data)?;
    }
    let (final_url, mut final_headers, final_body_value) = match operation.protocol {
        UtilityProtocol::OpenaiCompatible => {
            prepare_llm_request(
                &target.provider,
                &target.upstream_source,
                &target.model,
                data,
                original_headers,
                &[],
                operation.downstream_protocol,
                target.upstream_protocol,
                &operation.downstream_path,
                operation_url,
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
                &[],
                GeminiModelOperation::CountTokens,
            )
            .await?
        }
    };
    apply_upstream_accept_encoding(&mut final_headers, false);
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
            kind: operation.response_kind(),
        },
    })
}
