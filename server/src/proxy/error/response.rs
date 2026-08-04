use axum::{
    Json,
    body::Body,
    http::{
        HeaderMap, HeaderName, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER, WWW_AUTHENTICATE},
    },
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{
    proxy::request_context::{RequestId, X_REQUEST_ID},
    schema::enum_def::DownstreamProtocol,
};

use super::{ProxyError, ProxyErrorCode, upstream::UpstreamErrorPayload};

const ANTHROPIC_REQUEST_ID: HeaderName = HeaderName::from_static("request-id");
const X_CONTENT_TYPE_OPTIONS: HeaderName = HeaderName::from_static("x-content-type-options");

#[derive(Debug)]
pub(crate) struct ProtocolErrorResponseAdapter {
    downstream_protocol: DownstreamProtocol,
    request_id: RequestId,
}

impl ProtocolErrorResponseAdapter {
    pub(crate) const fn new(
        downstream_protocol: DownstreamProtocol,
        request_id: RequestId,
    ) -> Self {
        Self {
            downstream_protocol,
            request_id,
        }
    }

    pub(crate) fn proxy_error_response(&self, error: ProxyError) -> Response<Body> {
        crate::logging::log_proxy_error_event(
            "proxy.error_response",
            Some(self.request_id.as_str()),
            None,
            &error,
        );
        let fact = ProtocolResponseFact {
            status: error.status_code(),
            code: error.code().as_str(),
            category: proxy_error_category(error.code(), self.downstream_protocol),
            message: error.public_message(),
            upstream_error: error.upstream_error(),
        };
        self.render(fact, error.response_hints().retry_after(), HeaderMap::new())
    }

    pub(crate) fn router_rejection_response(&self, rejection: RouterRejection) -> Response<Body> {
        self.router_rejection_response_with_headers(rejection, HeaderMap::new())
    }

    pub(crate) fn router_rejection_response_with_headers(
        &self,
        rejection: RouterRejection,
        standard_headers: HeaderMap,
    ) -> Response<Body> {
        crate::debug_event!(
            "proxy.router_rejection_response",
            request_id = self.request_id.as_str(),
            downstream_protocol = downstream_protocol_name(self.downstream_protocol),
            error_code = rejection.code(),
            error_http_status = rejection.status_code().as_u16(),
        );
        let fact = ProtocolResponseFact {
            status: rejection.status_code(),
            code: rejection.code(),
            category: rejection.category(self.downstream_protocol),
            message: rejection.public_message(),
            upstream_error: None,
        };
        self.render(fact, None, standard_headers)
    }

    fn render(
        &self,
        fact: ProtocolResponseFact<'_>,
        retry_after: Option<super::RetryAfterSeconds>,
        standard_headers: HeaderMap,
    ) -> Response<Body> {
        let envelope = match self.downstream_protocol {
            DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
                ProtocolEnvelope::OpenAi(OpenAiEnvelope {
                    error: OpenAiError {
                        message: fact.message,
                        error_type: fact.category,
                        param: None,
                        code: fact.code,
                    },
                    upstream_error: fact.upstream_error,
                })
            }
            DownstreamProtocol::Anthropic => ProtocolEnvelope::Anthropic(AnthropicEnvelope {
                response_type: "error",
                error: AnthropicError {
                    error_type: fact.category,
                    message: fact.message,
                    code: fact.code,
                },
                request_id: self.request_id.as_str(),
                upstream_error: fact.upstream_error,
            }),
            DownstreamProtocol::Gemini => ProtocolEnvelope::Gemini(GeminiEnvelope {
                error: GeminiError {
                    code: fact.status.as_u16(),
                    message: fact.message,
                    status: fact.category,
                    details: [GoogleRpcErrorInfo {
                        detail_type: "type.googleapis.com/google.rpc.ErrorInfo",
                        reason: fact.code.to_ascii_uppercase(),
                        domain: "cyder.gateway",
                        metadata: GoogleRpcErrorInfoMetadata {
                            request_id: self.request_id.as_str(),
                            cyder_code: fact.code,
                        },
                    }],
                },
                upstream_error: fact.upstream_error,
            }),
        };

        let mut response = (fact.status, Json(envelope)).into_response();
        response.headers_mut().extend(standard_headers);
        self.apply_headers(response.headers_mut(), fact.status, retry_after);
        response
    }

    fn apply_headers(
        &self,
        headers: &mut HeaderMap,
        status: StatusCode,
        retry_after: Option<super::RetryAfterSeconds>,
    ) {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        headers.insert(
            &X_REQUEST_ID,
            HeaderValue::from_str(self.request_id.as_str())
                .expect("canonical request id must be a valid header value"),
        );

        if self.downstream_protocol == DownstreamProtocol::Anthropic {
            headers.insert(
                ANTHROPIC_REQUEST_ID,
                HeaderValue::from_str(self.request_id.as_str())
                    .expect("canonical request id must be a valid header value"),
            );
        }

        if status == StatusCode::UNAUTHORIZED
            && matches!(
                self.downstream_protocol,
                DownstreamProtocol::Openai
                    | DownstreamProtocol::Responses
                    | DownstreamProtocol::Anthropic
            )
        {
            headers.insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }

        if let Some(retry_after) = retry_after {
            headers.insert(
                RETRY_AFTER,
                HeaderValue::from_str(&retry_after.get().to_string())
                    .expect("non-zero retry-after seconds must be a valid header value"),
            );
        }
    }
}

