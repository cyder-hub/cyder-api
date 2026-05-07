use std::{ffi::OsString, fmt, str::FromStr};

use chrono_tz::Tz;
use config::{Map, Source, Value, ValueKind};
use cyder_tools::log::warn;

use crate::logging::THIRD_PARTY_DEBUG_ENV;

use super::persistence::{CYDER_CONFIG_PATH_ENV, CYDER_DATA_DIR_ENV};

pub const CYDER_HOST_ENV: &str = "CYDER_HOST";
pub const CYDER_PORT_ENV: &str = "CYDER_PORT";
pub const CYDER_BASE_PATH_ENV: &str = "CYDER_BASE_PATH";
pub const CYDER_LOG_LEVEL_ENV: &str = "CYDER_LOG_LEVEL";
pub const CYDER_TIMEZONE_ENV: &str = "CYDER_TIMEZONE";

pub const ALLOWED_CONFIG_ENV_VARS: &[&str] = &[
    CYDER_HOST_ENV,
    CYDER_PORT_ENV,
    CYDER_BASE_PATH_ENV,
    CYDER_LOG_LEVEL_ENV,
    CYDER_TIMEZONE_ENV,
];

pub const ENVIRONMENT_SOURCE_NAME: &str = "allowlisted environment variables: CYDER_HOST, CYDER_PORT, CYDER_BASE_PATH, CYDER_LOG_LEVEL, CYDER_TIMEZONE";

#[derive(Debug, Clone)]
pub struct EnvironmentConfigSource {
    values: Map<String, Value>,
}

impl EnvironmentConfigSource {
    pub fn current() -> Result<Self, EnvironmentConfigError> {
        Self::from_environment(SystemEnvironment::current())
    }

    fn from_environment(environment: SystemEnvironment) -> Result<Self, EnvironmentConfigError> {
        let mut values = Map::new();

        for ignored in ignored_environment_variables(&environment) {
            warn!(
                "ignoring environment variable {}; {}",
                ignored.name, ignored.reason
            );
        }

        insert_string_env(&mut values, &environment, CYDER_HOST_ENV, "host")?;
        insert_port_env(&mut values, &environment)?;
        insert_base_path_env(&mut values, &environment)?;
        insert_log_level_env(&mut values, &environment)?;
        insert_timezone_env(&mut values, &environment)?;

        Ok(Self { values })
    }
}

impl Source for EnvironmentConfigSource {
    fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
        Box::new(self.clone())
    }

    fn collect(&self) -> Result<Map<String, Value>, config::ConfigError> {
        Ok(self.values.clone())
    }
}

#[derive(Debug, Clone)]
pub enum EnvironmentConfigError {
    NonUnicode {
        name: &'static str,
    },
    InvalidValue {
        name: &'static str,
        expected: &'static str,
    },
}

impl fmt::Display for EnvironmentConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonUnicode { name } => {
                write!(f, "environment variable {name} contains non-Unicode data")
            }
            Self::InvalidValue { name, expected } => {
                write!(
                    f,
                    "environment variable {name} is invalid; expected {expected}"
                )
            }
        }
    }
}

impl std::error::Error for EnvironmentConfigError {}

#[derive(Debug, Clone, Default)]
struct SystemEnvironment {
    values: Vec<(String, OsString)>,
}

impl SystemEnvironment {
    fn current() -> Self {
        let values = std::env::vars_os()
            .filter_map(|(name, value)| name.into_string().ok().map(|name| (name, value)))
            .collect();
        Self { values }
    }

