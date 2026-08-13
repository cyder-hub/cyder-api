use std::sync::Arc;

use axum::{body::Body, http::HeaderMap, response::Response};
use serde_json::Value;

use super::{
    ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility,
    cancellation::ProxyCancellationContext,
    request::ParsedProxyRequest,
    request_context::ProxyRequestContext,
    runtime::{
        facade::{UtilityOrchestrationInput, execute_utility},
        route_resolver::ExecutionPlan,
    },
};
use crate::{
    schema::enum_def::{DownstreamProtocol, ModelKind, UpstreamProfileType, UpstreamProtocol},
    service::{
        app_state::AppState, cache::types::CacheApiKey, upstream_profile::UpstreamOperation,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UtilityProtocol {
    OpenaiCompatible,
    GeminiCompatible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::proxy) enum UtilityResponseKind {
    Embeddings,
    Rerank,
    GeminiCountTokens,
    Passthrough,
}

#[derive(Clone, Debug)]
pub(crate) struct UtilityOperation {
    pub name: String,
    pub downstream_protocol: DownstreamProtocol,
    pub protocol: UtilityProtocol,
    pub downstream_path: String,
}

impl UtilityOperation {
    pub(crate) fn required_model_kind(&self) -> ModelKind {
        match self.upstream_operation() {
            Some(UpstreamOperation::Embeddings) => ModelKind::Embedding,
            Some(UpstreamOperation::Rerank) => ModelKind::Rerank,
            Some(UpstreamOperation::ChatCompletions) | None => ModelKind::Chat,
        }
    }

    pub(crate) fn upstream_operation(&self) -> Option<UpstreamOperation> {
        if self.protocol != UtilityProtocol::OpenaiCompatible {
            return None;
        }
        match self.downstream_path.as_str() {
            "embeddings" => Some(UpstreamOperation::Embeddings),
            "rerank" => Some(UpstreamOperation::Rerank),
            _ => None,
        }
    }

    pub(in crate::proxy) fn response_kind(&self) -> UtilityResponseKind {
        match (self.protocol, self.downstream_path.as_str()) {
            (UtilityProtocol::OpenaiCompatible, "embeddings") => UtilityResponseKind::Embeddings,
            (UtilityProtocol::OpenaiCompatible, "rerank") => UtilityResponseKind::Rerank,
            (UtilityProtocol::GeminiCompatible, "countTokens") => {
                UtilityResponseKind::GeminiCountTokens
            }
            _ => UtilityResponseKind::Passthrough,
        }
    }
}

pub(super) struct UtilityExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub operation: UtilityOperation,
    pub execution_plan: ExecutionPlan,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub request_context: Arc<ProxyRequestContext>,
    pub parsed_request: ParsedProxyRequest,
}

pub(super) fn validate_utility_target(
    operation: &UtilityOperation,
    upstream_protocol: UpstreamProtocol,
    profile_type: UpstreamProfileType,
) -> Result<(), ProxyError> {
    match (operation.protocol, upstream_protocol, profile_type) {
        (UtilityProtocol::OpenaiCompatible, UpstreamProtocol::Openai, _) => Ok(()),
        (
            UtilityProtocol::GeminiCompatible,
            UpstreamProtocol::Gemini,
            UpstreamProfileType::Gemini | UpstreamProfileType::Vertex,
        ) => Ok(()),
        (UtilityProtocol::OpenaiCompatible, _, _) => {
            let message = format!(
                "'{}' is only supported for OpenAI-compatible providers.",
                operation.name
            );
            Err(ProxyError::gateway(
                ProxyErrorCode::UnsupportedCapabilityError,
                ExecutionStage::Capability,
                ResponseVisibility::NotVisible,
                Some(message.clone()),
                message,
            ))
        }
        (UtilityProtocol::GeminiCompatible, _, _) => {
            let message = format!(
                "Action '{}' is only supported for Gemini-compatible providers.",
                operation.name
            );
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

const MAX_COUNT_TOKENS_MODALITY_DETAILS: usize = 32;

fn invalid_count_tokens_request(path: &'static str, reason: &'static str) -> ProxyError {
    let message = format!("Invalid countTokens request at {path}: {reason}.");
    ProxyError::gateway(
        ProxyErrorCode::InvalidRequestError,
        ExecutionStage::Parse,
        ResponseVisibility::NotVisible,
        Some(message.clone()),
        message,
    )
}

fn validate_gemini_contents(value: &Value, path: &'static str) -> Result<(), ProxyError> {
    let contents = value
        .as_array()
        .ok_or_else(|| invalid_count_tokens_request(path, "contents must be a non-empty array"))?;
    if contents.is_empty() {
        return Err(invalid_count_tokens_request(
            path,
            "contents must be a non-empty array",
        ));
    }
    for content in contents {
        let parts = content
            .as_object()
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
            .filter(|parts| !parts.is_empty())
            .ok_or_else(|| {
                invalid_count_tokens_request("/contents/*/parts", "parts must be a non-empty array")
            })?;
        if parts
            .iter()
            .any(|part| !part.as_object().is_some_and(|part| !part.is_empty()))
        {
            return Err(invalid_count_tokens_request(
                "/contents/*/parts/*",
                "each part must be a non-empty object",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_gemini_count_tokens_request(data: &Value) -> Result<(), ProxyError> {
    let object = data.as_object().ok_or_else(|| {
        invalid_count_tokens_request("/", "the request body must be a JSON object")
    })?;
    if object.contains_key("model") {
        return Err(invalid_count_tokens_request(
            "/model",
            "the model is owned by the request path",
        ));
    }
    match (object.get("contents"), object.get("generateContentRequest")) {
        (Some(contents), None) => validate_gemini_contents(contents, "/contents"),
        (None, Some(request)) => {
            let request = request
                .as_object()
                .filter(|request| !request.is_empty())
                .ok_or_else(|| {
                    invalid_count_tokens_request(
                        "/generateContentRequest",
                        "generateContentRequest must be a non-empty object",
                    )
                })?;
            if request.contains_key("model") {
                return Err(invalid_count_tokens_request(
                    "/generateContentRequest/model",
                    "the model is owned by the request path",
                ));
            }
            let contents = request.get("contents").ok_or_else(|| {
                invalid_count_tokens_request(
                    "/generateContentRequest/contents",
                    "contents is required",
                )
            })?;
            validate_gemini_contents(contents, "/generateContentRequest/contents")
        }
        (Some(_), Some(_)) => Err(invalid_count_tokens_request(
            "/",
            "provide exactly one of contents or generateContentRequest",
        )),
        (None, None) => Err(invalid_count_tokens_request(
            "/",
            "provide exactly one of contents or generateContentRequest",
        )),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::proxy) enum GeminiTokenModality {
    Unspecified,
    Text,
    Image,
    Video,
    Audio,
    Document,
    Unknown,
}

impl GeminiTokenModality {
    pub(in crate::proxy) const fn as_key(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Text => "text",
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Document => "document",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::proxy) struct GeminiModalityTokenObservation {
    pub modality: GeminiTokenModality,
    pub token_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::proxy) struct GeminiCountTokensObservation {
    pub total_tokens: u64,
    pub cached_content_token_count: Option<u64>,
    pub prompt_token_details: Vec<GeminiModalityTokenObservation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::proxy) enum GeminiCountTokensObservationError {
    InvalidEnvelope,
    InvalidTotalTokens,
    InvalidCachedContentTokenCount,
    InvalidPromptTokenDetails,
    TooManyPromptTokenDetails,
    InvalidPromptTokenDetail,
}

impl GeminiCountTokensObservationError {
    pub(in crate::proxy) const fn as_key(self) -> &'static str {
        match self {
            Self::InvalidEnvelope => "invalid_envelope",
            Self::InvalidTotalTokens => "invalid_total_tokens",
            Self::InvalidCachedContentTokenCount => "invalid_cached_content_token_count",
            Self::InvalidPromptTokenDetails => "invalid_prompt_token_details",
            Self::TooManyPromptTokenDetails => "too_many_prompt_token_details",
            Self::InvalidPromptTokenDetail => "invalid_prompt_token_detail",
        }
    }
}

fn observe_gemini_modality(value: &str) -> GeminiTokenModality {
    match value {
        "MODALITY_UNSPECIFIED" => GeminiTokenModality::Unspecified,
        "TEXT" => GeminiTokenModality::Text,
        "IMAGE" => GeminiTokenModality::Image,
        "VIDEO" => GeminiTokenModality::Video,
        "AUDIO" => GeminiTokenModality::Audio,
        "DOCUMENT" => GeminiTokenModality::Document,
        _ => GeminiTokenModality::Unknown,
    }
}

pub(in crate::proxy) fn observe_gemini_count_tokens_response(
    data: &Value,
) -> Result<GeminiCountTokensObservation, GeminiCountTokensObservationError> {
    let object = data
        .as_object()
        .ok_or(GeminiCountTokensObservationError::InvalidEnvelope)?;
    let total_tokens = object
        .get("totalTokens")
        .and_then(Value::as_u64)
        .ok_or(GeminiCountTokensObservationError::InvalidTotalTokens)?;
    let cached_content_token_count = object
        .get("cachedContentTokenCount")
        .map(|value| {
            value
                .as_u64()
                .ok_or(GeminiCountTokensObservationError::InvalidCachedContentTokenCount)
        })
        .transpose()?;
    let prompt_token_details = object
        .get("promptTokensDetails")
        .map(|value| {
            let details = value
                .as_array()
                .ok_or(GeminiCountTokensObservationError::InvalidPromptTokenDetails)?;
            if details.len() > MAX_COUNT_TOKENS_MODALITY_DETAILS {
                return Err(GeminiCountTokensObservationError::TooManyPromptTokenDetails);
            }
            details
                .iter()
                .map(|detail| {
                    let detail = detail
                        .as_object()
                        .ok_or(GeminiCountTokensObservationError::InvalidPromptTokenDetail)?;
                    let modality = detail
                        .get("modality")
                        .and_then(Value::as_str)
                        .filter(|modality| !modality.is_empty())
                        .ok_or(GeminiCountTokensObservationError::InvalidPromptTokenDetail)?;
                    let token_count = detail
                        .get("tokenCount")
                        .and_then(Value::as_u64)
                        .ok_or(GeminiCountTokensObservationError::InvalidPromptTokenDetail)?;
                    Ok(GeminiModalityTokenObservation {
                        modality: observe_gemini_modality(modality),
                        token_count,
                    })
                })
                .collect()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(GeminiCountTokensObservation {
        total_tokens,
        cached_content_token_count,
        prompt_token_details,
    })
}

const MAX_EMBEDDING_INPUT_ARRAY_LENGTH: usize = 2_048;

fn invalid_embedding_request(path: &'static str, reason: &'static str) -> ProxyError {
    let message = format!("Invalid embeddings request at {path}: {reason}.");
    ProxyError::gateway(
        ProxyErrorCode::InvalidRequestError,
        ExecutionStage::Parse,
        ResponseVisibility::NotVisible,
        Some(message.clone()),
        message,
    )
}

fn validate_non_empty_embedding_text(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !text.trim().is_empty())
}

fn validate_embedding_token(value: &Value) -> bool {
    value.as_u64().is_some()
}

fn validate_embedding_token_array(values: &[Value]) -> bool {
    !values.is_empty()
        && values.len() <= MAX_EMBEDDING_INPUT_ARRAY_LENGTH
        && values.iter().all(validate_embedding_token)
}

fn validate_openai_embedding_input(input: &Value) -> bool {
    match input {
        Value::String(_) => validate_non_empty_embedding_text(input),
        Value::Array(values)
            if !values.is_empty() && values.len() <= MAX_EMBEDDING_INPUT_ARRAY_LENGTH =>
        {
            if values.iter().all(Value::is_string) {
                values.iter().all(validate_non_empty_embedding_text)
            } else if values.iter().all(Value::is_number) {
                validate_embedding_token_array(values)
            } else if values.iter().all(Value::is_array) {
                values.iter().all(|value| {
                    value
                        .as_array()
                        .is_some_and(|tokens| validate_embedding_token_array(tokens))
                })
            } else {
                false
            }
        }
        _ => false,
    }
}

pub(super) fn validate_embeddings_request(
    data: &Value,
    profile_type: UpstreamProfileType,
) -> Result<(), ProxyError> {
    let object = data
        .as_object()
        .ok_or_else(|| invalid_embedding_request("/", "the request body must be a JSON object"))?;
    if !object
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|model| !model.trim().is_empty())
    {
        return Err(invalid_embedding_request(
            "/model",
            "model must be a non-empty string",
        ));
    }
    let input = object
        .get("input")
        .ok_or_else(|| invalid_embedding_request("/input", "input is required"))?;

    match profile_type {
        UpstreamProfileType::Openai => {
            if !validate_openai_embedding_input(input) {
                return Err(invalid_embedding_request(
                    "/input",
                    "input must be a non-empty string, string array, token array, or token-array array",
                ));
            }
            if let Some(encoding_format) = object.get("encoding_format")
                && !matches!(encoding_format.as_str(), Some("float" | "base64"))
            {
                return Err(invalid_embedding_request(
                    "/encoding_format",
                    "encoding_format must be 'float' or 'base64'",
                ));
            }
            if let Some(dimensions) = object.get("dimensions")
                && !dimensions.as_u64().is_some_and(|dimensions| dimensions > 0)
            {
                return Err(invalid_embedding_request(
                    "/dimensions",
                    "dimensions must be a positive integer",
                ));
            }
            if object.get("user").is_some_and(|user| !user.is_string()) {
                return Err(invalid_embedding_request("/user", "user must be a string"));
            }
        }
        UpstreamProfileType::OpenaiCompatible => {
            if !validate_openai_embedding_input(input) {
                return Err(invalid_embedding_request(
                    "/input",
                    "input must be a non-empty string, string array, token array, or token-array array",
                ));
            }
        }
        UpstreamProfileType::GeminiOpenai => {
            if !validate_non_empty_embedding_text(input) {
                return Err(invalid_embedding_request(
                    "/input",
                    "GEMINI_OPENAI only supports a non-empty string input",
                ));
            }
            if let Some(field) = object
                .keys()
                .find(|field| !matches!(field.as_str(), "model" | "input"))
            {
                let path = if field == "encoding_format" {
                    "/encoding_format"
                } else if field == "dimensions" {
                    "/dimensions"
                } else if field == "user" {
                    "/user"
                } else {
                    "/<unknown>"
                };
                return Err(invalid_embedding_request(
                    path,
                    "the field is not verified for the GEMINI_OPENAI embeddings contract",
                ));
            }
        }
        _ => {
            let message = "The selected Source Profile does not support embeddings.";
            return Err(ProxyError::gateway(
                ProxyErrorCode::UnsupportedCapabilityError,
                ExecutionStage::Capability,
                ResponseVisibility::NotVisible,
                Some(message.to_string()),
                message,
            ));
        }
    }

    Ok(())
}

pub(super) fn validate_rerank_request(
    data: &Value,
    profile_type: UpstreamProfileType,
) -> Result<(), ProxyError> {
    if profile_type != UpstreamProfileType::OpenaiCompatible {
        let message = "The selected Source Profile does not support rerank.";
        return Err(ProxyError::gateway(
            ProxyErrorCode::UnsupportedCapabilityError,
            ExecutionStage::Capability,
            ResponseVisibility::NotVisible,
            Some(message.to_string()),
            message,
        ));
    }
    let object = data.as_object().ok_or_else(|| {
        let message = "Invalid rerank request at /: the request body must be a JSON object.";
        ProxyError::gateway(
            ProxyErrorCode::InvalidRequestError,
            ExecutionStage::Parse,
            ResponseVisibility::NotVisible,
            Some(message.to_string()),
            message,
        )
    })?;
    if !object
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|model| !model.trim().is_empty())
    {
        let message = "Invalid rerank request at /model: model must be a non-empty string.";
        return Err(ProxyError::gateway(
            ProxyErrorCode::InvalidRequestError,
            ExecutionStage::Parse,
            ResponseVisibility::NotVisible,
            Some(message.to_string()),
            message,
        ));
    }
    Ok(())
}

pub(super) async fn execute_utility_proxy(
    app_state: Arc<AppState>,
    input: UtilityExecutionInput,
) -> Result<Response<Body>, ProxyError> {
    let UtilityExecutionInput {
        cancellation,
        api_key,
        operation,
        execution_plan,
        original_headers,
        client_ip_addr,
        request_context,
        parsed_request,
    } = input;
    let ParsedProxyRequest { data } = parsed_request;

    execute_utility(
        app_state,
        UtilityOrchestrationInput {
            cancellation,
            api_key,
            operation,
            execution_plan,
            original_headers,
            client_ip_addr,
            request_context,
            data,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        GeminiCountTokensObservationError, GeminiTokenModality, MAX_EMBEDDING_INPUT_ARRAY_LENGTH,
        UtilityOperation, UtilityProtocol, observe_gemini_count_tokens_response,
        validate_embeddings_request, validate_gemini_count_tokens_request, validate_rerank_request,
        validate_utility_target,
    };
    use crate::{
        proxy::ProxyErrorCode,
        schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol},
    };

    #[test]
    fn validate_utility_target_enforces_openai_compatibility() {
        let operation = UtilityOperation {
            name: "embeddings".to_string(),
            downstream_protocol: DownstreamProtocol::Openai,
            protocol: UtilityProtocol::OpenaiCompatible,
            downstream_path: "embeddings".to_string(),
        };

        assert!(
            validate_utility_target(
                &operation,
                UpstreamProtocol::Openai,
                UpstreamProfileType::Openai,
            )
            .is_ok()
        );
        let error = validate_utility_target(
            &operation,
            UpstreamProtocol::Gemini,
            UpstreamProfileType::Gemini,
        )
        .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::UnsupportedCapabilityError);
    }

    #[test]
    fn validate_utility_target_enforces_gemini_compatibility() {
        let operation = UtilityOperation {
            name: "countTokens".to_string(),
            downstream_protocol: DownstreamProtocol::Gemini,
            protocol: UtilityProtocol::GeminiCompatible,
            downstream_path: "countTokens".to_string(),
        };

        for profile_type in [UpstreamProfileType::Gemini, UpstreamProfileType::Vertex] {
            assert!(
                validate_utility_target(&operation, UpstreamProtocol::Gemini, profile_type,)
                    .is_ok()
            );
        }
        let error = validate_utility_target(
            &operation,
            UpstreamProtocol::Openai,
            UpstreamProfileType::Openai,
        )
        .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::UnsupportedCapabilityError);
    }

    #[test]
    fn gemini_count_tokens_request_requires_one_path_owned_shape() {
        for valid in [
            json!({
                "contents": [{"role": "user", "parts": [{"text": "count me"}]}],
                "futureRootField": true
            }),
            json!({
                "generateContentRequest": {
                    "contents": [{"parts": [{"inlineData": {"mimeType": "image/png", "data": "AA=="}}]}],
                    "generationConfig": {"temperature": 0.2}
                }
            }),
        ] {
            validate_gemini_count_tokens_request(&valid).expect("valid CountTokens request");
        }

        for invalid in [
            json!(null),
            json!({}),
            json!({"contents": []}),
            json!({"contents": "secret"}),
            json!({"contents": [{"parts": []}]}),
            json!({"contents": [{"parts": [{}]}]}),
            json!({
                "contents": [{"parts": [{"text": "a"}]}],
                "generateContentRequest": {"contents": [{"parts": [{"text": "b"}]}]}
            }),
            json!({"model": "models/path-conflict", "contents": [{"parts": [{"text": "a"}]}]}),
            json!({"generateContentRequest": {}}),
            json!({"generateContentRequest": {"generationConfig": {"temperature": 0.2}}}),
            json!({
                "generateContentRequest": {
                    "model": "models/path-conflict",
                    "contents": [{"parts": [{"text": "a"}]}]
                }
            }),
        ] {
            let error = validate_gemini_count_tokens_request(&invalid)
                .expect_err("invalid CountTokens request");
            assert_eq!(error.code(), ProxyErrorCode::InvalidRequestError);
            assert!(!error.operator_message().contains("secret"));
            assert!(!error.operator_message().contains("path-conflict"));
        }
    }

    #[test]
    fn gemini_count_tokens_observer_is_typed_bounded_and_future_safe() {
        let observed = observe_gemini_count_tokens_response(&json!({
            "totalTokens": 23,
            "cachedContentTokenCount": 3,
            "promptTokensDetails": [
                {"modality": "TEXT", "tokenCount": 20},
                {"modality": "FUTURE_MODALITY", "tokenCount": 3}
            ],
            "futureCountField": {"private": "ignored"}
        }))
        .expect("valid CountTokens response observation");

        assert_eq!(observed.total_tokens, 23);
        assert_eq!(observed.cached_content_token_count, Some(3));
        assert_eq!(observed.prompt_token_details.len(), 2);
        assert_eq!(
            observed.prompt_token_details[0].modality,
            GeminiTokenModality::Text
        );
        assert_eq!(
            observed.prompt_token_details[1].modality,
            GeminiTokenModality::Unknown
        );

        for (invalid, expected) in [
            (
                json!([]),
                GeminiCountTokensObservationError::InvalidEnvelope,
            ),
            (
                json!({"totalTokens": -1}),
                GeminiCountTokensObservationError::InvalidTotalTokens,
            ),
            (
                json!({"totalTokens": 1, "cachedContentTokenCount": "private"}),
                GeminiCountTokensObservationError::InvalidCachedContentTokenCount,
            ),
            (
                json!({"totalTokens": 1, "promptTokensDetails": {}}),
                GeminiCountTokensObservationError::InvalidPromptTokenDetails,
            ),
            (
                json!({"totalTokens": 1, "promptTokensDetails": [{"modality": "TEXT"}]}),
                GeminiCountTokensObservationError::InvalidPromptTokenDetail,
            ),
        ] {
            assert_eq!(
                observe_gemini_count_tokens_response(&invalid),
                Err(expected)
            );
        }

        let too_many = vec![json!({"modality": "TEXT", "tokenCount": 1}); 33];
        assert_eq!(
            observe_gemini_count_tokens_response(&json!({
                "totalTokens": 33,
                "promptTokensDetails": too_many
            })),
            Err(GeminiCountTokensObservationError::TooManyPromptTokenDetails)
        );
    }

    #[test]
    fn openai_embeddings_validate_official_fields_and_keep_unknown_extensions() {
        for input in [
            json!("hello"),
            json!(["hello", "world"]),
            json!([1, 2, 3]),
            json!([[1, 2], [3, 4]]),
        ] {
            assert!(
                validate_embeddings_request(
                    &json!({
                        "model": "text-embedding-3-small",
                        "input": input,
                        "encoding_format": "base64",
                        "dimensions": 256,
                        "user": "operator-client",
                        "vendor_extension": {"enabled": true}
                    }),
                    UpstreamProfileType::Openai,
                )
                .is_ok()
            );
        }

        for invalid in [
            json!([]),
            json!([""]),
            json!([1, "mixed"]),
            json!([-1, 2]),
            json!([[1], []]),
            json!({"text": "hello"}),
        ] {
            assert!(
                validate_embeddings_request(
                    &json!({"model": "embedding", "input": invalid}),
                    UpstreamProfileType::Openai,
                )
                .is_err()
            );
        }

        let oversized = vec![json!(1); MAX_EMBEDDING_INPUT_ARRAY_LENGTH + 1];
        assert!(
            validate_embeddings_request(
                &json!({"model": "embedding", "input": oversized}),
                UpstreamProfileType::Openai,
            )
            .is_err()
        );
    }

    #[test]
    fn openai_embeddings_reject_invalid_known_official_fields() {
        for request in [
            json!({"model": "embedding", "input": "hello", "encoding_format": "hex"}),
            json!({"model": "embedding", "input": "hello", "dimensions": 0}),
            json!({"model": "embedding", "input": "hello", "user": 42}),
        ] {
            let error =
                validate_embeddings_request(&request, UpstreamProfileType::Openai).unwrap_err();
            assert_eq!(error.code(), ProxyErrorCode::InvalidRequestError);
        }
    }

    #[test]
    fn compatible_embeddings_only_validate_the_core_envelope() {
        assert!(
            validate_embeddings_request(
                &json!({
                    "model": "embedding",
                    "input": ["hello", "world"],
                    "encoding_format": {"vendor": true},
                    "dimensions": "vendor-default",
                    "vendor_extension": [1, 2, 3]
                }),
                UpstreamProfileType::OpenaiCompatible,
            )
            .is_ok()
        );
    }

    #[test]
    fn gemini_openai_embeddings_use_a_closed_model_and_string_input_contract() {
        assert!(
            validate_embeddings_request(
                &json!({"model": "gemini-embedding-001", "input": "hello"}),
                UpstreamProfileType::GeminiOpenai,
            )
            .is_ok()
        );
        for request in [
            json!({"model": "gemini-embedding-001", "input": ["hello"]}),
            json!({"model": "gemini-embedding-001", "input": "hello", "dimensions": 256}),
            json!({"model": "gemini-embedding-001", "input": "hello", "extension": true}),
        ] {
            assert!(
                validate_embeddings_request(&request, UpstreamProfileType::GeminiOpenai).is_err()
            );
        }
    }

    #[test]
    fn embeddings_require_an_object_non_empty_model_and_input() {
        for request in [
            json!(null),
            json!([]),
            json!({}),
            json!({"model": "", "input": "hello"}),
            json!({"model": "embedding"}),
        ] {
            let error =
                validate_embeddings_request(&request, UpstreamProfileType::Openai).unwrap_err();
            assert_eq!(error.code(), ProxyErrorCode::InvalidRequestError);
        }
    }

    #[test]
    fn rerank_is_a_compatible_only_transparent_envelope() {
        let request = json!({
            "model": "rerank-model",
            "query": {"provider": "specific"},
            "documents": "provider-defined",
            "private_extension": {"nested": [true, 42]}
        });
        assert!(validate_rerank_request(&request, UpstreamProfileType::OpenaiCompatible).is_ok());
        for profile in [
            UpstreamProfileType::Openai,
            UpstreamProfileType::GeminiOpenai,
            UpstreamProfileType::Gemini,
        ] {
            let error = validate_rerank_request(&request, profile).unwrap_err();
            assert_eq!(error.code(), ProxyErrorCode::UnsupportedCapabilityError);
        }
    }

    #[test]
    fn rerank_requires_an_object_with_a_non_empty_string_model() {
        for request in [
            json!(null),
            json!([]),
            json!({}),
            json!({"model": ""}),
            json!({"model": 42}),
        ] {
            let error = validate_rerank_request(&request, UpstreamProfileType::OpenaiCompatible)
                .unwrap_err();
            assert_eq!(error.code(), ProxyErrorCode::InvalidRequestError);
        }
    }
}
