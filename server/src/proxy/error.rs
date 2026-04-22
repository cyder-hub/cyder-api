use axum::{
    Json,
    response::{IntoResponse, Response},
};
use reqwest::{Error as ReqwestError, StatusCode};
use serde::Serialize;
use std::fmt;

pub(crate) const REQUEST_PATCH_CONFLICT_ERROR: &str = "request_patch_conflict_error";

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
    /// 403 — API key exists but is disabled.
    KeyDisabled(String),
    /// 403 — API key exists but is expired.
    KeyExpired(String),
    /// 400 — Malformed request body, missing fields, invalid model format.
    BadRequest(String),
    /// 403 — Access control policy denied the request.
    Forbidden(String),
    /// 429 — API key local rate limit was exceeded.
    RateLimited(String),
    /// 429 — API key concurrent request limit was exceeded.
    ConcurrencyLimited(String),
    /// 429 — API key quota was exhausted.
    QuotaExhausted(String),
    /// 403 — API key budget was exhausted.
    BudgetExhausted(String),
    /// 503 — Provider circuit is open and this candidate was skipped.
    ProviderOpenSkipped(String),
    /// 503 — Provider half-open probe is already in flight and this candidate was skipped.
    ProviderHalfOpenProbeInFlight(String),
    /// 413 — Request body exceeds configured size or upstream rejects payload size.
    PayloadTooLarge(String),
    /// 499 — Client disconnected before the request lifecycle completed.
    ClientCancelled(String),
    /// 500 — Effective provider/model request patch rules conflict.
    RequestPatchConflict(String),
    /// 500 — Internal gateway error (cache failure, serialization, etc.).
    InternalError(String),
    /// 500 — Request/response transform or serialization failed.
    ProtocolTransformError(String),
    /// 400 — Upstream rejected the request as invalid.
    UpstreamBadRequest(String),
    /// 429 — Upstream rate limited the request.
    UpstreamRateLimited(String),
    /// 502 — Upstream authentication/authorization failed.
    UpstreamAuthentication(String),
    /// 502 — Upstream LLM service unreachable or returned a connection error.
    BadGateway(String),
    /// 503 — Upstream service returned a server-side failure.
    UpstreamService(String),
    /// 504 — Upstream request or body read timed out.
    UpstreamTimeout(String),
}

