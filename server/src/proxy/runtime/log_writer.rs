use std::sync::Arc;

use axum::http::StatusCode;
use chrono::Utc;
use tokio::sync::Mutex as TokioMutex;

use crate::{
    cost::UsageNormalization,
    proxy::{
        ProxyError, ProxyErrorCode,
        logging::{RequestLogContext, record_request_completion_and_log},
        request_context::ProxyRequestContext,
        runtime::route_resolver::ExecutionTarget,
    },
    schema::enum_def::{DownstreamProtocol, RequestStatus},
    service::{
        app_state::AppState,
        cache::types::{CacheApiKey, CacheCostCatalogVersion},
    },
    utils::usage::UsageInfo,
};

pub(in crate::proxy) struct RequestLogContextInput<'a> {
    pub api_key: &'a CacheApiKey,
    pub target: &'a ExecutionTarget,
    pub requested_model_name: &'a str,
    pub base_requested_model_name: &'a str,
    pub resolved_reasoning_suffix: Option<&'a str>,
    pub resolved_reasoning_preset: Option<&'a str>,
    pub client_ip_addr: &'a Option<String>,
    pub request_context: &'a ProxyRequestContext,
    pub downstream_protocol: DownstreamProtocol,
}

pub(in crate::proxy) fn new_request_log_context(
    input: RequestLogContextInput<'_>,
) -> RequestLogContext {
    let mut context = RequestLogContext::new(
        input.api_key,
        &input.target.provider,
        &input.target.model,
        &input.target.upstream_source,
        None,
        input.requested_model_name,
        input.request_context,
        input.client_ip_addr,
        input.downstream_protocol,
        input.target.upstream_protocol,
    );
    context.set_model_resolution_trace(
        input.base_requested_model_name,
        input.resolved_reasoning_suffix,
        input.resolved_reasoning_preset,
    );
    context
}

pub(in crate::proxy) fn finalize_request_failure_context(
    context: &mut RequestLogContext,
    proxy_error: &ProxyError,
) {
    context.completed_at = Some(Utc::now().timestamp_millis());
    context.overall_status = if proxy_error.code() == ProxyErrorCode::ClientCancelledError {
        RequestStatus::Cancelled
    } else {
        RequestStatus::Error
    };
    apply_final_error_fact(context, proxy_error);
}

pub(in crate::proxy) fn apply_final_error_fact(
    context: &mut RequestLogContext,
    proxy_error: &ProxyError,
) {
    context.final_error_code = Some(proxy_error.code().as_str().to_string());
    context.final_error_message = Some(truncate(proxy_error.operator_message(), 2_000));
    context.final_error_stage = Some(proxy_error.stage());
    context.response_visibility = proxy_error.response_visibility();
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(in crate::proxy) async fn record_completion(
    app_state: &Arc<AppState>,
    log_context: RequestLogContext,
) -> bool {
    if let Some(coordinator) = &log_context.completion_coordinator
        && !coordinator.claim_request_log()
    {
        return false;
    }
    record_completion_claimed(app_state, log_context).await
}

pub(in crate::proxy) async fn record_completion_claimed(
    app_state: &Arc<AppState>,
    log_context: RequestLogContext,
) -> bool {
    record_request_completion_and_log(app_state, log_context).await;
    true
}

pub(in crate::proxy) async fn record_streaming_completion(
    app_state: &Arc<AppState>,
    log_context: &RequestLogContext,
) {
    record_completion(app_state, log_context.clone()).await;
}

pub(in crate::proxy) fn finalize_non_streaming_log_context(
    context: &mut RequestLogContext,
    url: &str,
    status_code: StatusCode,
    completed_at: i64,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    overall_status: RequestStatus,
    usage: Option<UsageInfo>,
    usage_normalization: Option<UsageNormalization>,
) {
    context.request_url = Some(url.to_string());
    context.llm_status = Some(status_code);
    context.completed_at = Some(completed_at);
    context.usage = usage;
    context.usage_normalization = usage_normalization;
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = overall_status;
}

pub(in crate::proxy) fn finalize_streaming_log_context(
    context: &mut RequestLogContext,
    url: &str,
    status_code: StatusCode,
    completed_at: i64,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    overall_status: RequestStatus,
    final_error: Option<&ProxyError>,
) {
    context.request_url = Some(url.to_string());
    context.llm_status = Some(status_code);
    context.completed_at = Some(completed_at);
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = overall_status;
    if let Some(error) = final_error {
        apply_final_error_fact(context, error);
    }
}

pub(in crate::proxy) async fn finalize_cancelled_log_context(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: Option<StatusCode>,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    proxy_error: &ProxyError,
) -> bool {
    let mut context = log_context.lock().await;
    if let Some(coordinator) = &context.completion_coordinator
        && !coordinator.claim_request_log()
    {
        return false;
    }
    context.request_url = Some(url.to_string());
    context.llm_status = status_code;
    context.completed_at = Some(Utc::now().timestamp_millis());
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = RequestStatus::Cancelled;
    apply_final_error_fact(&mut context, proxy_error);
    crate::logging::log_proxy_error_event(
        "proxy.stream_terminal_error",
        Some(context.request_id.as_str()),
        Some(context.id),
        proxy_error,
    );
    record_completion_claimed(app_state, context.clone()).await
}
