use std::sync::Arc;

use chrono::Utc;
use log::LevelFilter;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};

use crate::{
    config::{
        DeploymentMode,
        loader::{
            ConfigLoadError, ConfigLoadOptions, LoadedConfig, load_effective_config,
            load_effective_config_with_override_document,
        },
        paths::ConfigPaths,
    },
    logging,
    service::{
        diagnostics::{DiagnosticsPolicy, DiagnosticsPolicyManager},
        infra::{HttpClientBundle, HttpClientManager},
        runtime::ProviderGovernanceConfigManager,
    },
};

use super::{
    history::{
        SystemConfigHistoryError, append_history_item, build_history_item, read_history_items,
    },
    metadata::{
        build_resolved_config_report, metadata_by_path, refresh_resolved_config_file_state,
    },
    override_file::{
        OverrideFileError, read_override_document, remove_override_paths, set_override_paths,
        write_override_document_atomic,
    },
    override_model::set_override_path as set_json_path,
    runtime::RuntimeConfigSnapshot,
    types::{
        ResolvedConfigReport, SystemConfigChangeRequest, SystemConfigDiffItem,
        SystemConfigHistoryItem, SystemConfigHistoryOperation, SystemConfigPreviewResponse,
        SystemConfigReportSummary, SystemConfigValidationIssue, SystemConfigValidationReport,
    },
    validation::{
        SystemConfigValidationError, no_effective_change_error, preview_override_changes,
        validate_effective_runtime_config, validate_non_empty_apply_changes,
        validate_non_empty_reset_paths, validate_required_reason,
    },
};

const INITIAL_CONFIG_VERSION: u64 = 1;
const DEFAULT_HISTORY_LIMIT: usize = 100;
const MAX_HISTORY_LIMIT: usize = 500;
const MULTI_INSTANCE_WRITE_DISABLED_REASON: &str = "multi_instance_not_supported";

