use axum::{
    extract::{Path, Query},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    database::{
        ListResult,
        request_log::{
            RequestLog, RequestLogListItem, RequestLogQueryPayload as DbRequestLogQueryPayload,
            RequestLogRecord,
        },
    },
    schema::enum_def::{LlmApiType, RequestStatus},
    service::app_state::StateRouter,
    utils::HttpResult,
};

use super::error::BaseError;

#[derive(Deserialize, Debug, Default)]
struct RequestLogQueryParams {
    api_key_id: Option<i64>,
    provider_id: Option<i64>,
    model_id: Option<i64>,
    status: Option<RequestStatus>,
    user_api_type: Option<LlmApiType>,
    final_error_code: Option<String>,
    latency_ms_min: Option<i64>,
    latency_ms_max: Option<i64>,
    total_tokens_min: Option<i32>,
    total_tokens_max: Option<i32>,
    estimated_cost_nanos_min: Option<i64>,
    estimated_cost_nanos_max: Option<i64>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    page: Option<i64>,
    page_size: Option<i64>,
    search: Option<String>,
}

impl From<RequestLogQueryParams> for DbRequestLogQueryPayload {
    fn from(value: RequestLogQueryParams) -> Self {
        Self {
            api_key_id: value.api_key_id,
            provider_id: value.provider_id,
            model_id: value.model_id,
            status: value.status,
            user_api_type: value.user_api_type,
            final_error_code: value.final_error_code,
            latency_ms_min: value.latency_ms_min,
            latency_ms_max: value.latency_ms_max,
            total_tokens_min: value.total_tokens_min,
            total_tokens_max: value.total_tokens_max,
            estimated_cost_nanos_min: value.estimated_cost_nanos_min,
            estimated_cost_nanos_max: value.estimated_cost_nanos_max,
            start_time: value.start_time,
            end_time: value.end_time,
            page: value.page,
            page_size: value.page_size,
            search: value.search,
        }
    }
}

#[derive(Serialize, Debug)]
struct RequestLogListItemResponse {
    id: i64,
    api_key_id: i64,
    requested_model_name: Option<String>,
    base_requested_model_name: Option<String>,
    resolved_reasoning_suffix: Option<String>,
    resolved_reasoning_preset: Option<String>,
    overall_status: RequestStatus,
    request_received_at: i64,
    upstream_request_sent_at: Option<i64>,
    response_started_to_client_at: Option<i64>,
    completed_at: Option<i64>,
    is_stream: bool,
    provider_id: Option<i64>,
    provider_name: Option<String>,
    model_id: Option<i64>,
    model_name: Option<String>,
    real_model_name: Option<String>,
    upstream_http_status: Option<i32>,
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    total_input_tokens: Option<i32>,
    total_output_tokens: Option<i32>,
    output_text_tokens: Option<i32>,
    reasoning_tokens: Option<i32>,
    total_tokens: Option<i32>,
}

impl From<RequestLogListItem> for RequestLogListItemResponse {
    fn from(value: RequestLogListItem) -> Self {
        Self {
            id: value.id,
            api_key_id: value.api_key_id,
            requested_model_name: value.requested_model_name,
            base_requested_model_name: value.base_requested_model_name,
            resolved_reasoning_suffix: value.resolved_reasoning_suffix,
            resolved_reasoning_preset: value.resolved_reasoning_preset,
            overall_status: value.overall_status,
            request_received_at: value.request_received_at,
            upstream_request_sent_at: value.upstream_request_sent_at,
            response_started_to_client_at: value.response_started_to_client_at,
            completed_at: value.completed_at,
            is_stream: value.is_stream,
            provider_id: value.provider_id,
            provider_name: value.provider_name_snapshot,
            model_id: value.model_id,
            model_name: value.model_name_snapshot,
            real_model_name: value.real_model_name_snapshot,
            upstream_http_status: value.upstream_http_status,
            estimated_cost_nanos: value.estimated_cost_nanos,
            estimated_cost_currency: value.estimated_cost_currency,
            total_input_tokens: value.total_input_tokens,
            total_output_tokens: value.total_output_tokens,
            output_text_tokens: value.output_text_tokens,
            reasoning_tokens: value.reasoning_tokens,
            total_tokens: value.total_tokens,
        }
    }
}

