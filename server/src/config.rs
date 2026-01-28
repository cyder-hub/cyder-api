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

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PartialRedisConfig {
    pub url: Option<String>,
    pub pool_size: Option<usize>,
    pub key_prefix: Option<String>,
}

impl PartialRedisConfig {
    fn merge_into(self, final_config: &mut RedisConfig) {
        if let Some(url) = self.url {
            final_config.url = url;
        }
        if let Some(pool_size) = self.pool_size {
            final_config.pool_size = pool_size;
        }
        if let Some(key_prefix) = self.key_prefix {
            final_config.key_prefix = key_prefix;
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

// --- PARTIAL CACHE CONFIG for merging ---


#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PartialCacheRedisConfig {
    pub key_prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PartialCacheConfig {
    pub backend: Option<CacheBackendType>,
    pub ttl: Option<u64>,
    pub negative_ttl: Option<u64>,
    pub redis: Option<PartialCacheRedisConfig>,
}

impl PartialCacheConfig {
    fn merge_into(self, final_config: &mut CacheConfig) {
        if let Some(backend) = self.backend {
            final_config.backend = backend;
        }
        if let Some(ttl) = self.ttl {
            final_config.ttl = ttl;
        }
        if let Some(negative_ttl) = self.negative_ttl {
            final_config.negative_ttl = negative_ttl;
        }
        if let Some(redis) = self.redis {
            if let Some(key_prefix) = redis.key_prefix {
                final_config.redis.key_prefix = key_prefix;
            }
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


// Used for deserializing user-provided config files where all fields are optional.
#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PartialConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub base_path: Option<String>,
    pub secret_key: Option<String>,
    pub password_salt: Option<String>,
    pub jwt_secret: Option<String>,
    pub api_key_jwt_secret: Option<String>,
    pub db_url: Option<String>,
    pub proxy: Option<String>,
    pub log_level: Option<String>,
    pub timezone: Option<String>,
    pub redis: Option<PartialRedisConfig>,
    pub cache: Option<PartialCacheConfig>,
}

impl PartialConfig {
    /// Merges the fields of this partial config into a final config, overwriting existing values.
    fn merge_into(self, final_config: &mut FinalConfig) {
        if let Some(host) = self.host { final_config.host = host; }
        if let Some(port) = self.port { final_config.port = port; }
        if let Some(base_path) = self.base_path { final_config.base_path = base_path; }
        if let Some(secret_key) = self.secret_key { final_config.secret_key = secret_key; }
        if let Some(password_salt) = self.password_salt { final_config.password_salt = password_salt; }
        if let Some(jwt_secret) = self.jwt_secret { final_config.jwt_secret = jwt_secret; }
        if let Some(api_key_jwt_secret) = self.api_key_jwt_secret { final_config.api_key_jwt_secret = api_key_jwt_secret; }
        if let Some(db_url) = self.db_url { final_config.db_url = db_url; }
        if let Some(proxy) = self.proxy { final_config.proxy = Some(proxy); }
        if let Some(log_level) = self.log_level { final_config.log_level = log_level; }
        if let Some(timezone) = self.timezone { final_config.timezone = Some(timezone); }
        if let Some(redis) = self.redis {
            redis.merge_into(final_config.redis.get_or_insert_with(Default::default));
        }
        if let Some(cache) = self.cache {
            cache.merge_into(&mut final_config.cache)
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
    pub redis: Option<RedisConfig>,
    pub cache: CacheConfig,
}

fn generate_random_string(len: usize) -> String {
    rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn get_env_var<T: std::str::FromStr>(key: &str) -> Option<T> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

fn get_config_from_env() -> PartialConfig {
    PartialConfig {
        host: get_env_var("HOST"),
        port: get_env_var("PORT"),
        base_path: get_env_var("BASE_PATH"),
        secret_key: get_env_var("SECRET_KEY"),
        password_salt: get_env_var("PASSWORD_SALT"),
        jwt_secret: get_env_var("JWT_SECRET"),
        api_key_jwt_secret: get_env_var("API_KEY_JWT_SECRET"),
        db_url: get_env_var("DB_URL"),
        proxy: get_env_var("PROXY"),
        log_level: get_env_var("LOG_LEVEL"),
        timezone: get_env_var("TIMEZONE"),
        redis: None,
        cache: None,
    }
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
    let mut effective_default_config = FinalConfig {
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
        redis: None,
        cache: CacheConfig::default(),
    };

    // If a default config file exists, load it as partial and merge it over the programmatic defaults.
    if default_config_path.exists() {
        if let Ok(config_str) = fs::read_to_string(default_config_path) {
            let file_defaults: PartialConfig = serde_yaml::from_str(&config_str)
                .unwrap_or_else(|e| panic!("Failed to parse default configuration file at {:?}: {}", default_config_path, e));

            file_defaults.merge_into(&mut effective_default_config);
        }
    }

    // Write the (potentially updated) defaults back to the file.
    // This ensures new fields are added to config.default.yaml.
    let yaml_str = serde_yaml::to_string(&effective_default_config).unwrap();
    fs::write(default_config_path, yaml_str)
        .unwrap_or_else(|err| panic!("Failed to write default configuration file: {}", err));

    // Start with the effective defaults.
    let mut final_config = effective_default_config;

    // Load the user's config if it exists. It's optional and overrides the defaults.
    if user_config_path.exists() {
        if let Ok(config_str) = fs::read_to_string(user_config_path) {
            let user_config: PartialConfig = serde_yaml::from_str(&config_str)
                .unwrap_or_else(|e| panic!("Failed to parse user configuration file at {:?}: {}", user_config_path, e));

            // Merge user overrides into the final config
            user_config.merge_into(&mut final_config);
        }
    }

    // Load config from environment variables, which have the highest priority.
    get_config_from_env().merge_into(&mut final_config);

    if final_config.redis.is_none() && final_config.cache.backend == CacheBackendType::Redis {
        final_config.cache.backend = CacheBackendType::Memory;
    }

    final_config
});