#[derive(Debug, Error)]
pub enum SystemConfigServiceError {
    #[error(transparent)]
    Validation(#[from] SystemConfigValidationError),
    #[error(transparent)]
    OverrideFile(#[from] OverrideFileError),
    #[error(transparent)]
    History(#[from] SystemConfigHistoryError),
    #[error(transparent)]
    ConfigLoad(#[from] ConfigLoadError),
    #[error(
        "multi_instance_not_supported: local override writes and reloads are disabled in multi_instance mode"
    )]
    MultiInstanceUnsupported,
    #[error("failed to apply runtime system config: {0}")]
    RuntimeApply(String),
}

#[derive(Clone)]
struct SystemConfigState {
    loaded: LoadedConfig,
    report: ResolvedConfigReport,
    snapshot: RuntimeConfigSnapshot,
    version: u64,
    loaded_at: i64,
    last_error: Option<String>,
}

pub struct SystemConfigService {
    paths: ConfigPaths,
    load_options: ConfigLoadOptions,
    state: RwLock<SystemConfigState>,
    mutation_lock: Mutex<()>,
    http_client_manager: RwLock<Option<Arc<HttpClientManager>>>,
    provider_governance_config_manager: RwLock<Option<Arc<ProviderGovernanceConfigManager>>>,
    diagnostics_policy_manager: RwLock<Option<Arc<DiagnosticsPolicyManager>>>,
    #[cfg(test)]
    forced_http_client_preflight_error: RwLock<Option<String>>,
}

impl SystemConfigService {
    pub fn new(loaded: LoadedConfig, load_options: ConfigLoadOptions) -> Self {
        let version = INITIAL_CONFIG_VERSION;
        let paths = loaded.paths.clone();
        let state = build_state(loaded, version, None);
        Self {
            paths,
            load_options,
            state: RwLock::new(state),
            mutation_lock: Mutex::new(()),
            http_client_manager: RwLock::new(None),
            provider_governance_config_manager: RwLock::new(None),
            diagnostics_policy_manager: RwLock::new(None),
            #[cfg(test)]
            forced_http_client_preflight_error: RwLock::new(None),
        }
    }

    pub fn new_with_default_options(loaded: LoadedConfig) -> Self {
        Self::new(loaded, ConfigLoadOptions::default())
    }

    pub fn paths(&self) -> &ConfigPaths {
        &self.paths
    }

    pub async fn version(&self) -> u64 {
        self.state.read().await.version
    }

    pub async fn loaded_at(&self) -> i64 {
        self.state.read().await.loaded_at
    }

    pub async fn last_error(&self) -> Option<String> {
        self.state.read().await.last_error.clone()
    }

    pub async fn runtime_snapshot(&self) -> RuntimeConfigSnapshot {
        self.state.read().await.snapshot.clone()
    }

    pub async fn report(&self) -> ResolvedConfigReport {
        let (loaded, mut report, version, loaded_at, last_error) = {
            let state = self.state.read().await;
            (
                state.loaded.clone(),
                state.report.clone(),
                state.version,
                state.loaded_at,
                state.last_error.clone(),
            )
        };
        report.summary.version = version;
        report.summary.loaded_at = loaded_at;
        report.summary.last_error = last_error;
        refresh_resolved_config_file_state(&mut report, &loaded);
        report
    }

    pub async fn register_http_client_manager(&self, manager: Arc<HttpClientManager>) {
        *self.http_client_manager.write().await = Some(manager);
    }

    pub async fn register_provider_governance_config_manager(
        &self,
        manager: Arc<ProviderGovernanceConfigManager>,
    ) {
        *self.provider_governance_config_manager.write().await = Some(manager);
    }

    pub async fn register_diagnostics_policy_manager(
        &self,
        manager: Arc<DiagnosticsPolicyManager>,
    ) {
        *self.diagnostics_policy_manager.write().await = Some(manager);
    }

    #[cfg(test)]
    async fn force_next_http_client_preflight_error(&self, message: impl Into<String>) {
        *self.forced_http_client_preflight_error.write().await = Some(message.into());
    }

    pub async fn preview_changes(
        &self,
        request: &SystemConfigChangeRequest,
    ) -> Result<SystemConfigPreviewResponse, SystemConfigServiceError> {
        let state = self.state.read().await;
        let mut preview = preview_override_changes(&state.loaded, request)?;
        if state.loaded.config.deployment.mode == DeploymentMode::MultiInstance {
            preview.write_disabled_reason = Some(MULTI_INSTANCE_WRITE_DISABLED_REASON.to_string());
        }
        Ok(preview)
    }

    pub async fn apply_changes(
        &self,
        request: SystemConfigChangeRequest,
    ) -> Result<ResolvedConfigReport, SystemConfigServiceError> {
        match self.apply_changes_inner(request).await {
            Ok(report) => {
                self.clear_last_error().await;
                Ok(report)
            }
            Err(err) => {
                self.record_error(err.to_string()).await;
                Err(err)
            }
        }
    }

    pub async fn reset_paths(
        &self,
        paths: Vec<String>,
        reason: Option<String>,
    ) -> Result<ResolvedConfigReport, SystemConfigServiceError> {
        match self.reset_paths_inner(paths, reason).await {
            Ok(report) => {
                self.clear_last_error().await;
                Ok(report)
            }
            Err(err) => {
                self.record_error(err.to_string()).await;
                Err(err)
            }
        }
    }

    pub async fn reload_override_file(
        &self,
    ) -> Result<ResolvedConfigReport, SystemConfigServiceError> {
        match self.reload_override_file_inner().await {
            Ok(report) => {
                self.clear_last_error().await;
                Ok(report)
            }
            Err(err) => {
                self.record_error(err.to_string()).await;
                Err(err)
            }
        }
    }

    pub fn history(
        &self,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<SystemConfigHistoryItem>, SystemConfigServiceError> {
        let limit = limit
            .unwrap_or(DEFAULT_HISTORY_LIMIT)
            .clamp(1, MAX_HISTORY_LIMIT);
        let offset = offset.unwrap_or_default();
        read_history_items(&self.paths.override_history_path, limit, offset)
            .map_err(SystemConfigServiceError::from)
    }

    async fn apply_changes_inner(
        &self,
        request: SystemConfigChangeRequest,
    ) -> Result<ResolvedConfigReport, SystemConfigServiceError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        self.ensure_single_instance().await?;
        validate_non_empty_apply_changes(&request)?;
        let reason = validate_required_reason("apply", request.reason.as_deref())?;
        let current_state = self.state.read().await.clone();
        let preview = preview_override_changes(&current_state.loaded, &request)?;
        if preview.diff.is_empty() {
            let path = request
                .changes
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "<config>".to_string());
            return Err(no_effective_change_error(path).into());
        }
        let version_before = current_state.version;
        let version_after = version_before + 1;
        let old_snapshot = current_state.snapshot.clone();
        let next_snapshot = next_snapshot_from_changes(&old_snapshot, version_after, &request)?;
        let prepared_runtime = self
            .prepare_runtime_snapshot_effects(&old_snapshot, &next_snapshot, &preview.diff)
            .await?;

        let mut document = read_override_document(&self.paths.override_config_path)?;
        set_override_paths(&mut document, &request.changes)?;
        write_override_document_atomic(&self.paths.override_config_path, &document)?;

        let loaded = self.reload_loaded_config()?;
        let mut next_state = build_state(loaded, version_after, None);
        let diff = preview.diff;
        let history = build_history_item(
            SystemConfigHistoryOperation::Apply,
            Some(reason),
            version_before,
            version_after,
            diff.clone(),
        );
        append_history_item(&self.paths.override_history_path, &history)?;
        self.commit_runtime_snapshot_effects(
            &old_snapshot,
            &next_state.snapshot,
            &diff,
            prepared_runtime,
        )
        .await;

        refresh_resolved_config_file_state(&mut next_state.report, &next_state.loaded);
        let report = next_state.report.clone();
        *self.state.write().await = next_state;
        Ok(report)
    }

    async fn reset_paths_inner(
        &self,
        paths: Vec<String>,
        reason: Option<String>,
    ) -> Result<ResolvedConfigReport, SystemConfigServiceError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        self.ensure_single_instance().await?;
        validate_non_empty_reset_paths(&paths)?;
        let reason = validate_required_reason("reset", reason.as_deref())?;
        validate_writable_paths(&paths)?;
        let current_state = self.state.read().await.clone();
        let version_before = current_state.version;
        let version_after = version_before + 1;
        let old_loaded = current_state.loaded.clone();

        let mut document = read_override_document(&self.paths.override_config_path)?;
        let previous_document = document.clone();
        remove_override_paths(&mut document, &paths)?;
        if document == previous_document {
            let path = paths
                .first()
                .cloned()
                .unwrap_or_else(|| "<config>".to_string());
            return Err(no_effective_change_error(path).into());
        }

        let loaded = self.reload_loaded_config_with_override_document(&document)?;
        validate_effective_runtime_config(&loaded.config)?;
        let diff = diff_for_paths(&old_loaded, &loaded, &paths);
        let next_snapshot = RuntimeConfigSnapshot::from_config(version_after, &loaded.config);
        let old_snapshot = current_state.snapshot.clone();
        let prepared_runtime = self
            .prepare_runtime_snapshot_effects(&old_snapshot, &next_snapshot, &diff)
            .await?;

        write_override_document_atomic(&self.paths.override_config_path, &document)?;
        let mut next_state = build_state(loaded, version_after, None);
        let mut history = build_history_item(
            SystemConfigHistoryOperation::Reset,
            Some(reason),
            version_before,
            version_after,
            diff.clone(),
        );
        if history.changed_paths.is_empty() {
            history.changed_paths = paths.clone();
        }
        append_history_item(&self.paths.override_history_path, &history)?;
        self.commit_runtime_snapshot_effects(
            &old_snapshot,
            &next_state.snapshot,
            &diff,
            prepared_runtime,
        )
        .await;

        refresh_resolved_config_file_state(&mut next_state.report, &next_state.loaded);
        let report = next_state.report.clone();
        *self.state.write().await = next_state;
        Ok(report)
    }

