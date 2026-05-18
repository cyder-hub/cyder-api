use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{CacheBackendType, DeploymentMode, FinalConfig, RuntimeStateBackendType};
use crate::service::redis::{self, RedisPool};

use super::api_key_governance::{
    ApiKeyGovernanceService, MemoryApiKeyRuntimeStore, RedisApiKeyRuntimeStore,
};
use super::provider_circuit::{
    MemoryProviderCircuitStore, ProviderCircuitService, RedisProviderCircuitStore,
};
use super::provider_key_selection::{
    MemoryProviderKeyCursorStore, ProviderKeyCursorStore, RedisProviderKeyCursorStore,
};

#[derive(Debug, Error)]
pub enum RuntimeStateBackendError {
    #[error("runtime state configuration error: {0}")]
    Config(String),
    #[error("runtime state redis backend unavailable: {0}")]
    RedisUnavailable(String),
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RuntimeStateBackendStatus {
    pub deployment_mode: DeploymentMode,
    pub catalog_cache_backend: CacheBackendType,
    pub configured_backend: RuntimeStateBackendType,
    pub effective_backend: RuntimeStateBackendType,
    pub shared: bool,
    pub fallback_reason: Option<String>,
    pub last_error: Option<String>,
    pub last_checked_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStateBackendOperatorStatus {
    pub deployment_mode: String,
    pub catalog_cache_backend: String,
    pub catalog_cache_configured_backend: String,
    pub catalog_cache_effective_backend: String,
    pub catalog_cache_fallback_reason: Option<String>,
    pub runtime_configured_backend: String,
    pub runtime_effective_backend: String,
    pub runtime_shared: bool,
    pub runtime_degraded: bool,
    pub fallback_reason: Option<String>,
    pub last_error: Option<String>,
    pub last_checked_at: i64,
}

impl RuntimeStateBackendStatus {
    pub fn to_operator_status(
        &self,
        catalog_cache_configured_backend: CacheBackendType,
        catalog_cache_effective_backend: CacheBackendType,
        catalog_cache_fallback_reason: Option<String>,
        runtime_read_error: Option<String>,
        checked_at: i64,
    ) -> RuntimeStateBackendOperatorStatus {
        let last_error = runtime_read_error.or_else(|| self.last_error.clone());

        RuntimeStateBackendOperatorStatus {
            deployment_mode: self.deployment_mode.as_str().to_string(),
            catalog_cache_backend: catalog_cache_effective_backend.as_str().to_string(),
            catalog_cache_configured_backend: catalog_cache_configured_backend.as_str().to_string(),
            catalog_cache_effective_backend: catalog_cache_effective_backend.as_str().to_string(),
            catalog_cache_fallback_reason,
            runtime_configured_backend: self.configured_backend.as_str().to_string(),
            runtime_effective_backend: self.effective_backend.as_str().to_string(),
            runtime_shared: self.shared,
            runtime_degraded: last_error.is_some(),
            fallback_reason: self.fallback_reason.clone(),
            last_error,
            last_checked_at: checked_at,
        }
    }
}

pub struct RuntimeStateBackendBundle {
    pub api_key_governance: Arc<ApiKeyGovernanceService>,
    pub provider_circuit: Arc<ProviderCircuitService>,
    pub provider_key_cursor_store: Arc<dyn ProviderKeyCursorStore>,
    pub status: RuntimeStateBackendStatus,
}

impl RuntimeStateBackendBundle {
    pub async fn from_config(
        config: &FinalConfig,
        force_memory_backend: bool,
    ) -> Result<Self, RuntimeStateBackendError> {
        if force_memory_backend {
            return Ok(Self::memory(
                config,
                RuntimeStateBackendType::Memory,
                Some("test_isolation".to_string()),
                None,
            ));
        }

        config
            .validate_deployment_runtime_state()
            .map_err(RuntimeStateBackendError::Config)?;

        let redis_pool = if force_memory_backend
            || config.runtime_state.backend == RuntimeStateBackendType::Memory
        {
            None
        } else {
            redis::get_pool().await
        };

        Self::from_config_with_pool(config, force_memory_backend, redis_pool)
    }

    pub fn from_config_with_pool(
        config: &FinalConfig,
        force_memory_backend: bool,
        redis_pool: Option<RedisPool>,
    ) -> Result<Self, RuntimeStateBackendError> {
        if force_memory_backend {
            return Ok(Self::memory(
                config,
                RuntimeStateBackendType::Memory,
                Some("test_isolation".to_string()),
                None,
            ));
        }

        config
            .validate_deployment_runtime_state()
            .map_err(RuntimeStateBackendError::Config)?;

        match config.runtime_state.backend {
            RuntimeStateBackendType::Memory => Ok(Self::memory(
                config,
                RuntimeStateBackendType::Memory,
                None,
                None,
            )),
            RuntimeStateBackendType::Redis => {
                if let Some(pool) = redis_pool {
                    Ok(Self::redis(config, pool))
                } else if config.runtime_state.fallback_to_memory {
                    let reason = "redis_unavailable".to_string();
                    let error = "redis pool is unavailable".to_string();
                    crate::warn_event!(
                        "runtime_state.memory_fallback_enabled",
                        configured_backend = RuntimeStateBackendType::Redis.as_str(),
                        effective_backend = RuntimeStateBackendType::Memory.as_str(),
                        fallback_reason = &reason,
                        last_error = &error,
                    );
                    Ok(Self::memory(
                        config,
                        RuntimeStateBackendType::Redis,
                        Some(reason),
                        Some(error),
                    ))
                } else {
                    crate::warn_event!(
                        "runtime_state.redis_unavailable",
                        configured_backend = RuntimeStateBackendType::Redis.as_str(),
                        fallback_to_memory = config.runtime_state.fallback_to_memory,
                    );
                    Err(RuntimeStateBackendError::RedisUnavailable(
                        "redis pool is unavailable and runtime_state.fallback_to_memory=false"
                            .to_string(),
                    ))
                }
            }
        }
    }

    fn memory(
        config: &FinalConfig,
        configured_backend: RuntimeStateBackendType,
        fallback_reason: Option<String>,
        last_error: Option<String>,
    ) -> Self {
        let status = RuntimeStateBackendStatus {
            deployment_mode: config.deployment.mode.clone(),
            catalog_cache_backend: config.cache.catalog_backend(),
            configured_backend,
            effective_backend: RuntimeStateBackendType::Memory,
            shared: false,
            fallback_reason,
            last_error,
            last_checked_at: Utc::now().timestamp_millis(),
        };
        log_backend_selected(&status);

        Self {
            api_key_governance: Arc::new(ApiKeyGovernanceService::new(Arc::new(
                MemoryApiKeyRuntimeStore::default(),
            ))),
            provider_circuit: Arc::new(ProviderCircuitService::new_with_config(
                Arc::new(MemoryProviderCircuitStore::default()),
                config.provider_governance.clone(),
            )),
            provider_key_cursor_store: Arc::new(MemoryProviderKeyCursorStore::default()),
            status,
        }
    }

    fn redis(config: &FinalConfig, pool: RedisPool) -> Self {
        let redis_config = config
            .redis
            .as_ref()
            .expect("redis config should exist when redis pool exists");
        let key_prefix = format!(
            "{}{}",
            redis_config.key_prefix, config.runtime_state.redis.key_prefix
        );
        let state_ttl = config.runtime_state.state_ttl();
        let status = RuntimeStateBackendStatus {
            deployment_mode: config.deployment.mode.clone(),
            catalog_cache_backend: config.cache.catalog_backend(),
            configured_backend: RuntimeStateBackendType::Redis,
            effective_backend: RuntimeStateBackendType::Redis,
            shared: true,
            fallback_reason: None,
            last_error: None,
            last_checked_at: Utc::now().timestamp_millis(),
        };
        log_backend_selected(&status);

        Self {
            api_key_governance: Arc::new(ApiKeyGovernanceService::new(Arc::new(
                RedisApiKeyRuntimeStore::new(
                    pool.clone(),
                    key_prefix.clone(),
                    config.runtime_state.api_key_concurrency_lease_ttl(),
                    state_ttl,
                ),
            ))),
            provider_circuit: Arc::new(ProviderCircuitService::new_with_config(
                Arc::new(RedisProviderCircuitStore::new(
                    pool.clone(),
                    key_prefix.clone(),
                    config.runtime_state.provider_circuit_probe_lease_ttl(),
                    state_ttl,
                )),
                config.provider_governance.clone(),
            )),
            provider_key_cursor_store: Arc::new(RedisProviderKeyCursorStore::new(
                pool, key_prefix, state_ttl,
            )),
            status,
        }
    }
}

fn log_backend_selected(status: &RuntimeStateBackendStatus) {
    crate::info_event!(
        "deployment.mode_selected",
        mode = status.deployment_mode.as_str(),
    );
    crate::info_event!(
        "runtime_state.backend_selected",
        deployment_mode = status.deployment_mode.as_str(),
        catalog_cache_backend = cache_backend_name(status.catalog_cache_backend.clone()),
        configured_backend = status.configured_backend.as_str(),
        effective_backend = status.effective_backend.as_str(),
        shared = status.shared,
        fallback_reason = &status.fallback_reason,
        last_error = &status.last_error,
    );
}

fn cache_backend_name(backend: CacheBackendType) -> &'static str {
    backend.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb8::Pool;
    use bb8_redis::RedisConnectionManager;

    use crate::config::{
        AlertsConfig, CacheCatalogConfig, CacheConfig, CacheRedisConfig, DeploymentConfig,
        DiagnosticsConfig, IdConfig, MetricsConfig, NotificationConfig, RedisConfig,
        RuntimeStateConfig, RuntimeStateRedisConfig, StorageConfig,
    };

    fn base_config() -> FinalConfig {
        FinalConfig {
            host: "0.0.0.0".to_string(),
            port: 8000,
            base_path: "/ai".to_string(),
            secret_key: "secret".to_string(),
            password_salt: "salt".to_string(),
            jwt_secret: "jwt".to_string(),
            api_key_jwt_secret: "api-jwt".to_string(),
            db_url: "./storage/sqlite.db".to_string(),
            proxy: None,
            log_level: "info".to_string(),
            timezone: None,
            max_body_size: 100 * 1024 * 1024,
            replay_response_capture_max_bytes: 4 * 1024 * 1024,
            diagnostics: DiagnosticsConfig::default(),
            metrics: MetricsConfig::default(),
            alerts: AlertsConfig::default(),
            notification: NotificationConfig::default(),
            db_pool_size: 5,
            redis: None,
            deployment: DeploymentConfig::default(),
            id: IdConfig::default(),
            proxy_request: Default::default(),
            provider_governance: Default::default(),
            routing_resilience: Default::default(),
            cache: CacheConfig::default(),
            runtime_state: RuntimeStateConfig::default(),
            storage: StorageConfig::default(),
        }
    }

    fn redis_pool_without_connection() -> RedisPool {
        let manager = RedisConnectionManager::new("redis://127.0.0.1:1")
            .expect("redis test URL should be valid");
        Pool::builder().build_unchecked(manager)
    }

    #[tokio::test]
    async fn memory_backend_bundle_is_default_and_non_shared() {
        let config = base_config();
        let bundle = RuntimeStateBackendBundle::from_config_with_pool(&config, false, None)
            .expect("memory runtime backend should initialize");

        assert_eq!(
            bundle.status.configured_backend,
            RuntimeStateBackendType::Memory
        );
        assert_eq!(
            bundle.status.effective_backend,
            RuntimeStateBackendType::Memory
        );
        assert!(!bundle.status.shared);
        assert!(bundle.status.fallback_reason.is_none());
        assert_eq!(
            bundle
                .provider_key_cursor_store
                .next_queue_index(1, 2)
                .await
                .expect("memory cursor should work"),
            0
        );
    }

    #[test]
    fn operator_status_uses_effective_catalog_backend_and_runtime_read_error() {
        let status = RuntimeStateBackendStatus {
            deployment_mode: DeploymentMode::SingleInstance,
            catalog_cache_backend: CacheBackendType::Redis,
            configured_backend: RuntimeStateBackendType::Redis,
            effective_backend: RuntimeStateBackendType::Redis,
            shared: true,
            fallback_reason: None,
            last_error: None,
            last_checked_at: 100,
        };

        let operator_status = status.to_operator_status(
            CacheBackendType::Redis,
            CacheBackendType::Memory,
            Some("redis_unavailable".to_string()),
            Some("provider circuit snapshot failed".to_string()),
            200,
        );

        assert_eq!(operator_status.deployment_mode, "single_instance");
        assert_eq!(operator_status.catalog_cache_backend, "memory");
        assert_eq!(operator_status.catalog_cache_configured_backend, "redis");
        assert_eq!(operator_status.catalog_cache_effective_backend, "memory");
        assert_eq!(
            operator_status.catalog_cache_fallback_reason.as_deref(),
            Some("redis_unavailable")
        );
        assert_eq!(operator_status.runtime_configured_backend, "redis");
        assert_eq!(operator_status.runtime_effective_backend, "redis");
        assert!(operator_status.runtime_shared);
        assert!(operator_status.runtime_degraded);
        assert_eq!(
            operator_status.last_error.as_deref(),
            Some("provider circuit snapshot failed")
        );
        assert_eq!(operator_status.last_checked_at, 200);
    }

    #[test]
    fn redis_backend_without_pool_fails_when_fallback_is_disabled() {
        let mut config = base_config();
        config.runtime_state.backend = RuntimeStateBackendType::Redis;
        config.redis = Some(RedisConfig::default());

        let err = match RuntimeStateBackendBundle::from_config_with_pool(&config, false, None) {
            Ok(_) => panic!("redis backend without pool should fail"),
            Err(err) => err,
        };
        assert!(matches!(err, RuntimeStateBackendError::RedisUnavailable(_)));
    }

    #[test]
    fn redis_backend_without_pool_can_explicitly_fallback_to_memory() {
        let mut config = base_config();
        config.runtime_state.backend = RuntimeStateBackendType::Redis;
        config.runtime_state.fallback_to_memory = true;

        let bundle = RuntimeStateBackendBundle::from_config_with_pool(&config, false, None)
            .expect("fallback memory backend should initialize");
        assert_eq!(
            bundle.status.configured_backend,
            RuntimeStateBackendType::Redis
        );
        assert_eq!(
            bundle.status.effective_backend,
            RuntimeStateBackendType::Memory
        );
        assert!(!bundle.status.shared);
        assert_eq!(
            bundle.status.fallback_reason.as_deref(),
            Some("redis_unavailable")
        );
        assert!(bundle.status.last_error.is_some());
    }

    #[tokio::test]
    async fn redis_backend_bundle_creates_complete_shared_runtime_set() {
        let mut config = base_config();
        config.db_url = "postgres://cyder:cyder@localhost/cyder".to_string();
        config.redis = Some(RedisConfig {
            key_prefix: "cyder:".to_string(),
            ..RedisConfig::default()
        });
        config.cache = CacheConfig {
            catalog: CacheCatalogConfig {
                backend: CacheBackendType::Redis,
                redis: CacheRedisConfig {
                    key_prefix: "catalog:".to_string(),
                },
                ..CacheCatalogConfig::default()
            },
        };
        config.runtime_state = RuntimeStateConfig {
            backend: RuntimeStateBackendType::Redis,
            redis: RuntimeStateRedisConfig {
                key_prefix: "runtime:".to_string(),
                ..RuntimeStateRedisConfig::default()
            },
            fallback_to_memory: false,
        };

        let bundle = RuntimeStateBackendBundle::from_config_with_pool(
            &config,
            false,
            Some(redis_pool_without_connection()),
        )
        .expect("redis runtime backend should initialize with a provided pool");
        assert_eq!(
            bundle.status.effective_backend,
            RuntimeStateBackendType::Redis
        );
        assert!(bundle.status.shared);
    }

    #[test]
    fn force_memory_backend_ignores_invalid_runtime_config_for_tests() {
        let mut config = base_config();
        config.deployment.mode = DeploymentMode::MultiInstance;
        config.runtime_state.backend = RuntimeStateBackendType::Memory;

        let bundle = RuntimeStateBackendBundle::from_config_with_pool(&config, true, None)
            .expect("forced memory backend should be available for tests");
        assert_eq!(
            bundle.status.effective_backend,
            RuntimeStateBackendType::Memory
        );
        assert_eq!(
            bundle.status.fallback_reason.as_deref(),
            Some("test_isolation")
        );
    }
}
