use std::sync::Arc;

use axum::Router;
use chrono::Utc;
use thiserror::Error;

use crate::config::RuntimeStateBackendType;
use crate::service::cache::CacheError;

#[cfg(test)]
use crate::database::TestDbContext;

use super::admin::AdminServices;
use super::catalog::CatalogService;
use super::infra::AppInfra;
use super::runtime::{
    ApiKeyGovernanceService, ProviderCircuitService, ProviderKeySelector,
    RuntimeStateBackendBundle, RuntimeStateBackendError, RuntimeStateBackendOperatorStatus,
    RuntimeStateBackendStatus,
};

const RUNTIME_STATE_BACKEND_HEALTHCHECK_PROVIDER_ID: i64 = 0;

#[derive(Clone)]
pub struct AppState {
    pub infra: Arc<AppInfra>,
    pub catalog: Arc<CatalogService>,
    pub admin: Arc<AdminServices>,
    pub provider_key_selector: Arc<ProviderKeySelector>,
    pub api_key_governance: Arc<ApiKeyGovernanceService>,
    pub provider_circuit: Arc<ProviderCircuitService>,
    pub runtime_backend_status: Arc<RuntimeStateBackendStatus>,
}

impl AppState {
    #[cfg(not(test))]
    pub async fn new() -> Self {
        Self::try_new_with_test_db_context()
            .await
            .expect("failed to initialize app state")
    }

    #[cfg(test)]
    pub async fn new() -> Self {
        Self::try_new_with_test_db_context(None)
            .await
            .expect("failed to initialize app state")
    }

    #[cfg(test)]
    pub(crate) async fn new_for_test(test_db_context: TestDbContext) -> Self {
        Self::try_new_with_test_db_context(Some(test_db_context))
            .await
            .expect("failed to initialize test app state")
    }

    async fn try_new_with_test_db_context(
        #[cfg(test)] test_db_context: Option<TestDbContext>,
    ) -> Result<Self, RuntimeStateBackendError> {
        #[cfg(test)]
        let force_memory_cache = test_db_context.is_some();
        #[cfg(not(test))]
        let force_memory_cache = false;
        let force_memory_runtime_state = force_memory_cache;

        #[cfg(test)]
        let infra = match test_db_context.clone() {
            Some(test_db_context) => Arc::new(AppInfra::new_for_test(test_db_context).await),
            None => Arc::new(AppInfra::new().await),
        };

        #[cfg(not(test))]
        let infra = Arc::new(AppInfra::new().await);

        let runtime_backend = RuntimeStateBackendBundle::from_config(
            &crate::config::CONFIG,
            force_memory_runtime_state,
        )
        .await?;
        let catalog = Arc::new(CatalogService::new(force_memory_cache).await);
        let admin = Arc::new(AdminServices::new(Arc::clone(&catalog)));
        let provider_key_selector = ProviderKeySelector::new(
            Arc::clone(&catalog),
            Arc::clone(&runtime_backend.provider_key_cursor_store),
        )
        .await;

        Ok(Self {
            infra,
            catalog,
            admin,
            provider_key_selector,
            api_key_governance: Arc::clone(&runtime_backend.api_key_governance),
            provider_circuit: Arc::clone(&runtime_backend.provider_circuit),
            runtime_backend_status: Arc::new(runtime_backend.status),
        })
    }

    pub async fn flush_proxy_logs(&self) {
        self.infra.flush_proxy_logs().await;
    }

    pub async fn runtime_state_backend_operator_status(&self) -> RuntimeStateBackendOperatorStatus {
        let checked_at = Utc::now().timestamp_millis();
        let runtime_read_error =
            if self.runtime_backend_status.effective_backend == RuntimeStateBackendType::Redis {
                match self
                    .provider_circuit
                    .get_provider_health_snapshot(RUNTIME_STATE_BACKEND_HEALTHCHECK_PROVIDER_ID)
                    .await
                {
                    Ok(_) => None,
                    Err(err) => {
                        let error = err.to_string();
                        crate::warn_event!(
                            "runtime_state.read_failed",
                            component = "provider_circuit_healthcheck",
                            backend = self.runtime_backend_status.effective_backend.as_str(),
                            error = &error,
                        );
                        Some(error)
                    }
                }
            } else {
                None
            };
        let catalog_status = self.catalog.backend_status();

        self.runtime_backend_status.to_operator_status(
            catalog_status.configured_backend,
            catalog_status.effective_backend,
            catalog_status.fallback_reason,
            runtime_read_error,
            checked_at,
        )
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
    use crate::config::{CONFIG, RuntimeStateBackendType};
    use crate::database::TestDbContext;
    use crate::service::catalog::CatalogService;
    use crate::service::infra::AppInfra;
    use crate::service::runtime::{ProviderKeySelector, RuntimeStateBackendBundle};
    use std::sync::Arc;

    async fn test_app_state() -> AppState {
        let catalog = Arc::new(CatalogService::new(true).await);
        let admin = Arc::new(AdminServices::new(Arc::clone(&catalog)));
        let runtime_backend = RuntimeStateBackendBundle::from_config(&CONFIG, true)
            .await
            .expect("test runtime backend should initialize");
        let provider_key_selector = ProviderKeySelector::new(
            Arc::clone(&catalog),
            Arc::clone(&runtime_backend.provider_key_cursor_store),
        )
        .await;

        AppState {
            infra: Arc::new(AppInfra::new().await),
            catalog,
            admin,
            provider_key_selector,
            api_key_governance: Arc::clone(&runtime_backend.api_key_governance),
            provider_circuit: Arc::clone(&runtime_backend.provider_circuit),
            runtime_backend_status: Arc::new(runtime_backend.status),
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
        assert_eq!(Arc::strong_count(&app_state.runtime_backend_status), 1);
    }

    #[tokio::test]
    async fn new_for_test_injects_memory_runtime_backend_bundle() {
        let app_state =
            AppState::new_for_test(TestDbContext::new_sqlite("app-state-runtime-bundle.sqlite"))
                .await;

        assert_eq!(
            app_state.runtime_backend_status.effective_backend,
            RuntimeStateBackendType::Memory
        );
        assert_eq!(
            app_state.runtime_backend_status.fallback_reason.as_deref(),
            Some("test_isolation")
        );
    }

    #[tokio::test]
    async fn app_state_operator_status_exposes_memory_runtime_backend() {
        let app_state = AppState::new_for_test(TestDbContext::new_sqlite(
            "app-state-operator-status.sqlite",
        ))
        .await;

        let status = app_state.runtime_state_backend_operator_status().await;

        assert_eq!(status.runtime_effective_backend, "memory");
        assert_eq!(status.catalog_cache_backend, "memory");
        assert!(!status.runtime_shared);
        assert!(status.last_error.is_none());
    }
}