    async fn reload_override_file_inner(
        &self,
    ) -> Result<ResolvedConfigReport, SystemConfigServiceError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        self.ensure_single_instance().await?;
        let current_state = self.state.read().await.clone();
        let version_before = current_state.version;
        let version_after = version_before + 1;
        let old_loaded = current_state.loaded.clone();
        let loaded = self.reload_loaded_config()?;
        let diff = diff_all_metadata_paths(&old_loaded, &loaded);
        let mut next_state = build_state(loaded, version_after, None);
        let old_snapshot = RuntimeConfigSnapshot::from_config(version_before, &old_loaded.config);
        let prepared_runtime = self
            .prepare_runtime_snapshot_effects(&old_snapshot, &next_state.snapshot, &diff)
            .await?;
        let history = build_history_item(
            SystemConfigHistoryOperation::Reload,
            Some("manual override reload".to_string()),
            version_before,
            version_after,
            diff.clone(),
        );
        append_history_item(&self.paths.override_history_path, &history)?;
        self.commit_runtime_snapshot_effects(
            &old_snapshot,
            &next_state.snapshot,
            &diff,
            prepared_runtime,
        )
        .await;

        refresh_resolved_config_file_state(&mut next_state.report, &next_state.loaded);
        let report = next_state.report.clone();
        *self.state.write().await = next_state;
        Ok(report)
    }

    fn reload_loaded_config(&self) -> Result<LoadedConfig, ConfigLoadError> {
        load_effective_config(&self.paths, self.load_options)
    }

    fn reload_loaded_config_with_override_document(
        &self,
        document: &Value,
    ) -> Result<LoadedConfig, ConfigLoadError> {
        load_effective_config_with_override_document(&self.paths, self.load_options, document)
    }

    async fn ensure_single_instance(&self) -> Result<(), SystemConfigServiceError> {
        let state = self.state.read().await;
        if state.loaded.config.deployment.mode == DeploymentMode::MultiInstance {
            Err(SystemConfigServiceError::MultiInstanceUnsupported)
        } else {
            Ok(())
        }
    }

    async fn record_error(&self, error: String) {
        let mut state = self.state.write().await;
        state.last_error = Some(error.clone());
        state.report.summary.last_error = Some(error);
    }

    async fn clear_last_error(&self) {
        let mut state = self.state.write().await;
        state.last_error = None;
        state.report.summary.last_error = None;
    }

    async fn prepare_runtime_snapshot_effects(
        &self,
        before: &RuntimeConfigSnapshot,
        after: &RuntimeConfigSnapshot,
        diff: &[SystemConfigDiffItem],
    ) -> Result<PreparedRuntimeEffects, SystemConfigServiceError> {
        let http_bundle = if http_client_config_changed(diff) {
            #[cfg(test)]
            if let Some(message) = self.forced_http_client_preflight_error.write().await.take() {
                return Err(SystemConfigServiceError::RuntimeApply(message));
            }

            let manager = self.http_client_manager.read().await.clone();
            if manager.is_some() {
                Some(
                    HttpClientManager::build_bundle(
                        after.version,
                        after.proxy_request.clone(),
                        after.proxy.clone(),
                    )
                    .map_err(SystemConfigServiceError::RuntimeApply)?,
                )
            } else {
                None
            }
        } else {
            None
        };

        let log_level = if before.log_level != after.log_level {
            Some(
                logging::parse_level(&after.log_level)
                    .map_err(SystemConfigServiceError::RuntimeApply)?,
            )
        } else {
            None
        };

        let provider_governance =
            provider_governance_config_changed(diff).then(|| after.provider_governance.clone());
        let diagnostics_policy = diagnostics_policy_changed(diff)
            .then(|| DiagnosticsPolicy::from_config(&after.diagnostics));

        Ok(PreparedRuntimeEffects {
            http_bundle,
            log_level,
            provider_governance,
            diagnostics_policy,
        })
    }

    async fn commit_runtime_snapshot_effects(
        &self,
        before: &RuntimeConfigSnapshot,
        after: &RuntimeConfigSnapshot,
        diff: &[SystemConfigDiffItem],
        prepared: PreparedRuntimeEffects,
    ) {
        if let Some(level) = prepared.log_level {
            logging::set_level_filter(level);
        }

        if let Some(bundle) = prepared.http_bundle {
            if let Some(manager) = self.http_client_manager.read().await.clone() {
                let old_bundle = manager.current().await;
                let old_version = old_bundle.version;
                let proxy_enabled = bundle.proxy.is_some();
                let connect_timeout_seconds = bundle.proxy_request.connect_timeout_seconds;
                let first_byte_timeout_seconds = bundle.proxy_request.first_byte_timeout_seconds;
                let total_timeout_seconds = bundle.proxy_request.total_timeout_seconds;
                let new_version = bundle.version;
                manager.replace_bundle(bundle).await;
                crate::info_event!(
                    "manager.system_config_http_client_rebuilt",
                    old_version = old_version,
                    new_version = new_version,
                    proxy_enabled = proxy_enabled,
                    connect_timeout_seconds = connect_timeout_seconds,
                    first_byte_timeout_seconds = first_byte_timeout_seconds,
                    total_timeout_seconds = total_timeout_seconds,
                );
            }
        }

        if let Some(provider_governance) = prepared.provider_governance {
            if let Some(manager) = self.provider_governance_config_manager.read().await.clone() {
                let old_config = manager.current().await;
                manager.update(provider_governance).await;
                crate::info_event!(
                    "manager.system_config_provider_governance_updated",
                    old_enabled = old_config.enabled,
                    new_enabled = after.provider_governance.enabled,
                    old_consecutive_failure_threshold = old_config.consecutive_failure_threshold,
                    new_consecutive_failure_threshold =
                        after.provider_governance.consecutive_failure_threshold,
                    old_open_cooldown_seconds = old_config.open_cooldown_seconds,
                    new_open_cooldown_seconds = after.provider_governance.open_cooldown_seconds,
                );
            }
        }

        if let Some(new_policy) = prepared.diagnostics_policy {
            if let Some(manager) = self.diagnostics_policy_manager.read().await.clone() {
                let old_policy = manager.current().await;
                manager.update(new_policy.clone()).await;
                crate::info_event!(
                    "manager.system_config_diagnostics_policy_updated",
                    old_response_capture_max_bytes = old_policy.response_capture_max_bytes(),
                    new_response_capture_max_bytes = new_policy.response_capture_max_bytes(),
                    old_raw_bundle_download_enabled = old_policy.raw_bundle_download_enabled(),
                    new_raw_bundle_download_enabled = new_policy.raw_bundle_download_enabled(),
                    old_retention_enabled = old_policy.retention_enabled(),
                    new_retention_enabled = new_policy.retention_enabled(),
                );
            }
        }

        if !diff.is_empty() {
            let changed_paths = diff
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>()
                .join(",");
            crate::info_event!(
                "manager.system_config_runtime_applied",
                version_before = before.version,
                version_after = after.version,
                changed_paths = &changed_paths,
            );
        }
    }
}

