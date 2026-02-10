use axum::{
    extract::{Path, Query},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use cyder_tools::log::debug;
use serde::{Deserialize, Serialize};

use crate::{
    config::CONFIG,
    database::{
        request_log::{RequestLog, RequestLogQueryPayload},
        ListResult,
    },
    schema::enum_def::StorageType,
    service::{
        app_state::StateRouter,
        storage::{
            get_local_storage, get_s3_storage,
            types::{GetObjectOptions, PutObjectOptions},
            Storage,
        },
    },
    utils::{
        storage::{generate_storage_path_from_id, LogBodies},
        HttpResult,
    },
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

async fn get_request_log_content_by_hash(
    Path((storage_type_str, date_str, hash)): Path<(String, String, String)>,
) -> Result<Response, BaseError> {
    let storage_type: StorageType = serde_json::from_str(&format!("\"{}\"", storage_type_str))
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

    // Handle S3 Redirect mode first, as it's a special case.
    if matches!(storage_type, StorageType::S3) {
        if let Some(s3_config) = &CONFIG.storage.s3 {
            if s3_config.access_mode == crate::config::S3AccessMode::Redirect {
                if let Some(storage) = get_s3_storage().await {
                    let url = storage
                        .get_presigned_url(&key)
                        .await
                        .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
                    return Ok((
                        StatusCode::FOUND,
                        [
                            (header::LOCATION, url),
                            (header::CACHE_CONTROL, "public, max-age=3540".to_string()),
                        ],
                    )
                        .into_response());
                } else {
                    return Err(BaseError::NotFound(Some(
                        "S3 storage not available".to_string(),
                    )));
                }
            }
        }
    }

    // All other cases (FileSystem, S3 Proxy) are handled here.
    let storage: &dyn Storage = match storage_type {
        StorageType::FileSystem => get_local_storage().await,
        StorageType::S3 => get_s3_storage()
            .await
            .ok_or_else(|| BaseError::NotFound(Some("S3 storage not available".to_string())))?,
    };

    let content = storage
        .get_object(
            &key,
            None,
        )
        .await
        .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

    let cache_headers = [
        (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        (header::ETAG, hash.as_str()),
    ];

    let mut response = (cache_headers, content).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain"),
    );

    headers.insert(
        header::CONTENT_ENCODING,
        axum::http::HeaderValue::from_static("gzip"),
    );

    Ok(response)
}

async fn get_content_from_hash(
    storage: &dyn Storage,
    storage_type: &StorageType,
    date_str: &str,
    hash: &str,
) -> Result<Vec<u8>, BaseError> {
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
    storage
        .get_object(
            &key,
            None,
            //Some(GetObjectOptions {
            //    content_encoding: Some(""),
            //}),
        )
        .await
        .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))
        .map(|bytes| bytes.into())
}

pub fn create_record_router() -> StateRouter {
    StateRouter::new().nest(
        "/request_log",
        StateRouter::new()
            .route("/list", get(list_request_log))
            .route("/{id}", get(get_request_log))
            .route("/{id}/content", get(get_request_log_content))
            .route(
                "/{storage_type}/{date}/{hash}",
                get(get_request_log_content_by_hash),
            ),
    )
}
