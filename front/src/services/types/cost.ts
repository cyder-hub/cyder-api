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