struct ProtocolResponseFact<'a> {
    status: StatusCode,
    code: &'a str,
    category: &'a str,
    message: &'a str,
    upstream_error: Option<&'a UpstreamErrorPayload>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ProtocolEnvelope<'a> {
    OpenAi(OpenAiEnvelope<'a>),
    Anthropic(AnthropicEnvelope<'a>),
    Gemini(GeminiEnvelope<'a>),
}

#[derive(Serialize)]
struct OpenAiEnvelope<'a> {
    error: OpenAiError<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_error: Option<&'a UpstreamErrorPayload>,
}

#[derive(Serialize)]
struct OpenAiError<'a> {
    message: &'a str,
    #[serde(rename = "type")]
    error_type: &'a str,
    param: Option<&'a str>,
    code: &'a str,
}

#[derive(Serialize)]
struct AnthropicEnvelope<'a> {
    #[serde(rename = "type")]
    response_type: &'static str,
    error: AnthropicError<'a>,
    request_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_error: Option<&'a UpstreamErrorPayload>,
}

#[derive(Serialize)]
struct AnthropicError<'a> {
    #[serde(rename = "type")]
    error_type: &'a str,
    message: &'a str,
    code: &'a str,
}

#[derive(Serialize)]
struct GeminiEnvelope<'a> {
    error: GeminiError<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_error: Option<&'a UpstreamErrorPayload>,
}

#[derive(Serialize)]
struct GeminiError<'a> {
    code: u16,
    message: &'a str,
    status: &'a str,
    details: [GoogleRpcErrorInfo<'a>; 1],
}

#[derive(Serialize)]
struct GoogleRpcErrorInfo<'a> {
    #[serde(rename = "@type")]
    detail_type: &'static str,
    reason: String,
    domain: &'static str,
    metadata: GoogleRpcErrorInfoMetadata<'a>,
}

#[derive(Serialize)]
struct GoogleRpcErrorInfoMetadata<'a> {
    request_id: &'a str,
    cyder_code: &'a str,
}

const fn downstream_protocol_name(protocol: DownstreamProtocol) -> &'static str {
    match protocol {
        DownstreamProtocol::Openai => "openai",
        DownstreamProtocol::Responses => "responses",
        DownstreamProtocol::Anthropic => "anthropic",
        DownstreamProtocol::Gemini => "gemini",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouterRejection {
    RouteNotFound,
    MethodNotAllowed,
}

impl RouterRejection {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 2] = [Self::RouteNotFound, Self::MethodNotAllowed];

    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::RouteNotFound => "route_not_found_error",
            Self::MethodNotAllowed => "method_not_allowed_error",
        }
    }

    pub(crate) const fn status_code(self) -> StatusCode {
        match self {
            Self::RouteNotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
        }
    }

    pub(crate) const fn public_message(self) -> &'static str {
        match self {
            Self::RouteNotFound => "The requested gateway route was not found.",
            Self::MethodNotAllowed => {
                "The requested HTTP method is not allowed for this gateway route."
            }
        }
    }

    pub(crate) const fn category(self, downstream_protocol: DownstreamProtocol) -> &'static str {
        match downstream_protocol {
            DownstreamProtocol::Openai => match self {
                Self::RouteNotFound | Self::MethodNotAllowed => "invalid_request_error",
            },
            DownstreamProtocol::Responses => match self {
                Self::RouteNotFound | Self::MethodNotAllowed => "invalid_request_error",
            },
            DownstreamProtocol::Anthropic => match self {
                Self::RouteNotFound => "not_found_error",
                Self::MethodNotAllowed => "invalid_request_error",
            },
            DownstreamProtocol::Gemini => match self {
                Self::RouteNotFound => "NOT_FOUND",
                Self::MethodNotAllowed => "UNIMPLEMENTED",
            },
        }
    }
}

pub(crate) const fn proxy_error_category(
    code: ProxyErrorCode,
    downstream_protocol: DownstreamProtocol,
) -> &'static str {
    match downstream_protocol {
        DownstreamProtocol::Openai => openai_error_type(code),
        DownstreamProtocol::Responses => openai_error_type(code),
        DownstreamProtocol::Anthropic => anthropic_error_type(code),
        DownstreamProtocol::Gemini => gemini_error_status(code),
    }
}

const fn openai_error_type(code: ProxyErrorCode) -> &'static str {
    match code {
        ProxyErrorCode::AuthenticationError => "authentication_error",
        ProxyErrorCode::ApiKeyDisabledError
        | ProxyErrorCode::ApiKeyExpiredError
        | ProxyErrorCode::PermissionError
        | ProxyErrorCode::BudgetExhaustedError => "permission_error",
        ProxyErrorCode::InvalidRequestError
        | ProxyErrorCode::UnsupportedCapabilityError
        | ProxyErrorCode::UpstreamInvalidRequestError
        | ProxyErrorCode::RequestBodyTooLargeError
        | ProxyErrorCode::UpstreamPayloadTooLargeError
        | ProxyErrorCode::ClientCancelledError => "invalid_request_error",
        ProxyErrorCode::RateLimitError
        | ProxyErrorCode::ConcurrencyLimitError
        | ProxyErrorCode::QuotaExhaustedError
        | ProxyErrorCode::UpstreamRateLimitError => "rate_limit_error",
        ProxyErrorCode::RequestPatchConflictError
        | ProxyErrorCode::ServerError
        | ProxyErrorCode::ProtocolTransformError
        | ProxyErrorCode::ProviderConfigurationError
        | ProxyErrorCode::DownstreamSendError
        | ProxyErrorCode::UpstreamAuthenticationError
        | ProxyErrorCode::UpstreamUnexpectedStatusError
        | ProxyErrorCode::UpstreamConnectError
        | ProxyErrorCode::UpstreamRequestError
        | ProxyErrorCode::UpstreamResponseError
        | ProxyErrorCode::ProviderCircuitOpenError
        | ProxyErrorCode::ProviderHalfOpenProbeInFlightError
        | ProxyErrorCode::UpstreamServiceError
        | ProxyErrorCode::UpstreamTimeoutError => "server_error",
    }
}

