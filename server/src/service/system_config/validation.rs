use std::{collections::BTreeMap, fmt};

use chrono_tz::Tz;
use reqwest::Url;
use serde_json::Value;

use crate::{
    config::{FinalConfig, finalize_loaded_config, loader::LoadedConfig},
    logging,
};

use super::{
    metadata::metadata_by_path,
    override_model::{
        OverrideModelError, load_override_document, override_document_to_yaml, set_override_path,
        value_at_path,
    },
    redaction::redact_config_tree_value,
    types::{
        ConfigFieldMetadata, ConfigValueKind, SystemConfigChangeRequest, SystemConfigDiffItem,
        SystemConfigPreviewResponse, SystemConfigRuntimeActions, SystemConfigValidationIssue,
        SystemConfigValidationReport,
    },
};

#[derive(Debug, Clone)]
pub struct SystemConfigValidationError {
    pub validation: SystemConfigValidationReport,
}

impl fmt::Display for SystemConfigValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let summary = self
            .validation
            .errors
            .iter()
            .map(|issue| format!("{}: {}", issue.path, issue.message))
            .collect::<Vec<_>>()
            .join("; ");
        write!(f, "system config validation failed: {summary}")
    }
}

impl std::error::Error for SystemConfigValidationError {}

pub fn preview_override_changes(
    loaded: &LoadedConfig,
    request: &SystemConfigChangeRequest,
) -> Result<SystemConfigPreviewResponse, SystemConfigValidationError> {
    let metadata = metadata_by_path();
    let mut errors = Vec::new();

    for (path, value) in &request.changes {
        match metadata.get(path) {
            Some(field) => validate_change_field(path, value, field, &mut errors),
            None => errors.push(issue(
                path,
                "unknown_path",
                format!("configuration path '{path}' is not known"),
            )),
        }
    }

    if !errors.is_empty() {
        return Err(validation_error(errors));
    }

    let current_config_value = serde_json::to_value(&loaded.config).map_err(|err| {
        validation_error(vec![issue(
            "<config>",
            "serialize_failed",
            format!("failed to serialize current config: {err}"),
        )])
    })?;
    let mut next_config_value = current_config_value.clone();
    for (path, value) in &request.changes {
        set_override_path(&mut next_config_value, path, value.clone())
            .map_err(validation_error_from_override_model)?;
    }

    let next_config: FinalConfig =
        serde_json::from_value(next_config_value.clone()).map_err(|err| {
            validation_error(vec![issue(
                "<config>",
                "deserialize_failed",
                format!("changed configuration cannot be deserialized: {err}"),
            )])
        })?;
    let next_config = finalize_loaded_config(next_config);
    validate_config_combinations(&next_config).map_err(validation_error)?;
    let warnings = build_preview_warnings(&next_config, request);

    let mut next_override = load_override_document(&loaded.paths.override_config_path)
        .map_err(validation_error_from_override_model)?;
    for (path, value) in &request.changes {
        set_override_path(&mut next_override, path, value.clone())
            .map_err(validation_error_from_override_model)?;
    }
    let next_override_yaml =
        override_document_to_yaml(&next_override).map_err(validation_error_from_override_model)?;

    let next_config_value = serde_json::to_value(&next_config).map_err(|err| {
        validation_error(vec![issue(
            "<config>",
            "serialize_failed",
            format!("failed to serialize changed config: {err}"),
        )])
    })?;
    let diff = build_diff(&current_config_value, &next_config_value, &request.changes);
    let runtime_actions = build_runtime_actions(&diff);

    Ok(SystemConfigPreviewResponse {
        diff,
        validation: if warnings.is_empty() {
            SystemConfigValidationReport::valid()
        } else {
            SystemConfigValidationReport::valid_with_warnings(warnings)
        },
        next_override_yaml,
        runtime_actions,
        write_disabled_reason: None,
    })
}

