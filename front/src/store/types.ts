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

export interface UsageStatItem {
  provider_id: number | null;
  model_id: number | null;
  provider_key: string | null;
  model_name: string | null;
  real_model_name: string | null;
  total_input_tokens: number;
  total_output_tokens: number;
  total_reasoning_tokens: number;
  total_tokens: number;
  request_count: number;
  total_cost: Record<string, number>;
}

export interface UsageStatsPeriod {
  time: number;
  data: UsageStatItem[];
}

// ========== API Key Types ==========
export interface ApiKeyItem {
  id: number;
  name: string;
  api_key: string;
  description: string;
  is_enabled: boolean;
  access_control_policy_id?: number | null;
  access_control_policy_name?: string | null;
  created_at: number;
  updated_at: number;
  created_at_formatted?: string;
  updated_at_formatted?: string;
}

export interface ApiKeyCreatePayload {
  name: string;
  description?: string;
  is_enabled: boolean;
  access_control_policy_id: number | null;
}

export interface ApiKeyUpdatePayload extends ApiKeyCreatePayload {
  api_key?: string;
}

export interface IssueTokenPayload {
  uid: string;
  channel?: string;
  duration?: number;
  end_at?: number;
}

// ========== Access Control Types ==========
export interface AccessControlRule {
  id: number;
  policy_id: number;
  rule_type: string;
  priority: number;
  scope: string;
  provider_id: number | null;
  model_id: number | null;
  is_enabled: boolean;
  description: string | null;
  created_at: number;
  updated_at: number;
  deleted_at: number | null;
}

export interface AccessControlPolicyBase {
  name: string;
  default_action: string;
  description: string | null;
}

export interface AccessControlPolicyFromAPI extends AccessControlPolicyBase {
  id: number;
  created_at: number;
  updated_at: number;
  rules: AccessControlRule[];
}

export interface AccessControlRulePayload {
  rule_type: string;
  priority: number;
  scope: string;
  provider_id: number | null;
  model_id: number | null;
  description: string | null;
  is_enabled: boolean;
}

export interface AccessControlPayload {
  name: string;
  default_action: string;
  description: string | null;
  rules: AccessControlRulePayload[];
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

// ========== Model Alias Types ==========
export interface ModelAliasListItem {
  id: number;
  alias_name: string;
  provider_key: string;
  model_name: string;
  target_model_id: number;
  is_enabled: boolean;
  description: string | null;
  priority: number;
}

export interface EditingModelAlias {
  id: number | null;
  alias_name: string;
  provider_id: string | null;
  target_model_id: string | null;
  is_enabled: boolean;
}

export interface ModelAliasDetail {
  id: number;
  alias_name: string;
  target_model_id: number;
  is_enabled: boolean;
}

export interface ModelAliasPayload {
  alias_name: string;
  target_model_id: number;
  is_enabled: boolean;
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
