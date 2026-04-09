use cyder_tools::log::{info, warn};

use super::ProxyError;
use crate::service::app_state::{AppState, ProviderHealthStatus};

pub(super) async fn ensure_provider_request_allowed(
    app_state: &AppState,
    provider_id: i64,
    provider_label: &str,
) -> Result<(), ProxyError> {
    match app_state.allow_provider_request(provider_id).await {
        Ok(snapshot) => {
            if snapshot.status == ProviderHealthStatus::HalfOpen {
                info!(
                    "Provider governance entering half-open probe: provider_id={}, provider={}",
                    provider_id, provider_label
                );
            }
            Ok(())
        }
        Err(retry_after) => {
            let retry_hint = retry_after
                .map(|duration| format!(" Retry after {}s.", duration.as_secs()))
                .unwrap_or_else(|| " Another half-open probe is already in flight.".to_string());
            Err(ProxyError::UpstreamService(format!(
                "Provider '{}' is temporarily unavailable due to recent upstream failures.{}",
                provider_label, retry_hint
            )))
        }
    }
}

pub(super) async fn record_provider_success(
    app_state: &AppState,
    provider_id: i64,
    provider_label: &str,
) {
    let snapshot = app_state.record_provider_success(provider_id).await;
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
    }
}
