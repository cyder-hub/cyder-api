use std::fs;

use once_cell::sync::Lazy;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub base_path: String,
    pub secret_key: String,
    pub password_salt: String,
    pub jwt_secret: String,
    pub db_url: String,
    pub proxy: ProxyConfig,
}

#[derive(Debug, Deserialize)]
pub struct ProxyConfig {
    pub url: String,
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    let yaml_str = fs::read_to_string("config.local.yaml")
        .unwrap_or(fs::read_to_string("config.yaml").unwrap());
    let config: Config = serde_yaml::from_str(&yaml_str).unwrap();
    config
});