struct PreparedRuntimeEffects {
    http_bundle: Option<HttpClientBundle>,
    log_level: Option<LevelFilter>,
    provider_governance: Option<crate::config::ProviderGovernanceConfig>,
    diagnostics_policy: Option<DiagnosticsPolicy>,
}

fn build_state(
    loaded: LoadedConfig,
    version: u64,
    last_error: Option<String>,
) -> SystemConfigState {
    let loaded_at = Utc::now().timestamp_millis();
    let report = build_resolved_config_report(
        &loaded,
        SystemConfigReportSummary {
            version,
            loaded_at,
            last_error: last_error.clone(),
            override_path: loaded.paths.override_config_path.display().to_string(),
            override_exists: loaded.paths.override_config_path.exists(),
            history_path: loaded.paths.override_history_path.display().to_string(),
            history_exists: loaded.paths.override_history_path.exists(),
            deployment_mode: loaded.config.deployment.mode.as_str().to_string(),
        },
    );
    let snapshot = RuntimeConfigSnapshot::from_config(version, &loaded.config);
    SystemConfigState {
        loaded,
        report,
        snapshot,
        version,
        loaded_at,
        last_error,
    }
}

fn validate_writable_paths(paths: &[String]) -> Result<(), SystemConfigValidationError> {
    let metadata = metadata_by_path();
    let mut errors = Vec::new();
    for path in paths {
        match metadata.get(path) {
            Some(field) if field.editable && field.hot_reloadable => {}
            Some(_) => errors.push(SystemConfigValidationIssue {
                path: path.clone(),
                code: "readonly_path".to_string(),
                message: format!(
                    "configuration path '{path}' is read-only and cannot be written by the UI"
                ),
            }),
            None => errors.push(SystemConfigValidationIssue {
                path: path.clone(),
                code: "unknown_path".to_string(),
                message: format!("configuration path '{path}' is not known"),
            }),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(SystemConfigValidationError {
            validation: SystemConfigValidationReport::invalid(errors),
        })
    }
}

fn diff_all_metadata_paths(
    before: &LoadedConfig,
    after: &LoadedConfig,
) -> Vec<SystemConfigDiffItem> {
    let paths = metadata_by_path().into_keys().collect::<Vec<_>>();
    diff_for_paths(before, after, &paths)
}

fn diff_for_paths(
    before: &LoadedConfig,
    after: &LoadedConfig,
    paths: &[String],
) -> Vec<SystemConfigDiffItem> {
    let before_value = serde_json::to_value(&before.config).unwrap_or(Value::Null);
    let after_value = serde_json::to_value(&after.config).unwrap_or(Value::Null);
    paths
        .iter()
        .filter_map(|path| {
            let old_value = value_at_path(&before_value, path).unwrap_or(Value::Null);
            let new_value = value_at_path(&after_value, path).unwrap_or(Value::Null);
            (old_value != new_value).then(|| SystemConfigDiffItem {
                path: path.clone(),
                old_value,
                new_value,
            })
        })
        .collect()
}

fn value_at_path(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.as_object()?.get(segment)?;
    }
    Some(current.clone())
}

