use axum::http::StatusCode;
use serde::Serialize;
use std::{fmt, num::NonZeroU64, time::Duration};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ErrorResponseHints {
    retry_after: Option<RetryAfterSeconds>,
}

impl ErrorResponseHints {
    pub(crate) const fn retry_after(self) -> Option<RetryAfterSeconds> {
        self.retry_after
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetryAfterSeconds(NonZeroU64);

impl RetryAfterSeconds {
    pub(crate) fn from_duration_ceil(duration: Duration) -> Self {
        let seconds = duration
            .as_secs()
            .saturating_add(u64::from(duration.subsec_nanos() != 0))
            .max(1);
        Self(NonZeroU64::new(seconds).expect("retry-after seconds must be non-zero"))
    }

    pub(crate) const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyLogLevel {
    Debug,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProxyErrorCode {
    AuthenticationError,
    ApiKeyDisabledError,
    ApiKeyExpiredError,
    InvalidRequestError,
    PermissionError,
    RequestBodyTooLargeError,
    UnsupportedCapabilityError,
    RequestPatchConflictError,
    RateLimitError,
    ConcurrencyLimitError,
    QuotaExhaustedError,
    BudgetExhaustedError,
    ProviderCircuitOpenError,
    ProviderHalfOpenProbeInFlightError,
    ClientCancelledError,
    ServerError,
    ProtocolTransformError,
    ProviderConfigurationError,
    DownstreamSendError,
    UpstreamInvalidRequestError,
    UpstreamAuthenticationError,
    UpstreamRateLimitError,
    UpstreamPayloadTooLargeError,
    UpstreamTimeoutError,
    UpstreamServiceError,
    UpstreamUnexpectedStatusError,
    UpstreamConnectError,
    UpstreamRequestError,
    UpstreamResponseError,
}

impl ProxyErrorCode {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 29] = [
        Self::AuthenticationError,
        Self::ApiKeyDisabledError,
        Self::ApiKeyExpiredError,
        Self::InvalidRequestError,
        Self::PermissionError,
        Self::RequestBodyTooLargeError,
        Self::UnsupportedCapabilityError,
        Self::RequestPatchConflictError,
        Self::RateLimitError,
        Self::ConcurrencyLimitError,
        Self::QuotaExhaustedError,
        Self::BudgetExhaustedError,
        Self::ProviderCircuitOpenError,
        Self::ProviderHalfOpenProbeInFlightError,
        Self::ClientCancelledError,
        Self::ServerError,
        Self::ProtocolTransformError,
        Self::ProviderConfigurationError,
        Self::DownstreamSendError,
        Self::UpstreamInvalidRequestError,
        Self::UpstreamAuthenticationError,
        Self::UpstreamRateLimitError,
        Self::UpstreamPayloadTooLargeError,
        Self::UpstreamTimeoutError,
        Self::UpstreamServiceError,
        Self::UpstreamUnexpectedStatusError,
        Self::UpstreamConnectError,
        Self::UpstreamRequestError,
        Self::UpstreamResponseError,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationError => "authentication_error",
            Self::ApiKeyDisabledError => "api_key_disabled_error",
            Self::ApiKeyExpiredError => "api_key_expired_error",
            Self::InvalidRequestError => "invalid_request_error",
            Self::PermissionError => "permission_error",
            Self::RequestBodyTooLargeError => "request_body_too_large_error",
            Self::UnsupportedCapabilityError => "unsupported_capability_error",
            Self::RequestPatchConflictError => "request_patch_conflict_error",
            Self::RateLimitError => "rate_limit_error",
            Self::ConcurrencyLimitError => "concurrency_limit_error",
            Self::QuotaExhaustedError => "quota_exhausted_error",
            Self::BudgetExhaustedError => "budget_exhausted_error",
            Self::ProviderCircuitOpenError => "provider_circuit_open_error",
            Self::ProviderHalfOpenProbeInFlightError => "provider_half_open_probe_in_flight_error",
            Self::ClientCancelledError => "client_cancelled_error",
            Self::ServerError => "server_error",
            Self::ProtocolTransformError => "protocol_transform_error",
            Self::ProviderConfigurationError => "provider_configuration_error",
            Self::DownstreamSendError => "downstream_send_error",
            Self::UpstreamInvalidRequestError => "upstream_invalid_request_error",
            Self::UpstreamAuthenticationError => "upstream_authentication_error",
            Self::UpstreamRateLimitError => "upstream_rate_limit_error",
            Self::UpstreamPayloadTooLargeError => "upstream_payload_too_large_error",
            Self::UpstreamTimeoutError => "upstream_timeout_error",
            Self::UpstreamServiceError => "upstream_service_error",
            Self::UpstreamUnexpectedStatusError => "upstream_unexpected_status_error",
            Self::UpstreamConnectError => "upstream_connect_error",
            Self::UpstreamRequestError => "upstream_request_error",
            Self::UpstreamResponseError => "upstream_response_error",
        }
    }

    pub(crate) fn status_code(self) -> StatusCode {
        match self {
            Self::AuthenticationError => StatusCode::UNAUTHORIZED,
            Self::ApiKeyDisabledError
            | Self::ApiKeyExpiredError
            | Self::PermissionError
            | Self::BudgetExhaustedError => StatusCode::FORBIDDEN,
            Self::InvalidRequestError
            | Self::UnsupportedCapabilityError
            | Self::UpstreamInvalidRequestError => StatusCode::BAD_REQUEST,
            Self::RequestBodyTooLargeError | Self::UpstreamPayloadTooLargeError => {
                StatusCode::PAYLOAD_TOO_LARGE
            }
            Self::RateLimitError
            | Self::ConcurrencyLimitError
            | Self::QuotaExhaustedError
            | Self::UpstreamRateLimitError => StatusCode::TOO_MANY_REQUESTS,
            Self::ClientCancelledError => {
                StatusCode::from_u16(499).expect("499 should be a valid status code")
            }
            Self::RequestPatchConflictError
            | Self::ServerError
            | Self::ProtocolTransformError
            | Self::ProviderConfigurationError
            | Self::DownstreamSendError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::UpstreamAuthenticationError
            | Self::UpstreamUnexpectedStatusError
            | Self::UpstreamConnectError
            | Self::UpstreamRequestError
            | Self::UpstreamResponseError => StatusCode::BAD_GATEWAY,
            Self::ProviderCircuitOpenError
            | Self::ProviderHalfOpenProbeInFlightError
            | Self::UpstreamServiceError => StatusCode::SERVICE_UNAVAILABLE,
            Self::UpstreamTimeoutError => StatusCode::GATEWAY_TIMEOUT,
        }
    }

    pub(crate) const fn default_public_message(self) -> &'static str {
        match self {
            Self::AuthenticationError => "Authentication failed.",
            Self::ApiKeyDisabledError => "The API key is disabled.",
            Self::ApiKeyExpiredError => "The API key has expired.",
            Self::InvalidRequestError => "The request is invalid.",
            Self::PermissionError => "The request is not permitted.",
            Self::RequestBodyTooLargeError => "The request body is too large.",
            Self::UnsupportedCapabilityError => {
                "The selected target does not support the requested capability."
            }
            Self::RequestPatchConflictError => {
                "The gateway request patch configuration is conflicting."
            }
            Self::RateLimitError => "The API key rate limit was exceeded.",
            Self::ConcurrencyLimitError => "The API key concurrency limit was exceeded.",
            Self::QuotaExhaustedError => "The API key quota was exhausted.",
            Self::BudgetExhaustedError => "The API key budget was exhausted.",
            Self::ProviderCircuitOpenError => "The upstream provider is temporarily unavailable.",
            Self::ProviderHalfOpenProbeInFlightError => {
                "The upstream provider probe is already in progress."
            }
            Self::ClientCancelledError => "The client cancelled the request.",
            Self::ServerError => "The gateway encountered an internal error.",
            Self::ProtocolTransformError => {
                "The gateway could not transform the request or response."
            }
            Self::ProviderConfigurationError => "The gateway provider configuration is invalid.",
            Self::DownstreamSendError => "The gateway could not construct the downstream response.",
            Self::UpstreamInvalidRequestError => "Upstream provider rejected the request.",
            Self::UpstreamAuthenticationError => "Upstream provider rejected its credentials.",
            Self::UpstreamRateLimitError => "Upstream provider rate limited the request.",
            Self::UpstreamPayloadTooLargeError => {
                "Upstream provider rejected the request payload as too large."
            }
            Self::UpstreamTimeoutError => "Upstream provider timed out.",
            Self::UpstreamServiceError => "Upstream provider service failed.",
            Self::UpstreamUnexpectedStatusError => {
                "Upstream provider returned an unexpected HTTP status."
            }
            Self::UpstreamConnectError => "The gateway could not connect to the upstream provider.",
            Self::UpstreamRequestError => {
                "The gateway could not send the request to the upstream provider."
            }
            Self::UpstreamResponseError => {
                "The gateway could not read a valid response from the upstream provider."
            }
        }
    }

    pub(crate) const fn operator_log_level(self) -> ProxyLogLevel {
        match self {
            Self::AuthenticationError
            | Self::ApiKeyDisabledError
            | Self::ApiKeyExpiredError
            | Self::InvalidRequestError
            | Self::PermissionError
            | Self::RequestBodyTooLargeError
            | Self::UnsupportedCapabilityError
            | Self::RateLimitError
            | Self::ConcurrencyLimitError
            | Self::QuotaExhaustedError
            | Self::BudgetExhaustedError
            | Self::ClientCancelledError => ProxyLogLevel::Debug,
            Self::ProviderCircuitOpenError | Self::ProviderHalfOpenProbeInFlightError => {
                ProxyLogLevel::Warn
            }
            Self::RequestPatchConflictError
            | Self::ServerError
            | Self::ProtocolTransformError
            | Self::ProviderConfigurationError
            | Self::DownstreamSendError
            | Self::UpstreamInvalidRequestError
            | Self::UpstreamAuthenticationError
            | Self::UpstreamRateLimitError
            | Self::UpstreamPayloadTooLargeError
            | Self::UpstreamTimeoutError
            | Self::UpstreamServiceError
            | Self::UpstreamUnexpectedStatusError
            | Self::UpstreamConnectError
            | Self::UpstreamRequestError
            | Self::UpstreamResponseError => ProxyLogLevel::Error,
        }
    }

    pub(crate) const fn allows_gateway_detail(self) -> bool {
        matches!(
            self,
            Self::AuthenticationError
                | Self::ApiKeyDisabledError
                | Self::ApiKeyExpiredError
                | Self::InvalidRequestError
                | Self::PermissionError
                | Self::RequestBodyTooLargeError
                | Self::UnsupportedCapabilityError
                | Self::RateLimitError
                | Self::ConcurrencyLimitError
                | Self::QuotaExhaustedError
                | Self::BudgetExhaustedError
        )
    }

    pub(crate) const fn requires_upstream_payload(self) -> bool {
        matches!(
            self,
            Self::UpstreamInvalidRequestError
                | Self::UpstreamAuthenticationError
                | Self::UpstreamRateLimitError
                | Self::UpstreamPayloadTooLargeError
                | Self::UpstreamServiceError
                | Self::UpstreamUnexpectedStatusError
        )
    }

    pub(crate) const fn accepts_upstream_payload(self) -> bool {
        self.requires_upstream_payload() || matches!(self, Self::UpstreamTimeoutError)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionStage {
    Receive,
    Authentication,
    Parse,
    Capability,
    Patch,
    Transform,
    Materialize,
    Governance,
    Connect,
    UpstreamResponse,
    DownstreamSend,
}

impl ExecutionStage {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 11] = [
        Self::Receive,
        Self::Authentication,
        Self::Parse,
        Self::Capability,
        Self::Patch,
        Self::Transform,
        Self::Materialize,
        Self::Governance,
        Self::Connect,
        Self::UpstreamResponse,
        Self::DownstreamSend,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Receive => "receive",
            Self::Authentication => "authentication",
            Self::Parse => "parse",
            Self::Capability => "capability",
            Self::Patch => "patch",
            Self::Transform => "transform",
            Self::Materialize => "materialize",
            Self::Governance => "governance",
            Self::Connect => "connect",
            Self::UpstreamResponse => "upstream_response",
            Self::DownstreamSend => "downstream_send",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ClientErrorPayload {
    Gateway {
        message: String,
    },
    Upstream {
        message: &'static str,
        upstream_error: super::upstream::UpstreamErrorPayload,
    },
}

impl ClientErrorPayload {
    pub(crate) fn public_message(&self) -> &str {
        match self {
            Self::Gateway { message } => message,
            Self::Upstream { message, .. } => message,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProxyError {
    code: ProxyErrorCode,
    stage: ExecutionStage,
    response_visibility: super::visibility::ResponseVisibility,
    client_payload: ClientErrorPayload,
    operator_message: String,
    response_hints: ErrorResponseHints,
}

impl ProxyError {
    pub(crate) fn gateway(
        code: ProxyErrorCode,
        stage: ExecutionStage,
        response_visibility: super::visibility::ResponseVisibility,
        detailed_public_message: Option<String>,
        operator_message: impl Into<String>,
    ) -> Self {
        assert!(
            !code.requires_upstream_payload(),
            "explicit upstream status errors require an upstream payload"
        );
        let message = if code.allows_gateway_detail() {
            detailed_public_message.unwrap_or_else(|| code.default_public_message().to_string())
        } else {
            code.default_public_message().to_string()
        };
        Self {
            code,
            stage,
            response_visibility,
            client_payload: ClientErrorPayload::Gateway { message },
            operator_message: operator_message.into(),
            response_hints: ErrorResponseHints::default(),
        }
    }

    pub(crate) fn upstream(
        code: ProxyErrorCode,
        stage: ExecutionStage,
        response_visibility: super::visibility::ResponseVisibility,
        upstream_error: super::upstream::UpstreamErrorPayload,
        operator_message: impl Into<String>,
    ) -> Self {
        assert!(
            code.accepts_upstream_payload(),
            "only explicit upstream status errors may carry an upstream payload"
        );
        Self {
            code,
            stage,
            response_visibility,
            client_payload: ClientErrorPayload::Upstream {
                message: code.default_public_message(),
                upstream_error,
            },
            operator_message: operator_message.into(),
            response_hints: ErrorResponseHints::default(),
        }
    }

    pub(crate) fn with_retry_after(mut self, duration: Duration) -> Self {
        self.response_hints.retry_after = Some(RetryAfterSeconds::from_duration_ceil(duration));
        self
    }

    pub(crate) const fn code(&self) -> ProxyErrorCode {
        self.code
    }

    pub(crate) fn status_code(&self) -> StatusCode {
        self.code.status_code()
    }

    pub(crate) const fn operator_log_level(&self) -> ProxyLogLevel {
        self.code.operator_log_level()
    }

    pub(crate) const fn stage(&self) -> ExecutionStage {
        self.stage
    }

    pub(crate) const fn response_visibility(&self) -> super::visibility::ResponseVisibility {
        self.response_visibility
    }

    pub(crate) fn upstream_error(&self) -> Option<&super::upstream::UpstreamErrorPayload> {
        match &self.client_payload {
            ClientErrorPayload::Upstream { upstream_error, .. } => Some(upstream_error),
            ClientErrorPayload::Gateway { .. } => None,
        }
    }

    pub(crate) fn public_message(&self) -> &str {
        self.client_payload.public_message()
    }

    pub(crate) const fn response_hints(&self) -> ErrorResponseHints {
        self.response_hints
    }

    #[cfg(test)]
    pub(crate) fn client_payload(&self) -> &ClientErrorPayload {
        &self.client_payload
    }

    pub(crate) fn operator_message(&self) -> &str {
        &self.operator_message
    }
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{} stage={} visibility={}] {}",
            self.code.as_str(),
            self.stage.as_str(),
            self.response_visibility.as_str(),
            self.operator_message
        )
    }
}

impl std::error::Error for ProxyError {}

#[cfg(test)]
mod tests {
    use super::{
        ClientErrorPayload, ExecutionStage, ProxyError, ProxyErrorCode, ProxyLogLevel,
        RetryAfterSeconds,
    };
    use crate::proxy::error::visibility::ResponseVisibility;
    use axum::http::StatusCode;
    use std::{collections::HashSet, time::Duration};

    #[test]
    fn retry_after_seconds_rounds_up_and_never_represents_zero() {
        assert_eq!(
            RetryAfterSeconds::from_duration_ceil(Duration::ZERO).get(),
            1
        );
        assert_eq!(
            RetryAfterSeconds::from_duration_ceil(Duration::from_millis(1)).get(),
            1
        );
        assert_eq!(
            RetryAfterSeconds::from_duration_ceil(Duration::from_secs(2)).get(),
            2
        );
        assert_eq!(
            RetryAfterSeconds::from_duration_ceil(Duration::from_millis(2_001)).get(),
            3
        );
    }

    #[test]
    fn proxy_error_response_hints_are_immutable_builder_facts() {
        let error = ProxyError::gateway(
            ProxyErrorCode::RateLimitError,
            ExecutionStage::Governance,
            ResponseVisibility::NotVisible,
            None,
            "rate limit exceeded",
        );
        assert_eq!(error.response_hints().retry_after(), None);

        let error = error.with_retry_after(Duration::from_millis(1_001));
        assert_eq!(
            error
                .response_hints()
                .retry_after()
                .map(|value| value.get()),
            Some(2)
        );
        assert_eq!(
            error.public_message(),
            "The API key rate limit was exceeded."
        );
    }

    #[test]
    fn stable_error_metadata_is_exhaustive_and_unique() {
        assert_eq!(ProxyErrorCode::ALL.len(), 29);
        let codes = ProxyErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(codes.len(), ProxyErrorCode::ALL.len());

        for code in ProxyErrorCode::ALL {
            assert!(code.as_str().ends_with("_error"));
            assert!(!code.default_public_message().is_empty());
            assert!(code.status_code().is_client_error() || code.status_code().is_server_error());
            assert!(matches!(
                code.operator_log_level(),
                ProxyLogLevel::Debug | ProxyLogLevel::Warn | ProxyLogLevel::Error
            ));
            assert_eq!(
                serde_json::to_value(code).expect("error code should serialize"),
                code.as_str()
            );
        }
    }

    #[test]
    fn stable_stage_metadata_is_exhaustive_and_serializes_as_snake_case() {
        assert_eq!(ExecutionStage::ALL.len(), 11);
        let stages = ExecutionStage::ALL
            .iter()
            .map(|stage| stage.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(stages.len(), ExecutionStage::ALL.len());
        for stage in ExecutionStage::ALL {
            assert_eq!(
                serde_json::to_value(stage).expect("stage should serialize"),
                stage.as_str()
            );
        }
    }

    #[test]
    fn code_status_contract_matches_the_r3_3_table() {
        let expected = [
            (
                ProxyErrorCode::AuthenticationError,
                StatusCode::UNAUTHORIZED,
            ),
            (ProxyErrorCode::ApiKeyDisabledError, StatusCode::FORBIDDEN),
            (ProxyErrorCode::ApiKeyExpiredError, StatusCode::FORBIDDEN),
            (ProxyErrorCode::InvalidRequestError, StatusCode::BAD_REQUEST),
            (ProxyErrorCode::PermissionError, StatusCode::FORBIDDEN),
            (
                ProxyErrorCode::RequestBodyTooLargeError,
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            (
                ProxyErrorCode::UnsupportedCapabilityError,
                StatusCode::BAD_REQUEST,
            ),
            (
                ProxyErrorCode::RequestPatchConflictError,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ProxyErrorCode::RateLimitError,
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                ProxyErrorCode::ConcurrencyLimitError,
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                ProxyErrorCode::QuotaExhaustedError,
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (ProxyErrorCode::BudgetExhaustedError, StatusCode::FORBIDDEN),
            (
                ProxyErrorCode::ProviderCircuitOpenError,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                ProxyErrorCode::ProviderHalfOpenProbeInFlightError,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                ProxyErrorCode::ClientCancelledError,
                StatusCode::from_u16(499).expect("valid 499"),
            ),
            (
                ProxyErrorCode::ServerError,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ProxyErrorCode::ProtocolTransformError,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ProxyErrorCode::ProviderConfigurationError,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ProxyErrorCode::DownstreamSendError,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ProxyErrorCode::UpstreamInvalidRequestError,
                StatusCode::BAD_REQUEST,
            ),
            (
                ProxyErrorCode::UpstreamAuthenticationError,
                StatusCode::BAD_GATEWAY,
            ),
            (
                ProxyErrorCode::UpstreamRateLimitError,
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                ProxyErrorCode::UpstreamPayloadTooLargeError,
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            (
                ProxyErrorCode::UpstreamTimeoutError,
                StatusCode::GATEWAY_TIMEOUT,
            ),
            (
                ProxyErrorCode::UpstreamServiceError,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                ProxyErrorCode::UpstreamUnexpectedStatusError,
                StatusCode::BAD_GATEWAY,
            ),
            (
                ProxyErrorCode::UpstreamConnectError,
                StatusCode::BAD_GATEWAY,
            ),
            (
                ProxyErrorCode::UpstreamRequestError,
                StatusCode::BAD_GATEWAY,
            ),
            (
                ProxyErrorCode::UpstreamResponseError,
                StatusCode::BAD_GATEWAY,
            ),
        ];

        assert_eq!(expected.len(), ProxyErrorCode::ALL.len());
        for (code, status) in expected {
            assert_eq!(
                code.status_code(),
                status,
                "status mismatch for {}",
                code.as_str()
            );
        }
    }

    #[test]
    fn internal_diagnostic_never_replaces_the_fixed_public_message() {
        let error = ProxyError::gateway(
            ProxyErrorCode::ServerError,
            ExecutionStage::Materialize,
            ResponseVisibility::NotVisible,
            Some("secret internal detail".to_string()),
            "database URL and stack detail",
        );

        assert_eq!(
            error.client_payload().public_message(),
            "The gateway encountered an internal error."
        );
        assert_eq!(error.operator_message(), "database URL and stack detail");
        assert!(!error.public_message().contains("database URL"));
    }

    #[test]
    fn caller_correctable_error_may_use_gateway_authored_detail() {
        let error = ProxyError::gateway(
            ProxyErrorCode::InvalidRequestError,
            ExecutionStage::Parse,
            ResponseVisibility::NotVisible,
            Some("The `model` field is required.".to_string()),
            "missing model field",
        );

        assert!(matches!(
            error.client_payload(),
            ClientErrorPayload::Gateway { .. }
        ));
        assert_eq!(
            error.client_payload().public_message(),
            "The `model` field is required."
        );
    }

    #[test]
    fn upstream_payload_is_a_separate_top_level_extension() {
        let error = ProxyError::upstream(
            ProxyErrorCode::UpstreamRateLimitError,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
            crate::proxy::error::upstream::UpstreamErrorPayload::capture(
                StatusCode::TOO_MANY_REQUESTS,
                None,
                br#"{"error":"quota"}"#,
                65_536,
            ),
            "upstream returned HTTP 429",
        );

        let upstream_error = serde_json::to_value(
            error
                .upstream_error()
                .expect("explicit upstream status should retain payload"),
        )
        .expect("upstream payload should serialize");
        assert_eq!(
            error.public_message(),
            "Upstream provider rate limited the request."
        );
        assert_eq!(upstream_error["status"], 429);
        assert_eq!(error.code(), ProxyErrorCode::UpstreamRateLimitError);
        assert_eq!(error.stage(), ExecutionStage::UpstreamResponse);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
    }
}