pub fn validate_effective_runtime_config(
    config: &FinalConfig,
) -> Result<(), SystemConfigValidationError> {
    let metadata = metadata_by_path();
    let config_value = serde_json::to_value(config).map_err(|err| {
        validation_error(vec![issue(
            "<config>",
            "serialize_failed",
            format!("failed to serialize effective config: {err}"),
        )])
    })?;
    let mut errors = Vec::new();

    for (path, field) in metadata {
        if !field.editable || !field.hot_reloadable {
            continue;
        }

        let value = value_at_path(&config_value, &path).unwrap_or(Value::Null);
        if let Err(message) = validate_value_kind(&path, &value, &field.value_kind) {
            errors.push(issue(path, "invalid_type", message));
            continue;
        }
        if let Err(message) = validate_field_constraints(&path, &value) {
            errors.push(issue(path, "invalid_value", message));
        }
    }

    if let Err(mut combination_errors) = validate_config_combinations(config) {
        errors.append(&mut combination_errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation_error(errors))
    }
}

pub fn validate_non_empty_apply_changes(
    request: &SystemConfigChangeRequest,
) -> Result<(), SystemConfigValidationError> {
    if request.changes.is_empty() {
        return Err(validation_error(vec![issue(
            "<config>",
            "empty_changes",
            "empty_changes: system config apply requires at least one change",
        )]));
    }
    Ok(())
}

pub fn validate_non_empty_reset_paths(paths: &[String]) -> Result<(), SystemConfigValidationError> {
    if paths.is_empty() {
        return Err(validation_error(vec![issue(
            "<config>",
            "empty_paths",
            "empty_paths: system config reset requires at least one path",
        )]));
    }
    Ok(())
}

pub fn no_effective_change_error(path: impl Into<String>) -> SystemConfigValidationError {
    validation_error(vec![issue(
        path,
        "no_effective_change",
        "no_effective_change: request does not change effective configuration or managed override state",
    )])
}

pub fn validate_required_reason(
    operation: &str,
    reason: Option<&str>,
) -> Result<String, SystemConfigValidationError> {
    let trimmed = reason.unwrap_or_default().trim();
    if trimmed.is_empty() {
        return Err(validation_error(vec![issue(
            "reason",
            "required_reason",
            format!("required_reason: system config {operation} requires a non-empty reason"),
        )]));
    }
    Ok(trimmed.to_string())
}

fn validate_change_field(
    path: &str,
    value: &Value,
    metadata: &ConfigFieldMetadata,
    errors: &mut Vec<SystemConfigValidationIssue>,
) {
    if !metadata.editable || !metadata.hot_reloadable {
        errors.push(issue(
            path,
            "readonly_path",
            format!("configuration path '{path}' is read-only and cannot be written by the UI"),
        ));
        return;
    }

    if let Err(message) = validate_value_kind(path, value, &metadata.value_kind) {
        errors.push(issue(path, "invalid_type", message));
        return;
    }

    if let Err(message) = validate_field_constraints(path, value) {
        errors.push(issue(path, "invalid_value", message));
    }
}