#[derive(Serialize, Debug)]
struct RequestLogResponse {
    id: i64,
    api_key_id: i64,
    requested_model_name: Option<String>,
    base_requested_model_name: Option<String>,
    resolved_reasoning_suffix: Option<String>,
    resolved_reasoning_preset: Option<String>,
    user_api_type: LlmApiType,
    overall_status: RequestStatus,
    final_error_code: Option<String>,
    final_error_message: Option<String>,
    request_received_at: i64,
    upstream_request_sent_at: Option<i64>,
    response_started_to_client_at: Option<i64>,
    completed_at: Option<i64>,
    is_stream: bool,
    client_ip: Option<String>,
    provider_id: Option<i64>,
    provider_api_key_id: Option<i64>,
    model_id: Option<i64>,
    provider_key: Option<String>,
    provider_name: Option<String>,
    model_name: Option<String>,
    real_model_name: Option<String>,
    llm_api_type: Option<LlmApiType>,
    upstream_http_status: Option<i32>,
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    cost_catalog_id: Option<i64>,
    cost_catalog_version_id: Option<i64>,
    cost_snapshot_json: Option<String>,
    total_input_tokens: Option<i32>,
    total_output_tokens: Option<i32>,
    input_text_tokens: Option<i32>,
    output_text_tokens: Option<i32>,
    input_image_tokens: Option<i32>,
    output_image_tokens: Option<i32>,
    cache_read_tokens: Option<i32>,
    cache_write_tokens: Option<i32>,
    reasoning_tokens: Option<i32>,
    total_tokens: Option<i32>,
    created_at: i64,
    updated_at: i64,
}

impl From<RequestLogRecord> for RequestLogResponse {
    fn from(value: RequestLogRecord) -> Self {
        Self {
            id: value.id,
            api_key_id: value.api_key_id,
            requested_model_name: value.requested_model_name,
            base_requested_model_name: value.base_requested_model_name,
            resolved_reasoning_suffix: value.resolved_reasoning_suffix,
            resolved_reasoning_preset: value.resolved_reasoning_preset,
            user_api_type: value.user_api_type,
            overall_status: value.overall_status,
            final_error_code: value.final_error_code,
            final_error_message: value.final_error_message,
            request_received_at: value.request_received_at,
            upstream_request_sent_at: value.upstream_request_sent_at,
            response_started_to_client_at: value.response_started_to_client_at,
            completed_at: value.completed_at,
            is_stream: value.is_stream,
            client_ip: value.client_ip,
            provider_id: value.provider_id,
            provider_api_key_id: value.provider_api_key_id,
            model_id: value.model_id,
            provider_key: value.provider_key_snapshot,
            provider_name: value.provider_name_snapshot,
            model_name: value.model_name_snapshot,
            real_model_name: value.real_model_name_snapshot,
            llm_api_type: value.llm_api_type,
            upstream_http_status: value.upstream_http_status,
            estimated_cost_nanos: value.estimated_cost_nanos,
            estimated_cost_currency: value.estimated_cost_currency,
            cost_catalog_id: value.cost_catalog_id,
            cost_catalog_version_id: value.cost_catalog_version_id,
            cost_snapshot_json: value.cost_snapshot_json,
            total_input_tokens: value.total_input_tokens,
            total_output_tokens: value.total_output_tokens,
            input_text_tokens: value.input_text_tokens,
            output_text_tokens: value.output_text_tokens,
            input_image_tokens: value.input_image_tokens,
            output_image_tokens: value.output_image_tokens,
            cache_read_tokens: value.cache_read_tokens,
            cache_write_tokens: value.cache_write_tokens,
            reasoning_tokens: value.reasoning_tokens,
            total_tokens: value.total_tokens,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

async fn list_request_log(
    Query(params): Query<RequestLogQueryParams>,
) -> Result<HttpResult<ListResult<RequestLogListItemResponse>>, BaseError> {
    let result = RequestLog::list(params.into())?;
    Ok(HttpResult::new(ListResult {
        total: result.total,
        page: result.page,
        page_size: result.page_size,
        list: result.list.into_iter().map(Into::into).collect(),
    }))
}

async fn get_request_log(Path(id): Path<i64>) -> Result<HttpResult<RequestLogResponse>, BaseError> {
    Ok(HttpResult::new(RequestLog::get_by_id(id)?.into()))
}

pub fn create_record_router() -> StateRouter {
    StateRouter::new().nest(
        "/request_log",
        StateRouter::new()
            .route("/list", get(list_request_log))
            .route("/{id}", get(get_request_log)),
    )
}