impl ProxyError {
    pub(crate) fn status_code(&self) -> StatusCode {
        match self {
            ProxyError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ProxyError::KeyDisabled(_) => StatusCode::FORBIDDEN,
            ProxyError::KeyExpired(_) => StatusCode::FORBIDDEN,
            ProxyError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ProxyError::Forbidden(_) => StatusCode::FORBIDDEN,
            ProxyError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            ProxyError::ConcurrencyLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            ProxyError::QuotaExhausted(_) => StatusCode::TOO_MANY_REQUESTS,
            ProxyError::BudgetExhausted(_) => StatusCode::FORBIDDEN,
            ProxyError::ProviderOpenSkipped(_) | ProxyError::ProviderHalfOpenProbeInFlight(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            ProxyError::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            ProxyError::ClientCancelled(_) => {
                StatusCode::from_u16(499).expect("499 should be a valid status code")
            }
            ProxyError::RequestPatchConflict(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::ProtocolTransformError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::UpstreamBadRequest(_) => StatusCode::BAD_REQUEST,
            ProxyError::UpstreamRateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            ProxyError::UpstreamAuthentication(_) => StatusCode::BAD_GATEWAY,
            ProxyError::BadGateway(_) => StatusCode::BAD_GATEWAY,
            ProxyError::UpstreamService(_) => StatusCode::SERVICE_UNAVAILABLE,
            ProxyError::UpstreamTimeout(_) => StatusCode::GATEWAY_TIMEOUT,
        }
    }

    pub(crate) fn error_code(&self) -> &'static str {
        match self {
            ProxyError::Unauthorized(_) => "authentication_error",
            ProxyError::KeyDisabled(_) => "api_key_disabled_error",
            ProxyError::KeyExpired(_) => "api_key_expired_error",
            ProxyError::BadRequest(_) => "invalid_request_error",
            ProxyError::Forbidden(_) => "permission_error",
            ProxyError::RateLimited(_) => "rate_limit_error",
            ProxyError::ConcurrencyLimited(_) => "concurrency_limit_error",
            ProxyError::QuotaExhausted(_) => "quota_exhausted_error",
            ProxyError::BudgetExhausted(_) => "budget_exhausted_error",
            ProxyError::ProviderOpenSkipped(_) => "provider_open_skipped",
            ProxyError::ProviderHalfOpenProbeInFlight(_) => "provider_half_open_skipped",
            ProxyError::PayloadTooLarge(_) => "body_too_large_error",
            ProxyError::ClientCancelled(_) => "client_cancelled_error",
            ProxyError::RequestPatchConflict(_) => REQUEST_PATCH_CONFLICT_ERROR,
            ProxyError::InternalError(_) => "server_error",
            ProxyError::ProtocolTransformError(_) => "protocol_transform_error",
            ProxyError::UpstreamBadRequest(_) => "upstream_invalid_request_error",
            ProxyError::UpstreamRateLimited(_) => "upstream_rate_limit_error",
            ProxyError::UpstreamAuthentication(_) => "upstream_authentication_error",
            ProxyError::BadGateway(_) => "upstream_error",
            ProxyError::UpstreamService(_) => "upstream_service_error",
            ProxyError::UpstreamTimeout(_) => "upstream_timeout_error",
        }
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            ProxyError::Unauthorized(msg)
            | ProxyError::KeyDisabled(msg)
            | ProxyError::KeyExpired(msg)
            | ProxyError::BadRequest(msg)
            | ProxyError::Forbidden(msg)
            | ProxyError::RateLimited(msg)
            | ProxyError::ConcurrencyLimited(msg)
            | ProxyError::QuotaExhausted(msg)
            | ProxyError::BudgetExhausted(msg)
            | ProxyError::ProviderOpenSkipped(msg)
            | ProxyError::ProviderHalfOpenProbeInFlight(msg)
            | ProxyError::PayloadTooLarge(msg)
            | ProxyError::ClientCancelled(msg)
            | ProxyError::RequestPatchConflict(msg)
            | ProxyError::InternalError(msg)
            | ProxyError::ProtocolTransformError(msg)
            | ProxyError::UpstreamBadRequest(msg)
            | ProxyError::UpstreamRateLimited(msg)
            | ProxyError::UpstreamAuthentication(msg)
            | ProxyError::BadGateway(msg)
            | ProxyError::UpstreamService(msg)
            | ProxyError::UpstreamTimeout(msg) => msg,
        }
    }
}

pub(super) fn classify_request_body_error(message: impl Into<String>) -> ProxyError {
    let message = message.into();
    if is_body_too_large_message(&message) {
        ProxyError::PayloadTooLarge(format!(
            "Request body exceeds configured size limit: {message}"
        ))
    } else {
        ProxyError::BadRequest(format!("Failed to read request body: {message}"))
    }
}

pub(crate) fn protocol_transform_error(operation: &str, err: impl fmt::Display) -> ProxyError {
    ProxyError::ProtocolTransformError(format!("{operation}: {err}"))
}

pub(crate) fn classify_reqwest_error(context: &str, err: &ReqwestError) -> ProxyError {
    if err.is_timeout() {
        return ProxyError::UpstreamTimeout(format!("{context} timed out: {err}"));
    }

    if let Some(status) = err.status() {
        return classify_upstream_status(status, err.to_string().as_bytes());
    }

    if err.is_connect() {
        return ProxyError::BadGateway(format!("{context} could not connect to upstream: {err}"));
    }

    if err.is_body() || err.is_decode() {
        return ProxyError::BadGateway(format!(
            "{context} failed while reading upstream body: {err}"
        ));
    }

    if err.is_request() {
        return ProxyError::BadGateway(format!("{context} could not be sent to upstream: {err}"));
    }

    ProxyError::BadGateway(format!("{context} failed: {err}"))
}

