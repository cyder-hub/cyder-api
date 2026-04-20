import { computed, ref } from "vue";
import { defineStore } from "pinia";

import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import type { ModelSummaryItem } from "./types";
import {
  buildModelNameById,
  buildModelOptions,
  buildModelsByProviderId,
  getModelById as findModelById,
} from "./summaryViewModel";

export const useModelStore = defineStore("model", () => {
  const models = ref<ModelSummaryItem[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function fetchModels() {
    loading.value = true;
    error.value = null;
    try {
      const data = await Api.getModelSummaryList();
      models.value = data || [];
      return models.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch model summaries:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      loading.value = false;
    }
  }

  const modelOptions = computed(() =>
    buildModelOptions(models.value),
  );

  const modelNameById = computed(() => {
    return buildModelNameById(models.value);
  });

  const modelById = (modelId: number | string | null | undefined) =>
    findModelById(models.value, modelId);

  const modelsByProviderId = computed(() => {
    return buildModelsByProviderId(models.value);
  });

  return {
    models,
    loading,
    error,
    modelOptions,
    modelNameById,
    modelById,
    modelsByProviderId,
    fetchModels,
  };
});
