use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use cyder_tools::log::debug;
use serde::{Deserialize, Serialize};

use crate::{
    database::{
        ListResult,
        request_attempt::{RequestAttempt, RequestAttemptDetail},
        request_log::{
            RequestLog, RequestLogListItem, RequestLogQueryPayload as DbRequestLogQueryPayload,
            RequestLogRecord,
        },
        request_replay_run::{RequestReplayRun, RequestReplayRunRecord},
    },
    schema::enum_def::{
        LlmApiType, RequestAttemptStatus, RequestReplayKind, RequestReplayMode,
        RequestReplaySemanticBasis, RequestReplayStatus, RequestStatus, SchedulerAction,
        StorageType,
    },
    service::{
        app_state::{AppState, StateRouter},
        request_log_artifact::{
            RequestLogArtifactResponse, get_request_log_artifacts as load_request_log_artifacts,
        },
        request_replay::{
            AttemptReplayExecuteParams, AttemptReplayPreviewParams, AttemptReplayPreviewResponse,
            GatewayReplayExecuteParams, GatewayReplayPreviewParams, GatewayReplayPreviewResponse,
            RequestReplayArtifact, execute_attempt_replay as execute_attempt_replay_service,
            execute_gateway_replay as execute_gateway_replay_service, load_replay_artifact_for_run,
            preview_attempt_replay as preview_attempt_replay_service,
            preview_gateway_replay as preview_gateway_replay_service,
        },
        storage::{Storage, get_local_storage, get_s3_storage, types::GetObjectOptions},
    },
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
    resolved_name_scope: Option<String>,
    final_error_code: Option<String>,
    has_retry: Option<bool>,
    has_fallback: Option<bool>,
    has_transform_diagnostics: Option<bool>,
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
            resolved_name_scope: value.resolved_name_scope,
            final_error_code: value.final_error_code,
            has_retry: value.has_retry,
            has_fallback: value.has_fallback,
            has_transform_diagnostics: value.has_transform_diagnostics,
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
    resolved_name_scope: Option<String>,
    resolved_route_name: Option<String>,
    overall_status: RequestStatus,
    attempt_count: i32,
    retry_count: i32,
    fallback_count: i32,
    request_received_at: i64,
    first_attempt_started_at: Option<i64>,
    response_started_to_client_at: Option<i64>,
    completed_at: Option<i64>,
    final_provider_id: Option<i64>,
    final_provider_name_snapshot: Option<String>,
    final_model_id: Option<i64>,
    final_model_name_snapshot: Option<String>,
    final_real_model_name_snapshot: Option<String>,
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    total_input_tokens: Option<i32>,
    total_output_tokens: Option<i32>,
    reasoning_tokens: Option<i32>,
    total_tokens: Option<i32>,
    has_transform_diagnostics: bool,
    transform_diagnostic_count: i32,
    transform_diagnostic_max_loss_level: Option<String>,
}

impl From<RequestLogListItem> for RequestLogListItemResponse {
    fn from(value: RequestLogListItem) -> Self {
        Self {
            id: value.id,
            api_key_id: value.api_key_id,
            requested_model_name: value.requested_model_name,
            resolved_name_scope: value.resolved_name_scope,
            resolved_route_name: value.resolved_route_name,
            overall_status: value.overall_status,
            attempt_count: value.attempt_count,
            retry_count: value.retry_count,
            fallback_count: value.fallback_count,
            request_received_at: value.request_received_at,
            first_attempt_started_at: value.first_attempt_started_at,
            response_started_to_client_at: value.response_started_to_client_at,
            completed_at: value.completed_at,
            final_provider_id: value.final_provider_id,
            final_provider_name_snapshot: value.final_provider_name_snapshot,
            final_model_id: value.final_model_id,
            final_model_name_snapshot: value.final_model_name_snapshot,
            final_real_model_name_snapshot: value.final_real_model_name_snapshot,
            estimated_cost_nanos: value.estimated_cost_nanos,
            estimated_cost_currency: value.estimated_cost_currency,
            total_input_tokens: value.total_input_tokens,
            total_output_tokens: value.total_output_tokens,
            reasoning_tokens: value.reasoning_tokens,
            total_tokens: value.total_tokens,
            has_transform_diagnostics: value.has_transform_diagnostics,
            transform_diagnostic_count: value.transform_diagnostic_count,
            transform_diagnostic_max_loss_level: value.transform_diagnostic_max_loss_level,
        }
    }
}

