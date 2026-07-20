use rand::{Rng, distr::Alphanumeric, rng};
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, sync::LazyLock, time::Duration};

use crate::utils::ID_MAX_WORKER_ID;

pub mod env;
pub mod loader;
pub mod paths;
pub mod persistence;

// --- START DEPLOYMENT CONFIG ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    SingleInstance,
    MultiInstance,
}

impl Default for DeploymentMode {
    fn default() -> Self {
        Self::SingleInstance
    }
}

impl DeploymentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeploymentMode::SingleInstance => "single_instance",
            DeploymentMode::MultiInstance => "multi_instance",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeploymentConfig {
    #[serde(default)]
    pub mode: DeploymentMode,
}

// --- START ID CONFIG ---

fn default_id_worker_id() -> u64 {
    1
}

#[derive(Debug, Clone, Serialize)]
pub struct IdConfig {
    pub worker_id: u64,
}

impl Default for IdConfig {
    fn default() -> Self {
        Self {
            worker_id: default_id_worker_id(),
        }
    }
}

impl<'de> Deserialize<'de> for IdConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawIdConfig {
            #[serde(default = "default_id_worker_id")]
            worker_id: u64,
        }

        let raw = RawIdConfig::deserialize(deserializer)?;
        if raw.worker_id > ID_MAX_WORKER_ID {
            return Err(serde::de::Error::custom(format!(
                "id.worker_id must be in 0..={ID_MAX_WORKER_ID}"
            )));
        }

        Ok(Self {
            worker_id: raw.worker_id,
        })
    }
}

// --- START REDIS CONFIG ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    #[serde(default = "default_redis_url")]
    pub url: String,
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,
    #[serde(default = "default_key_prefix")]
    pub key_prefix: String,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: default_redis_url(),
            pool_size: default_pool_size(),
            key_prefix: default_key_prefix(),
        }
    }
}

// --- START CACHE CONFIG ---

/// Cache backend type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CacheBackendType {
    Memory,
    Redis,
}

impl Default for CacheBackendType {
    fn default() -> Self {
        CacheBackendType::Memory
    }
}

impl CacheBackendType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CacheBackendType::Memory => "memory",
            CacheBackendType::Redis => "redis",
        }
    }
}

/// Redis cache specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheRedisConfig {
    #[serde(default = "default_cache_redis_key_prefix")]
    pub key_prefix: String,
}

impl Default for CacheRedisConfig {
    fn default() -> Self {
        Self {
            key_prefix: default_cache_redis_key_prefix(),
        }
    }
}

/// Catalog cache domain configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheCatalogConfig {
    #[serde(default)]
    pub backend: CacheBackendType,
    #[serde(default = "default_ttl_seconds")]
    pub ttl: u64,
    #[serde(default = "default_negative_ttl_seconds")]
    pub negative_ttl: u64,
    #[serde(default)]
    pub redis: CacheRedisConfig,
}

impl Default for CacheCatalogConfig {
    fn default() -> Self {
        Self {
            backend: CacheBackendType::default(),
            ttl: default_ttl_seconds(),
            negative_ttl: default_negative_ttl_seconds(),
            redis: CacheRedisConfig::default(),
        }
    }
}

impl CacheCatalogConfig {
    pub fn ttl(&self) -> Duration {
        Duration::from_secs(self.ttl)
    }

    pub fn negative_ttl(&self) -> Duration {
        Duration::from_secs(self.negative_ttl)
    }
}

/// Overall cache configuration. Legacy `cache.backend` / `ttl` / `negative_ttl`
/// / `redis` are accepted as shorthand for `cache.catalog.*` during
/// deserialization, but serialization emits only the domain-based shape.
#[derive(Debug, Clone, Serialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub catalog: CacheCatalogConfig,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            catalog: CacheCatalogConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawCacheConfig {
    #[serde(default)]
    backend: Option<CacheBackendType>,
    #[serde(default)]
    ttl: Option<u64>,
    #[serde(default)]
    negative_ttl: Option<u64>,
    #[serde(default)]
    redis: Option<CacheRedisConfig>,
    #[serde(default)]
    catalog: Option<CacheCatalogConfig>,
}

impl<'de> Deserialize<'de> for CacheConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawCacheConfig::deserialize(deserializer)?;
        if let Some(catalog) = raw.catalog {
            return Ok(Self { catalog });
        }

        Ok(Self {
            catalog: CacheCatalogConfig {
                backend: raw.backend.unwrap_or_default(),
                ttl: raw.ttl.unwrap_or_else(default_ttl_seconds),
                negative_ttl: raw
                    .negative_ttl
                    .unwrap_or_else(default_negative_ttl_seconds),
                redis: raw.redis.unwrap_or_default(),
            },
        })
    }
}

