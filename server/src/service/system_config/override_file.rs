use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use crate::config::override_policy;

use super::override_model::{
    OverrideModelError, empty_override_document, load_override_document, override_document_to_yaml,
    set_override_path,
};

#[derive(Debug)]
pub enum OverrideFileError {
    Model(OverrideModelError),
    InvalidPaths(Vec<String>),
    Io { path: PathBuf, source: io::Error },
}

impl std::fmt::Display for OverrideFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverrideFileError::Model(err) => write!(f, "{err}"),
            OverrideFileError::InvalidPaths(paths) => {
                write!(
                    f,
                    "override document contains unsupported paths: {}",
                    paths.join(", ")
                )
            }
            OverrideFileError::Io { path, source } => {
                write!(
                    f,
                    "failed to write override file '{}': {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for OverrideFileError {}

impl From<OverrideModelError> for OverrideFileError {
    fn from(err: OverrideModelError) -> Self {
        OverrideFileError::Model(err)
    }
}

pub fn read_override_document(path: &Path) -> Result<Value, OverrideFileError> {
    load_override_document(path).map_err(OverrideFileError::from)
}

pub fn validate_override_document(document: &Value) -> Result<(), OverrideFileError> {
    let yaml_value = serde_yaml::to_value(document).map_err(OverrideModelError::Serialize)?;
    override_policy::validate_override_document(&yaml_value)
        .map_err(OverrideFileError::InvalidPaths)
}

pub fn set_override_paths(
    document: &mut Value,
    changes: &BTreeMap<String, Value>,
) -> Result<(), OverrideFileError> {
    for (path, value) in changes {
        set_override_path(document, path, value.clone())?;
    }
    validate_override_document(document)
}

pub fn remove_override_paths(
    document: &mut Value,
    paths: &[String],
) -> Result<(), OverrideFileError> {
    for path in paths {
        remove_override_path(document, path);
    }
    prune_empty_objects(document);
    validate_override_document(document)
}

pub fn write_override_document_atomic(
    path: &Path,
    document: &Value,
) -> Result<(), OverrideFileError> {
    write_override_document_atomic_inner(path, document, false)
}

#[cfg(test)]
pub(crate) fn write_override_document_atomic_simulated_failure(
    path: &Path,
    document: &Value,
) -> Result<(), OverrideFileError> {
    write_override_document_atomic_inner(path, document, true)
}

fn write_override_document_atomic_inner(
    path: &Path,
    document: &Value,
    fail_before_rename: bool,
) -> Result<(), OverrideFileError> {
    validate_override_document(document)?;
    let yaml = override_document_to_yaml(document)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| OverrideFileError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let temp_path = temp_path_for(path);
    let write_result = write_temp_file(&temp_path, yaml.as_bytes());
    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    if fail_before_rename {
        let _ = fs::remove_file(&temp_path);
        return Err(OverrideFileError::Io {
            path: path.to_path_buf(),
            source: io::Error::other("simulated atomic write failure before rename"),
        });
    }

    fs::rename(&temp_path, path).map_err(|source| {
        let _ = fs::remove_file(&temp_path);
        OverrideFileError::Io {
            path: path.to_path_buf(),
            source,
        }
    })?;

    if let Err(err) = set_owner_read_write_permissions(path) {
        crate::warn_event!(
            "manager.system_config_override_permission_failed",
            path = &path.display().to_string(),
            error = &err.to_string(),
        );
    }
    sync_parent_dir(parent);
    Ok(())
}

fn write_temp_file(path: &Path, bytes: &[u8]) -> Result<(), OverrideFileError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| OverrideFileError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(bytes)
        .map_err(|source| OverrideFileError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.sync_all().map_err(|source| OverrideFileError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn temp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config.override.yaml");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nanos))
}

#[cfg(unix)]
fn set_owner_read_write_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn set_owner_read_write_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn sync_parent_dir(parent: &Path) {
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
}

fn remove_override_path(document: &mut Value, path: &str) {
    let segments = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    remove_segments(document, &segments);
}

fn remove_segments(value: &mut Value, segments: &[&str]) -> bool {
    if segments.is_empty() {
        return false;
    }

    let Some(object) = value.as_object_mut() else {
        return false;
    };

    if segments.len() == 1 {
        object.remove(segments[0]);
    } else if let Some(child) = object.get_mut(segments[0]) {
        let remove_child = remove_segments(child, &segments[1..]);
        if remove_child {
            object.remove(segments[0]);
        }
    }

    object.is_empty()
}

fn prune_empty_objects(value: &mut Value) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };

    let keys = object.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let remove_child = object
            .get_mut(&key)
            .map(prune_empty_objects)
            .unwrap_or(false);
        if remove_child {
            object.remove(&key);
        }
    }

    object.is_empty()
}

pub fn empty_document() -> Value {
    empty_override_document()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    #[test]
    fn set_path_builds_nested_yaml_structure() {
        let mut document = empty_document();
        let changes = BTreeMap::from([
            (
                "routing_resilience.max_candidates_per_request".to_string(),
                json!(3),
            ),
            (
                "proxy_request.total_timeout_seconds".to_string(),
                Value::Null,
            ),
        ]);

        set_override_paths(&mut document, &changes).expect("paths should set");
        let yaml = override_document_to_yaml(&document).expect("yaml should serialize");
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("yaml should parse");

        assert_eq!(
            parsed["routing_resilience"]["max_candidates_per_request"],
            serde_yaml::Value::Number(3.into())
        );
        assert!(parsed["proxy_request"]["total_timeout_seconds"].is_null());
    }

    #[test]
    fn reset_path_prunes_empty_parent_maps() {
        let mut document = json!({
            "routing_resilience": {
                "max_candidates_per_request": 3
            },
            "proxy_request": {
                "total_timeout_seconds": null
            }
        });

        remove_override_paths(
            &mut document,
            &[
                "routing_resilience.max_candidates_per_request".to_string(),
                "proxy_request.total_timeout_seconds".to_string(),
            ],
        )
        .expect("paths should reset");

        assert_eq!(document, json!({}));
    }

    #[test]
    fn current_override_with_non_whitelisted_path_is_rejected() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let path = temp_dir.path().join("config.override.yaml");
        std::fs::write(
            &path,
            r#"
db_url: postgres://example
log_level: debug
"#,
        )
        .expect("override should be written");

        let err = read_override_document(&path).expect_err("invalid override should fail");

        assert!(err.to_string().contains("db_url"));
    }

    #[test]
    fn atomic_write_simulated_failure_keeps_old_file_parseable() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let path = temp_dir.path().join("config.override.yaml");
        std::fs::write(&path, "log_level: info\n").expect("old override should be written");

        let document = json!({
            "log_level": "debug"
        });
        let err = write_override_document_atomic_simulated_failure(&path, &document)
            .expect_err("simulated write should fail");
        assert!(err.to_string().contains("simulated atomic write failure"));

        let loaded = read_override_document(&path).expect("old override should still parse");
        assert_eq!(loaded["log_level"], json!("info"));
    }
}