const fn anthropic_error_type(code: ProxyErrorCode) -> &'static str {
    match code {
        ProxyErrorCode::AuthenticationError => "authentication_error",
        ProxyErrorCode::ApiKeyDisabledError
        | ProxyErrorCode::ApiKeyExpiredError
        | ProxyErrorCode::PermissionError
        | ProxyErrorCode::BudgetExhaustedError => "permission_error",
        ProxyErrorCode::InvalidRequestError
        | ProxyErrorCode::UnsupportedCapabilityError
        | ProxyErrorCode::UpstreamInvalidRequestError
        | ProxyErrorCode::ClientCancelledError => "invalid_request_error",
        ProxyErrorCode::RequestBodyTooLargeError | ProxyErrorCode::UpstreamPayloadTooLargeError => {
            "request_too_large"
        }
        ProxyErrorCode::RateLimitError
        | ProxyErrorCode::ConcurrencyLimitError
        | ProxyErrorCode::QuotaExhaustedError
        | ProxyErrorCode::UpstreamRateLimitError => "rate_limit_error",
        ProxyErrorCode::RequestPatchConflictError
        | ProxyErrorCode::ServerError
        | ProxyErrorCode::ProtocolTransformError
        | ProxyErrorCode::ProviderConfigurationError
        | ProxyErrorCode::DownstreamSendError
        | ProxyErrorCode::UpstreamAuthenticationError
        | ProxyErrorCode::UpstreamUnexpectedStatusError
        | ProxyErrorCode::UpstreamConnectError
        | ProxyErrorCode::UpstreamRequestError
        | ProxyErrorCode::UpstreamResponseError
        | ProxyErrorCode::ProviderCircuitOpenError
        | ProxyErrorCode::ProviderHalfOpenProbeInFlightError
        | ProxyErrorCode::UpstreamServiceError => "api_error",
        ProxyErrorCode::UpstreamTimeoutError => "timeout_error",
    }
}

const fn gemini_error_status(code: ProxyErrorCode) -> &'static str {
    match code {
        ProxyErrorCode::AuthenticationError => "UNAUTHENTICATED",
        ProxyErrorCode::ApiKeyDisabledError
        | ProxyErrorCode::ApiKeyExpiredError
        | ProxyErrorCode::PermissionError
        | ProxyErrorCode::BudgetExhaustedError => "PERMISSION_DENIED",
        ProxyErrorCode::InvalidRequestError
        | ProxyErrorCode::UnsupportedCapabilityError
        | ProxyErrorCode::UpstreamInvalidRequestError
        | ProxyErrorCode::RequestBodyTooLargeError
        | ProxyErrorCode::UpstreamPayloadTooLargeError => "INVALID_ARGUMENT",
        ProxyErrorCode::RateLimitError
        | ProxyErrorCode::ConcurrencyLimitError
        | ProxyErrorCode::QuotaExhaustedError
        | ProxyErrorCode::UpstreamRateLimitError => "RESOURCE_EXHAUSTED",
        ProxyErrorCode::ClientCancelledError => "CANCELLED",
        ProxyErrorCode::RequestPatchConflictError
        | ProxyErrorCode::ServerError
        | ProxyErrorCode::ProtocolTransformError
        | ProxyErrorCode::ProviderConfigurationError
        | ProxyErrorCode::DownstreamSendError => "INTERNAL",
        ProxyErrorCode::UpstreamAuthenticationError
        | ProxyErrorCode::UpstreamUnexpectedStatusError
        | ProxyErrorCode::UpstreamConnectError
        | ProxyErrorCode::UpstreamRequestError
        | ProxyErrorCode::UpstreamResponseError => "UNKNOWN",
        ProxyErrorCode::ProviderCircuitOpenError
        | ProxyErrorCode::ProviderHalfOpenProbeInFlightError
        | ProxyErrorCode::UpstreamServiceError => "UNAVAILABLE",
        ProxyErrorCode::UpstreamTimeoutError => "DEADLINE_EXCEEDED",
    }
}

#[cfg(test)]
mod tests {
    use super::{ProtocolErrorResponseAdapter, RouterRejection, proxy_error_category};
    use crate::{
        proxy::{
            ResponseVisibility,
            error::{
                ExecutionStage, ProxyError, ProxyErrorCode, TimeoutPhase, UpstreamErrorPayload,
            },
            request_context::{RequestId, X_REQUEST_ID},
        },
        schema::enum_def::DownstreamProtocol,
    };
    use axum::{
        body::to_bytes,
        http::{
            HeaderMap, HeaderValue, StatusCode,
            header::{ALLOW, CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER, WWW_AUTHENTICATE},
        },
    };
    use serde_json::{Value, json};
    use std::time::Duration;

