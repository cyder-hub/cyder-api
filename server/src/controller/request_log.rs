use axum::{
    extract::{Path, Query},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
};
use cyder_tools::log::debug;
use serde::{Deserialize, Serialize};

use crate::{
    database::{
        ListResult,
        request_log::{
            RequestLog, RequestLogListItem, RequestLogQueryPayload as DbRequestLogQueryPayload,
            RequestLogRecord,
        },
    },
    schema::enum_def::{LlmApiType, RequestStatus, StorageType},
    service::{
        app_state::StateRouter,
        storage::{Storage, get_local_storage, get_s3_storage, types::GetObjectOptions},
    },
    utils::{HttpResult, storage::generate_storage_path_from_id},
};

use super::error::BaseError;

#[derive(Deserialize, Debug, Default)]
struct RequestLogQueryParams {
    api_key_id: Option<i64>,
    provider_id: Option<i64>,
    model_id: Option<i64>,
    status: Option<RequestStatus>,
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
    provider_id: i64,
    requested_model_name: Option<String>,
    resolved_name_scope: Option<String>,
    resolved_route_name: Option<String>,
    model_name: String,
    request_received_at: i64,
    llm_request_sent_at: i64,
    llm_response_first_chunk_at: Option<i64>,
    llm_response_completed_at: Option<i64>,
    status: Option<RequestStatus>,
    is_stream: bool,
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    total_input_tokens: Option<i32>,
    total_output_tokens: Option<i32>,
    reasoning_tokens: Option<i32>,
    total_tokens: Option<i32>,
}

impl From<RequestLogListItem> for RequestLogListItemResponse {
    fn from(value: RequestLogListItem) -> Self {
        Self {
            id: value.id,
            api_key_id: value.api_key_id,
            provider_id: value.provider_id,
            requested_model_name: value.requested_model_name,
            resolved_name_scope: value.resolved_name_scope,
            resolved_route_name: value.resolved_route_name,
            model_name: value.model_name,
            request_received_at: value.request_received_at,
            llm_request_sent_at: value.llm_request_sent_at,
            llm_response_first_chunk_at: value.llm_response_first_chunk_at,
            llm_response_completed_at: value.llm_response_completed_at,
            status: value.status,
            is_stream: value.is_stream,
            estimated_cost_nanos: value.estimated_cost_nanos,
            estimated_cost_currency: value.estimated_cost_currency,
            total_input_tokens: value.total_input_tokens,
            total_output_tokens: value.total_output_tokens,
            reasoning_tokens: value.reasoning_tokens,
            total_tokens: value.total_tokens,
        }
    }
}

#[derive(Serialize, Debug)]
struct RequestLogResponse {
    id: i64,
    api_key_id: i64,
    provider_id: i64,
    model_id: i64,
    provider_api_key_id: i64,
    requested_model_name: Option<String>,
    resolved_name_scope: Option<String>,
    resolved_route_id: Option<i64>,
    resolved_route_name: Option<String>,
    model_name: String,
    real_model_name: String,
    request_received_at: i64,
    llm_request_sent_at: i64,
    llm_response_first_chunk_at: Option<i64>,
    llm_response_completed_at: Option<i64>,
    client_ip: Option<String>,
    llm_request_uri: Option<String>,
    llm_response_status: Option<i32>,
    status: Option<RequestStatus>,
    is_stream: bool,
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
    storage_type: Option<StorageType>,
    user_request_body: Option<String>,
    llm_request_body: Option<String>,
    llm_response_body: Option<String>,
    user_response_body: Option<String>,
    applied_request_patch_ids_json: Option<String>,
    request_patch_summary_json: Option<String>,
    user_api_type: LlmApiType,
    llm_api_type: LlmApiType,
}

impl From<RequestLogRecord> for RequestLogResponse {
    fn from(value: RequestLogRecord) -> Self {
        Self {
            id: value.id,
            api_key_id: value.api_key_id,
            provider_id: value.provider_id,
            model_id: value.model_id,
            provider_api_key_id: value.provider_api_key_id,
            requested_model_name: value.requested_model_name,
            resolved_name_scope: value.resolved_name_scope,
            resolved_route_id: value.resolved_route_id,
            resolved_route_name: value.resolved_route_name,
            model_name: value.model_name,
            real_model_name: value.real_model_name,
            request_received_at: value.request_received_at,
            llm_request_sent_at: value.llm_request_sent_at,
            llm_response_first_chunk_at: value.llm_response_first_chunk_at,
            llm_response_completed_at: value.llm_response_completed_at,
            client_ip: value.client_ip,
            llm_request_uri: value.llm_request_uri,
            llm_response_status: value.llm_response_status,
            status: value.status,
            is_stream: value.is_stream,
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
            storage_type: value.storage_type,
            user_request_body: value.user_request_body,
            llm_request_body: value.llm_request_body,
            llm_response_body: value.llm_response_body,
            user_response_body: value.user_response_body,
            applied_request_patch_ids_json: value.applied_request_patch_ids_json,
            request_patch_summary_json: value.request_patch_summary_json,
            user_api_type: value.user_api_type,
            llm_api_type: value.llm_api_type,
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

async fn get_request_log(Path(id): Path<i64>) -> Result<HttpResult<RequestLogResponse>, BaseError> {
    match RequestLog::get_by_id(id) {
        Ok(record) => Ok(HttpResult::new(record.into())),
        Err(e) => Err(e),
    }
}

async fn get_request_log_content(Path(id): Path<i64>) -> Result<Response, BaseError> {
    match RequestLog::get_by_id(id) {
        Ok(record) => {
            if let Some(storage_type) = record.storage_type {
                let key =
                    generate_storage_path_from_id(record.created_at, record.id, &storage_type);
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
            } else {
                Err(BaseError::NotFound(Some(
                    "Storage type not found".to_string(),
                )))
            }
        }
        Err(e) => Err(e),
    }
}

pub fn create_record_router() -> StateRouter {
    StateRouter::new().nest(
        "/request_log",
        StateRouter::new()
            .route("/list", get(list_request_log))
            .route("/{id}", get(get_request_log))
            .route("/{id}/content", get(get_request_log_content)),
    )
}

#[cfg(test)]
mod tests {
    use super::{RequestLogListItemResponse, RequestLogQueryParams, create_record_router};
    use crate::schema::enum_def::RequestStatus;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn create_record_router_registers_content_endpoint() {
        let _router = create_record_router();
    }

    #[test]
    fn request_log_query_params_use_api_key_id() {
        let params: RequestLogQueryParams = from_value(json!({
            "api_key_id": 42,
            "status": "SUCCESS"
        }))
        .expect("query params should deserialize");

        assert_eq!(params.api_key_id, Some(42));
        assert_eq!(params.status, Some(RequestStatus::Success));
    }

    #[test]
    fn request_log_list_item_response_serializes_api_key_id() {
        let value = to_value(RequestLogListItemResponse {
            id: 1,
            api_key_id: 2,
            provider_id: 3,
            requested_model_name: None,
            resolved_name_scope: None,
            resolved_route_name: None,
            model_name: "gpt-test".to_string(),
            request_received_at: 10,
            llm_request_sent_at: 11,
            llm_response_first_chunk_at: None,
            llm_response_completed_at: None,
            status: Some(RequestStatus::Success),
            is_stream: false,
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        })
        .expect("response should serialize");

        assert_eq!(value.get("api_key_id").and_then(|v| v.as_i64()), Some(2));
        assert!(value.get("system_api_key_id").is_none());
    }
}
