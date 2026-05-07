// ========== Request Log Types ==========
export interface RecordListItem {
  id: number;
  api_key_id: number;
  requested_model_name?: string | null;
  base_requested_model_name?: string | null;
  resolved_reasoning_suffix?: string | null;
  resolved_reasoning_preset?: string | null;
  resolved_name_scope?: string | null;
  resolved_route_name?: string | null;
  overall_status: string;
  attempt_count: number;
  retry_count: number;
  fallback_count: number;
  request_received_at: number;
  first_attempt_started_at: number | null;
  response_started_to_client_at: number | null;
  completed_at: number | null;
  is_stream: boolean;
  final_provider_id: number | null;
  final_provider_name_snapshot: string | null;
  final_model_id: number | null;
  final_model_name_snapshot: string | null;
  final_real_model_name_snapshot: string | null;
  estimated_cost_nanos: number | null;
  estimated_cost_currency: string | null;
  total_input_tokens: number | null;
  total_output_tokens: number | null;
  output_text_tokens: number | null;
  reasoning_tokens: number | null;
  total_tokens: number | null;
  has_transform_diagnostics: boolean;
  transform_diagnostic_count: number;
  transform_diagnostic_max_loss_level: string | null;
}

export interface RecordRequest extends RecordListItem {
  resolved_route_id: number | null;
  user_api_type: string;
  final_error_code: string | null;
  final_error_message: string | null;
  client_ip: string | null;
  final_attempt_id: number | null;
  final_provider_api_key_id: number | null;
  final_provider_key_snapshot: string | null;
  final_llm_api_type: string | null;
  cost_catalog_id: number | null;
  cost_catalog_version_id: number | null;
  cost_snapshot_json: string | null;
  created_at: number;
  updated_at: number;
  input_text_tokens: number | null;
  input_image_tokens: number | null;
  output_image_tokens: number | null;
  cache_read_tokens: number | null;
  cache_write_tokens: number | null;
  bundle_version: number | null;
  bundle_storage_type: string | null;
  bundle_storage_key: string | null;
}

export interface RecordAttempt {
  id: number;
  request_log_id: number;
  attempt_index: number;
  candidate_position: number;
  provider_id: number | null;
  provider_api_key_id: number | null;
  model_id: number | null;
  provider_key_snapshot: string | null;
  provider_name_snapshot: string | null;
  model_name_snapshot: string | null;
  real_model_name_snapshot: string | null;
  llm_api_type: string | null;
  attempt_status: string;
  scheduler_action: string;
  error_code: string | null;
  error_message: string | null;
  request_uri: string | null;
  request_headers_json: string | null;
  response_headers_json: string | null;
  http_status: number | null;
  started_at: number | null;
  first_byte_at: number | null;
  completed_at: number | null;
  response_started_to_client: boolean;
  backoff_ms: number | null;
  applied_request_patch_ids_json: string | null;
  request_patch_summary_json: string | null;
  estimated_cost_nanos: number | null;
  estimated_cost_currency: string | null;
  cost_catalog_version_id: number | null;
  total_input_tokens: number | null;
  total_output_tokens: number | null;
  input_text_tokens: number | null;
  output_text_tokens: number | null;
  input_image_tokens: number | null;
  output_image_tokens: number | null;
  cache_read_tokens: number | null;
  cache_write_tokens: number | null;
  reasoning_tokens: number | null;
  total_tokens: number | null;
  llm_request_blob_id: number | null;
  llm_request_patch_id: number | null;
  llm_response_blob_id: number | null;
  llm_response_capture_state: string | null;
  created_at: number;
  updated_at: number;
}

export interface RecordDetail {
  request: RecordRequest;
  attempts: RecordAttempt[];
}

export interface RecordPayloadRequestManifest {
  has_user_request_body: boolean;
  user_request_blob_id: number | null;
  has_user_response_body: boolean;
  user_response_blob_id: number | null;
  user_response_capture_state: string | null;
}

export interface RecordPayloadAttemptManifest {
  attempt_id: number | null;
  attempt_index: number;
  has_llm_request_body: boolean;
  llm_request_blob_id: number | null;
  llm_request_patch_id: number | null;
  has_llm_response_body: boolean;
  llm_response_blob_id: number | null;
  llm_response_capture_state: string | null;
}

export interface RecordPayloadManifest {
  bundle_version: number | null;
  log_id: number;
  created_at: number | null;
  request: RecordPayloadRequestManifest;
  attempts: RecordPayloadAttemptManifest[];
  blob_count: number;
  patch_count: number;
}

export interface RecordNameValue {
  name: string;
  value: string | null;
  value_present?: boolean | null;
}

export interface RecordRequestSnapshot {
  request_path: string;
  operation_kind: string;
  query_params: RecordNameValue[];
  sanitized_original_headers: RecordNameValue[];
}

