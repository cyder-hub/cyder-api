use std::sync::Arc;

use axum::Router;
use chrono::Utc;
use thiserror::Error;

use crate::config::{CONFIG, RuntimeStateBackendType};
use crate::proxy::logging::RequestLogPersistedSink;
use crate::service::cache::CacheError;
use crate::service::metrics::MetricsService;
use crate::service::secret_encryption::SecretEncryptionService;

#[cfg(test)]
use crate::database::TestDbContext;

use super::admin::AdminServices;
use super::catalog::CatalogService;
use super::infra::AppInfra;
use super::runtime::{
    ApiKeyGovernanceService, ProviderKeySelector, ReasoningContinuationStore,
    RuntimeStateBackendBundle, RuntimeStateBackendError, RuntimeStateBackendOperatorStatus,
    RuntimeStateBackendStatus, SourceCircuitService,
};

const RUNTIME_STATE_BACKEND_HEALTHCHECK_SOURCE_ID: i64 = 0;

#[derive(Clone)]
pub struct AppState {
    pub infra: Arc<AppInfra>,
    pub catalog: Arc<CatalogService>,
    pub admin: Arc<AdminServices>,
    pub provider_key_selector: Arc<ProviderKeySelector>,
    pub api_key_governance: Arc<ApiKeyGovernanceService>,
    pub source_circuit: Arc<SourceCircuitService>,
    pub reasoning_continuation_store: Arc<dyn ReasoningContinuationStore>,
    pub metrics: Arc<MetricsService>,
    pub runtime_backend_status: Arc<RuntimeStateBackendStatus>,
    pub secret_encryption: Arc<SecretEncryptionService>,
    pub manager_auth_browser_origin: Option<String>,
    pub base_path: String,
    pub max_body_size: usize,
    pub timezone: Option<String>,
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
        let config = CONFIG.clone();
        #[cfg(test)]
        let config = {
            let mut config = config;
            if test_db_context.is_some() {
                config.manager_auth.browser_origin = Some("http://127.0.0.1:29528".to_string());
            }
            config
        };

        #[cfg(test)]
        let infra = Arc::new(
            AppInfra::new_with_config(
                config.outbound_http.clone(),
                config.proxy_request.clone(),
                config.proxy.as_ref().map(|proxy| proxy.expose().to_owned()),
                test_db_context.clone(),
            )
            .await,
        );

        #[cfg(not(test))]
        let infra = Arc::new(
            AppInfra::new_with_config(
                config.outbound_http.clone(),
                config.proxy_request.clone(),
                config.proxy.as_ref().map(|proxy| proxy.expose().to_owned()),
            )
            .await,
        );
        let metrics = Arc::new(MetricsService::new(config.metrics.clone()));
        let metrics_sink: Arc<dyn RequestLogPersistedSink> = metrics.clone();
        infra
            .log_manager()
            .set_request_log_persisted_sink(metrics_sink);

