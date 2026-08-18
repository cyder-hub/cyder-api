use std::sync::Arc;

use bb8_redis::redis as redis_client;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{CacheBackendType, FinalConfig, RuntimeStateBackendType};
use crate::database::runtime::DatabaseRuntime;
use crate::service::redis::{RedisPool, get_pool};

use super::api_key_governance::{
    ApiKeyGovernanceService, MemoryApiKeyRuntimeStore, RedisApiKeyRuntimeStore,
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
    pub catalog_cache_backend: CacheBackendType,
    pub configured_backend: RuntimeStateBackendType,
    pub effective_backend: RuntimeStateBackendType,
    pub fallback_reason: Option<String>,
    pub last_error: Option<String>,
    pub last_checked_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStateBackendOperatorStatus {
    pub catalog_cache_backend: String,
    pub catalog_cache_configured_backend: String,
    pub catalog_cache_effective_backend: String,
    pub catalog_cache_fallback_reason: Option<String>,
    pub runtime_configured_backend: String,
    pub runtime_effective_backend: String,
    pub runtime_degraded: bool,
    pub fallback_reason: Option<String>,
    pub last_error: Option<String>,
    pub last_checked_at: i64,
}

#[derive(Clone)]
pub struct RuntimeStateBackendHealth {
    effective_backend: RuntimeStateBackendType,
    redis_pool: Option<RedisPool>,
}

impl RuntimeStateBackendHealth {
    #[cfg(test)]
    pub(crate) fn redis_without_pool() -> Self {
        Self {
            effective_backend: RuntimeStateBackendType::Redis,
            redis_pool: None,
        }
    }

    pub(crate) async fn check(&self) -> Option<String> {
        if self.effective_backend != RuntimeStateBackendType::Redis {
            return None;
        }

        let Some(pool) = self.redis_pool.as_ref() else {
            return Some("redis runtime backend pool is unavailable".to_string());
        };
        let mut connection = match pool.get().await {
            Ok(connection) => connection,
            Err(error) => {
                return Some(format!(
                    "failed to get redis health-check connection: {error}"
                ));
            }
        };
        redis_client::cmd("PING")
            .query_async::<()>(&mut *connection)
            .await
            .err()
            .map(|error| format!("redis runtime backend PING failed: {error}"))
    }
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
            catalog_cache_backend: catalog_cache_effective_backend.as_str().to_string(),
            catalog_cache_configured_backend: catalog_cache_configured_backend.as_str().to_string(),
            catalog_cache_effective_backend: catalog_cache_effective_backend.as_str().to_string(),
            catalog_cache_fallback_reason,
            runtime_configured_backend: self.configured_backend.as_str().to_string(),
            runtime_effective_backend: self.effective_backend.as_str().to_string(),
            runtime_degraded: last_error.is_some(),
            fallback_reason: self.fallback_reason.clone(),
            last_error,
            last_checked_at: checked_at,
        }
    }
}

pub struct RuntimeStateBackendBundle {
    pub api_key_governance: Arc<ApiKeyGovernanceService>,
    pub provider_key_cursor_store: Arc<dyn ProviderKeyCursorStore>,
    pub health: RuntimeStateBackendHealth,
    pub status: RuntimeStateBackendStatus,
}

impl RuntimeStateBackendBundle {
    pub async fn from_config(
        config: &FinalConfig,
        force_memory_backend: bool,
        database: Arc<DatabaseRuntime>,
    ) -> Result<Self, RuntimeStateBackendError> {
        if force_memory_backend {
            return Ok(Self::memory(
                config,
                RuntimeStateBackendType::Memory,
                Some("test_isolation".to_string()),
                None,
                database,
            ));
        }

        config
            .validate_runtime_state()
            .map_err(RuntimeStateBackendError::Config)?;

        let redis_pool = if force_memory_backend
            || config.runtime_state.backend == RuntimeStateBackendType::Memory
        {
            None
        } else {
            get_pool().await
        };

        Self::from_config_with_pool(config, force_memory_backend, redis_pool, database)
    }

