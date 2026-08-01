use cyder_tools::log::{info, warn};

use super::{ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility};
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
    Open,
    HalfOpenProbeInFlight,
}

impl ProviderGovernanceRejection {
    pub(super) fn to_proxy_error(self, provider_label: &str) -> ProxyError {
        let (code, operator_message) = match self {
            Self::Open => (
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
        ProxyError::gateway(
            code,
            ExecutionStage::Governance,
            ResponseVisibility::NotVisible,
            None,
            operator_message,
        )
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
                let rejection = provider_circuit_rejection_to_governance_rejection(rejection);
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

fn counts_against_provider_governance(error: &ProxyError) -> bool {
    matches!(
        error.code(),
        ProxyErrorCode::UpstreamConnectError
            | ProxyErrorCode::UpstreamRequestError
            | ProxyErrorCode::UpstreamResponseError
            | ProxyErrorCode::UpstreamAuthenticationError
            | ProxyErrorCode::UpstreamRateLimitError
            | ProxyErrorCode::UpstreamServiceError
            | ProxyErrorCode::UpstreamTimeoutError
            | ProxyErrorCode::UpstreamUnexpectedStatusError
    )
}

fn provider_circuit_rejection_to_governance_rejection(
    rejection: ProviderCircuitRejection,
) -> ProviderGovernanceRejection {
    match rejection {
        ProviderCircuitRejection::OpenCooldown => ProviderGovernanceRejection::Open,
        ProviderCircuitRejection::HalfOpenProbeInFlight => {
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
    use super::counts_against_provider_governance;
    use crate::proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, error::UpstreamErrorPayload,
    };

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
            ProxyErrorCode::UpstreamTimeoutError,
            ProxyErrorCode::UpstreamUnexpectedStatusError,
        ] {
            assert!(counts_against_provider_governance(&error(code)));
        }
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
