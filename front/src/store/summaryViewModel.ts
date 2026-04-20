import type { ModelSummaryItem, ProviderSummaryItem } from "./types";

export const buildProviderOptions = (providers: ProviderSummaryItem[]) =>
  providers.map((provider) => ({
    value: provider.id,
    label: `${provider.name} (${provider.provider_key})`,
    isEnabled: provider.is_enabled,
  }));

export const buildProviderNameById = (providers: ProviderSummaryItem[]) => {
  const map = new Map<number, string>();
  for (const provider of providers) {
    map.set(provider.id, `${provider.name} (${provider.provider_key})`);
  }
  return map;
};

export const getProviderById = (
  providers: ProviderSummaryItem[],
  providerId: number | string | null | undefined,
) => providers.find((provider) => String(provider.id) === String(providerId));

export const buildModelOptions = (models: ModelSummaryItem[]) =>
  models.map((model) => ({
    value: model.id,
    label: `${model.provider_key} / ${model.model_name}`,
    providerId: model.provider_id,
    providerName: model.provider_name,
    isEnabled: model.is_enabled,
  }));

export const buildModelNameById = (models: ModelSummaryItem[]) => {
  const map = new Map<number, string>();
  for (const model of models) {
    map.set(model.id, `${model.provider_key} / ${model.model_name}`);
  }
  return map;
};

export const getModelById = (
  models: ModelSummaryItem[],
  modelId: number | string | null | undefined,
) => models.find((model) => String(model.id) === String(modelId));

export const buildModelsByProviderId = (models: ModelSummaryItem[]) => {
  const map = new Map<number, ModelSummaryItem[]>();
  for (const model of models) {
    const items = map.get(model.provider_id) ?? [];
    items.push(model);
    map.set(model.provider_id, items);
  }
  return map;
};
