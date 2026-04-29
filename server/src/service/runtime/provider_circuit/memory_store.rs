use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

use crate::config::ProviderGovernanceConfig;

use super::types::{
    ProviderCircuitDecision, ProviderCircuitError, ProviderCircuitProbePermit,
    ProviderCircuitStore, ProviderHealthSnapshot, ProviderHealthState,
};

pub const DEFAULT_PROVIDER_CIRCUIT_PROBE_LEASE_TTL: Duration = Duration::from_secs(600);

/// Single-instance default and dev/test backend; not a multi-instance correctness backend.
pub struct MemoryProviderCircuitStore {
    inner: tokio::sync::Mutex<HashMap<i64, ProviderHealthState>>,
    probe_lease_ttl: Duration,
}

impl MemoryProviderCircuitStore {
    pub fn with_probe_lease_ttl(probe_lease_ttl: Duration) -> Self {
        Self {
            inner: tokio::sync::Mutex::new(HashMap::new()),
            probe_lease_ttl,
        }
    }
}

impl Default for MemoryProviderCircuitStore {
    fn default() -> Self {
        Self::with_probe_lease_ttl(DEFAULT_PROVIDER_CIRCUIT_PROBE_LEASE_TTL)
    }
}

#[async_trait]
impl ProviderCircuitStore for MemoryProviderCircuitStore {
    async fn allow_request(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
    ) -> Result<ProviderCircuitDecision, ProviderCircuitError> {
        if !config.is_enabled() {
            return Ok(ProviderCircuitDecision::allowed(
                ProviderHealthSnapshot::synthetic_healthy(),
                None,
            ));
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut state = self.inner.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        Ok(provider_state.allow_request(provider_id, config, now_ms, self.probe_lease_ttl))
    }

    async fn record_success(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
        permit: Option<&ProviderCircuitProbePermit>,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
        if !config.is_enabled() {
            return Ok(ProviderHealthSnapshot::synthetic_healthy());
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut state = self.inner.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        provider_state.record_success(config, now_ms, permit);
        Ok(provider_state.snapshot())
    }

    async fn record_failure(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
        error_message: String,
        permit: Option<&ProviderCircuitProbePermit>,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
        if !config.is_enabled() {
            return Ok(ProviderHealthSnapshot::synthetic_healthy());
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut state = self.inner.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        provider_state.record_failure(config, now_ms, error_message, permit);
        Ok(provider_state.snapshot())
    }

    async fn snapshot(
        &self,
        provider_id: i64,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut state = self.inner.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        provider_state.prune_expired_probe(now_ms);
        Ok(provider_state.clone().snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        ProviderCircuitRejection, ProviderCircuitStore, ProviderHealthSnapshot,
        ProviderHealthState, ProviderHealthStatus,
    };
    use super::MemoryProviderCircuitStore;
    use crate::config::ProviderGovernanceConfig;
    use std::time::Duration;

    fn config(threshold: u32, cooldown_seconds: u64) -> ProviderGovernanceConfig {
        ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: threshold,
            open_cooldown_seconds: cooldown_seconds,
        }
    }

    #[test]
    fn provider_health_opens_after_threshold_failures() {
        let config = config(2, 30);
        let now_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();

        state.record_failure(&config, now_ms, "timeout".to_string(), None);
        assert_eq!(state.status, ProviderHealthStatus::Healthy);
        assert_eq!(state.consecutive_failures, 1);
        assert_eq!(state.last_failure_at, Some(now_ms));
        assert!(state.opened_at.is_none());

        state.record_failure(&config, now_ms + 1_000, "another timeout".to_string(), None);
        assert_eq!(state.status, ProviderHealthStatus::Open);
        assert_eq!(state.consecutive_failures, 2);
        assert_eq!(state.last_failure_at, Some(now_ms + 1_000));
        assert_eq!(state.opened_at, Some(now_ms + 1_000));
    }

    #[test]
    fn provider_health_transitions_to_half_open_after_cooldown() {
        let config = config(1, 30);
        let now_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, now_ms, "timeout".to_string(), None);

        let blocked = state.allow_request(1, &config, now_ms + 10_000, Duration::from_secs(600));
        assert!(!blocked.allowed);
        assert_eq!(
            blocked.rejection,
            Some(ProviderCircuitRejection::OpenCooldown)
        );
        assert_eq!(blocked.retry_after, Some(Duration::from_secs(20)));
        assert_eq!(state.status, ProviderHealthStatus::Open);

        let allowed = state.allow_request(1, &config, now_ms + 31_000, Duration::from_secs(600));
        assert!(allowed.allowed);
        let permit = allowed
            .probe_permit
            .as_ref()
            .expect("half-open probe should include a permit");
        assert_eq!(permit.provider_id(), 1);
        assert!(!permit.decision_id().is_empty());
        assert!(!permit.lease_id().is_empty());
        assert_eq!(permit.issued_at_ms(), now_ms + 31_000);
        assert_eq!(permit.probe_expires_at_ms(), now_ms + 31_000 + 600_000);
        assert_eq!(state.status, ProviderHealthStatus::HalfOpen);
        assert!(state.snapshot().half_open_probe_in_flight);
    }

    #[test]
    fn provider_health_half_open_success_closes_circuit() {
        let config = config(1, 30);
        let open_at_ms = 1_700_000_000_000;
        let recover_at_ms = open_at_ms + 31_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, open_at_ms, "timeout".to_string(), None);
        let decision = state.allow_request(1, &config, recover_at_ms, Duration::from_secs(600));
        let permit = decision
            .probe_permit
            .as_ref()
            .expect("half-open probe should have a permit");

        state.record_success(&config, recover_at_ms, Some(permit));
        assert_eq!(state.status, ProviderHealthStatus::Healthy);
        assert_eq!(state.consecutive_failures, 0);
        assert!(!state.snapshot().half_open_probe_in_flight);
        assert_eq!(state.last_recovered_at, Some(recover_at_ms));
        assert_eq!(state.last_failure_at, Some(open_at_ms));
        assert!(state.opened_at.is_none());
        assert!(state.last_error.is_none());
    }

    #[test]
    fn provider_health_half_open_failure_reopens_circuit() {
        let config = config(3, 30);
        let first_failure_at_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, first_failure_at_ms, "timeout".to_string(), None);
        state.record_failure(
            &config,
            first_failure_at_ms + 1_000,
            "timeout".to_string(),
            None,
        );
        state.record_failure(
            &config,
            first_failure_at_ms + 2_000,
            "timeout".to_string(),
            None,
        );
        let decision = state.allow_request(
            1,
            &config,
            first_failure_at_ms + 33_000,
            Duration::from_secs(600),
        );
        let permit = decision
            .probe_permit
            .as_ref()
            .expect("half-open probe should have a permit");

        state.record_failure(
            &config,
            first_failure_at_ms + 33_000,
            "half-open timeout".to_string(),
            Some(permit),
        );
        assert_eq!(state.status, ProviderHealthStatus::Open);
        assert!(!state.snapshot().half_open_probe_in_flight);
        assert_eq!(state.last_failure_at, Some(first_failure_at_ms + 33_000));
        assert_eq!(state.opened_at, Some(first_failure_at_ms + 33_000));
    }