#[derive(Serialize, Debug)]
struct RequestLogResponse {
    id: i64,
    api_key_id: i64,
    requested_model_name: Option<String>,
    resolved_name_scope: Option<String>,
    resolved_route_id: Option<i64>,
    resolved_route_name: Option<String>,
    user_api_type: LlmApiType,
    overall_status: RequestStatus,
    final_error_code: Option<String>,
    final_error_message: Option<String>,
    attempt_count: i32,
    retry_count: i32,
    fallback_count: i32,
    request_received_at: i64,
    first_attempt_started_at: Option<i64>,
    response_started_to_client_at: Option<i64>,
    completed_at: Option<i64>,
    client_ip: Option<String>,
    final_attempt_id: Option<i64>,
    final_provider_id: Option<i64>,
    final_provider_api_key_id: Option<i64>,
    final_model_id: Option<i64>,
    final_provider_key_snapshot: Option<String>,
    final_provider_name_snapshot: Option<String>,
    final_model_name_snapshot: Option<String>,
    final_real_model_name_snapshot: Option<String>,
    final_llm_api_type: Option<LlmApiType>,
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    cost_catalog_id: Option<i64>,
    cost_catalog_version_id: Option<i64>,
    cost_snapshot_json: Option<String>,
    created_at: i64,
    updated_at: i64,
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
    has_transform_diagnostics: bool,
    transform_diagnostic_count: i32,
    transform_diagnostic_max_loss_level: Option<String>,
    bundle_version: Option<i32>,
    bundle_storage_type: Option<StorageType>,
    bundle_storage_key: Option<String>,
}

impl From<RequestLogRecord> for RequestLogResponse {
    fn from(value: RequestLogRecord) -> Self {
        Self {
            id: value.id,
            api_key_id: value.api_key_id,
            requested_model_name: value.requested_model_name,
            resolved_name_scope: value.resolved_name_scope,
            resolved_route_id: value.resolved_route_id,
            resolved_route_name: value.resolved_route_name,
            user_api_type: value.user_api_type,
            overall_status: value.overall_status,
            final_error_code: value.final_error_code,
            final_error_message: value.final_error_message,
            attempt_count: value.attempt_count,
            retry_count: value.retry_count,
            fallback_count: value.fallback_count,
            request_received_at: value.request_received_at,
            first_attempt_started_at: value.first_attempt_started_at,
            response_started_to_client_at: value.response_started_to_client_at,
            completed_at: value.completed_at,
            client_ip: value.client_ip,
            final_attempt_id: value.final_attempt_id,
            final_provider_id: value.final_provider_id,
            final_provider_api_key_id: value.final_provider_api_key_id,
            final_model_id: value.final_model_id,
            final_provider_key_snapshot: value.final_provider_key_snapshot,
            final_provider_name_snapshot: value.final_provider_name_snapshot,
            final_model_name_snapshot: value.final_model_name_snapshot,
            final_real_model_name_snapshot: value.final_real_model_name_snapshot,
            final_llm_api_type: value.final_llm_api_type,
            estimated_cost_nanos: value.estimated_cost_nanos,
            estimated_cost_currency: value.estimated_cost_currency,
            cost_catalog_id: value.cost_catalog_id,
            cost_catalog_version_id: value.cost_catalog_version_id,
            cost_snapshot_json: value.cost_snapshot_json,
            created_at: value.created_at,
            updated_at: value.updated_at,
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
            has_transform_diagnostics: value.has_transform_diagnostics,
            transform_diagnostic_count: value.transform_diagnostic_count,
            transform_diagnostic_max_loss_level: value.transform_diagnostic_max_loss_level,
            bundle_version: value.bundle_version,
            bundle_storage_type: value.bundle_storage_type,
            bundle_storage_key: value.bundle_storage_key,
        }
    }
}

