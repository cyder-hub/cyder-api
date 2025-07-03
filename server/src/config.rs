use std::{fs, path::Path};

use once_cell::sync::Lazy;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub base_path: String,
    pub secret_key: String,
    pub jwt_secret: String,
    pub api_key_jwt_secret: Option<String>,
    pub db_url: String,
    pub proxy: ProxyConfig,
    pub log_level: String,
    pub timezone: Option<String>, // e.g., "America/New_York", "Asia/Shanghai", "Etc/UTC"
}

#[derive(Debug, Deserialize)]
pub struct ProxyConfig {
    pub url: String,
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    // Determine paths based on build profile
    let (local_path, default_path) = if cfg!(debug_assertions) {
        // Debug mode (e.g., cargo run): Assume running from server/, look in workspace root ../
        (Path::new("../config.local.yaml"), Path::new("../config.yaml"))
    } else {
        // Release mode (e.g., cargo build --release): Assume running from executable's location
        (Path::new("config.local.yaml"), Path::new("config.yaml"))
    };

    // Try reading local config first, then default config
    let yaml_str = fs::read_to_string(local_path)
        .or_else(|_| fs::read_to_string(default_path))
        .unwrap_or_else(|err| {
            panic!(
                "Failed to read configuration file ({:?} or {:?}): {}",
                local_path, default_path, err
            )
        });

    // Parse the YAML string
    let config: Config = serde_yaml::from_str(&yaml_str)
        .unwrap_or_else(|err| panic!("Failed to parse configuration YAML: {}", err));

    config
});
