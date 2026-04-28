use std::{collections::HashMap, sync::Arc};

use axum::{
    body::{Body, Bytes},
    http::HeaderMap,
    response::Response,
};
use serde_json::Value;

use super::{
    executor::AttemptExecutionKind,
    policy::{RuntimeExecutionPolicy, RuntimeLogMode},
    route_resolver::ExecutionPlan,
    scheduler::{SchedulerExecutionInput, schedule_execution},
};
use crate::{
    proxy::{
        ProxyError,
        cancellation::ProxyCancellationContext,
        runtime::replay_adapter::{
            GatewayReplayExecutionFailure, GatewayReplayExecutionSuccess, GatewayReplayInput,
            GatewayReplayPreparedRequest,
        },
        utility::UtilityOperation,
    },
    schema::enum_def::LlmApiType,
    service::app_state::AppState,
    service::cache::types::CacheApiKey,
    utils::storage::{RequestLogBundleQueryParam, RequestLogBundleRequestSnapshot},
};

pub(in crate::proxy) struct GenerationOrchestrationInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub api_type: LlmApiType,
    pub execution_plan: ExecutionPlan,
    pub is_stream: bool,
    pub query_params: HashMap<String, String>,
    pub replay_query_params: Option<Vec<RequestLogBundleQueryParam>>,
    pub original_headers: HeaderMap,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub data: Value,
    pub original_request_value: Value,
    pub original_request_body: Bytes,
    pub execution_policy: RuntimeExecutionPolicy,
}

pub(in crate::proxy) struct UtilityOrchestrationInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub operation: UtilityOperation,
    pub execution_plan: ExecutionPlan,
    pub query_params: HashMap<String, String>,
    pub replay_query_params: Option<Vec<RequestLogBundleQueryParam>>,
    pub original_headers: HeaderMap,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub data: Value,
    pub original_request_body: Bytes,
    pub execution_policy: RuntimeExecutionPolicy,
}

pub(in crate::proxy) async fn execute_generation(
    app_state: Arc<AppState>,
    input: GenerationOrchestrationInput,
) -> Result<Response<Body>, ProxyError> {
    debug_assert!(RuntimeExecutionPolicy::Normal.sends_upstream_request());
    debug_assert!(RuntimeExecutionPolicy::Normal.uses_mutating_provider_governance());
    debug_assert!(RuntimeLogMode::RecordAll.should_record_immediate());
    let GenerationOrchestrationInput {
        cancellation,
        api_key,
        api_type,
        execution_plan,
        is_stream,
        query_params,
        replay_query_params,
        original_headers,
        request_snapshot,
        client_ip_addr,
        start_time,
        data,
        original_request_value,
        original_request_body,
        execution_policy,
    } = input;

    Box::pin(schedule_execution(
        Arc::clone(&app_state),
        SchedulerExecutionInput {
            cancellation,
            api_key,
            execution_plan,
            query_params,
            replay_query_params,
            original_headers,
            request_snapshot,
            original_request_body,
            client_ip_addr,
            start_time,
            log_mode: RuntimeLogMode::DeferNonStreaming,
            execution_policy,
            kind: AttemptExecutionKind::Generation {
                user_api_type: api_type,
                is_stream,
                data,
                original_request_value,
            },
        },
    ))
    .await
    .map(|success| success.response)
    .map_err(|failure| failure.error)
}

pub(in crate::proxy) async fn execute_utility(
    app_state: Arc<AppState>,
    input: UtilityOrchestrationInput,
) -> Result<Response<Body>, ProxyError> {
    debug_assert!(RuntimeExecutionPolicy::Normal.sends_upstream_request());
    debug_assert!(RuntimeExecutionPolicy::Normal.uses_mutating_provider_governance());
    let UtilityOrchestrationInput {
        cancellation,
        api_key,
        operation,
        execution_plan,
        query_params,
        replay_query_params,
        original_headers,
        request_snapshot,
        client_ip_addr,
        start_time,
        data,
        original_request_body,
        execution_policy,
    } = input;

    Box::pin(schedule_execution(
        Arc::clone(&app_state),
        SchedulerExecutionInput {
            cancellation,
            api_key,
            execution_plan,
            query_params,
            replay_query_params,
            original_headers,
            request_snapshot,
            original_request_body,
            client_ip_addr,
            start_time,
            log_mode: RuntimeLogMode::DeferNonStreaming,
            execution_policy,
            kind: AttemptExecutionKind::Utility { operation, data },
        },
    ))
    .await
    .map(|success| success.response)
    .map_err(|failure| failure.error)
}

pub(crate) async fn preview_gateway_replay_request(
    app_state: Arc<AppState>,
    input: GatewayReplayInput,
) -> Result<GatewayReplayPreparedRequest, ProxyError> {
    debug_assert!(!RuntimeExecutionPolicy::ReplayDryRun.sends_upstream_request());
    debug_assert!(RuntimeExecutionPolicy::ReplayDryRun.uses_read_only_provider_governance());
    super::replay_adapter::preview_gateway_replay_request(app_state, input).await
}

pub(crate) async fn execute_gateway_replay_request(
    app_state: Arc<AppState>,
    input: GatewayReplayInput,
) -> Result<GatewayReplayExecutionSuccess, GatewayReplayExecutionFailure> {
    debug_assert!(RuntimeExecutionPolicy::ReplayLive.sends_upstream_request());
    debug_assert!(RuntimeExecutionPolicy::ReplayLive.uses_read_only_provider_governance());
    super::replay_adapter::execute_gateway_replay_request(app_state, input).await
}