export interface RecordCandidateManifestItem {
  candidate_position: number;
  route_id: number | null;
  route_name: string | null;
  provider_id: number;
  provider_key: string;
  model_id: number;
  model_name: string;
  real_model_name: string | null;
  llm_api_type: string;
  provider_api_key_mode: string;
}

export interface RecordCandidateManifest {
  has_asset: boolean;
  items: RecordCandidateManifestItem[];
}

export interface RecordTransformDiagnosticsSummaryBody {
  count: number;
  max_loss_level: string | null;
  kinds: string[];
  phases: string[];
}

export interface RecordTransformDiagnosticItem {
  phase: string;
  diagnostic: Record<string, unknown>;
}

export interface RecordTransformDiagnosticsSummary {
  has_asset: boolean;
  summary: RecordTransformDiagnosticsSummaryBody;
  items: RecordTransformDiagnosticItem[];
}

export interface RecordReplayKindCapability {
  available: boolean;
  reasons: string[];
  attempt_ids: number[];
}

export interface RecordReplayCapabilitySummary {
  attempt_upstream: RecordReplayKindCapability;
  gateway_request: RecordReplayKindCapability;
}

export interface RecordArtifactResponse {
  payload_manifest: RecordPayloadManifest;
  request_snapshot: RecordRequestSnapshot | null;
  candidate_manifest: RecordCandidateManifest;
  transform_diagnostics: RecordTransformDiagnosticsSummary;
  replay_capability: RecordReplayCapabilitySummary;
}

export interface RecordListParams {
  page?: number;
  page_size?: number;
  api_key_id?: number;
  provider_id?: number;
  model_id?: number;
  status?: string;
  user_api_type?: string;
  resolved_name_scope?: string;
  final_error_code?: string;
  has_retry?: boolean;
  has_fallback?: boolean;
  has_transform_diagnostics?: boolean;
  latency_ms_min?: number;
  latency_ms_max?: number;
  total_tokens_min?: number;
  total_tokens_max?: number;
  estimated_cost_nanos_min?: number;
  estimated_cost_nanos_max?: number;
  start_time?: number;
  end_time?: number;
  search?: string;
  [key: string]: string | number | boolean | undefined;
}

export type RecordStorageType = "FILE_SYSTEM" | "S3";

export interface RecordDiagnosticsRetentionParams {
  request_log_bundle_retention_days?: number | null;
  replay_artifact_retention_days?: number | null;
  delete_batch_size?: number | null;
  include_request_log_bundles?: boolean | null;
  include_replay_artifacts?: boolean | null;
}

export type RecordDiagnosticsRetentionItemStatus =
  | "candidate"
  | "deleted"
  | "skipped"
  | "failed";

export interface RecordDiagnosticsRetentionItem {
  id: number;
  request_log_id: number | null;
  replay_run_id: number | null;
  storage_type: RecordStorageType;
  storage_key: string;
  artifact_version: number | null;
  bundle_version: number | null;
  created_at: number;
  status: RecordDiagnosticsRetentionItemStatus;
  message: string | null;
}

export interface RecordDiagnosticsRetentionStorageTypeCount {
  storage_type: RecordStorageType;
  count: number;
}

export interface RecordDiagnosticsRetentionBucket {
  retention_days: number;
  cutoff_created_before: number;
  candidate_count: number;
  storage_type_counts: RecordDiagnosticsRetentionStorageTypeCount[];
  oldest_created_at: number | null;
  newest_created_at: number | null;
  sample_storage_keys: string[];
  succeeded_count: number;
  skipped_count: number;
  failed_count: number;
  items: RecordDiagnosticsRetentionItem[];
}

export interface RecordDiagnosticsRetentionResponse {
  enabled: boolean;
  executed: boolean;
  now_ms: number;
  delete_batch_size: number;
  request_log_bundles: RecordDiagnosticsRetentionBucket;
  replay_artifacts: RecordDiagnosticsRetentionBucket;
}

export interface RecordDiagnosticsStorageInventoryParams {
  storage_types?: RecordStorageType[] | null;
  prefix?: string | null;
  object_sample_limit?: number | null;
  missing_locator_sample_limit?: number | null;
  object_scan_limit?: number | null;
  db_locator_limit?: number | null;
}

export type RecordDiagnosticsStorageInventoryStatus =
  | "available"
  | "skipped"
  | "failed";

export type RecordDiagnosticsStorageLocatorKind =
  | "request_log_bundle"
  | "replay_artifact";

export interface RecordDiagnosticsStorageObjectSample {
  key: string;
  size_bytes: number | null;
  last_modified_ms: number | null;
}

