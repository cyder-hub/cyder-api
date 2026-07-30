use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, http::HeaderMap, response::Response};
use serde_json::Value;

use super::{
    executor::{RequestExecutionInput, RequestExecutionKind, execute_request},
    route_resolver::ExecutionPlan,
};
use crate::{
    proxy::{ProxyError, cancellation::ProxyCancellationContext, utility::UtilityOperation},
    schema::enum_def::DownstreamProtocol,
    service::{app_state::AppState, cache::types::CacheApiKey},
};

pub(in crate::proxy) struct GenerationOrchestrationInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub downstream_protocol: DownstreamProtocol,
    pub execution_plan: ExecutionPlan,
    pub is_stream: bool,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub data: Value,
}

pub(in crate::proxy) struct UtilityOrchestrationInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub operation: UtilityOperation,
    pub execution_plan: ExecutionPlan,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub data: Value,
}

pub(in crate::proxy) async fn execute_generation(
    app_state: Arc<AppState>,
    input: GenerationOrchestrationInput,
) -> Result<Response<Body>, ProxyError> {
    execute_request(
        app_state,
        RequestExecutionInput {
            cancellation: input.cancellation,
            api_key: input.api_key,
            execution_plan: input.execution_plan,
            query_params: input.query_params,
            original_headers: input.original_headers,
            client_ip_addr: input.client_ip_addr,
            start_time: input.start_time,
            kind: RequestExecutionKind::Generation {
                downstream_protocol: input.downstream_protocol,
                is_stream: input.is_stream,
                data: input.data,
            },
        },
    )
    .await
}

pub(in crate::proxy) async fn execute_utility(
    app_state: Arc<AppState>,
    input: UtilityOrchestrationInput,
) -> Result<Response<Body>, ProxyError> {
    execute_request(
        app_state,
        RequestExecutionInput {
            cancellation: input.cancellation,
            api_key: input.api_key,
            execution_plan: input.execution_plan,
            query_params: input.query_params,
            original_headers: input.original_headers,
            client_ip_addr: input.client_ip_addr,
            start_time: input.start_time,
            kind: RequestExecutionKind::Utility {
                operation: input.operation,
                data: input.data,
            },
        },
    )
    .await
}
