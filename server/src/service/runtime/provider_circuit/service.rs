use std::sync::Arc;

use crate::config::{CONFIG, ProviderGovernanceConfig};

use super::memory_store::MemoryProviderCircuitStore;
use super::types::{
    ProviderCircuitDecision, ProviderCircuitError, ProviderCircuitProbePermit,
    ProviderCircuitStore, ProviderHealthSnapshot,
};

pub struct ProviderCircuitService {
    store: Arc<dyn ProviderCircuitStore>,
    config: ProviderGovernanceConfig,
}

impl ProviderCircuitService {
    pub fn new(store: Arc<dyn ProviderCircuitStore>) -> Self {
        Self::new_with_config(store, CONFIG.provider_governance.clone())
    }

    pub fn new_with_config(
        store: Arc<dyn ProviderCircuitStore>,
        config: ProviderGovernanceConfig,
    ) -> Self {
        Self { store, config }
    }

    pub fn new_memory() -> Self {
        Self::new(Arc::new(MemoryProviderCircuitStore::default()))
    }

    pub async fn allow_provider_request(
        &self,
        provider_id: i64,
    ) -> Result<ProviderCircuitDecision, ProviderCircuitError> {
        if !self.config.is_enabled() {
            return Ok(ProviderCircuitDecision::allowed(
                ProviderHealthSnapshot::synthetic_healthy(),
                None,
            ));
        }

        self.store.allow_request(provider_id, &self.config).await
    }

    pub async fn record_provider_success(
        &self,
        provider_id: i64,
        permit: Option<&ProviderCircuitProbePermit>,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
        if !self.config.is_enabled() {
            return Ok(ProviderHealthSnapshot::synthetic_healthy());
        }

        self.store
            .record_success(provider_id, &self.config, permit)
            .await
    }

    pub async fn record_provider_failure(
        &self,
        provider_id: i64,
        error_message: String,
        permit: Option<&ProviderCircuitProbePermit>,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
        if !self.config.is_enabled() {
            return Ok(ProviderHealthSnapshot::synthetic_healthy());
        }

        self.store
            .record_failure(provider_id, &self.config, error_message, permit)
            .await
    }

    pub async fn get_provider_health_snapshot(
        &self,
        provider_id: i64,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
        if !self.config.is_enabled() {
            return Ok(ProviderHealthSnapshot::synthetic_healthy());
        }

        self.store.snapshot(provider_id).await
    }
}

impl Default for ProviderCircuitService {
    fn default() -> Self {
        Self::new_memory()
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderCircuitService;
    use crate::config::{CONFIG, ProviderGovernanceConfig};
    use crate::service::runtime::provider_circuit::{
        MemoryProviderCircuitStore, ProviderCircuitStore, ProviderHealthSnapshot,
        ProviderHealthStatus,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn service_exposes_circuit_flow_without_app_state() {
        if !CONFIG.provider_governance.is_enabled() {
            return;
        }

        let service = ProviderCircuitService::default();
        let provider_id = 17;

        let initial = service
            .get_provider_health_snapshot(provider_id)
            .await
            .expect("snapshot should succeed");
        assert_eq!(initial.status, ProviderHealthStatus::Healthy);

        let opened = service
            .record_provider_failure(provider_id, "timeout".to_string(), None)
            .await;
        let opened = opened.expect("record failure should succeed");
        assert!(matches!(
            opened.status,
            ProviderHealthStatus::Healthy | ProviderHealthStatus::Open
        ));

        let _ = service
            .record_provider_success(provider_id, None)
            .await
            .expect("record success should succeed");
        let recovered = service
            .get_provider_health_snapshot(provider_id)
            .await
            .expect("snapshot should succeed");
        assert_eq!(recovered.status, ProviderHealthStatus::Healthy);
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
        let store = Arc::new(MemoryProviderCircuitStore::default());
        let provider_id = 21;
        store
            .record_failure(provider_id, &enabled_config, "timeout".to_string(), None)
            .await
            .expect("seed failure should open circuit");
        assert_eq!(
            store
                .snapshot(provider_id)
                .await
                .expect("seed snapshot should load")
                .status,
            ProviderHealthStatus::Open
        );

        let service = ProviderCircuitService::new_with_config(store.clone(), disabled_config);
        let allow = service
            .allow_provider_request(provider_id)
            .await
            .expect("disabled allow should succeed");
        assert!(allow.allowed);
        assert_eq!(allow.snapshot, ProviderHealthSnapshot::synthetic_healthy());
        assert!(allow.probe_permit.is_none());

        let failure = service
            .record_provider_failure(provider_id, "new timeout".to_string(), None)
            .await
            .expect("disabled failure should be a no-op");
        assert_eq!(failure, ProviderHealthSnapshot::synthetic_healthy());
        let success = service
            .record_provider_success(provider_id, None)
            .await
            .expect("disabled success should be a no-op");
        assert_eq!(success, ProviderHealthSnapshot::synthetic_healthy());
        let snapshot = service
            .get_provider_health_snapshot(provider_id)
            .await
            .expect("disabled snapshot should be synthetic");
        assert_eq!(snapshot, ProviderHealthSnapshot::synthetic_healthy());

        assert_eq!(
            store
                .snapshot(provider_id)
                .await
                .expect("underlying stale state should remain untouched")
                .status,
            ProviderHealthStatus::Open
        );
    }
}
