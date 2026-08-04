use cyder_tools::log::{info, warn};
use std::time::Duration;

use super::{ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, TimeoutPhase};
use crate::service::{
    app_state::AppState,
    runtime::{
        ProviderCircuitError, ProviderCircuitProbePermit, ProviderCircuitRejection,
        ProviderHealthStatus,
    },
};

#[derive(Debug)]
pub(super) enum ProviderGovernanceCheckError {
    Rejected(ProviderGovernanceRejection),
    Backend(ProxyError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProviderGovernanceRejection {
    Open { retry_after: Option<Duration> },
    HalfOpenProbeInFlight,
}

impl ProviderGovernanceRejection {
    pub(super) fn to_proxy_error(self, provider_label: &str) -> ProxyError {
        let (code, operator_message) = match self {
            Self::Open { .. } => (
                ProxyErrorCode::ProviderCircuitOpenError,
                format!(
                    "Provider '{provider_label}' is temporarily unavailable due to recent upstream failures."
                ),
            ),
            Self::HalfOpenProbeInFlight => (
                ProxyErrorCode::ProviderHalfOpenProbeInFlightError,
                format!(
                    "Provider '{provider_label}' is temporarily unavailable because another half-open probe is already in flight."
                ),
            ),
        };
        let error = ProxyError::gateway(
            code,
            ExecutionStage::Governance,
            ResponseVisibility::NotVisible,
            None,
            operator_message,
        );
        match self {
            Self::Open {
                retry_after: Some(retry_after),
            } => error.with_retry_after(retry_after),
            Self::Open { retry_after: None } | Self::HalfOpenProbeInFlight => error,
        }
    }
}

pub(super) async fn ensure_provider_request_allowed(
    app_state: &AppState,
    provider_id: i64,
    provider_label: &str,
) -> Result<Option<ProviderCircuitProbePermit>, ProviderGovernanceCheckError> {
    let decision = app_state
        .provider_circuit
        .allow_provider_request(provider_id)
        .await;

    match decision {
        Ok(decision) => {
            if !decision.allowed {
                let Some(rejection) = decision.rejection else {
                    return Err(ProviderGovernanceCheckError::Backend(ProxyError::gateway(
                        ProxyErrorCode::ServerError,
                        ExecutionStage::Governance,
                        ResponseVisibility::NotVisible,
                        None,
                        "Provider circuit rejected without a domain reason",
                    )));
                };
                let rejection = provider_circuit_rejection_to_governance_rejection(
                    rejection,
                    decision.retry_after,
                );
                return Err(ProviderGovernanceCheckError::Rejected(rejection));
            }

            if decision.snapshot.status == ProviderHealthStatus::HalfOpen {
                info!(
                    "Provider governance entering half-open probe: provider_id={}, provider={}",
                    provider_id, provider_label
                );
            }
            Ok(decision.probe_permit)
        }
        Err(err) => {
            log_provider_circuit_error("allow", provider_id, &err);
            Err(ProviderGovernanceCheckError::Backend(
                provider_circuit_error_to_proxy_error(err),
            ))
        }
    }
}

pub(super) async fn record_provider_success(
    app_state: &AppState,
    provider_id: i64,
    provider_label: &str,
    permit: Option<&ProviderCircuitProbePermit>,
) {
    let snapshot = app_state
        .provider_circuit
        .record_provider_success(provider_id, permit)
        .await;
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(err) => {
            log_provider_circuit_error("record_success", provider_id, &err);
            return;
        }
    };
    if snapshot.status == ProviderHealthStatus::Healthy && snapshot.consecutive_failures == 0 {
        info!(
            "Provider governance marked provider healthy: provider_id={}, provider={}",
            provider_id, provider_label
        );
    }
}

pub(super) async fn record_provider_failure(
    app_state: &AppState,
    provider_id: i64,
    provider_label: &str,
    error: &ProxyError,
    permit: Option<&ProviderCircuitProbePermit>,
) {
    if !counts_against_provider_governance(error) {
        return;
    }

    let snapshot = app_state
        .provider_circuit
        .record_provider_failure(provider_id, error.to_string(), permit)
        .await;
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(err) => {
            log_provider_circuit_error("record_failure", provider_id, &err);
            return;
        }
    };
    if snapshot.status == ProviderHealthStatus::Open {
        warn!(
            "Provider governance opened circuit: provider_id={}, provider={}, consecutive_failures={}, error={}",
            provider_id, provider_label, snapshot.consecutive_failures, error
        );
    }
}

/// Release a half-open probe without recording a provider failure.
pub(super) async fn release_provider_probe(
    app_state: &AppState,
    provider_id: i64,
    permit: Option<&ProviderCircuitProbePermit>,
) {
    if permit.is_none() {
        return;
    }
    match app_state
        .provider_circuit
        .release_provider_probe(provider_id, permit)
        .await
    {
        Ok(_) => {}
        Err(err) => log_provider_circuit_error("release_probe", provider_id, &err),
    }
}

/// Record a provider-attributable outcome once, or release a half-open probe
/// when the request ended before there was a provider outcome to count.
pub(super) async fn record_provider_failure_or_release_probe(
    app_state: &AppState,
    cancellation: &crate::proxy::cancellation::ProxyCancellationContext,
    provider_id: i64,
    provider_label: &str,
    error: &ProxyError,
    permit: Option<&ProviderCircuitProbePermit>,
) {
    if !counts_against_provider_governance(error) {
        release_provider_probe(app_state, provider_id, permit).await;
        return;
    }

    if cancellation.try_provider_failure() {
        record_provider_failure(app_state, provider_id, provider_label, error, permit).await;
    }
}

