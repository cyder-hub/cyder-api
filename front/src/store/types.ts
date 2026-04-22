// Shared Type Definitions

// ========== Auth Types ==========
export interface User {
  username: string;
}

// ========== System / Dashboard Types ==========
export interface SystemOverviewStats {
  providers_count: number;
  models_count: number;
  provider_keys_count: number;
}

export interface TodayRequestLogStats {
  requests_count: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_reasoning_tokens: number;
  total_tokens: number;
  total_cost: Record<string, number>;
}

export type DashboardRuntimeWindow = "15m" | "1h" | "6h" | "24h";

export interface DashboardOverviewStats {
  provider_count: number;
  enabled_provider_count: number;
  model_count: number;
  enabled_model_count: number;
  provider_key_count: number;
  enabled_provider_key_count: number;
  api_key_count: number;
  enabled_api_key_count: number;
}

export interface DashboardTodayStats {
  request_count: number;
  success_count: number;
  error_count: number;
  success_rate: number | null;
  total_input_tokens: number;
  total_output_tokens: number;
  total_reasoning_tokens: number;
  total_tokens: number;
  total_cost: Record<string, number>;
  avg_first_byte_ms: number | null;
  avg_total_latency_ms: number | null;
  active_provider_count: number;
  active_model_count: number;
  active_api_key_count: number;
}

export interface DashboardRuntimeSummary {
  window: DashboardRuntimeWindow;
  healthy_count: number;
  degraded_count: number;
  half_open_count: number;
  open_count: number;
  no_traffic_count: number;
}

export interface DashboardProviderAlertItem {
  provider_id: number;
  provider_key: string;
  provider_name: string;
  runtime_level: ProviderRuntimeLevel;
  request_count: number;
  error_count: number;
  success_rate: number | null;
  avg_total_latency_ms: number | null;
  last_error_at: number | null;
  last_error_summary: string | null;
}

export interface DashboardCostProviderAlertItem {
  provider_id: number;
  provider_key: string;
  provider_name: string;
  request_count: number;
  success_rate: number | null;
  avg_total_latency_ms: number | null;
  total_cost: Record<string, number>;
}

export interface DashboardCostModelAlertItem {
  provider_id: number;
  provider_key: string;
  model_id: number;
  model_name: string;
  real_model_name: string | null;
  request_count: number;
  total_tokens: number;
  total_cost: Record<string, number>;
}

export interface DashboardAlerts {
  open_providers: DashboardProviderAlertItem[];
  half_open_providers: DashboardProviderAlertItem[];
  degraded_providers: DashboardProviderAlertItem[];
  top_error_providers: DashboardProviderAlertItem[];
  top_cost_providers: DashboardCostProviderAlertItem[];
  top_cost_models: DashboardCostModelAlertItem[];
}

export interface DashboardTopProviderItem {
  provider_id: number;
  provider_key: string;
  provider_name: string;
  request_count: number;
  success_count: number;
  error_count: number;
  success_rate: number | null;
  total_cost: Record<string, number>;
  avg_total_latency_ms: number | null;
}

export interface DashboardTopModelItem {
  provider_id: number;
  provider_key: string;
  model_id: number;
  model_name: string;
  real_model_name: string | null;
  request_count: number;
  total_tokens: number;
  total_cost: Record<string, number>;
}

export interface DashboardResponse {
  overview: DashboardOverviewStats;
  today: DashboardTodayStats;
  runtime: DashboardRuntimeSummary;
  alerts: DashboardAlerts;
  top_providers: DashboardTopProviderItem[];
  top_models: DashboardTopModelItem[];
}

export interface DashboardKpiSection {
  today: DashboardTodayStats;
  runtime: DashboardRuntimeSummary;
}

export interface DashboardResourcesSection {
  overview: DashboardOverviewStats;
  today: DashboardTodayStats;
  runtime: DashboardRuntimeSummary;
}

export interface DashboardAlertsSection {
  alerts: DashboardAlerts;
  top_providers: DashboardTopProviderItem[];
  top_models: DashboardTopModelItem[];
}