    async fn response_body(response: axum::response::Response) -> serde_json::Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("protocol error response should be JSON")
    }

    fn gateway_error(code: ProxyErrorCode) -> ProxyError {
        if code == ProxyErrorCode::UpstreamTimeoutError {
            ProxyError::upstream_timeout(
                TimeoutPhase::Total,
                ExecutionStage::Governance,
                ResponseVisibility::NotVisible,
                "test operator detail",
            )
        } else {
            ProxyError::gateway(
                code,
                ExecutionStage::Governance,
                ResponseVisibility::NotVisible,
                None,
                "test operator detail",
            )
        }
    }

    #[derive(Clone, Copy)]
    enum UpstreamFixture {
        Json,
        Text,
        Base64,
        Truncated,
        ServiceJson,
        UnexpectedText,
        TimeoutJson,
    }

    impl UpstreamFixture {
        fn payload(self) -> UpstreamErrorPayload {
            match self {
                Self::Json => {
                    capture_upstream(400, Some("application/json"), br#"{"e":"bad"}"#, 32)
                }
                Self::Text => capture_upstream(401, Some("text/plain"), b"denied", 32),
                Self::Base64 => capture_upstream(
                    429,
                    Some("application/octet-stream"),
                    &[0xff, 0x00, 0x01],
                    32,
                ),
                Self::Truncated => {
                    capture_upstream(413, Some("application/json"), b"abcdefghij", 4)
                }
                Self::ServiceJson => capture_upstream(
                    503,
                    Some("application/problem+json"),
                    br#"{"down":true}"#,
                    32,
                ),
                Self::UnexpectedText => {
                    capture_upstream(418, Some("text/plain; charset=utf-8"), b"teapot", 32)
                }
                Self::TimeoutJson => {
                    capture_upstream(504, Some("application/json"), br#"{"timeout":true}"#, 32)
                }
            }
        }

        fn expected_value(self) -> Value {
            match self {
                Self::Json => json!({
                    "status": 400,
                    "content_type": "application/json",
                    "body": { "e": "bad" },
                    "truncated": false,
                    "captured_bytes": 11,
                    "limit_bytes": 32
                }),
                Self::Text => json!({
                    "status": 401,
                    "content_type": "text/plain",
                    "body_text": "denied",
                    "truncated": false,
                    "captured_bytes": 6,
                    "limit_bytes": 32
                }),
                Self::Base64 => json!({
                    "status": 429,
                    "content_type": "application/octet-stream",
                    "body_base64": "/wAB",
                    "body_encoding": "base64",
                    "truncated": false,
                    "captured_bytes": 3,
                    "limit_bytes": 32
                }),
                Self::Truncated => json!({
                    "status": 413,
                    "content_type": "application/json",
                    "body_text": "abcd",
                    "truncated": true,
                    "captured_bytes": 4,
                    "limit_bytes": 4,
                    "notice": "Upstream error body was truncated by the gateway."
                }),
                Self::ServiceJson => json!({
                    "status": 503,
                    "content_type": "application/problem+json",
                    "body": { "down": true },
                    "truncated": false,
                    "captured_bytes": 13,
                    "limit_bytes": 32
                }),
                Self::UnexpectedText => json!({
                    "status": 418,
                    "content_type": "text/plain; charset=utf-8",
                    "body_text": "teapot",
                    "truncated": false,
                    "captured_bytes": 6,
                    "limit_bytes": 32
                }),
                Self::TimeoutJson => json!({
                    "status": 504,
                    "content_type": "application/json",
                    "body": { "timeout": true },
                    "truncated": false,
                    "captured_bytes": 16,
                    "limit_bytes": 32
                }),
            }
        }
    }

    fn capture_upstream(
        status: u16,
        content_type: Option<&'static str>,
        body: &[u8],
        limit: usize,
    ) -> UpstreamErrorPayload {
        let content_type = content_type.map(HeaderValue::from_static);
        UpstreamErrorPayload::capture(
            StatusCode::from_u16(status).expect("fixture status should be valid"),
            content_type.as_ref(),
            body,
            limit,
        )
    }

    struct ExpectedProxyContract {
        code: ProxyErrorCode,
        stable_code: &'static str,
        status: u16,
        message: &'static str,
        categories: [&'static str; 4],
        upstream: Option<UpstreamFixture>,
        retry_after: Option<&'static str>,
    }

    const EXPECTED_PROXY_CONTRACTS: [ExpectedProxyContract; 29] = [
        ExpectedProxyContract {
            code: ProxyErrorCode::AuthenticationError,
            stable_code: "authentication_error",
            status: 401,
            message: "Authentication failed.",
            categories: [
                "authentication_error",
                "authentication_error",
                "authentication_error",
                "UNAUTHENTICATED",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::ApiKeyDisabledError,
            stable_code: "api_key_disabled_error",
            status: 403,
            message: "The API key is disabled.",
            categories: [
                "permission_error",
                "permission_error",
                "permission_error",
                "PERMISSION_DENIED",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::ApiKeyExpiredError,
            stable_code: "api_key_expired_error",
            status: 403,
            message: "The API key has expired.",
            categories: [
                "permission_error",
                "permission_error",
                "permission_error",
                "PERMISSION_DENIED",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::InvalidRequestError,
            stable_code: "invalid_request_error",
            status: 400,
            message: "The request is invalid.",
            categories: [
                "invalid_request_error",
                "invalid_request_error",
                "invalid_request_error",
                "INVALID_ARGUMENT",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::PermissionError,
            stable_code: "permission_error",
            status: 403,
            message: "The request is not permitted.",
            categories: [
                "permission_error",
                "permission_error",
                "permission_error",
                "PERMISSION_DENIED",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::RequestBodyTooLargeError,
            stable_code: "request_body_too_large_error",
            status: 413,
            message: "The request body is too large.",
            categories: [
                "invalid_request_error",
                "invalid_request_error",
                "request_too_large",
                "INVALID_ARGUMENT",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UnsupportedCapabilityError,
            stable_code: "unsupported_capability_error",
            status: 400,
            message: "The selected target does not support the requested capability.",
            categories: [
                "invalid_request_error",
                "invalid_request_error",
                "invalid_request_error",
                "INVALID_ARGUMENT",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::RequestPatchConflictError,
            stable_code: "request_patch_conflict_error",
            status: 500,
            message: "The gateway request patch configuration is conflicting.",
            categories: ["server_error", "server_error", "api_error", "INTERNAL"],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::RateLimitError,
            stable_code: "rate_limit_error",
            status: 429,
            message: "The API key rate limit was exceeded.",
            categories: [
                "rate_limit_error",
                "rate_limit_error",
                "rate_limit_error",
                "RESOURCE_EXHAUSTED",
            ],
            upstream: None,
            retry_after: Some("2"),
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::ConcurrencyLimitError,
            stable_code: "concurrency_limit_error",
            status: 429,
            message: "The API key concurrency limit was exceeded.",
            categories: [
                "rate_limit_error",
                "rate_limit_error",
                "rate_limit_error",
                "RESOURCE_EXHAUSTED",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::QuotaExhaustedError,
            stable_code: "quota_exhausted_error",
            status: 429,
            message: "The API key quota was exhausted.",
            categories: [
                "rate_limit_error",
                "rate_limit_error",
                "rate_limit_error",
                "RESOURCE_EXHAUSTED",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::BudgetExhaustedError,
            stable_code: "budget_exhausted_error",
            status: 403,
            message: "The API key budget was exhausted.",
            categories: [
                "permission_error",
                "permission_error",
                "permission_error",
                "PERMISSION_DENIED",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::ProviderCircuitOpenError,
            stable_code: "provider_circuit_open_error",
            status: 503,
            message: "The upstream provider is temporarily unavailable.",
            categories: ["server_error", "server_error", "api_error", "UNAVAILABLE"],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::ProviderHalfOpenProbeInFlightError,
            stable_code: "provider_half_open_probe_in_flight_error",
            status: 503,
            message: "The upstream provider probe is already in progress.",
            categories: ["server_error", "server_error", "api_error", "UNAVAILABLE"],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::ClientCancelledError,
            stable_code: "client_cancelled_error",
            status: 499,
            message: "The client cancelled the request.",
            categories: [
                "invalid_request_error",
                "invalid_request_error",
                "invalid_request_error",
                "CANCELLED",
            ],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::ServerError,
            stable_code: "server_error",
            status: 500,
            message: "The gateway encountered an internal error.",
            categories: ["server_error", "server_error", "api_error", "INTERNAL"],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::ProtocolTransformError,
            stable_code: "protocol_transform_error",
            status: 500,
            message: "The gateway could not transform the request or response.",
            categories: ["server_error", "server_error", "api_error", "INTERNAL"],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::ProviderConfigurationError,
            stable_code: "provider_configuration_error",
            status: 500,
            message: "The gateway provider configuration is invalid.",
            categories: ["server_error", "server_error", "api_error", "INTERNAL"],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::DownstreamSendError,
            stable_code: "downstream_send_error",
            status: 500,
            message: "The gateway could not construct the downstream response.",
            categories: ["server_error", "server_error", "api_error", "INTERNAL"],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamInvalidRequestError,
            stable_code: "upstream_invalid_request_error",
            status: 400,
            message: "Upstream provider rejected the request.",
            categories: [
                "invalid_request_error",
                "invalid_request_error",
                "invalid_request_error",
                "INVALID_ARGUMENT",
            ],
            upstream: Some(UpstreamFixture::Json),
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamAuthenticationError,
            stable_code: "upstream_authentication_error",
            status: 502,
            message: "Upstream provider rejected its credentials.",
            categories: ["server_error", "server_error", "api_error", "UNKNOWN"],
            upstream: Some(UpstreamFixture::Text),
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamRateLimitError,
            stable_code: "upstream_rate_limit_error",
            status: 429,
            message: "Upstream provider rate limited the request.",
            categories: [
                "rate_limit_error",
                "rate_limit_error",
                "rate_limit_error",
                "RESOURCE_EXHAUSTED",
            ],
            upstream: Some(UpstreamFixture::Base64),
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamPayloadTooLargeError,
            stable_code: "upstream_payload_too_large_error",
            status: 413,
            message: "Upstream provider rejected the request payload as too large.",
            categories: [
                "invalid_request_error",
                "invalid_request_error",
                "request_too_large",
                "INVALID_ARGUMENT",
            ],
            upstream: Some(UpstreamFixture::Truncated),
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamTimeoutError,
            stable_code: "upstream_timeout_error",
            status: 504,
            message: "Upstream provider timed out.",
            categories: [
                "server_error",
                "server_error",
                "timeout_error",
                "DEADLINE_EXCEEDED",
            ],
            upstream: Some(UpstreamFixture::TimeoutJson),
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamServiceError,
            stable_code: "upstream_service_error",
            status: 503,
            message: "Upstream provider service failed.",
            categories: ["server_error", "server_error", "api_error", "UNAVAILABLE"],
            upstream: Some(UpstreamFixture::ServiceJson),
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamUnexpectedStatusError,
            stable_code: "upstream_unexpected_status_error",
            status: 502,
            message: "Upstream provider returned an unexpected HTTP status.",
            categories: ["server_error", "server_error", "api_error", "UNKNOWN"],
            upstream: Some(UpstreamFixture::UnexpectedText),
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamConnectError,
            stable_code: "upstream_connect_error",
            status: 502,
            message: "The gateway could not connect to the upstream provider.",
            categories: ["server_error", "server_error", "api_error", "UNKNOWN"],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamRequestError,
            stable_code: "upstream_request_error",
            status: 502,
            message: "The gateway could not send the request to the upstream provider.",
            categories: ["server_error", "server_error", "api_error", "UNKNOWN"],
            upstream: None,
            retry_after: None,
        },
        ExpectedProxyContract {
            code: ProxyErrorCode::UpstreamResponseError,
            stable_code: "upstream_response_error",
            status: 502,
            message: "The gateway could not read a valid response from the upstream provider.",
            categories: ["server_error", "server_error", "api_error", "UNKNOWN"],
            upstream: None,
            retry_after: None,
        },
    ];

    #[derive(Clone, Copy)]
    struct ExpectedRouterContract {
        rejection: RouterRejection,
        stable_code: &'static str,
        status: u16,
        message: &'static str,
        categories: [&'static str; 4],
        allow: Option<&'static str>,
    }

    const EXPECTED_ROUTER_CONTRACTS: [ExpectedRouterContract; 2] = [
        ExpectedRouterContract {
            rejection: RouterRejection::RouteNotFound,
            stable_code: "route_not_found_error",
            status: 404,
            message: "The requested gateway route was not found.",
            categories: [
                "invalid_request_error",
                "invalid_request_error",
                "not_found_error",
                "NOT_FOUND",
            ],
            allow: None,
        },
        ExpectedRouterContract {
            rejection: RouterRejection::MethodNotAllowed,
            stable_code: "method_not_allowed_error",
            status: 405,
            message: "The requested HTTP method is not allowed for this gateway route.",
            categories: [
                "invalid_request_error",
                "invalid_request_error",
                "invalid_request_error",
                "UNIMPLEMENTED",
            ],
            allow: Some("POST"),
        },
    ];

    fn protocol_category<'a>(
        categories: &'a [&'static str; 4],
        protocol: DownstreamProtocol,
    ) -> &'a str {
        match protocol {
            DownstreamProtocol::Openai => categories[0],
            DownstreamProtocol::Responses => categories[1],
            DownstreamProtocol::Anthropic => categories[2],
            DownstreamProtocol::Gemini => categories[3],
        }
    }

    fn synthetic_error(expected: &ExpectedProxyContract) -> ProxyError {
        let error = match expected.upstream {
            Some(fixture) => ProxyError::upstream(
                expected.code,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
                fixture.payload(),
                "synthetic upstream contract fixture",
            ),
            None => ProxyError::gateway(
                expected.code,
                ExecutionStage::Governance,
                ResponseVisibility::NotVisible,
                None,
                "synthetic gateway contract fixture",
            ),
        };
        if expected.retry_after.is_some() {
            error.with_retry_after(Duration::from_millis(1_001))
        } else {
            error
        }
    }

    fn expected_envelope(
        protocol: DownstreamProtocol,
        status: u16,
        stable_code: &str,
        category: &str,
        message: &str,
        request_id: &str,
        upstream_error: Option<Value>,
    ) -> Value {
        let mut envelope = match protocol {
            DownstreamProtocol::Openai => json!({
                "error": {
                    "message": message,
                    "type": category,
                    "param": null,
                    "code": stable_code
                }
            }),
            DownstreamProtocol::Responses => json!({
                "error": {
                    "message": message,
                    "type": category,
                    "param": null,
                    "code": stable_code
                }
            }),
            DownstreamProtocol::Anthropic => json!({
                "type": "error",
                "error": {
                    "type": category,
                    "message": message,
                    "code": stable_code
                },
                "request_id": request_id
            }),
            DownstreamProtocol::Gemini => json!({
                "error": {
                    "code": status,
                    "message": message,
                    "status": category,
                    "details": [{
                        "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                        "reason": stable_code.to_ascii_uppercase(),
                        "domain": "cyder.gateway",
                        "metadata": {
                            "request_id": request_id,
                            "cyder_code": stable_code
                        }
                    }]
                }
            }),
        };
        if let Some(upstream_error) = upstream_error {
            envelope
                .as_object_mut()
                .expect("expected envelope should be an object")
                .insert("upstream_error".to_string(), upstream_error);
        }
        envelope
    }

    async fn assert_exact_contract(
        response: axum::response::Response,
        protocol: DownstreamProtocol,
        status: u16,
        stable_code: &str,
        category: &str,
        message: &str,
        request_id: &str,
        upstream_error: Option<Value>,
        retry_after: Option<&str>,
        allow: Option<&str>,
    ) {
        assert_eq!(response.status().as_u16(), status, "{stable_code}");
        let headers = response.headers();
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
        assert_eq!(headers.get(CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(headers.get(&X_REQUEST_ID).unwrap(), request_id);
        assert_eq!(
            headers
                .get("request-id")
                .map(|value| value.to_str().expect("request-id should be text")),
            (protocol == DownstreamProtocol::Anthropic).then_some(request_id)
        );
        assert_eq!(
            headers
                .get(WWW_AUTHENTICATE)
                .map(|value| value.to_str().expect("challenge should be text")),
            (status == 401 && protocol != DownstreamProtocol::Gemini).then_some("Bearer")
        );
        assert_eq!(
            headers
                .get(RETRY_AFTER)
                .map(|value| value.to_str().expect("retry-after should be text")),
            retry_after
        );
        assert_eq!(
            headers
                .get(ALLOW)
                .map(|value| value.to_str().expect("allow should be text")),
            allow
        );

        let parsed_request_id =
            uuid::Uuid::parse_str(request_id).expect("request id should be a UUID");
        assert_eq!(parsed_request_id.get_version(), Some(uuid::Version::Random));
        assert_eq!(parsed_request_id.get_variant(), uuid::Variant::RFC4122);

        let body = response_body(response).await;
        assert!(body.get("code").is_none(), "old top-level code leaked");
        assert!(
            body.get("message").is_none(),
            "old top-level message leaked"
        );
        if protocol == DownstreamProtocol::Gemini {
            let reason = body["error"]["details"][0]["reason"]
                .as_str()
                .expect("Gemini ErrorInfo.reason should be text");
            assert!(
                reason.len() <= 63,
                "Gemini ErrorInfo.reason must not exceed 63 ASCII characters: {reason}"
            );
            assert!(
                reason
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_uppercase()),
                "Gemini ErrorInfo.reason must start with an ASCII uppercase letter: {reason}"
            );
            assert!(
                reason
                    .chars()
                    .all(|character| character.is_ascii_uppercase()
                        || character.is_ascii_digit()
                        || character == '_'),
                "Gemini ErrorInfo.reason must use UPPER_SNAKE_CASE: {reason}"
            );
            assert!(
                reason.chars().last().is_some_and(
                    |character| character.is_ascii_uppercase() || character.is_ascii_digit()
                ),
                "Gemini ErrorInfo.reason must end with an ASCII uppercase letter or digit: {reason}"
            );
            assert_eq!(
                body["error"]["details"][0]["metadata"]["cyder_code"], stable_code,
                "Gemini must preserve the exact Cyder stable code in ErrorInfo.metadata"
            );
        }
        assert_eq!(
            body,
            expected_envelope(
                protocol,
                status,
                stable_code,
                category,
                message,
                request_id,
                upstream_error,
            ),
            "body mismatch for {protocol:?}/{stable_code}"
        );
    }

    #[tokio::test]
    async fn protocol_error_contracts_cover_all_116_proxy_and_8_router_combinations() {
        assert_eq!(EXPECTED_PROXY_CONTRACTS.len(), ProxyErrorCode::ALL.len());
        assert_eq!(EXPECTED_ROUTER_CONTRACTS.len(), RouterRejection::ALL.len());
        assert_eq!(DownstreamProtocol::ALL.len(), 4);

        let mut proxy_combinations = 0;
        for (expected, actual_code) in EXPECTED_PROXY_CONTRACTS.iter().zip(ProxyErrorCode::ALL) {
            assert_eq!(expected.code, actual_code, "expected table order drifted");
            for protocol in DownstreamProtocol::ALL {
                let request_id = RequestId::new();
                let request_id_value = request_id.as_str().to_string();
                let response = ProtocolErrorResponseAdapter::new(protocol, request_id)
                    .proxy_error_response(synthetic_error(expected));
                assert_exact_contract(
                    response,
                    protocol,
                    expected.status,
                    expected.stable_code,
                    protocol_category(&expected.categories, protocol),
                    expected.message,
                    &request_id_value,
                    expected.upstream.map(UpstreamFixture::expected_value),
                    expected.retry_after,
                    None,
                )
                .await;
                proxy_combinations += 1;
            }
        }
        assert_eq!(proxy_combinations, 116);

        let mut router_combinations = 0;
        for (expected, actual_rejection) in
            EXPECTED_ROUTER_CONTRACTS.iter().zip(RouterRejection::ALL)
        {
            assert_eq!(
                expected.rejection, actual_rejection,
                "router table order drifted"
            );
            for protocol in DownstreamProtocol::ALL {
                let request_id = RequestId::new();
                let request_id_value = request_id.as_str().to_string();
                let adapter = ProtocolErrorResponseAdapter::new(protocol, request_id);
                let response = if let Some(allow) = expected.allow {
                    let mut standard_headers = HeaderMap::new();
                    standard_headers.insert(
                        ALLOW,
                        HeaderValue::from_str(allow).expect("allow fixture should be valid"),
                    );
                    adapter.router_rejection_response_with_headers(
                        expected.rejection,
                        standard_headers,
                    )
                } else {
                    adapter.router_rejection_response(expected.rejection)
                };
                assert_exact_contract(
                    response,
                    protocol,
                    expected.status,
                    expected.stable_code,
                    protocol_category(&expected.categories, protocol),
                    expected.message,
                    &request_id_value,
                    None,
                    None,
                    expected.allow,
                )
                .await;
                router_combinations += 1;
            }
        }
        assert_eq!(router_combinations, 8);
    }

    #[tokio::test]
    async fn upstream_timeout_contract_covers_absent_payload_for_all_protocols() {
        let expected = EXPECTED_PROXY_CONTRACTS
            .iter()
            .find(|expected| expected.code == ProxyErrorCode::UpstreamTimeoutError)
            .expect("timeout contract should be present");
        for protocol in DownstreamProtocol::ALL {
            let request_id = RequestId::new();
            let request_id_value = request_id.as_str().to_string();
            let response = ProtocolErrorResponseAdapter::new(protocol, request_id)
                .proxy_error_response(gateway_error(ProxyErrorCode::UpstreamTimeoutError));
            assert_exact_contract(
                response,
                protocol,
                expected.status,
                expected.stable_code,
                protocol_category(&expected.categories, protocol),
                expected.message,
                &request_id_value,
                None,
                None,
                None,
            )
            .await;
        }
    }

    #[tokio::test]
    async fn adapter_renders_four_protocol_envelopes_and_fact_driven_headers() {
        for protocol in DownstreamProtocol::ALL {
            let request_id = RequestId::new();
            let request_id_value = request_id.as_str().to_string();
            let adapter = ProtocolErrorResponseAdapter::new(protocol, request_id);
            let response = adapter.proxy_error_response(
                gateway_error(ProxyErrorCode::RateLimitError)
                    .with_retry_after(Duration::from_millis(1_001)),
            );

            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(
                response.headers().get(&X_REQUEST_ID),
                Some(
                    &HeaderValue::from_str(&request_id_value)
                        .expect("request id should be a header value")
                )
            );
            assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-store");
            assert_eq!(
                response.headers().get("x-content-type-options").unwrap(),
                "nosniff"
            );
            assert_eq!(response.headers().get(RETRY_AFTER).unwrap(), "2");
            assert!(response.headers().get(WWW_AUTHENTICATE).is_none());
            assert_eq!(
                response
                    .headers()
                    .get("request-id")
                    .map(|value| value.to_str().unwrap()),
                (protocol == DownstreamProtocol::Anthropic).then_some(request_id_value.as_str())
            );

            let body = response_body(response).await;
            assert!(body.get("code").is_none());
            assert!(body.get("message").is_none());
            match protocol {
                DownstreamProtocol::Openai | DownstreamProtocol::Responses => assert_eq!(
                    body,
                    json!({
                        "error": {
                            "message": "The API key rate limit was exceeded.",
                            "type": "rate_limit_error",
                            "param": null,
                            "code": "rate_limit_error"
                        }
                    })
                ),
                DownstreamProtocol::Anthropic => assert_eq!(
                    body,
                    json!({
                        "type": "error",
                        "error": {
                            "type": "rate_limit_error",
                            "message": "The API key rate limit was exceeded.",
                            "code": "rate_limit_error"
                        },
                        "request_id": request_id_value
                    })
                ),
                DownstreamProtocol::Gemini => assert_eq!(
                    body,
                    json!({
                        "error": {
                            "code": 429,
                            "message": "The API key rate limit was exceeded.",
                            "status": "RESOURCE_EXHAUSTED",
                            "details": [{
                                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                                "reason": "RATE_LIMIT_ERROR",
                                "domain": "cyder.gateway",
                                "metadata": {
                                    "request_id": request_id_value,
                                    "cyder_code": "rate_limit_error"
                                }
                            }]
                        }
                    })
                ),
            }
        }
    }

    #[tokio::test]
    async fn adapter_preserves_top_level_upstream_payload_and_auth_challenge_rules() {
        let payload = UpstreamErrorPayload::capture(
            StatusCode::TOO_MANY_REQUESTS,
            Some(&HeaderValue::from_static("application/json")),
            br#"{"error":{"message":"provider quota"}}"#,
            65_536,
        );
        let expected_payload =
            serde_json::to_value(&payload).expect("upstream payload should serialize");

        for protocol in DownstreamProtocol::ALL {
            let adapter = ProtocolErrorResponseAdapter::new(protocol, RequestId::new());
            let upstream_response = adapter.proxy_error_response(ProxyError::upstream(
                ProxyErrorCode::UpstreamRateLimitError,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
                payload.clone(),
                "provider returned 429",
            ));
            assert!(upstream_response.headers().get(RETRY_AFTER).is_none());
            let body = response_body(upstream_response).await;
            assert_eq!(body["upstream_error"], expected_payload);

            let auth_response =
                adapter.proxy_error_response(gateway_error(ProxyErrorCode::AuthenticationError));
            assert_eq!(
                auth_response
                    .headers()
                    .get(WWW_AUTHENTICATE)
                    .map(|value| value.to_str().unwrap()),
                (protocol != DownstreamProtocol::Gemini).then_some("Bearer")
            );
        }
    }

    #[tokio::test]
    async fn router_adapter_preserves_standard_allow_header() {
        let adapter =
            ProtocolErrorResponseAdapter::new(DownstreamProtocol::Anthropic, RequestId::new());
        let mut standard_headers = HeaderMap::new();
        standard_headers.insert(ALLOW, HeaderValue::from_static("GET,POST"));

        let response = adapter.router_rejection_response_with_headers(
            RouterRejection::MethodNotAllowed,
            standard_headers,
        );
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers().get(ALLOW).unwrap(), "GET,POST");
        let body = response_body(response).await;
        assert_eq!(body["error"]["code"], "method_not_allowed_error");
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert!(body.get("upstream_error").is_none());
    }

    #[test]
    fn all_proxy_codes_have_categories_for_all_downstream_protocols() {
        for code in ProxyErrorCode::ALL {
            for protocol in DownstreamProtocol::ALL {
                assert!(!proxy_error_category(code, protocol).is_empty());
            }
        }
    }

    #[test]
    fn representative_protocol_categories_match_the_r3_4_contract() {
        let expected = [
            (
                ProxyErrorCode::AuthenticationError,
                [
                    "authentication_error",
                    "authentication_error",
                    "authentication_error",
                    "UNAUTHENTICATED",
                ],
            ),
            (
                ProxyErrorCode::UpstreamAuthenticationError,
                ["server_error", "server_error", "api_error", "UNKNOWN"],
            ),
            (
                ProxyErrorCode::UpstreamTimeoutError,
                [
                    "server_error",
                    "server_error",
                    "timeout_error",
                    "DEADLINE_EXCEEDED",
                ],
            ),
        ];

        for (code, expected_categories) in expected {
            for (protocol, expected_category) in
                DownstreamProtocol::ALL.into_iter().zip(expected_categories)
            {
                assert_eq!(proxy_error_category(code, protocol), expected_category);
            }
        }
    }

    #[test]
    fn router_rejections_have_independent_stable_contracts() {
        let expected = [
            (
                RouterRejection::RouteNotFound,
                "route_not_found_error",
                StatusCode::NOT_FOUND,
                "The requested gateway route was not found.",
                [
                    "invalid_request_error",
                    "invalid_request_error",
                    "not_found_error",
                    "NOT_FOUND",
                ],
            ),
            (
                RouterRejection::MethodNotAllowed,
                "method_not_allowed_error",
                StatusCode::METHOD_NOT_ALLOWED,
                "The requested HTTP method is not allowed for this gateway route.",
                [
                    "invalid_request_error",
                    "invalid_request_error",
                    "invalid_request_error",
                    "UNIMPLEMENTED",
                ],
            ),
        ];

        assert_eq!(RouterRejection::ALL.len(), expected.len());
        for (rejection, code, status, message, categories) in expected {
            assert_eq!(rejection.code(), code);
            assert_eq!(rejection.status_code(), status);
            assert_eq!(rejection.public_message(), message);
            for (protocol, category) in DownstreamProtocol::ALL.into_iter().zip(categories) {
                assert_eq!(rejection.category(protocol), category);
            }
        }
    }
}
