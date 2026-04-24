use std::sync::Arc;

use crate::config::CONFIG;

use super::memory_store::MemoryProviderCircuitStore;
use super::types::{ProviderCircuitStore, ProviderHealthSnapshot};

pub struct ProviderCircuitService {
    store: Arc<dyn ProviderCircuitStore>,
}

impl ProviderCircuitService {
    pub fn new(store: Arc<dyn ProviderCircuitStore>) -> Self {
        Self { store }
    }

    pub fn new_memory() -> Self {
        Self::new(Arc::new(MemoryProviderCircuitStore::default()))
    }

    pub async fn allow_provider_request(
        &self,
        provider_id: i64,
    ) -> Result<ProviderHealthSnapshot, Option<std::time::Duration>> {
        self.store
            .allow_request(provider_id, &CONFIG.provider_governance)
            .await
    }

    pub async fn record_provider_success(&self, provider_id: i64) -> ProviderHealthSnapshot {
        self.store.record_success(provider_id).await
    }

    pub async fn record_provider_failure(
        &self,
        provider_id: i64,
        error_message: String,
    ) -> ProviderHealthSnapshot {
        self.store
            .record_failure(provider_id, &CONFIG.provider_governance, error_message)
            .await
    }

    pub async fn get_provider_health_snapshot(&self, provider_id: i64) -> ProviderHealthSnapshot {
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
    use crate::config::CONFIG;
    use crate::service::runtime::provider_circuit::ProviderHealthStatus;

    #[tokio::test]
    async fn service_exposes_circuit_flow_without_app_state() {
        if !CONFIG.provider_governance.is_enabled() {
            return;
        }

        let service = ProviderCircuitService::default();
        let provider_id = 17;

        let initial = service.get_provider_health_snapshot(provider_id).await;
        assert_eq!(initial.status, ProviderHealthStatus::Healthy);

        let opened = service
            .record_provider_failure(provider_id, "timeout".to_string())
            .await;
        assert!(matches!(
            opened.status,
            ProviderHealthStatus::Healthy | ProviderHealthStatus::Open
        ));

        let _ = service.record_provider_success(provider_id).await;
        let recovered = service.get_provider_health_snapshot(provider_id).await;
        assert_eq!(recovered.status, ProviderHealthStatus::Healthy);
    }
}
