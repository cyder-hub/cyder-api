use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use chrono::Utc;

use super::{
    redaction::redact_config_tree_value,
    types::{SystemConfigDiffItem, SystemConfigHistoryItem, SystemConfigHistoryOperation},
};

#[derive(Debug)]
pub enum SystemConfigHistoryError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Serialize(serde_json::Error),
    Deserialize {
        line: usize,
        source: serde_json::Error,
    },
}

impl std::fmt::Display for SystemConfigHistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemConfigHistoryError::Io { path, source } => {
                write!(
                    f,
                    "failed to access system config history '{}': {source}",
                    path.display()
                )
            }
            SystemConfigHistoryError::Serialize(source) => {
                write!(
                    f,
                    "failed to serialize system config history item: {source}"
                )
            }
            SystemConfigHistoryError::Deserialize { line, source } => {
                write!(
                    f,
                    "failed to deserialize system config history item at line {line}: {source}"
                )
            }
        }
    }
}

impl std::error::Error for SystemConfigHistoryError {}

pub fn build_history_item(
    operation: SystemConfigHistoryOperation,
    reason: Option<String>,
    version_before: u64,
    version_after: u64,
    diff: Vec<SystemConfigDiffItem>,
) -> SystemConfigHistoryItem {
    let changed_paths = diff.iter().map(|item| item.path.clone()).collect();
    SystemConfigHistoryItem {
        changed_at: Utc::now().timestamp_millis(),
        actor: "single_admin".to_string(),
        reason,
        operation,
        version_before,
        version_after,
        changed_paths,
        diff: redact_diff(diff),
    }
}

pub fn append_history_item(
    path: &Path,
    item: &SystemConfigHistoryItem,
) -> Result<(), SystemConfigHistoryError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| SystemConfigHistoryError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| SystemConfigHistoryError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let line = serde_json::to_string(item).map_err(SystemConfigHistoryError::Serialize)?;
    file.write_all(line.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|source| SystemConfigHistoryError::Io {
            path: path.to_path_buf(),
            source,
        })
}

pub fn read_history_items(
    path: &Path,
    limit: usize,
    offset: usize,
) -> Result<Vec<SystemConfigHistoryItem>, SystemConfigHistoryError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = OpenOptions::new().read(true).open(path).map_err(|source| {
        SystemConfigHistoryError::Io {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let mut items = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| SystemConfigHistoryError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let item = serde_json::from_str(&line).map_err(|source| {
            SystemConfigHistoryError::Deserialize {
                line: index + 1,
                source,
            }
        })?;
        items.push(item);
    }

    items.reverse();
    Ok(items.into_iter().skip(offset).take(limit).collect())
}

fn redact_diff(diff: Vec<SystemConfigDiffItem>) -> Vec<SystemConfigDiffItem> {
    diff.into_iter()
        .map(|item| SystemConfigDiffItem {
            old_value: redact_config_tree_value(&item.path, &item.old_value),
            new_value: redact_config_tree_value(&item.path, &item.new_value),
            path: item.path,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn history_jsonl_round_trips_line_by_line() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let path = temp_dir.path().join("config.override.history.jsonl");
        let first = build_history_item(
            SystemConfigHistoryOperation::Apply,
            Some("increase retry budget".to_string()),
            1,
            2,
            vec![SystemConfigDiffItem {
                path: "routing_resilience.max_candidates_per_request".to_string(),
                old_value: json!(2),
                new_value: json!(3),
            }],
        );
        let second = build_history_item(
            SystemConfigHistoryOperation::Reset,
            Some("restore default".to_string()),
            2,
            3,
            vec![SystemConfigDiffItem {
                path: "log_level".to_string(),
                old_value: json!("debug"),
                new_value: json!("info"),
            }],
        );

        append_history_item(&path, &first).expect("first item should append");
        append_history_item(&path, &second).expect("second item should append");

        let items = read_history_items(&path, 10, 0).expect("history should read");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].operation, SystemConfigHistoryOperation::Reset);
        assert_eq!(items[1].operation, SystemConfigHistoryOperation::Apply);

        let paged = read_history_items(&path, 1, 1).expect("paged history should read");
        assert_eq!(paged.len(), 1);
        assert_eq!(paged[0].operation, SystemConfigHistoryOperation::Apply);
    }

    #[test]
    fn history_redacts_sensitive_diff_values() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let path = temp_dir.path().join("config.override.history.jsonl");
        let item = build_history_item(
            SystemConfigHistoryOperation::Apply,
            Some("test redaction".to_string()),
            1,
            2,
            vec![SystemConfigDiffItem {
                path: "secret_key".to_string(),
                old_value: json!("old-secret-value"),
                new_value: json!("new-secret-value"),
            }],
        );

        append_history_item(&path, &item).expect("item should append");

        let content = std::fs::read_to_string(&path).expect("history should be readable");
        assert!(!content.contains("old-secret-value"));
        assert!(!content.contains("new-secret-value"));
        assert!(content.contains("sha256_prefix"));
    }
}