export interface RecordDiagnosticsStorageMissingLocatorSample {
  locator_kind: RecordDiagnosticsStorageLocatorKind;
  request_log_id: number | null;
  replay_run_id: number | null;
  storage_type: RecordStorageType;
  storage_key: string;
  artifact_version: number | null;
  bundle_version: number | null;
  created_at: number;
  message: string | null;
}

export interface RecordDiagnosticsStorageInventoryBucket {
  storage_type: RecordStorageType;
  status: RecordDiagnosticsStorageInventoryStatus;
  message: string | null;
  prefix: string | null;
  object_scan_limit: number;
  object_limit_reached: boolean;
  object_count: number;
  total_size_bytes: number;
  unknown_size_object_count: number;
  referenced_object_count: number;
  unreferenced_object_count: number;
  missing_locator_count: number;
  locator_check_failed_count: number;
  object_samples: RecordDiagnosticsStorageObjectSample[];
  unreferenced_samples: RecordDiagnosticsStorageObjectSample[];
  missing_locator_samples: RecordDiagnosticsStorageMissingLocatorSample[];
}

export interface RecordDiagnosticsStorageInventoryResponse {
  prefix: string | null;
  object_sample_limit: number;
  missing_locator_sample_limit: number;
  object_scan_limit: number;
  db_locator_limit: number;
  db_locator_scanned_count: number;
  db_locator_limit_reached: boolean;
  storage: RecordDiagnosticsStorageInventoryBucket[];
}

export type RecordReplayKind = "attempt_upstream" | "gateway_request";
export type RecordReplayMode = "dry_run" | "live";
export type RecordReplaySemanticBasis =
  | "historical_attempt_snapshot"
  | "historical_request_snapshot_with_current_config";
export type RecordReplayStatus =
  | "pending"
  | "running"
  | "success"
  | "error"
  | "cancelled"
  | "rejected";
export type RecordReplayDiffBaselineKind =
  | "original_attempt"
  | "original_request_result";

export interface RecordReplayNameValue {
  name: string;
  value?: string | null;
}

export interface RecordReplayQueryParam {
  name: string;
  value?: string | null;
  value_present?: boolean | null;
}

export interface RecordReplayBody {
  media_type?: string | null;
  json?: unknown;
  text?: string | null;
  capture_state?: string | null;
}

export interface RecordReplayProviderSnapshot {
  provider_id?: number | null;
  provider_api_key_id?: number | null;
  provider_key?: string | null;
  provider_name?: string | null;
}

export interface RecordReplayModelSnapshot {
  model_id?: number | null;
  model_name?: string | null;
  real_model_name?: string | null;
  llm_api_type?: string | null;
}

export type RecordReplayInputSnapshot =
  | {
      kind: "attempt_upstream";
      request_uri: string;
      sanitized_request_headers?: RecordReplayNameValue[];
      llm_request_body?: RecordReplayBody | null;
      provider?: RecordReplayProviderSnapshot | null;
      model?: RecordReplayModelSnapshot | null;
    }
  | {
      kind: "gateway_request";
      request_path: string;
      query_params?: RecordReplayQueryParam[];
      sanitized_original_headers?: RecordReplayNameValue[];
      user_request_body?: RecordReplayBody | null;
    };

export interface RecordReplayResolvedRoute {
  route_id?: number | null;
  route_name?: string | null;
}

export interface RecordReplayResolvedCandidate {
  candidate_position?: number | null;
  provider_id?: number | null;
  provider_api_key_id?: number | null;
  model_id?: number | null;
  llm_api_type?: string | null;
}

export interface RecordReplayCandidateDecision {
  candidate_position: number;
  provider_id?: number | null;
  provider_api_key_id?: number | null;
  model_id?: number | null;
  llm_api_type?: string | null;
  attempt_status: string;
  scheduler_action: string;
  error_code?: string | null;
  error_message?: string | null;
  request_uri?: string | null;
}

export interface RecordReplayExecutionPreview {
  semantic_basis: RecordReplaySemanticBasis;
  requested_model_name?: string | null;
  base_requested_model_name?: string | null;
  resolved_reasoning_suffix?: string | null;
  resolved_reasoning_preset?: string | null;
  resolved_route?: RecordReplayResolvedRoute | null;
  resolved_candidate?: RecordReplayResolvedCandidate | null;
  candidate_decisions?: RecordReplayCandidateDecision[];
  applied_request_patch_summary?: unknown;
  final_request_uri?: string | null;
  final_request_headers?: RecordReplayNameValue[];
  final_request_body?: RecordReplayBody | null;
}

export interface RecordAttemptReplayBaseline {
  attempt_status: string;
  http_status?: number | null;
  response_headers?: RecordReplayNameValue[];
  response_body_capture_state?: string | null;
  total_tokens?: number | null;
  estimated_cost_nanos?: number | null;
  estimated_cost_currency?: string | null;
}

