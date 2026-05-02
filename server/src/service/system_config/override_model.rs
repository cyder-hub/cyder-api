use std::{fs, path::Path};

use serde_json::{Map, Value};

use crate::config::override_policy;

#[derive(Debug)]
pub enum OverrideModelError {
    Read {
        path: String,
        source: std::io::Error,
    },
    Parse {
        path: String,
        source: serde_yaml::Error,
    },
    Convert {
        path: String,
        source: serde_json::Error,
    },
    InvalidPaths {
        paths: Vec<String>,
    },
    InvalidRoot,
    InvalidParent {
        path: String,
    },
    Serialize(serde_yaml::Error),
}

impl std::fmt::Display for OverrideModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverrideModelError::Read { path, source } => {
                write!(f, "failed to read override file '{path}': {source}")
            }
            OverrideModelError::Parse { path, source } => {
                write!(f, "failed to parse override file '{path}': {source}")
            }
            OverrideModelError::Convert { path, source } => {
                write!(f, "failed to convert override file '{path}': {source}")
            }
            OverrideModelError::InvalidPaths { paths } => {
                write!(
                    f,
                    "override file contains unsupported paths: {}",
                    paths.join(", ")
                )
            }
            OverrideModelError::InvalidRoot => {
                write!(f, "override document must be a mapping")
            }
            OverrideModelError::InvalidParent { path } => {
                write!(f, "override path '{path}' traverses a non-object value")
            }
            OverrideModelError::Serialize(source) => {
                write!(f, "failed to serialize override YAML: {source}")
            }
        }
    }
}

impl std::error::Error for OverrideModelError {}

pub fn empty_override_document() -> Value {
    Value::Object(Map::new())
}

pub fn load_override_document(path: &Path) -> Result<Value, OverrideModelError> {
    if !path.exists() {
        return Ok(empty_override_document());
    }

    let content = fs::read_to_string(path).map_err(|source| OverrideModelError::Read {
        path: path.display().to_string(),
        source,
    })?;

    if content.trim().is_empty() {
        return Ok(empty_override_document());
    }

    let yaml_value: serde_yaml::Value =
        serde_yaml::from_str(&content).map_err(|source| OverrideModelError::Parse {
            path: path.display().to_string(),
            source,
        })?;

    override_policy::validate_override_document(&yaml_value)
        .map_err(|paths| OverrideModelError::InvalidPaths { paths })?;

    let value = serde_json::to_value(yaml_value).map_err(|source| OverrideModelError::Convert {
        path: path.display().to_string(),
        source,
    })?;
    normalize_override_document(value)
}

pub fn normalize_override_document(value: Value) -> Result<Value, OverrideModelError> {
    match value {
        Value::Null => Ok(empty_override_document()),
        Value::Object(_) => Ok(value),
        _ => Err(OverrideModelError::InvalidRoot),
    }
}

pub fn set_override_path(
    document: &mut Value,
    path: &str,
    value: Value,
) -> Result<(), OverrideModelError> {
    let segments = split_path(path);
    if segments.is_empty() {
        return Err(OverrideModelError::InvalidParent {
            path: path.to_string(),
        });
    }

    let mut current = document;
    for segment in &segments[..segments.len() - 1] {
        let object = current
            .as_object_mut()
            .ok_or_else(|| OverrideModelError::InvalidParent {
                path: path.to_string(),
            })?;
        current = object
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }

    let object = current
        .as_object_mut()
        .ok_or_else(|| OverrideModelError::InvalidParent {
            path: path.to_string(),
        })?;
    object.insert(segments[segments.len() - 1].to_string(), value);
    Ok(())
}

pub fn value_at_path(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;
    for segment in split_path(path) {
        current = current.as_object()?.get(segment)?;
    }
    Some(current.clone())
}

pub fn override_document_to_yaml(document: &Value) -> Result<String, OverrideModelError> {
    serde_yaml::to_string(document).map_err(OverrideModelError::Serialize)
}

fn split_path(path: &str) -> Vec<&str> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .collect()
}