fn counts_against_provider_governance(error: &ProxyError) -> bool {
    match error.code() {
        ProxyErrorCode::UpstreamTimeoutError => {
            !matches!(error.timeout_phase(), Some(TimeoutPhase::Total))
        }
        ProxyErrorCode::UpstreamConnectError
        | ProxyErrorCode::UpstreamRequestError
        | ProxyErrorCode::UpstreamResponseError
        | ProxyErrorCode::UpstreamAuthenticationError
        | ProxyErrorCode::UpstreamRateLimitError
        | ProxyErrorCode::UpstreamServiceError
        | ProxyErrorCode::UpstreamUnexpectedStatusError => true,
        _ => false,
    }
}

fn provider_circuit_rejection_to_governance_rejection(
    rejection: ProviderCircuitRejection,
    retry_after: Option<Duration>,
) -> ProviderGovernanceRejection {
    match rejection {
        ProviderCircuitRejection::OpenCooldown => ProviderGovernanceRejection::Open { retry_after },
        ProviderCircuitRejection::HalfOpenProbeInFlight => {
            debug_assert!(retry_after.is_none());
            ProviderGovernanceRejection::HalfOpenProbeInFlight
        }
    }
}

fn provider_circuit_error_to_proxy_error(error: ProviderCircuitError) -> ProxyError {
    ProxyError::gateway(
        ProxyErrorCode::ServerError,
        ExecutionStage::Governance,
        ResponseVisibility::NotVisible,
        None,
        format!("Provider circuit state backend error: {error}"),
    )
}

fn log_provider_circuit_error(
    operation: &'static str,
    provider_id: i64,
    error: &ProviderCircuitError,
) {
    warn!(
        "Provider governance state backend error: operation={}, provider_id={}, error={}",
        operation, provider_id, error
    );
}

#[cfg(test)]
mod tests {
    use super::{
        counts_against_provider_governance, provider_circuit_rejection_to_governance_rejection,
    };
    use crate::{
        proxy::{
            ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, TimeoutPhase,
            error::UpstreamErrorPayload,
        },
        service::runtime::ProviderCircuitRejection,
    };
    use std::time::Duration;

    #[test]
    fn provider_circuit_retry_fact_is_forwarded_only_for_open_cooldown() {
        let open = provider_circuit_rejection_to_governance_rejection(
            ProviderCircuitRejection::OpenCooldown,
            Some(Duration::from_millis(1_001)),
        )
        .to_proxy_error("provider");
        assert_eq!(open.code(), ProxyErrorCode::ProviderCircuitOpenError);
        assert_eq!(
            open.response_hints().retry_after().map(|value| value.get()),
            Some(2)
        );

        let half_open = provider_circuit_rejection_to_governance_rejection(
            ProviderCircuitRejection::HalfOpenProbeInFlight,
            None,
        )
        .to_proxy_error("provider");
        assert_eq!(
            half_open.response_hints().retry_after(),
            None,
            "half-open recovery depends on the active probe"
        );
    }

    #[test]
    fn provider_governance_counts_only_upstream_availability_failures() {
        let error = |code: ProxyErrorCode| {
            if code.accepts_upstream_payload() {
                ProxyError::upstream(
                    code,
                    ExecutionStage::UpstreamResponse,
                    ResponseVisibility::NotVisible,
                    UpstreamErrorPayload::capture(code.status_code(), None, b"test", 65_536),
                    "test",
                )
            } else if code == ProxyErrorCode::UpstreamTimeoutError {
                ProxyError::upstream_timeout(
                    TimeoutPhase::Total,
                    ExecutionStage::UpstreamResponse,
                    ResponseVisibility::NotVisible,
                    "test",
                )
            } else {
                ProxyError::gateway(
                    code,
                    ExecutionStage::Governance,
                    ResponseVisibility::NotVisible,
                    None,
                    "test",
                )
            }
        };

        for code in [
            ProxyErrorCode::UpstreamConnectError,
            ProxyErrorCode::UpstreamRequestError,
            ProxyErrorCode::UpstreamResponseError,
            ProxyErrorCode::UpstreamAuthenticationError,
            ProxyErrorCode::UpstreamRateLimitError,
            ProxyErrorCode::UpstreamServiceError,
            ProxyErrorCode::UpstreamUnexpectedStatusError,
        ] {
            assert!(counts_against_provider_governance(&error(code)));
        }
        for phase in [
            TimeoutPhase::Connect,
            TimeoutPhase::RequestSend,
            TimeoutPhase::FirstByte,
            TimeoutPhase::ResponseIdle,
        ] {
            let timeout = ProxyError::upstream_timeout(
                phase,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
                "test",
            );
            assert!(counts_against_provider_governance(&timeout));
        }
        let total = ProxyError::upstream_timeout(
            TimeoutPhase::Total,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
            "test",
        );
        assert!(!counts_against_provider_governance(&total));
        let provider_timeout_status = ProxyError::upstream(
            ProxyErrorCode::UpstreamTimeoutError,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
            UpstreamErrorPayload::capture(
                axum::http::StatusCode::GATEWAY_TIMEOUT,
                None,
                b"provider timeout",
                65_536,
            ),
            "provider timeout status",
        );
        assert!(counts_against_provider_governance(&provider_timeout_status));
        for code in [
            ProxyErrorCode::InvalidRequestError,
            ProxyErrorCode::PermissionError,
            ProxyErrorCode::ProviderCircuitOpenError,
            ProxyErrorCode::ProviderHalfOpenProbeInFlightError,
            ProxyErrorCode::ProviderConfigurationError,
            ProxyErrorCode::ClientCancelledError,
        ] {
            assert!(!counts_against_provider_governance(&error(code)));
        }
    }
}
