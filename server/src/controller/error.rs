use axum::{
    response::{IntoResponse, Response},
    Json,
};
use reqwest::StatusCode;
use serde_json::json;

#[derive(Debug)]
pub enum BaseError {
    ParamInvalid(Option<String>),
    DatabaseFatal(Option<String>),
    DatabaseDup(Option<String>),
    NotFound(Option<String>),
    Unauthorized(Option<String>),
    StoreError(Option<String>), // For AppStoreError
    InternalServerError(Option<String>),
}

impl From<crate::service::app_state::AppStoreError> for BaseError {
    fn from(err: crate::service::app_state::AppStoreError) -> Self {
        BaseError::StoreError(Some(err.to_string()))
    }
}

impl From<diesel::result::Error> for BaseError {
    fn from(err: diesel::result::Error) -> Self {
        BaseError::DatabaseFatal(Some(err.to_string()))
    }
}

impl IntoResponse for BaseError {
    fn into_response(self) -> Response {
        let (status, error_code, error_message) = match self {
            BaseError::ParamInvalid(msg) => (
                StatusCode::BAD_REQUEST,
                1001,
                msg.unwrap_or("request params invalid".to_string()),
            ),
            BaseError::DatabaseFatal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                1100,
                msg.unwrap_or("database unknown error".to_string()),
            ),
            BaseError::DatabaseDup(msg) => (
                StatusCode::BAD_REQUEST,
                1101,
                msg.unwrap_or("some unique keys have conflicted".to_string()),
            ),
            BaseError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                1002,
                msg.unwrap_or("data not found".to_string()),
            ),
            BaseError::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                1003,
                msg.unwrap_or("Unauthorized".to_string()),
            ),
            BaseError::StoreError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                1200, // New error code category for store errors
                msg.unwrap_or("Application cache/store operation failed".to_string()),
            ),
            BaseError::InternalServerError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                0,
                msg.unwrap_or("internal server error".to_string()),
            ),
        };
        let body = Json(json!({
            "code": error_code,
            "msg": error_message,
        }));
        (status, body).into_response()
    }
}