#[derive(Serialize, Debug)]
struct RequestAttemptResponse {
    id: i64,
    request_log_id: i64,
    attempt_index: i32,
    candidate_position: i32,
    provider_id: Option<i64>,
    provider_api_key_id: Option<i64>,
    model_id: Option<i64>,
    provider_key_snapshot: Option<String>,
    provider_name_snapshot: Option<String>,
    model_name_snapshot: Option<String>,
    real_model_name_snapshot: Option<String>,
    llm_api_type: Option<LlmApiType>,
    attempt_status: RequestAttemptStatus,
    scheduler_action: SchedulerAction,
    error_code: Option<String>,
    error_message: Option<String>,
    request_uri: Option<String>,
    request_headers_json: Option<String>,
    response_headers_json: Option<String>,
    http_status: Option<i32>,
    started_at: Option<i64>,
    first_byte_at: Option<i64>,
    completed_at: Option<i64>,
    response_started_to_client: bool,
    backoff_ms: Option<i32>,
    applied_request_patch_ids_json: Option<String>,
    request_patch_summary_json: Option<String>,
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    cost_catalog_version_id: Option<i64>,
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
    llm_request_blob_id: Option<i32>,
    llm_request_patch_id: Option<i32>,
    llm_response_blob_id: Option<i32>,
    llm_response_capture_state: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl From<RequestAttemptDetail> for RequestAttemptResponse {
    fn from(value: RequestAttemptDetail) -> Self {
        Self {
            id: value.id,
            request_log_id: value.request_log_id,
            attempt_index: value.attempt_index,
            candidate_position: value.candidate_position,
            provider_id: value.provider_id,
            provider_api_key_id: value.provider_api_key_id,
            model_id: value.model_id,
            provider_key_snapshot: value.provider_key_snapshot,
            provider_name_snapshot: value.provider_name_snapshot,
            model_name_snapshot: value.model_name_snapshot,
            real_model_name_snapshot: value.real_model_name_snapshot,
            llm_api_type: value.llm_api_type,
            attempt_status: value.attempt_status,
            scheduler_action: value.scheduler_action,
            error_code: value.error_code,
            error_message: value.error_message,
            request_uri: value.request_uri,
            request_headers_json: value.request_headers_json,
            response_headers_json: value.response_headers_json,
            http_status: value.http_status,
            started_at: value.started_at,
            first_byte_at: value.first_byte_at,
            completed_at: value.completed_at,
            response_started_to_client: value.response_started_to_client,
            backoff_ms: value.backoff_ms,
            applied_request_patch_ids_json: value.applied_request_patch_ids_json,
            request_patch_summary_json: value.request_patch_summary_json,
            estimated_cost_nanos: value.estimated_cost_nanos,
            estimated_cost_currency: value.estimated_cost_currency,
            cost_catalog_version_id: value.cost_catalog_version_id,
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
            llm_request_blob_id: value.llm_request_blob_id,
            llm_request_patch_id: value.llm_request_patch_id,
            llm_response_blob_id: value.llm_response_blob_id,
            llm_response_capture_state: value.llm_response_capture_state,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Serialize, Debug)]
struct RequestLogDetailResponse {
    request: RequestLogResponse,
    attempts: Vec<RequestAttemptResponse>,
}

#[derive(Serialize, Debug)]
struct RequestReplayRunResponse {
    id: i64,
    source_request_log_id: i64,
    source_attempt_id: Option<i64>,
    replay_kind: RequestReplayKind,
    replay_mode: RequestReplayMode,
    semantic_basis: RequestReplaySemanticBasis,
    status: RequestReplayStatus,
    executed_route_id: Option<i64>,
    executed_route_name: Option<String>,
    executed_provider_id: Option<i64>,
    executed_provider_api_key_id: Option<i64>,
    executed_model_id: Option<i64>,
    executed_llm_api_type: Option<LlmApiType>,
    downstream_request_uri: Option<String>,
    http_status: Option<i32>,
    error_code: Option<String>,
    error_message: Option<String>,
    total_input_tokens: Option<i32>,
    total_output_tokens: Option<i32>,
    reasoning_tokens: Option<i32>,
    total_tokens: Option<i32>,
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    diff_summary_json: Option<String>,
    artifact_version: Option<i32>,
    artifact_storage_type: Option<StorageType>,
    artifact_storage_key: Option<String>,
    started_at: Option<i64>,
    first_byte_at: Option<i64>,
    completed_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

impl From<RequestReplayRunRecord> for RequestReplayRunResponse {
    fn from(value: RequestReplayRunRecord) -> Self {
        Self {
            id: value.id,
            source_request_log_id: value.source_request_log_id,
            source_attempt_id: value.source_attempt_id,
            replay_kind: value.replay_kind,
            replay_mode: value.replay_mode,
            semantic_basis: value.semantic_basis,
            status: value.status,
            executed_route_id: value.executed_route_id,
            executed_route_name: value.executed_route_name,
            executed_provider_id: value.executed_provider_id,
            executed_provider_api_key_id: value.executed_provider_api_key_id,
            executed_model_id: value.executed_model_id,
            executed_llm_api_type: value.executed_llm_api_type,
            downstream_request_uri: value.downstream_request_uri,
            http_status: value.http_status,
            error_code: value.error_code,
            error_message: value.error_message,
            total_input_tokens: value.total_input_tokens,
            total_output_tokens: value.total_output_tokens,
            reasoning_tokens: value.reasoning_tokens,
            total_tokens: value.total_tokens,
            estimated_cost_nanos: value.estimated_cost_nanos,
            estimated_cost_currency: value.estimated_cost_currency,
            diff_summary_json: value.diff_summary_json,
            artifact_version: value.artifact_version,
            artifact_storage_type: value.artifact_storage_type,
            artifact_storage_key: value.artifact_storage_key,
            started_at: value.started_at,
            first_byte_at: value.first_byte_at,
            completed_at: value.completed_at,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

async fn list_request_log(
    Query(params): Query<RequestLogQueryParams>,
) -> Result<HttpResult<ListResult<RequestLogListItemResponse>>, BaseError> {
    match RequestLog::list(params.into()) {
        Ok(result) => Ok(HttpResult::new(ListResult {
            total: result.total,
            page: result.page,
            page_size: result.page_size,
            list: result.list.into_iter().map(Into::into).collect(),
        })),
        Err(e) => Err(e),
    }
}

async fn get_request_log(
    Path(id): Path<i64>,
) -> Result<HttpResult<RequestLogDetailResponse>, BaseError> {
    match RequestLog::get_by_id(id) {
        Ok(record) => {
            let attempts = RequestAttempt::list_by_request_log_id(id)?;
            Ok(HttpResult::new(RequestLogDetailResponse {
                request: record.into(),
                attempts: attempts.into_iter().map(Into::into).collect(),
            }))
        }
        Err(e) => Err(e),
    }
}

async fn get_request_log_artifacts(
    Path(id): Path<i64>,
) -> Result<HttpResult<RequestLogArtifactResponse>, BaseError> {
    Ok(HttpResult::new(load_request_log_artifacts(id).await?))
}

async fn preview_attempt_replay(
    State(app_state): State<Arc<AppState>>,
    Path((request_log_id, attempt_id)): Path<(i64, i64)>,
    Json(payload): Json<AttemptReplayPreviewParams>,
) -> Result<HttpResult<AttemptReplayPreviewResponse>, BaseError> {
    Ok(HttpResult::new(
        preview_attempt_replay_service(&app_state, request_log_id, attempt_id, payload).await?,
    ))
}

async fn execute_attempt_replay(
    State(app_state): State<Arc<AppState>>,
    Path((request_log_id, attempt_id)): Path<(i64, i64)>,
    Json(payload): Json<AttemptReplayExecuteParams>,
) -> Result<HttpResult<RequestReplayRunResponse>, BaseError> {
    let run =
        execute_attempt_replay_service(&app_state, request_log_id, attempt_id, payload).await?;
    Ok(HttpResult::new(run.into()))
}

async fn preview_gateway_replay(
    State(app_state): State<Arc<AppState>>,
    Path(request_log_id): Path<i64>,
    Json(payload): Json<GatewayReplayPreviewParams>,
) -> Result<HttpResult<GatewayReplayPreviewResponse>, BaseError> {
    Ok(HttpResult::new(
        preview_gateway_replay_service(&app_state, request_log_id, payload).await?,
    ))
}

async fn execute_gateway_replay(
    State(app_state): State<Arc<AppState>>,
    Path(request_log_id): Path<i64>,
    Json(payload): Json<GatewayReplayExecuteParams>,
) -> Result<HttpResult<RequestReplayRunResponse>, BaseError> {
    let run = execute_gateway_replay_service(&app_state, request_log_id, payload).await?;
    Ok(HttpResult::new(run.into()))
}

async fn list_request_replay_runs(
    Path(request_log_id): Path<i64>,
) -> Result<HttpResult<Vec<RequestReplayRunResponse>>, BaseError> {
    let runs = RequestReplayRun::list_by_source_request_log_id(request_log_id)?;
    Ok(HttpResult::new(runs.into_iter().map(Into::into).collect()))
}

async fn get_request_replay_run(
    Path((request_log_id, replay_run_id)): Path<(i64, i64)>,
) -> Result<HttpResult<RequestReplayRunResponse>, BaseError> {
    let run = RequestReplayRun::get_by_source_and_id(request_log_id, replay_run_id)?;
    Ok(HttpResult::new(run.into()))
}

async fn get_request_replay_artifacts(
    Path((request_log_id, replay_run_id)): Path<(i64, i64)>,
) -> Result<HttpResult<RequestReplayArtifact>, BaseError> {
    let run = RequestReplayRun::get_by_source_and_id(request_log_id, replay_run_id)?;
    Ok(HttpResult::new(load_replay_artifact_for_run(&run).await?))
}

fn resolve_request_log_content_location(
    storage_type: Option<StorageType>,
    storage_key: Option<String>,
) -> Result<(StorageType, String), BaseError> {
    match (storage_type, storage_key) {
        (Some(storage_type), Some(key)) => Ok((storage_type, key)),
        _ => Err(BaseError::NotFound(Some(
            "Storage type not found".to_string(),
        ))),
    }
}

async fn get_request_log_content(Path(id): Path<i64>) -> Result<Response, BaseError> {
    match RequestLog::get_by_id(id) {
        Ok(record) => match resolve_request_log_content_location(
            record.bundle_storage_type,
            record.bundle_storage_key,
        ) {
            Ok((storage_type, key)) => {
                let storage: &dyn Storage = match storage_type {
                    StorageType::FileSystem => get_local_storage().await,
                    StorageType::S3 => get_s3_storage().await.ok_or_else(|| {
                        BaseError::NotFound(Some("S3 storage not available".to_string()))
                    })?,
                };
                debug!("Getting request log content for key: {}", key);
                let content = storage
                    .get_object(
                        &key,
                        Some(GetObjectOptions {
                            content_encoding: Some(&""),
                        }),
                    )
                    .await
                    .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

                let cache_headers =
                    [(header::CACHE_CONTROL, "public, max-age=31536000, immutable")];
                let mut response = (cache_headers, content).into_response();
                let headers = response.headers_mut();
                headers.insert(
                    header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("application/msgpack"),
                );
                headers.insert(
                    header::CONTENT_ENCODING,
                    axum::http::HeaderValue::from_static("gzip"),
                );
                Ok(response)
            }
            Err(error) => Err(error),
        },
        Err(e) => Err(e),
    }
}

pub fn create_record_router() -> StateRouter {
    StateRouter::new().nest(
        "/request_log",
        StateRouter::new()
            .route("/list", get(list_request_log))
            .route("/{id}", get(get_request_log))
            .route("/{id}/artifacts", get(get_request_log_artifacts))
            .route(
                "/{id}/replay/attempt/{attempt_id}/preview",
                post(preview_attempt_replay),
            )
            .route(
                "/{id}/replay/attempt/{attempt_id}/execute",
                post(execute_attempt_replay),
            )
            .route("/{id}/replay/gateway/preview", post(preview_gateway_replay))
            .route("/{id}/replay/gateway/execute", post(execute_gateway_replay))
            .route("/{id}/replay", get(list_request_replay_runs))
            .route("/{id}/replay/{replay_run_id}", get(get_request_replay_run))
            .route(
                "/{id}/replay/{replay_run_id}/artifacts",
                get(get_request_replay_artifacts),
            )
            .route("/{id}/content", get(get_request_log_content)),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        RequestAttemptResponse, RequestLogDetailResponse, RequestLogListItemResponse,
        RequestLogQueryParams, RequestLogResponse, RequestReplayRunResponse, create_record_router,
        resolve_request_log_content_location,
    };
    use crate::controller::BaseError;
    use crate::database::request_replay_run::RequestReplayRunRecord;
    use crate::schema::enum_def::{
        LlmApiType, RequestAttemptStatus, RequestReplayKind, RequestReplayMode,
        RequestReplaySemanticBasis, RequestReplayStatus, RequestStatus, SchedulerAction,
        StorageType,
    };
    use serde_json::{from_value, json, to_value};

    #[test]
    fn create_record_router_registers_content_endpoint() {
        let _router = create_record_router();
    }

    #[test]
    fn request_replay_run_response_serializes_lightweight_summary() {
        let value = to_value(RequestReplayRunResponse::from(RequestReplayRunRecord {
            id: 1001,
            source_request_log_id: 42,
            source_attempt_id: Some(101),
            replay_kind: RequestReplayKind::AttemptUpstream,
            replay_mode: RequestReplayMode::Live,
            semantic_basis: RequestReplaySemanticBasis::HistoricalAttemptSnapshot,
            status: RequestReplayStatus::Success,
            http_status: Some(200),
            total_tokens: Some(12),
            diff_summary_json: Some("{\"status_changed\":false}".to_string()),
            artifact_version: Some(1),
            artifact_storage_type: Some(StorageType::FileSystem),
            artifact_storage_key: Some("replays/2026/04/22/1001.mp.gz".to_string()),
            created_at: 100,
            updated_at: 200,
            ..Default::default()
        }))
        .expect("response should serialize");

        assert_eq!(value.get("id").and_then(|item| item.as_i64()), Some(1001));
        assert_eq!(
            value.get("replay_kind").and_then(|item| item.as_str()),
            Some("attempt_upstream")
        );
        assert_eq!(
            value.get("status").and_then(|item| item.as_str()),
            Some("success")
        );
        assert_eq!(
            value
                .get("artifact_storage_type")
                .and_then(|item| item.as_str()),
            Some("FILE_SYSTEM")
        );
        assert!(value.get("response_body").is_none());
        assert!(value.get("input_snapshot").is_none());
    }

    #[test]
    fn request_log_query_params_use_api_key_id() {
        let params: RequestLogQueryParams = from_value(json!({
            "api_key_id": 42,
            "status": "SUCCESS",
            "has_retry": true,
            "latency_ms_min": 100
        }))
        .expect("query params should deserialize");

        assert_eq!(params.api_key_id, Some(42));
        assert_eq!(params.status, Some(RequestStatus::Success));
        assert_eq!(params.has_retry, Some(true));
        assert_eq!(params.latency_ms_min, Some(100));
    }

    #[test]
    fn request_log_list_item_response_serializes_api_key_id() {
        let value = to_value(RequestLogListItemResponse {
            id: 1,
            api_key_id: 2,
            requested_model_name: None,
            resolved_name_scope: None,
            resolved_route_name: None,
            overall_status: RequestStatus::Success,
            attempt_count: 1,
            retry_count: 0,
            fallback_count: 0,
            request_received_at: 10,
            first_attempt_started_at: Some(11),
            response_started_to_client_at: None,
            completed_at: Some(12),
            final_provider_id: Some(3),
            final_provider_name_snapshot: Some("OpenAI".to_string()),
            final_model_id: Some(4),
            final_model_name_snapshot: Some("gpt-test".to_string()),
            final_real_model_name_snapshot: Some("gpt-test-real".to_string()),
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            has_transform_diagnostics: true,
            transform_diagnostic_count: 1,
            transform_diagnostic_max_loss_level: Some("lossy_minor".to_string()),
        })
        .expect("response should serialize");

        assert_eq!(value.get("api_key_id").and_then(|v| v.as_i64()), Some(2));
        assert_eq!(
            value
                .get("has_transform_diagnostics")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        let legacy_api_key_id_field = ["system", "api", "key", "id"].join("_");
        assert!(value.get(&legacy_api_key_id_field).is_none());
    }

    #[test]
    fn request_log_content_location_uses_persisted_bundle_key_only() {
        let (storage_type, key) = resolve_request_log_content_location(
            Some(StorageType::FileSystem),
            Some("explicit/bundle-key.mp.gz".to_string()),
        )
        .expect("persisted bundle key should resolve");

        assert_eq!(storage_type, StorageType::FileSystem);
        assert_eq!(key, "explicit/bundle-key.mp.gz");

        let missing_key = resolve_request_log_content_location(Some(StorageType::FileSystem), None);
        assert!(matches!(missing_key, Err(BaseError::NotFound(_))));

        let missing_storage =
            resolve_request_log_content_location(None, Some("legacy-derived-key".to_string()));
        assert!(matches!(missing_storage, Err(BaseError::NotFound(_))));
    }

    #[test]
    fn request_log_detail_response_wraps_request_and_attempts() {
        let value = to_value(RequestLogDetailResponse {
            request: RequestLogResponse {
                id: 1,
                api_key_id: 2,
                requested_model_name: Some("gpt-test".to_string()),
                resolved_name_scope: Some("direct".to_string()),
                resolved_route_id: None,
                resolved_route_name: None,
                user_api_type: LlmApiType::Openai,
                overall_status: RequestStatus::Success,
                final_error_code: None,
                final_error_message: None,
                attempt_count: 1,
                retry_count: 0,
                fallback_count: 0,
                request_received_at: 10,
                first_attempt_started_at: Some(11),
                response_started_to_client_at: Some(12),
                completed_at: Some(13),
                client_ip: Some("127.0.0.1".to_string()),
                final_attempt_id: Some(100),
                final_provider_id: Some(3),
                final_provider_api_key_id: Some(4),
                final_model_id: Some(5),
                final_provider_key_snapshot: Some("openai".to_string()),
                final_provider_name_snapshot: Some("OpenAI".to_string()),
                final_model_name_snapshot: Some("gpt-test".to_string()),
                final_real_model_name_snapshot: Some("gpt-test-real".to_string()),
                final_llm_api_type: Some(LlmApiType::Openai),
                estimated_cost_nanos: Some(1000),
                estimated_cost_currency: Some("USD".to_string()),
                cost_catalog_id: None,
                cost_catalog_version_id: None,
                cost_snapshot_json: None,
                created_at: 10,
                updated_at: 13,
                total_input_tokens: Some(10),
                total_output_tokens: Some(20),
                input_text_tokens: Some(10),
                output_text_tokens: Some(20),
                input_image_tokens: None,
                output_image_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: Some(1),
                total_tokens: Some(31),
                has_transform_diagnostics: true,
                transform_diagnostic_count: 1,
                transform_diagnostic_max_loss_level: Some("lossy_major".to_string()),
                bundle_version: Some(2),
                bundle_storage_type: Some(StorageType::FileSystem),
                bundle_storage_key: Some("logs/1.mp.gz".to_string()),
            },
            attempts: vec![RequestAttemptResponse {
                id: 100,
                request_log_id: 1,
                attempt_index: 1,
                candidate_position: 1,
                provider_id: Some(3),
                provider_api_key_id: Some(4),
                model_id: Some(5),
                provider_key_snapshot: Some("openai".to_string()),
                provider_name_snapshot: Some("OpenAI".to_string()),
                model_name_snapshot: Some("gpt-test".to_string()),
                real_model_name_snapshot: Some("gpt-test-real".to_string()),
                llm_api_type: Some(LlmApiType::Openai),
                attempt_status: RequestAttemptStatus::Success,
                scheduler_action: SchedulerAction::ReturnSuccess,
                error_code: None,
                error_message: None,
                request_uri: Some("https://api.example.com/v1/chat/completions".to_string()),
                request_headers_json: Some("{}".to_string()),
                response_headers_json: Some("{}".to_string()),
                http_status: Some(200),
                started_at: Some(11),
                first_byte_at: Some(12),
                completed_at: Some(13),
                response_started_to_client: true,
                backoff_ms: None,
                applied_request_patch_ids_json: Some("[1]".to_string()),
                request_patch_summary_json: Some("{\"rules\":1}".to_string()),
                estimated_cost_nanos: Some(1000),
                estimated_cost_currency: Some("USD".to_string()),
                cost_catalog_version_id: None,
                total_input_tokens: Some(10),
                total_output_tokens: Some(20),
                input_text_tokens: Some(10),
                output_text_tokens: Some(20),
                input_image_tokens: None,
                output_image_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: Some(1),
                total_tokens: Some(31),
                llm_request_blob_id: Some(1),
                llm_request_patch_id: None,
                llm_response_blob_id: Some(2),
                llm_response_capture_state: None,
                created_at: 11,
                updated_at: 13,
            }],
        })
        .expect("response should serialize");

        assert!(value.get("request").is_some());
        assert_eq!(
            value
                .pointer("/request/api_key_id")
                .and_then(|item| item.as_i64()),
            Some(2)
        );
        assert_eq!(
            value
                .pointer("/request/overall_status")
                .and_then(|item| item.as_str()),
            Some("SUCCESS")
        );
        assert_eq!(
            value
                .pointer("/attempts/0/attempt_status")
                .and_then(|item| item.as_str()),
            Some("SUCCESS")
        );
        let legacy_api_key_id_field = ["system", "api", "key", "id"].join("_");
        assert!(value.get(&legacy_api_key_id_field).is_none());
        assert!(value.pointer("/request/llm_request_body").is_none());
    }
}
