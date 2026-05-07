import type { DeploymentMode, JsonValue } from "./shared";

// ========== System Config Types ==========
export type SystemConfigLayerKind =
  | "program_default"
  | "default_file"
  | "user_file"
  | "environment"
  | "override_file"
  | "derived";

export type SystemConfigValueKind =
  | "bool"
  | "enum"
  | "nullable_object"
  | "nullable_string"
  | "nullable_u64"
  | "object"
  | "string"
  | "u16"
  | "u32"
  | "u64"
  | "usize";

export interface SystemConfigFieldSource {
  kind: SystemConfigLayerKind;
  source_name: string;
  configured: boolean;
  warnings: string[];
}

export interface SystemConfigField {
  path: string;
  section: string;
  value_kind: SystemConfigValueKind;
  editable: boolean;
  hot_reloadable: boolean;
  restart_required: boolean;
  sensitive: boolean;
  description: string;
  constraints: string[];
  value: JsonValue;
  source: SystemConfigFieldSource;
}

export interface SystemConfigReportSummary {
  version: number;
  loaded_at: number;
  last_error: string | null;
  override_path: string;
  override_exists: boolean;
  history_path: string;
  history_exists: boolean;
  deployment_mode: DeploymentMode;
}

export interface SystemConfigOverrideFileReport {
  path: string;
  exists: boolean;
  yaml: string;
  invalid_paths: string[];
  last_modified_ms: number | null;
}

export type SystemConfigPersistenceHealthStatus =
  | "ok"
  | "warning"
  | "error"
  | "skipped";

export interface SystemConfigPersistenceHealthItem {
  key: string;
  path: string;
  exists: boolean;
  readable: boolean;
  writable: boolean;
  status: SystemConfigPersistenceHealthStatus;
  message: string;
}

export interface SystemConfigPersistenceHealthReport {
  status: SystemConfigPersistenceHealthStatus;
  items: SystemConfigPersistenceHealthItem[];
}

export interface SystemConfigReport {
  summary: SystemConfigReportSummary;
  fields: SystemConfigField[];
  effective: JsonValue;
  override_file: SystemConfigOverrideFileReport;
  persistence_health: SystemConfigPersistenceHealthReport;
}

export interface SystemConfigChangeRequest {
  changes: Record<string, JsonValue>;
  reason?: string | null;
}

export interface SystemConfigApplyRequest {
  changes: Record<string, JsonValue>;
  reason: string;
}

export interface SystemConfigResetRequest {
  paths: string[];
  reason?: string | null;
}

export interface SystemConfigResetApplyRequest {
  paths: string[];
  reason: string;
}

export interface SystemConfigHistoryQuery {
  limit?: number;
  offset?: number;
}

export interface SystemConfigValidationIssue {
  path: string;
  code: string;
  message: string;
}

export interface SystemConfigValidationReport {
  valid: boolean;
  errors: SystemConfigValidationIssue[];
  warnings: SystemConfigValidationIssue[];
}

export interface SystemConfigDiffItem {
  path: string;
  old_value: JsonValue;
  new_value: JsonValue;
}

export interface SystemConfigRuntimeActions {
  update_runtime_snapshot: boolean;
  update_log_level: boolean;
  rebuild_http_client: boolean;
  hot_reloadable_paths: string[];
}

export interface SystemConfigPreviewResponse {
  diff: SystemConfigDiffItem[];
  validation: SystemConfigValidationReport;
  next_override_yaml: string;
  runtime_actions: SystemConfigRuntimeActions;
  write_disabled_reason: string | null;
}

export type SystemConfigHistoryOperation = "apply" | "reset" | "reload";

export interface SystemConfigHistoryItem {
  changed_at: number;
  actor: string;
  reason: string | null;
  operation: SystemConfigHistoryOperation;
  version_before: number;
  version_after: number;
  changed_paths: string[];
  diff: SystemConfigDiffItem[];
}