fn next_snapshot_from_changes(
    before: &RuntimeConfigSnapshot,
    version_after: u64,
    request: &SystemConfigChangeRequest,
) -> Result<RuntimeConfigSnapshot, SystemConfigServiceError> {
    let mut value = serde_json::to_value(before).map_err(|err| {
        SystemConfigServiceError::RuntimeApply(format!(
            "failed to serialize runtime snapshot for preflight: {err}"
        ))
    })?;
    for (path, change) in &request.changes {
        set_json_path(&mut value, path, change.clone()).map_err(OverrideFileError::from)?;
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("version".to_string(), Value::from(version_after));
    }
    serde_json::from_value(value).map_err(|err| {
        SystemConfigServiceError::RuntimeApply(format!(
            "failed to build runtime snapshot for preflight: {err}"
        ))
    })
}

fn http_client_config_changed(diff: &[SystemConfigDiffItem]) -> bool {
    diff.iter()
        .any(|item| item.path == "proxy" || item.path.starts_with("proxy_request."))
}

fn provider_governance_config_changed(diff: &[SystemConfigDiffItem]) -> bool {
    diff.iter()
        .any(|item| item.path.starts_with("provider_governance."))
}

fn diagnostics_policy_changed(diff: &[SystemConfigDiffItem]) -> bool {
    diff.iter()
        .any(|item| item.path.starts_with("diagnostics."))
}

