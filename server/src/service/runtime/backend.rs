use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{CacheBackendType, FinalConfig, RuntimeStateBackendType};
use crate::service::redis::{self, RedisPool};

use super::api_key_governance::{
    ApiKeyGovernanceService, MemoryApiKeyRuntimeStore, RedisApiKeyRuntimeStore,
};
use super::provider_key_selection::{
    MemoryProviderKeyCursorStore, ProviderKeyCursorStore, RedisProviderKeyCursorStore,
};
use super::source_circuit::{
    MemorySourceCircuitStore, RedisSourceCircuitStore, SourceCircuitService,
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
    pub source_circuit: Arc<SourceCircuitService>,
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
            .validate_runtime_state()
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
            .validate_runtime_state()
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
            catalog_cache_backend: config.cache.catalog_backend(),
            configured_backend,
            effective_backend: RuntimeStateBackendType::Memory,
            fallback_reason,
            last_error,
            last_checked_at: Utc::now().timestamp_millis(),
        };
        log_backend_selected(&status);

        Self {
            api_key_governance: Arc::new(ApiKeyGovernanceService::new(Arc::new(
                MemoryApiKeyRuntimeStore::default(),
            ))),
            source_circuit: Arc::new(SourceCircuitService::new_with_config(
                Arc::new(MemorySourceCircuitStore::default()),
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
            catalog_cache_backend: config.cache.catalog_backend(),
            configured_backend: RuntimeStateBackendType::Redis,
            effective_backend: RuntimeStateBackendType::Redis,
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
            source_circuit: Arc::new(SourceCircuitService::new_with_config(
                Arc::new(RedisSourceCircuitStore::new(
                    pool.clone(),
                    key_prefix.clone(),
                    config.runtime_state.provider_circuit_probe_lease_ttl(),
                    state_ttl,
                )),
                config.provider_governance.clone(),
            )),
            provider_key_cursor_store: Arc::new(RedisProviderKeyCursorStore::new(
                pool.clone(),
                key_prefix.clone(),
                state_ttl,
            )),
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
