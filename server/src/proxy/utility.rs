use std::{collections::HashMap, sync::Arc};

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
}

pub(super) struct UtilityExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub operation: UtilityOperation,
    pub execution_plan: ExecutionPlan,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub request_context: Arc<ProxyRequestContext>,
    pub parsed_request: ParsedProxyRequest,
}

pub(super) fn validate_utility_target(
    operation: &UtilityOperation,
    upstream_protocol: UpstreamProtocol,
) -> Result<(), ProxyError> {
    match (operation.protocol, upstream_protocol) {
        (UtilityProtocol::OpenaiCompatible, UpstreamProtocol::Openai) => Ok(()),
        (UtilityProtocol::GeminiCompatible, UpstreamProtocol::Gemini) => Ok(()),
        (UtilityProtocol::OpenaiCompatible, _) => {
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
        (UtilityProtocol::GeminiCompatible, _) => {
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
        query_params,
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
            query_params,
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
        MAX_EMBEDDING_INPUT_ARRAY_LENGTH, UtilityOperation, UtilityProtocol,
        validate_embeddings_request, validate_rerank_request, validate_utility_target,
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

        assert!(validate_utility_target(&operation, UpstreamProtocol::Openai).is_ok());
        let error = validate_utility_target(&operation, UpstreamProtocol::Gemini).unwrap_err();
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

        assert!(validate_utility_target(&operation, UpstreamProtocol::Gemini).is_ok());
        let error = validate_utility_target(&operation, UpstreamProtocol::Openai).unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::UnsupportedCapabilityError);
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
