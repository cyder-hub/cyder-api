use std::{fs, path::Path};

use once_cell::sync::Lazy;
use rand::{distr::Alphanumeric, rng, Rng};
use serde::{Deserialize, Serialize};

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
    pub redis_url: Option<String>,
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
    pub redis_url: Option<String>,
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
        redis_url: None,
    };

    // If a default config file exists, load it as partial and merge it over the programmatic defaults.
    if default_config_path.exists() {
        if let Ok(config_str) = fs::read_to_string(default_config_path) {
            let file_defaults: PartialConfig = serde_yaml::from_str(&config_str)
                .unwrap_or_else(|e| panic!("Failed to parse default configuration file at {:?}: {}", default_config_path, e));

            if let Some(host) = file_defaults.host { effective_default_config.host = host; }
            if let Some(port) = file_defaults.port { effective_default_config.port = port; }
            if let Some(base_path) = file_defaults.base_path { effective_default_config.base_path = base_path; }
            if let Some(secret_key) = file_defaults.secret_key { effective_default_config.secret_key = secret_key; }
            if let Some(password_salt) = file_defaults.password_salt { effective_default_config.password_salt = password_salt; }
            if let Some(jwt_secret) = file_defaults.jwt_secret { effective_default_config.jwt_secret = jwt_secret; }
            if let Some(api_key_jwt_secret) = file_defaults.api_key_jwt_secret { effective_default_config.api_key_jwt_secret = api_key_jwt_secret; }
            if let Some(db_url) = file_defaults.db_url { effective_default_config.db_url = db_url; }
            if let Some(proxy) = file_defaults.proxy { effective_default_config.proxy = Some(proxy); }
            if let Some(log_level) = file_defaults.log_level { effective_default_config.log_level = log_level; }
            if let Some(timezone) = file_defaults.timezone { effective_default_config.timezone = Some(timezone); }
            if let Some(redis_url) = file_defaults.redis_url { effective_default_config.redis_url = Some(redis_url); }
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
            if let Some(host) = user_config.host { final_config.host = host; }
            if let Some(port) = user_config.port { final_config.port = port; }
            if let Some(base_path) = user_config.base_path { final_config.base_path = base_path; }
            if let Some(secret_key) = user_config.secret_key { final_config.secret_key = secret_key; }
            if let Some(password_salt) = user_config.password_salt { final_config.password_salt = password_salt; }
            if let Some(jwt_secret) = user_config.jwt_secret { final_config.jwt_secret = jwt_secret; }
            if let Some(api_key_jwt_secret) = user_config.api_key_jwt_secret { final_config.api_key_jwt_secret = api_key_jwt_secret; }
            if let Some(db_url) = user_config.db_url { final_config.db_url = db_url; }
            if let Some(proxy) = user_config.proxy { final_config.proxy = Some(proxy); }
            if let Some(log_level) = user_config.log_level { final_config.log_level = log_level; }
            if let Some(timezone) = user_config.timezone { final_config.timezone = Some(timezone); }
            if let Some(redis_url) = user_config.redis_url { final_config.redis_url = Some(redis_url); }
        }
    }

    println!("111{:?}", final_config);

    final_config
});
