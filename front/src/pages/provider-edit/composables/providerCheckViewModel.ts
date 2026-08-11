import type { ProviderCheckResponse } from "@/services/types";
import type { EditingProviderSource, LocalEditableModelItem, LocalProviderApiKeyItem } from "../types";

export type CheckDialogKind = "model" | "apiKey" | "source";

export interface CheckOption {
  value: number;
  label: string;
}

export interface CheckOptionsResult {
  options: CheckOption[];
  defaultSelectedValue: null;
}

export function buildCheckOptions<T>(
  items: T[],
  getLabel: (item: T, index: number) => string,
): CheckOptionsResult {
  return {
    options: items.map((item, index) => ({
      value: index,
      label: `#${index + 1} ${getLabel(item, index)}`,
    })),
    defaultSelectedValue: null,
  };
}

export function formatCheckSourceEvidence(
  result: ProviderCheckResponse,
): string {
  const keyEvidence =
    result.provider_api_key_id === null
      ? "draft"
      : `key #${result.provider_api_key_id}`;
  return `${result.profile_type} · source #${result.source_id} · ${keyEvidence}`;
}

export function buildEnabledSourceOptions(
  sources: EditingProviderSource[],
  model?: LocalEditableModelItem | null,
): CheckOption[] {
  return sources
    .filter(
      (source) =>
        source.deleted_at === null &&
        source.is_enabled &&
        modelAllowsSource(model, source.id),
    )
    .map((source) => ({
      value: source.id,
      label: `${source.profile_type} · #${source.id}`,
    }));
}

export function modelAllowsSource(
  model: LocalEditableModelItem | null | undefined,
  sourceId: number,
): boolean {
  if (model?.source_config?.source_selection_mode !== "EXPLICIT") {
    return true;
  }
  return model.source_config.bindings.some((binding) => binding.source_id === sourceId);
}

export function buildEnabledModelOptions(
  models: LocalEditableModelItem[],
  getLabel: (model: LocalEditableModelItem, index: number) => string,
  sourceId: number | null = null,
): CheckOption[] {
  return models
    .map((model, index) => ({ model, index }))
    .filter(
      ({ model }) =>
        model.id !== null &&
        model.is_enabled &&
        (sourceId === null || modelAllowsSource(model, sourceId)),
    )
    .map(({ model, index }, optionIndex) => ({
      value: index,
      label: `#${optionIndex + 1} ${getLabel(model, index)}`,
    }));
}

export function buildEnabledApiKeyOptions(
  keys: LocalProviderApiKeyItem[],
  getLabel: (key: LocalProviderApiKeyItem, index: number) => string,
): CheckOption[] {
  return buildCheckOptions(
    keys
      .map((key, index) => ({ key, index }))
      .filter(({ key }) => key.is_enabled),
    ({ key, index }) => getLabel(key, index),
  ).options.map((option, optionIndex) => {
    const source = keys
      .map((key, index) => ({ key, index }))
      .filter(({ key }) => key.is_enabled)[optionIndex];
    return { ...option, value: source.index };
  });
}

export function resolveAutomaticSource(
  sources: EditingProviderSource[],
  model?: LocalEditableModelItem | null,
): { status: "none" | "selected" | "prompt"; sourceId: number | null } {
  const enabled = sources.filter(
    (source) =>
      source.deleted_at === null &&
      source.is_enabled &&
      modelAllowsSource(model, source.id),
  );
  if (enabled.length === 0) return { status: "none", sourceId: null };
  if (enabled.length === 1) return { status: "selected", sourceId: enabled[0].id };

  if (model?.source_config?.source_selection_mode === "EXPLICIT") {
    const modelDefault = model.source_config.bindings.find(
      (binding) => binding.is_default,
    );
    const modelDefaultSource = modelDefault
      ? enabled.find((source) => source.id === modelDefault.source_id)
      : undefined;
    if (modelDefaultSource) {
      return { status: "selected", sourceId: modelDefaultSource.id };
    }
    return { status: "prompt", sourceId: null };
  }

  const providerDefault = enabled.find((source) => source.is_default);
  if (providerDefault) return { status: "selected", sourceId: providerDefault.id };
  return { status: "prompt", sourceId: null };
}