    #[test]
    fn provider_health_ignores_half_open_completion_without_matching_permit() {
        let config = config(1, 30);
        let open_at_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, open_at_ms, "timeout".to_string(), None);
        let decision =
            state.allow_request(1, &config, open_at_ms + 31_000, Duration::from_secs(600));
        assert!(decision.probe_permit.is_some());

        state.record_success(&config, open_at_ms + 32_000, None);
        assert_eq!(state.status, ProviderHealthStatus::HalfOpen);
        assert!(state.snapshot().half_open_probe_in_flight);
    }

    #[test]
    fn provider_health_open_state_does_not_recover_from_ordinary_success() {
        let config = config(1, 30);
        let open_at_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, open_at_ms, "timeout".to_string(), None);

        state.record_success(&config, open_at_ms + 1_000, None);

        assert_eq!(state.status, ProviderHealthStatus::Open);
        assert_eq!(state.opened_at, Some(open_at_ms));
        assert_eq!(state.consecutive_failures, 1);
    }

    #[test]
    fn provider_health_snapshot_exposes_runtime_timestamps() {
        let config = config(1, 30);
        let open_at_ms = 1_700_000_000_000;
        let recover_at_ms = open_at_ms + 31_000;
        let mut state = ProviderHealthState::default();

        state.record_failure(&config, open_at_ms, "timeout".to_string(), None);
        let open_snapshot = state.snapshot();
        assert_eq!(open_snapshot.opened_at, Some(open_at_ms));
        assert_eq!(open_snapshot.last_failure_at, Some(open_at_ms));
        assert_eq!(open_snapshot.last_recovered_at, None);
        assert_eq!(open_snapshot.last_error.as_deref(), Some("timeout"));

        let decision = state.allow_request(1, &config, recover_at_ms, Duration::from_secs(600));
        let permit = decision
            .probe_permit
            .as_ref()
            .expect("half-open probe should have a permit");
        state.record_success(&config, recover_at_ms, Some(permit));

        let recovered_snapshot = state.snapshot();
        assert_eq!(recovered_snapshot.opened_at, None);
        assert_eq!(recovered_snapshot.last_failure_at, Some(open_at_ms));
        assert_eq!(recovered_snapshot.last_recovered_at, Some(recover_at_ms));
        assert!(recovered_snapshot.last_error.is_none());
    }

    #[tokio::test]
    async fn memory_store_returns_default_snapshot_for_unknown_provider() {
        let store = MemoryProviderCircuitStore::default();
        assert_eq!(
            store.snapshot(404).await.expect("snapshot should load"),
            ProviderHealthSnapshot {
                status: ProviderHealthStatus::Healthy,
                consecutive_failures: 0,
                half_open_probe_in_flight: false,
                opened_at: None,
                last_failure_at: None,
                last_recovered_at: None,
                last_error: None,
            }
        );
    }
}
