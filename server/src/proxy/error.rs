use axum::http::HeaderValue;
use reqwest::{Error as ReqwestError, StatusCode};
use std::fmt;

use crate::service::upstream_response::UpstreamHttpErrorKind;

pub(crate) mod fact;
pub(crate) mod response;
pub(crate) mod upstream;
pub(crate) mod visibility;

#[allow(unused_imports)] // C1 exports consumed by the retry producer and HTTP adapter tasks.
pub(crate) use fact::{
    ErrorResponseHints, ExecutionStage, ProxyError, ProxyErrorCode, ProxyLogLevel,
    RetryAfterSeconds,
};
pub(crate) use response::{ProtocolErrorResponseAdapter, RouterRejection};
pub(crate) use upstream::UpstreamErrorPayload;
pub(crate) use visibility::{ResponseVisibility, ResponseVisibilityTracker};

pub(super) fn classify_request_body_error(message: impl Into<String>) -> ProxyError {
    let message = message.into();
    if is_body_too_large_message(&message) {
        let operator_message = format!("Request body exceeds configured size limit: {message}");
        ProxyError::gateway(
            ProxyErrorCode::RequestBodyTooLargeError,
            ExecutionStage::Parse,
            ResponseVisibility::NotVisible,
            Some(operator_message.clone()),
            operator_message,
        )
    } else {
        let operator_message = format!("Failed to read request body: {message}");
        ProxyError::gateway(
            ProxyErrorCode::InvalidRequestError,
            ExecutionStage::Parse,
            ResponseVisibility::NotVisible,
            Some(operator_message.clone()),
            operator_message,
        )
    }
}

pub(crate) fn protocol_transform_error(
    stage: ExecutionStage,
    response_visibility: ResponseVisibility,
    operation: &str,
    err: impl fmt::Display,
) -> ProxyError {
    ProxyError::gateway(
        ProxyErrorCode::ProtocolTransformError,
        stage,
        response_visibility,
        None,
        format!("{operation}: {err}"),
    )
}

pub(crate) fn classify_reqwest_error(
    context: &str,
    err: &ReqwestError,
    stage: ExecutionStage,
    response_visibility: ResponseVisibility,
) -> ProxyError {
    let kind = UpstreamHttpErrorKind::from_reqwest(err);
    let (code, message) = if kind == UpstreamHttpErrorKind::Timeout {
        (
            ProxyErrorCode::UpstreamTimeoutError,
            format!("{context} timed out ({})", kind.as_str()),
        )
    } else if kind == UpstreamHttpErrorKind::Connect {
        (
            ProxyErrorCode::UpstreamConnectError,
            format!(
                "{context} could not connect to upstream ({})",
                kind.as_str()
            ),
        )
    } else if matches!(
        kind,
        UpstreamHttpErrorKind::Body | UpstreamHttpErrorKind::Decode
    ) {
        (
            ProxyErrorCode::UpstreamResponseError,
            format!(
                "{context} failed while reading upstream body ({})",
                kind.as_str()
            ),
        )
    } else if kind == UpstreamHttpErrorKind::Request {
        (
            ProxyErrorCode::UpstreamRequestError,
            format!(
                "{context} could not be sent to upstream ({})",
                kind.as_str()
            ),
        )
    } else if kind == UpstreamHttpErrorKind::Status || stage == ExecutionStage::UpstreamResponse {
        (
            ProxyErrorCode::UpstreamResponseError,
            format!(
                "{context} failed while processing the upstream response ({})",
                kind.as_str()
            ),
        )
    } else {
        (
            ProxyErrorCode::UpstreamRequestError,
            format!("{context} failed ({})", kind.as_str()),
        )
    };

    ProxyError::gateway(code, stage, response_visibility, None, message)
}

#[cfg(test)]
pub(crate) fn classify_upstream_status(
    status: StatusCode,
    content_type: Option<&HeaderValue>,
    body: &[u8],
    limit_bytes: usize,
    response_visibility: ResponseVisibility,
) -> ProxyError {
    classify_upstream_status_captured(
        status,
        content_type,
        &body[..body.len().min(limit_bytes)],
        limit_bytes,
        body.len() > limit_bytes,
        response_visibility,
    )
}

