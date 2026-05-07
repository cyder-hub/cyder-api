use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

use config::{Config, File, Source};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{FinalConfig, env, paths::ConfigPaths};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigLayerKind {
    ProgramDefault,
    DefaultFile,
    UserFile,
    Environment,
    OverrideFile,
    Derived,
}

impl ConfigLayerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProgramDefault => "program_default",
            Self::DefaultFile => "default_file",
            Self::UserFile => "user_file",
            Self::Environment => "environment",
            Self::OverrideFile => "override_file",
            Self::Derived => "derived",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedConfigLayer {
    pub kind: ConfigLayerKind,
    pub source_name: String,
    pub source_path: Option<PathBuf>,
    pub value: Value,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFieldSource {
    pub kind: ConfigLayerKind,
    pub source_name: String,
    pub source_path: Option<PathBuf>,
    pub configured: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ConfigSourceReport {
    pub layers: Vec<LoadedConfigLayer>,
    pub fields: BTreeMap<String, ConfigFieldSource>,
}

impl ConfigSourceReport {
    pub fn resolve_field_source(&self, path: &str) -> Option<&ConfigFieldSource> {
        self.fields.get(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigSourceTraceOptions {
    pub include_environment: bool,
    pub include_override: bool,
}

#[derive(Debug)]
pub enum ConfigSourceError {
    SerializeLayer {
        layer: ConfigLayerKind,
        source: serde_json::Error,
    },
    BuildLayer {
        layer: ConfigLayerKind,
        source_name: String,
        source: config::ConfigError,
    },
    DeserializeLayer {
        layer: ConfigLayerKind,
        source_name: String,
        source: config::ConfigError,
    },
    Environment(env::EnvironmentConfigError),
}

impl fmt::Display for ConfigSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigSourceError::SerializeLayer { layer, source } => write!(
                f,
                "failed to serialize {} layer for source tracking: {source}",
                layer.as_str()
            ),
            ConfigSourceError::BuildLayer {
                layer,
                source_name,
                source,
            } => write!(
                f,
                "failed to build {} layer '{}' for source tracking: {source}",
                layer.as_str(),
                source_name
            ),
            ConfigSourceError::DeserializeLayer {
                layer,
                source_name,
                source,
            } => write!(
                f,
                "failed to deserialize {} layer '{}' for source tracking: {source}",
                layer.as_str(),
                source_name
            ),
            ConfigSourceError::Environment(err) => {
                write!(f, "failed to read environment configuration source: {err}")
            }
        }
    }
}

impl std::error::Error for ConfigSourceError {}

pub fn environment_source() -> Result<env::EnvironmentConfigSource, env::EnvironmentConfigError> {
    env::EnvironmentConfigSource::current()
}

pub fn build_config_source_report(
    paths: &ConfigPaths,
    program_default_config: &FinalConfig,
    default_config: &FinalConfig,
    final_config: &FinalConfig,
    options: ConfigSourceTraceOptions,
) -> Result<ConfigSourceReport, ConfigSourceError> {
    build_config_source_report_inner(
        paths,
        program_default_config,
        default_config,
        final_config,
        options,
        None,
        None,
    )
}

pub fn build_config_source_report_with_override_value(
    paths: &ConfigPaths,
    program_default_config: &FinalConfig,
    default_config: &FinalConfig,
    final_config: &FinalConfig,
    options: ConfigSourceTraceOptions,
    override_value: Value,
) -> Result<ConfigSourceReport, ConfigSourceError> {
    build_config_source_report_inner(
        paths,
        program_default_config,
        default_config,
        final_config,
        options,
        Some(override_value),
        None,
    )
}

pub fn build_config_source_report_with_runtime_sources(
    paths: &ConfigPaths,
    program_default_config: &FinalConfig,
    default_config: &FinalConfig,
    final_config: &FinalConfig,
    options: ConfigSourceTraceOptions,
    override_value: Option<Value>,
    runtime_environment_source: Option<env::EnvironmentConfigSource>,
) -> Result<ConfigSourceReport, ConfigSourceError> {
    build_config_source_report_inner(
        paths,
        program_default_config,
        default_config,
        final_config,
        options,
        override_value,
        runtime_environment_source,
    )
}

fn build_config_source_report_inner(
    paths: &ConfigPaths,
    program_default_config: &FinalConfig,
    default_config: &FinalConfig,
    final_config: &FinalConfig,
    options: ConfigSourceTraceOptions,
    override_value: Option<Value>,
    runtime_environment_source: Option<env::EnvironmentConfigSource>,
) -> Result<ConfigSourceReport, ConfigSourceError> {
    let program_default_value =
        serialize_config_layer(ConfigLayerKind::ProgramDefault, program_default_config)?;
    let default_config_value =
        serialize_config_layer(ConfigLayerKind::DefaultFile, default_config)?;
    let final_config_value = serialize_config_layer(ConfigLayerKind::Derived, final_config)?;

    let program_default_layer = LoadedConfigLayer {
        kind: ConfigLayerKind::ProgramDefault,
        source_name: "program defaults".to_string(),
        source_path: None,
        fields: flatten_json_paths(&program_default_value),
        value: program_default_value.clone(),
    };

    let default_file_value = read_file_layer_value(
        ConfigLayerKind::DefaultFile,
        &paths.default_config_path,
        "config.default.yaml",
    )?;
    let default_file_layer = LoadedConfigLayer {
        kind: ConfigLayerKind::DefaultFile,
        source_name: paths.default_config_path.display().to_string(),
        source_path: Some(paths.default_config_path.clone()),
        fields: diff_json_paths(&program_default_value, &default_config_value),
        value: default_file_value,
    };

    let user_file_layer = if paths.user_config_path.exists() {
        let value = read_file_layer_value(
            ConfigLayerKind::UserFile,
            &paths.user_config_path,
            "user config",
        )?;
        LoadedConfigLayer {
            kind: ConfigLayerKind::UserFile,
            source_name: paths.user_config_path.display().to_string(),
            source_path: Some(paths.user_config_path.clone()),
            fields: flatten_json_paths(&value),
            value,
        }
    } else {
        empty_layer(
            ConfigLayerKind::UserFile,
            paths.user_config_path.display().to_string(),
            Some(paths.user_config_path.clone()),
        )
    };

    let environment_layer = if options.include_environment {
        let environment_source = match runtime_environment_source {
            Some(environment_source) => environment_source,
            None => environment_source().map_err(ConfigSourceError::Environment)?,
        };
        let value = read_source_layer_value(
            ConfigLayerKind::Environment,
            env::ENVIRONMENT_SOURCE_NAME,
            environment_source,
        )?;
        LoadedConfigLayer {
            kind: ConfigLayerKind::Environment,
            source_name: env::ENVIRONMENT_SOURCE_NAME.to_string(),
            source_path: None,
            fields: flatten_json_paths(&value),
            value,
        }
    } else {
        empty_layer(
            ConfigLayerKind::Environment,
            env::ENVIRONMENT_SOURCE_NAME.to_string(),
            None,
        )
    };

    let override_file_layer = if options.include_override {
        let value = match override_value {
            Some(value) => value,
            None if paths.override_config_path.exists() => read_file_layer_value(
                ConfigLayerKind::OverrideFile,
                &paths.override_config_path,
                "config.override.yaml",
            )?,
            None => Value::Object(Default::default()),
        };
        LoadedConfigLayer {
            kind: ConfigLayerKind::OverrideFile,
            source_name: paths.override_config_path.display().to_string(),
            source_path: Some(paths.override_config_path.clone()),
            fields: flatten_json_paths(&value),
            value,
        }
    } else {
        empty_layer(
            ConfigLayerKind::OverrideFile,
            paths.override_config_path.display().to_string(),
            Some(paths.override_config_path.clone()),
        )
    };

    let layers = vec![
        program_default_layer,
        default_file_layer,
        user_file_layer,
        environment_layer,
        override_file_layer,
    ];
    let fields = resolve_field_sources(&final_config_value, &layers);

    Ok(ConfigSourceReport { layers, fields })
}

fn serialize_config_layer<T: Serialize>(
    layer: ConfigLayerKind,
    value: &T,
) -> Result<Value, ConfigSourceError> {
    serde_json::to_value(value)
        .map_err(|source| ConfigSourceError::SerializeLayer { layer, source })
}

fn read_file_layer_value(
    layer: ConfigLayerKind,
    path: &Path,
    fallback_name: &str,
) -> Result<Value, ConfigSourceError> {
    if !path.exists() {
        return Ok(Value::Object(Default::default()));
    }

    read_source_layer_value(
        layer,
        &path
            .to_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| fallback_name.to_string()),
        File::from(path).required(false),
    )
}

fn read_source_layer_value<S>(
    layer: ConfigLayerKind,
    source_name: &str,
    source: S,
) -> Result<Value, ConfigSourceError>
where
    S: Source + Send + Sync + 'static,
{
    let config = Config::builder()
        .add_source(source)
        .build()
        .map_err(|source| ConfigSourceError::BuildLayer {
            layer,
            source_name: source_name.to_string(),
            source,
        })?;

    config
        .try_deserialize::<Value>()
        .map_err(|source| ConfigSourceError::DeserializeLayer {
            layer,
            source_name: source_name.to_string(),
            source,
        })
}

fn empty_layer(
    kind: ConfigLayerKind,
    source_name: String,
    source_path: Option<PathBuf>,
) -> LoadedConfigLayer {
    LoadedConfigLayer {
        kind,
        source_name,
        source_path,
        value: Value::Object(Default::default()),
        fields: BTreeMap::new(),
    }
}

fn resolve_field_sources(
    final_config_value: &Value,
    layers: &[LoadedConfigLayer],
) -> BTreeMap<String, ConfigFieldSource> {
    let final_fields = flatten_json_paths(final_config_value);
    let mut resolved = BTreeMap::new();

    for path in final_fields.keys() {
        let Some(layer) = layers
            .iter()
            .rev()
            .find(|layer| layer.fields.contains_key(path))
        else {
            continue;
        };

        resolved.insert(path.clone(), field_source_from_layer(layer));
    }

    apply_legacy_capture_derivation(&mut resolved);
    resolved
}

fn field_source_from_layer(layer: &LoadedConfigLayer) -> ConfigFieldSource {
    ConfigFieldSource {
        kind: layer.kind,
        source_name: layer.source_name.clone(),
        source_path: layer.source_path.clone(),
        configured: layer.kind != ConfigLayerKind::ProgramDefault,
        warnings: Vec::new(),
    }
}

fn apply_legacy_capture_derivation(fields: &mut BTreeMap<String, ConfigFieldSource>) {
    let diagnostics_path = "diagnostics.response_capture_max_bytes";
    let legacy_path = "replay_response_capture_max_bytes";
    let Some(legacy_source) = fields.get(legacy_path).cloned() else {
        return;
    };

    if !legacy_source.configured {
        return;
    }

    let diagnostics_source = fields.get(diagnostics_path).cloned();
    if diagnostics_source
        .as_ref()
        .is_some_and(|source| source.kind != ConfigLayerKind::ProgramDefault)
    {
        return;
    }

    let mut warnings = legacy_source.warnings;
    warnings.push(
        "diagnostics.response_capture_max_bytes was derived from legacy replay_response_capture_max_bytes"
            .to_string(),
    );

    fields.insert(
        diagnostics_path.to_string(),
        ConfigFieldSource {
            kind: ConfigLayerKind::Derived,
            source_name: format!(
                "derived from {legacy_path} via {}",
                legacy_source.kind.as_str()
            ),
            source_path: legacy_source.source_path,
            configured: legacy_source.configured,
            warnings,
        },
    );
}

pub fn flatten_json_paths(value: &Value) -> BTreeMap<String, Value> {
    let mut fields = BTreeMap::new();
    flatten_json_paths_into(value, "", &mut fields);
    fields
}

fn flatten_json_paths_into(value: &Value, prefix: &str, fields: &mut BTreeMap<String, Value>) {
    if !prefix.is_empty() {
        fields.insert(prefix.to_string(), value.clone());
    }

    if let Value::Object(map) = value {
        for (key, child) in map {
            let path = if prefix.is_empty() {
                key.to_string()
            } else {
                format!("{prefix}.{key}")
            };
            flatten_json_paths_into(child, &path, fields);
        }
    }
}

fn diff_json_paths(base: &Value, value: &Value) -> BTreeMap<String, Value> {
    let mut fields = BTreeMap::new();
    diff_json_paths_into(Some(base), value, "", &mut fields);
    fields
}

fn diff_json_paths_into(
    base: Option<&Value>,
    value: &Value,
    prefix: &str,
    fields: &mut BTreeMap<String, Value>,
) {
    if !prefix.is_empty() && base != Some(value) {
        fields.insert(prefix.to_string(), value.clone());
    }

    if let Value::Object(map) = value {
        for (key, child) in map {
            let child_base = base
                .and_then(Value::as_object)
                .and_then(|base_map| base_map.get(key));
            let path = if prefix.is_empty() {
                key.to_string()
            } else {
                format!("{prefix}.{key}")
            };
            diff_json_paths_into(child_base, child, &path, fields);
        }
    }
}
