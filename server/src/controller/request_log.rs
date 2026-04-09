use axum::{
    extract::{Path, Query},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
};
use cyder_tools::log::debug;

use crate::{
    database::{
        ListResult,
        request_log::{RequestLog, RequestLogQueryPayload},
    },
    schema::enum_def::StorageType,
    service::{
        app_state::StateRouter,
        storage::{Storage, get_local_storage, get_s3_storage, types::GetObjectOptions},
    },
    utils::{HttpResult, storage::generate_storage_path_from_id},
};

use super::error::BaseError;

async fn list_request_log(
    Query(payload): Query<RequestLogQueryPayload>,
) -> Result<HttpResult<ListResult<RequestLog>>, BaseError> {
    match RequestLog::list(payload) {
        Ok(result) => Ok(HttpResult::new(result)),
        Err(e) => Err(e),
    }
}

async fn get_request_log(Path(id): Path<i64>) -> Result<HttpResult<RequestLog>, BaseError> {
    match RequestLog::get_by_id(id) {
        Ok(record) => Ok(HttpResult::new(record)),
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
    use super::create_record_router;

    #[test]
    fn create_record_router_registers_content_endpoint() {
        let _router = create_record_router();
    }
}
