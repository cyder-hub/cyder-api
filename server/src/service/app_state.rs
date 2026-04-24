use std::sync::Arc;

use axum::Router;
use thiserror::Error;

use crate::service::cache::CacheError;

#[cfg(test)]
use crate::database::TestDbContext;

use super::admin::AdminServices;
use super::catalog::CatalogService;
use super::infra::AppInfra;
use super::runtime::{ApiKeyGovernanceService, ProviderCircuitService, ProviderKeySelector};

#[derive(Clone)]
pub struct AppState {
    pub infra: Arc<AppInfra>,
    pub catalog: Arc<CatalogService>,
    pub admin: Arc<AdminServices>,
    pub provider_key_selector: Arc<ProviderKeySelector>,
    pub api_key_governance: Arc<ApiKeyGovernanceService>,
    pub provider_circuit: Arc<ProviderCircuitService>,
}

impl AppState {
    #[cfg(not(test))]
    pub async fn new() -> Self {
        Self::new_with_test_db_context().await
    }

    #[cfg(test)]
    pub async fn new() -> Self {
        Self::new_with_test_db_context(None).await
    }

    #[cfg(test)]
    pub(crate) async fn new_for_test(test_db_context: TestDbContext) -> Self {
        Self::new_with_test_db_context(Some(test_db_context)).await
    }

    async fn new_with_test_db_context(#[cfg(test)] test_db_context: Option<TestDbContext>) -> Self {
        #[cfg(test)]
        let force_memory_cache = test_db_context.is_some();
        #[cfg(not(test))]
        let force_memory_cache = false;

        #[cfg(test)]
        let infra = match test_db_context.clone() {
            Some(test_db_context) => Arc::new(AppInfra::new_for_test(test_db_context).await),
            None => Arc::new(AppInfra::new().await),
        };

        #[cfg(not(test))]
        let infra = Arc::new(AppInfra::new().await);

        let catalog = Arc::new(CatalogService::new(force_memory_cache).await);
        let admin = Arc::new(AdminServices::new(Arc::clone(&catalog)));
        let provider_key_selector = ProviderKeySelector::new(Arc::clone(&catalog)).await;

        Self {
            infra,
            catalog,
            admin,
            provider_key_selector,
            api_key_governance: Arc::new(ApiKeyGovernanceService::default()),
            provider_circuit: Arc::new(ProviderCircuitService::default()),
        }
    }

    pub async fn flush_proxy_logs(&self) {
        self.infra.flush_proxy_logs().await;
    }
}

#[derive(Debug, Error)]
pub enum AppStoreError {
    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Resource already exists: {0}")]
    AlreadyExists(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("Lock error: {0}")]
    LockError(String),
}

impl From<CacheError> for AppStoreError {
    fn from(e: CacheError) -> Self {
        match e {
            CacheError::NotFound(msg) => AppStoreError::NotFound(msg),
            CacheError::AlreadyExists(msg) => AppStoreError::AlreadyExists(msg),
            _ => AppStoreError::CacheError(e.to_string()),
        }
    }
}

pub async fn create_app_state() -> Arc<AppState> {
    create_configured_app_state(AppState::new().await).await
}

#[cfg(test)]
pub(crate) async fn create_test_app_state(test_db_context: TestDbContext) -> Arc<AppState> {
    create_configured_app_state(AppState::new_for_test(test_db_context).await).await
}

async fn create_configured_app_state(app_state: AppState) -> Arc<AppState> {
    let app_state = Arc::new(app_state);
    app_state.catalog.clear_cache().await;
    app_state.catalog.reload().await;
    app_state
}

pub type StateRouter = Router<Arc<AppState>>;

pub fn create_state_router() -> StateRouter {
    Router::<Arc<AppState>>::new()
}

#[cfg(test)]
mod tests {
    use super::super::admin::AdminServices;
    use super::AppState;
    use crate::service::catalog::CatalogService;
    use crate::service::infra::AppInfra;
    use crate::service::runtime::{
        ApiKeyGovernanceService, ProviderCircuitService, ProviderKeySelector,
    };
    use std::sync::Arc;

    async fn test_app_state() -> AppState {
        let catalog = Arc::new(CatalogService::new(true).await);
        let admin = Arc::new(AdminServices::new(Arc::clone(&catalog)));
        let provider_key_selector = ProviderKeySelector::new(Arc::clone(&catalog)).await;

        AppState {
            infra: Arc::new(AppInfra::new().await),
            catalog,
            admin,
            provider_key_selector,
            api_key_governance: Arc::new(ApiKeyGovernanceService::default()),
            provider_circuit: Arc::new(ProviderCircuitService::default()),
        }
    }

    #[tokio::test]
    async fn app_state_exposes_target_module_handles() {
        let app_state = test_app_state().await;

        assert_eq!(Arc::strong_count(&app_state.infra), 1);
        assert_eq!(Arc::strong_count(&app_state.catalog), 3);
        assert_eq!(Arc::strong_count(&app_state.admin), 1);
        assert_eq!(Arc::strong_count(&app_state.provider_key_selector), 1);
        assert_eq!(Arc::strong_count(&app_state.api_key_governance), 1);
        assert_eq!(Arc::strong_count(&app_state.provider_circuit), 1);
    }
}
