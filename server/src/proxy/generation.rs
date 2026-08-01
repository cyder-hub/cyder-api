use std::sync::Arc;

use axum::{body::Body, http::HeaderMap, response::Response};
use serde_json::Value;

use super::{
    ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility,
    cancellation::ProxyCancellationContext,
    request::ParsedProxyRequest,
    request_context::ProxyRequestContext,
    runtime::{
        facade::{GenerationOrchestrationInput, execute_generation},
        route_resolver::ExecutionPlan,
    },
};
use crate::{
    schema::enum_def::DownstreamProtocol,
    service::{app_state::AppState, cache::types::CacheApiKey},
};

pub(super) struct GenerationExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub downstream_protocol: DownstreamProtocol,
    pub execution_plan: ExecutionPlan,
    pub is_stream: bool,
    pub query_params: std::collections::HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub request_context: Arc<ProxyRequestContext>,
    pub parsed_request: ParsedProxyRequest,
}

pub(super) fn extract_model_from_request(data: &Value) -> Result<&str, ProxyError> {
    data.get("model").and_then(Value::as_str).ok_or_else(|| {
        ProxyError::gateway(
            ProxyErrorCode::InvalidRequestError,
            ExecutionStage::Parse,
            ResponseVisibility::NotVisible,
            Some("'model' field must be a string".to_string()),
            "'model' field must be a string",
        )
    })
}

pub(super) async fn execute_generation_proxy(
    app_state: Arc<AppState>,
    input: GenerationExecutionInput,
) -> Result<Response<Body>, ProxyError> {
    let GenerationExecutionInput {
        cancellation,
        api_key,
        downstream_protocol,
        execution_plan,
        is_stream,
        query_params,
        original_headers,
        client_ip_addr,
        request_context,
        parsed_request,
    } = input;
    let ParsedProxyRequest { data } = parsed_request;

    execute_generation(
        app_state,
        GenerationOrchestrationInput {
            cancellation,
            api_key,
            downstream_protocol,
            execution_plan,
            is_stream,
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
    use super::extract_model_from_request;
    use crate::proxy::ProxyErrorCode;
    use serde_json::json;

    #[test]
    fn extract_model_from_request_reads_string_model() {
        let data = json!({"model":"provider/gpt-test"});

        assert_eq!(
            extract_model_from_request(&data).unwrap(),
            "provider/gpt-test"
        );
    }

    #[test]
    fn extract_model_from_request_rejects_non_string_model() {
        let data = json!({"model":123});

        let err = extract_model_from_request(&data).unwrap_err();

        assert_eq!(err.code(), ProxyErrorCode::InvalidRequestError);
    }
}
