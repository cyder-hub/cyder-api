use std::{fmt, fs, io, path::PathBuf};

use config::{Config, File, FileFormat};
use serde_json::Value;

use super::{
    FinalConfig, finalize_loaded_config, override_policy, paths::ConfigPaths,
    programmatic_default_config_for_paths, source,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigLoadOptions {
    pub include_environment: bool,
    pub include_override: bool,
}

impl Default for ConfigLoadOptions {
    fn default() -> Self {
        Self {
            include_environment: true,
            include_override: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedDefaultConfig {
    pub program_default_config: FinalConfig,
    pub config: FinalConfig,
    pub merged_yaml: String,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: FinalConfig,
    pub source_report: source::ConfigSourceReport,
    pub program_default_config: FinalConfig,
    pub default_config: FinalConfig,
    pub merged_default_yaml: String,
    pub paths: ConfigPaths,
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
    ReadOverride {
        path: PathBuf,
        source: std::io::Error,
    },
    ParseOverride {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    InvalidOverridePaths {
        path: PathBuf,
        paths: Vec<String>,
    },
    Environment(super::env::EnvironmentConfigError),
    TraceSources(source::ConfigSourceError),
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
            ConfigLoadError::ReadOverride { path, source } => write!(
                f,
                "failed to read override configuration file '{}': {source}",
                path.display()
            ),
            ConfigLoadError::ParseOverride { path, source } => write!(
                f,
                "failed to parse override configuration file '{}': {source}",
                path.display()
            ),
            ConfigLoadError::InvalidOverridePaths { path, paths } => write!(
                f,
                "override configuration file '{}' contains unsupported paths: {}",
                path.display(),
                paths.join(", ")
            ),
            ConfigLoadError::Environment(err) => {
                write!(f, "failed to read environment configuration: {err}")
            }
            ConfigLoadError::TraceSources(err) => {
                write!(f, "failed to trace configuration sources: {err}")
            }
        }
    }
}

impl std::error::Error for ConfigLoadError {}

pub fn load_default_config(paths: &ConfigPaths) -> Result<LoadedDefaultConfig, ConfigLoadError> {
    let program_default_config = programmatic_default_config_for_paths(paths);
    let default_yaml_str = serde_yaml::to_string(&program_default_config)
        .map_err(|err| ConfigLoadError::SerializeDefault(err.to_string()))?;

    let mut default_builder =
        Config::builder().add_source(File::from_str(&default_yaml_str, FileFormat::Yaml));

    if paths.default_config_path.exists() {
        default_builder = default_builder
            .add_source(File::from(paths.default_config_path.as_path()).required(false));
    }

    let default_config: FinalConfig = default_builder
        .build()
        .map_err(|err| ConfigLoadError::BuildDefault(err.to_string()))?
        .try_deserialize()
        .map_err(|err| ConfigLoadError::DeserializeDefault(err.to_string()))?;

    let merged_yaml = serde_yaml::to_string(&default_config)
        .map_err(|err| ConfigLoadError::SerializeDefault(err.to_string()))?;

    Ok(LoadedDefaultConfig {
        program_default_config,
        config: default_config,
        merged_yaml,
    })
}

pub fn load_effective_config(
    paths: &ConfigPaths,
    options: ConfigLoadOptions,
) -> Result<LoadedConfig, ConfigLoadError> {
    load_effective_config_inner(paths, options, None, None)
}

pub fn load_effective_config_with_override_document(
    paths: &ConfigPaths,
    options: ConfigLoadOptions,
    override_document: &Value,
) -> Result<LoadedConfig, ConfigLoadError> {
    load_effective_config_inner(paths, options, Some(override_document), None)
}

#[cfg(test)]
pub(crate) fn load_effective_config_with_environment_source(
    paths: &ConfigPaths,
    options: ConfigLoadOptions,
    environment_source: super::env::EnvironmentConfigSource,
) -> Result<LoadedConfig, ConfigLoadError> {
    load_effective_config_inner(paths, options, None, Some(environment_source))
}

fn load_effective_config_inner(
    paths: &ConfigPaths,
    options: ConfigLoadOptions,
    override_document: Option<&Value>,
    runtime_environment_source: Option<super::env::EnvironmentConfigSource>,
) -> Result<LoadedConfig, ConfigLoadError> {
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
            None => source::environment_source().map_err(ConfigLoadError::Environment)?,
        })
    } else {
        None
    };
    if let Some(environment_source) = environment_source.clone() {
        builder = builder.add_source(environment_source);
    }

    let override_report_value = if options.include_override {
        match override_document {
            Some(document) => {
                let yaml_value = serde_yaml::to_value(document)
                    .map_err(|err| ConfigLoadError::BuildEffective(err.to_string()))?;
                override_policy::validate_override_document(&yaml_value).map_err(
                    |invalid_paths| ConfigLoadError::InvalidOverridePaths {
                        path: paths.override_config_path.clone(),
                        paths: invalid_paths,
                    },
                )?;
                let yaml = serde_yaml::to_string(document)
                    .map_err(|err| ConfigLoadError::BuildEffective(err.to_string()))?;
                builder = builder.add_source(File::from_str(&yaml, FileFormat::Yaml));
                Some(document.clone())
            }
            None if paths.override_config_path.exists() => {
                validate_override_file(paths)?;
                builder = builder
                    .add_source(File::from(paths.override_config_path.as_path()).required(false));
                None
            }
            None => None,
        }
    } else {
        None
    };

    let source_trace_options = source::ConfigSourceTraceOptions {
        include_environment: options.include_environment,
        include_override: options.include_override,
    };

    let final_config: FinalConfig = builder
        .build()
        .map_err(|err| ConfigLoadError::BuildEffective(err.to_string()))?
        .try_deserialize()
        .map_err(|err| ConfigLoadError::DeserializeEffective(err.to_string()))?;
    let final_config = finalize_loaded_config(final_config);

    let source_report = source::build_config_source_report_with_runtime_sources(
        paths,
        &default.program_default_config,
        &default.config,
        &final_config,
        source_trace_options,
        override_report_value,
        environment_source,
    )
    .map_err(ConfigLoadError::TraceSources)?;

    Ok(LoadedConfig {
        config: final_config,
        source_report,
        program_default_config: default.program_default_config,
        default_config: default.config,
        merged_default_yaml: default.merged_yaml,
        paths: paths.clone(),
    })
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

fn validate_override_file(paths: &ConfigPaths) -> Result<(), ConfigLoadError> {
    let content = fs::read_to_string(&paths.override_config_path).map_err(|source| {
        ConfigLoadError::ReadOverride {
            path: paths.override_config_path.clone(),
            source,
        }
    })?;

    if content.trim().is_empty() {
        return Ok(());
    }

    let value =
        serde_yaml::from_str(&content).map_err(|source| ConfigLoadError::ParseOverride {
            path: paths.override_config_path.clone(),
            source,
        })?;

    override_policy::validate_override_document(&value).map_err(|invalid_paths| {
        ConfigLoadError::InvalidOverridePaths {
            path: paths.override_config_path.clone(),
            paths: invalid_paths,
        }
    })
}
