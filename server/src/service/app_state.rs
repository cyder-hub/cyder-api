use std::sync::Arc;

use axum::Router;
use chrono::Utc;
use thiserror::Error;

use crate::config::{
    RuntimeStateBackendType,
    loader::{ConfigLoadOptions, LoadedConfig, load_effective_config},
    paths::ConfigPaths,
};
use crate::proxy::logging::RequestLogPersistedSink;
use crate::service::cache::CacheError;
use crate::service::{
    alerts::AlertsService,
    diagnostics::{DiagnosticsPolicy, DiagnosticsPolicyManager, DiagnosticsService},
    metrics::MetricsService,
    notification::NotificationService,
    system_config::SystemConfigService,
};

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
    pub diagnostics: Arc<DiagnosticsService>,
    pub metrics: Arc<MetricsService>,
    pub alerts: Arc<AlertsService>,
    pub notification: Arc<NotificationService>,
    pub runtime_backend_status: Arc<RuntimeStateBackendStatus>,
    pub system_config: Arc<SystemConfigService>,
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
        let loaded_config = load_initial_config()?;
        let system_config = Arc::new(SystemConfigService::new(
            loaded_config.clone(),
            ConfigLoadOptions::default(),
        ));
        let initial_snapshot = system_config.runtime_snapshot().await;

        #[cfg(test)]
        let infra = Arc::new(
            AppInfra::new_with_config(
                initial_snapshot.version,
                initial_snapshot.proxy_request.clone(),
                initial_snapshot.proxy.clone(),
                test_db_context.clone(),
            )
            .await,
        );

        #[cfg(not(test))]
        let infra = Arc::new(
            AppInfra::new_with_config(
                initial_snapshot.version,
                initial_snapshot.proxy_request.clone(),
                initial_snapshot.proxy.clone(),
            )
            .await,
        );
        system_config
            .register_http_client_manager(infra.http_clients())
            .await;
        let diagnostics_policy_manager = Arc::new(DiagnosticsPolicyManager::new(
            DiagnosticsPolicy::from_config(&initial_snapshot.diagnostics),
        ));
        system_config
            .register_diagnostics_policy_manager(Arc::clone(&diagnostics_policy_manager))
            .await;
        let diagnostics = Arc::new(DiagnosticsService::new(diagnostics_policy_manager));
        let metrics = Arc::new(MetricsService::new(loaded_config.config.metrics.clone()));
        let alerts = Arc::new(AlertsService::new(loaded_config.config.alerts.clone()));
        let notification = Arc::new(
            NotificationService::new_with_default_channel_cooldown_seconds(
                loaded_config.config.notification.clone(),
                loaded_config.config.alerts.default_cooldown_seconds,
            ),
        );
        let metrics_sink: Arc<dyn RequestLogPersistedSink> = metrics.clone();
        infra
            .log_manager()
            .set_request_log_persisted_sink(metrics_sink);

        let runtime_backend = RuntimeStateBackendBundle::from_config(
            &loaded_config.config,
            force_memory_runtime_state,
        )
        .await?;
        system_config
            .register_provider_governance_config_manager(
                runtime_backend.provider_circuit.config_manager(),
            )
            .await;
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
            diagnostics,
            metrics,
            alerts,
            notification,
            runtime_backend_status: Arc::new(runtime_backend.status),
            system_config,
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

    #[cfg(not(test))]
    pub fn start_background_workers(self: &Arc<Self>) {
        self.spawn_metrics_reconciliation_worker();
        self.spawn_alert_evaluation_worker();
        self.spawn_notification_delivery_worker();
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
    fn spawn_alert_evaluation_worker(self: &Arc<Self>) {
        if !self.alerts.config().enabled {
            return;
        }
        let app_state = Arc::clone(self);
        let interval_seconds = app_state.alerts.config().evaluation_interval_seconds.max(1);
        self.infra.spawn_background_task(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(interval_seconds));
            loop {
                interval.tick().await;
                let result = app_state.alerts.tick_evaluation_worker(&app_state).await;
                if result.failed > 0 {
                    crate::warn_event!(
                        "alerts.evaluation_worker_tick_degraded",
                        evaluated = result.evaluated,
                        fired = result.fired,
                        resolved = result.resolved,
                        failed = result.failed
                    );
                } else if result.fired > 0 || result.resolved > 0 {
                    crate::debug_event!(
                        "alerts.evaluation_worker_tick_completed",
                        evaluated = result.evaluated,
                        fired = result.fired,
                        resolved = result.resolved
                    );
                }
            }
        });
    }

    #[cfg(not(test))]
    fn spawn_notification_delivery_worker(self: &Arc<Self>) {
        if !self.notification.config().enabled {
            return;
        }
        let app_state = Arc::clone(self);
        let interval_seconds = app_state
            .notification
            .config()
            .worker_interval_seconds
            .max(1);
        self.infra.spawn_background_task(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(interval_seconds));
            loop {
                interval.tick().await;
                let client = app_state.infra.client().await;
                let result = app_state
                    .notification
                    .tick_delivery_worker(client.as_ref())
                    .await;
                if result.failed > 0 {
                    crate::warn_event!(
                        "notification.delivery_worker_tick_degraded",
                        processed = result.processed,
                        succeeded = result.succeeded,
                        retry_scheduled = result.retry_scheduled,
                        failed = result.failed
                    );
                } else if result.processed > 0 {
                    crate::debug_event!(
                        "notification.delivery_worker_tick_completed",
                        processed = result.processed,
                        succeeded = result.succeeded,
                        retry_scheduled = result.retry_scheduled
                    );
                }
            }
        });
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