export interface UsageStatItem {
  provider_id: number | null;
  model_id: number | null;
  api_key_id: number | null;
  provider_key: string | null;
  model_name: string | null;
  real_model_name: string | null;
  api_key_name: string | null;
  group_key: string;
  group_label: string;
  group_detail: string | null;
  total_input_tokens: number;
  total_output_tokens: number;
  total_reasoning_tokens: number;
  total_tokens: number;
  request_count: number;
  success_count: number;
  error_count: number;
  success_rate: number | null;
  avg_total_latency_ms: number | null;
  latency_sample_count: number;
  total_cost: Record<string, number>;
  is_other: boolean;
}

export interface UsageStatsPeriod {
  time: number;
  data: UsageStatItem[];
}

// ========== API Key Types ==========
export type ApiKeyAction = "ALLOW" | "DENY";
export type ApiKeyAclRuleScope = "PROVIDER" | "MODEL";

export interface ApiKeyAclRule {
  id: number;
  effect: ApiKeyAction;
  priority: number;
  scope: ApiKeyAclRuleScope;
  provider_id: number | null;
  model_id: number | null;
  is_enabled: boolean;
  description: string | null;
}

export interface ApiKeyItem {
  id: number;
  key_prefix: string;
  key_last4: string;
  name: string;
  description: string | null;
  default_action: ApiKeyAction;
  is_enabled: boolean;
  expires_at: number | null;
  rate_limit_rpm: number | null;
  max_concurrent_requests: number | null;
  quota_daily_requests: number | null;
  quota_daily_tokens: number | null;
  quota_monthly_tokens: number | null;
  budget_daily_nanos: number | null;
  budget_daily_currency: string | null;
  budget_monthly_nanos: number | null;
  budget_monthly_currency: string | null;
  created_at: number;
  updated_at: number;
  created_at_formatted?: string;
  updated_at_formatted?: string;
}

export interface ApiKeyDetail extends ApiKeyItem {
  acl_rules: ApiKeyAclRule[];
  model_overrides: ApiKeyModelOverrideItem[];
}

export interface ApiKeyReveal {
  id: number;
  name: string;
  key_prefix: string;
  key_last4: string;
  api_key: string;
  updated_at: number;
}

export interface ApiKeyCreateResponse {
  detail: ApiKeyDetail;
  reveal: ApiKeyReveal;
}

export interface ApiKeyRuntimeBilledAmount {
  currency: string;
  amount_nanos: number;
}

export interface ApiKeyRuntimeSnapshot {
  api_key_id: number;
  current_concurrency: number;
  current_minute_bucket: number | null;
  current_minute_request_count: number;
  day_bucket: number | null;
  daily_request_count: number;
  daily_token_count: number;
  month_bucket: number | null;
  monthly_token_count: number;
  daily_billed_amounts: ApiKeyRuntimeBilledAmount[];
  monthly_billed_amounts: ApiKeyRuntimeBilledAmount[];
}

export interface ApiKeyAclRulePayload {
  id?: number;
  effect: ApiKeyAction;
  priority: number;
  scope: ApiKeyAclRuleScope;
  provider_id: number | null;
  model_id: number | null;
  is_enabled?: boolean;
  description?: string | null;
}

export interface ApiKeyModelOverrideItem {
  id: number;
  source_name: string;
  target_route_id: number;
  target_route_name: string | null;
  description: string | null;
  is_enabled: boolean;
}

export interface ApiKeyModelOverridePayload {
  source_name: string;
  target_route_id: number;
  description?: string | null;
  is_enabled?: boolean;
}

export interface ApiKeyCreatePayload {
  name: string;
  description?: string | null;
  default_action?: ApiKeyAction;
  is_enabled?: boolean;
  expires_at?: number | null;
  rate_limit_rpm?: number | null;
  max_concurrent_requests?: number | null;
  quota_daily_requests?: number | null;
  quota_daily_tokens?: number | null;
  quota_monthly_tokens?: number | null;
  budget_daily_nanos?: number | null;
  budget_daily_currency?: string | null;
  budget_monthly_nanos?: number | null;
  budget_monthly_currency?: string | null;
  acl_rules?: ApiKeyAclRulePayload[];
  model_overrides?: ApiKeyModelOverridePayload[];
}

