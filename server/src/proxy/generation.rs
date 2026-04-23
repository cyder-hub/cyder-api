use std::sync::Arc;

use axum::{body::Body, http::HeaderMap, response::Response};
use serde_json::Value;

use super::{
    ProxyError,
    cancellation::ProxyCancellationContext,
    core::ProxyExecutionPolicy,
    orchestrator::{GenerationOrchestrationInput, orchestrate_generation},
    prepare::ExecutionPlan,
    request::ParsedProxyRequest,
};
use crate::{
    schema::enum_def::LlmApiType,
    service::{app_state::AppState, cache::types::CacheApiKey},
    utils::storage::RequestLogBundleRequestSnapshot,
};

pub(super) struct GenerationExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub system_api_key: Arc<CacheApiKey>,
    pub api_type: LlmApiType,
    pub execution_plan: ExecutionPlan,
    pub is_stream: bool,
    pub query_params: std::collections::HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub parsed_request: ParsedProxyRequest,
}

pub(super) fn extract_model_from_request(data: &Value) -> Result<&str, ProxyError> {
    data.get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| ProxyError::BadRequest("'model' field must be a string".to_string()))
}

pub(super) async fn execute_generation_proxy(
    app_state: Arc<AppState>,
    input: GenerationExecutionInput,
) -> Result<Response<Body>, ProxyError> {
    let GenerationExecutionInput {
        cancellation,
        system_api_key,
        api_type,
        execution_plan,
        is_stream,
        query_params,
        original_headers,
        request_snapshot,
        client_ip_addr,
        start_time,
        parsed_request,
    } = input;
    let ParsedProxyRequest {
        data,
        original_request_value,
        original_request_body,
    } = parsed_request;

    orchestrate_generation(
        app_state,
        GenerationOrchestrationInput {
            cancellation,
            system_api_key,
            api_type,
            execution_plan,
            is_stream,
            query_params,
            replay_query_params: None,
            original_headers,
            request_snapshot,
            client_ip_addr,
            start_time,
            data,
            original_request_value,
            original_request_body,
            execution_policy: ProxyExecutionPolicy::Normal,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::extract_model_from_request;
    use crate::proxy::ProxyError;
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

        assert!(matches!(err, ProxyError::BadRequest(_)));
    }
}
