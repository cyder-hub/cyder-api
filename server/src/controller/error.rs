use axum::{
    Json,
    response::{IntoResponse, Response},
};
use reqwest::StatusCode;
use serde_json::json;

#[derive(Debug)]
pub enum BaseError {
    ParamInvalid(Option<String>),
    DatabaseFatal(Option<String>),
    DatabaseDup(Option<String>),
    NotFound(Option<String>),
    ApiKeySecretUnavailable,
    ProviderApiKeySecretUnavailable,
    ProviderRuntimeRefreshFailed,
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

impl From<crate::database::error::PersistenceError> for BaseError {
    fn from(err: crate::database::error::PersistenceError) -> Self {
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
            BaseError::ApiKeySecretUnavailable => (
                StatusCode::CONFLICT,
                1004,
                "api key secret is unavailable".to_string(),
            ),
            BaseError::ProviderApiKeySecretUnavailable => (
                StatusCode::CONFLICT,
                1005,
                "provider API key secret is unavailable; replace the credential".to_string(),
            ),
            BaseError::ProviderRuntimeRefreshFailed => (
                StatusCode::SERVICE_UNAVAILABLE,
                1201,
                "provider configuration was committed, but runtime refresh failed; the provider is fail-closed"
                    .to_string(),
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

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode, response::IntoResponse};

    use super::BaseError;

    #[tokio::test]
    async fn manager_base_error_status_and_code_contract_is_stable() {
        let cases = [
            (BaseError::ParamInvalid(None), StatusCode::BAD_REQUEST, 1001),
            (
                BaseError::DatabaseFatal(None),
                StatusCode::INTERNAL_SERVER_ERROR,
                1100,
            ),
            (BaseError::DatabaseDup(None), StatusCode::BAD_REQUEST, 1101),
            (BaseError::NotFound(None), StatusCode::NOT_FOUND, 1002),
            (
                BaseError::ApiKeySecretUnavailable,
                StatusCode::CONFLICT,
                1004,
            ),
            (
                BaseError::ProviderApiKeySecretUnavailable,
                StatusCode::CONFLICT,
                1005,
            ),
            (
                BaseError::ProviderRuntimeRefreshFailed,
                StatusCode::SERVICE_UNAVAILABLE,
                1201,
            ),
            (
                BaseError::Unauthorized(None),
                StatusCode::UNAUTHORIZED,
                1003,
            ),
            (
                BaseError::StoreError(None),
                StatusCode::INTERNAL_SERVER_ERROR,
                1200,
            ),
            (
                BaseError::InternalServerError(None),
                StatusCode::INTERNAL_SERVER_ERROR,
                0,
            ),
        ];

        for (error, expected_status, expected_code) in cases {
            let response = error.into_response();
            assert_eq!(response.status(), expected_status);
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("manager error response body should read");
            let body: serde_json::Value =
                serde_json::from_slice(&body).expect("manager error response should be JSON");
            assert_eq!(body["code"], expected_code);
            assert!(
                body["msg"]
                    .as_str()
                    .is_some_and(|message| !message.is_empty())
            );
        }
    }
}
