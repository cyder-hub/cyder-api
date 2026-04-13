import type {
  CostCatalogVersion,
  CostComponent,
  CostPreviewResponse,
  CostTemplateSummary,
  UsageNormalization,
} from "@/store/types";

export type ChargeKind = "per_unit" | "flat" | "tiered_per_unit";
export type TierBasis = "meter_quantity" | "total_input_tokens";

export interface TierRowDraft {
  up_to: string;
  unit_price: string;
}

export interface CatalogDraft {
  id: number | null;
  name: string;
  description: string;
}

export interface VersionDraft {
  version: string;
  currency: string;
  source: string;
  effective_from: string;
  effective_until: string;
  is_enabled: boolean;
}

export interface ComponentDraft {
  id: number | null;
  meter_key: string;
  charge_kind: ChargeKind;
  unit_price: string;
  flat_fee: string;
  match_attributes_json: string;
  priority: string;
  description: string;
  tier_basis: TierBasis;
  tiers: TierRowDraft[];
}

export interface PreviewDraft {
  total_input_tokens: string;
  total_output_tokens: string;
  input_text_tokens: string;
  output_text_tokens: string;
  input_image_tokens: string;
  output_image_tokens: string;
  cache_read_tokens: string;
  cache_write_tokens: string;
  reasoning_tokens: string;
}

export interface ParsedTierConfig {
  basis: TierBasis;
  tiers: TierRowDraft[];
}

export interface CostOption<T extends string = string> {
  value: T;
  labelKey: string;
}

export type CostVersionSummary = CostCatalogVersion | null;

export type {
  CostComponent,
  CostPreviewResponse,
  CostTemplateSummary,
  UsageNormalization,
};
