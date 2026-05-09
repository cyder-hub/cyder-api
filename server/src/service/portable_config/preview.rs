use super::{
    file_crypto::{PortableFileCryptoError, detect_file_protection},
    schema::{
        FileProtectionMode, PORTABLE_SCHEMA_VERSION, PortableBlockedItem,
        PortableFileProtectionStatus, PortableModuleId, PortablePreviewResponse,
        PortableSubrangeId,
    },
};

pub fn excluded_data_types() -> Vec<String> {
    [
        "request_log",
        "request_attempt",
        "request_replay_run",
        "request_replay_artifact",
        "object_storage_bundle",
        "object_storage_artifact",
        "metric_ingested_request_log",
        "metric_request_rollup_minute",
        "metric_attempt_rollup_minute",
        "metric_http_status_rollup_minute",
        "metric_cost_rollup_minute",
        "alert_event",
        "alert_rule_state",
        "notification_channel",
        "notification_channel_state",
        "notification_delivery",
        "notification_test_result",
        "api_key_rollup_daily",
        "api_key_rollup_monthly",
        "manager_auth_instance",
        "manager_auth_refresh_session",
        "manager_auth_login_rate_limit_runtime",
        "provider_circuit_runtime_state",
        "provider_key_cursor_runtime_state",
        "api_key_concurrency_window",
        "api_key_rpm_window",
        "config.default.yaml",
        "config.yaml",
        "config.override.yaml",
        "config.override.history.jsonl",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub fn blocked_item(
    code: impl Into<String>,
    message: impl Into<String>,
    path: impl Into<String>,
    module_id: Option<PortableModuleId>,
    subrange_id: Option<PortableSubrangeId>,
) -> PortableBlockedItem {
    PortableBlockedItem {
        code: code.into(),
        message: message.into(),
        path: path.into(),
        target: None,
        module_id,
        subrange_id,
    }
}

pub fn blocked_item_with_target(
    code: impl Into<String>,
    message: impl Into<String>,
    path: impl Into<String>,
    target: impl Into<String>,
    module_id: Option<PortableModuleId>,
    subrange_id: Option<PortableSubrangeId>,
) -> PortableBlockedItem {
    PortableBlockedItem {
        code: code.into(),
        message: message.into(),
        path: path.into(),
        target: Some(target.into()),
        module_id,
        subrange_id,
    }
}

pub fn blocked_file_preview(
    content: &str,
    err: &PortableFileCryptoError,
) -> PortablePreviewResponse {
    let mode = detect_file_protection(content);
    let (code, message, integrity_checked, integrity_valid) = match err {
        PortableFileCryptoError::PasswordRequired | PortableFileCryptoError::EmptyPassword => {
            ("password_required", err.to_string(), false, None)
        }
        PortableFileCryptoError::IntegrityMismatch => {
            ("integrity_mismatch", err.to_string(), true, Some(false))
        }
        PortableFileCryptoError::DecryptFailed => {
            ("decrypt_failed", err.to_string(), true, Some(true))
        }
        PortableFileCryptoError::InvalidArmor(_) | PortableFileCryptoError::Base64 { .. } => {
            ("invalid_armor", err.to_string(), false, Some(false))
        }
        _ => ("invalid_portable_file", err.to_string(), false, None),
    };

    PortablePreviewResponse {
        schema_version: PORTABLE_SCHEMA_VERSION.to_string(),
        exported_at: 0,
        cyder_version: String::new(),
        bundle_digest: String::new(),
        file_protection: PortableFileProtectionStatus {
            mode,
            requires_password: mode == FileProtectionMode::PasswordEncrypted,
            decrypted: false,
            integrity_checked,
            integrity_valid,
        },
        modules: Vec::new(),
        default_selected_modules: Vec::new(),
        unsupported_modules: Vec::new(),
        blocking_issues: vec![blocked_item(code, message, "$", None, None)],
        excluded_data_types: excluded_data_types(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{blocked_file_preview, excluded_data_types};
    use crate::service::portable_config::file_crypto::PortableFileCryptoError;

    #[test]
    fn excluded_data_types_are_explicit_about_runtime_and_stateful_categories() {
        let excluded = excluded_data_types();
        let excluded_set = excluded.iter().map(String::as_str).collect::<BTreeSet<_>>();

        for expected in [
            "request_log",
            "request_attempt",
            "request_replay_run",
            "request_replay_artifact",
            "object_storage_bundle",
            "object_storage_artifact",
            "metric_ingested_request_log",
            "metric_request_rollup_minute",
            "metric_attempt_rollup_minute",
            "metric_http_status_rollup_minute",
            "metric_cost_rollup_minute",
            "alert_event",
            "alert_rule_state",
            "notification_channel",
            "notification_channel_state",
            "notification_delivery",
            "notification_test_result",
            "api_key_rollup_daily",
            "api_key_rollup_monthly",
            "manager_auth_instance",
            "manager_auth_refresh_session",
            "manager_auth_login_rate_limit_runtime",
            "provider_circuit_runtime_state",
            "provider_key_cursor_runtime_state",
            "api_key_concurrency_window",
            "api_key_rpm_window",
            "config.default.yaml",
            "config.yaml",
            "config.override.yaml",
            "config.override.history.jsonl",
        ] {
            assert!(
                excluded_set.contains(expected),
                "excluded_data_types should include `{expected}`"
            );
        }

        for too_broad in [
            "metrics",
            "alert_state",
            "notification_state",
            "manager_auth_sessions",
            "runtime_redis_state",
            "system_config_files",
            "config_override_history",
        ] {
            assert!(
                !excluded_set.contains(too_broad),
                "excluded_data_types should not use broad category `{too_broad}`"
            );
        }
    }

    #[test]
    fn blocked_file_preview_returns_the_same_excluded_data_types() {
        let excluded = excluded_data_types();
        let preview = blocked_file_preview("", &PortableFileCryptoError::PasswordRequired);

        assert_eq!(preview.excluded_data_types, excluded);
    }
}
