use axum::{extract::{Path, Query}, routing::get}; // Added Path, Router will be replaced by StateRouter
use crate::service::app_state::{create_state_router, StateRouter};
use crate::{
    database::{
        request_log::{RequestLog, RequestLogQueryPayload}, // Updated to use request_log module
        ListResult,
    },
    utils::HttpResult,
};

use super::error::BaseError;

async fn list_request_log(
    Query(payload): Query<RequestLogQueryPayload>, // Use RequestLogQueryPayload
) -> Result<HttpResult<ListResult<RequestLog>>, BaseError> { // Return ListResult<RequestLog>
    match RequestLog::list(payload) { // Call RequestLog::list
        Ok(result) => Ok(HttpResult::new(result)),
        Err(e) => Err(e), // Propagate the actual error
    }
}

async fn get_request_log(Path(id): Path<i64>) -> Result<HttpResult<RequestLog>, BaseError> { // Implement get_record
    match RequestLog::get_by_id(id) {
        Ok(record) => Ok(HttpResult::new(record)),
        Err(e) => Err(e), // Propagate the actual error
    }
}

pub fn create_record_router() -> StateRouter {
    create_state_router().nest(
        "/request_log", // API endpoint remains /record
        create_state_router()
            .route("/list", get(list_request_log))
            .route("/{id}", get(get_request_log)),
    )
}