pub(crate) fn classify_upstream_status(status: StatusCode, body: &[u8]) -> ProxyError {
    let body_message = extract_upstream_error_message(body);
    let message = format!("Upstream returned {}: {}", status.as_u16(), body_message);

    match status {
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => {
            ProxyError::UpstreamTimeout(message)
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            ProxyError::UpstreamAuthentication(message)
        }
        StatusCode::PAYLOAD_TOO_LARGE => ProxyError::PayloadTooLarge(message),
        StatusCode::TOO_MANY_REQUESTS => ProxyError::UpstreamRateLimited(message),
        status if status.is_client_error() => ProxyError::UpstreamBadRequest(message),
        status if status.is_server_error() => ProxyError::UpstreamService(message),
        _ => ProxyError::BadGateway(message),
    }
}

fn extract_upstream_error_message(body: &[u8]) -> String {
    if body.is_empty() {
        return "empty upstream error body".to_string();
    }

    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
        if let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
        {
            return truncate_message(message);
        }

        if let Some(message) = value.get("message").and_then(serde_json::Value::as_str) {
            return truncate_message(message);
        }

        return truncate_message(&value.to_string());
    }

    truncate_message(&String::from_utf8_lossy(body))
}

fn truncate_message(message: &str) -> String {
    const MAX_LEN: usize = 512;
    if message.chars().count() <= MAX_LEN {
        return message.to_string();
    }

    let truncated = message.chars().take(MAX_LEN).collect::<String>();
    format!("{truncated}...")
}

fn is_body_too_large_message(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("length limit exceeded")
        || normalized.contains("body too large")
        || normalized.contains("payload too large")
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

#[cfg(test)]
mod tests {
    use super::{
        ProxyError, classify_request_body_error, classify_upstream_status, protocol_transform_error,
    };
    use axum::response::IntoResponse;
    use reqwest::StatusCode;

    #[test]
    fn classify_request_body_error_maps_length_limit_to_payload_too_large() {
        assert!(matches!(
            classify_request_body_error("length limit exceeded"),
            ProxyError::PayloadTooLarge(_)
        ));
    }

    #[test]
    fn classify_upstream_status_extracts_json_message() {
        let err = classify_upstream_status(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":{"message":"quota exceeded"}}"#,
        );

        match err {
            ProxyError::UpstreamRateLimited(message) => {
                assert!(message.contains("quota exceeded"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn protocol_transform_error_uses_dedicated_code() {
        let response =
            protocol_transform_error("serialize final request body", "boom").into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn key_lifecycle_errors_use_dedicated_status_and_codes() {
        let disabled = ProxyError::KeyDisabled("disabled".to_string()).into_response();
        assert_eq!(disabled.status(), StatusCode::FORBIDDEN);

        let expired = ProxyError::KeyExpired("expired".to_string()).into_response();
        assert_eq!(expired.status(), StatusCode::FORBIDDEN);

        assert_eq!(
            ProxyError::KeyDisabled("disabled".to_string()).to_string(),
            "[api_key_disabled_error] disabled"
        );
        assert_eq!(
            ProxyError::KeyExpired("expired".to_string()).to_string(),
            "[api_key_expired_error] expired"
        );
    }

    #[test]
    fn provider_governance_skip_errors_use_dedicated_codes() {
        let open = ProxyError::ProviderOpenSkipped("open".to_string()).into_response();
        let half_open =
            ProxyError::ProviderHalfOpenProbeInFlight("half-open".to_string()).into_response();

        assert_eq!(open.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(half_open.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            ProxyError::ProviderOpenSkipped("open".to_string()).to_string(),
            "[provider_open_skipped] open"
        );
        assert_eq!(
            ProxyError::ProviderHalfOpenProbeInFlight("half-open".to_string()).to_string(),
            "[provider_half_open_skipped] half-open"
        );
    }

    #[test]
    fn request_patch_conflict_uses_dedicated_code() {
        let response = ProxyError::RequestPatchConflict("conflict".to_string()).into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            ProxyError::RequestPatchConflict("conflict".to_string()).to_string(),
            "[request_patch_conflict_error] conflict"
        );
    }
}
