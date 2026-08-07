use std::sync::Arc;

use crate::config::ProviderGovernanceConfig;

use super::memory_store::MemorySourceCircuitStore;
use super::types::{
    SourceCircuitDecision, SourceCircuitError, SourceCircuitProbePermit, SourceCircuitStore,
    SourceHealthSnapshot,
};

pub struct SourceCircuitService {
    store: Arc<dyn SourceCircuitStore>,
    config: ProviderGovernanceConfig,
}

impl SourceCircuitService {
    pub fn new(store: Arc<dyn SourceCircuitStore>) -> Self {
        Self::new_with_config(store, ProviderGovernanceConfig::default())
    }

    pub fn new_with_config(
        store: Arc<dyn SourceCircuitStore>,
        config: ProviderGovernanceConfig,
    ) -> Self {
        Self { store, config }
    }

    pub fn new_memory() -> Self {
        Self::new(Arc::new(MemorySourceCircuitStore::default()))
    }

    pub async fn allow_source_request(
        &self,
        source_id: i64,
    ) -> Result<SourceCircuitDecision, SourceCircuitError> {
        if !self.config.is_enabled() {
            return Ok(SourceCircuitDecision::allowed(
                SourceHealthSnapshot::synthetic_healthy(),
                None,
            ));
        }

        self.store.allow_request(source_id, &self.config).await
    }

    pub async fn record_source_success(
        &self,
        source_id: i64,
        permit: Option<&SourceCircuitProbePermit>,
    ) -> Result<SourceHealthSnapshot, SourceCircuitError> {
        if !self.config.is_enabled() {
            return Ok(SourceHealthSnapshot::synthetic_healthy());
        }

        self.store
            .record_success(source_id, &self.config, permit)
            .await
    }

    pub async fn record_source_failure(
        &self,
        source_id: i64,
        error_message: String,
        permit: Option<&SourceCircuitProbePermit>,
    ) -> Result<SourceHealthSnapshot, SourceCircuitError> {
        if !self.config.is_enabled() {
            return Ok(SourceHealthSnapshot::synthetic_healthy());
        }

        self.store
            .record_failure(source_id, &self.config, error_message, permit)
            .await
    }

    pub async fn release_source_probe(
        &self,
        source_id: i64,
        permit: Option<&SourceCircuitProbePermit>,
    ) -> Result<SourceHealthSnapshot, SourceCircuitError> {
        if !self.config.is_enabled() {
            return Ok(SourceHealthSnapshot::synthetic_healthy());
        }

        self.store
            .release_probe(source_id, &self.config, permit)
            .await
    }

    pub async fn clear_source(&self, source_id: i64) -> Result<(), SourceCircuitError> {
        self.store.clear(source_id).await
    }

    pub async fn get_source_health_snapshot(
        &self,
        source_id: i64,
    ) -> Result<SourceHealthSnapshot, SourceCircuitError> {
        if !self.config.is_enabled() {
            return Ok(SourceHealthSnapshot::synthetic_healthy());
        }

        self.store.snapshot(source_id).await
    }
}

impl Default for SourceCircuitService {
    fn default() -> Self {
        Self::new_memory()
    }
}

