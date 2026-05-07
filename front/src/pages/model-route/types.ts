import type {
  ModelRouteCandidatePayload,
  ModelRouteDetail,
  ModelRouteListItem,
  ModelSummaryItem,
  ProviderSummaryItem,
} from "@/services/types";

export type CandidateMoveDelta = -1 | 1;

export interface EditingCandidate {
  local_id: string;
  provider_id: string | null;
  model_id: string | null;
  is_enabled: boolean;
  priority: number;
}

export interface EditingRoute {
  id: number | null;
  route_name: string;
  description: string;
  is_enabled: boolean;
  expose_in_models: boolean;
  candidates: EditingCandidate[];
}

export interface ModelRouteOption {
  value: string;
  label: string;
  is_enabled?: boolean;
}

export interface ModelRouteSummaryCard {
  key: string;
  label: string;
  value: number;
}

export interface ModelRouteQueueValidationResult {
  valid: boolean;
  issue:
    | "route_name_required"
    | "candidate_required"
    | "candidate_model_required"
    | "duplicate_candidate"
    | null;
}

export interface ModelRouteQueueApi {
  addCandidate: () => void;
  removeCandidate: (index: number) => void;
  moveCandidate: (index: number, delta: CandidateMoveDelta) => void;
  setCandidateProvider: (index: number, value: unknown) => void;
  setCandidateModel: (index: number, value: unknown) => void;
  setCandidateEnabled: (index: number, isEnabled: boolean) => void;
  getModelOptions: (providerId: string | null) => ModelRouteOption[];
  getCandidateSummary: (candidate: EditingCandidate) => string;
}

export type ModelRouteEditorDependencies = {
  getRouteDetail: (id: number) => Promise<ModelRouteDetail>;
  createRoute: (payload: {
    route_name: string;
    description?: string | null;
    is_enabled?: boolean;
    expose_in_models?: boolean;
    candidates: ModelRouteCandidatePayload[];
  }) => Promise<ModelRouteDetail>;
  updateRoute: (
    id: number,
    payload: {
      route_name?: string;
      description?: string | null;
      is_enabled?: boolean;
      expose_in_models?: boolean;
      candidates?: ModelRouteCandidatePayload[];
    },
  ) => Promise<ModelRouteDetail>;
};

export type ModelRouteListDependencies = {
  getRouteList: () => Promise<ModelRouteListItem[]>;
  updateRoute: (
    id: number,
    payload: {
      is_enabled?: boolean;
      expose_in_models?: boolean;
    },
  ) => Promise<ModelRouteDetail>;
  deleteRoute: (id: number) => Promise<void>;
};

export type ModelRouteProviderItem = ProviderSummaryItem;
export type ModelRouteModelItem = ModelSummaryItem;
