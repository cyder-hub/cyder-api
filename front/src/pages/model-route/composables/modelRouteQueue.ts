import type {
  ModelRouteCandidateDetail,
  ModelRouteCandidatePayload,
  ModelRouteDetail,
} from "../../../services/types/modelRoutes";
import type {
  EditingCandidate,
  EditingRoute,
  ModelRouteQueueValidationResult,
} from "../types";

export const MODEL_ROUTE_PRIORITY_STEP = 10;

const defaultLocalIdFactory = () => `${Date.now()}-${Math.random()}`;

export function asModelRouteSelectValue(value: unknown): string | null {
  if (value === null || value === undefined) {
    return null;
  }

  const normalizedValue = String(value);
  return normalizedValue.length > 0 ? normalizedValue : null;
}

export function normalizeModelRouteCandidatePriorities(
  candidates: EditingCandidate[],
): EditingCandidate[] {
  return candidates.map((candidate, index) => ({
    ...candidate,
    priority: index * MODEL_ROUTE_PRIORITY_STEP,
  }));
}

export function createEditingCandidate(
  overrides: Partial<EditingCandidate> = {},
  localIdFactory: () => string = defaultLocalIdFactory,
): EditingCandidate {
  return {
    local_id: overrides.local_id ?? localIdFactory(),
    provider_id: overrides.provider_id ?? null,
    model_id: overrides.model_id ?? null,
    is_enabled: overrides.is_enabled ?? true,
    priority: overrides.priority ?? 0,
  };
}

export function createModelRouteTemplate(
  localIdFactory: () => string = defaultLocalIdFactory,
): EditingRoute {
  return {
    id: null,
    route_name: "",
    description: "",
    is_enabled: true,
    expose_in_models: true,
    candidates: [createEditingCandidate({}, localIdFactory)],
  };
}

export function sortModelRouteCandidateDetails(
  candidates: ModelRouteCandidateDetail[],
): ModelRouteCandidateDetail[] {
  return [...candidates].sort((left, right) => {
    return (
      left.candidate.priority - right.candidate.priority ||
      left.candidate.id - right.candidate.id
    );
  });
}

export function mapModelRouteDetailToEditingRoute(
  detail: ModelRouteDetail,
): EditingRoute {
  return {
    id: detail.route.id,
    route_name: detail.route.route_name,
    description: detail.route.description || "",
    is_enabled: detail.route.is_enabled,
    expose_in_models: detail.route.expose_in_models,
    candidates: sortModelRouteCandidateDetails(detail.candidates).map((candidate) => ({
      local_id: `${candidate.candidate.id}`,
      provider_id: String(candidate.provider_id),
      model_id: String(candidate.candidate.model_id),
      is_enabled: candidate.candidate.is_enabled,
      priority: candidate.candidate.priority,
    })),
  };
}

export function addModelRouteCandidate(
  candidates: EditingCandidate[],
  candidate: EditingCandidate,
): EditingCandidate[] {
  return normalizeModelRouteCandidatePriorities([...candidates, candidate]);
}

export function removeModelRouteCandidate(
  candidates: EditingCandidate[],
  index: number,
): EditingCandidate[] {
  if (index < 0 || index >= candidates.length) {
    return candidates;
  }

  return normalizeModelRouteCandidatePriorities(
    candidates.filter((_, candidateIndex) => candidateIndex !== index),
  );
}

export function moveModelRouteCandidate(
  candidates: EditingCandidate[],
  index: number,
  delta: -1 | 1,
): EditingCandidate[] {
  const targetIndex = index + delta;
  if (targetIndex < 0 || targetIndex >= candidates.length) {
    return candidates;
  }

  const next = [...candidates];
  const [candidate] = next.splice(index, 1);
  if (!candidate) {
    return candidates;
  }

  next.splice(targetIndex, 0, candidate);
  return normalizeModelRouteCandidatePriorities(next);
}

export function setModelRouteCandidateProvider(
  candidates: EditingCandidate[],
  index: number,
  providerId: string | null,
): EditingCandidate[] {
  return candidates.map((candidate, candidateIndex) =>
    candidateIndex === index
      ? {
          ...candidate,
          provider_id: providerId,
          model_id: null,
        }
      : candidate,
  );
}

export function setModelRouteCandidateModel(
  candidates: EditingCandidate[],
  index: number,
  modelId: string | null,
): EditingCandidate[] {
  return candidates.map((candidate, candidateIndex) =>
    candidateIndex === index
      ? {
          ...candidate,
          model_id: modelId,
        }
      : candidate,
  );
}

export function setModelRouteCandidateEnabled(
  candidates: EditingCandidate[],
  index: number,
  isEnabled: boolean,
): EditingCandidate[] {
  return candidates.map((candidate, candidateIndex) =>
    candidateIndex === index
      ? {
          ...candidate,
          is_enabled: isEnabled,
        }
      : candidate,
  );
}

export function buildModelRouteCandidatePayload(
  candidates: EditingCandidate[],
): ModelRouteCandidatePayload[] {
  return normalizeModelRouteCandidatePriorities(candidates).map(
    (candidate) => ({
      model_id: Number(candidate.model_id),
      priority: candidate.priority,
      is_enabled: candidate.is_enabled,
    }),
  );
}

export function buildModelRoutePayload(route: EditingRoute) {
  return {
    route_name: route.route_name.trim(),
    description: route.description.trim() || null,
    is_enabled: route.is_enabled,
    expose_in_models: route.expose_in_models,
    candidates: buildModelRouteCandidatePayload(route.candidates),
  };
}

export function validateModelRouteEditor(
  route: EditingRoute,
): ModelRouteQueueValidationResult {
  if (!route.route_name.trim()) {
    return { valid: false, issue: "route_name_required" };
  }

  if (route.candidates.length === 0) {
    return { valid: false, issue: "candidate_required" };
  }

  if (route.candidates.some((candidate) => !candidate.model_id)) {
    return { valid: false, issue: "candidate_model_required" };
  }

  const seenModelIds = new Set<string>();
  for (const candidate of route.candidates) {
    if (!candidate.model_id) {
      continue;
    }
    if (seenModelIds.has(candidate.model_id)) {
      return { valid: false, issue: "duplicate_candidate" };
    }
    seenModelIds.add(candidate.model_id);
  }

  return { valid: true, issue: null };
}
