use rand::{Rng, distr::Alphanumeric, rng};
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, sync::LazyLock, time::Duration};

use crate::utils::ID_MAX_WORKER_ID;

pub mod env;
pub mod loader;
pub mod override_policy;
pub mod paths;
pub mod persistence;
pub mod source;

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

pub const DEFAULT_SAME_CANDIDATE_MAX_RETRIES: u32 = 1;
pub const DEFAULT_MAX_CANDIDATES_PER_REQUEST: u32 = 2;
pub const DEFAULT_BASE_BACKOFF_MS: u64 = 250;
pub const DEFAULT_MAX_BACKOFF_MS: u64 = 1500;
pub const DEFAULT_RESPECT_RETRY_AFTER_UP_TO_SECONDS: u64 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

fn default_reasoning_continuation_ttl_seconds() -> u64 {
    30 * 60
}

fn default_reasoning_continuation_memory_capacity() -> usize {
    4096
}

fn default_redis_url() -> String {
    "redis://127.0.0.1:6379/".to_string()
}

fn default_local_storage_root() -> String {
    "/data/cyder/storage".to_string()
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

fn default_alerts_enabled() -> bool {
    true
}

fn default_alerts_evaluation_interval_seconds() -> u64 {
    60
}

fn default_alerts_default_cooldown_seconds() -> u64 {
    900
}

fn default_alerts_provider_degraded_min_requests() -> i64 {
    5
}

fn default_alerts_provider_degraded_error_rate() -> f64 {
    0.2
}

fn default_alerts_provider_degraded_latency_ms() -> i64 {
    10_000
}

fn default_alerts_high_error_min_requests() -> i64 {
    20
}

fn default_alerts_high_error_rate() -> f64 {
    0.3
}

fn default_alerts_high_latency_min_samples() -> i64 {
    10
}

fn default_alerts_high_latency_ms() -> i64 {
    10_000
}

fn default_alerts_transform_diagnostic_count_threshold() -> i64 {
    20
}

fn default_alerts_transform_diagnostic_lossy_major_threshold() -> i64 {
    1
}

fn default_alerts_logging_pending_threshold() -> u64 {
    1_000
}

fn default_alerts_logging_in_flight_threshold() -> u64 {
    100
}

fn default_alerts_runtime_state_backend_degraded_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlertRulesConfig {
    #[serde(default = "default_alerts_provider_degraded_min_requests")]
    pub provider_degraded_min_requests: i64,
    #[serde(default = "default_alerts_provider_degraded_error_rate")]
    pub provider_degraded_error_rate: f64,
    #[serde(default = "default_alerts_provider_degraded_latency_ms")]
    pub provider_degraded_latency_ms: i64,
    #[serde(default = "default_alerts_high_error_min_requests")]
    pub high_error_min_requests: i64,
    #[serde(default = "default_alerts_high_error_rate")]
    pub high_error_rate: f64,
    #[serde(default = "default_alerts_high_latency_min_samples")]
    pub high_latency_min_samples: i64,
    #[serde(default = "default_alerts_high_latency_ms")]
    pub high_latency_ms: i64,
    #[serde(default = "default_alerts_transform_diagnostic_count_threshold")]
    pub transform_diagnostic_count_threshold: i64,
    #[serde(default = "default_alerts_transform_diagnostic_lossy_major_threshold")]
    pub transform_diagnostic_lossy_major_threshold: i64,
    #[serde(default)]
    pub cost_hotspot_amount_nanos: Option<i64>,
    #[serde(default = "default_alerts_logging_pending_threshold")]
    pub logging_pending_threshold: u64,
    #[serde(default = "default_alerts_logging_in_flight_threshold")]
    pub logging_in_flight_threshold: u64,
    #[serde(default = "default_alerts_runtime_state_backend_degraded_enabled")]
    pub runtime_state_backend_degraded_enabled: bool,
}

impl Default for AlertRulesConfig {
    fn default() -> Self {
        Self {
            provider_degraded_min_requests: default_alerts_provider_degraded_min_requests(),
            provider_degraded_error_rate: default_alerts_provider_degraded_error_rate(),
            provider_degraded_latency_ms: default_alerts_provider_degraded_latency_ms(),
            high_error_min_requests: default_alerts_high_error_min_requests(),
            high_error_rate: default_alerts_high_error_rate(),
            high_latency_min_samples: default_alerts_high_latency_min_samples(),
            high_latency_ms: default_alerts_high_latency_ms(),
            transform_diagnostic_count_threshold:
                default_alerts_transform_diagnostic_count_threshold(),
            transform_diagnostic_lossy_major_threshold:
                default_alerts_transform_diagnostic_lossy_major_threshold(),
            cost_hotspot_amount_nanos: None,
            logging_pending_threshold: default_alerts_logging_pending_threshold(),
            logging_in_flight_threshold: default_alerts_logging_in_flight_threshold(),
            runtime_state_backend_degraded_enabled:
                default_alerts_runtime_state_backend_degraded_enabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlertsConfig {
    #[serde(default = "default_alerts_enabled")]
    pub enabled: bool,
    #[serde(default = "default_alerts_evaluation_interval_seconds")]
    pub evaluation_interval_seconds: u64,
    #[serde(default = "default_alerts_default_cooldown_seconds")]
    pub default_cooldown_seconds: u64,
    #[serde(default)]
    pub rules: AlertRulesConfig,
}

impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            enabled: default_alerts_enabled(),
            evaluation_interval_seconds: default_alerts_evaluation_interval_seconds(),
            default_cooldown_seconds: default_alerts_default_cooldown_seconds(),
            rules: AlertRulesConfig::default(),
        }
    }
}

fn default_notification_enabled() -> bool {
    true
}

fn default_notification_worker_interval_seconds() -> u64 {
    10
}

fn default_notification_webhook_timeout_seconds() -> u64 {
    10
}

fn default_notification_max_delivery_attempts() -> u32 {
    5
}

fn default_notification_retry_base_backoff_seconds() -> u64 {
    30
}

fn default_notification_retry_max_backoff_seconds() -> u64 {
    900
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationConfig {
    #[serde(default = "default_notification_enabled")]
    pub enabled: bool,
    #[serde(default = "default_notification_worker_interval_seconds")]
    pub worker_interval_seconds: u64,
    #[serde(default = "default_notification_webhook_timeout_seconds")]
    pub webhook_timeout_seconds: u64,
    #[serde(default = "default_notification_max_delivery_attempts")]
    pub max_delivery_attempts: u32,
    #[serde(default = "default_notification_retry_base_backoff_seconds")]
    pub retry_base_backoff_seconds: u64,
    #[serde(default = "default_notification_retry_max_backoff_seconds")]
    pub retry_max_backoff_seconds: u64,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: default_notification_enabled(),
            worker_interval_seconds: default_notification_worker_interval_seconds(),
            webhook_timeout_seconds: default_notification_webhook_timeout_seconds(),
            max_delivery_attempts: default_notification_max_delivery_attempts(),
            retry_base_backoff_seconds: default_notification_retry_base_backoff_seconds(),
            retry_max_backoff_seconds: default_notification_retry_max_backoff_seconds(),
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
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub alerts: AlertsConfig,
    #[serde(default)]
    pub notification: NotificationConfig,
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

pub fn load_bootstrapped_config() -> Result<loader::LoadedConfig, ConfigInitError> {
    let paths = paths::ConfigPaths::for_current_build();
    persistence::bootstrap_config_paths(&paths).map_err(ConfigInitError::Bootstrap)?;
    loader::load_effective_config(&paths, loader::ConfigLoadOptions::default())
        .map_err(ConfigInitError::Load)
}

pub static LOADED_CONFIG: LazyLock<loader::LoadedConfig> = LazyLock::new(|| {
    load_bootstrapped_config()
        .unwrap_or_else(|err| panic!("Failed to initialize configuration: {err}"))
});

pub static CONFIG: LazyLock<FinalConfig> = LazyLock::new(|| LOADED_CONFIG.config.clone());

pub(crate) fn programmatic_default_config() -> FinalConfig {
    FinalConfig {
        host: "0.0.0.0".to_string(),
        port: 8000,
        base_path: "/ai".to_string(),
        secret_key: generate_random_string(48),
        password_salt: generate_random_string(48),
        jwt_secret: generate_random_string(48),
        api_key_jwt_secret: generate_random_string(48),
        db_url: "/data/cyder/db/cyder.sqlite".to_string(),
        proxy: None,
        log_level: "info".to_string(),
        timezone: None,
        max_body_size: 100 * 1024 * 1024, // 100MB
        replay_response_capture_max_bytes: default_replay_response_capture_max_bytes(),
        diagnostics: DiagnosticsConfig::default(),
        metrics: MetricsConfig::default(),
        alerts: AlertsConfig::default(),
        notification: NotificationConfig::default(),
        db_pool_size: 5,
        redis: None,
        deployment: DeploymentConfig::default(),
        id: IdConfig::default(),
        proxy_request: ProxyRequestConfig::default(),
        provider_governance: ProviderGovernanceConfig::default(),
        routing_resilience: RoutingResilienceConfig::default(),
        cache: CacheConfig::default(),
        runtime_state: RuntimeStateConfig::default(),
        storage: StorageConfig::default(),
    }
}

pub(crate) fn programmatic_default_config_for_paths(paths: &paths::ConfigPaths) -> FinalConfig {
    let mut config = programmatic_default_config();
    if paths.persistence.data_dir.is_some() {
        config.db_url = paths.persistence.sqlite_db_path.display().to_string();
        config.storage.local.root = paths.persistence.local_storage_root.display().to_string();
    }
    config
}

pub(crate) fn finalize_loaded_config(mut final_config: FinalConfig) -> FinalConfig {
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
        AlertsConfig, CacheBackendType, DeploymentMode, DiagnosticsConfig, FinalConfig, IdConfig,
        MetricsConfig, NotificationConfig, ProviderGovernanceConfig, ProxyRequestConfig,
        RoutingResilienceConfig, RuntimeStateBackendType, StorageDriver,
        default_replay_response_capture_max_bytes, source::ConfigLayerKind,
    };
    use std::{fs, path::Path};

    fn load_without_environment(paths: &super::paths::ConfigPaths) -> super::loader::LoadedConfig {
        super::loader::load_effective_config(
            paths,
            super::loader::ConfigLoadOptions {
                include_environment: false,
                include_override: true,
            },
        )
        .expect("config should load")
    }

    fn write_test_config(path: &Path, yaml: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("config parent should be created");
        }
        fs::write(path, yaml).expect("config file should be written");
    }

    fn load_with_test_environment(
        paths: &super::paths::ConfigPaths,
        pairs: &[(&str, &str)],
        include_override: bool,
    ) -> Result<super::loader::LoadedConfig, super::loader::ConfigLoadError> {
        let environment_source =
            super::env::source_from_pairs(pairs).expect("test environment should parse");
        super::loader::load_effective_config_with_environment_source(
            paths,
            super::loader::ConfigLoadOptions {
                include_environment: true,
                include_override,
            },
            environment_source,
        )
    }

    fn test_paths_from_persistence_env(
        data_dir: Option<&str>,
        config_path: Option<&str>,
        current_dir: &Path,
    ) -> super::paths::ConfigPaths {
        let resolved = super::persistence::resolve_path_set(
            super::persistence::PersistenceEnvironment::from_values(data_dir, config_path),
            super::persistence::BuildProfile::Release,
            current_dir.to_path_buf(),
            current_dir.join(".cyder").join("dev"),
        );
        super::paths::ConfigPaths {
            default_config_path: resolved.default_config_path,
            user_config_path: resolved.user_config_path,
            user_config_path_required: resolved.user_config_path_required,
            override_config_path: resolved.override_config_path,
            override_history_path: resolved.override_history_path,
            persistence: resolved.persistence,
            ignored_empty_environment_variables: resolved.ignored_empty_environment_variables,
        }
    }

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
    fn id_config_defaults_to_worker_one() {
        assert_eq!(IdConfig::default().worker_id, 1);
        assert_eq!(super::programmatic_default_config().id.worker_id, 1);
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
    fn metrics_alerts_notification_defaults_match_task_contract() {
        let metrics = MetricsConfig::default();
        assert!(metrics.enabled);
        assert_eq!(metrics.rollup_bucket_seconds, 60);
        assert_eq!(metrics.ingest_batch_size, 500);
        assert_eq!(metrics.reconciliation_batch_size, 500);
        assert_eq!(metrics.provider_runtime_default_window_seconds, 3_600);
        assert!(metrics.request_log_query_fallback_enabled);
        assert_eq!(metrics.reconciliation_worker_interval_seconds, 60);
        assert_eq!(metrics.reconciliation_worker_recent_window_seconds, 3_600);
        assert_eq!(metrics.reconciliation_worker_safety_lag_seconds, 5);

        let alerts = AlertsConfig::default();
        assert!(alerts.enabled);
        assert_eq!(alerts.evaluation_interval_seconds, 60);
        assert_eq!(alerts.default_cooldown_seconds, 900);
        assert_eq!(alerts.rules.provider_degraded_min_requests, 5);
        assert_eq!(alerts.rules.provider_degraded_error_rate, 0.2);
        assert_eq!(alerts.rules.provider_degraded_latency_ms, 10_000);
        assert_eq!(alerts.rules.high_error_min_requests, 20);
        assert_eq!(alerts.rules.high_error_rate, 0.3);
        assert_eq!(alerts.rules.high_latency_min_samples, 10);
        assert_eq!(alerts.rules.high_latency_ms, 10_000);
        assert_eq!(alerts.rules.transform_diagnostic_count_threshold, 20);
        assert_eq!(alerts.rules.transform_diagnostic_lossy_major_threshold, 1);
        assert_eq!(alerts.rules.cost_hotspot_amount_nanos, None);
        assert_eq!(alerts.rules.logging_pending_threshold, 1_000);
        assert_eq!(alerts.rules.logging_in_flight_threshold, 100);
        assert!(alerts.rules.runtime_state_backend_degraded_enabled);

        let notification = NotificationConfig::default();
        assert!(notification.enabled);
        assert_eq!(notification.worker_interval_seconds, 10);
        assert_eq!(notification.webhook_timeout_seconds, 10);
        assert_eq!(notification.max_delivery_attempts, 5);
        assert_eq!(notification.retry_base_backoff_seconds, 30);
        assert_eq!(notification.retry_max_backoff_seconds, 900);
    }

    #[test]
    fn config_loader_reads_user_config_without_bootstrap_side_effects() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(
            &paths.user_config_path,
            r#"
port: 3456
log_level: debug
proxy_request:
  first_byte_timeout_seconds: 45
"#,
        );

        let loaded = load_without_environment(&paths);

        assert_eq!(loaded.config.port, 3456);
        assert_eq!(loaded.config.log_level, "debug");
        assert_eq!(
            loaded.config.proxy_request.first_byte_timeout_seconds,
            Some(45)
        );
        assert_eq!(loaded.config.proxy_request.connect_timeout_seconds, 10);
        assert!(!paths.default_config_path.exists());
    }

    #[test]
    fn config_loader_reads_id_worker_id_from_user_config() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(
            &paths.user_config_path,
            r#"
id:
  worker_id: 31
"#,
        );

        let loaded = load_without_environment(&paths);

        assert_eq!(loaded.config.id.worker_id, 31);
    }

    #[test]
    fn config_loader_rejects_out_of_range_id_worker_id() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(
            &paths.user_config_path,
            r#"
id:
  worker_id: 32
"#,
        );

        let error = super::loader::load_effective_config(
            &paths,
            super::loader::ConfigLoadOptions {
                include_environment: false,
                include_override: true,
            },
        )
        .expect_err("out of range worker id should fail");
        let message = error.to_string();

        assert!(
            message.contains("id.worker_id must be in 0..=31"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn config_loader_uses_dev_config_path_in_test_paths() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        fs::write(temp_dir.path().join("config.yaml"), "port: 1111\n")
            .expect("root config should be written");
        fs::write(temp_dir.path().join("config.local.yaml"), "port: 2222\n")
            .expect("root local config should be written");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(&paths.user_config_path, "port: 3333\n");

        let loaded = load_without_environment(&paths);

        assert_eq!(
            paths.user_config_path,
            temp_dir
                .path()
                .join(".cyder")
                .join("dev")
                .join("config")
                .join("config.yaml")
        );
        assert_eq!(loaded.config.port, 3333);
    }

    #[test]
    fn config_loader_treats_missing_override_as_empty() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());

        let loaded = load_without_environment(&paths);

        assert_eq!(loaded.config.base_path, "/ai");
        assert!(!paths.override_config_path.exists());
    }

    #[test]
    fn config_loader_uses_persistence_paths_for_default_local_state() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());

        let loaded = load_without_environment(&paths);

        assert_eq!(
            loaded.config.db_url,
            paths.persistence.sqlite_db_path.display().to_string()
        );
        assert_eq!(
            loaded.config.storage.local.root,
            paths.persistence.local_storage_root.display().to_string()
        );
    }

    #[test]
    fn explicit_postgres_and_s3_config_are_not_overridden_by_persistence_defaults() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        super::persistence::bootstrap_config_paths(&paths).expect("bootstrap should succeed");
        fs::write(
            &paths.user_config_path,
            r#"
db_url: postgres://cyder:secret@localhost/cyder
storage:
  driver: s3
  s3:
    bucket: gateway-bundles
"#,
        )
        .expect("user config should be written");

        let loaded = load_without_environment(&paths);

        assert_eq!(
            loaded.config.db_url,
            "postgres://cyder:secret@localhost/cyder"
        );
        assert_eq!(loaded.config.storage.driver, StorageDriver::S3);
        assert_eq!(
            loaded
                .config
                .storage
                .s3
                .as_ref()
                .expect("s3 config should be present")
                .bucket,
            "gateway-bundles"
        );
        assert!(!paths.persistence.sqlite_db_path.exists());
        assert!(!paths.persistence.local_storage_root.exists());
    }

    #[test]
    fn bootstrapped_config_load_is_stable_across_repeated_reads() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let data_dir = temp_dir.path().to_str().expect("temp path should be utf8");
        let paths = test_paths_from_persistence_env(Some(data_dir), None, temp_dir.path());
        super::persistence::bootstrap_config_paths(&paths).expect("bootstrap should succeed");

        let first = load_without_environment(&paths);
        let first_default = fs::read_to_string(&paths.default_config_path)
            .expect("default config should be written");
        let second = load_without_environment(&paths);
        let second_default = fs::read_to_string(&paths.default_config_path)
            .expect("default config should be readable");

        assert_eq!(first_default, second_default);
        assert_eq!(first.config.secret_key, second.config.secret_key);
        assert_eq!(first.config.jwt_secret, second.config.jwt_secret);
        assert_eq!(
            first.config.db_url,
            temp_dir
                .path()
                .join("db")
                .join("cyder.sqlite")
                .display()
                .to_string()
        );
    }

    #[test]
    fn bootstrapped_config_requires_explicit_user_config_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let data_dir = temp_dir.path().join("data");
        let missing_config_path = temp_dir.path().join("missing-config.yaml");
        let paths = test_paths_from_persistence_env(
            Some(data_dir.to_str().expect("data dir should be utf8")),
            Some(
                missing_config_path
                    .to_str()
                    .expect("config path should be utf8"),
            ),
            temp_dir.path(),
        );
        super::persistence::bootstrap_config_paths(&paths).expect("bootstrap should succeed");

        let error = super::loader::load_effective_config(
            &paths,
            super::loader::ConfigLoadOptions::default(),
        )
        .expect_err("missing explicit user config should fail");

        let message = error.to_string();
        assert!(
            message.contains("required user configuration file"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains(&missing_config_path.display().to_string()),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn config_loader_override_file_wins_over_environment() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(&paths.user_config_path, "log_level: debug\n");
        write_test_config(&paths.override_config_path, "log_level: error\n");
        let loaded =
            load_with_test_environment(&paths, &[(super::env::CYDER_LOG_LEVEL_ENV, "warn")], true);

        let loaded = loaded.expect("config should load");
        assert_eq!(loaded.config.log_level, "error");
        let source = loaded
            .source_report
            .resolve_field_source("log_level")
            .expect("log_level source should be resolved");
        assert_eq!(source.kind, ConfigLayerKind::OverrideFile);
        assert!(source.configured);
    }

    #[test]
    fn config_source_tracks_program_default_fields_as_unconfigured() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());

        let loaded = load_without_environment(&paths);

        let log_level = loaded
            .source_report
            .resolve_field_source("log_level")
            .expect("log_level source should be resolved");
        assert_eq!(log_level.kind, ConfigLayerKind::ProgramDefault);
        assert!(!log_level.configured);

        let nested = loaded
            .source_report
            .resolve_field_source("routing_resilience.max_candidates_per_request")
            .expect("nested source should be resolved");
        assert_eq!(nested.kind, ConfigLayerKind::ProgramDefault);
        assert!(!nested.configured);
    }

    #[test]
    fn config_source_ignores_default_file_values_that_match_program_defaults() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(&paths.default_config_path, "log_level: info\n");

        let loaded = load_without_environment(&paths);

        let source = loaded
            .source_report
            .resolve_field_source("log_level")
            .expect("log_level source should be resolved");
        assert_eq!(source.kind, ConfigLayerKind::ProgramDefault);
        assert!(!source.configured);
    }

    #[test]
    fn config_source_marks_default_file_true_overrides_as_configured() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(&paths.default_config_path, "log_level: debug\n");

        let loaded = load_without_environment(&paths);

        assert_eq!(loaded.config.log_level, "debug");
        let source = loaded
            .source_report
            .resolve_field_source("log_level")
            .expect("log_level source should be resolved");
        assert_eq!(source.kind, ConfigLayerKind::DefaultFile);
        assert!(source.configured);
    }

    #[test]
    fn config_source_tracks_user_file_over_default_config() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(
            &paths.user_config_path,
            r#"
log_level: debug
storage:
  driver: s3
  s3:
    bucket: gateway-bundles
"#,
        );

        let loaded = load_without_environment(&paths);

        assert_eq!(loaded.config.log_level, "debug");
        assert_eq!(
            loaded
                .config
                .storage
                .s3
                .as_ref()
                .expect("S3 config should be present")
                .bucket,
            "gateway-bundles"
        );
        let log_level = loaded
            .source_report
            .resolve_field_source("log_level")
            .expect("log_level source should be resolved");
        assert_eq!(log_level.kind, ConfigLayerKind::UserFile);
        assert_eq!(
            log_level.source_path.as_ref(),
            Some(&paths.user_config_path)
        );
        assert!(log_level.configured);

        let storage_bucket = loaded
            .source_report
            .resolve_field_source("storage.s3.bucket")
            .expect("storage bucket source should be resolved");
        assert_eq!(storage_bucket.kind, ConfigLayerKind::UserFile);
        assert_eq!(
            storage_bucket.source_path.as_ref(),
            Some(&paths.user_config_path)
        );
        assert!(storage_bucket.configured);

        let storage = loaded
            .source_report
            .resolve_field_source("storage")
            .expect("storage source should be resolved");
        assert_eq!(storage.kind, ConfigLayerKind::UserFile);
        assert!(storage.configured);
    }

    #[test]
    fn config_source_tracks_environment_over_user_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(&paths.user_config_path, "log_level: debug\n");
        let loaded =
            load_with_test_environment(&paths, &[(super::env::CYDER_LOG_LEVEL_ENV, "warn")], false);

        let loaded = loaded.expect("config should load");
        assert_eq!(loaded.config.log_level, "warn");
        let source = loaded
            .source_report
            .resolve_field_source("log_level")
            .expect("log_level source should be resolved");
        assert_eq!(source.kind, ConfigLayerKind::Environment);
        assert!(source.configured);
    }

    #[test]
    fn config_source_ignores_unallowlisted_environment_variables() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(
            &paths.user_config_path,
            r#"
log_level: debug
db_url: sqlite-from-yaml
secret_key: file-secret
"#,
        );
        let loaded = load_with_test_environment(
            &paths,
            &[
                ("LOG_LEVEL", "warn"),
                ("DB_URL", "postgres://env-should-not-win"),
                ("CYDER_DB_URL", "postgres://cyder-env-should-not-win"),
                ("SECRET_KEY", "env-secret-should-not-win"),
            ],
            false,
        );

        let loaded = loaded.expect("config should load");
        assert_eq!(loaded.config.log_level, "debug");
        assert_eq!(loaded.config.db_url, "sqlite-from-yaml");
        assert_eq!(loaded.config.secret_key, "file-secret");
        let source = loaded
            .source_report
            .resolve_field_source("log_level")
            .expect("log_level source should be resolved");
        assert_eq!(source.kind, ConfigLayerKind::UserFile);
    }

    #[test]
    fn config_source_ignores_unknown_cyder_environment_variables() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(
            &paths.user_config_path,
            r#"
storage:
  driver: local
  local:
    root: /yaml/storage
redis:
  url: redis://yaml/
  pool_size: 2
  key_prefix: "yaml:"
"#,
        );
        let loaded = load_with_test_environment(
            &paths,
            &[
                ("CYDER_STORAGE__LOCAL__ROOT", "/env/ignored-storage"),
                ("CYDER_RUNTIME_STATE__BACKEND", "redis"),
                ("STORAGE__LOCAL__ROOT", "/env/bare-storage"),
                ("REDIS_URL", "redis://env-ignored/"),
            ],
            false,
        );

        let loaded = loaded.expect("config should load");
        assert_eq!(loaded.config.storage.local.root, "/yaml/storage");
        assert_eq!(
            loaded
                .config
                .redis
                .as_ref()
                .expect("redis config should load")
                .url,
            "redis://yaml/"
        );
        assert_eq!(
            loaded.config.runtime_state.backend,
            RuntimeStateBackendType::Memory
        );

        let storage_root = loaded
            .source_report
            .resolve_field_source("storage.local.root")
            .expect("storage root source should be resolved");
        assert_eq!(storage_root.kind, ConfigLayerKind::UserFile);
        let redis_url = loaded
            .source_report
            .resolve_field_source("redis.url")
            .expect("redis url source should be resolved");
        assert_eq!(redis_url.kind, ConfigLayerKind::UserFile);
        let runtime_backend = loaded
            .source_report
            .resolve_field_source("runtime_state.backend")
            .expect("runtime state backend source should be resolved");
        assert_eq!(runtime_backend.kind, ConfigLayerKind::ProgramDefault);
    }

    #[test]
    fn allowlisted_environment_source_rejects_invalid_value() {
        let error = super::env::source_from_pairs(&[(super::env::CYDER_PORT_ENV, "not-a-port")])
            .expect_err("invalid environment value should fail");

        let message = error.to_string();
        assert!(
            message.contains(super::env::CYDER_PORT_ENV),
            "unexpected error: {message}"
        );
        assert!(
            !message.contains("not-a-port"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn config_source_marks_legacy_replay_capture_alias_as_derived() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(
            &paths.user_config_path,
            "replay_response_capture_max_bytes: 2048\n",
        );

        let loaded = load_without_environment(&paths);

        assert_eq!(loaded.config.diagnostics.response_capture_max_bytes, 2048);
        assert_eq!(loaded.config.replay_response_capture_max_bytes, 2048);

        let derived = loaded
            .source_report
            .resolve_field_source("diagnostics.response_capture_max_bytes")
            .expect("diagnostics response capture source should be resolved");
        assert_eq!(derived.kind, ConfigLayerKind::Derived);
        assert!(derived.configured);
        assert!(
            derived
                .warnings
                .iter()
                .any(|warning| warning.contains("replay_response_capture_max_bytes")),
            "derived source should explain legacy alias mapping: {:?}",
            derived.warnings
        );

        let legacy = loaded
            .source_report
            .resolve_field_source("replay_response_capture_max_bytes")
            .expect("legacy alias source should be resolved");
        assert_eq!(legacy.kind, ConfigLayerKind::UserFile);
        assert!(legacy.configured);
    }

    #[test]
    fn config_loader_rejects_non_whitelisted_override_paths() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = super::paths::ConfigPaths::for_test(temp_dir.path());
        write_test_config(
            &paths.override_config_path,
            r#"
db_url: postgres://example
secret_key: should-not-be-here
storage:
  driver: s3
"#,
        );

        let error = super::loader::load_effective_config(
            &paths,
            super::loader::ConfigLoadOptions {
                include_environment: false,
                include_override: true,
            },
        )
        .expect_err("non-whitelisted override paths should fail");
        let message = error.to_string();

        assert!(message.contains("db_url"), "unexpected error: {message}");
        assert!(
            message.contains("secret_key"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("storage.driver"),
            "unexpected error: {message}"
        );
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
    fn final_config_deserializes_metrics_alerts_notification_domains() {
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
metrics:
  enabled: false
  rollup_bucket_seconds: 120
  ingest_batch_size: 250
  reconciliation_batch_size: 300
  provider_runtime_default_window_seconds: 900
  request_log_query_fallback_enabled: false
  reconciliation_worker_interval_seconds: 15
  reconciliation_worker_recent_window_seconds: 1800
  reconciliation_worker_safety_lag_seconds: 12
alerts:
  enabled: false
  evaluation_interval_seconds: 30
  default_cooldown_seconds: 120
  rules:
    provider_degraded_min_requests: 9
    provider_degraded_error_rate: 0.4
    provider_degraded_latency_ms: 8000
    high_error_min_requests: 50
    high_error_rate: 0.5
    high_latency_min_samples: 20
    high_latency_ms: 12000
    transform_diagnostic_count_threshold: 7
    transform_diagnostic_lossy_major_threshold: 2
    cost_hotspot_amount_nanos: 123
    logging_pending_threshold: 44
    logging_in_flight_threshold: 8
    runtime_state_backend_degraded_enabled: false
notification:
  enabled: false
  worker_interval_seconds: 5
  webhook_timeout_seconds: 6
  max_delivery_attempts: 3
  retry_base_backoff_seconds: 4
  retry_max_backoff_seconds: 60
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");

        assert!(!config.metrics.enabled);
        assert_eq!(config.metrics.rollup_bucket_seconds, 120);
        assert_eq!(config.metrics.ingest_batch_size, 250);
        assert_eq!(config.metrics.reconciliation_batch_size, 300);
        assert_eq!(config.metrics.provider_runtime_default_window_seconds, 900);
        assert!(!config.metrics.request_log_query_fallback_enabled);
        assert_eq!(config.metrics.reconciliation_worker_interval_seconds, 15);
        assert_eq!(
            config.metrics.reconciliation_worker_recent_window_seconds,
            1_800
        );
        assert_eq!(config.metrics.reconciliation_worker_safety_lag_seconds, 12);
        assert!(!config.alerts.enabled);
        assert_eq!(config.alerts.evaluation_interval_seconds, 30);
        assert_eq!(config.alerts.default_cooldown_seconds, 120);
        assert_eq!(config.alerts.rules.provider_degraded_min_requests, 9);
        assert_eq!(config.alerts.rules.provider_degraded_error_rate, 0.4);
        assert_eq!(config.alerts.rules.provider_degraded_latency_ms, 8_000);
        assert_eq!(config.alerts.rules.high_error_min_requests, 50);
        assert_eq!(config.alerts.rules.high_error_rate, 0.5);
        assert_eq!(config.alerts.rules.high_latency_min_samples, 20);
        assert_eq!(config.alerts.rules.high_latency_ms, 12_000);
        assert_eq!(config.alerts.rules.transform_diagnostic_count_threshold, 7);
        assert_eq!(
            config
                .alerts
                .rules
                .transform_diagnostic_lossy_major_threshold,
            2
        );
        assert_eq!(config.alerts.rules.cost_hotspot_amount_nanos, Some(123));
        assert_eq!(config.alerts.rules.logging_pending_threshold, 44);
        assert_eq!(config.alerts.rules.logging_in_flight_threshold, 8);
        assert!(!config.alerts.rules.runtime_state_backend_degraded_enabled);
        assert!(!config.notification.enabled);
        assert_eq!(config.notification.worker_interval_seconds, 5);
        assert_eq!(config.notification.webhook_timeout_seconds, 6);
        assert_eq!(config.notification.max_delivery_attempts, 3);
        assert_eq!(config.notification.retry_base_backoff_seconds, 4);
        assert_eq!(config.notification.retry_max_backoff_seconds, 60);
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
  reasoning_continuation_ttl_seconds: 44
  reasoning_continuation_memory_capacity: 55
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
        assert_eq!(
            config.runtime_state.reasoning_continuation_ttl().as_secs(),
            44
        );
        assert_eq!(
            config.runtime_state.reasoning_continuation_memory_capacity,
            55
        );
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
