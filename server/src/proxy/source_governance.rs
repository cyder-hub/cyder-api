use cyder_tools::log::{info, warn};
use std::time::Duration;

use super::{ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, TimeoutPhase};
use crate::service::{
    app_state::AppState,
    runtime::{
        SourceCircuitError, SourceCircuitProbePermit, SourceCircuitRejection, SourceHealthStatus,
    },
};

#[derive(Debug)]
pub(super) enum SourceGovernanceCheckError {
    Rejected(SourceGovernanceRejection),
    Backend(ProxyError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourceGovernanceRejection {
    Open { retry_after: Option<Duration> },
    HalfOpenProbeInFlight,
}

impl SourceGovernanceRejection {
    pub(super) fn to_proxy_error(self, source_label: &str) -> ProxyError {
        let (code, operator_message) = match self {
            Self::Open { .. } => (
                ProxyErrorCode::ProviderCircuitOpenError,
                format!(
                    "Upstream source '{source_label}' is temporarily unavailable due to recent upstream failures."
                ),
            ),
            Self::HalfOpenProbeInFlight => (
                ProxyErrorCode::ProviderHalfOpenProbeInFlightError,
                format!(
                    "Upstream source '{source_label}' is temporarily unavailable because another half-open probe is already in flight."
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

pub(super) async fn ensure_source_request_allowed(
    app_state: &AppState,
    source_id: i64,
    source_label: &str,
) -> Result<Option<SourceCircuitProbePermit>, SourceGovernanceCheckError> {
    let decision = app_state
        .source_circuit
        .allow_source_request(source_id)
        .await;

    match decision {
        Ok(decision) => {
            if !decision.allowed {
                let Some(rejection) = decision.rejection else {
                    return Err(SourceGovernanceCheckError::Backend(ProxyError::gateway(
                        ProxyErrorCode::ServerError,
                        ExecutionStage::Governance,
                        ResponseVisibility::NotVisible,
                        None,
                        "Source circuit rejected without a domain reason",
                    )));
                };
                let rejection = source_circuit_rejection_to_governance_rejection(
                    rejection,
                    decision.retry_after,
                );
                return Err(SourceGovernanceCheckError::Rejected(rejection));
            }

            if decision.snapshot.status == SourceHealthStatus::HalfOpen {
                info!(
                    "Source governance entering half-open probe: source_id={}, source={}",
                    source_id, source_label
                );
            }
            Ok(decision.probe_permit)
        }
        Err(err) => {
            log_source_circuit_error("allow", source_id, &err);
            Err(SourceGovernanceCheckError::Backend(
                source_circuit_error_to_proxy_error(err),
            ))
        }
    }
}

pub(super) async fn record_source_success(
    app_state: &AppState,
    source_id: i64,
    source_label: &str,
    permit: Option<&SourceCircuitProbePermit>,
) {
    let snapshot = app_state
        .source_circuit
        .record_source_success(source_id, permit)
        .await;
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(err) => {
            log_source_circuit_error("record_success", source_id, &err);
            return;
        }
    };
    if snapshot.status == SourceHealthStatus::Healthy && snapshot.consecutive_failures == 0 {
        info!(
            "Source governance marked source healthy: source_id={}, source={}",
            source_id, source_label
        );
    }
}

pub(super) async fn record_source_failure(
    app_state: &AppState,
    source_id: i64,
    source_label: &str,
    error: &ProxyError,
    permit: Option<&SourceCircuitProbePermit>,
) {
    if !counts_against_source_governance(error) {
        return;
    }

    let snapshot = app_state
        .source_circuit
        .record_source_failure(source_id, error.to_string(), permit)
        .await;
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(err) => {
            log_source_circuit_error("record_failure", source_id, &err);
            return;
        }
    };
    if snapshot.status == SourceHealthStatus::Open {
        warn!(
            "Source governance opened circuit: source_id={}, source={}, consecutive_failures={}, error={}",
            source_id, source_label, snapshot.consecutive_failures, error
        );
    }
}

/// Release a half-open probe without recording a source failure.
pub(super) async fn release_source_probe(
    app_state: &AppState,
    source_id: i64,
    permit: Option<&SourceCircuitProbePermit>,
) {
    if permit.is_none() {
        return;
    }
    match app_state
        .source_circuit
        .release_source_probe(source_id, permit)
        .await
    {
        Ok(_) => {}
        Err(err) => log_source_circuit_error("release_probe", source_id, &err),
    }
}

/// Record a source-attributable outcome once, or release a half-open probe
/// when the request ended before there was a source outcome to count.
pub(super) async fn record_source_failure_or_release_probe(
    app_state: &AppState,
    cancellation: &crate::proxy::cancellation::ProxyCancellationContext,
    source_id: i64,
    source_label: &str,
    error: &ProxyError,
    permit: Option<&SourceCircuitProbePermit>,
) {
    if !counts_against_source_governance(error) {
        release_source_probe(app_state, source_id, permit).await;
        return;
    }

    if cancellation.try_source_failure() {
        record_source_failure(app_state, source_id, source_label, error, permit).await;
    }
}

fn counts_against_source_governance(error: &ProxyError) -> bool {
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

fn source_circuit_rejection_to_governance_rejection(
    rejection: SourceCircuitRejection,
    retry_after: Option<Duration>,
) -> SourceGovernanceRejection {
    match rejection {
        SourceCircuitRejection::OpenCooldown => SourceGovernanceRejection::Open { retry_after },
        SourceCircuitRejection::HalfOpenProbeInFlight => {
            debug_assert!(retry_after.is_none());
            SourceGovernanceRejection::HalfOpenProbeInFlight
        }
    }
}

fn source_circuit_error_to_proxy_error(error: SourceCircuitError) -> ProxyError {
    ProxyError::gateway(
        ProxyErrorCode::ServerError,
        ExecutionStage::Governance,
        ResponseVisibility::NotVisible,
        None,
        format!("Source circuit state backend error: {error}"),
    )
}

fn log_source_circuit_error(operation: &'static str, source_id: i64, error: &SourceCircuitError) {
    warn!(
        "Source governance state backend error: operation={}, source_id={}, error={}",
        operation, source_id, error
    );
}

#[cfg(test)]
mod tests {
    use super::{
        counts_against_source_governance, source_circuit_rejection_to_governance_rejection,
    };
    use crate::{
        proxy::{
            ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, TimeoutPhase,
            error::UpstreamErrorPayload,
        },
        service::runtime::SourceCircuitRejection,
    };
    use std::time::Duration;

    #[test]
    fn source_circuit_retry_fact_is_forwarded_only_for_open_cooldown() {
        let open = source_circuit_rejection_to_governance_rejection(
            SourceCircuitRejection::OpenCooldown,
            Some(Duration::from_millis(1_001)),
        )
        .to_proxy_error("provider");
        assert_eq!(open.code(), ProxyErrorCode::ProviderCircuitOpenError);
        assert_eq!(
            open.response_hints().retry_after().map(|value| value.get()),
            Some(2)
        );

        let half_open = source_circuit_rejection_to_governance_rejection(
            SourceCircuitRejection::HalfOpenProbeInFlight,
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
            assert!(counts_against_source_governance(&error(code)));
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
            assert!(counts_against_source_governance(&timeout));
        }
        let total = ProxyError::upstream_timeout(
            TimeoutPhase::Total,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
            "test",
        );
        assert!(!counts_against_source_governance(&total));
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
        assert!(counts_against_source_governance(&provider_timeout_status));
        for code in [
            ProxyErrorCode::InvalidRequestError,
            ProxyErrorCode::PermissionError,
            ProxyErrorCode::ProviderCircuitOpenError,
            ProxyErrorCode::ProviderHalfOpenProbeInFlightError,
            ProxyErrorCode::ProviderConfigurationError,
            ProxyErrorCode::ClientCancelledError,
        ] {
            assert!(!counts_against_source_governance(&error(code)));
        }
    }
}