    #[cfg(test)]
    fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        Self {
            values: pairs
                .iter()
                .map(|(name, value)| ((*name).to_string(), OsString::from(value)))
                .collect(),
        }
    }

    fn value(&self, name: &'static str) -> Option<&OsString> {
        self.values
            .iter()
            .find_map(|(candidate, value)| (candidate == name).then_some(value))
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        self.values.iter().map(|(name, _)| name.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IgnoredEnvironmentVariable {
    name: String,
    reason: &'static str,
}

fn ignored_environment_variables(
    environment: &SystemEnvironment,
) -> Vec<IgnoredEnvironmentVariable> {
    environment
        .names()
        .filter_map(ignored_environment_variable)
        .collect()
}

fn ignored_environment_variable(name: &str) -> Option<IgnoredEnvironmentVariable> {
    if ALLOWED_CONFIG_ENV_VARS.contains(&name)
        || matches!(
            name,
            CYDER_DATA_DIR_ENV | CYDER_CONFIG_PATH_ENV | THIRD_PARTY_DEBUG_ENV
        )
        || name.starts_with("CYDER_TEST_")
    {
        return None;
    }

    if name.starts_with("CYDER_") {
        return Some(IgnoredEnvironmentVariable {
            name: name.to_string(),
            reason: "not in the configuration environment allowlist; configure this value in YAML",
        });
    }

    match name {
        "HOST" | "PORT" | "BASE_PATH" | "LOG_LEVEL" | "TIMEZONE" => {
            Some(IgnoredEnvironmentVariable {
                name: name.to_string(),
                reason: "unprefixed configuration variables are ignored; use the CYDER_* allowlist",
            })
        }
        "DB_URL"
        | "SECRET_KEY"
        | "PASSWORD_SALT"
        | "JWT_SECRET"
        | "API_KEY_JWT_SECRET"
        | "REDIS_URL"
        | "STORAGE__LOCAL__ROOT" => Some(IgnoredEnvironmentVariable {
            name: name.to_string(),
            reason: "this configuration field must come from YAML and cannot be overridden by environment",
        }),
        _ => None,
    }
}

fn insert_string_env(
    values: &mut Map<String, Value>,
    environment: &SystemEnvironment,
    name: &'static str,
    path: &'static str,
) -> Result<(), EnvironmentConfigError> {
    let Some(raw) = read_non_empty_string(environment, name)? else {
        return Ok(());
    };
    values.insert(path.to_string(), env_value(raw));
    Ok(())
}

fn insert_port_env(
    values: &mut Map<String, Value>,
    environment: &SystemEnvironment,
) -> Result<(), EnvironmentConfigError> {
    let Some(raw) = read_non_empty_string(environment, CYDER_PORT_ENV)? else {
        return Ok(());
    };
    let port = raw.parse::<u16>().ok().filter(|value| *value > 0).ok_or(
        EnvironmentConfigError::InvalidValue {
            name: CYDER_PORT_ENV,
            expected: "an integer in 1..=65535",
        },
    )?;
    values.insert("port".to_string(), env_value(port));
    Ok(())
}

fn insert_base_path_env(
    values: &mut Map<String, Value>,
    environment: &SystemEnvironment,
) -> Result<(), EnvironmentConfigError> {
    let Some(raw) = read_non_empty_string(environment, CYDER_BASE_PATH_ENV)? else {
        return Ok(());
    };
    if !raw.starts_with('/') {
        return Err(EnvironmentConfigError::InvalidValue {
            name: CYDER_BASE_PATH_ENV,
            expected: "a path starting with /",
        });
    }
    values.insert("base_path".to_string(), env_value(raw));
    Ok(())
}

fn insert_log_level_env(
    values: &mut Map<String, Value>,
    environment: &SystemEnvironment,
) -> Result<(), EnvironmentConfigError> {
    let Some(raw) = read_non_empty_string(environment, CYDER_LOG_LEVEL_ENV)? else {
        return Ok(());
    };
    let level = raw.to_ascii_lowercase();
    if !matches!(
        level.as_str(),
        "trace" | "debug" | "info" | "warn" | "error"
    ) {
        return Err(EnvironmentConfigError::InvalidValue {
            name: CYDER_LOG_LEVEL_ENV,
            expected: "trace, debug, info, warn, or error",
        });
    }
    values.insert("log_level".to_string(), env_value(level));
    Ok(())
}

fn insert_timezone_env(
    values: &mut Map<String, Value>,
    environment: &SystemEnvironment,
) -> Result<(), EnvironmentConfigError> {
    let Some(raw) = read_non_empty_string(environment, CYDER_TIMEZONE_ENV)? else {
        return Ok(());
    };
    Tz::from_str(&raw).map_err(|_| EnvironmentConfigError::InvalidValue {
        name: CYDER_TIMEZONE_ENV,
        expected: "a valid IANA timezone name",
    })?;
    values.insert("timezone".to_string(), env_value(raw));
    Ok(())
}

fn read_non_empty_string(
    environment: &SystemEnvironment,
    name: &'static str,
) -> Result<Option<String>, EnvironmentConfigError> {
    let Some(value) = environment.value(name) else {
        return Ok(None);
    };
    if value.is_empty() {
        warn!("ignoring empty environment variable {name}; treating it as unset");
        return Ok(None);
    }

    let value = value
        .clone()
        .into_string()
        .map_err(|_| EnvironmentConfigError::NonUnicode { name })?;
    if value.trim().is_empty() {
        warn!("ignoring empty environment variable {name}; treating it as unset");
        return Ok(None);
    }

    Ok(Some(value))
}

fn env_value(value: impl Into<ValueKind>) -> Value {
    let origin = ENVIRONMENT_SOURCE_NAME.to_string();
    Value::new(Some(&origin), value.into())
}

#[cfg(test)]
pub(crate) fn source_from_pairs(
    pairs: &[(&str, &str)],
) -> Result<EnvironmentConfigSource, EnvironmentConfigError> {
    EnvironmentConfigSource::from_environment(SystemEnvironment::from_pairs(pairs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlisted_environment_source_maps_only_cyder_config_fields() {
        let source = source_from_pairs(&[
            (CYDER_HOST_ENV, "127.0.0.1"),
            (CYDER_PORT_ENV, "9000"),
            (CYDER_BASE_PATH_ENV, "/gateway"),
            (CYDER_LOG_LEVEL_ENV, "DEBUG"),
            (CYDER_TIMEZONE_ENV, "Asia/Shanghai"),
            ("LOG_LEVEL", "error"),
            ("CYDER_DB_URL", "postgres://ignored"),
        ])
        .expect("environment should parse");

        let values = source.collect().expect("source should collect");
        assert!(values.contains_key("host"));
        assert!(values.contains_key("port"));
        assert!(values.contains_key("base_path"));
        assert!(values.contains_key("log_level"));
        assert!(values.contains_key("timezone"));
        assert!(!values.contains_key("db_url"));
        assert!(!values.contains_key("cyder_db_url"));
    }

    #[test]
    fn invalid_port_reports_variable_name_without_value() {
        let error = source_from_pairs(&[(CYDER_PORT_ENV, "not-a-port")])
            .expect_err("invalid port should fail");
        let message = error.to_string();

        assert!(
            message.contains(CYDER_PORT_ENV),
            "unexpected error: {message}"
        );
        assert!(
            !message.contains("not-a-port"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn invalid_timezone_reports_variable_name_without_value() {
        let error = source_from_pairs(&[(CYDER_TIMEZONE_ENV, "Not/AZone")])
            .expect_err("invalid timezone should fail");
        let message = error.to_string();

        assert!(
            message.contains(CYDER_TIMEZONE_ENV),
            "unexpected error: {message}"
        );
        assert!(
            !message.contains("Not/AZone"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn ignored_environment_variable_detection_covers_legacy_and_unknown_cyder_names() {
        let environment = SystemEnvironment::from_pairs(&[
            ("LOG_LEVEL", "debug"),
            ("DB_URL", "postgres://ignored"),
            ("CYDER_DB_URL", "postgres://ignored"),
            (CYDER_LOG_LEVEL_ENV, "info"),
            (CYDER_DATA_DIR_ENV, "/data/cyder"),
        ]);
        let ignored = ignored_environment_variables(&environment);
        let names: Vec<_> = ignored.iter().map(|item| item.name.as_str()).collect();

        assert_eq!(names, vec!["LOG_LEVEL", "DB_URL", "CYDER_DB_URL"]);
    }
}
