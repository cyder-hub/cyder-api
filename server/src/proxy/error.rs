use axum::{
    response::{IntoResponse, Response},
    Json,
};
use reqwest::StatusCode;
use serde::Serialize;
use std::fmt;

/// Structured error type for the proxy module.
///
/// All proxy errors are serialized to a flat JSON format:
/// ```json
/// { "code": "invalid_request_error", "message": "..." }
/// ```
#[derive(Debug)]
pub enum ProxyError {
    /// 401 — API key missing, invalid, or format error.
    Unauthorized(String),
    /// 400 — Malformed request body, missing fields, invalid model format.
    BadRequest(String),
    /// 403 — Access control policy denied the request.
    Forbidden(String),
    /// 500 — Internal gateway error (cache failure, serialization, etc.).
    InternalError(String),
    /// 502 — Upstream LLM service unreachable or returned a connection error.
    BadGateway(String),
}

impl ProxyError {
    fn status_code(&self) -> StatusCode {
        match self {
            ProxyError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ProxyError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ProxyError::Forbidden(_) => StatusCode::FORBIDDEN,
            ProxyError::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::BadGateway(_) => StatusCode::BAD_GATEWAY,
        }
    }

    fn error_code(&self) -> &'static str {
        match self {
            ProxyError::Unauthorized(_) => "authentication_error",
            ProxyError::BadRequest(_) => "invalid_request_error",
            ProxyError::Forbidden(_) => "permission_error",
            ProxyError::InternalError(_) => "server_error",
            ProxyError::BadGateway(_) => "upstream_error",
        }
    }

    fn message(&self) -> &str {
        match self {
            ProxyError::Unauthorized(msg)
            | ProxyError::BadRequest(msg)
            | ProxyError::Forbidden(msg)
            | ProxyError::InternalError(msg)
            | ProxyError::BadGateway(msg) => msg,
        }
    }
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.error_code(), self.message())
    }
}

#[derive(Serialize)]
struct ProxyErrorBody {
    code: &'static str,
    message: String,
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ProxyErrorBody {
            code: self.error_code(),
            message: self.message().to_string(),
        };
        (status, Json(body)).into_response()
    }
}
