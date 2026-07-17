use std::{fmt, fs, io, path::PathBuf};

use config::{Config, File, FileFormat};

use super::{
    FinalConfig, env, finalize_loaded_config, paths::ConfigPaths,
    programmatic_default_config_for_paths,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigLoadOptions {
    pub include_environment: bool,
}

impl Default for ConfigLoadOptions {
    fn default() -> Self {
        Self {
            include_environment: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedDefaultConfig {
    pub merged_yaml: String,
}

#[derive(Debug)]
pub enum ConfigLoadError {
    BuildDefault(String),
    DeserializeDefault(String),
    SerializeDefault(String),
    ReadUser {
        path: PathBuf,
        source: std::io::Error,
    },
    BuildEffective(String),
    DeserializeEffective(String),
    Environment(super::env::EnvironmentConfigError),
}

impl fmt::Display for ConfigLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigLoadError::BuildDefault(err) => {
                write!(f, "failed to build default configuration: {err}")
            }
            ConfigLoadError::DeserializeDefault(err) => {
                write!(f, "failed to deserialize default configuration: {err}")
            }
            ConfigLoadError::SerializeDefault(err) => {
                write!(f, "failed to serialize default configuration: {err}")
            }
            ConfigLoadError::ReadUser { path, source } => write!(
                f,
                "failed to read required user configuration file '{}': {source}",
                path.display()
            ),
            ConfigLoadError::BuildEffective(err) => {
                write!(f, "failed to build effective configuration: {err}")
            }
            ConfigLoadError::DeserializeEffective(err) => {
                write!(f, "failed to deserialize effective configuration: {err}")
            }
            ConfigLoadError::Environment(err) => {
                write!(f, "failed to read environment configuration: {err}")
            }
        }
    }
}

impl std::error::Error for ConfigLoadError {}

pub fn load_default_config(paths: &ConfigPaths) -> Result<LoadedDefaultConfig, ConfigLoadError> {
    let program_default_config = programmatic_default_config_for_paths(paths);
    let default_yaml_str = serde_yaml::to_string(&program_default_config)
        .map_err(|err| ConfigLoadError::SerializeDefault(err.to_string()))?;

    let default_builder = Config::builder()
        .add_source(File::from_str(&default_yaml_str, FileFormat::Yaml))
        .add_source(File::from(paths.default_config_path.as_path()).required(false));

    let default_config: FinalConfig = default_builder
        .build()
        .map_err(|err| ConfigLoadError::BuildDefault(err.to_string()))?
        .try_deserialize()
        .map_err(|err| ConfigLoadError::DeserializeDefault(err.to_string()))?;

    let merged_yaml = serde_yaml::to_string(&default_config)
        .map_err(|err| ConfigLoadError::SerializeDefault(err.to_string()))?;

    Ok(LoadedDefaultConfig { merged_yaml })
}

pub fn load_effective_config(
    paths: &ConfigPaths,
    options: ConfigLoadOptions,
) -> Result<FinalConfig, ConfigLoadError> {
    load_effective_config_inner(paths, options, None)
}

#[cfg(test)]
pub(crate) fn load_effective_config_with_environment_source(
    paths: &ConfigPaths,
    options: ConfigLoadOptions,
    environment_source: super::env::EnvironmentConfigSource,
) -> Result<FinalConfig, ConfigLoadError> {
    load_effective_config_inner(paths, options, Some(environment_source))
}

fn load_effective_config_inner(
    paths: &ConfigPaths,
    options: ConfigLoadOptions,
    runtime_environment_source: Option<super::env::EnvironmentConfigSource>,
) -> Result<FinalConfig, ConfigLoadError> {
    let default = load_default_config(paths)?;

    let mut builder =
        Config::builder().add_source(File::from_str(&default.merged_yaml, FileFormat::Yaml));

    if paths.user_config_path_required {
        validate_required_user_config_file(paths)?;
        builder = builder.add_source(File::from(paths.user_config_path.as_path()).required(true));
    } else if paths.user_config_path.exists() {
        builder = builder.add_source(File::from(paths.user_config_path.as_path()).required(false));
    }

    let environment_source = if options.include_environment {
        Some(match runtime_environment_source {
            Some(environment_source) => environment_source,
            None => {
                env::EnvironmentConfigSource::current().map_err(ConfigLoadError::Environment)?
            }
        })
    } else {
        None
    };
    if let Some(environment_source) = environment_source {
        builder = builder.add_source(environment_source);
    }

    let final_config: FinalConfig = builder
        .build()
        .map_err(|err| ConfigLoadError::BuildEffective(err.to_string()))?
        .try_deserialize()
        .map_err(|err| ConfigLoadError::DeserializeEffective(err.to_string()))?;
    let final_config = finalize_loaded_config(final_config);

    Ok(final_config)
}

fn validate_required_user_config_file(paths: &ConfigPaths) -> Result<(), ConfigLoadError> {
    let metadata =
        fs::metadata(&paths.user_config_path).map_err(|source| ConfigLoadError::ReadUser {
            path: paths.user_config_path.clone(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(ConfigLoadError::ReadUser {
            path: paths.user_config_path.clone(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "path is not a file"),
        });
    }
    fs::File::open(&paths.user_config_path)
        .map(|_| ())
        .map_err(|source| ConfigLoadError::ReadUser {
            path: paths.user_config_path.clone(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_user_yaml(yaml: &str) -> Result<FinalConfig, ConfigLoadError> {
        let temp_dir = tempfile::tempdir().expect("config test directory should be created");
        let paths = ConfigPaths::new(
            temp_dir.path().join("config.default.yaml"),
            temp_dir.path().join("config.yaml"),
        );
        fs::write(&paths.user_config_path, yaml).expect("user config should be written");

        load_effective_config(
            &paths,
            ConfigLoadOptions {
                include_environment: false,
            },
        )
    }

    #[test]
    fn effective_config_ignores_unknown_top_level_fields() {
        let config = load_user_yaml("port: 9123\nrouting_resilience: {}\n")
            .expect("unknown top-level fields should be ignored");

        assert_eq!(config.port, 9123);
    }

    #[test]
    fn effective_config_ignores_unknown_nested_fields() {
        let config = load_user_yaml(
            "provider_governance:\n  open_cooldown_seconds: 17\n  unknown_policy: true\n",
        )
        .expect("unknown nested fields should be ignored");

        assert_eq!(config.provider_governance.open_cooldown_seconds, 17);
    }

    #[test]
    fn effective_config_rejects_invalid_known_field_values() {
        let error = load_user_yaml("deployment:\n  mode: invalid_mode\n")
            .expect_err("invalid known enum value should be rejected");

        assert!(
            error.to_string().contains("invalid_mode"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn effective_config_rejects_invalid_known_field_types() {
        let error = load_user_yaml("port: not-a-number\n")
            .expect_err("invalid known field type should be rejected");

        assert!(
            error.to_string().contains("port"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn managed_default_snapshot_ignores_unknown_fields() {
        let temp_dir = tempfile::tempdir().expect("config test directory should be created");
        let paths = ConfigPaths::new(
            temp_dir.path().join("config.default.yaml"),
            temp_dir.path().join("config.yaml"),
        );
        fs::write(
            &paths.default_config_path,
            "port: 9123\nreplay_response_capture_max_bytes: 4194304\n",
        )
        .expect("managed default should be written");

        let loaded = load_default_config(&paths).expect("managed default should be loaded");
        let config: FinalConfig =
            serde_yaml::from_str(&loaded.merged_yaml).expect("merged default should deserialize");

        assert_eq!(config.port, 9123);
        assert!(
            !loaded
                .merged_yaml
                .contains("replay_response_capture_max_bytes")
        );
    }
}
