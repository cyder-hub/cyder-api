use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, http::HeaderMap, response::Response};
use cyder_tools::log::info;

use super::{
    ProxyError,
    cancellation::ProxyCancellationContext,
    core::ProxyExecutionPolicy,
    orchestrator::{UtilityOrchestrationInput, orchestrate_utility},
    prepare::ExecutionPlan,
    request::ParsedProxyRequest,
};
use crate::{
    schema::enum_def::LlmApiType,
    service::{app_state::AppState, cache::types::CacheApiKey},
    utils::storage::RequestLogBundleRequestSnapshot,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UtilityProtocol {
    OpenaiCompatible,
    GeminiCompatible,
}

#[derive(Clone, Debug)]
pub(crate) struct UtilityOperation {
    pub name: String,
    pub api_type: LlmApiType,
    pub protocol: UtilityProtocol,
    pub downstream_path: String,
}

pub(super) struct UtilityExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub system_api_key: Arc<CacheApiKey>,
    pub operation: UtilityOperation,
    pub execution_plan: ExecutionPlan,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub parsed_request: ParsedProxyRequest,
}

pub(super) fn validate_utility_target(
    operation: &UtilityOperation,
    target_api_type: LlmApiType,
) -> Result<(), ProxyError> {
    match (operation.protocol, target_api_type) {
        (UtilityProtocol::OpenaiCompatible, LlmApiType::Openai) => Ok(()),
        (UtilityProtocol::GeminiCompatible, LlmApiType::Gemini) => Ok(()),
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
        system_api_key,
        operation,
        execution_plan,
        query_params,
        original_headers,
        request_snapshot,
        client_ip_addr,
        start_time,
        parsed_request,
    } = input;
    let ParsedProxyRequest {
        data,
        original_request_body,
        ..
    } = parsed_request;

    info!(
        "Processing {:?} utility request ({}) for model: {}",
        operation.api_type, operation.name, execution_plan.requested_name
    );

    orchestrate_utility(
        app_state,
        UtilityOrchestrationInput {
            cancellation,
            system_api_key,
            operation,
            execution_plan,
            query_params,
            replay_query_params: None,
            original_headers,
            request_snapshot,
            client_ip_addr,
            start_time,
            data,
            original_request_body,
            execution_policy: ProxyExecutionPolicy::Normal,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{UtilityOperation, UtilityProtocol, validate_utility_target};
    use crate::{proxy::ProxyError, schema::enum_def::LlmApiType};

    #[test]
    fn validate_utility_target_enforces_openai_compatibility() {
        let operation = UtilityOperation {
            name: "embeddings".to_string(),
            api_type: LlmApiType::Openai,
            protocol: UtilityProtocol::OpenaiCompatible,
            downstream_path: "embeddings".to_string(),
        };

        assert!(validate_utility_target(&operation, LlmApiType::Openai).is_ok());
        assert!(matches!(
            validate_utility_target(&operation, LlmApiType::Gemini),
            Err(ProxyError::BadRequest(_))
        ));
    }

    #[test]
    fn validate_utility_target_enforces_gemini_compatibility() {
        let operation = UtilityOperation {
            name: "countTokens".to_string(),
            api_type: LlmApiType::Gemini,
            protocol: UtilityProtocol::GeminiCompatible,
            downstream_path: "countTokens".to_string(),
        };

        assert!(validate_utility_target(&operation, LlmApiType::Gemini).is_ok());
        assert!(matches!(
            validate_utility_target(&operation, LlmApiType::Openai),
            Err(ProxyError::BadRequest(_))
        ));
    }
}
