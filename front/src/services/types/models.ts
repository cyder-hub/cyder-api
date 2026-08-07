import type {
  InheritedRequestPatchRule,
  RequestPatchConflict,
  RequestPatchExplainEntry,
  RequestPatchRule,
  ResolvedRequestPatchRule,
} from "./requestPatch";

export type ModelSourceSelectionMode = "INHERIT_ALL" | "EXPLICIT";

export interface ModelSourceBindingPayload {
  source_id: number;
  is_default: boolean;
}

export interface ModelSourceConfigPayload {
  source_selection_mode: ModelSourceSelectionMode;
  bindings: ModelSourceBindingPayload[];
}

export interface ModelSourceBindingSummary extends ModelSourceBindingPayload {
  profile_type: string;
  is_enabled: boolean;
}

export interface ModelSourceConfigSummary {
  source_selection_mode: ModelSourceSelectionMode | string;
  bindings: ModelSourceBindingSummary[];
  declared_source_count: number;
  enabled_source_count: number;
  model_default_source_id: number | null;
  warnings: string[];
}

export interface SourceSelectionTraceEntry {
  key: string;
  source_ids: number[];
}

export interface ModelSourceProtocolExplain {
  downstream_protocol: string;
  selection_status: string;
  source_id: number | null;
  profile_type: string | null;
  upstream_protocol: string | null;
  selection_reason: string | null;
  transform_required: boolean | null;
  decision_trace: SourceSelectionTraceEntry[];
  failure_reason: string | null;
  generation_execution_status: string;
  generation_execution_reason: string;
}

export interface ModelSourceExplain {
  model_id: number;
  provider_id: number;
  source_selection_mode: ModelSourceSelectionMode | string;
  provider_enabled: boolean;
  model_enabled: boolean;
  declared_source_count: number;
  enabled_source_count: number;
  model_default_source_id: number | null;
  warnings: string[];
  protocols: ModelSourceProtocolExplain[];
}

export interface ModelItem {
  id: number;
  model_name: string;
  real_model_name: string | null;
  source_selection_mode: ModelSourceSelectionMode | string;
  source_config: ModelSourceConfigSummary;
  is_enabled: boolean;
}

export interface ModelDetail {
  model: ModelDetailModel;
  request_patches: RequestPatchRule[];
  source_config: ModelSourceConfigSummary;
}

export interface ModelSummaryItem {
  id: number;
  provider_id: number;
  provider_key: string;
  provider_name: string;
  model_name: string;
  real_model_name: string | null;
  source_selection_mode: ModelSourceSelectionMode | string;
  source_config: ModelSourceConfigSummary;
  is_enabled: boolean;
}


export interface ModelDetailModel {
  id: number;
  provider_id: number;
  model_name: string;
  real_model_name: string | null;
  cost_catalog_id: number | null;
  source_selection_mode: ModelSourceSelectionMode | string;
  deleted_at: number | null;
  is_enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface ModelDetailResponse {
  model: ModelDetailModel;
  request_patches: RequestPatchRule[];
  inherited_request_patches: InheritedRequestPatchRule[];
  effective_request_patches: ResolvedRequestPatchRule[];
  request_patch_explain: RequestPatchExplainEntry[];
  request_patch_conflicts: RequestPatchConflict[];
  has_request_patch_conflicts: boolean;
  source_config: ModelSourceConfigSummary;
}


// ========== Model CRUD Payloads ==========
export interface ModelPayload {
  provider_id?: number;
  model_name: string;
  real_model_name?: string | null;
  is_enabled: boolean;
  cost_catalog_id?: number | null;
  source_config?: ModelSourceConfigPayload;
}
