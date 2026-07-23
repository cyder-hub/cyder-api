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

    // Secret values are intentionally redacted when FinalConfig is serialized. Re-add the
    // managed default source so a deployment that explicitly placed secret_encryption there
    // still loads it without ever copying the value into the generated merged YAML snapshot.
    if paths.default_config_path.exists() {
        builder =
            builder.add_source(File::from(paths.default_config_path.as_path()).required(false));
    }

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
    final_config
        .secret_encryption
        .validate_for_runtime()
        .map_err(ConfigLoadError::DeserializeEffective)?;
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

    const TEST_ENCRYPTION_KEY: &str =
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn load_user_yaml(yaml: &str) -> Result<FinalConfig, ConfigLoadError> {
        load_user_yaml_with_managed_key(yaml, true)
    }

    fn load_user_yaml_with_managed_key(
        yaml: &str,
        include_managed_key: bool,
    ) -> Result<FinalConfig, ConfigLoadError> {
        let temp_dir = tempfile::tempdir().expect("config test directory should be created");
        let paths = ConfigPaths::new(
            temp_dir.path().join("config.default.yaml"),
            temp_dir.path().join("config.yaml"),
        );
        if include_managed_key {
            fs::write(
                &paths.default_config_path,
                format!("secret_encryption:\n  encryption_key: '{TEST_ENCRYPTION_KEY}'\n"),
            )
            .expect("managed test config should be written");
        }
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
    fn effective_config_ignores_retired_manager_credential_fields_without_reserializing_them() {
        let config =
            load_user_yaml("port: 9123\nsecret_key: retired-secret\npassword_salt: retired-salt\n")
                .expect("retired manager credential fields should be ignored");

        assert_eq!(config.port, 9123);
        let serialized = serde_yaml::to_string(&config).expect("effective config should serialize");
        assert!(!serialized.contains("secret_key"));
        assert!(!serialized.contains("password_salt"));
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
    fn effective_config_rejects_multi_instance_deployment_mode() {
        let error = load_user_yaml("deployment:\n  mode: multi_instance\n")
            .expect_err("multi-instance deployment must not be accepted");

        assert!(
            error.to_string().contains("multi_instance"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn redis_runtime_state_requires_redis_unless_memory_fallback_is_enabled() {
        let mut config = crate::config::programmatic_default_config();
        config.runtime_state.backend = crate::config::RuntimeStateBackendType::Redis;

        let error = config
            .validate_runtime_state()
            .expect_err("redis runtime state without redis must fail");
        assert_eq!(
            error,
            "runtime_state.backend=redis requires redis configuration"
        );

        config.runtime_state.fallback_to_memory = true;
        config
            .validate_runtime_state()
            .expect("explicit memory fallback should permit missing redis");
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
    fn secret_encryption_requires_current_key_in_every_downstream_mode() {
        for yaml in [
            "port: 9123\n",
            "secret_encryption:\n  downstream_mode: one_time\n",
            "secret_encryption:\n  downstream_mode: recoverable\n",
        ] {
            let error = load_user_yaml_with_managed_key(yaml, false)
                .expect_err("effective config without current key should fail");
            assert!(
                error
                    .to_string()
                    .contains("encryption_key is required for all downstream modes")
            );
            assert!(!error.to_string().contains(TEST_ENCRYPTION_KEY));
        }
    }

    #[test]
    fn generated_default_redacts_required_key_as_null_without_bypassing_runtime_validation() {
        let temp_dir = tempfile::tempdir().expect("config test directory should be created");
        let paths = ConfigPaths::new(
            temp_dir.path().join("config.default.yaml"),
            temp_dir.path().join("config.yaml"),
        );

        let generated = load_default_config(&paths).expect("default snapshot should serialize");
        assert!(generated.merged_yaml.contains("encryption_key: null"));
        assert!(!generated.merged_yaml.contains(TEST_ENCRYPTION_KEY));

        let error = load_effective_config(
            &paths,
            ConfigLoadOptions {
                include_environment: false,
            },
        )
        .expect_err("redacted generated default must not satisfy required current key");
        assert!(
            error
                .to_string()
                .contains("encryption_key is required for all downstream modes")
        );
    }

    #[test]
    fn tracked_config_sample_marks_current_key_as_required_redacted_input() {
        let sample = include_str!("../../../config.sample.yaml");
        let document: serde_yaml::Value =
            serde_yaml::from_str(sample).expect("tracked config sample should parse");
        assert!(
            document["secret_encryption"]["encryption_key"].is_null(),
            "tracked sample must not contain a usable master key"
        );
        assert!(sample.contains("Required in every mode"));
        assert!(sample.contains("Provider credentials are always encrypted"));
    }

    #[test]
    fn secret_encryption_accepts_recoverable_current_and_distinct_previous_keys() {
        let current = "000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F";
        let previous = "FFEEDDCCBBAA99887766554433221100FFEEDDCCBBAA99887766554433221100";
        let config = load_user_yaml(&format!(
            "secret_encryption:\n  downstream_mode: recoverable\n  encryption_key: '{current}'\n  previous_encryption_key: '{previous}'\n"
        ))
        .expect("valid secret encryption config should load");

        assert_eq!(
            config.secret_encryption.downstream_mode,
            crate::config::DownstreamSecretMode::Recoverable
        );
        assert!(config.secret_encryption.encryption_key().is_some());
        assert!(config.secret_encryption.has_previous_encryption_key());

        let debug = format!("{:?}", config.secret_encryption);
        let serialized =
            serde_yaml::to_string(&config).expect("effective config should serialize safely");
        for output in [debug, serialized] {
            assert!(!output.contains(current));
            assert!(!output.contains(previous));
        }
    }

    #[test]
    fn secret_encryption_rejects_invalid_key_relationships_without_echoing_values() {
        let valid = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let cases = [
            "secret_encryption:\n  downstream_mode: recoverable\n  encryption_key: 'not-a-key'\n",
            "secret_encryption:\n  downstream_mode: recoverable\n  encryption_key: 'hex:000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'\n",
        ];
        for yaml in cases {
            let error = load_user_yaml(yaml).expect_err("invalid secret config should fail");
            assert!(!error.to_string().contains(valid));
        }

        let error = load_user_yaml_with_managed_key(
            "secret_encryption:\n  previous_encryption_key: '000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'\n",
            false,
        )
        .expect_err("previous key without current key should fail");
        assert!(error.to_string().contains("requires encryption_key"));
        assert!(!error.to_string().contains(valid));

        let error = load_user_yaml(&format!(
            "secret_encryption:\n  encryption_key: '{valid}'\n  previous_encryption_key: '{valid}'\n"
        ))
        .expect_err("equal current and previous keys should fail");
        assert!(error.to_string().contains("must differ"));
        assert!(!error.to_string().contains(valid));
    }

    #[test]
    fn managed_default_secret_value_is_reapplied_after_redacted_snapshot() {
        let temp_dir = tempfile::tempdir().expect("config test directory should be created");
        let paths = ConfigPaths::new(
            temp_dir.path().join("config.default.yaml"),
            temp_dir.path().join("config.yaml"),
        );
        let key = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        fs::write(
            &paths.default_config_path,
            format!(
                "secret_encryption:\n  downstream_mode: recoverable\n  encryption_key: '{key}'\n"
            ),
        )
        .expect("managed default should be written");

        let loaded_default = load_default_config(&paths).expect("default snapshot should load");
        assert!(!loaded_default.merged_yaml.contains(key));
        let config = load_effective_config(
            &paths,
            ConfigLoadOptions {
                include_environment: false,
            },
        )
        .expect("managed default secret should be reapplied");
        assert_eq!(
            config.secret_encryption.downstream_mode,
            crate::config::DownstreamSecretMode::Recoverable
        );
        assert!(config.secret_encryption.encryption_key().is_some());
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