export interface ApiKeyUpdatePayload {
  name?: string;
  description?: string | null;
  default_action?: ApiKeyAction;
  is_enabled?: boolean;
  expires_at?: number | null;
  rate_limit_rpm?: number | null;
  max_concurrent_requests?: number | null;
  quota_daily_requests?: number | null;
  quota_daily_tokens?: number | null;
  quota_monthly_tokens?: number | null;
  budget_daily_nanos?: number | null;
  budget_daily_currency?: string | null;
  budget_monthly_nanos?: number | null;
  budget_monthly_currency?: string | null;
  acl_rules?: ApiKeyAclRulePayload[];
  model_overrides?: ApiKeyModelOverridePayload[];
}

// ========== Provider Types ==========
export interface ProviderBase {
  id: number;
  provider_key: string;
  name: string;
  endpoint: string;
  use_proxy: boolean;
  provider_type: string;
}

export interface ProviderSummaryItem {
  id: number;
  provider_key: string;
  name: string;
  is_enabled: boolean;
}

export interface ProviderApiKeyItem {
  id: number;
  api_key: string;
  description: string | null;
}

export interface ModelItem {
  id: number;
  model_name: string;
  real_model_name: string | null;
  supports_streaming: boolean;
  supports_tools: boolean;
  supports_reasoning: boolean;
  supports_image_input: boolean;
  supports_embeddings: boolean;
  supports_rerank: boolean;
  is_enabled: boolean;
}

export interface ModelDetail {
  model: ModelItem;
  request_patches: RequestPatchRule[];
}

export interface ModelSummaryItem {
  id: number;
  provider_id: number;
  provider_key: string;
  provider_name: string;
  model_name: string;
  real_model_name: string | null;
  supports_streaming: boolean;
  supports_tools: boolean;
  supports_reasoning: boolean;
  supports_image_input: boolean;
  supports_embeddings: boolean;
  supports_rerank: boolean;
  is_enabled: boolean;
}

export interface ProviderListItem {
  provider: ProviderBase;
  models: ModelDetail[];
  provider_keys: ProviderApiKeyItem[];
  request_patches: RequestPatchRule[];
}

export type ProviderRuntimeWindow = "15m" | "1h" | "6h" | "24h";

export type ProviderRuntimeHealthStatus = "healthy" | "open" | "half_open";

export type ProviderRuntimeLevel =
  | "healthy"
  | "degraded"
  | "open"
  | "half_open"
  | "no_traffic";

export type ProviderRuntimeStatusFilter =
  | "all"
  | "healthy"
  | "degraded"
  | "open"
  | "half_open"
  | "no_traffic";

export type ProviderRuntimeSortField =
  | "health"
  | "error_rate"
  | "latency"
  | "last_error_at"
  | "request_count";

export type SortDirection = "asc" | "desc";

export interface ProviderRuntimeStatusCodeStat {
  status_code: number;
  count: number;
}

export interface ProviderRuntimeCostStat {
  currency: string;
  amount_nanos: number;
}

export interface ProviderRuntimeItem {
  provider_id: number;
  provider_key: string;
  provider_name: string;
  provider_type: string;
  is_enabled: boolean;
  use_proxy: boolean;
  enabled_model_count: number;
  enabled_provider_key_count: number;
  health_status: ProviderRuntimeHealthStatus;
  runtime_level: ProviderRuntimeLevel;
  consecutive_failures: number;
  half_open_probe_in_flight: boolean;
  opened_at: number | null;
  last_failure_at: number | null;
  last_recovered_at: number | null;
  last_error: string | null;
  request_count: number;
  success_count: number;
  error_count: number;
  success_rate: number | null;
  avg_first_byte_ms: number | null;
  avg_total_latency_ms: number | null;
  last_request_at: number | null;
  last_success_at: number | null;
  last_error_at: number | null;
  last_error_summary: string | null;
  status_code_breakdown: ProviderRuntimeStatusCodeStat[];
  total_cost: ProviderRuntimeCostStat[];
  sort_score: number;
}

export interface ProviderRuntimeSummary {
  total_provider_count: number;
  healthy_count: number;
  degraded_count: number;
  half_open_count: number;
  open_count: number;
  no_traffic_count: number;
  window: ProviderRuntimeWindow;
  generated_at: number;
}

export interface ProviderRuntimeListParams {
  window?: ProviderRuntimeWindow;
  status?: ProviderRuntimeStatusFilter;
  search?: string;
  sort?: ProviderRuntimeSortField;
  direction?: SortDirection;
  only_enabled?: boolean;
}