    pub fn from_config_with_pool(
        config: &FinalConfig,
        force_memory_backend: bool,
        redis_pool: Option<RedisPool>,
        database: Arc<DatabaseRuntime>,
    ) -> Result<Self, RuntimeStateBackendError> {
        if force_memory_backend {
            return Ok(Self::memory(
                config,
                RuntimeStateBackendType::Memory,
                Some("test_isolation".to_string()),
                None,
                database,
            ));
        }

        config
            .validate_runtime_state()
            .map_err(RuntimeStateBackendError::Config)?;

        match config.runtime_state.backend {
            RuntimeStateBackendType::Memory => Ok(Self::memory(
                config,
                RuntimeStateBackendType::Memory,
                None,
                None,
                database,
            )),
            RuntimeStateBackendType::Redis => {
                if let Some(pool) = redis_pool {
                    Ok(Self::redis(config, pool, database))
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
                        database,
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
        database: Arc<DatabaseRuntime>,
    ) -> Self {
        let status = RuntimeStateBackendStatus {
            catalog_cache_backend: config.cache.catalog_backend(),
            configured_backend,
            effective_backend: RuntimeStateBackendType::Memory,
            fallback_reason,
            last_error,
            last_checked_at: Utc::now().timestamp_millis(),
        };
        log_backend_selected(&status);

        Self {
            api_key_governance: Arc::new(ApiKeyGovernanceService::new(
                database,
                Arc::new(MemoryApiKeyRuntimeStore::default()),
            )),
            provider_key_cursor_store: Arc::new(MemoryProviderKeyCursorStore::default()),
            health: RuntimeStateBackendHealth {
                effective_backend: RuntimeStateBackendType::Memory,
                redis_pool: None,
            },
            status,
        }
    }

    fn redis(config: &FinalConfig, pool: RedisPool, database: Arc<DatabaseRuntime>) -> Self {
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
            catalog_cache_backend: config.cache.catalog_backend(),
            configured_backend: RuntimeStateBackendType::Redis,
            effective_backend: RuntimeStateBackendType::Redis,
            fallback_reason: None,
            last_error: None,
            last_checked_at: Utc::now().timestamp_millis(),
        };
        log_backend_selected(&status);

        Self {
            api_key_governance: Arc::new(ApiKeyGovernanceService::new(
                database,
                Arc::new(RedisApiKeyRuntimeStore::new(
                    pool.clone(),
                    key_prefix.clone(),
                    config.runtime_state.api_key_concurrency_lease_ttl(),
                    state_ttl,
                )),
            )),
            provider_key_cursor_store: Arc::new(RedisProviderKeyCursorStore::new(
                pool.clone(),
                key_prefix.clone(),
                state_ttl,
            )),
            health: RuntimeStateBackendHealth {
                effective_backend: RuntimeStateBackendType::Redis,
                redis_pool: Some(pool),
            },
            status,
        }
    }
}

fn log_backend_selected(status: &RuntimeStateBackendStatus) {
    crate::info_event!(
        "runtime_state.backend_selected",
        catalog_cache_backend = cache_backend_name(status.catalog_cache_backend.clone()),
        configured_backend = status.configured_backend.as_str(),
        effective_backend = status.effective_backend.as_str(),
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
    use crate::config::DatabaseIoConfig;
    use crate::database::test_support::TestDatabase;

    #[tokio::test]
    async fn memory_backend_health_does_not_probe_remote_services() {
        let health = RuntimeStateBackendHealth {
            effective_backend: RuntimeStateBackendType::Memory,
            redis_pool: None,
        };

        assert_eq!(health.check().await, None);
    }

    #[tokio::test]
    async fn redis_backend_health_reports_missing_pool() {
        let health = RuntimeStateBackendHealth {
            effective_backend: RuntimeStateBackendType::Redis,
            redis_pool: None,
        };

        assert_eq!(
            health.check().await.as_deref(),
            Some("redis runtime backend pool is unavailable")
        );
    }

    #[test]
    fn redis_unavailable_fallback_preserves_operator_backend_status() {
        let mut config = crate::config::CONFIG.clone();
        config.runtime_state.backend = RuntimeStateBackendType::Redis;
        config.runtime_state.fallback_to_memory = true;
        config.redis = None;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");
        let database = runtime.block_on(TestDatabase::new_sqlite(
            "runtime-state-backend.sqlite",
            2,
            DatabaseIoConfig::default(),
        ));
        let bundle = RuntimeStateBackendBundle::from_config_with_pool(
            &config,
            false,
            None,
            database.runtime(),
        )
        .expect("redis fallback should initialize memory runtime state");

        assert_eq!(
            bundle.status.configured_backend,
            RuntimeStateBackendType::Redis
        );
        assert_eq!(
            bundle.status.effective_backend,
            RuntimeStateBackendType::Memory
        );
        assert_eq!(
            bundle.status.fallback_reason.as_deref(),
            Some("redis_unavailable")
        );

        let operator_status = bundle.status.to_operator_status(
            config.cache.catalog_backend(),
            config.cache.catalog_backend(),
            None,
            None,
            1,
        );
        assert_eq!(operator_status.runtime_configured_backend, "redis");
        assert_eq!(operator_status.runtime_effective_backend, "memory");
        assert!(operator_status.runtime_degraded);
        assert_eq!(
            operator_status.last_error.as_deref(),
            Some("redis pool is unavailable")
        );
    }
}
