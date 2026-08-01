use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, http::HeaderMap, response::Response};

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
    schema::enum_def::{DownstreamProtocol, UpstreamProtocol},
    service::{app_state::AppState, cache::types::CacheApiKey},
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
    use super::{UtilityOperation, UtilityProtocol, validate_utility_target};
    use crate::{
        proxy::ProxyErrorCode,
        schema::enum_def::{DownstreamProtocol, UpstreamProtocol},
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
}
