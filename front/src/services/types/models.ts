import type {
  InheritedRequestPatchRule,
  RequestPatchConflict,
  RequestPatchExplainEntry,
  RequestPatchRule,
  ResolvedRequestPatchRule,
} from "./requestPatch";

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
