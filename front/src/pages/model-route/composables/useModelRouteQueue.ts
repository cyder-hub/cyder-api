import type { Ref } from "vue";
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import { useModelStore } from "@/store/modelStore";
import { useProviderStore } from "@/store/providerStore";
import type { EditingRoute, ModelRouteOption } from "../types";
import {
  addModelRouteCandidate,
  asModelRouteSelectValue,
  createEditingCandidate,
  moveModelRouteCandidate,
  removeModelRouteCandidate,
  setModelRouteCandidateEnabled,
  setModelRouteCandidateModel,
  setModelRouteCandidateProvider,
} from "./modelRouteQueue";

export function useModelRouteQueue(editingRoute: Ref<EditingRoute>) {
  const { t } = useI18n();
  const providerStore = useProviderStore();
  const modelStore = useModelStore();

  const providerOptions = computed<ModelRouteOption[]>(() =>
    providerStore.providerOptions.map((provider) => ({
      ...provider,
      value: String(provider.value),
    })),
  );

  const getProviderById = (providerId: string | null) =>
    providerStore.getProviderById(providerId);

  const getModelOptions = (providerId: string | null): ModelRouteOption[] => {
    const provider = getProviderById(providerId);
    if (!provider) {
      return [];
    }

    return modelStore.models
      .filter((item) => String(item.provider_id) === String(provider.id))
      .map((item) => ({
        value: String(item.id),
        label: item.real_model_name
          ? `${item.model_name} -> ${item.real_model_name}`
          : item.model_name,
        is_enabled: item.is_enabled,
      }));
  };

  const ensureRouteModalDependencies = async () => {
    await Promise.all([
      providerStore.providers.length > 0
        ? Promise.resolve(providerStore.providers)
        : providerStore.fetchProviders(),
      modelStore.models.length > 0
        ? Promise.resolve(modelStore.models)
        : modelStore.fetchModels(),
    ]);
  };

  const getCandidateSummary = (candidate: { provider_id: string | null; model_id: string | null }) => {
    const provider = getProviderById(candidate.provider_id);
    const model = modelStore.modelById(candidate.model_id);

    if (!provider || !model) {
      return t("modelRoutePage.modal.emptyCandidates");
    }

    return `${provider.provider_key}/${model.real_model_name || model.model_name}`;
  };

  const addCandidate = () => {
    editingRoute.value.candidates = addModelRouteCandidate(
      editingRoute.value.candidates,
      createEditingCandidate(),
    );
  };

  const removeCandidate = (index: number) => {
    editingRoute.value.candidates = removeModelRouteCandidate(
      editingRoute.value.candidates,
      index,
    );
  };

  const moveCandidate = (index: number, delta: -1 | 1) => {
    editingRoute.value.candidates = moveModelRouteCandidate(
      editingRoute.value.candidates,
      index,
      delta,
    );
  };

  const setCandidateProvider = (index: number, value: unknown) => {
    editingRoute.value.candidates = setModelRouteCandidateProvider(
      editingRoute.value.candidates,
      index,
      asModelRouteSelectValue(value),
    );
  };

  const setCandidateModel = (index: number, value: unknown) => {
    editingRoute.value.candidates = setModelRouteCandidateModel(
      editingRoute.value.candidates,
      index,
      asModelRouteSelectValue(value),
    );
  };

  const setCandidateEnabled = (index: number, isEnabled: boolean) => {
    editingRoute.value.candidates = setModelRouteCandidateEnabled(
      editingRoute.value.candidates,
      index,
      isEnabled,
    );
  };

  return {
    providerOptions,
    ensureRouteModalDependencies,
    getModelOptions,
    getCandidateSummary,
    addCandidate,
    removeCandidate,
    moveCandidate,
    setCandidateProvider,
    setCandidateModel,
    setCandidateEnabled,
  };
}
