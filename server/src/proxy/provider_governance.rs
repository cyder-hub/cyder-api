use cyder_tools::log::{info, warn};

use super::{ProxyError, retry_policy::ProviderGovernanceRejection};
use crate::{
    config::CONFIG,
    service::{app_state::AppState, runtime::ProviderHealthStatus},
};

pub(super) async fn ensure_provider_request_allowed(
    app_state: &AppState,
    provider_id: i64,
    provider_label: &str,
) -> Result<(), ProviderGovernanceRejection> {
    match app_state
        .provider_circuit
        .allow_provider_request(provider_id)
        .await
    {
        Ok(snapshot) => {
            if snapshot.status == ProviderHealthStatus::HalfOpen {
                info!(
                    "Provider governance entering half-open probe: provider_id={}, provider={}",
                    provider_id, provider_label
                );
            }
            Ok(())
        }
        Err(Some(_retry_after)) => Err(ProviderGovernanceRejection::Open),
        Err(None) => Err(ProviderGovernanceRejection::HalfOpenProbeInFlight),
    }
}

pub(super) async fn preview_provider_request_allowed(
    app_state: &AppState,
    provider_id: i64,
) -> Result<(), ProviderGovernanceRejection> {
    if !CONFIG.provider_governance.is_enabled() {
        return Ok(());
    }

    let snapshot = app_state
        .provider_circuit
        .get_provider_health_snapshot(provider_id)
        .await;
    match snapshot.status {
        ProviderHealthStatus::Healthy => Ok(()),
        ProviderHealthStatus::Open => {
            let Some(opened_at) = snapshot.opened_at else {
                return Err(ProviderGovernanceRejection::Open);
            };
            let now = chrono::Utc::now().timestamp_millis();
            let elapsed_ms = now.saturating_sub(opened_at);
            let cooldown_ms = i64::try_from(CONFIG.provider_governance.open_cooldown().as_millis())
                .unwrap_or(i64::MAX);
            if elapsed_ms < cooldown_ms {
                Err(ProviderGovernanceRejection::Open)
            } else {
                Ok(())
            }
        }
        ProviderHealthStatus::HalfOpen => {
            if snapshot.half_open_probe_in_flight {
                Err(ProviderGovernanceRejection::HalfOpenProbeInFlight)
            } else {
                Ok(())
            }
        }
    }
}

pub(super) async fn record_provider_success(
    app_state: &AppState,
    provider_id: i64,
    provider_label: &str,
) {
    let snapshot = app_state
        .provider_circuit
        .record_provider_success(provider_id)
        .await;
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
) {
    if !counts_against_provider_governance(error) {
        return;
    }

    let snapshot = app_state
        .provider_circuit
        .record_provider_failure(provider_id, error.to_string())
        .await;
    if snapshot.status == ProviderHealthStatus::Open {
        warn!(
            "Provider governance opened circuit: provider_id={}, provider={}, consecutive_failures={}, error={}",
            provider_id, provider_label, snapshot.consecutive_failures, error
        );
    }
}

fn counts_against_provider_governance(error: &ProxyError) -> bool {
    matches!(
        error,
        ProxyError::BadGateway(_)
            | ProxyError::UpstreamAuthentication(_)
            | ProxyError::UpstreamRateLimited(_)
            | ProxyError::UpstreamService(_)
            | ProxyError::UpstreamTimeout(_)
    )
}

#[cfg(test)]
mod tests {
    use super::counts_against_provider_governance;
    use crate::proxy::ProxyError;

    #[test]
    fn provider_governance_counts_only_upstream_availability_failures() {
        assert!(counts_against_provider_governance(
            &ProxyError::UpstreamTimeout("timeout".to_string())
        ));
        assert!(counts_against_provider_governance(
            &ProxyError::UpstreamService("service".to_string())
        ));
        assert!(counts_against_provider_governance(
            &ProxyError::UpstreamRateLimited("limited".to_string())
        ));
        assert!(!counts_against_provider_governance(
            &ProxyError::BadRequest("client error".to_string())
        ));
        assert!(!counts_against_provider_governance(&ProxyError::Forbidden(
            "forbidden".to_string()
        )));
        assert!(!counts_against_provider_governance(
            &ProxyError::ProviderOpenSkipped("open".to_string())
        ));
        assert!(!counts_against_provider_governance(
            &ProxyError::ProviderHalfOpenProbeInFlight("probe".to_string())
        ));
    }
}