// ========== Model Route Types ==========
export interface ModelRouteItem {
  id: number;
  route_name: string;
  description: string | null;
  is_enabled: boolean;
  expose_in_models: boolean;
  deleted_at?: number | null;
  created_at?: number;
  updated_at?: number;
}

export interface ModelRouteListItem {
  route: ModelRouteItem;
  candidate_count: number;
}

export interface ModelRouteCandidate {
  id: number;
  route_id: number;
  model_id: number;
  priority: number;
  is_enabled: boolean;
  deleted_at?: number | null;
  created_at?: number;
  updated_at?: number;
}

export interface ModelRouteCandidateDetail {
  candidate: ModelRouteCandidate;
  provider_id: number;
  provider_key: string;
  model_name: string;
  real_model_name: string | null;
  model_is_enabled: boolean;
}

export interface ModelRouteDetail {
  route: ModelRouteItem;
  candidates: ModelRouteCandidateDetail[];
}

export interface ModelRouteCandidatePayload {
  model_id: number;
  priority: number;
  is_enabled?: boolean;
}

export interface ModelRoutePayload {
  route_name: string;
  description?: string | null;
  is_enabled?: boolean;
  expose_in_models?: boolean;
  candidates: ModelRouteCandidatePayload[];
}

export interface ModelRouteUpdatePayload {
  route_name?: string;
  description?: string | null;
  is_enabled?: boolean;
  expose_in_models?: boolean;
  candidates?: ModelRouteCandidatePayload[];
}

// ========== Request Patch Types ==========
export type RequestPatchPlacement = "HEADER" | "QUERY" | "BODY";

export type RequestPatchOperation = "SET" | "REMOVE";

export type RequestPatchScopeKind = "PROVIDER" | "MODEL";

export type RequestPatchRuleOrigin = "ProviderDirect" | "ModelDirect";

export type RequestPatchExplainStatus =
  | "Effective"
  | "Overridden"
  | "Conflicted";

