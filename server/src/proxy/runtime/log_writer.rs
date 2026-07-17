use std::sync::Arc;

use axum::http::StatusCode;
use chrono::Utc;
use tokio::sync::Mutex as TokioMutex;

use crate::{
    cost::UsageNormalization,
    proxy::{
        ProxyError,
        logging::{RequestLogContext, record_request_completion_and_log},
        runtime::route_resolver::ExecutionTarget,
    },
    schema::enum_def::{LlmApiType, RequestStatus},
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
    pub start_time: i64,
    pub user_api_type: LlmApiType,
}

pub(in crate::proxy) fn new_request_log_context(
    input: RequestLogContextInput<'_>,
) -> RequestLogContext {
    let mut context = RequestLogContext::new(
        input.api_key,
        &input.target.provider,
        &input.target.model,
        None,
        input.requested_model_name,
        input.start_time,
        input.client_ip_addr,
        input.user_api_type,
        input.target.llm_api_type,
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
    context.completion_ts = Some(Utc::now().timestamp_millis());
    context.overall_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
        RequestStatus::Cancelled
    } else {
        RequestStatus::Error
    };
    context.final_error_code = Some(proxy_error.error_code().to_string());
    context.final_error_message = Some(truncate(proxy_error.message(), 2_000));
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(in crate::proxy) async fn record_completion(
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
    completion_ts: i64,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    overall_status: RequestStatus,
    usage: Option<UsageInfo>,
    usage_normalization: Option<UsageNormalization>,
) {
    context.request_url = Some(url.to_string());
    context.llm_status = Some(status_code);
    context.completion_ts = Some(completion_ts);
    context.usage = usage;
    context.usage_normalization = usage_normalization;
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = overall_status;
}

pub(in crate::proxy) fn finalize_streaming_log_context(
    context: &mut RequestLogContext,
    url: &str,
    status_code: StatusCode,
    completion_ts: i64,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    overall_status: RequestStatus,
    final_error: Option<&ProxyError>,
) {
    context.request_url = Some(url.to_string());
    context.llm_status = Some(status_code);
    context.completion_ts = Some(completion_ts);
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = overall_status;
    if let Some(error) = final_error {
        context.final_error_code = Some(error.error_code().to_string());
        context.final_error_message = Some(truncate(error.message(), 2_000));
    }
}

pub(in crate::proxy) async fn finalize_cancelled_log_context(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: Option<StatusCode>,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
) -> bool {
    let mut context = log_context.lock().await;
    context.request_url = Some(url.to_string());
    context.llm_status = status_code;
    context.completion_ts = Some(Utc::now().timestamp_millis());
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = RequestStatus::Cancelled;
    record_completion(app_state, context.clone()).await
}
