use rand::{Rng, distr::Alphanumeric, rng};
use serde::{Deserialize, Deserializer, Serialize};
use std::{fs, path::Path, sync::LazyLock, time::Duration};

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
}

impl Default for RuntimeStateConfig {
    fn default() -> Self {
        Self {
            backend: RuntimeStateBackendType::default(),
            redis: RuntimeStateRedisConfig::default(),
            fallback_to_memory: false,
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
}

// --- START PROXY REQUEST CONFIG ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub const DEFAULT_SAME_CANDIDATE_MAX_RETRIES: u32 = 1;
pub const DEFAULT_MAX_CANDIDATES_PER_REQUEST: u32 = 2;
pub const DEFAULT_BASE_BACKOFF_MS: u64 = 250;
pub const DEFAULT_MAX_BACKOFF_MS: u64 = 1500;
pub const DEFAULT_RESPECT_RETRY_AFTER_UP_TO_SECONDS: u64 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingResilienceConfig {
    #[serde(default = "default_same_candidate_max_retries")]
    pub same_candidate_max_retries: u32,
    #[serde(default = "default_max_candidates_per_request")]
    pub max_candidates_per_request: u32,
    #[serde(default = "default_base_backoff_ms")]
    pub base_backoff_ms: u64,
    #[serde(default = "default_max_backoff_ms")]
    pub max_backoff_ms: u64,
    #[serde(default = "default_respect_retry_after_up_to_seconds")]
    pub respect_retry_after_up_to_seconds: u64,
}

impl Default for RoutingResilienceConfig {
    fn default() -> Self {
        Self {
            same_candidate_max_retries: default_same_candidate_max_retries(),
            max_candidates_per_request: default_max_candidates_per_request(),
            base_backoff_ms: default_base_backoff_ms(),
            max_backoff_ms: default_max_backoff_ms(),
            respect_retry_after_up_to_seconds: default_respect_retry_after_up_to_seconds(),
        }
    }
}

// --- START STORAGE CONFIG ---

/// S3 Access Mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum S3AccessMode {
    Proxy,
    Redirect,
}

impl Default for S3AccessMode {
    fn default() -> Self {
        S3AccessMode::Proxy
    }
}

/// Storage driver type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StorageDriver {
    Local,
    S3,
}

impl Default for StorageDriver {
    fn default() -> Self {
        StorageDriver::Local
    }
}

/// Local storage specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalStorageConfig {
    #[serde(default = "default_local_storage_root")]
    pub root: String,
}

impl Default for LocalStorageConfig {
    fn default() -> Self {
        Self {
            root: default_local_storage_root(),
        }
    }
}

/// S3 storage specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3StorageConfig {
    pub endpoint: Option<String>,
    pub region: Option<String>,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub access_mode: S3AccessMode,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    #[serde(default)]
    pub force_path_style: bool,
    pub public_url: Option<String>,
}

/// Overall storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default)]
    pub driver: StorageDriver,
    #[serde(default)]
    pub local: LocalStorageConfig,
    pub s3: Option<S3StorageConfig>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            driver: StorageDriver::default(),
            local: LocalStorageConfig::default(),
            s3: None,
        }
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

fn default_same_candidate_max_retries() -> u32 {
    DEFAULT_SAME_CANDIDATE_MAX_RETRIES
}

fn default_max_candidates_per_request() -> u32 {
    DEFAULT_MAX_CANDIDATES_PER_REQUEST
}

fn default_base_backoff_ms() -> u64 {
    DEFAULT_BASE_BACKOFF_MS
}

fn default_max_backoff_ms() -> u64 {
    DEFAULT_MAX_BACKOFF_MS
}