export interface RequestPatchRule {
  id: number;
  provider_id: number | null;
  model_id: number | null;
  scope?: RequestPatchScopeKind;
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json: JsonValue | string | null;
  description: string | null;
  is_enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface InheritedRequestPatchRule {
  rule: RequestPatchRule;
  overridden_by_rule_id: number | null;
  conflict_with_rule_ids: number[];
  is_effective: boolean;
}

export interface ResolvedRequestPatchRule {
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json: string | null;
  source_rule_id: number;
  source_origin: RequestPatchRuleOrigin;
  overridden_rule_ids: number[];
  description: string | null;
}

export interface RequestPatchConflict {
  provider_rule_id: number;
  model_rule_id: number;
  placement: RequestPatchPlacement;
  provider_target: string;
  model_target: string;
  reason: string;
}

export interface RequestPatchExplainEntry {
  rule: RequestPatchRule;
  origin: RequestPatchRuleOrigin;
  status: RequestPatchExplainStatus;
  effective_rule_id: number | null;
  conflict_with_rule_ids: number[];
  message: string | null;
}

export interface ModelDetailModel {
  id: number;
  provider_id: number;
  model_name: string;
  real_model_name: string | null;
  cost_catalog_id: number | null;
  supports_streaming: boolean;
  supports_tools: boolean;
  supports_reasoning: boolean;
  supports_image_input: boolean;
  supports_embeddings: boolean;
  supports_rerank: boolean;
  deleted_at: number | null;
  is_enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface ModelRouteReferenceItem {
  id: number;
  route_name: string;
  description: string | null;
  is_enabled: boolean;
  expose_in_models: boolean;
}

export interface ModelDetailResponse {
  model: ModelDetailModel;
  request_patches: RequestPatchRule[];
  inherited_request_patches: InheritedRequestPatchRule[];
  effective_request_patches: ResolvedRequestPatchRule[];
  request_patch_explain: RequestPatchExplainEntry[];
  request_patch_conflicts: RequestPatchConflict[];
  has_request_patch_conflicts: boolean;
  route_references: ModelRouteReferenceItem[];
}

export interface RequestPatchPayload {
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json?: JsonValue | null;
  description?: string | null;
  is_enabled?: boolean;
  confirm_dangerous_target?: boolean;
}

export interface DangerousRequestPatchSavePayload
  extends RequestPatchPayload {
  confirm_dangerous_target: true;
}

export interface RequestPatchUpdatePayload {
  placement?: RequestPatchPlacement;
  target?: string;
  operation?: RequestPatchOperation;
  value_json?: JsonValue | null;
  description?: string | null;
  is_enabled?: boolean;
  confirm_dangerous_target?: boolean;
}

export interface RequestPatchDangerousTargetConfirmation {
  placement: RequestPatchPlacement;
  target: string;
  reason: string;
  confirm_field: string;
}

export type RequestPatchMutationOutcome =
  | {
      result: "saved";
      rule: RequestPatchRule;
    }
  | {
      result: "confirmation_required";
      confirmation: RequestPatchDangerousTargetConfirmation;
    };

export interface ModelEffectiveRequestPatchResponse {
  provider_id: number;
  model_id: number;
  effective_rules: ResolvedRequestPatchRule[];
  conflicts: RequestPatchConflict[];
  has_conflicts: boolean;
}

export interface RequestPatchExplainResponse {
  provider_id: number;
  model_id: number;
  direct_rules: RequestPatchRule[];
  inherited_rules: InheritedRequestPatchRule[];
  effective_rules: ResolvedRequestPatchRule[];
  explain: RequestPatchExplainEntry[];
  conflicts: RequestPatchConflict[];
  has_conflicts: boolean;
}

export interface RequestPatchTraceSummary {
  provider_id: number;
  model_id: number | null;
  effective_rules: ResolvedRequestPatchRule[];
  explain: RequestPatchExplainEntry[];
  conflicts: RequestPatchConflict[];
  has_conflicts: boolean;
}

// ========== Cost Types ==========
export interface CostCatalog {
  id: number;
  name: string;
  description: string | null;
  created_at: number;
  updated_at: number;
}

export interface CostCatalogPayload {
  name: string;
  description?: string;
}

export interface CostCatalogVersion {
  id: number;
  catalog_id: number;
  version: string;
  currency: string;
  source: string | null;
  effective_from: number;
  effective_until: number | null;
  first_used_at: number | null;
  is_archived: boolean;
  is_enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface CostCatalogVersionPayload {
  version: string;
  currency: string;
  source?: string | null;
  effective_from: number;
  effective_until?: number | null;
  is_enabled: boolean;
}

export interface CostComponent {
  id: number;
  catalog_version_id: number;
  meter_key: string;
  charge_kind: string;
  unit_price_nanos: number | null;
  flat_fee_nanos: number | null;
  tier_config_json: string | null;
  match_attributes_json: string | null;
  priority: number;
  description: string | null;
  created_at: number;
  updated_at: number;
}

export interface CostComponentPayload {
  catalog_version_id: number;
  meter_key: string;
  charge_kind: string;
  unit_price_nanos?: number | null;
  flat_fee_nanos?: number | null;
  tier_config_json?: string | null;
  match_attributes_json?: string | null;
  priority: number;
  description?: string | null;
}

export interface CostComponentUpdatePayload {
  meter_key?: string;
  charge_kind?: string;
  unit_price_nanos?: number | null;
  flat_fee_nanos?: number | null;
  tier_config_json?: string | null;
  match_attributes_json?: string | null;
  priority?: number;
  description?: string | null;
}

export interface CostCatalogListItem {
  catalog: CostCatalog;
  versions: CostCatalogVersion[];
}

export interface CostCatalogVersionDetail {
  version: CostCatalogVersion;
  components: CostComponent[];
}

export interface CostTemplateSummary {
  key: string;
  title: string;
  catalog_name: string;
  description: string;
  currency: string;
  version: string;
  source: string;
  effective_from: number;
  effective_until: number | null;
  tags: string[];
  supported_meters: string[];
  rounding_note: string | null;
}

export interface ImportCostTemplatePayload {
  template_key: string;
  catalog_name?: string | null;
}

export interface ImportCostTemplateResponse {
  template: CostTemplateSummary;
  imported: {
    catalog: CostCatalog;
    version: CostCatalogVersion;
    components: CostComponent[];
    created_catalog: boolean;
  };
}

export interface CostLedgerItem {
  meter_key: string;
  quantity: number;
  unit: string;
  attributes?: Record<string, string>;
}

export interface CostLedger {
  items: CostLedgerItem[];
}

export interface UsageNormalization {
  total_input_tokens: number;
  total_output_tokens: number;
  input_text_tokens: number;
  output_text_tokens: number;
  input_image_tokens: number;
  output_image_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  reasoning_tokens: number;
  warnings: string[];
}

export interface CostDetailLine {
  meter_key: string;
  quantity: number;
  unit: string;
  charge_kind: string;
  amount_nanos: number;
  unit_price_nanos: number | null;
  component_id: number | null;
  catalog_version_id: number | null;
  description: string | null;
  attributes?: Record<string, string>;
}

export interface CostRatingResult {
  total_cost_nanos: number;
  currency: string;
  detail_lines: CostDetailLine[];
  unmatched_items: string[];
  warnings: string[];
}

export interface CostPreviewPayload {
  catalog_version_id: number;
  normalization?: UsageNormalization;
  ledger?: CostLedger;
  total_input_tokens?: number;
}

export interface CostPreviewResponse {
  normalization?: UsageNormalization;
  ledger: CostLedger;
  result: CostRatingResult;
}

export interface CostSnapshot {
  schema_version: number;
  cost_catalog_id: number;
  cost_catalog_version_id: number;
  total_cost_nanos: number;
  currency: string;
  detail_lines: CostDetailLine[];
  unmatched_items: string[];
  warnings: string[];
}

// ========== Request Log Types ==========
export interface RecordListItem {
  id: number;
  api_key_id: number;
  requested_model_name?: string | null;
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
  final_provider_id: number | null;
  final_provider_name_snapshot: string | null;
  final_model_id: number | null;
  final_model_name_snapshot: string | null;
  final_real_model_name_snapshot: string | null;
  estimated_cost_nanos: number | null;
  estimated_cost_currency: string | null;
  total_input_tokens: number | null;
  total_output_tokens: number | null;
  reasoning_tokens: number | null;
  total_tokens: number | null;
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
  output_text_tokens: number | null;
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

export interface RecordListParams {
  page?: number;
  page_size?: number;
  api_key_id?: number;
  provider_id?: number;
  status?: string;
  search?: string;
  [key: string]: string | number | undefined;
}

// ========== Provider CRUD Payloads ==========
export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonObject | JsonValue[];
export interface JsonObject {
  [key: string]: JsonValue;
}

export interface ProviderRemoteModelItem {
  [key: string]: JsonValue | undefined;
  id?: string;
  name?: string;
  owned_by?: string;
}

export type ProviderRemoteModelsResponse =
  | ProviderRemoteModelItem[]
  | {
      data?: ProviderRemoteModelItem[];
      models?: ProviderRemoteModelItem[];
    };

export interface ProviderCheckPayload {
  model_id?: number;
  model_name?: string;
  provider_api_key_id?: number;
  provider_api_key?: string;
}

export interface ProviderBootstrapPayload {
  endpoint: string;
  api_key: string;
  model_name: string;
  provider_type?: string;
  name?: string;
  key?: string;
  real_model_name?: string | null;
  use_proxy?: boolean;
  save_and_test?: boolean;
  api_key_description?: string | null;
}

export interface ProviderBootstrapResponse {
  provider?: ProviderBase;
  created_key?: ProviderApiKeyItem | null;
  created_model?: ModelItem | null;
  provider_name?: string | null;
  provider_key?: string | null;
  check_result?: unknown;
}

export interface ProviderPayload {
  key: string;
  name: string;
  endpoint: string;
  use_proxy: boolean;
  provider_type: string;
  omit_config?: JsonObject | null;
  api_keys?: ProviderKeyPayload[];
}

export interface ProviderKeyPayload {
  api_key: string;
  description?: string | null;
}

// ========== Model CRUD Payloads ==========
export interface ModelPayload {
  provider_id?: number;
  model_name: string;
  real_model_name?: string | null;
  is_enabled: boolean;
  cost_catalog_id?: number | null;
  supports_streaming?: boolean;
  supports_tools?: boolean;
  supports_reasoning?: boolean;
  supports_image_input?: boolean;
  supports_embeddings?: boolean;
  supports_rerank?: boolean;
}

// ========== Paginated Response ==========
export interface PaginatedResponse<T> {
  list: T[];
  total?: number;
}
