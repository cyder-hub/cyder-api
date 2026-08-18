use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::Router;
use chrono::Utc;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::config::CONFIG;
use crate::database::runtime::DatabaseRuntime;
use crate::proxy::logging::RequestLogPersistedSink;
use crate::service::cache::CacheError;
use crate::service::metrics::MetricsService;
use crate::service::secret_encryption::SecretEncryptionService;

#[cfg(test)]
use crate::database::test_support::TestDatabase;

use super::admin::AdminServices;
use super::catalog::CatalogService;
use super::infra::AppInfra;
use super::runtime::{
    ApiKeyGovernanceService, ProviderKeySelector, RuntimeStateBackendBundle,
    RuntimeStateBackendError, RuntimeStateBackendHealth, RuntimeStateBackendOperatorStatus,
    RuntimeStateBackendStatus,
};

pub(crate) struct PersistenceWorkers {
    cancellation: CancellationToken,
    handles: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    started: AtomicBool,
}

impl PersistenceWorkers {
    fn new() -> Self {
        Self {
            cancellation: CancellationToken::new(),
            handles: Mutex::new(Vec::new()),
            started: AtomicBool::new(false),
        }
    }

    fn start_once(&self) -> bool {
        self.started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    fn push(&self, handle: tokio::task::JoinHandle<()>) {
        self.handles
            .lock()
            .expect("persistence worker handle mutex should not be poisoned")
            .push(handle);
    }

    async fn stop_and_join(&self, overdue_after: Duration) {
        self.cancellation.cancel();
        let handles = std::mem::take(
            &mut *self
                .handles
                .lock()
                .expect("persistence worker handle mutex should not be poisoned"),
        );
        if handles.is_empty() {
            return;
        }
        let task_count = handles.len();
        let join = async move {
            for handle in handles {
                if let Err(error) = handle.await {
                    crate::error_event!(
                        "persistence.worker_join_failed",
                        error = error.to_string(),
                    );
                }
            }
        };
        tokio::pin!(join);
        let overdue = tokio::time::sleep(overdue_after);
        tokio::pin!(overdue);
        tokio::select! {
            () = &mut join => {}
            () = &mut overdue => {
                crate::warn_event!(
                    "persistence.worker_drain_overdue",
                    worker_count = task_count,
                    operation_deadline_ms = overdue_after.as_millis(),
                );
                join.await;
            }
        }
    }

    #[cfg(test)]
    fn handle_count(&self) -> usize {
        self.handles
            .lock()
            .expect("persistence worker handle mutex should not be poisoned")
            .len()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub database: Arc<DatabaseRuntime>,
    pub infra: Arc<AppInfra>,
    pub catalog: Arc<CatalogService>,
    pub admin: Arc<AdminServices>,
    pub provider_key_selector: Arc<ProviderKeySelector>,
    pub api_key_governance: Arc<ApiKeyGovernanceService>,
    pub metrics: Arc<MetricsService>,
    pub runtime_backend_status: Arc<RuntimeStateBackendStatus>,
    pub runtime_backend_health: Arc<RuntimeStateBackendHealth>,
    pub secret_encryption: Arc<SecretEncryptionService>,
    pub manager_auth_browser_origin: Option<String>,
    pub base_path: String,
    pub max_body_size: usize,
    pub timezone: Option<String>,
    pub(crate) persistence_workers: Arc<PersistenceWorkers>,
    #[cfg(test)]
    pub(crate) _test_database: Option<TestDatabase>,
}

impl AppState {
    #[cfg(not(test))]
    pub async fn new(database: Arc<DatabaseRuntime>) -> Self {
        Self::try_new_with_database(database)
            .await
            .expect("failed to initialize app state")
    }

    #[cfg(test)]
    pub async fn new(database: Arc<DatabaseRuntime>) -> Self {
        Self::try_new_with_database(database, None)
            .await
            .expect("failed to initialize app state")
    }

    #[cfg(test)]
    pub(crate) async fn new_for_test(test_database: TestDatabase) -> Self {
        let database = test_database.runtime();
        Self::try_new_with_database(database, Some(test_database))
            .await
            .expect("failed to initialize test app state")
    }

    async fn try_new_with_database(
        database: Arc<DatabaseRuntime>,
        #[cfg(test)] test_database: Option<TestDatabase>,
    ) -> Result<Self, RuntimeStateBackendError> {
        #[cfg(test)]
        let force_memory_cache = test_database.is_some();
        #[cfg(not(test))]
        let force_memory_cache = false;
        let force_memory_runtime_state = force_memory_cache;
        let config = CONFIG.clone();
        #[cfg(test)]
        let config = {
            let mut config = config;
            if test_database.is_some() {
                config.manager_auth.browser_origin = Some("http://127.0.0.1:29528".to_string());
            }
            config
        };

        #[cfg(test)]
        let infra = Arc::new(
            AppInfra::new_with_config(
                Arc::clone(&database),
                config.outbound_http.clone(),
                config.proxy_request.clone(),
                config.proxy.as_ref().map(|proxy| proxy.expose().to_owned()),
            )
            .await,
        );

        #[cfg(not(test))]
        let infra = Arc::new(
            AppInfra::new_with_config(
                Arc::clone(&database),
                config.outbound_http.clone(),
                config.proxy_request.clone(),
                config.proxy.as_ref().map(|proxy| proxy.expose().to_owned()),
            )
            .await,
        );
        let metrics = Arc::new(MetricsService::new(
            Arc::clone(&database),
            config.metrics.clone(),
        ));
        let metrics_sink: Arc<dyn RequestLogPersistedSink> = metrics.clone();
        infra
            .log_manager()
            .set_request_log_persisted_sink(metrics_sink);

        let runtime_backend = RuntimeStateBackendBundle::from_config(
            &config,
            force_memory_runtime_state,
            Arc::clone(&database),
        )
        .await?;
        let catalog =
            Arc::new(CatalogService::new(Arc::clone(&database), force_memory_cache).await);
        let secret_encryption = Arc::new(SecretEncryptionService::from_config(
            &config.secret_encryption,
        ));
        let admin = Arc::new(
            AdminServices::new(Arc::clone(&catalog), Arc::clone(&secret_encryption)).await,
        );
        let provider_key_selector = ProviderKeySelector::new(
            Arc::clone(&catalog),
            Arc::clone(&runtime_backend.provider_key_cursor_store),
        )
        .await;

        Ok(Self {
            database,
            infra,
            catalog,
            admin,
            provider_key_selector,
            api_key_governance: Arc::clone(&runtime_backend.api_key_governance),
            metrics,
            runtime_backend_status: Arc::new(runtime_backend.status),
            runtime_backend_health: Arc::new(runtime_backend.health),
            secret_encryption,
            manager_auth_browser_origin: config.manager_auth.browser_origin.clone(),
            base_path: config.base_path.clone(),
            max_body_size: config.max_body_size,
            timezone: config.timezone.clone(),
            persistence_workers: Arc::new(PersistenceWorkers::new()),
            #[cfg(test)]
            _test_database: test_database,
        })
    }

    pub async fn flush_proxy_logs(&self) {
        self.infra.flush_proxy_logs().await;
    }

    pub async fn shutdown_persistence(&self) {
        crate::info_event!(
            "persistence.shutdown_phase_started",
            phase = "periodic_workers",
        );
        self.persistence_workers
            .stop_and_join(self.database.operation_deadline())
            .await;
        crate::info_event!(
            "persistence.shutdown_phase_completed",
            phase = "periodic_workers",
        );

        crate::info_event!(
            "persistence.shutdown_phase_started",
            phase = "request_log_drain",
        );
        self.infra.close_and_drain_proxy_logs().await;
        crate::info_event!(
            "persistence.shutdown_phase_completed",
            phase = "request_log_drain",
        );

        crate::info_event!(
            "persistence.shutdown_phase_started",
            phase = "database_runtime",
        );
        self.database.close_and_drain().await;
        crate::info_event!(
            "persistence.shutdown_phase_completed",
            phase = "database_runtime",
        );
    }

    pub async fn runtime_state_backend_operator_status(&self) -> RuntimeStateBackendOperatorStatus {
        let checked_at = Utc::now().timestamp_millis();
        let runtime_read_error = self.runtime_backend_health.check().await;
        if let Some(error) = runtime_read_error.as_deref() {
            crate::warn_event!(
                "runtime_state.health_check_failed",
                component = "runtime_state_backend",
                backend = self.runtime_backend_status.effective_backend.as_str(),
                error = error,
            );
        }
        let catalog_status = self.catalog.backend_status();

        self.runtime_backend_status.to_operator_status(
            catalog_status.configured_backend,
            catalog_status.effective_backend,
            catalog_status.fallback_reason,
            runtime_read_error,
            checked_at,
        )
    }

    pub fn start_background_workers(self: &Arc<Self>) {
        if !self.persistence_workers.start_once() {
            return;
        }
        self.spawn_metrics_reconciliation_worker();
        self.spawn_manager_session_cleanup_worker();
    }

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
        let cancellation = self.persistence_workers.cancellation();
        let handle = self.infra.spawn_background_task(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(interval_seconds));
            loop {
                tokio::select! {
                    biased;
                    () = cancellation.cancelled() => break,
                    _ = interval.tick() => {}
                }
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
        self.persistence_workers.push(handle);
    }

    fn spawn_manager_session_cleanup_worker(self: &Arc<Self>) {
        let app_state = Arc::clone(self);
        let cancellation = self.persistence_workers.cancellation();
        let handle = self.infra.spawn_background_task(async move {
            let period = std::time::Duration::from_secs(60 * 60);
            let mut interval =
                tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            loop {
                tokio::select! {
                    biased;
                    () = cancellation.cancelled() => break,
                    _ = interval.tick() => {}
                }
                app_state.tick_manager_session_cleanup().await;
            }
        });
        self.persistence_workers.push(handle);
    }

    async fn tick_manager_session_cleanup(&self) -> Option<usize> {
        match self.admin.auth.cleanup_expired_instances().await {
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

pub async fn create_app_state(database: Arc<DatabaseRuntime>) -> Arc<AppState> {
    let app_state = create_configured_app_state(AppState::new(database).await).await;
    #[cfg(not(test))]
    app_state.start_background_workers();
    app_state
}

#[cfg(test)]
pub(crate) async fn create_test_app_state(test_database: TestDatabase) -> Arc<AppState> {
    create_configured_app_state(AppState::new_for_test(test_database).await).await
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::super::admin::AdminServices;
    use super::{AppState, PersistenceWorkers};
    use crate::config::{CONFIG, RuntimeStateBackendType};
    use crate::database::TestDatabase;
    use crate::database::manager_auth_instance::ManagerAuthInstance;
    use crate::service::catalog::CatalogService;
    use crate::service::infra::AppInfra;
    use crate::service::metrics::MetricsService;
    use crate::service::runtime::{ProviderKeySelector, RuntimeStateBackendBundle};
    use crate::service::secret_encryption::SecretEncryptionService;
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    async fn test_app_state() -> AppState {
        let test_db_context = TestDatabase::new_sqlite_default("app-state-unit.sqlite").await;
        let database = test_db_context.runtime();
        let catalog = Arc::new(CatalogService::new(Arc::clone(&database), true).await);
        let config = CONFIG.clone();
        let secret_encryption = Arc::new(SecretEncryptionService::from_config(
            &config.secret_encryption,
        ));
        let infra = Arc::new(
            AppInfra::new_with_config(
                Arc::clone(&database),
                config.outbound_http.clone(),
                config.proxy_request.clone(),
                config.proxy.as_ref().map(|proxy| proxy.expose().to_owned()),
            )
            .await,
        );
        let metrics = Arc::new(MetricsService::new(
            Arc::clone(&database),
            CONFIG.metrics.clone(),
        ));
        let runtime_backend =
            RuntimeStateBackendBundle::from_config(&CONFIG, true, Arc::clone(&database))
                .await
                .expect("test runtime backend should initialize");
        let admin = Arc::new(
            AdminServices::new(Arc::clone(&catalog), Arc::clone(&secret_encryption)).await,
        );
        let provider_key_selector = ProviderKeySelector::new(
            Arc::clone(&catalog),
            Arc::clone(&runtime_backend.provider_key_cursor_store),
        )
        .await;

        AppState {
            database,
            infra,
            catalog,
            admin,
            provider_key_selector,
            api_key_governance: Arc::clone(&runtime_backend.api_key_governance),
            metrics,
            runtime_backend_status: Arc::new(runtime_backend.status),
            runtime_backend_health: Arc::new(runtime_backend.health),
            secret_encryption,
            manager_auth_browser_origin: config.manager_auth.browser_origin.clone(),
            base_path: config.base_path.clone(),
            max_body_size: config.max_body_size,
            timezone: config.timezone,
            persistence_workers: Arc::new(super::PersistenceWorkers::new()),
            _test_database: Some(test_db_context),
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
        assert_eq!(Arc::strong_count(&app_state.runtime_backend_status), 1);
        assert_eq!(Arc::strong_count(&app_state.runtime_backend_health), 1);
        assert!(Arc::ptr_eq(
            &app_state.secret_encryption,
            &app_state.admin.secret_encryption,
        ));
        assert!(Arc::ptr_eq(
            &app_state.database,
            &app_state.catalog.database(),
        ));
        assert!(std::ptr::eq(
            app_state.database.as_ref(),
            app_state.metrics.database(),
        ));
        assert!(std::ptr::eq(
            app_state.database.as_ref(),
            app_state.api_key_governance.database(),
        ));
        assert!(std::ptr::eq(
            app_state.database.as_ref(),
            app_state.admin.auth.database(),
        ));
    }

    #[tokio::test]
    async fn new_for_test_injects_memory_runtime_backend_bundle() {
        let app_state = AppState::new_for_test(
            TestDatabase::new_sqlite_default("app-state-runtime-bundle.sqlite").await,
        )
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
        let app_state = AppState::new_for_test(
            TestDatabase::new_sqlite_default("app-state-operator-status.sqlite").await,
        )
        .await;

        let status = app_state.runtime_state_backend_operator_status().await;

        assert_eq!(status.runtime_effective_backend, "memory");
        assert_eq!(status.catalog_cache_backend, "memory");
        assert!(status.last_error.is_none());
    }

    #[tokio::test]
    async fn app_state_exposes_static_request_settings() {
        let app_state = AppState::new_for_test(
            TestDatabase::new_sqlite_default("app-state-static-config.sqlite").await,
        )
        .await;

        assert_eq!(app_state.max_body_size, CONFIG.max_body_size);
        assert_eq!(app_state.timezone, CONFIG.timezone);
    }

    #[tokio::test]
    async fn manager_session_cleanup_tick_removes_expired_rows_and_survives_storage_failure() {
        let test_db_context =
            TestDatabase::new_sqlite_default("app-state-session-cleanup.sqlite").await;

        (async {
            let app_state = super::create_test_app_state(test_db_context.clone()).await;
            let database = test_db_context.runtime();
            let now = crate::utils::auth::get_current_timestamp();
            let expired = ManagerAuthInstance::create_instance(
                &database,
                "expired-cleanup".to_string(),
                crate::utils::auth::manager_jwt_key_id().to_string(),
                uuid::Uuid::new_v4().to_string(),
                now - 2,
                now - 1,
                now + 100,
            )
            .await
            .expect("expired fixture should create");

            assert_eq!(app_state.tick_manager_session_cleanup().await, Some(1));
            assert!(
                ManagerAuthInstance::get_instance(&database, expired.id)
                    .await
                    .expect("expired lookup should query")
                    .is_none()
            );

            test_db_context
                .execute_sqlite_batch("DROP TABLE manager_auth_instance")
                .await
                .expect("session table should drop");

            assert_eq!(app_state.tick_manager_session_cleanup().await, None);
            assert!(
                app_state.max_body_size > 0,
                "proxy state must remain usable"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn persistence_worker_stop_waits_for_started_tick_and_prevents_the_next_tick() {
        let workers = Arc::new(PersistenceWorkers::new());
        assert!(workers.start_once());
        let cancellation = workers.cancellation();
        let trigger = Arc::new(Semaphore::new(0));
        let started = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        let ticks = Arc::new(AtomicUsize::new(0));
        let worker = {
            let trigger = Arc::clone(&trigger);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            let ticks = Arc::clone(&ticks);
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        biased;
                        () = cancellation.cancelled() => break,
                        permit = trigger.acquire() => {
                            permit.expect("trigger should remain open").forget();
                        }
                    }
                    ticks.fetch_add(1, Ordering::AcqRel);
                    started.add_permits(1);
                    release
                        .acquire()
                        .await
                        .expect("release should remain open")
                        .forget();
                }
            })
        };
        workers.push(worker);
        trigger.add_permits(1);
        started
            .acquire()
            .await
            .expect("first tick should start")
            .forget();

        let stop_workers = Arc::clone(&workers);
        let mut stop = tokio::spawn(async move {
            stop_workers.stop_and_join(Duration::from_secs(1)).await;
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut stop)
                .await
                .is_err(),
            "stop must wait for the already-started tick"
        );
        trigger.add_permits(1);
        release.add_permits(1);
        stop.await.expect("worker stop should join");

        assert_eq!(ticks.load(Ordering::Acquire), 1);
        assert_eq!(workers.handle_count(), 0);
    }

    #[tokio::test]
    async fn app_state_shutdown_joins_workers_drains_logs_and_closes_database_last() {
        let app_state = super::create_test_app_state(
            TestDatabase::new_sqlite_default("app-state-persistence-shutdown.sqlite").await,
        )
        .await;
        app_state.start_background_workers();
        let expected_workers = 1 + usize::from(app_state.metrics.config().enabled);
        assert_eq!(
            app_state.persistence_workers.handle_count(),
            expected_workers
        );

        app_state.shutdown_persistence().await;

        assert_eq!(app_state.persistence_workers.handle_count(), 0);
        assert!(app_state.database.snapshot().shutting_down);
        assert!(matches!(
            app_state.database.readiness_probe().await,
            Err(crate::database::error::PersistenceError::ShuttingDown)
        ));
    }
}