export interface RecordGatewayReplayBaseline {
  overall_status: string;
  final_error_code?: string | null;
  final_error_message?: string | null;
  total_tokens?: number | null;
  estimated_cost_nanos?: number | null;
  estimated_cost_currency?: string | null;
  user_response_body_capture_state?: string | null;
}

export interface RecordAttemptReplayPreviewResponse {
  source_request_log_id: number;
  source_attempt_id: number;
  replay_kind: "attempt_upstream";
  semantic_basis: RecordReplaySemanticBasis;
  preview_fingerprint: string;
  preview_created_at: number;
  selected_provider_api_key_id: number;
  used_provider_api_key_override: boolean;
  input_snapshot: RecordReplayInputSnapshot;
  execution_preview: RecordReplayExecutionPreview;
  baseline: RecordAttemptReplayBaseline;
}

export interface RecordGatewayReplayPreviewResponse {
  source_request_log_id: number;
  replay_kind: "gateway_request";
  semantic_basis: RecordReplaySemanticBasis;
  preview_fingerprint: string;
  preview_created_at: number;
  input_snapshot: RecordReplayInputSnapshot;
  execution_preview: RecordReplayExecutionPreview;
  baseline: RecordGatewayReplayBaseline;
}

export type RecordReplayPreviewResponse =
  | RecordAttemptReplayPreviewResponse
  | RecordGatewayReplayPreviewResponse;

export interface RecordAttemptReplayPreviewParams {
  provider_api_key_id_override?: number | null;
}

export interface RecordAttemptReplayExecuteParams extends RecordAttemptReplayPreviewParams {
  replay_mode: RecordReplayMode;
  confirm_live_request: boolean;
  preview_fingerprint: string;
}

export interface RecordGatewayReplayPreviewParams {}

export interface RecordGatewayReplayExecuteParams {
  replay_mode: RecordReplayMode;
  confirm_live_request: boolean;
  preview_fingerprint: string;
}

export interface RecordReplayRun {
  id: number;
  source_request_log_id: number;
  source_attempt_id: number | null;
  replay_kind: RecordReplayKind;
  replay_mode: RecordReplayMode;
  semantic_basis: RecordReplaySemanticBasis;
  status: RecordReplayStatus;
  executed_route_id: number | null;
  executed_route_name: string | null;
  executed_provider_id: number | null;
  executed_provider_api_key_id: number | null;
  executed_model_id: number | null;
  executed_llm_api_type: string | null;
  downstream_request_uri: string | null;
  http_status: number | null;
  error_code: string | null;
  error_message: string | null;
  total_input_tokens: number | null;
  total_output_tokens: number | null;
  reasoning_tokens: number | null;
  total_tokens: number | null;
  estimated_cost_nanos: number | null;
  estimated_cost_currency: string | null;
  diff_summary_json: string | null;
  artifact_version: number | null;
  artifact_storage_type: string | null;
  artifact_storage_key: string | null;
  started_at: number | null;
  first_byte_at: number | null;
  completed_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface RecordReplayArtifactSource {
  request_log_id: number;
  attempt_id?: number | null;
  replay_kind: RecordReplayKind;
  replay_mode: RecordReplayMode;
}

export interface RecordReplayBodyCaptureMetadata {
  state: string;
  bytes_captured: number;
  original_size_bytes?: number | null;
  original_size_known: boolean;
  truncated: boolean;
  sha256: string;
  capture_limit_bytes: number;
  body_encoding: string;
}

export interface RecordReplayArtifactResult {
  status: RecordReplayStatus;
  http_status?: number | null;
  response_headers?: RecordReplayNameValue[];
  response_body?: RecordReplayBody | null;
  response_body_capture_state?: string | null;
  response_body_capture?: RecordReplayBodyCaptureMetadata | null;
  usage_normalization?: unknown;
  transform_diagnostics?: unknown[];
  attempt_timeline?: RecordReplayCandidateDecision[];
}

export interface RecordReplayArtifactDiff {
  baseline_kind: RecordReplayDiffBaselineKind;
  status_changed?: boolean | null;
  headers_changed?: boolean | null;
  body_changed?: boolean | null;
  token_delta?: number | null;
  cost_delta?: number | null;
  summary_lines: string[];
}

export interface RecordReplayArtifact {
  version: number;
  replay_run_id: number;
  created_at: number;
  source: RecordReplayArtifactSource;
  input_snapshot?: RecordReplayInputSnapshot | null;
  execution_preview?: RecordReplayExecutionPreview | null;
  result?: RecordReplayArtifactResult | null;
  diff?: RecordReplayArtifactDiff | null;
}
