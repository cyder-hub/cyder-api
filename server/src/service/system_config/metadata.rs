use std::{
    collections::BTreeMap,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use crate::config::{
    loader::LoadedConfig,
    override_policy,
    override_policy::OVERRIDE_ALLOWED_PATHS,
    source::{ConfigFieldSource, ConfigLayerKind},
};

use super::{
    override_model::{normalize_override_document, override_document_to_yaml},
    redaction::{is_sensitive_config_path, redact_config_tree_value},
    types::{
        ConfigFieldMetadata, ConfigFieldReport, ConfigFieldSourceReport, ConfigValueKind,
        OverrideFileReport, ResolvedConfigReport, SystemConfigReportSummary,
    },
};

pub const UI_WRITE_FORBIDDEN_PATHS: &[&str] = &[
    "host",
    "port",
    "base_path",
    "secret_key",
    "password_salt",
    "jwt_secret",
    "api_key_jwt_secret",
    "db_url",
    "db_pool_size",
    "redis",
    "redis.url",
    "redis.pool_size",
    "redis.key_prefix",
    "deployment",
    "deployment.mode",
    "cache",
    "cache.catalog",
    "cache.catalog.backend",
    "cache.catalog.ttl",
    "cache.catalog.negative_ttl",
    "cache.catalog.redis",
    "cache.catalog.redis.key_prefix",
    "runtime_state",
    "runtime_state.backend",
    "runtime_state.redis",
    "runtime_state.redis.key_prefix",
    "runtime_state.redis.api_key_concurrency_lease_ttl_seconds",
    "runtime_state.redis.provider_circuit_probe_lease_ttl_seconds",
    "runtime_state.redis.state_ttl_seconds",
    "runtime_state.fallback_to_memory",
    "storage",
    "storage.driver",
    "storage.local",
    "storage.local.root",
    "storage.s3",
    "storage.s3.endpoint",
    "storage.s3.region",
    "storage.s3.bucket",
    "storage.s3.access_mode",
    "storage.s3.access_key",
    "storage.s3.secret_key",
    "storage.s3.force_path_style",
    "storage.s3.public_url",
    "replay_response_capture_max_bytes",
];

pub fn config_field_metadata() -> Vec<ConfigFieldMetadata> {
    let mut fields = vec![
        meta(
            "server",
            "host",
            ConfigValueKind::String,
            "Bind host for the management and proxy HTTP server.",
            &["read-only", "restart required"],
        ),
        meta(
            "server",
            "port",
            ConfigValueKind::U16,
            "Bind port for the management and proxy HTTP server.",
            &["1..=65535", "read-only", "restart required"],
        ),
        meta(
            "server",
            "base_path",
            ConfigValueKind::String,
            "Base path used to nest manager and proxy routes.",
            &["must start with /", "read-only", "restart required"],
        ),
        meta(
            "security",
            "secret_key",
            ConfigValueKind::String,
            "Manager login secret.",
            &["sensitive", "read-only", "restart required"],
        ),
        meta(
            "security",
            "password_salt",
            ConfigValueKind::String,
            "Password hashing salt.",
            &["sensitive", "read-only", "restart required"],
        ),
        meta(
            "security",
            "jwt_secret",
            ConfigValueKind::String,
            "Manager JWT signing secret.",
            &["sensitive", "read-only", "restart required"],
        ),
        meta(
            "security",
            "api_key_jwt_secret",
            ConfigValueKind::String,
            "Downstream API key JWT signing secret.",
            &["sensitive", "read-only", "restart required"],
        ),
        meta(
            "database",
            "db_url",
            ConfigValueKind::String,
            "Database connection URL or SQLite path.",
            &["password redacted", "read-only", "restart required"],
        ),
        meta(
            "database",
            "db_pool_size",
            ConfigValueKind::U32,
            "Database connection pool size.",
            &["positive integer", "read-only", "restart required"],
        ),
        meta(
            "network",
            "proxy",
            ConfigValueKind::NullableString,
            "Optional upstream HTTP proxy URL.",
            &["null or http(s) URL", "userinfo redacted"],
        ),
        meta(
            "observability",
            "log_level",
            ConfigValueKind::String,
            "Backend log level.",
            &["trace, debug, info, warn, or error"],
        ),
        meta(
            "server",
            "timezone",
            ConfigValueKind::NullableString,
            "Optional timezone used by date-boundary statistics.",
            &["null or IANA timezone name"],
        ),
        meta(
            "server",
            "max_body_size",
            ConfigValueKind::Usize,
            "Maximum accepted proxy request body size in bytes.",
            &["positive integer"],
        ),
        meta(
            "diagnostics",
            "replay_response_capture_max_bytes",
            ConfigValueKind::Usize,
            "Legacy response capture alias retained for compatibility.",
            &["legacy alias", "read-only"],
        ),
        meta(
            "diagnostics",
            "diagnostics",
            ConfigValueKind::Object,
            "Diagnostics configuration group.",
            &["object"],
        ),
        meta(
            "diagnostics",
            "diagnostics.replay_preview_confirmation_ttl_seconds",
            ConfigValueKind::U64,
            "Replay preview confirmation TTL in seconds.",
            &["1..=86400"],
        ),
        meta(
            "diagnostics",
            "diagnostics.replay_preview_confirmation_clock_skew_seconds",
            ConfigValueKind::U64,
            "Allowed replay confirmation clock skew in seconds.",
            &["0..=3600", "must be lower than confirmation TTL"],
        ),
        meta(
            "diagnostics",
            "diagnostics.response_capture_max_bytes",
            ConfigValueKind::Usize,
            "Maximum captured upstream response body bytes for diagnostics.",
            &["positive integer"],
        ),
        meta(
            "diagnostics",
            "diagnostics.raw_bundle_download_enabled",
            ConfigValueKind::Bool,
            "Whether raw request log bundle download is enabled.",
            &["boolean"],
        ),
        meta(
            "diagnostics",
            "diagnostics.retention",
            ConfigValueKind::Object,
            "Diagnostics retention policy group.",
            &["object"],
        ),
        meta(
            "diagnostics",
            "diagnostics.retention.enabled",
            ConfigValueKind::Bool,
            "Whether diagnostics retention cleanup is enabled.",
            &["boolean"],
        ),
        meta(
            "diagnostics",
            "diagnostics.retention.request_log_bundle_retention_days",
            ConfigValueKind::U64,
            "Request log bundle retention window in days.",
            &["positive integer"],
        ),
        meta(
            "diagnostics",
            "diagnostics.retention.replay_artifact_retention_days",
            ConfigValueKind::U64,
            "Replay artifact retention window in days.",
            &["positive integer"],
        ),
        meta(
            "diagnostics",
            "diagnostics.retention.delete_batch_size",
            ConfigValueKind::Usize,
            "Maximum diagnostics retention delete batch size.",
            &["positive integer"],
        ),
        meta(
            "redis",
            "redis",
            ConfigValueKind::NullableObject,
            "Optional shared Redis connection settings.",
            &["read-only", "restart required"],
        ),
        meta(
            "redis",
            "redis.url",
            ConfigValueKind::String,
            "Redis connection URL.",
            &["password redacted", "read-only", "restart required"],
        ),
        meta(
            "redis",
            "redis.pool_size",
            ConfigValueKind::Usize,
            "Redis connection pool size.",
            &["positive integer", "read-only", "restart required"],
        ),
        meta(
            "redis",
            "redis.key_prefix",
            ConfigValueKind::String,
            "Redis key prefix.",
            &["read-only", "restart required"],
        ),
        meta(
            "deployment",
            "deployment",
            ConfigValueKind::Object,
            "Deployment mode group.",
            &["object", "read-only", "restart required"],
        ),
        meta(
            "deployment",
            "deployment.mode",
            ConfigValueKind::Enum,
            "Single-instance or multi-instance deployment mode.",
            &[
                "single_instance or multi_instance",
                "read-only",
                "restart required",
            ],
        ),
        meta(
            "proxy",
            "proxy_request",
            ConfigValueKind::Object,
            "Proxy HTTP request timeout group.",
            &["object"],
        ),
        meta(
            "proxy",
            "proxy_request.connect_timeout_seconds",
            ConfigValueKind::U64,
            "HTTP connect timeout in seconds.",
            &["positive integer"],
        ),
        meta(
            "proxy",
            "proxy_request.first_byte_timeout_seconds",
            ConfigValueKind::NullableU64,
            "Optional first-byte timeout in seconds.",
            &["null or positive integer"],
        ),
        meta(
            "proxy",
            "proxy_request.total_timeout_seconds",
            ConfigValueKind::NullableU64,
            "Optional total request timeout in seconds.",
            &["null or positive integer"],
        ),
        meta(
            "governance",
            "provider_governance",
            ConfigValueKind::Object,
            "Provider circuit governance group.",
            &["object"],
        ),
        meta(
            "governance",
            "provider_governance.enabled",
            ConfigValueKind::Bool,
            "Whether provider circuit governance is enabled.",
            &["boolean", "false disables circuit decision updates"],
        ),
        meta(
            "governance",
            "provider_governance.consecutive_failure_threshold",
            ConfigValueKind::U32,
            "Consecutive provider failures before opening a circuit.",
            &["non-negative integer"],
        ),
        meta(
            "governance",
            "provider_governance.open_cooldown_seconds",
            ConfigValueKind::U64,
            "Provider circuit open cooldown in seconds.",
            &["positive integer"],
        ),
        meta(
            "routing",
            "routing_resilience",
            ConfigValueKind::Object,
            "Request retry and route fallback policy group.",
            &["object"],
        ),
        meta(
            "routing",
            "routing_resilience.same_candidate_max_retries",
            ConfigValueKind::U32,
            "Maximum retries against the same route candidate.",
            &["non-negative integer"],
        ),
        meta(
            "routing",
            "routing_resilience.max_candidates_per_request",
            ConfigValueKind::U32,
            "Maximum route candidates attempted per request.",
            &["positive integer"],
        ),
        meta(
            "routing",
            "routing_resilience.base_backoff_ms",
            ConfigValueKind::U64,
            "Base retry backoff in milliseconds.",
            &["must be <= max_backoff_ms unless both are zero"],
        ),
        meta(
            "routing",
            "routing_resilience.max_backoff_ms",
            ConfigValueKind::U64,
            "Maximum retry backoff in milliseconds.",
            &["must be >= base_backoff_ms unless both are zero"],
        ),
        meta(
            "routing",
            "routing_resilience.respect_retry_after_up_to_seconds",
            ConfigValueKind::U64,
            "Maximum Retry-After seconds respected by routing resilience.",
            &["non-negative integer"],
        ),
        meta(
            "cache",
            "cache",
            ConfigValueKind::Object,
            "Cache configuration group.",
            &["read-only", "restart required"],
        ),
        meta(
            "cache",
            "cache.catalog",
            ConfigValueKind::Object,
            "Catalog cache configuration group.",
            &["read-only", "restart required"],
        ),
        meta(
            "cache",
            "cache.catalog.backend",
            ConfigValueKind::Enum,
            "Catalog cache backend.",
            &["memory or redis", "read-only", "restart required"],
        ),
        meta(
            "cache",
            "cache.catalog.ttl",
            ConfigValueKind::U64,
            "Catalog cache TTL in seconds.",
            &["positive integer", "read-only", "restart required"],
        ),
        meta(
            "cache",
            "cache.catalog.negative_ttl",
            ConfigValueKind::U64,
            "Catalog negative cache TTL in seconds.",
            &["positive integer", "read-only", "restart required"],
        ),
        meta(
            "cache",
            "cache.catalog.redis",
            ConfigValueKind::Object,
            "Catalog Redis cache settings.",
            &["read-only", "restart required"],
        ),
        meta(
            "cache",
            "cache.catalog.redis.key_prefix",
            ConfigValueKind::String,
            "Catalog Redis cache key prefix.",
            &["read-only", "restart required"],
        ),
        meta(
            "runtime_state",
            "runtime_state",
            ConfigValueKind::Object,
            "Runtime state backend configuration group.",
            &["read-only", "restart required"],
        ),
        meta(
            "runtime_state",
            "runtime_state.backend",
            ConfigValueKind::Enum,
            "Runtime state backend.",
            &["memory or redis", "read-only", "restart required"],
        ),
        meta(
            "runtime_state",
            "runtime_state.redis",
            ConfigValueKind::Object,
            "Runtime state Redis settings.",
            &["read-only", "restart required"],
        ),
        meta(
            "runtime_state",
            "runtime_state.redis.key_prefix",
            ConfigValueKind::String,
            "Runtime state Redis key prefix.",
            &["read-only", "restart required"],
        ),
        meta(
            "runtime_state",
            "runtime_state.redis.api_key_concurrency_lease_ttl_seconds",
            ConfigValueKind::U64,
            "API key concurrency lease TTL in seconds.",
            &["positive integer", "read-only", "restart required"],
        ),
        meta(
            "runtime_state",
            "runtime_state.redis.provider_circuit_probe_lease_ttl_seconds",
            ConfigValueKind::U64,
            "Provider circuit probe lease TTL in seconds.",
            &["positive integer", "read-only", "restart required"],
        ),
        meta(
            "runtime_state",
            "runtime_state.redis.state_ttl_seconds",
            ConfigValueKind::U64,
            "Runtime state TTL in seconds.",
            &["positive integer", "read-only", "restart required"],
        ),
        meta(
            "runtime_state",
            "runtime_state.fallback_to_memory",
            ConfigValueKind::Bool,
            "Whether Redis runtime-state failures may fall back to memory.",
            &["boolean", "read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage",
            ConfigValueKind::Object,
            "Request log bundle storage configuration group.",
            &["read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.driver",
            ConfigValueKind::Enum,
            "Storage driver.",
            &["local or s3", "read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.local",
            ConfigValueKind::Object,
            "Local storage settings.",
            &["read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.local.root",
            ConfigValueKind::String,
            "Local storage root path.",
            &["read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.s3",
            ConfigValueKind::NullableObject,
            "Optional S3-compatible storage settings.",
            &["read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.s3.endpoint",
            ConfigValueKind::NullableString,
            "S3-compatible endpoint URL.",
            &["read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.s3.region",
            ConfigValueKind::NullableString,
            "S3 region.",
            &["read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.s3.bucket",
            ConfigValueKind::String,
            "S3 bucket name.",
            &["read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.s3.access_mode",
            ConfigValueKind::Enum,
            "S3 artifact access mode.",
            &["proxy or redirect", "read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.s3.access_key",
            ConfigValueKind::NullableString,
            "S3 access key.",
            &["sensitive", "read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.s3.secret_key",
            ConfigValueKind::NullableString,
            "S3 secret key.",
            &["sensitive", "read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.s3.force_path_style",
            ConfigValueKind::Bool,
            "Whether S3 path-style access is forced.",
            &["boolean", "read-only", "restart required"],
        ),
        meta(
            "storage",
            "storage.s3.public_url",
            ConfigValueKind::NullableString,
            "Optional public URL base for S3 artifacts.",
            &["read-only", "restart required"],
        ),
    ];

    fields.sort_by(|left, right| {
        left.section
            .cmp(&right.section)
            .then_with(|| left.path.cmp(&right.path))
    });
    fields
}

pub fn build_resolved_config_report(
    loaded: &LoadedConfig,
    summary: SystemConfigReportSummary,
) -> ResolvedConfigReport {
    let value = serde_json::to_value(&loaded.config).unwrap_or(Value::Null);
    let effective = redact_config_tree_value("", &value);
    let override_file = build_override_file_report(loaded);
    let fields = config_field_metadata()
        .into_iter()
        .map(|metadata| {
            let raw_value = value_at_path(&value, &metadata.path).unwrap_or(Value::Null);
            let source = source_for_path(loaded, &metadata.path);
            ConfigFieldReport {
                value: redact_config_tree_value(&metadata.path, &raw_value),
                source,
                path: metadata.path,
                section: metadata.section,
                value_kind: metadata.value_kind,
                editable: metadata.editable,
                hot_reloadable: metadata.hot_reloadable,
                restart_required: metadata.restart_required,
                sensitive: metadata.sensitive,
                description: metadata.description,
                constraints: metadata.constraints,
            }
        })
        .collect();

    ResolvedConfigReport {
        summary,
        fields,
        effective,
        override_file,
    }
}

pub fn refresh_resolved_config_file_state(
    report: &mut ResolvedConfigReport,
    loaded: &LoadedConfig,
) {
    report.summary.override_exists = loaded.paths.override_config_path.exists();
    report.summary.history_exists = loaded.paths.override_history_path.exists();
    report.override_file = build_override_file_report(loaded);
}

pub fn metadata_by_path() -> BTreeMap<String, ConfigFieldMetadata> {
    config_field_metadata()
        .into_iter()
        .map(|metadata| (metadata.path.clone(), metadata))
        .collect()
}

pub fn is_ui_write_forbidden_path(path: &str) -> bool {
    UI_WRITE_FORBIDDEN_PATHS.contains(&path)
}

fn meta(
    section: &str,
    path: &str,
    value_kind: ConfigValueKind,
    description: &str,
    constraints: &[&str],
) -> ConfigFieldMetadata {
    let hot_reloadable = OVERRIDE_ALLOWED_PATHS.contains(&path);
    ConfigFieldMetadata {
        path: path.to_string(),
        section: section.to_string(),
        value_kind,
        editable: hot_reloadable,
        hot_reloadable,
        restart_required: !hot_reloadable,
        sensitive: is_sensitive_config_path(path),
        description: description.to_string(),
        constraints: constraints
            .iter()
            .map(|constraint| constraint.to_string())
            .collect(),
    }
}

fn source_for_path(loaded: &LoadedConfig, path: &str) -> ConfigFieldSourceReport {
    let source = loaded
        .source_report
        .resolve_field_source(path)
        .or_else(|| nearest_parent_source(loaded, path));

    source
        .map(source_report_from_source)
        .unwrap_or_else(default_source_report)
}

fn build_override_file_report(loaded: &LoadedConfig) -> OverrideFileReport {
    let path = &loaded.paths.override_config_path;
    let path_display = path.display().to_string();
    let exists = path.exists();
    if !exists {
        return OverrideFileReport {
            path: path_display,
            exists: false,
            yaml: String::new(),
            invalid_paths: Vec::new(),
            last_modified_ms: None,
        };
    }

    let last_modified_ms = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_to_millis);

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            return OverrideFileReport {
                path: path_display,
                exists,
                yaml: String::new(),
                invalid_paths: vec![format!("read_error: {err}")],
                last_modified_ms,
            };
        }
    };

    if content.trim().is_empty() {
        return OverrideFileReport {
            path: path_display,
            exists,
            yaml: String::new(),
            invalid_paths: Vec::new(),
            last_modified_ms,
        };
    }

    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(value) => value,
        Err(err) => {
            return OverrideFileReport {
                path: path_display,
                exists,
                yaml: String::new(),
                invalid_paths: vec![format!("parse_error: {err}")],
                last_modified_ms,
            };
        }
    };

    if let Err(invalid_paths) = override_policy::validate_override_document(&yaml_value) {
        return OverrideFileReport {
            path: path_display,
            exists,
            yaml: String::new(),
            invalid_paths,
            last_modified_ms,
        };
    }

    let json_value = match serde_json::to_value(yaml_value)
        .ok()
        .and_then(|value| normalize_override_document(value).ok())
    {
        Some(value) => value,
        None => {
            return OverrideFileReport {
                path: path_display,
                exists,
                yaml: String::new(),
                invalid_paths: vec!["invalid_root".to_string()],
                last_modified_ms,
            };
        }
    };
    let redacted = redact_config_tree_value("", &json_value);
    let yaml = override_document_to_yaml(&redacted)
        .unwrap_or_else(|err| format!("# failed to serialize redacted override view: {err}\n"));

    OverrideFileReport {
        path: path_display,
        exists,
        yaml,
        invalid_paths: Vec::new(),
        last_modified_ms,
    }
}

fn system_time_to_millis(value: SystemTime) -> Option<i64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

fn nearest_parent_source<'a>(
    loaded: &'a LoadedConfig,
    path: &str,
) -> Option<&'a ConfigFieldSource> {
    let mut candidate = path;
    while let Some((parent, _)) = candidate.rsplit_once('.') {
        if let Some(source) = loaded.source_report.resolve_field_source(parent) {
            return Some(source);
        }
        candidate = parent;
    }

    None
}

fn source_report_from_source(source: &ConfigFieldSource) -> ConfigFieldSourceReport {
    ConfigFieldSourceReport {
        kind: source.kind,
        source_name: source.source_name.clone(),
        configured: source.configured,
        warnings: source.warnings.clone(),
    }
}

fn default_source_report() -> ConfigFieldSourceReport {
    ConfigFieldSourceReport {
        kind: ConfigLayerKind::ProgramDefault,
        source_name: "program defaults".to_string(),
        configured: false,
        warnings: Vec::new(),
    }
}

fn value_at_path(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.as_object()?.get(segment)?;
    }
    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs};

    use crate::config::{
        loader::{ConfigLoadOptions, load_effective_config},
        override_policy::OVERRIDE_ALLOWED_PATHS,
        paths::ConfigPaths,
        programmatic_default_config,
        source::flatten_json_paths,
    };

    use super::{
        SystemConfigReportSummary, build_resolved_config_report, config_field_metadata,
        is_ui_write_forbidden_path, metadata_by_path,
    };

    fn load_without_environment(paths: &ConfigPaths) -> crate::config::loader::LoadedConfig {
        load_effective_config(
            paths,
            ConfigLoadOptions {
                include_environment: false,
                include_override: true,
            },
        )
        .expect("config should load")
    }

    fn test_summary(paths: &ConfigPaths) -> SystemConfigReportSummary {
        SystemConfigReportSummary {
            version: 1,
            loaded_at: 0,
            last_error: None,
            override_path: paths.override_config_path.display().to_string(),
            override_exists: paths.override_config_path.exists(),
            history_path: paths.override_history_path.display().to_string(),
            history_exists: paths.override_history_path.exists(),
            deployment_mode: "single_instance".to_string(),
        }
    }

    #[test]
    fn metadata_covers_final_config_paths() {
        let metadata = metadata_by_path();
        let value =
            serde_json::to_value(programmatic_default_config()).expect("config should serialize");
        let config_paths = flatten_json_paths(&value);

        for path in config_paths.keys() {
            assert!(
                metadata.contains_key(path),
                "missing metadata for config path {path}"
            );
        }

        for path in [
            "storage.s3.bucket",
            "storage.s3.secret_key",
            "redis.url",
            "cache.catalog.redis.key_prefix",
        ] {
            assert!(
                metadata.contains_key(path),
                "missing metadata for optional config path {path}"
            );
        }
    }

    #[test]
    fn metadata_paths_are_unique_and_sorted_by_section_then_path() {
        let metadata = config_field_metadata();
        let mut seen = BTreeSet::new();
        let mut sorted = metadata.clone();
        sorted.sort_by(|left, right| {
            left.section
                .cmp(&right.section)
                .then_with(|| left.path.cmp(&right.path))
        });

        assert_eq!(metadata, sorted);
        for field in metadata {
            assert!(
                seen.insert(field.path.clone()),
                "duplicate metadata path {}",
                field.path
            );
        }
    }

    #[test]
    fn resolved_report_redacts_sensitive_values() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        fs::write(
            temp_dir.path().join("config.yaml"),
            r#"
secret_key: manager-super-secret
password_salt: password-salt-secret
jwt_secret: jwt-secret-value
api_key_jwt_secret: api-key-jwt-secret-value
db_url: postgres://db-user:db-password-secret@localhost:5432/cyder
proxy: http://proxy-user:proxy-password-secret@localhost:8080
redis:
  url: redis://:redis-password-secret@localhost:6379/2
  pool_size: 4
  key_prefix: "cyder-test:"
storage:
  driver: s3
  s3:
    endpoint: http://localhost:9000
    region: us-east-1
    bucket: bundles
    access_mode: proxy
    access_key: s3-access-secret
    secret_key: s3-secret-secret
    force_path_style: true
    public_url: null
"#,
        )
        .expect("user config should be written");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_without_environment(&paths);
        let report = build_resolved_config_report(&loaded, test_summary(&paths));
        let serialized = serde_json::to_string(&report).expect("report should serialize");

        for secret in [
            "manager-super-secret",
            "password-salt-secret",
            "jwt-secret-value",
            "api-key-jwt-secret-value",
            "db-password-secret",
            "proxy-user",
            "proxy-password-secret",
            "redis-password-secret",
            "s3-access-secret",
            "s3-secret-secret",
        ] {
            assert!(
                !serialized.contains(secret),
                "serialized report leaked secret value {secret}: {serialized}"
            );
        }
    }

    #[test]
    fn resolved_report_effective_tree_redacts_sensitive_values() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        fs::write(
            temp_dir.path().join("config.yaml"),
            r#"
secret_key: manager-super-secret
proxy: http://proxy-user:proxy-password-secret@localhost:8080
"#,
        )
        .expect("user config should be written");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_without_environment(&paths);
        let report = build_resolved_config_report(&loaded, test_summary(&paths));
        let serialized = serde_json::to_string(&report.effective).expect("effective serializes");

        assert!(!serialized.contains("manager-super-secret"));
        assert!(!serialized.contains("proxy-user"));
        assert!(!serialized.contains("proxy-password-secret"));
        assert_eq!(
            report.effective["secret_key"]["redacted"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn override_file_report_contains_only_current_override_document() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        fs::write(
            temp_dir.path().join("config.override.yaml"),
            "log_level: debug\n",
        )
        .expect("override config should be written");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let loaded = load_without_environment(&paths);
        let report = build_resolved_config_report(&loaded, test_summary(&paths));

        assert!(report.override_file.exists);
        assert!(report.override_file.invalid_paths.is_empty());
        assert!(report.override_file.yaml.contains("log_level: debug"));
        assert!(!report.override_file.yaml.contains("secret_key"));
        assert!(!report.override_file.yaml.contains("db_url"));
    }

    #[test]
    fn override_whitelist_fields_are_editable_and_hot_reloadable() {
        let metadata = metadata_by_path();

        for path in OVERRIDE_ALLOWED_PATHS {
            let field = metadata
                .get(*path)
                .unwrap_or_else(|| panic!("metadata missing override path {path}"));
            assert!(field.editable, "{path} should be editable");
            assert!(field.hot_reloadable, "{path} should be hot reloadable");
            assert!(!field.restart_required, "{path} should not require restart");
        }
    }

    #[test]
    fn storage_s3_bucket_is_readonly_restart_configuration() {
        let metadata = metadata_by_path();
        let field = metadata
            .get("storage.s3.bucket")
            .expect("storage bucket metadata should exist");

        assert!(!field.editable);
        assert!(!field.hot_reloadable);
        assert!(field.restart_required);
        assert!(is_ui_write_forbidden_path("storage.s3.bucket"));
    }
}
