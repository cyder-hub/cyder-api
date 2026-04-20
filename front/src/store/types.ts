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
  system_api_key_count: number;
  enabled_system_api_key_count: number;
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
  active_system_api_key_count: number;
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
  system_api_key_id: number | null;
  provider_key: string | null;
  model_name: string | null;
  real_model_name: string | null;
  system_api_key_name: string | null;
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

export interface ProviderApiKeyItem {
  id: number;
  api_key: string;
  description: string | null;
}

export interface ModelItem {
  id: number;
  model_name: string;
  real_model_name: string;
}

export interface ModelDetail {
  model: ModelItem;
  custom_fields: CustomFieldItem[];
}

export interface ProviderListItem {
  provider: ProviderBase;
  models: ModelDetail[];
  provider_keys: ProviderApiKeyItem[];
  custom_fields: CustomFieldDefinition[];
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

// ========== Custom Field Types ==========
export type CustomFieldType =
  | "unset"
  | "text"
  | "integer"
  | "float"
  | "boolean";

export interface CustomFieldItem {
  id?: number;
  name?: string | null;
  field_name: string;
  field_value: string;
  description?: string | null;
  field_type: CustomFieldType;
}

export interface CustomFieldDefinition {
  id: number;
  name: string | null;
  description: string | null;
  field_name: string;
  field_placement: string;
  field_type: string;
  string_value: string | null;
  integer_value: number | null;
  number_value: number | null;
  boolean_value: boolean | null;
  is_enabled: boolean;
}

export interface ModelDetailModel {
  id: number;
  provider_id: number;
  model_name: string;
  real_model_name: string | null;
  cost_catalog_id: number | null;
  deleted_at: number | null;
  is_enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface ModelDetailResponse {
  model: ModelDetailModel;
  custom_fields: CustomFieldDefinition[];
}

export interface CustomFieldPayload {
  name?: string | null;
  description?: string | null;
  field_name: string;
  field_placement: string;
  field_type: string;
  string_value?: string | null;
  integer_value?: number | null;
  number_value?: number | null;
  boolean_value?: boolean | null;
  is_enabled: boolean;
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
  provider_id: number;
  system_api_key_id: number;
  requested_model_name?: string | null;
  resolved_name_scope?: string | null;
  resolved_route_name?: string | null;
  model_name: string;
  is_stream: boolean;
  status: string | null;
  total_input_tokens: number | null;
  total_output_tokens: number | null;
  reasoning_tokens: number | null;
  total_tokens: number | null;
  estimated_cost_nanos: number | null;
  estimated_cost_currency: string | null;
  request_received_at: number;
  llm_request_sent_at: number;
  llm_response_first_chunk_at: number | null;
  llm_response_completed_at: number | null;
}

export interface RecordDetail extends RecordListItem {
  cost_catalog_id?: number | null;
  cost_catalog_version_id?: number | null;
  resolved_route_id?: number | null;
  real_model_name?: string | null;
  input_text_tokens?: number | null;
  output_text_tokens?: number | null;
  input_image_tokens?: number | null;
  output_image_tokens?: number | null;
  cache_read_tokens?: number | null;
  cache_write_tokens?: number | null;
  request_headers: string | null;
  response_headers: string | null;
  storage_type: string | null;
  error_message: string | null;
  user_api_type?: string | null;
  llm_api_type?: string | null;
  response_sent_to_client_at: number | null;
  cost_snapshot_json?: string | null;
  user_request_body?: string | null;
  llm_request_body?: string | null;
  llm_response_body?: string | null;
  user_response_body?: string | null;
}

export interface RecordListParams {
  page?: number;
  page_size?: number;
  system_api_key_id?: number;
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
}

// ========== Custom Field Link Payloads ==========
export interface CustomFieldLinkPayload {
  custom_field_definition_id: number;
  provider_id?: number;
  model_id?: number;
  is_enabled?: boolean;
}

export interface CustomFieldUnlinkPayload {
  custom_field_definition_id: number;
  provider_id?: number;
  model_id?: number;
}

// ========== Paginated Response ==========
export interface PaginatedResponse<T> {
  list: T[];
  total?: number;
}
