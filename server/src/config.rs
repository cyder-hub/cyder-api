use std::{fs, path::Path, time::Duration};

use once_cell::sync::Lazy;
use rand::{distr::Alphanumeric, rng, Rng};
use serde::{Deserialize, Serialize};

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
    pub db_pool_size: u32,
    pub redis: Option<RedisConfig>,
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

pub static CONFIG: Lazy<FinalConfig> = Lazy::new(|| {
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
        db_pool_size: 5,
        redis: None,
        cache: CacheConfig::default(),
        storage: StorageConfig::default(),
    };

    let default_yaml_str = serde_yaml::to_string(&effective_default_config).unwrap();

    // First stage: parse default config
    let mut default_builder = config::Config::builder()
        .add_source(config::File::from_str(&default_yaml_str, config::FileFormat::Yaml));

    if default_config_path.exists() {
        default_builder = default_builder.add_source(config::File::from(default_config_path).required(false));
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
    let mut builder = config::Config::builder()
        .add_source(config::File::from_str(&merged_default_yaml, config::FileFormat::Yaml));

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