fn validate_value_kind(path: &str, value: &Value, kind: &ConfigValueKind) -> Result<(), String> {
    match kind {
        ConfigValueKind::Bool => value
            .as_bool()
            .map(|_| ())
            .ok_or_else(|| format!("configuration path '{path}' must be a boolean")),
        ConfigValueKind::String | ConfigValueKind::Enum => value
            .as_str()
            .map(|_| ())
            .ok_or_else(|| format!("configuration path '{path}' must be a string")),
        ConfigValueKind::NullableString => {
            if value.is_null() || value.as_str().is_some() {
                Ok(())
            } else {
                Err(format!(
                    "configuration path '{path}' must be null or a string"
                ))
            }
        }
        ConfigValueKind::NullableU64 => {
            if value.is_null() || as_u64(value).is_some() {
                Ok(())
            } else {
                Err(format!(
                    "configuration path '{path}' must be null or an unsigned integer"
                ))
            }
        }
        ConfigValueKind::U16 => as_u64(value)
            .filter(|raw| *raw <= u16::MAX as u64)
            .map(|_| ())
            .ok_or_else(|| {
                format!("configuration path '{path}' must be an unsigned 16-bit integer")
            }),
        ConfigValueKind::U32 => as_u64(value)
            .filter(|raw| *raw <= u32::MAX as u64)
            .map(|_| ())
            .ok_or_else(|| {
                format!("configuration path '{path}' must be an unsigned 32-bit integer")
            }),
        ConfigValueKind::U64 => as_u64(value)
            .map(|_| ())
            .ok_or_else(|| format!("configuration path '{path}' must be an unsigned integer")),
        ConfigValueKind::Usize => as_u64(value)
            .filter(|raw| usize::try_from(*raw).is_ok())
            .map(|_| ())
            .ok_or_else(|| {
                format!("configuration path '{path}' must be an unsigned platform-size integer")
            }),
        ConfigValueKind::Object => value
            .as_object()
            .map(|_| ())
            .ok_or_else(|| format!("configuration path '{path}' must be an object")),
        ConfigValueKind::NullableObject => {
            if value.is_null() || value.as_object().is_some() {
                Ok(())
            } else {
                Err(format!(
                    "configuration path '{path}' must be null or an object"
                ))
            }
        }
    }
}

fn validate_field_constraints(path: &str, value: &Value) -> Result<(), String> {
    match path {
        "log_level" => validate_log_level(value),
        "timezone" => validate_timezone(value),
        "max_body_size" => validate_usize_range(path, value, 1024 * 1024, 512 * 1024 * 1024),
        "proxy" => validate_proxy(value),
        "proxy_request.connect_timeout_seconds" => validate_u64_range(path, value, 1, 120),
        "proxy_request.first_byte_timeout_seconds" | "proxy_request.total_timeout_seconds" => {
            validate_nullable_u64_range(path, value, 1, 86_400)
        }
        "provider_governance.consecutive_failure_threshold" => {
            validate_u64_range(path, value, 0, 100)
        }
        "provider_governance.open_cooldown_seconds" => validate_u64_range(path, value, 1, 86_400),
        "routing_resilience.same_candidate_max_retries" => validate_u64_range(path, value, 0, 10),
        "routing_resilience.max_candidates_per_request" => validate_u64_range(path, value, 1, 20),
        "routing_resilience.base_backoff_ms" => validate_u64_range(path, value, 0, 60_000),
        "routing_resilience.max_backoff_ms" => validate_u64_range(path, value, 0, 300_000),
        "routing_resilience.respect_retry_after_up_to_seconds" => {
            validate_u64_range(path, value, 0, 3_600)
        }
        "diagnostics.replay_preview_confirmation_ttl_seconds" => {
            validate_u64_range(path, value, 1, 86_400)
        }
        "diagnostics.replay_preview_confirmation_clock_skew_seconds" => {
            validate_u64_range(path, value, 0, 3_600)
        }
        "diagnostics.response_capture_max_bytes" => {
            validate_usize_range(path, value, 1024, 128 * 1024 * 1024)
        }
        "diagnostics.retention.request_log_bundle_retention_days"
        | "diagnostics.retention.replay_artifact_retention_days" => {
            validate_u64_range(path, value, 1, 3_650)
        }
        "diagnostics.retention.delete_batch_size" => validate_usize_range(path, value, 1, 10_000),
        _ => Ok(()),
    }
}

fn build_preview_warnings(
    next_config: &FinalConfig,
    request: &SystemConfigChangeRequest,
) -> Vec<SystemConfigValidationIssue> {
    let touches_provider_governance_runtime_fields = request.changes.keys().any(|path| {
        matches!(
            path.as_str(),
            "provider_governance.consecutive_failure_threshold"
                | "provider_governance.open_cooldown_seconds"
        )
    });

    if !next_config.provider_governance.enabled && touches_provider_governance_runtime_fields {
        vec![issue(
            "provider_governance.enabled",
            "provider_governance_disabled",
            "provider governance is disabled; threshold and cooldown changes will not affect circuit decisions until governance is enabled",
        )]
    } else {
        Vec::new()
    }
}

