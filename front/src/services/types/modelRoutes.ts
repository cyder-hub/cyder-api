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