impl CacheConfig {
    pub fn catalog_backend(&self) -> CacheBackendType {
        self.catalog.backend.clone()
    }

    pub fn catalog_ttl(&self) -> Duration {
        self.catalog.ttl()
    }

    pub fn catalog_negative_ttl(&self) -> Duration {
        self.catalog.negative_ttl()
    }

    pub fn catalog_redis_key_prefix(&self) -> &str {
        &self.catalog.redis.key_prefix
    }
}

// --- START RUNTIME STATE CONFIG ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeStateBackendType {
    Memory,
    Redis,
}

impl Default for RuntimeStateBackendType {
    fn default() -> Self {
        Self::Memory
    }
}

impl RuntimeStateBackendType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimeStateBackendType::Memory => "memory",
            RuntimeStateBackendType::Redis => "redis",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStateRedisConfig {
    #[serde(default = "default_runtime_state_redis_key_prefix")]
    pub key_prefix: String,
    #[serde(default = "default_api_key_concurrency_lease_ttl_seconds")]
    pub api_key_concurrency_lease_ttl_seconds: u64,
    #[serde(default = "default_provider_circuit_probe_lease_ttl_seconds")]
    pub provider_circuit_probe_lease_ttl_seconds: u64,
    #[serde(default = "default_runtime_state_ttl_seconds")]
    pub state_ttl_seconds: u64,
}