fn load_initial_config() -> Result<LoadedConfig, RuntimeStateBackendError> {
    load_effective_config(
        &ConfigPaths::for_current_build(),
        ConfigLoadOptions::default(),
    )
    .map_err(|err| RuntimeStateBackendError::Config(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::super::admin::AdminServices;
    use super::AppState;
    use crate::config::{CONFIG, RuntimeStateBackendType};
    use crate::database::TestDbContext;
    use crate::service::alerts::AlertsService;
    use crate::service::catalog::CatalogService;
    use crate::service::diagnostics::{
        DiagnosticsPolicy, DiagnosticsPolicyManager, DiagnosticsService,
    };
    use crate::service::infra::AppInfra;
    use crate::service::metrics::MetricsService;
    use crate::service::notification::NotificationService;
    use crate::service::runtime::{ProviderKeySelector, RuntimeStateBackendBundle};
    use crate::service::system_config::SystemConfigService;
    use std::sync::Arc;

    async fn test_app_state() -> AppState {
        let catalog = Arc::new(CatalogService::new(true).await);
        let admin = Arc::new(AdminServices::new(Arc::clone(&catalog)));
        let loaded_config = super::load_initial_config().expect("config should load");
        let system_config = Arc::new(SystemConfigService::new_with_default_options(loaded_config));
        let initial_snapshot = system_config.runtime_snapshot().await;
        let infra = Arc::new(
            AppInfra::new_with_config(
                initial_snapshot.version,
                initial_snapshot.proxy_request.clone(),
                initial_snapshot.proxy.clone(),
                None,
            )
            .await,
        );
        system_config
            .register_http_client_manager(infra.http_clients())
            .await;
        let diagnostics_policy_manager = Arc::new(DiagnosticsPolicyManager::new(
            DiagnosticsPolicy::from_config(&initial_snapshot.diagnostics),
        ));
        system_config
            .register_diagnostics_policy_manager(Arc::clone(&diagnostics_policy_manager))
            .await;
        let diagnostics = Arc::new(DiagnosticsService::new(diagnostics_policy_manager));
        let metrics = Arc::new(MetricsService::new(CONFIG.metrics.clone()));
        let alerts = Arc::new(AlertsService::new(CONFIG.alerts.clone()));
        let notification = Arc::new(
            NotificationService::new_with_default_channel_cooldown_seconds(
                CONFIG.notification.clone(),
                CONFIG.alerts.default_cooldown_seconds,
            ),
        );
        let runtime_backend = RuntimeStateBackendBundle::from_config(&CONFIG, true)
            .await
            .expect("test runtime backend should initialize");
        system_config
            .register_provider_governance_config_manager(
                runtime_backend.provider_circuit.config_manager(),
            )
            .await;
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
            provider_circuit: Arc::clone(&runtime_backend.provider_circuit),
            diagnostics,
            metrics,
            alerts,
            notification,
            runtime_backend_status: Arc::new(runtime_backend.status),
            system_config,
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
        assert_eq!(Arc::strong_count(&app_state.diagnostics), 1);
        assert_eq!(Arc::strong_count(&app_state.runtime_backend_status), 1);
        assert_eq!(Arc::strong_count(&app_state.system_config), 1);
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

    #[tokio::test]
    async fn app_state_exposes_initial_system_config_snapshot() {
        let app_state =
            AppState::new_for_test(TestDbContext::new_sqlite("app-state-system-config.sqlite"))
                .await;

        let snapshot = app_state.system_config.runtime_snapshot().await;
        let expected = super::load_initial_config()
            .expect("config should load")
            .config
            .log_level;

        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.log_level, expected);
    }
}
