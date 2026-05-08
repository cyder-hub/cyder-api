import type { JsonValue } from "./shared";

// ========== Request Patch Types ==========
export type RequestPatchPlacement = "HEADER" | "QUERY" | "BODY";

export type RequestPatchOperation = "SET" | "REMOVE";

export type RequestPatchScopeKind = "PROVIDER" | "MODEL";

export type RequestPatchRuleOrigin = "ProviderDirect" | "ModelDirect";

export type ReasoningPresetKey =
  | "disabled"
  | "enabled"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "auto"
  | string;

export type ReasoningPatchFamilyKey = string;

export type ReasoningConfigScope = "provider" | "model" | string;

export type ReasoningConfigSource =
  | "provider_default"
  | "model_custom"
  | "model_disabled"
  | "missing"
  | string;

export interface ReasoningPresetMetadata {
  preset_key: ReasoningPresetKey;
  suffix: string;
  requires_reasoning: boolean;
  allowed_operation_kinds: string[];
}

export interface ReasoningFamilyMetadata {
  family_key: ReasoningPatchFamilyKey;
  supported_presets: ReasoningPresetKey[];
  target_api_types: string[];
}

export type ReasoningConfigMode = "custom" | "disabled" | string;

export type ReasoningConfigOwnerStatus =
  | "custom"
  | "disabled"
  | "inherited"
  | "missing"
  | string;

export type ModelReasoningConfigWriteMode = "inherit" | "disabled" | "custom";

export interface ReasoningConfigCatalog {
  families: ReasoningFamilyMetadata[];
  presets: ReasoningPresetMetadata[];
}

export interface ReasoningConfigPreset {
  id: number;
  config_id: number;
  preset_key: ReasoningPresetKey;
  suffix: string;
  requires_reasoning: boolean;
  allowed_operation_kinds: string[];
  expose_in_models: boolean;
  is_enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface ReasoningConfigDetail {
  id: number;
  scope_kind: ReasoningConfigScope;
  provider_id: number | null;
  model_id: number | null;
  mode: ReasoningConfigMode;
  family_key: ReasoningPatchFamilyKey | null;
  presets: ReasoningConfigPreset[];
  created_at: number;
  updated_at: number;
}

export interface ReasoningConfigResponse {
  owner_kind: ReasoningConfigScope;
  owner_id: number;
  owner_config: ReasoningConfigDetail | null;
  provider_config: ReasoningConfigDetail | null;
  effective_config: ReasoningConfigDetail | null;
  effective_source: ReasoningConfigSource;
  status: ReasoningConfigOwnerStatus;
}

export interface ReasoningGeneratedPatchPreview {
  placement: RequestPatchPlacement | string;
  target: string;
  operation: RequestPatchOperation | string;
  value_json: JsonValue | null;
  description: string | null;
}

export interface ReasoningConfigPreviewPreset {
  preset_key: ReasoningPresetKey;
  suffix: string;
  requires_reasoning: boolean;
  allowed_operation_kinds: string[];
  family_supported: boolean;
  enabled: boolean;
  expose_in_models: boolean;
  runtime_supported: boolean;
  unsupported_reason: string | null;
  generated_patches: ReasoningGeneratedPatchPreview[];
}

export interface ReasoningConfigPreview {
  config: ReasoningConfigResponse;
  target_api_type: string;
  presets: ReasoningConfigPreviewPreset[];
}

export interface ReasoningConfigPresetPayload {
  preset_key: ReasoningPresetKey;
  expose_in_models: boolean;
  is_enabled: boolean;
}

export interface ProviderReasoningConfigPayload {
  family_key: ReasoningPatchFamilyKey;
  presets: ReasoningConfigPresetPayload[];
}

export interface ProviderReasoningConfigPreviewPayload {
  provider_type?: string | null;
  family_key?: ReasoningPatchFamilyKey | null;
  presets?: ReasoningConfigPresetPayload[];
}

export interface ModelReasoningConfigPayload {
  mode: ModelReasoningConfigWriteMode;
  family_key?: ReasoningPatchFamilyKey | null;
  presets?: ReasoningConfigPresetPayload[];
}

export interface ReasoningRouteCandidatePreview {
  candidate_position: number;
  runtime_status: "valid" | "stale_skipped";
  provider_id: number | null;
  provider_key: string | null;
  model_id: number;
  model_name: string | null;
  preset_key: ReasoningPresetKey;
  suffix: string;
  supported: boolean;
  reason: string | null;
  config_source: ReasoningConfigSource | null;
  config_scope: ReasoningConfigScope | null;
  config_id: number | null;
  config_preset_id: number | null;
  family: ReasoningPatchFamilyKey | null;
}

export interface ReasoningRoutePresetPreview {
  preset_key: ReasoningPresetKey;
  suffix: string;
  requires_reasoning: boolean;
  allowed_operation_kinds: string[];
  stable: boolean;
  reason: string | null;
  candidates: ReasoningRouteCandidatePreview[];
}

export interface ReasoningRoutePreview {
  route_id: number;
  route_name: string;
  presets: ReasoningRoutePresetPreview[];
}

export type RequestPatchSource =
  | {
      kind: "provider_rule";
      rule_id: number;
    }
  | {
      kind: "model_rule";
      rule_id: number;
    }
  | {
      kind: "reasoning_preset";
      config_id: number;
      config_scope: ReasoningConfigScope;
      config_preset_id: number;
      family: ReasoningPatchFamilyKey;
      preset: ReasoningPresetKey;
      suffix: string;
    }
  | {
      kind: "reasoning_preset";
      profile_id: number;
      profile_preset_id: number;
      family: ReasoningPatchFamilyKey;
      preset: ReasoningPresetKey;
      suffix: string;
    }
  | {
      kind: string;
      [key: string]: unknown;
    };

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

export interface RuntimeResolvedRequestPatchRule {
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json: string | null;
  source: RequestPatchSource;
  source_rule_id: number | null;
  source_origin: RequestPatchRuleOrigin | null;
  overridden_rule_ids: number[];
  overridden_sources: RequestPatchSource[];
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

export interface RuntimeRequestPatchConflict {
  placement: RequestPatchPlacement;
  lower_priority_source: RequestPatchSource;
  higher_priority_source: RequestPatchSource;
  lower_priority_target: string;
  higher_priority_target: string;
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


export interface RequestPatchPayload {
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json?: JsonValue | null;
  description?: string | null;
  is_enabled?: boolean;
  confirm_dangerous_target?: boolean;
}

export interface DangerousRequestPatchSavePayload extends RequestPatchPayload {
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
  effective_rules: RuntimeResolvedRequestPatchRule[];
  explain: RequestPatchExplainEntry[];
  conflicts: RuntimeRequestPatchConflict[];
  has_conflicts: boolean;
}
