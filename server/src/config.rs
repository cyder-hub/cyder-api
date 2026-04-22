use rand::{Rng, distr::Alphanumeric, rng};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, sync::LazyLock, time::Duration};

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

/// Overall cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub backend: CacheBackendType,
    #[serde(default = "default_ttl_seconds")]
    pub ttl: u64,
    #[serde(default = "default_negative_ttl_seconds")]
    pub negative_ttl: u64,
    #[serde(default)]
    pub redis: CacheRedisConfig,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            backend: CacheBackendType::default(),
            ttl: default_ttl_seconds(),
            negative_ttl: default_negative_ttl_seconds(),
            redis: CacheRedisConfig::default(),
        }
    }
}

impl CacheConfig {
    pub fn ttl(&self) -> Duration {
        Duration::from_secs(self.ttl)
    }

    pub fn negative_ttl(&self) -> Duration {
        Duration::from_secs(self.negative_ttl)
    }
}

// --- START PROXY REQUEST CONFIG ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRequestConfig {
    #[serde(default = "default_proxy_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
    #[serde(default)]
    pub first_byte_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub total_timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing)]
    pub timeout_seconds: Option<u64>,
}

impl Default for ProxyRequestConfig {
    fn default() -> Self {
        Self {
            connect_timeout_seconds: default_proxy_connect_timeout_seconds(),
            first_byte_timeout_seconds: default_proxy_first_byte_timeout_seconds(),
            total_timeout_seconds: None,
            timeout_seconds: None,
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
        self.total_timeout_seconds
            .or(self.timeout_seconds)
            .map(Duration::from_secs)
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

fn default_redis_url() -> String {
    "redis://127.0.0.1:6379/".to_string()
}

fn default_local_storage_root() -> String {
    "storage/storage".to_string()
}

fn default_replay_response_capture_max_bytes() -> usize {
    4 * 1024 * 1024
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
    pub db_pool_size: u32,
    pub redis: Option<RedisConfig>,
    #[serde(default)]
    pub proxy_request: ProxyRequestConfig,
    #[serde(default)]
    pub provider_governance: ProviderGovernanceConfig,
    #[serde(default)]
    pub routing_resilience: RoutingResilienceConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub storage: StorageConfig,
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
        db_pool_size: 5,
        redis: None,
        proxy_request: ProxyRequestConfig::default(),
        provider_governance: ProviderGovernanceConfig::default(),
        routing_resilience: RoutingResilienceConfig::default(),
        cache: CacheConfig::default(),
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

    let mut final_config: FinalConfig = builder
        .build()
        .expect("Failed to build user and environment merged config")
        .try_deserialize()
        .expect("Failed to deserialize final configuration from merged tree");

    if final_config.redis.is_none() && final_config.cache.backend == CacheBackendType::Redis {
        final_config.cache.backend = CacheBackendType::Memory;
    }

    if final_config.storage.driver == StorageDriver::S3 && final_config.storage.s3.is_none() {
        final_config.storage.driver = StorageDriver::Local;
    }

    final_config
});

#[cfg(test)]
mod tests {
    use super::{
        FinalConfig, ProviderGovernanceConfig, ProxyRequestConfig, RoutingResilienceConfig,
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
    fn final_config_deserializes_legacy_timeout_field_as_total_timeout() {
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
  timeout_seconds: 123
"#;

        let config: FinalConfig = serde_yaml::from_str(yaml).expect("config should deserialize");
        assert_eq!(config.proxy_request.timeout_seconds, Some(123));
        assert_eq!(config.proxy_request.total_timeout_seconds, None);
        assert_eq!(
            config
                .proxy_request
                .total_timeout()
                .map(|value| value.as_secs()),
            Some(123)
        );
    }
}