#[cfg(test)]
mod tests {
    use super::SourceCircuitService;
    use crate::config::ProviderGovernanceConfig;
    use crate::service::runtime::source_circuit::{
        MemorySourceCircuitStore, SourceCircuitStore, SourceHealthSnapshot, SourceHealthStatus,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn service_exposes_circuit_flow_without_app_state() {
        let service = SourceCircuitService::default();
        let source_id = 17;

        let initial = service
            .get_source_health_snapshot(source_id)
            .await
            .expect("snapshot should succeed");
        assert_eq!(initial.status, SourceHealthStatus::Healthy);

        let opened = service
            .record_source_failure(source_id, "timeout".to_string(), None)
            .await;
        let opened = opened.expect("record failure should succeed");
        assert!(matches!(
            opened.status,
            SourceHealthStatus::Healthy | SourceHealthStatus::Open
        ));

        let _ = service
            .record_source_success(source_id, None)
            .await
            .expect("record success should succeed");
        let recovered = service
            .get_source_health_snapshot(source_id)
            .await
            .expect("snapshot should succeed");
        assert_eq!(recovered.status, SourceHealthStatus::Healthy);
    }

    #[tokio::test]
    async fn service_returns_synthetic_healthy_without_touching_store_when_governance_disabled() {
        let enabled_config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let disabled_config = ProviderGovernanceConfig {
            enabled: false,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let store = Arc::new(MemorySourceCircuitStore::default());
        let source_id = 21;
        store
            .record_failure(source_id, &enabled_config, "timeout".to_string(), None)
            .await
            .expect("seed failure should open circuit");
        assert_eq!(
            store
                .snapshot(source_id)
                .await
                .expect("seed snapshot should load")
                .status,
            SourceHealthStatus::Open
        );

        let service = SourceCircuitService::new_with_config(store.clone(), disabled_config);
        let allow = service
            .allow_source_request(source_id)
            .await
            .expect("disabled allow should succeed");
        assert!(allow.allowed);
        assert_eq!(allow.snapshot, SourceHealthSnapshot::synthetic_healthy());
        assert!(allow.probe_permit.is_none());

        let failure = service
            .record_source_failure(source_id, "new timeout".to_string(), None)
            .await
            .expect("disabled failure should be a no-op");
        assert_eq!(failure, SourceHealthSnapshot::synthetic_healthy());
        let success = service
            .record_source_success(source_id, None)
            .await
            .expect("disabled success should be a no-op");
        assert_eq!(success, SourceHealthSnapshot::synthetic_healthy());
        let snapshot = service
            .get_source_health_snapshot(source_id)
            .await
            .expect("disabled snapshot should be synthetic");
        assert_eq!(snapshot, SourceHealthSnapshot::synthetic_healthy());

        assert_eq!(
            store
                .snapshot(source_id)
                .await
                .expect("underlying stale state should remain untouched")
                .status,
            SourceHealthStatus::Open
        );
    }

    #[tokio::test]
    async fn service_uses_startup_threshold_for_failures() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 2,
            open_cooldown_seconds: 30,
        };
        let service = SourceCircuitService::new_with_config(
            Arc::new(MemorySourceCircuitStore::default()),
            config,
        );
        let source_id = 31;

        let first = service
            .record_source_failure(source_id, "timeout".to_string(), None)
            .await
            .expect("first failure should be recorded");
        assert_eq!(first.status, SourceHealthStatus::Healthy);
        assert_eq!(first.consecutive_failures, 1);

        let second = service
            .record_source_failure(source_id, "timeout again".to_string(), None)
            .await
            .expect("second failure should use startup threshold");

        assert_eq!(second.status, SourceHealthStatus::Open);
        assert_eq!(second.consecutive_failures, 2);
    }

    #[tokio::test]
    async fn clear_source_removes_state_even_when_governance_is_disabled() {
        let enabled_config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let disabled_config = ProviderGovernanceConfig {
            enabled: false,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let store = Arc::new(MemorySourceCircuitStore::default());
        let source_id = 32;
        store
            .record_failure(source_id, &enabled_config, "timeout".to_string(), None)
            .await
            .expect("seed failure should open circuit");
        assert_eq!(
            store
                .snapshot(source_id)
                .await
                .expect("seed snapshot should load")
                .status,
            SourceHealthStatus::Open
        );

        let service = SourceCircuitService::new_with_config(store.clone(), disabled_config);
        service
            .clear_source(source_id)
            .await
            .expect("clear should not depend on governance being enabled");

        assert_eq!(
            store
                .snapshot(source_id)
                .await
                .expect("cleared snapshot should load"),
            SourceHealthSnapshot::synthetic_healthy()
        );
    }
}