fn default_respect_retry_after_up_to_seconds() -> u64 {
    DEFAULT_RESPECT_RETRY_AFTER_UP_TO_SECONDS
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

fn default_redis_url() -> String {
    "redis://127.0.0.1:6379/".to_string()
}

fn default_local_storage_root() -> String {
    "storage/storage".to_string()
}

fn default_replay_response_capture_max_bytes() -> usize {
    4 * 1024 * 1024
}

pub const DEFAULT_DIAGNOSTICS_REPLAY_PREVIEW_CONFIRMATION_TTL_SECONDS: u64 = 15 * 60;
pub const DEFAULT_DIAGNOSTICS_REPLAY_PREVIEW_CONFIRMATION_CLOCK_SKEW_SECONDS: u64 = 60;
pub const DEFAULT_DIAGNOSTICS_RESPONSE_CAPTURE_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_DIAGNOSTICS_RAW_BUNDLE_DOWNLOAD_ENABLED: bool = true;
pub const DEFAULT_DIAGNOSTICS_RETENTION_ENABLED: bool = false;
pub const DEFAULT_DIAGNOSTICS_REQUEST_LOG_BUNDLE_RETENTION_DAYS: u64 = 30;
pub const DEFAULT_DIAGNOSTICS_REPLAY_ARTIFACT_RETENTION_DAYS: u64 = 30;
pub const DEFAULT_DIAGNOSTICS_RETENTION_DELETE_BATCH_SIZE: usize = 200;

fn default_diagnostics_replay_preview_confirmation_ttl_seconds() -> u64 {
    DEFAULT_DIAGNOSTICS_REPLAY_PREVIEW_CONFIRMATION_TTL_SECONDS
}

fn default_diagnostics_replay_preview_confirmation_clock_skew_seconds() -> u64 {
    DEFAULT_DIAGNOSTICS_REPLAY_PREVIEW_CONFIRMATION_CLOCK_SKEW_SECONDS
}

fn default_diagnostics_response_capture_max_bytes() -> usize {
    DEFAULT_DIAGNOSTICS_RESPONSE_CAPTURE_MAX_BYTES
}

fn default_diagnostics_raw_bundle_download_enabled() -> bool {
    DEFAULT_DIAGNOSTICS_RAW_BUNDLE_DOWNLOAD_ENABLED
}

fn default_diagnostics_retention_enabled() -> bool {
    DEFAULT_DIAGNOSTICS_RETENTION_ENABLED
}

fn default_diagnostics_request_log_bundle_retention_days() -> u64 {
    DEFAULT_DIAGNOSTICS_REQUEST_LOG_BUNDLE_RETENTION_DAYS
}

fn default_diagnostics_replay_artifact_retention_days() -> u64 {
    DEFAULT_DIAGNOSTICS_REPLAY_ARTIFACT_RETENTION_DAYS
}

fn default_diagnostics_retention_delete_batch_size() -> usize {
    DEFAULT_DIAGNOSTICS_RETENTION_DELETE_BATCH_SIZE
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticsRetentionConfig {
    #[serde(default = "default_diagnostics_retention_enabled")]
    pub enabled: bool,
    #[serde(default = "default_diagnostics_request_log_bundle_retention_days")]
    pub request_log_bundle_retention_days: u64,
    #[serde(default = "default_diagnostics_replay_artifact_retention_days")]
    pub replay_artifact_retention_days: u64,
    #[serde(default = "default_diagnostics_retention_delete_batch_size")]
    pub delete_batch_size: usize,
}

impl Default for DiagnosticsRetentionConfig {
    fn default() -> Self {
        Self {
            enabled: default_diagnostics_retention_enabled(),
            request_log_bundle_retention_days:
                default_diagnostics_request_log_bundle_retention_days(),
            replay_artifact_retention_days: default_diagnostics_replay_artifact_retention_days(),
            delete_batch_size: default_diagnostics_retention_delete_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticsConfig {
    #[serde(default = "default_diagnostics_replay_preview_confirmation_ttl_seconds")]
    pub replay_preview_confirmation_ttl_seconds: u64,
    #[serde(default = "default_diagnostics_replay_preview_confirmation_clock_skew_seconds")]
    pub replay_preview_confirmation_clock_skew_seconds: u64,
    #[serde(default = "default_diagnostics_response_capture_max_bytes")]
    pub response_capture_max_bytes: usize,
    #[serde(default = "default_diagnostics_raw_bundle_download_enabled")]
    pub raw_bundle_download_enabled: bool,
    #[serde(default)]
    pub retention: DiagnosticsRetentionConfig,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            replay_preview_confirmation_ttl_seconds:
                default_diagnostics_replay_preview_confirmation_ttl_seconds(),
            replay_preview_confirmation_clock_skew_seconds:
                default_diagnostics_replay_preview_confirmation_clock_skew_seconds(),
            response_capture_max_bytes: default_diagnostics_response_capture_max_bytes(),
            raw_bundle_download_enabled: default_diagnostics_raw_bundle_download_enabled(),
            retention: DiagnosticsRetentionConfig::default(),
        }
    }
}

// The fully resolved configuration used by the application.
// This is also the format for the default configuration file.
#[derive(Debug, Deserialize, Serialize)]
pub struct FinalConfig {
    pub host: String,
    pub port: u16,
    pub base_path: String,
    pub secret_key: String,
    pub password_salt: String,
    pub jwt_secret: String,
    pub api_key_jwt_secret: String,
    pub db_url: String,
    pub proxy: Option<String>,
    pub log_level: String,
    pub timezone: Option<String>,
    pub max_body_size: usize,
    #[serde(default = "default_replay_response_capture_max_bytes")]
    pub replay_response_capture_max_bytes: usize,
    #[serde(default)]
    pub diagnostics: DiagnosticsConfig,
    pub db_pool_size: u32,
    pub redis: Option<RedisConfig>,
    #[serde(default)]
    pub deployment: DeploymentConfig,
    #[serde(default)]
    pub proxy_request: ProxyRequestConfig,
    #[serde(default)]
    pub provider_governance: ProviderGovernanceConfig,
    #[serde(default)]
    pub routing_resilience: RoutingResilienceConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub runtime_state: RuntimeStateConfig,
    #[serde(default)]
    pub storage: StorageConfig,
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

pub static CONFIG: LazyLock<FinalConfig> = LazyLock::new(|| {
    let default_config_path = if cfg!(debug_assertions) {
        Path::new("../config.default.yaml")
    } else {
        Path::new("config.default.yaml")
    };
    let user_config_path_release = Path::new("config.yaml");
    let user_config_path_dev_primary = Path::new("../config.local.yaml");
    let user_config_path_dev_fallback = Path::new("../config.yaml");

    // Determine which user config file to use for overrides
    let user_config_path = if cfg!(debug_assertions) {
        if user_config_path_dev_primary.exists() {
            user_config_path_dev_primary
        } else {
            user_config_path_dev_fallback
        }
    } else {
        user_config_path_release
    };

    // Create a FinalConfig with programmatic defaults.
    let effective_default_config = FinalConfig {
        host: "0.0.0.0".to_string(),
        port: 8000,
        base_path: "/ai".to_string(),
        secret_key: generate_random_string(48),
        password_salt: generate_random_string(48),
        jwt_secret: generate_random_string(48),
        api_key_jwt_secret: generate_random_string(48),
        db_url: "./storage/sqlite.db".to_string(),
        proxy: None,
        log_level: "info".to_string(),
        timezone: None,
        max_body_size: 100 * 1024 * 1024, // 100MB
        replay_response_capture_max_bytes: default_replay_response_capture_max_bytes(),
        diagnostics: DiagnosticsConfig::default(),
        db_pool_size: 5,
        redis: None,
        deployment: DeploymentConfig::default(),
        proxy_request: ProxyRequestConfig::default(),
        provider_governance: ProviderGovernanceConfig::default(),
        routing_resilience: RoutingResilienceConfig::default(),
        cache: CacheConfig::default(),
        runtime_state: RuntimeStateConfig::default(),
        storage: StorageConfig::default(),
    };

    let default_yaml_str = serde_yaml::to_string(&effective_default_config).unwrap();

    // First stage: parse default config
    let mut default_builder = config::Config::builder().add_source(config::File::from_str(
        &default_yaml_str,
        config::FileFormat::Yaml,
    ));

    if default_config_path.exists() {
        default_builder =
            default_builder.add_source(config::File::from(default_config_path).required(false));
    }

    let default_config: FinalConfig = default_builder
        .build()
        .expect("Failed to build default block in config")
        .try_deserialize()
        .expect("Failed to deserialize default configuration");

    let merged_default_yaml = serde_yaml::to_string(&default_config).unwrap();
    fs::write(default_config_path, &merged_default_yaml)
        .unwrap_or_else(|err| panic!("Failed to write default configuration file: {}", err));

    // Second stage: user config and optionally override env vars
    let mut builder = config::Config::builder().add_source(config::File::from_str(
        &merged_default_yaml,
        config::FileFormat::Yaml,
    ));

    if user_config_path.exists() {
        builder = builder.add_source(config::File::from(user_config_path).required(false));
    }

    // Load configuration from environment variables, which have the highest priority.
    let env_config = config::Environment::default()
        .try_parsing(true)
        .ignore_empty(true);

    builder = builder.add_source(env_config);

    let final_config: FinalConfig = builder
        .build()
        .expect("Failed to build user and environment merged config")
        .try_deserialize()
        .expect("Failed to deserialize final configuration from merged tree");

    finalize_loaded_config(final_config)
});

fn finalize_loaded_config(mut final_config: FinalConfig) -> FinalConfig {
    let legacy_default = default_replay_response_capture_max_bytes();
    let diagnostics_default = default_diagnostics_response_capture_max_bytes();
    if final_config.diagnostics.response_capture_max_bytes == diagnostics_default
        && final_config.replay_response_capture_max_bytes != legacy_default
    {
        final_config.diagnostics.response_capture_max_bytes =
            final_config.replay_response_capture_max_bytes;
    } else {
        final_config.replay_response_capture_max_bytes =
            final_config.diagnostics.response_capture_max_bytes;
    }

    final_config
}

#[cfg(test)]
mod tests {
    use super::{
        CacheBackendType, DeploymentMode, DiagnosticsConfig, FinalConfig, ProviderGovernanceConfig,
        ProxyRequestConfig, RoutingResilienceConfig, RuntimeStateBackendType, StorageDriver,
        default_replay_response_capture_max_bytes,
    };

    #[test]
    fn proxy_request_config_defaults_keep_overall_timeout_disabled() {
        let config = ProxyRequestConfig::default();
        assert_eq!(config.connect_timeout_seconds, 10);
        assert_eq!(config.connect_timeout().as_secs(), 10);
        assert_eq!(
            config.first_byte_timeout().map(|value| value.as_secs()),
            Some(60)
        );
        assert_eq!(config.total_timeout(), None);
    }

    #[test]
    fn provider_governance_defaults_are_conservative() {
        let config = ProviderGovernanceConfig::default();
        assert!(config.enabled);
        assert_eq!(config.consecutive_failure_threshold, 5);
        assert_eq!(config.open_cooldown().as_secs(), 30);
        assert!(config.is_enabled());
    }

    #[test]
    fn routing_resilience_defaults_match_task_contract() {
        let config = RoutingResilienceConfig::default();
        assert_eq!(config.same_candidate_max_retries, 1);
        assert_eq!(config.max_candidates_per_request, 2);
        assert_eq!(config.base_backoff_ms, 250);
        assert_eq!(config.max_backoff_ms, 1500);
        assert_eq!(config.respect_retry_after_up_to_seconds, 3);
    }

    #[test]
    fn replay_response_capture_default_is_independent_from_request_body_limit() {
        assert_eq!(default_replay_response_capture_max_bytes(), 4 * 1024 * 1024);
        assert_eq!(
            DiagnosticsConfig::default().response_capture_max_bytes,
            4 * 1024 * 1024
        );
        assert_eq!(
            DiagnosticsConfig::default().replay_preview_confirmation_ttl_seconds,
            900
        );
        assert_eq!(
            DiagnosticsConfig::default().replay_preview_confirmation_clock_skew_seconds,
            60
        );
        assert!(DiagnosticsConfig::default().raw_bundle_download_enabled);
        assert!(!DiagnosticsConfig::default().retention.enabled);
    }

    #[test]
    fn final_config_accepts_diagnostics_domain_and_legacy_capture_alias() {
        let diagnostics_yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
diagnostics:
  replay_preview_confirmation_ttl_seconds: 30
  replay_preview_confirmation_clock_skew_seconds: 5
  response_capture_max_bytes: 1234
  raw_bundle_download_enabled: false
  retention:
    enabled: true
    request_log_bundle_retention_days: 7
    replay_artifact_retention_days: 8
    delete_batch_size: 9
"#;

        let config: FinalConfig =
            serde_yaml::from_str(diagnostics_yaml).expect("diagnostics config should deserialize");
        assert_eq!(config.diagnostics.response_capture_max_bytes, 1234);
        assert_eq!(
            config.diagnostics.replay_preview_confirmation_ttl_seconds,
            30
        );
        assert_eq!(
            config
                .diagnostics
                .replay_preview_confirmation_clock_skew_seconds,
            5
        );
        assert!(!config.diagnostics.raw_bundle_download_enabled);
        assert!(config.diagnostics.retention.enabled);
        assert_eq!(
            config
                .diagnostics
                .retention
                .request_log_bundle_retention_days,
            7
        );
        assert_eq!(
            config.diagnostics.retention.replay_artifact_retention_days,
            8
        );
        assert_eq!(config.diagnostics.retention.delete_batch_size, 9);

        let legacy_yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
replay_response_capture_max_bytes: 2048
db_pool_size: 5
"#;

        let legacy: FinalConfig =
            serde_yaml::from_str(legacy_yaml).expect("legacy capture config should deserialize");
        let finalized = super::finalize_loaded_config(legacy);
        assert_eq!(finalized.diagnostics.response_capture_max_bytes, 2048);
        assert_eq!(finalized.replay_response_capture_max_bytes, 2048);
    }

    #[test]
    fn final_config_preserves_s3_driver_when_s3_config_is_missing() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
storage:
  driver: s3
  s3: null
"#;

        let config: FinalConfig =
            serde_yaml::from_str(yaml).expect("S3 config with null block should deserialize");
        let finalized = super::finalize_loaded_config(config);

        assert_eq!(finalized.storage.driver, StorageDriver::S3);
        assert!(finalized.storage.s3.is_none());
    }

    #[test]
    fn final_config_preserves_s3_driver_when_s3_config_is_incomplete() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
storage:
  driver: s3
  s3:
    region: null
    bucket: ""
    access_key: "   "
    secret_key: ""
"#;

        let config: FinalConfig =
            serde_yaml::from_str(yaml).expect("half configured S3 should deserialize");
        let finalized = super::finalize_loaded_config(config);
        let s3 = finalized
            .storage
            .s3
            .as_ref()
            .expect("S3 config block should remain available for validation");

        assert_eq!(finalized.storage.driver, StorageDriver::S3);
        assert!(s3.region.is_none());
        assert!(s3.bucket.is_empty());
        assert_eq!(s3.access_key.as_deref(), Some("   "));
        assert_eq!(s3.secret_key.as_deref(), Some(""));
    }

    #[test]
    fn final_config_deserializes_proxy_request_and_resilience_overrides() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
proxy_request:
  connect_timeout_seconds: 3
  first_byte_timeout_seconds: 15
  total_timeout_seconds: 600
provider_governance:
  enabled: true
  consecutive_failure_threshold: 7
  open_cooldown_seconds: 45
routing_resilience:
  same_candidate_max_retries: 2
  max_candidates_per_request: 3
  base_backoff_ms: 125
  max_backoff_ms: 1000
  respect_retry_after_up_to_seconds: 2
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        assert_eq!(config.proxy_request.connect_timeout_seconds, 3);
        assert_eq!(config.proxy_request.first_byte_timeout_seconds, Some(15));
        assert_eq!(config.proxy_request.total_timeout_seconds, Some(600));
        assert!(config.provider_governance.enabled);
        assert_eq!(config.provider_governance.consecutive_failure_threshold, 7);
        assert_eq!(config.provider_governance.open_cooldown_seconds, 45);
        assert_eq!(config.routing_resilience.same_candidate_max_retries, 2);
        assert_eq!(config.routing_resilience.max_candidates_per_request, 3);
        assert_eq!(config.routing_resilience.base_backoff_ms, 125);
        assert_eq!(config.routing_resilience.max_backoff_ms, 1000);
        assert_eq!(
            config.routing_resilience.respect_retry_after_up_to_seconds,
            2
        );
        assert_eq!(
            config
                .proxy_request
                .total_timeout()
                .map(|value| value.as_secs()),
            Some(600)
        );
    }

    #[test]
    fn final_config_deserializes_runtime_state_and_catalog_cache_domains() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: postgres://cyder:cyder@localhost/cyder
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
redis:
  url: redis://127.0.0.1:6379/
  pool_size: 10
  key_prefix: "cyder:"
deployment:
  mode: multi_instance
cache:
  catalog:
    backend: redis
    ttl: 120
    negative_ttl: 10
    redis:
      key_prefix: "catalog:"
runtime_state:
  backend: redis
  fallback_to_memory: false
  redis:
    key_prefix: "runtime:"
    api_key_concurrency_lease_ttl_seconds: 11
    provider_circuit_probe_lease_ttl_seconds: 22
    state_ttl_seconds: 33
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        assert_eq!(config.deployment.mode, DeploymentMode::MultiInstance);
        assert_eq!(config.cache.catalog_backend(), CacheBackendType::Redis);
        assert_eq!(config.cache.catalog_ttl().as_secs(), 120);
        assert_eq!(config.cache.catalog_negative_ttl().as_secs(), 10);
        assert_eq!(config.cache.catalog_redis_key_prefix(), "catalog:");
        assert_eq!(config.runtime_state.backend, RuntimeStateBackendType::Redis);
        assert_eq!(
            config
                .runtime_state
                .api_key_concurrency_lease_ttl()
                .as_secs(),
            11
        );
        assert_eq!(
            config
                .runtime_state
                .provider_circuit_probe_lease_ttl()
                .as_secs(),
            22
        );
        assert_eq!(config.runtime_state.state_ttl().as_secs(), 33);
        assert!(config.validate_deployment_runtime_state().is_ok());
    }

    #[test]
    fn cache_config_accepts_legacy_backend_shorthand() {
        let yaml = r#"
backend: redis
ttl: 120
negative_ttl: 10
redis:
  key_prefix: "legacy-cache:"
"#;

        let cache: super::CacheConfig = serde_yaml::from_str(yaml).expect("cache should parse");
        assert_eq!(cache.catalog_backend(), CacheBackendType::Redis);
        assert_eq!(cache.catalog_ttl().as_secs(), 120);
        assert_eq!(cache.catalog_negative_ttl().as_secs(), 10);
        assert_eq!(cache.catalog_redis_key_prefix(), "legacy-cache:");
    }

    #[test]
    fn cache_catalog_domain_takes_precedence_over_legacy_shorthand() {
        let yaml = r#"
backend: memory
ttl: 120
negative_ttl: 10
catalog:
  backend: redis
  ttl: 300
  negative_ttl: 30
  redis:
    key_prefix: "catalog:"
"#;

        let cache: super::CacheConfig = serde_yaml::from_str(yaml).expect("cache should parse");
        assert_eq!(cache.catalog_backend(), CacheBackendType::Redis);
        assert_eq!(cache.catalog_ttl().as_secs(), 300);
        assert_eq!(cache.catalog_negative_ttl().as_secs(), 30);
        assert_eq!(cache.catalog_redis_key_prefix(), "catalog:");
    }

    #[test]
    fn final_config_preserves_explicit_catalog_redis_without_redis_config() {
        let mut config: FinalConfig = serde_yaml::from_str(
            r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
redis: null
cache:
  catalog:
    backend: redis
runtime_state:
  backend: memory
"#,
        )
        .expect("config should deserialize");
        config.cache.catalog.backend = CacheBackendType::Redis;

        let finalized = super::finalize_loaded_config(config);

        assert!(finalized.redis.is_none());
        assert_eq!(finalized.cache.catalog_backend(), CacheBackendType::Redis);
        assert_eq!(
            finalized.runtime_state.backend,
            RuntimeStateBackendType::Memory
        );
        assert!(finalized.validate_deployment_runtime_state().is_ok());
    }

    #[test]
    fn default_single_instance_memory_runtime_does_not_require_redis() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        assert_eq!(config.deployment.mode, DeploymentMode::SingleInstance);
        assert_eq!(config.cache.catalog_backend(), CacheBackendType::Memory);
        assert_eq!(
            config.runtime_state.backend,
            RuntimeStateBackendType::Memory
        );
        assert!(config.redis.is_none());
        assert!(config.validate_deployment_runtime_state().is_ok());
    }

    #[test]
    fn single_instance_can_opt_into_redis_runtime_state_without_redis_catalog_cache() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