        let runtime_backend =
            RuntimeStateBackendBundle::from_config(&config, force_memory_runtime_state).await?;
        let catalog = Arc::new(CatalogService::new(force_memory_cache).await);
        let secret_encryption = Arc::new(SecretEncryptionService::from_config(
            &config.secret_encryption,
        ));
        let admin = Arc::new(AdminServices::new(
            Arc::clone(&catalog),
            Arc::clone(&secret_encryption),
        ));
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
            source_circuit: Arc::clone(&runtime_backend.source_circuit),
            reasoning_continuation_store: Arc::clone(&runtime_backend.reasoning_continuation_store),
            metrics,
            runtime_backend_status: Arc::new(runtime_backend.status),
            secret_encryption,
            manager_auth_browser_origin: config.manager_auth.browser_origin.clone(),
            base_path: config.base_path.clone(),
            max_body_size: config.max_body_size,
            timezone: config.timezone.clone(),
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
                    .source_circuit
                    .get_source_health_snapshot(RUNTIME_STATE_BACKEND_HEALTHCHECK_SOURCE_ID)
                    .await
                {
                    Ok(_) => None,
                    Err(err) => {
                        let error = err.to_string();
                        crate::warn_event!(
                            "runtime_state.read_failed",
                            component = "source_circuit_healthcheck",
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

    #[cfg(not(test))]
    pub fn start_background_workers(self: &Arc<Self>) {
        self.spawn_metrics_reconciliation_worker();
        self.spawn_manager_session_cleanup_worker();
    }

    #[cfg(not(test))]
    fn spawn_metrics_reconciliation_worker(self: &Arc<Self>) {
        if !self.metrics.config().enabled {
            return;
        }
        let app_state = Arc::clone(self);
        let interval_seconds = app_state
            .metrics
            .config()
            .reconciliation_worker_interval_seconds
            .max(1);
        self.infra.spawn_background_task(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(interval_seconds));
            loop {
                interval.tick().await;
                let result = app_state.metrics.tick_reconciliation_worker().await;
                if result.failed > 0 {
                    crate::warn_event!(
                        "metrics.reconciliation_worker_tick_degraded",
                        processed = result.processed,
                        skipped = result.skipped,
                        failed = result.failed
                    );
                } else if result.processed > 0 || result.skipped > 0 {
                    crate::debug_event!(
                        "metrics.reconciliation_worker_tick_completed",
                        processed = result.processed,
                        skipped = result.skipped
                    );
                }
            }
        });
    }

    #[cfg(not(test))]
    fn spawn_manager_session_cleanup_worker(self: &Arc<Self>) {
        let app_state = Arc::clone(self);
        self.infra.spawn_background_task(async move {
            let period = std::time::Duration::from_secs(60 * 60);
            let mut interval =
                tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            loop {
                interval.tick().await;
                app_state.tick_manager_session_cleanup();
            }
        });
    }

    fn tick_manager_session_cleanup(&self) -> Option<usize> {
        match self.admin.auth.cleanup_expired_instances() {
            Ok(removed) => {
                if removed > 0 {
                    crate::debug_event!(
                        "manager.auth.session_cleanup_completed",
                        removed_sessions = removed
                    );
                }
                Some(removed)
            }
            Err(_) => {
                crate::warn_event!("manager.auth.session_cleanup_failed", reason = "storage");
                None
            }
        }
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
    let app_state = create_configured_app_state(AppState::new().await).await;
    #[cfg(not(test))]
    app_state.start_background_workers();
    app_state
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
    use crate::database::manager_auth_instance::ManagerAuthInstance;
    use crate::database::{DbConnection, TestDbContext, get_connection};
    use crate::service::catalog::CatalogService;
    use crate::service::infra::AppInfra;
    use crate::service::metrics::MetricsService;
    use crate::service::runtime::{ProviderKeySelector, RuntimeStateBackendBundle};
    use crate::service::secret_encryption::SecretEncryptionService;
    use diesel::RunQueryDsl;
    use std::sync::Arc;

    async fn test_app_state() -> AppState {
        let catalog = Arc::new(CatalogService::new(true).await);
        let config = CONFIG.clone();
        let secret_encryption = Arc::new(SecretEncryptionService::from_config(
            &config.secret_encryption,
        ));
        let admin = Arc::new(AdminServices::new(
            Arc::clone(&catalog),
            Arc::clone(&secret_encryption),
        ));
        let infra = Arc::new(
            AppInfra::new_with_config(
                config.outbound_http.clone(),
                config.proxy_request.clone(),
                config.proxy.as_ref().map(|proxy| proxy.expose().to_owned()),
                None,
            )
            .await,
        );
        let metrics = Arc::new(MetricsService::new(CONFIG.metrics.clone()));
        let runtime_backend = RuntimeStateBackendBundle::from_config(&CONFIG, true)
            .await
            .expect("test runtime backend should initialize");
        let provider_key_selector = ProviderKeySelector::new(
            Arc::clone(&catalog),
            Arc::clone(&runtime_backend.provider_key_cursor_store),
        )
        .await;

        AppState {
            infra,
            catalog,
            admin,
            provider_key_selector,
            api_key_governance: Arc::clone(&runtime_backend.api_key_governance),
            source_circuit: Arc::clone(&runtime_backend.source_circuit),
            reasoning_continuation_store: Arc::clone(&runtime_backend.reasoning_continuation_store),
            metrics,
            runtime_backend_status: Arc::new(runtime_backend.status),
            secret_encryption,
            manager_auth_browser_origin: config.manager_auth.browser_origin.clone(),
            base_path: config.base_path.clone(),
            max_body_size: config.max_body_size,
            timezone: config.timezone,
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
        assert_eq!(Arc::strong_count(&app_state.source_circuit), 1);
        assert_eq!(
            Arc::strong_count(&app_state.reasoning_continuation_store),
            1
        );
        assert_eq!(Arc::strong_count(&app_state.runtime_backend_status), 1);
        assert!(Arc::ptr_eq(
            &app_state.secret_encryption,
            &app_state.admin.secret_encryption,
        ));
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
        assert!(status.last_error.is_none());
    }

    #[tokio::test]
    async fn app_state_exposes_static_request_settings() {
        let app_state =
            AppState::new_for_test(TestDbContext::new_sqlite("app-state-static-config.sqlite"))
                .await;

        assert_eq!(app_state.max_body_size, CONFIG.max_body_size);
        assert_eq!(app_state.timezone, CONFIG.timezone);
    }

    #[tokio::test]
    async fn manager_session_cleanup_tick_removes_expired_rows_and_survives_storage_failure() {
        let test_db_context = TestDbContext::new_sqlite("app-state-session-cleanup.sqlite");

        test_db_context
            .run_async(async {
                let app_state = super::create_test_app_state(test_db_context.clone()).await;
                let now = crate::utils::auth::get_current_timestamp();
                let expired = ManagerAuthInstance::create_instance(
                    "expired-cleanup".to_string(),
                    crate::utils::auth::manager_jwt_key_id().to_string(),
                    uuid::Uuid::new_v4().to_string(),
                    now - 2,
                    now - 1,
                    now + 100,
                )
                .expect("expired fixture should create");

                assert_eq!(app_state.tick_manager_session_cleanup(), Some(1));
                assert!(
                    ManagerAuthInstance::get_instance(expired.id)
                        .expect("expired lookup should query")
                        .is_none()
                );

                let mut conn = get_connection().expect("connection should load");
                match &mut conn {
                    DbConnection::Postgres(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("session table should drop");
                    }
                    DbConnection::Sqlite(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("session table should drop");
                    }
                }
                drop(conn);

                assert_eq!(app_state.tick_manager_session_cleanup(), None);
                assert!(
                    app_state.max_body_size > 0,
                    "proxy state must remain usable"
                );
            })
            .await;
    }
}