impl Default for RuntimeStateRedisConfig {
    fn default() -> Self {
        Self {
            key_prefix: default_runtime_state_redis_key_prefix(),
            api_key_concurrency_lease_ttl_seconds: default_api_key_concurrency_lease_ttl_seconds(),
            provider_circuit_probe_lease_ttl_seconds:
                default_provider_circuit_probe_lease_ttl_seconds(),
            state_ttl_seconds: default_runtime_state_ttl_seconds(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStateConfig {
    #[serde(default)]
    pub backend: RuntimeStateBackendType,
    #[serde(default)]
    pub redis: RuntimeStateRedisConfig,
    #[serde(default)]
    pub fallback_to_memory: bool,
    #[serde(default = "default_reasoning_continuation_ttl_seconds")]
    pub reasoning_continuation_ttl_seconds: u64,
    #[serde(default = "default_reasoning_continuation_memory_capacity")]
    pub reasoning_continuation_memory_capacity: usize,
}

impl Default for RuntimeStateConfig {
    fn default() -> Self {
        Self {
            backend: RuntimeStateBackendType::default(),
            redis: RuntimeStateRedisConfig::default(),
            fallback_to_memory: false,
            reasoning_continuation_ttl_seconds: default_reasoning_continuation_ttl_seconds(),
            reasoning_continuation_memory_capacity: default_reasoning_continuation_memory_capacity(
            ),
        }
    }
}

impl RuntimeStateConfig {
    pub fn api_key_concurrency_lease_ttl(&self) -> Duration {
        Duration::from_secs(self.redis.api_key_concurrency_lease_ttl_seconds)
    }

    pub fn provider_circuit_probe_lease_ttl(&self) -> Duration {
        Duration::from_secs(self.redis.provider_circuit_probe_lease_ttl_seconds)
    }

    pub fn state_ttl(&self) -> Duration {
        Duration::from_secs(self.redis.state_ttl_seconds)
    }

    pub fn reasoning_continuation_ttl(&self) -> Duration {
        Duration::from_secs(self.reasoning_continuation_ttl_seconds)
    }
}

// --- START PROXY REQUEST CONFIG ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyRequestConfig {
    #[serde(default = "default_proxy_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
    #[serde(default)]
    pub first_byte_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub total_timeout_seconds: Option<u64>,
}

impl Default for ProxyRequestConfig {
    fn default() -> Self {
        Self {
            connect_timeout_seconds: default_proxy_connect_timeout_seconds(),
            first_byte_timeout_seconds: default_proxy_first_byte_timeout_seconds(),
            total_timeout_seconds: None,
        }
    }
}

impl ProxyRequestConfig {
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_seconds)
    }

    pub fn first_byte_timeout(&self) -> Option<Duration> {
        self.first_byte_timeout_seconds.map(Duration::from_secs)
    }

    pub fn total_timeout(&self) -> Option<Duration> {
        self.total_timeout_seconds.map(Duration::from_secs)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderGovernanceConfig {
    #[serde(default = "default_provider_governance_enabled")]
    pub enabled: bool,
    #[serde(default = "default_provider_governance_consecutive_failure_threshold")]
    pub consecutive_failure_threshold: u32,
    #[serde(default = "default_provider_governance_open_cooldown_seconds")]
    pub open_cooldown_seconds: u64,
}

impl Default for ProviderGovernanceConfig {
    fn default() -> Self {
        Self {
            enabled: default_provider_governance_enabled(),
            consecutive_failure_threshold:
                default_provider_governance_consecutive_failure_threshold(),
            open_cooldown_seconds: default_provider_governance_open_cooldown_seconds(),
        }
    }
}

impl ProviderGovernanceConfig {
    pub fn open_cooldown(&self) -> Duration {
        Duration::from_secs(self.open_cooldown_seconds)
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled && self.consecutive_failure_threshold > 0
    }
}

// Default values for cache
fn default_ttl_seconds() -> u64 {
    3600 // 1 hour
}

fn default_negative_ttl_seconds() -> u64 {
    60 // 1 minute
}

fn default_proxy_connect_timeout_seconds() -> u64 {
    10
}

fn default_proxy_first_byte_timeout_seconds() -> Option<u64> {
    Some(60)
}

fn default_provider_governance_enabled() -> bool {
    true
}

fn default_provider_governance_consecutive_failure_threshold() -> u32 {
    5
}

fn default_provider_governance_open_cooldown_seconds() -> u64 {
    30
}

fn default_pool_size() -> usize {
    10
}

fn default_key_prefix() -> String {
    "cyder:".to_string()
}

fn default_cache_redis_key_prefix() -> String {
    "cache:".to_string()
}

fn default_runtime_state_redis_key_prefix() -> String {
    "runtime:".to_string()
}

fn default_api_key_concurrency_lease_ttl_seconds() -> u64 {
    900
}

fn default_provider_circuit_probe_lease_ttl_seconds() -> u64 {
    600
}

fn default_runtime_state_ttl_seconds() -> u64 {
    30 * 24 * 60 * 60
}

fn default_reasoning_continuation_ttl_seconds() -> u64 {
    30 * 60
}

fn default_reasoning_continuation_memory_capacity() -> usize {
    4096
}

fn default_redis_url() -> String {
    "redis://127.0.0.1:6379/".to_string()
}

fn default_metrics_enabled() -> bool {
    true
}

fn default_metrics_rollup_bucket_seconds() -> u64 {
    60
}

fn default_metrics_ingest_batch_size() -> usize {
    500
}

fn default_metrics_reconciliation_batch_size() -> usize {
    500
}

fn default_metrics_provider_runtime_default_window_seconds() -> u64 {
    3_600
}

fn default_metrics_request_log_query_fallback_enabled() -> bool {
    true
}

fn default_metrics_reconciliation_worker_interval_seconds() -> u64 {
    60
}

fn default_metrics_reconciliation_worker_recent_window_seconds() -> u64 {
    3_600
}

fn default_metrics_reconciliation_worker_safety_lag_seconds() -> u64 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricsConfig {
    #[serde(default = "default_metrics_enabled")]
    pub enabled: bool,
    #[serde(default = "default_metrics_rollup_bucket_seconds")]
    pub rollup_bucket_seconds: u64,
    #[serde(default = "default_metrics_ingest_batch_size")]
    pub ingest_batch_size: usize,
    #[serde(default = "default_metrics_reconciliation_batch_size")]
    pub reconciliation_batch_size: usize,
    #[serde(default = "default_metrics_provider_runtime_default_window_seconds")]
    pub provider_runtime_default_window_seconds: u64,
    #[serde(default = "default_metrics_request_log_query_fallback_enabled")]
    pub request_log_query_fallback_enabled: bool,
    #[serde(default = "default_metrics_reconciliation_worker_interval_seconds")]
    pub reconciliation_worker_interval_seconds: u64,
    #[serde(default = "default_metrics_reconciliation_worker_recent_window_seconds")]
    pub reconciliation_worker_recent_window_seconds: u64,
    #[serde(default = "default_metrics_reconciliation_worker_safety_lag_seconds")]
    pub reconciliation_worker_safety_lag_seconds: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: default_metrics_enabled(),
            rollup_bucket_seconds: default_metrics_rollup_bucket_seconds(),
            ingest_batch_size: default_metrics_ingest_batch_size(),
            reconciliation_batch_size: default_metrics_reconciliation_batch_size(),
            provider_runtime_default_window_seconds:
                default_metrics_provider_runtime_default_window_seconds(),
            request_log_query_fallback_enabled: default_metrics_request_log_query_fallback_enabled(
            ),
            reconciliation_worker_interval_seconds:
                default_metrics_reconciliation_worker_interval_seconds(),
            reconciliation_worker_recent_window_seconds:
                default_metrics_reconciliation_worker_recent_window_seconds(),
            reconciliation_worker_safety_lag_seconds:
                default_metrics_reconciliation_worker_safety_lag_seconds(),
        }
    }
}

// The fully resolved configuration used by the application.
// This is also the format for the default configuration file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FinalConfig {
    pub host: String,
    pub port: u16,
    pub base_path: String,
    pub jwt_secret: String,
    pub api_key_jwt_secret: String,
    pub db_url: String,
    pub proxy: Option<String>,
    pub log_level: String,
    pub timezone: Option<String>,
    pub max_body_size: usize,
    #[serde(default)]
    pub metrics: MetricsConfig,
    pub db_pool_size: u32,
    pub redis: Option<RedisConfig>,
    #[serde(default)]
    pub deployment: DeploymentConfig,
    #[serde(default)]
    pub id: IdConfig,
    #[serde(default)]
    pub proxy_request: ProxyRequestConfig,
    #[serde(default)]
    pub provider_governance: ProviderGovernanceConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub runtime_state: RuntimeStateConfig,
}