pub(crate) fn classify_upstream_status_captured(
    status: StatusCode,
    content_type: Option<&HeaderValue>,
    captured_body: &[u8],
    limit_bytes: usize,
    truncated: bool,
    response_visibility: ResponseVisibility,
) -> ProxyError {
    let code = match status {
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => {
            ProxyErrorCode::UpstreamTimeoutError
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            ProxyErrorCode::UpstreamAuthenticationError
        }
        StatusCode::PAYLOAD_TOO_LARGE => ProxyErrorCode::UpstreamPayloadTooLargeError,
        StatusCode::TOO_MANY_REQUESTS => ProxyErrorCode::UpstreamRateLimitError,
        status if status.is_client_error() => ProxyErrorCode::UpstreamInvalidRequestError,
        status if status.is_server_error() => ProxyErrorCode::UpstreamServiceError,
        _ => ProxyErrorCode::UpstreamUnexpectedStatusError,
    };
    let payload = UpstreamErrorPayload::from_captured_prefix(
        status,
        content_type,
        captured_body,
        limit_bytes,
        truncated,
    );
    let body_message = summarize_upstream_error_body(captured_body, truncated);

    ProxyError::upstream(
        code,
        ExecutionStage::UpstreamResponse,
        response_visibility,
        payload,
        format!("Upstream returned {}: {body_message}", status.as_u16()),
    )
}

fn summarize_upstream_error_body(body: &[u8], truncated: bool) -> String {
    if body.is_empty() {
        return "empty upstream error body".to_string();
    }

    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
        if value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .is_some()
        {
            return format!(
                "JSON error body with a message field ({} captured bytes{})",
                body.len(),
                if truncated { ", truncated" } else { "" }
            );
        }

        if value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .is_some()
        {
            return format!(
                "JSON error body with a message field ({} captured bytes{})",
                body.len(),
                if truncated { ", truncated" } else { "" }
            );
        }

        return "JSON error body without a message field".to_string();
    }

    if std::str::from_utf8(body).is_ok() {
        format!("text error body ({} bytes)", body.len())
    } else {
        format!("binary error body ({} bytes)", body.len())
    }
}

fn is_body_too_large_message(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("length limit exceeded")
        || normalized.contains("body too large")
        || normalized.contains("payload too large")
}

#[cfg(test)]
mod tests {
    use super::{
        ExecutionStage, ProtocolErrorResponseAdapter, ProxyError, ProxyErrorCode, ProxyLogLevel,
        ResponseVisibility, classify_request_body_error, classify_reqwest_error,
        classify_upstream_status, protocol_transform_error,
    };
    use crate::{proxy::request_context::RequestId, schema::enum_def::DownstreamProtocol};
    use axum::{body::to_bytes, http::HeaderValue};
    use reqwest::StatusCode;
    use tokio::net::TcpListener;

    #[test]
    fn classify_request_body_error_maps_length_limit_to_request_payload_code() {
        let error = classify_request_body_error("length limit exceeded");
        assert_eq!(error.code(), ProxyErrorCode::RequestBodyTooLargeError);
        assert_eq!(error.stage(), ExecutionStage::Parse);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
    }

    #[tokio::test]
    async fn reqwest_error_classification_never_copies_url_or_query() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let marker = "query-secret-marker";
        let reqwest_error = reqwest::Client::new()
            .get(format!("http://{address}/private?api_key={marker}"))
            .send()
            .await
            .expect_err("closed local address must reject the connection");
        assert!(
            reqwest_error
                .url()
                .is_some_and(|url| url.as_str().contains(marker)),
            "fixture must prove the source error carries the sensitive URL"
        );

