use axum::{
    extract::{Path, Query},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};

use crate::{
    config::CONFIG,
    database::{
        request_log::{RequestLog, RequestLogQueryPayload},
        ListResult,
    },
    schema::enum_def::StorageType,
    service::{
        app_state::StateRouter,
        storage::{get_local_storage, get_s3_storage, Storage},
    },
    utils::HttpResult,
};

use super::error::BaseError;

fn rewrite_body_path(log: &mut RequestLog) {
    let storage_type_str = if let Some(st) = log.storage_type.as_ref() {
        st.to_string()
    } else {
        return;
    };

    let dt = DateTime::from_timestamp_millis(log.created_at).unwrap_or_else(|| Utc::now());
    let date_str = dt.format("%Y-%m-%d").to_string();

    let rewrite_rules: [fn(&mut RequestLog) -> &mut Option<String>; 4] = [
        |log| &mut log.user_request_body,
        |log| &mut log.llm_request_body,
        |log| &mut log.llm_response_body,
        |log| &mut log.user_response_body,
    ];

    for rule in &rewrite_rules {
        let path_option = rule(log);
        if let Some(hash) = path_option.take() {
            *path_option = Some(format!(
                "/request_log/{}/{}/{}",
                storage_type_str, date_str, hash
            ));
        }
    }
}

async fn list_request_log(
    Query(payload): Query<RequestLogQueryPayload>,
) -> Result<HttpResult<ListResult<RequestLog>>, BaseError> {
    match RequestLog::list(payload) {
        Ok(mut result) => {
            for log in &mut result.list {
                rewrite_body_path(log);
            }
            Ok(HttpResult::new(result))
        }
        Err(e) => Err(e),
    }
}

async fn get_request_log(Path(id): Path<i64>) -> Result<HttpResult<RequestLog>, BaseError> {
    match RequestLog::get_by_id(id) {
        Ok(mut record) => {
            rewrite_body_path(&mut record);
            Ok(HttpResult::new(record))
        }
        Err(e) => Err(e),
    }
}

async fn get_request_log_content_by_hash(
    Path((storage_type_str, date_str, hash)): Path<(String, String, String)>,
) -> Result<Response, BaseError> {
    let storage_type: StorageType =
        serde_json::from_str(&format!("\"{}\"", storage_type_str))
            .map_err(|_| BaseError::ParamInvalid(Some("Invalid storage type".to_string())))?;

    let key = match storage_type {
        StorageType::FileSystem => {
            if hash.len() < 2 {
                return Err(BaseError::ParamInvalid(Some(
                    "Invalid hash format".to_string(),
                )));
            }
            let hash_prefix = &hash[..2];
            format!("{}/{}/{}", date_str, hash_prefix, hash)
        }
        StorageType::S3 => format!("logs/{}/{}", date_str, hash),
    };

    let cache_headers = [
        (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        (header::ETAG, &hash),
    ];

    match storage_type {
        StorageType::FileSystem => {
            let storage = get_local_storage().await;
            let content = storage
                .get_object(&key)
                .await
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
            Ok((
                cache_headers,
                [(header::CONTENT_TYPE, "text/plain")],
                content,
            )
                .into_response())
        }
        StorageType::S3 => {
            if let Some(s3_config) = &CONFIG.storage.s3 {
                if s3_config.access_mode == crate::config::S3AccessMode::Proxy {
                    let storage = get_s3_storage()
                        .await
                        .ok_or_else(|| BaseError::NotFound(Some("S3 storage not available".to_string())))?;
                    let content = storage
                        .get_object(&key)
                        .await
                        .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
                    return Ok((
                        cache_headers,
                        [(header::CONTENT_TYPE, "text/plain")],
                        content,
                    )
                        .into_response());
                }
            }

            if let Some(storage) = get_s3_storage().await {
                let url = storage
                    .get_presigned_url(&key)
                    .await
                    .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
                Ok((
                    StatusCode::FOUND,
                    [
                        (header::LOCATION, url),
                        (header::CACHE_CONTROL, "public, max-age=3540".to_string()),
                    ],
                )
                    .into_response())
            } else {
                Err(BaseError::NotFound(Some(
                    "S3 storage not available".to_string(),
                )))
            }
        }
    }
}

pub fn create_record_router() -> StateRouter {
    StateRouter::new().nest(
        "/request_log",
        StateRouter::new()
            .route("/list", get(list_request_log))
            .route("/{id}", get(get_request_log))
            .route(
                "/{storage_type}/{date}/{hash}",
                get(get_request_log_content_by_hash),
            ),
    )
}