impl FinalConfig {
    pub fn deployment_mode(&self) -> &DeploymentMode {
        &self.deployment.mode
    }

    pub fn uses_postgres_database(&self) -> bool {
        self.db_url
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("postgres")
    }

    pub fn validate_deployment_runtime_state(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        if self.deployment.mode == DeploymentMode::MultiInstance {
            if !self.uses_postgres_database() {
                errors.push(
                    "deployment.mode=multi_instance requires a shared PostgreSQL database"
                        .to_string(),
                );
            }
            if self.cache.catalog_backend() != CacheBackendType::Redis {
                errors.push(
                    "deployment.mode=multi_instance requires cache.catalog.backend=redis"
                        .to_string(),
                );
            }
            if self.runtime_state.backend != RuntimeStateBackendType::Redis {
                errors.push(
                    "deployment.mode=multi_instance requires runtime_state.backend=redis"
                        .to_string(),
                );
            }
            if self.runtime_state.fallback_to_memory {
                errors.push(
                    "deployment.mode=multi_instance requires runtime_state.fallback_to_memory=false"
                        .to_string(),
                );
            }
        }

        if self.runtime_state.backend == RuntimeStateBackendType::Redis
            && self.redis.is_none()
            && !self.runtime_state.fallback_to_memory
        {
            errors.push("runtime_state.backend=redis requires redis configuration".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

fn generate_random_string(len: usize) -> String {
    rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

#[derive(Debug)]
pub enum ConfigInitError {
    Bootstrap(persistence::ConfigBootstrapError),
    Load(loader::ConfigLoadError),
}

impl fmt::Display for ConfigInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bootstrap(err) => write!(f, "failed to bootstrap configuration paths: {err}"),
            Self::Load(err) => write!(f, "failed to load configuration: {err}"),
        }
    }
}

impl std::error::Error for ConfigInitError {}

pub fn load_bootstrapped_config() -> Result<FinalConfig, ConfigInitError> {
    let paths = paths::ConfigPaths::for_current_build();
    persistence::bootstrap_config_paths(&paths).map_err(ConfigInitError::Bootstrap)?;
    loader::load_effective_config(&paths, loader::ConfigLoadOptions::default())
        .map_err(ConfigInitError::Load)
}

pub static CONFIG: LazyLock<FinalConfig> = LazyLock::new(|| {
    load_bootstrapped_config()
        .unwrap_or_else(|err| panic!("Failed to initialize configuration: {err}"))
});

pub(crate) fn programmatic_default_config() -> FinalConfig {
    FinalConfig {
        host: "0.0.0.0".to_string(),
        port: 8000,
        base_path: "/ai".to_string(),
        jwt_secret: generate_random_string(48),
        api_key_jwt_secret: generate_random_string(48),
        db_url: "/data/cyder/db/cyder.sqlite".to_string(),
        proxy: None,
        log_level: "info".to_string(),
        timezone: None,
        max_body_size: 100 * 1024 * 1024, // 100MB
        metrics: MetricsConfig::default(),
        db_pool_size: 5,
        redis: None,
        deployment: DeploymentConfig::default(),
        id: IdConfig::default(),
        proxy_request: ProxyRequestConfig::default(),
        provider_governance: ProviderGovernanceConfig::default(),
        cache: CacheConfig::default(),
        runtime_state: RuntimeStateConfig::default(),
    }
}

pub(crate) fn programmatic_default_config_for_paths(paths: &paths::ConfigPaths) -> FinalConfig {
    let mut config = programmatic_default_config();
    if paths.persistence.data_dir.is_some() {
        config.db_url = paths.persistence.sqlite_db_path.display().to_string();
    }
    config
}

pub(crate) fn finalize_loaded_config(final_config: FinalConfig) -> FinalConfig {
    final_config
}
