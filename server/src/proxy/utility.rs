use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, http::HeaderMap, response::Response};

use super::{
    ProxyError,
    cancellation::ProxyCancellationContext,
    request::ParsedProxyRequest,
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
    pub start_time: i64,
    pub parsed_request: ParsedProxyRequest,
}

pub(super) fn validate_utility_target(
    operation: &UtilityOperation,
    upstream_protocol: UpstreamProtocol,
) -> Result<(), ProxyError> {
    match (operation.protocol, upstream_protocol) {
        (UtilityProtocol::OpenaiCompatible, UpstreamProtocol::Openai) => Ok(()),
        (UtilityProtocol::GeminiCompatible, UpstreamProtocol::Gemini) => Ok(()),
        (UtilityProtocol::OpenaiCompatible, _) => Err(ProxyError::BadRequest(format!(
            "'{}' is only supported for OpenAI-compatible providers.",
            operation.name
        ))),
        (UtilityProtocol::GeminiCompatible, _) => Err(ProxyError::BadRequest(format!(
            "Action '{}' is only supported for Gemini-compatible providers.",
            operation.name
        ))),
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
        start_time,
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
            start_time,
            data,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{UtilityOperation, UtilityProtocol, validate_utility_target};
    use crate::{
        proxy::ProxyError,
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
        assert!(matches!(
            validate_utility_target(&operation, UpstreamProtocol::Gemini),
            Err(ProxyError::BadRequest(_))
        ));
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
        assert!(matches!(
            validate_utility_target(&operation, UpstreamProtocol::Openai),
            Err(ProxyError::BadRequest(_))
        ));
    }
}