redis:
  url: redis://127.0.0.1:6379/
  pool_size: 10
  key_prefix: "cyder:"
runtime_state:
  backend: redis
  fallback_to_memory: false
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        assert_eq!(config.deployment.mode, DeploymentMode::SingleInstance);
        assert_eq!(config.cache.catalog_backend(), CacheBackendType::Memory);
        assert_eq!(config.runtime_state.backend, RuntimeStateBackendType::Redis);
        assert!(config.redis.is_some());
        assert!(config.validate_deployment_runtime_state().is_ok());
    }

    #[test]
    fn multi_instance_redis_runtime_state_requires_redis_configuration() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: postgres://cyder:cyder@localhost/cyder
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
deployment:
  mode: multi_instance
cache:
  catalog:
    backend: redis
runtime_state:
  backend: redis
  fallback_to_memory: false
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        let error = config
            .validate_deployment_runtime_state()
            .expect_err("multi-instance redis state should require redis config");
        assert!(error.contains("runtime_state.backend=redis requires redis configuration"));
    }

    #[test]
    fn multi_instance_rejects_memory_catalog_cache() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: postgres://cyder:cyder@localhost/cyder
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
redis:
  url: redis://127.0.0.1:6379/
deployment:
  mode: multi_instance
cache:
  catalog:
    backend: memory
