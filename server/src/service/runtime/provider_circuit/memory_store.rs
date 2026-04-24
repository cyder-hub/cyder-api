use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Instant;

use crate::config::ProviderGovernanceConfig;

use super::types::{ProviderCircuitStore, ProviderHealthSnapshot, ProviderHealthState};

#[derive(Default)]
pub struct MemoryProviderCircuitStore {
    inner: tokio::sync::Mutex<HashMap<i64, ProviderHealthState>>,
}

#[async_trait]
impl ProviderCircuitStore for MemoryProviderCircuitStore {
    async fn allow_request(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
    ) -> Result<ProviderHealthSnapshot, Option<std::time::Duration>> {
        let now = Instant::now();
        let mut state = self.inner.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        provider_state
            .allow_request(config, now)
            .map(|_| provider_state.snapshot())
    }

    async fn record_success(&self, provider_id: i64) -> ProviderHealthSnapshot {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut state = self.inner.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        provider_state.record_success(now_ms);
        provider_state.snapshot()
    }

    async fn record_failure(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
        error_message: String,
    ) -> ProviderHealthSnapshot {
        let now = Instant::now();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut state = self.inner.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        provider_state.record_failure(config, now, now_ms, error_message);
        provider_state.snapshot()
    }

    async fn snapshot(&self, provider_id: i64) -> ProviderHealthSnapshot {
        let state = self.inner.lock().await;
        state
            .get(&provider_id)
            .cloned()
            .unwrap_or_default()
            .snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        ProviderCircuitStore, ProviderHealthSnapshot, ProviderHealthState, ProviderHealthStatus,
    };
    use super::MemoryProviderCircuitStore;
    use crate::config::ProviderGovernanceConfig;
    use std::time::{Duration, Instant};

    #[test]
    fn provider_health_opens_after_threshold_failures() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 2,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let now_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();

        state.record_failure(&config, now, now_ms, "timeout".to_string());
        assert_eq!(state.status, ProviderHealthStatus::Healthy);
        assert_eq!(state.consecutive_failures, 1);
        assert_eq!(state.last_failure_at, Some(now_ms));
        assert!(state.opened_at.is_none());

        state.record_failure(&config, now, now_ms + 1_000, "another timeout".to_string());
        assert_eq!(state.status, ProviderHealthStatus::Open);
        assert_eq!(state.consecutive_failures, 2);
        assert_eq!(state.last_failure_at, Some(now_ms + 1_000));
        assert_eq!(state.opened_at, Some(now_ms + 1_000));
        assert!(state.opened_at.is_some());
        assert!(state.opened_at_instant.is_some());
    }

    #[test]
    fn provider_health_transitions_to_half_open_after_cooldown() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let now_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, now, now_ms, "timeout".to_string());

        assert_eq!(
            state.allow_request(&config, now + Duration::from_secs(10)),
            Err(Some(Duration::from_secs(20)))
        );
        assert_eq!(state.status, ProviderHealthStatus::Open);

        assert_eq!(
            state.allow_request(&config, now + Duration::from_secs(31)),
            Ok(())
        );
        assert_eq!(state.status, ProviderHealthStatus::HalfOpen);
        assert!(state.half_open_probe_in_flight);
    }

    #[test]
    fn provider_health_half_open_success_closes_circuit() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let open_at_ms = 1_700_000_000_000;
        let recover_at_ms = open_at_ms + 31_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, now, open_at_ms, "timeout".to_string());
        state
            .allow_request(&config, now + Duration::from_secs(31))
            .expect("half-open probe should be allowed");

        state.record_success(recover_at_ms);
        assert_eq!(state.status, ProviderHealthStatus::Healthy);
        assert_eq!(state.consecutive_failures, 0);
        assert!(!state.half_open_probe_in_flight);
        assert_eq!(state.last_recovered_at, Some(recover_at_ms));
        assert_eq!(state.last_failure_at, Some(open_at_ms));
        assert!(state.opened_at.is_none());
        assert!(state.opened_at_instant.is_none());
        assert!(state.last_error.is_none());
    }

    #[test]
    fn provider_health_half_open_failure_reopens_circuit() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 3,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let first_failure_at_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, now, first_failure_at_ms, "timeout".to_string());
        state.record_failure(
            &config,
            now,
            first_failure_at_ms + 1_000,
            "timeout".to_string(),
        );
        state.record_failure(
            &config,
            now,
            first_failure_at_ms + 2_000,
            "timeout".to_string(),
        );
        state
            .allow_request(&config, now + Duration::from_secs(31))
            .expect("half-open probe should be allowed");

        state.record_failure(
            &config,
            now + Duration::from_secs(31),
            first_failure_at_ms + 31_000,
            "half-open timeout".to_string(),
        );
        assert_eq!(state.status, ProviderHealthStatus::Open);
        assert!(!state.half_open_probe_in_flight);
        assert_eq!(state.last_failure_at, Some(first_failure_at_ms + 31_000));
        assert_eq!(state.opened_at, Some(first_failure_at_ms + 31_000));
    }

    #[test]
    fn provider_health_snapshot_exposes_runtime_timestamps() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let open_at_ms = 1_700_000_000_000;
        let recover_at_ms = open_at_ms + 31_000;
        let mut state = ProviderHealthState::default();

        state.record_failure(&config, now, open_at_ms, "timeout".to_string());
        let open_snapshot = state.snapshot();
        assert_eq!(open_snapshot.opened_at, Some(open_at_ms));
        assert_eq!(open_snapshot.last_failure_at, Some(open_at_ms));
        assert_eq!(open_snapshot.last_recovered_at, None);
        assert_eq!(open_snapshot.last_error.as_deref(), Some("timeout"));

        state
            .allow_request(&config, now + Duration::from_secs(31))
            .expect("half-open probe should be allowed");
        state.record_success(recover_at_ms);

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
            store.snapshot(404).await,
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