pub type SharedSystemConfigService = Arc<SystemConfigService>;

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc, sync::Mutex};

    use log::LevelFilter;
    use serde_json::json;

    use crate::config::{
        ProxyRequestConfig,
        loader::{ConfigLoadOptions, load_effective_config},
        paths::ConfigPaths,
    };
    use crate::service::diagnostics::{DiagnosticsPolicy, DiagnosticsPolicyManager};
    use crate::service::infra::HttpClientManager;
    use crate::service::runtime::ProviderGovernanceConfigManager;

    use super::*;

    static SERVICE_LOG_LEVEL_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn load_temp_config(paths: &ConfigPaths) -> LoadedConfig {
        load_effective_config(
            paths,
            ConfigLoadOptions {
                include_environment: false,
                include_override: true,
            },
        )
        .expect("config should load")
    }

    fn service_for_paths(paths: &ConfigPaths) -> SystemConfigService {
        SystemConfigService::new(
            load_temp_config(paths),
            ConfigLoadOptions {
                include_environment: false,
                include_override: true,
            },
        )
    }

    fn write_test_config(path: &Path, yaml: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("config parent should be created");
        }
        fs::write(path, yaml).expect("config file should be written");
    }

    #[tokio::test]
    async fn apply_success_increments_version_and_snapshot() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);

        let report = service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("max_body_size".to_string(), json!(2 * 1024 * 1024))]
                    .into_iter()
                    .collect(),
                reason: Some("raise body limit".to_string()),
            })
            .await
            .expect("apply should succeed");

        assert_eq!(service.version().await, 2);
        assert_eq!(
            service.runtime_snapshot().await.max_body_size,
            2 * 1024 * 1024
        );
        assert!(paths.override_config_path.exists());
        assert!(report.fields.iter().any(|field| {
            field.path == "max_body_size" && field.value == json!(2 * 1024 * 1024)
        }));
    }

    #[tokio::test]
    async fn apply_returned_report_reflects_new_history_file_state() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        assert!(!paths.override_history_path.exists());

        let report = service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("max_body_size".to_string(), json!(2 * 1024 * 1024))]
                    .into_iter()
                    .collect(),
                reason: Some("raise body limit".to_string()),
            })
            .await
            .expect("apply should succeed");

        assert!(report.summary.history_exists);
        let history = report
            .persistence_health
            .items
            .iter()
            .find(|item| item.key == "override_history")
            .expect("history health item should exist");
        assert!(history.exists);
        assert!(history.readable);
        assert!(history.writable);

        let refreshed = service.report().await;
        assert!(refreshed.summary.history_exists);
    }

    #[tokio::test]
    async fn failed_apply_keeps_version_and_snapshot() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let before = service.runtime_snapshot().await;

        let err = service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("db_url".to_string(), json!("postgres://example"))]
                    .into_iter()
                    .collect(),
                reason: Some("bad change".to_string()),
            })
            .await
            .expect_err("readonly apply should fail");

        assert!(err.to_string().contains("db_url"));
        assert_eq!(service.version().await, 1);
        assert_eq!(service.runtime_snapshot().await, before);
    }

    #[tokio::test]
    async fn apply_requires_non_empty_reason_before_file_changes() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);

        for reason in [None, Some(String::new()), Some("   ".to_string())] {
            let err = service
                .apply_changes(SystemConfigChangeRequest {
                    changes: [("max_body_size".to_string(), json!(2 * 1024 * 1024))]
                        .into_iter()
                        .collect(),
                    reason,
                })
                .await
                .expect_err("missing reason should fail");

            assert!(err.to_string().contains("reason"));
            assert!(err.to_string().contains("required_reason"));
        }

        assert_eq!(service.version().await, 1);
        assert!(!paths.override_config_path.exists());
        assert!(!paths.override_history_path.exists());
    }

    #[tokio::test]
    async fn apply_rejects_empty_changes_without_files_or_history() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);

        let err = service
            .apply_changes(SystemConfigChangeRequest {
                changes: Default::default(),
                reason: Some("empty change should fail".to_string()),
            })
            .await
            .expect_err("empty apply should fail");

        assert!(err.to_string().contains("empty_changes"));
        assert_eq!(service.version().await, 1);
        assert!(!paths.override_config_path.exists());
        assert!(!paths.override_history_path.exists());
    }

    #[tokio::test]
    async fn apply_rejects_same_effective_value_without_version_increment() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let before = service.runtime_snapshot().await;

        let err = service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("log_level".to_string(), json!("info"))]
                    .into_iter()
                    .collect(),
                reason: Some("same value should fail".to_string()),
            })
            .await
            .expect_err("same effective value should fail");

        assert!(err.to_string().contains("no_effective_change"));
        assert_eq!(service.version().await, 1);
        assert_eq!(service.runtime_snapshot().await, before);
        assert!(!paths.override_config_path.exists());
        assert!(!paths.override_history_path.exists());
    }

    #[tokio::test]
    async fn reset_paths_returns_field_to_base_config_value() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        write_test_config(&paths.user_config_path, "max_body_size: 1048576\n");
        let service = service_for_paths(&paths);

        service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("max_body_size".to_string(), json!(2 * 1024 * 1024))]
                    .into_iter()
                    .collect(),
                reason: Some("raise body limit".to_string()),
            })
            .await
            .expect("apply should succeed");
        service
            .reset_paths(
                vec!["max_body_size".to_string()],
                Some("restore base".to_string()),
            )
            .await
            .expect("reset should succeed");

        assert_eq!(service.version().await, 3);
        assert_eq!(service.runtime_snapshot().await.max_body_size, 1024 * 1024);
        let document = read_override_document(&paths.override_config_path)
            .expect("override should remain valid");
        assert_eq!(document, json!({}));
    }

    #[tokio::test]
    async fn reset_requires_reason_and_leaves_override_unchanged() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("max_body_size".to_string(), json!(2 * 1024 * 1024))]
                    .into_iter()
                    .collect(),
                reason: Some("raise body limit".to_string()),
            })
            .await
            .expect("apply should succeed");
        let before_override =
            fs::read_to_string(&paths.override_config_path).expect("override should exist");
        let before_history =
            fs::read_to_string(&paths.override_history_path).expect("history should exist");

        let err = service
            .reset_paths(vec!["max_body_size".to_string()], None)
            .await
            .expect_err("missing reset reason should fail");

        assert!(err.to_string().contains("required_reason"));
        assert_eq!(service.version().await, 2);
        assert_eq!(
            fs::read_to_string(&paths.override_config_path).expect("override should exist"),
            before_override
        );
        assert_eq!(
            fs::read_to_string(&paths.override_history_path).expect("history should exist"),
            before_history
        );
    }

    #[tokio::test]
    async fn reset_rejects_empty_paths_without_files_or_history() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);

        let err = service
            .reset_paths(Vec::new(), Some("empty reset should fail".to_string()))
            .await
            .expect_err("empty reset should fail");

        assert!(err.to_string().contains("empty_paths"));
        assert_eq!(service.version().await, 1);
        assert!(!paths.override_config_path.exists());
        assert!(!paths.override_history_path.exists());
    }

    #[tokio::test]
    async fn reset_rejects_absent_override_path_without_history() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let before = service.runtime_snapshot().await;

        let err = service
            .reset_paths(
                vec!["max_body_size".to_string()],
                Some("absent reset should fail".to_string()),
            )
            .await
            .expect_err("reset with no override state should fail");

        assert!(err.to_string().contains("no_effective_change"));
        assert_eq!(service.version().await, 1);
        assert_eq!(service.runtime_snapshot().await, before);
        assert!(!paths.override_config_path.exists());
        assert!(!paths.override_history_path.exists());
    }

    #[tokio::test]
    async fn reset_allows_source_only_override_file_change() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        write_test_config(&paths.user_config_path, "log_level: debug\n");
        write_test_config(&paths.override_config_path, "log_level: debug\n");
        let service = service_for_paths(&paths);

        service
            .reset_paths(
                vec!["log_level".to_string()],
                Some("remove redundant override".to_string()),
            )
            .await
            .expect("source-only reset should succeed");

        assert_eq!(service.version().await, 2);
        assert_eq!(service.runtime_snapshot().await.log_level, "debug");
        let document = read_override_document(&paths.override_config_path)
            .expect("override should remain valid");
        assert_eq!(document, json!({}));
        let history =
            read_history_items(&paths.override_history_path, 10, 0).expect("history should read");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].changed_paths, vec!["log_level".to_string()]);
        assert!(history[0].diff.is_empty());
    }

    #[tokio::test]
    async fn reset_rejects_candidate_config_invalid_after_override_removal() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        write_test_config(&paths.user_config_path, "proxy: socks5://127.0.0.1:1080\n");
        write_test_config(
            &paths.override_config_path,
            "proxy: http://127.0.0.1:1080\n",
        );
        let service = service_for_paths(&paths);
        let before = service.runtime_snapshot().await;
        let before_override =
            fs::read_to_string(&paths.override_config_path).expect("override should exist");

        let err = service
            .reset_paths(
                vec!["proxy".to_string()],
                Some("remove proxy override".to_string()),
            )
            .await
            .expect_err("reset should reject invalid candidate effective config");

        assert!(err.to_string().contains("proxy"));
        assert!(err.to_string().contains("http:// or https://"));
        assert_eq!(service.version().await, 1);
        assert_eq!(service.runtime_snapshot().await, before);
        assert_eq!(
            fs::read_to_string(&paths.override_config_path).expect("override should exist"),
            before_override
        );
        assert!(!paths.override_history_path.exists());
    }

    #[tokio::test]
    async fn reset_runtime_preflight_failure_leaves_override_and_history_unchanged() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        service
            .apply_changes(SystemConfigChangeRequest {
                changes: [(
                    "proxy_request.first_byte_timeout_seconds".to_string(),
                    json!(120),
                )]
                .into_iter()
                .collect(),
                reason: Some("raise first byte timeout".to_string()),
            })
            .await
            .expect("apply should create override");
        let before = service.runtime_snapshot().await;
        let before_override =
            fs::read_to_string(&paths.override_config_path).expect("override should exist");
        let before_history =
            fs::read_to_string(&paths.override_history_path).expect("history should exist");
        service
            .force_next_http_client_preflight_error("simulated reset preflight failure")
            .await;

        let err = service
            .reset_paths(
                vec!["proxy_request.first_byte_timeout_seconds".to_string()],
                Some("restore first byte timeout".to_string()),
            )
            .await
            .expect_err("reset preflight should fail");

        assert!(
            err.to_string()
                .contains("simulated reset preflight failure")
        );
        assert_eq!(service.version().await, 2);
        assert_eq!(service.runtime_snapshot().await, before);
        assert_eq!(
            fs::read_to_string(&paths.override_config_path).expect("override should exist"),
            before_override
        );
        assert_eq!(
            fs::read_to_string(&paths.override_history_path).expect("history should exist"),
            before_history
        );
    }

    #[tokio::test]
    async fn concurrent_applies_are_serialized_with_unique_versions() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = Arc::new(service_for_paths(&paths));

        let first = Arc::clone(&service);
        let second = Arc::clone(&service);
        let (first_report, second_report) = tokio::join!(
            async move {
                first
                    .apply_changes(SystemConfigChangeRequest {
                        changes: [("max_body_size".to_string(), json!(2 * 1024 * 1024))]
                            .into_iter()
                            .collect(),
                        reason: Some("raise body limit".to_string()),
                    })
                    .await
            },
            async move {
                second
                    .apply_changes(SystemConfigChangeRequest {
                        changes: [(
                            "routing_resilience.max_candidates_per_request".to_string(),
                            json!(3),
                        )]
                        .into_iter()
                        .collect(),
                        reason: Some("raise fallback budget".to_string()),
                    })
                    .await
            }
        );

        let first_version = first_report
            .expect("first apply should succeed")
            .summary
            .version;
        let second_version = second_report
            .expect("second apply should succeed")
            .summary
            .version;
        assert_ne!(first_version, second_version);
        assert_eq!(service.version().await, 3);
        let document = read_override_document(&paths.override_config_path)
            .expect("override should remain valid");
        assert_eq!(document["max_body_size"], json!(2 * 1024 * 1024));
        assert_eq!(
            document["routing_resilience"]["max_candidates_per_request"],
            json!(3)
        );
    }

    #[tokio::test]
    async fn history_failure_does_not_commit_runtime_or_state() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let before = service.runtime_snapshot().await;
        if let Some(parent) = paths.override_history_path.parent() {
            fs::create_dir_all(parent).expect("history parent should be created");
        }
        fs::create_dir(&paths.override_history_path)
            .expect("history path directory should force append failure");

        let err = service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("max_body_size".to_string(), json!(2 * 1024 * 1024))]
                    .into_iter()
                    .collect(),
                reason: Some("raise body limit".to_string()),
            })
            .await
            .expect_err("history append should fail");

        assert!(err.to_string().contains("history"));
        assert_eq!(service.version().await, 1);
        assert_eq!(service.runtime_snapshot().await, before);
        assert!(paths.override_config_path.exists());
        assert!(
            service
                .report()
                .await
                .summary
                .last_error
                .as_deref()
                .is_some_and(|message| message.contains("history"))
        );
    }

    #[tokio::test]
    async fn http_client_preflight_failure_does_not_write_or_commit() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let manager = Arc::new(
            HttpClientManager::new(1, ProxyRequestConfig::default(), None)
                .expect("manager should build"),
        );
        service
            .register_http_client_manager(Arc::clone(&manager))
            .await;
        service
            .force_next_http_client_preflight_error("simulated client build failure")
            .await;
        let before = service.runtime_snapshot().await;

        let err = service
            .apply_changes(SystemConfigChangeRequest {
                changes: [(
                    "proxy_request.first_byte_timeout_seconds".to_string(),
                    json!(120),
                )]
                .into_iter()
                .collect(),
                reason: Some("raise first byte timeout".to_string()),
            })
            .await
            .expect_err("preflight should fail");

        assert!(err.to_string().contains("simulated client build failure"));
        assert_eq!(service.version().await, 1);
        assert_eq!(service.runtime_snapshot().await, before);
        assert_eq!(manager.current().await.version, 1);
        assert!(!paths.override_config_path.exists());
        assert!(!paths.override_history_path.exists());
    }

    #[tokio::test]
    async fn invalid_manual_override_reload_keeps_runtime_snapshot() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let before = service.runtime_snapshot().await;
        write_test_config(
            &paths.override_config_path,
            "db_url: postgres://example\nlog_level: debug\n",
        );

        let err = service
            .reload_override_file()
            .await
            .expect_err("invalid override should fail reload");

        assert!(err.to_string().contains("unsupported paths"));
        assert_eq!(service.version().await, 1);
        assert_eq!(service.runtime_snapshot().await, before);
        assert!(
            service
                .report()
                .await
                .summary
                .last_error
                .as_deref()
                .is_some_and(|message| message.contains("unsupported paths"))
        );
    }

    #[tokio::test]
    async fn report_refreshes_manual_override_view_without_reloading_runtime() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let before = service.runtime_snapshot().await;

        write_test_config(&paths.override_config_path, "log_level: debug\n");

        let report = service.report().await;

        assert_eq!(report.summary.version, 1);
        assert_eq!(service.version().await, 1);
        assert_eq!(service.runtime_snapshot().await, before);
        assert!(report.summary.override_exists);
        assert!(report.override_file.exists);
        assert!(report.override_file.yaml.contains("log_level: debug"));
        assert!(report.override_file.last_modified_ms.is_some());
        assert!(
            report
                .fields
                .iter()
                .any(|field| { field.path == "log_level" && field.value == json!("info") })
        );
    }

    #[tokio::test]
    async fn report_summarizes_invalid_manual_override_without_leaking_yaml_or_reloading() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let before = service.runtime_snapshot().await;

        write_test_config(
            &paths.override_config_path,
            "db_url: postgres://secret@example/cyder\nlog_level: debug\n",
        );

        let report = service.report().await;
        let serialized = serde_json::to_string(&report).expect("report should serialize");

        assert_eq!(report.summary.version, 1);
        assert_eq!(service.version().await, 1);
        assert_eq!(service.runtime_snapshot().await, before);
        assert!(report.summary.override_exists);
        assert!(report.override_file.exists);
        assert_eq!(report.override_file.yaml, "");
        assert!(
            report
                .override_file
                .invalid_paths
                .contains(&"db_url".to_string())
        );
        assert!(!serialized.contains("postgres://secret"));
        assert!(!serialized.contains("log_level: debug"));
        assert_eq!(service.last_error().await, None);
    }

    #[tokio::test]
    async fn multi_instance_rejects_write_operations() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        write_test_config(
            &paths.user_config_path,
            "deployment:\n  mode: multi_instance\n",
        );
        let service = service_for_paths(&paths);

        let err = service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("max_body_size".to_string(), json!(2 * 1024 * 1024))]
                    .into_iter()
                    .collect(),
                reason: Some("raise body limit".to_string()),
            })
            .await
            .expect_err("multi instance apply should fail");

        assert!(err.to_string().contains("multi_instance_not_supported"));
        assert!(!paths.override_config_path.exists());
    }

    #[tokio::test]
    async fn apply_log_level_updates_global_max_level() {
        let _guard = SERVICE_LOG_LEVEL_TEST_LOCK
            .lock()
            .expect("service log level lock should be available");
        let previous = log::max_level();
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);

        service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("log_level".to_string(), json!("debug"))]
                    .into_iter()
                    .collect(),
                reason: Some("debug incident".to_string()),
            })
            .await
            .expect("log level apply should succeed");

        assert_eq!(log::max_level(), LevelFilter::Debug);
        log::set_max_level(previous);
    }

    #[tokio::test]
    async fn apply_proxy_request_replaces_http_client_bundle() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let manager = Arc::new(
            HttpClientManager::new(1, ProxyRequestConfig::default(), None)
                .expect("manager should build"),
        );
        service
            .register_http_client_manager(Arc::clone(&manager))
            .await;
        let old_bundle = manager.current().await;

        service
            .apply_changes(SystemConfigChangeRequest {
                changes: [(
                    "proxy_request.first_byte_timeout_seconds".to_string(),
                    json!(120),
                )]
                .into_iter()
                .collect(),
                reason: Some("raise first byte timeout".to_string()),
            })
            .await
            .expect("proxy request apply should succeed");

        let current = manager.current().await;
        assert_eq!(old_bundle.version, 1);
        assert_eq!(current.version, 2);
        assert_eq!(current.proxy_request.first_byte_timeout_seconds, Some(120));
    }

    #[tokio::test]
    async fn apply_provider_governance_updates_dynamic_manager() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let manager = Arc::new(ProviderGovernanceConfigManager::new(
            service.runtime_snapshot().await.provider_governance,
        ));
        service
            .register_provider_governance_config_manager(Arc::clone(&manager))
            .await;

        service
            .apply_changes(SystemConfigChangeRequest {
                changes: [("provider_governance.enabled".to_string(), json!(false))]
                    .into_iter()
                    .collect(),
                reason: Some("disable provider circuit during incident".to_string()),
            })
            .await
            .expect("provider governance apply should succeed");

        assert!(!manager.current().await.enabled);
    }

    #[tokio::test]
    async fn apply_diagnostics_updates_policy_manager() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let service = service_for_paths(&paths);
        let manager = Arc::new(DiagnosticsPolicyManager::new(
            DiagnosticsPolicy::from_config(&service.runtime_snapshot().await.diagnostics),
        ));
        service
            .register_diagnostics_policy_manager(Arc::clone(&manager))
            .await;

        service
            .apply_changes(SystemConfigChangeRequest {
                changes: [(
                    "diagnostics.response_capture_max_bytes".to_string(),
                    json!(2048),
                )]
                .into_iter()
                .collect(),
                reason: Some("lower replay capture limit".to_string()),
            })
            .await
            .expect("diagnostics apply should succeed");

        assert_eq!(manager.current().await.response_capture_max_bytes(), 2048);
    }
}