runtime_state:
  backend: redis
  fallback_to_memory: false
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        let error = config
            .validate_deployment_runtime_state()
            .expect_err("multi-instance memory catalog cache should be rejected");
        assert!(error.contains("cache.catalog.backend=redis"));
        assert!(!error.contains("runtime_state.backend=redis"));
    }

    #[test]
    fn multi_instance_rejects_memory_runtime_state() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: postgres://cyder:cyder@localhost/cyder
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
redis:
  url: redis://127.0.0.1:6379/
deployment:
  mode: multi_instance
cache:
  catalog:
    backend: redis
runtime_state:
  backend: memory
  fallback_to_memory: false
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        let error = config
            .validate_deployment_runtime_state()
            .expect_err("multi-instance memory runtime state should be rejected");
        assert!(error.contains("runtime_state.backend=redis"));
        assert!(!error.contains("cache.catalog.backend=redis"));
    }

    #[test]
    fn multi_instance_rejects_local_sqlite_database() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
redis:
  url: redis://127.0.0.1:6379/
deployment:
  mode: multi_instance
cache:
  catalog:
    backend: redis
runtime_state:
  backend: redis
  fallback_to_memory: false
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        let error = config
            .validate_deployment_runtime_state()
            .expect_err("multi-instance sqlite database should be rejected");
        assert!(error.contains("shared PostgreSQL"));
        assert!(!error.contains("cache.catalog.backend=redis"));
        assert!(!error.contains("runtime_state.backend=redis"));
    }

    #[test]
    fn multi_instance_runtime_validation_rejects_local_state() {
        let yaml = r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
deployment:
  mode: multi_instance
cache:
  catalog:
    backend: memory
runtime_state:
  backend: memory
  fallback_to_memory: true
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        let error = config
            .validate_deployment_runtime_state()
            .expect_err("multi-instance local state should be rejected");
        assert!(error.contains("shared PostgreSQL"));
        assert!(error.contains("cache.catalog.backend=redis"));
        assert!(error.contains("runtime_state.backend=redis"));
        assert!(error.contains("runtime_state.fallback_to_memory=false"));
    }

    #[test]
    fn final_config_rejects_legacy_proxy_request_timeout_field() {
        let legacy_field = ["timeout", "_seconds"].concat();
        let yaml = format!(
            r#"
host: 0.0.0.0
port: 8000
base_path: /ai
secret_key: secret
password_salt: salt
jwt_secret: jwt
api_key_jwt_secret: api-jwt
db_url: ./storage/sqlite.db
proxy: null
log_level: info
timezone: null
max_body_size: 104857600
db_pool_size: 5
proxy_request:
  {legacy_field}: 123
"#
        );

        let error =
            serde_yaml::from_str::<FinalConfig>(&yaml).expect_err("legacy timeout field must fail");
        assert!(
            error
                .to_string()
                .contains(&format!("unknown field `{legacy_field}`")),
            "unexpected error: {error}"
        );
    }
}
