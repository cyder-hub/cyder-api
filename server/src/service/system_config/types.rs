use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::source::ConfigLayerKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigValueKind {
    Bool,
    Enum,
    NullableObject,
    NullableString,
    NullableU64,
    Object,
    String,
    U16,
    U32,
    U64,
    Usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFieldMetadata {
    pub path: String,
    pub section: String,
    pub value_kind: ConfigValueKind,
    pub editable: bool,
    pub hot_reloadable: bool,
    pub restart_required: bool,
    pub sensitive: bool,
    pub description: String,
    pub constraints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFieldSourceReport {
    pub kind: ConfigLayerKind,
    pub source_name: String,
    pub configured: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigFieldReport {
    pub path: String,
    pub section: String,
    pub value_kind: ConfigValueKind,
    pub editable: bool,
    pub hot_reloadable: bool,
    pub restart_required: bool,
    pub sensitive: bool,
    pub description: String,
    pub constraints: Vec<String>,
    pub value: Value,
    pub source: ConfigFieldSourceReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemConfigReportSummary {
    pub version: u64,
    pub loaded_at: i64,
    pub last_error: Option<String>,
    pub override_path: String,
    pub override_exists: bool,
    pub history_path: String,
    pub history_exists: bool,
    pub deployment_mode: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverrideFileReport {
    pub path: String,
    pub exists: bool,
    pub yaml: String,
    pub invalid_paths: Vec<String>,
    pub last_modified_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceHealthStatus {
    Ok,
    Warning,
    Error,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistenceHealthItem {
    pub key: String,
    pub path: String,
    pub exists: bool,
    pub readable: bool,
    pub writable: bool,
    pub status: PersistenceHealthStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistenceHealthReport {
    pub status: PersistenceHealthStatus,
    pub items: Vec<PersistenceHealthItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedConfigReport {
    pub summary: SystemConfigReportSummary,
    pub fields: Vec<ConfigFieldReport>,
    pub effective: Value,
    pub override_file: OverrideFileReport,
    pub persistence_health: PersistenceHealthReport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemConfigChangeRequest {
    #[serde(default)]
    pub changes: BTreeMap<String, Value>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemConfigResetRequest {
    #[serde(default)]
    pub paths: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemConfigHistoryQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemConfigValidationIssue {
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemConfigValidationReport {
    pub valid: bool,
    pub errors: Vec<SystemConfigValidationIssue>,
    pub warnings: Vec<SystemConfigValidationIssue>,
}

impl SystemConfigValidationReport {
    pub fn valid() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn invalid(errors: Vec<SystemConfigValidationIssue>) -> Self {
        Self {
            valid: false,
            errors,
            warnings: Vec::new(),
        }
    }

    pub fn valid_with_warnings(warnings: Vec<SystemConfigValidationIssue>) -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemConfigDiffItem {
    pub path: String,
    pub old_value: Value,
    pub new_value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemConfigRuntimeActions {
    pub update_runtime_snapshot: bool,
    pub update_log_level: bool,
    pub rebuild_http_client: bool,
    pub hot_reloadable_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemConfigPreviewResponse {
    pub diff: Vec<SystemConfigDiffItem>,
    pub validation: SystemConfigValidationReport,
    pub next_override_yaml: String,
    pub runtime_actions: SystemConfigRuntimeActions,
    pub write_disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemConfigHistoryOperation {
    Apply,
    Reset,
    Reload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemConfigHistoryItem {
    pub changed_at: i64,
    pub actor: String,
    pub reason: Option<String>,
    pub operation: SystemConfigHistoryOperation,
    pub version_before: u64,
    pub version_after: u64,
    pub changed_paths: Vec<String>,
    pub diff: Vec<SystemConfigDiffItem>,
}
