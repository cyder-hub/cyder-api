import type { JsonValue } from "./shared";

export type RequestPatchPlacement = "HEADER" | "QUERY" | "BODY";
export type RequestPatchOperation = "SET" | "REMOVE";
export type RequestPatchVariantOrigin =
  | "SourceBase"
  | "ModelBase"
  | "SourceSuffix"
  | "ModelSuffix";
export type RequestPatchExplainStatus =
  | "Effective"
  | "Overridden"
  | "Conflicted"
  | "Masked"
  | "Dormant";

export interface RequestPatchVariant {
  id: number;
  source_id: number;
  model_id: number | null;
  suffix: string | null;
  enabled: boolean;
  expose_in_models: boolean;
  deleted_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface RequestPatchRule {
  id: number;
  variant_id: number;
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json: string | null;
  description: string | null;
  deleted_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface RequestPatchVariantAggregate {
  variant: RequestPatchVariant;
  rules: RequestPatchRule[];
}

export interface RequestPatchRuleInput {
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json: JsonValue | null;
  description: string | null;
}

export interface RequestPatchVariantInput {
  source_id: number;
  model_id?: number | null;
  suffix?: string | null;
  enabled: boolean;
  expose_in_models: boolean;
  rules: RequestPatchRuleInput[];
}

export interface RequestPatchVariantListResponse {
  source_id: number;
  model_id: number | null;
  variants: RequestPatchVariantAggregate[];
  variant_count: number;
  rule_count: number;
}

export interface ModelRequestPatchOverviewResponse {
  model_id: number;
  variants: RequestPatchVariantAggregate[];
  variant_count: number;
  rule_count: number;
}

export interface RequestPatchPreviewConflict {
  existing_variant_id: number;
  existing_model_id: number | null;
  existing_suffix: string | null;
  placement: RequestPatchPlacement;
  candidate_target: string;
  existing_target: string;
  reason: string;
}

export interface RequestPatchVariantPreview {
  suffix: string | null;
  rule_count: number;
  conflicts: RequestPatchPreviewConflict[];
  affected_model_count: number;
  valid: boolean;
  failure_reason: string | null;
}

export interface RequestPatchPreviewResponse {
  historical_snapshot: boolean;
  preview: RequestPatchVariantPreview;
  evaluation: RequestPatchEvaluation | null;
}

export interface RequestPatchLayerState {
  origin: RequestPatchVariantOrigin;
  variant_id: number | null;
  enabled: boolean;
  rule_count: number;
  expose_in_models: boolean;
  status: RequestPatchExplainStatus;
  reason: string | null;
}

export interface RequestPatchEffectiveRule {
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json: string | null;
  source_variant_id: number;
  source_rule_id: number;
  source_origin: RequestPatchVariantOrigin;
  overridden_rule_ids: number[];
  description: string | null;
}

export interface RequestPatchExplainRule {
  id: number;
  variant_id: number;
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json: string | null;
  description: string | null;
  created_at: number;
  updated_at: number;
}

export interface RequestPatchExplainEntry {
  rule: RequestPatchExplainRule;
  origin: RequestPatchVariantOrigin;
  status: RequestPatchExplainStatus;
  effective_rule_id: number | null;
  conflict_with_rule_ids: number[];
  message: string | null;
}

export interface RequestPatchConflict {
  lower_priority_variant_id: number;
  higher_priority_variant_id: number;
  lower_priority_origin: RequestPatchVariantOrigin;
  higher_priority_origin: RequestPatchVariantOrigin;
  placement: RequestPatchPlacement;
  lower_priority_target: string;
  higher_priority_target: string;
  reason: string;
}

export interface RequestPatchEvaluation {
  source_id: number;
  model_id: number | null;
  suffix: string | null;
  layers: RequestPatchLayerState[];
  effective_rules: RequestPatchEffectiveRule[];
  explain: RequestPatchExplainEntry[];
  conflicts: RequestPatchConflict[];
  has_conflicts: boolean;
  exposed_in_models: boolean;
  executable: boolean;
  failure_reason: string | null;
}

export interface RequestPatchExplainResponse {
  historical_snapshot: boolean;
  source_id: number;
  model_id: number | null;
  suffix: string | null;
  evaluation: RequestPatchEvaluation;
}
