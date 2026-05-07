import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { normalizeError } from "@/utils/error";
import * as providerService from "@/services/providers";
import type { ProviderSummaryItem } from "@/services/types";
import {
  buildProviderNameById,
  buildProviderOptions,
  getProviderById as findProviderById,
} from "./summaryViewModel";

// --- Pinia Store Definition ---

export const useProviderStore = defineStore("provider", () => {
  const providers = ref<ProviderSummaryItem[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function fetchProviders() {
    loading.value = true;
    error.value = null;
    try {
      const data = await providerService.getProviderSummaryList();
      providers.value = data || [];
      return providers.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch global providers:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      loading.value = false;
    }
  }

  const providerOptions = computed(() =>
    buildProviderOptions(providers.value),
  );

  const providerNameById = computed(() => {
    return buildProviderNameById(providers.value);
  });

  const getProviderById = (providerId: number | string | null | undefined) =>
    findProviderById(providers.value, providerId);

  return {
    providers,
    loading,
    error,
    providerOptions,
    providerNameById,
    getProviderById,
    fetchProviders,
  };
});