fn validate_config_combinations(
    config: &FinalConfig,
) -> Result<(), Vec<SystemConfigValidationIssue>> {
    let mut errors = Vec::new();

    let routing = &config.routing_resilience;
    if !(routing.base_backoff_ms == 0 && routing.max_backoff_ms == 0)
        && routing.base_backoff_ms > routing.max_backoff_ms
    {
        errors.push(issue(
            "routing_resilience.base_backoff_ms",
            "invalid_combination",
            "routing_resilience.base_backoff_ms must be less than or equal to routing_resilience.max_backoff_ms unless both are 0",
        ));
    }

    if let Some(total_timeout) = config.proxy_request.total_timeout_seconds {
        if config.proxy_request.connect_timeout_seconds > total_timeout {
            errors.push(issue(
                "proxy_request.connect_timeout_seconds",
                "invalid_combination",
                "proxy_request.connect_timeout_seconds must be less than or equal to proxy_request.total_timeout_seconds",
            ));
        }
    }

    if config
        .diagnostics
        .replay_preview_confirmation_clock_skew_seconds
        >= config.diagnostics.replay_preview_confirmation_ttl_seconds
    {
        errors.push(issue(
            "diagnostics.replay_preview_confirmation_clock_skew_seconds",
            "invalid_combination",
            "diagnostics.replay_preview_confirmation_clock_skew_seconds must be lower than diagnostics.replay_preview_confirmation_ttl_seconds",
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_log_level(value: &Value) -> Result<(), String> {
    let raw = value.as_str().unwrap_or_default();
    logging::parse_level(raw).map(|_| ())
}

fn validate_timezone(value: &Value) -> Result<(), String> {
    let Some(raw) = value.as_str() else {
        return Ok(());
    };

    raw.parse::<Tz>()
        .map(|_| ())
        .map_err(|_| format!("configuration path 'timezone' must be null or a valid IANA timezone"))
}

fn validate_proxy(value: &Value) -> Result<(), String> {
    let Some(raw) = value.as_str() else {
        return Ok(());
    };

    let url = Url::parse(raw)
        .map_err(|err| format!("configuration path 'proxy' must be a valid URL: {err}"))?;
    match url.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(format!(
            "configuration path 'proxy' only supports http:// or https:// URLs, got scheme '{scheme}'"
        )),
    }
}

fn validate_u64_range(path: &str, value: &Value, min: u64, max: u64) -> Result<(), String> {
    let raw = as_u64(value).unwrap_or_default();
    if (min..=max).contains(&raw) {
        Ok(())
    } else {
        Err(format!(
            "configuration path '{path}' must be between {min} and {max}"
        ))
    }
}

fn validate_nullable_u64_range(
    path: &str,
    value: &Value,
    min: u64,
    max: u64,
) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    validate_u64_range(path, value, min, max)
}

fn validate_usize_range(path: &str, value: &Value, min: usize, max: usize) -> Result<(), String> {
    let raw = as_u64(value)
        .and_then(|raw| usize::try_from(raw).ok())
        .unwrap_or_default();
    if (min..=max).contains(&raw) {
        Ok(())
    } else {
        Err(format!(
            "configuration path '{path}' must be between {min} and {max}"
        ))
    }
}

fn as_u64(value: &Value) -> Option<u64> {
    if value.is_number() {
        value.as_u64()
    } else {
        None
    }
}

fn build_diff(
    current: &Value,
    next: &Value,
    changes: &BTreeMap<String, Value>,
) -> Vec<SystemConfigDiffItem> {
    changes
        .keys()
        .filter_map(|path| {
            let old_value = value_at_path(current, path).unwrap_or(Value::Null);
            let new_value = value_at_path(next, path).unwrap_or(Value::Null);
            (old_value != new_value).then(|| SystemConfigDiffItem {
                path: path.clone(),
                old_value: redact_config_tree_value(path, &old_value),
                new_value: redact_config_tree_value(path, &new_value),
            })
        })
        .collect()
}

fn build_runtime_actions(diff: &[SystemConfigDiffItem]) -> SystemConfigRuntimeActions {
    let hot_reloadable_paths = diff
        .iter()
        .map(|item| item.path.clone())
        .collect::<Vec<_>>();
    let update_log_level = diff.iter().any(|item| item.path == "log_level");
    let rebuild_http_client = diff
        .iter()
        .any(|item| item.path == "proxy" || item.path.starts_with("proxy_request."));

    SystemConfigRuntimeActions {
        update_runtime_snapshot: !diff.is_empty(),
        update_log_level,
        rebuild_http_client,
        hot_reloadable_paths,
    }
}

fn validation_error(errors: Vec<SystemConfigValidationIssue>) -> SystemConfigValidationError {
    SystemConfigValidationError {
        validation: SystemConfigValidationReport::invalid(errors),
    }
}

fn validation_error_from_override_model(err: OverrideModelError) -> SystemConfigValidationError {
    validation_error(vec![issue(
        "config.override.yaml",
        "override_model_error",
        err.to_string(),
    )])
}

fn issue(
    path: impl Into<String>,
    code: impl Into<String>,
    message: impl Into<String>,
) -> SystemConfigValidationIssue {
    SystemConfigValidationIssue {
        path: path.into(),
        code: code.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use crate::config::{
        loader::{ConfigLoadOptions, load_effective_config},
        paths::ConfigPaths,
    };

    use super::*;

    fn load_test_config(paths: &ConfigPaths) -> LoadedConfig {
        load_effective_config(
            paths,
            ConfigLoadOptions {
                include_environment: false,
                include_override: true,
            },
        )
        .expect("config should load")
    }

    fn change_request(changes: &[(&str, Value)]) -> SystemConfigChangeRequest {
        SystemConfigChangeRequest {
            changes: changes
                .iter()
                .map(|(path, value)| ((*path).to_string(), value.clone()))
                .collect(),
            reason: None,
        }
    }

    #[test]
    fn preview_rejects_unknown_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let err = preview_override_changes(
            &loaded,
            &change_request(&[("not.a.real.path", json!(true))]),
        )
        .expect_err("unknown path should fail");

        assert_eq!(err.validation.errors[0].path, "not.a.real.path");
        assert_eq!(err.validation.errors[0].code, "unknown_path");
    }

    #[test]
    fn preview_rejects_readonly_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let err =
            preview_override_changes(&loaded, &change_request(&[("db_url", json!("sqlite.db"))]))
                .expect_err("read-only path should fail");

        assert_eq!(err.validation.errors[0].path, "db_url");
        assert_eq!(err.validation.errors[0].code, "readonly_path");
    }

    #[test]
    fn preview_rejects_invalid_proxy_url() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let err = preview_override_changes(
            &loaded,
            &change_request(&[("proxy", json!("socks5://127.0.0.1:1080"))]),
        )
        .expect_err("invalid proxy should fail");

        assert_eq!(err.validation.errors[0].path, "proxy");
        assert!(
            err.validation.errors[0]
                .message
                .contains("http:// or https://")
        );
    }

    #[test]
    fn preview_rejects_invalid_log_level() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let err =
            preview_override_changes(&loaded, &change_request(&[("log_level", json!("verbose"))]))
                .expect_err("invalid log level should fail");

        assert_eq!(err.validation.errors[0].path, "log_level");
        assert!(
            err.validation.errors[0]
                .message
                .contains("invalid log level")
        );
    }

    #[test]
    fn preview_rejects_log_level_off() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let err =
            preview_override_changes(&loaded, &change_request(&[("log_level", json!("off"))]))
                .expect_err("off log level should fail");

        assert_eq!(err.validation.errors[0].path, "log_level");
        assert!(err.validation.errors[0].message.contains("warn, or error"));
    }

    #[test]
    fn preview_rejects_invalid_timezone() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let err = preview_override_changes(
            &loaded,
            &change_request(&[("timezone", json!("Not/AZone"))]),
        )
        .expect_err("invalid timezone should fail");

        assert_eq!(err.validation.errors[0].path, "timezone");
        assert!(
            err.validation.errors[0]
                .message
                .contains("valid IANA timezone")
        );
    }

    #[test]
    fn preview_rejects_backoff_combination_error() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let err = preview_override_changes(
            &loaded,
            &change_request(&[("routing_resilience.base_backoff_ms", json!(2_000))]),
        )
        .expect_err("invalid backoff relationship should fail");

        assert_eq!(
            err.validation.errors[0].path,
            "routing_resilience.base_backoff_ms"
        );
        assert_eq!(err.validation.errors[0].code, "invalid_combination");
    }

    #[test]
    fn preview_rejects_diagnostics_confirmation_ranges() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let ttl_err = preview_override_changes(
            &loaded,
            &change_request(&[(
                "diagnostics.replay_preview_confirmation_ttl_seconds",
                json!(86_401),
            )]),
        )
        .expect_err("ttl above range should fail");
        assert_eq!(
            ttl_err.validation.errors[0].path,
            "diagnostics.replay_preview_confirmation_ttl_seconds"
        );

        let skew_err = preview_override_changes(
            &loaded,
            &change_request(&[(
                "diagnostics.replay_preview_confirmation_clock_skew_seconds",
                json!(3_601),
            )]),
        )
        .expect_err("clock skew above range should fail");
        assert_eq!(
            skew_err.validation.errors[0].path,
            "diagnostics.replay_preview_confirmation_clock_skew_seconds"
        );
    }

    #[test]
    fn preview_warns_when_provider_governance_runtime_fields_change_while_disabled() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        fs::write(
            temp_dir.path().join("config.yaml"),
            "provider_governance:\n  enabled: false\n",
        )
        .expect("base config should be written");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let response = preview_override_changes(
            &loaded,
            &change_request(&[(
                "provider_governance.consecutive_failure_threshold",
                json!(5),
            )]),
        )
        .expect("disabled governance threshold preview should remain legal");

        assert!(response.validation.valid);
        assert_eq!(response.validation.warnings.len(), 1);
        assert_eq!(
            response.validation.warnings[0].code,
            "provider_governance_disabled"
        );
    }

    #[test]
    fn preview_returns_diff_and_does_not_create_override_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let response = preview_override_changes(
            &loaded,
            &change_request(&[
                ("routing_resilience.max_candidates_per_request", json!(3)),
                ("proxy_request.first_byte_timeout_seconds", json!(120)),
            ]),
        )
        .expect("preview should succeed");

        assert!(response.validation.valid);
        assert_eq!(response.diff.len(), 2);
        assert!(response.diff.iter().any(|item| {
            item.path == "routing_resilience.max_candidates_per_request"
                && item.old_value == json!(2)
                && item.new_value == json!(3)
        }));
        assert!(response.runtime_actions.update_runtime_snapshot);
        assert!(response.runtime_actions.rebuild_http_client);
        assert!(!paths.override_config_path.exists());

        let yaml: serde_yaml::Value =
            serde_yaml::from_str(&response.next_override_yaml).expect("preview YAML should parse");
        assert_eq!(
            yaml["routing_resilience"]["max_candidates_per_request"],
            serde_yaml::Value::Number(3.into())
        );
    }

    #[test]
    fn preview_preserves_existing_legal_override_fields() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        fs::write(
            temp_dir.path().join("config.override.yaml"),
            "log_level: debug\n",
        )
        .expect("override should be written");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_test_config(&paths);

        let response = preview_override_changes(
            &loaded,
            &change_request(&[("routing_resilience.max_candidates_per_request", json!(4))]),
        )
        .expect("preview should succeed");

        let yaml: serde_yaml::Value =
            serde_yaml::from_str(&response.next_override_yaml).expect("preview YAML should parse");
        assert_eq!(yaml["log_level"], serde_yaml::Value::String("debug".into()));
        assert_eq!(
            yaml["routing_resilience"]["max_candidates_per_request"],
            serde_yaml::Value::Number(4.into())
        );
    }
}
