import type {
  ModelSourceBindingPayload,
  ModelSourceConfigPayload,
  ModelSourceConfigSummary,
  ModelSourceSelectionMode,
} from "@/services/types";
import type { UpstreamSource } from "@/services/types";

export interface SourceConfigDraft {
  source_selection_mode: ModelSourceSelectionMode;
  bindings: ModelSourceBindingPayload[];
}

export interface SourceConfigSourceOption {
  id: number;
  profile_type: string;
  is_enabled: boolean;
  is_default: boolean;
}

export function visibleSourceOptions(
  sources: UpstreamSource[] = [],
): SourceConfigSourceOption[] {
  return sources
    .filter((source) => source.deleted_at === null)
    .map((source) => ({
      id: source.id,
      profile_type: source.profile_type,
      is_enabled: source.is_enabled,
      is_default: source.is_default,
    }))
    .sort((left, right) => left.id - right.id);
}

export function createSourceConfigDraft(
  summary?: ModelSourceConfigSummary | null,
  sources: UpstreamSource[] = [],
): SourceConfigDraft {
  const mode = summary?.source_selection_mode === "EXPLICIT" ? "EXPLICIT" : "INHERIT_ALL";
  if (mode === "INHERIT_ALL") {
    return { source_selection_mode: mode, bindings: [] };
  }

  const visibleIds = new Set(visibleSourceOptions(sources).map((source) => source.id));
  return {
    source_selection_mode: mode,
    bindings: (summary?.bindings ?? [])
      .filter((binding) => visibleIds.size === 0 || visibleIds.has(binding.source_id))
      .map((binding) => ({
        source_id: binding.source_id,
        is_default: binding.is_default,
      }))
      .sort((left, right) => left.source_id - right.source_id),
  };
}

export function applySourceSelectionMode(
  draft: SourceConfigDraft,
  nextMode: ModelSourceSelectionMode,
  sources: UpstreamSource[] = [],
): SourceConfigDraft {
  if (nextMode === draft.source_selection_mode) {
    return cloneSourceConfigDraft(draft);
  }
  if (nextMode === "INHERIT_ALL") {
    return { source_selection_mode: nextMode, bindings: [] };
  }

  const options = visibleSourceOptions(sources);
  const providerDefault = options.find((source) => source.is_default)?.id ?? null;
  return {
    source_selection_mode: nextMode,
    bindings: options.map((source) => ({
      source_id: source.id,
      is_default: source.id === providerDefault,
    })),
  };
}

export function toggleSourceBinding(
  draft: SourceConfigDraft,
  sourceId: number,
  checked: boolean,
): SourceConfigDraft {
  if (draft.source_selection_mode !== "EXPLICIT") {
    return cloneSourceConfigDraft(draft);
  }

  const bindings = draft.bindings.filter((binding) => binding.source_id !== sourceId);
  if (checked) {
    bindings.push({ source_id: sourceId, is_default: false });
  }
  return {
    source_selection_mode: draft.source_selection_mode,
    bindings: bindings.sort((left, right) => left.source_id - right.source_id),
  };
}

export function setSourceDefault(
  draft: SourceConfigDraft,
  sourceId: number,
): SourceConfigDraft {
  if (draft.source_selection_mode !== "EXPLICIT") {
    return cloneSourceConfigDraft(draft);
  }
  const hasSource = draft.bindings.some((binding) => binding.source_id === sourceId);
  if (!hasSource) {
    return cloneSourceConfigDraft(draft);
  }
  return {
    source_selection_mode: draft.source_selection_mode,
    bindings: draft.bindings
      .map((binding) => ({
        source_id: binding.source_id,
        is_default: binding.source_id === sourceId,
      }))
      .sort((left, right) => left.source_id - right.source_id),
  };
}

export function clearSourceDefault(draft: SourceConfigDraft): SourceConfigDraft {
  if (draft.source_selection_mode !== "EXPLICIT") {
    return cloneSourceConfigDraft(draft);
  }
  return {
    source_selection_mode: draft.source_selection_mode,
    bindings: draft.bindings
      .map((binding) => ({
        source_id: binding.source_id,
        is_default: false,
      }))
      .sort((left, right) => left.source_id - right.source_id),
  };
}

export function toSourceConfigPayload(
  draft: SourceConfigDraft,
): ModelSourceConfigPayload {
  return {
    source_selection_mode: draft.source_selection_mode,
    bindings: draft.bindings
      .map((binding) => ({
        source_id: binding.source_id,
        is_default: binding.is_default,
      }))
      .sort((left, right) => left.source_id - right.source_id),
  };
}

export function sourceConfigPayloadEquals(
  left: ModelSourceConfigPayload,
  right: ModelSourceConfigPayload,
): boolean {
  return JSON.stringify(toSourceConfigPayload(left)) === JSON.stringify(toSourceConfigPayload(right));
}

export function cloneSourceConfigDraft(draft: SourceConfigDraft): SourceConfigDraft {
  return {
    source_selection_mode: draft.source_selection_mode,
    bindings: draft.bindings.map((binding) => ({
      source_id: binding.source_id,
      is_default: binding.is_default,
    })),
  };
}

export function sourceConfigWarningKey(warning: string): string {
  return `modelSourceConfig.warnings.${warning}`;
}