        let error = classify_reqwest_error(
            "sentinel request",
            &reqwest_error,
            ExecutionStage::Connect,
            ResponseVisibility::NotVisible,
        );
        assert_eq!(error.code(), ProxyErrorCode::UpstreamConnectError);
        assert!(!error.operator_message().contains(marker));
        assert!(!error.operator_message().contains(&address.to_string()));
        assert!(!error.operator_message().contains("api_key"));
    }

    #[tokio::test]
    async fn classify_upstream_status_preserves_json_payload() {
        let content_type = HeaderValue::from_static("application/json");
        let error = classify_upstream_status(
            StatusCode::TOO_MANY_REQUESTS,
            Some(&content_type),
            br#"{"error":{"message":"quota exceeded","type":"provider_quota"}}"#,
            65_536,
            ResponseVisibility::NotVisible,
        );

        assert_eq!(error.code(), ProxyErrorCode::UpstreamRateLimitError);
        assert_eq!(error.stage(), ExecutionStage::UpstreamResponse);
        assert!(
            error
                .operator_message()
                .contains("JSON error body with a message field")
        );
        assert!(!error.operator_message().contains("quota exceeded"));
        assert_eq!(
            error.response_hints().retry_after(),
            None,
            "provider Retry-After is not a local R3.4 producer fact"
        );

        let response =
            ProtocolErrorResponseAdapter::new(DownstreamProtocol::Openai, RequestId::new())
                .proxy_error_response(error);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response should be json");
        assert_eq!(
            body["error"]["message"],
            "Upstream provider rate limited the request."
        );
        assert_eq!(body["upstream_error"]["status"], 429);
        assert_eq!(
            body["upstream_error"]["body"]["error"]["type"],
            "provider_quota"
        );
    }

    #[test]
    fn upstream_operator_summary_never_copies_complete_body_fallbacks() {
        for (body, expected_summary) in [
            (
                br#"{"detail":"provider-secret-json-detail"}"#.as_slice(),
                "JSON error body without a message field",
            ),
            (
                b"provider-secret-text-detail".as_slice(),
                "text error body (27 bytes)",
            ),
            (
                b"\xff\x00provider-secret-binary-detail".as_slice(),
                "binary error body (31 bytes)",
            ),
        ] {
            let error = classify_upstream_status(
                StatusCode::BAD_GATEWAY,
                None,
                body,
                65_536,
                ResponseVisibility::NotVisible,
            );

            assert!(error.operator_message().contains(expected_summary));
            assert!(!error.operator_message().contains("provider-secret"));
        }
    }

    #[test]
    fn protocol_transform_error_uses_fixed_public_message() {
        let error = protocol_transform_error(
            ExecutionStage::Transform,
            ResponseVisibility::NotVisible,
            "serialize final request body",
            "secret detail",
        );

        assert_eq!(error.code(), ProxyErrorCode::ProtocolTransformError);
        assert_eq!(
            error.client_payload().public_message(),
            "The gateway could not transform the request or response."
        );
        assert!(error.operator_message().contains("secret detail"));
    }

    #[test]
    fn key_lifecycle_errors_use_dedicated_status_and_codes() {
        for (code, expected_message) in [
            (
                ProxyErrorCode::ApiKeyDisabledError,
                "The API key is disabled.",
            ),
            (
                ProxyErrorCode::ApiKeyExpiredError,
                "The API key has expired.",
            ),
        ] {
            let error = ProxyError::gateway(
                code,
                ExecutionStage::Authentication,
                ResponseVisibility::NotVisible,
                None,
                "operator detail",
            );
            assert_eq!(error.status_code(), StatusCode::FORBIDDEN);
            assert_eq!(error.client_payload().public_message(), expected_message);
        }
    }

    #[test]
    fn provider_governance_errors_use_stable_codes() {
        for code in [
            ProxyErrorCode::ProviderCircuitOpenError,
            ProxyErrorCode::ProviderHalfOpenProbeInFlightError,
        ] {
            let error = ProxyError::gateway(
                code,
                ExecutionStage::Governance,
                ResponseVisibility::NotVisible,
                None,
                "operator detail",
            );
            assert_eq!(error.status_code(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(error.operator_log_level(), ProxyLogLevel::Warn);
        }
    }

    #[test]
    fn request_patch_conflict_uses_dedicated_code() {
        let error = ProxyError::gateway(
            ProxyErrorCode::RequestPatchConflictError,
            ExecutionStage::Patch,
            ResponseVisibility::NotVisible,
            None,
            "conflict",
        );

        assert_eq!(error.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.code().as_str(), "request_patch_conflict_error");
    }

    #[test]
    fn operator_log_level_is_derived_from_stable_code() {
        for code in ProxyErrorCode::ALL {
            let level = code.operator_log_level();
            assert!(matches!(
                level,
                ProxyLogLevel::Debug | ProxyLogLevel::Warn | ProxyLogLevel::Error
            ));
        }
    }
}
