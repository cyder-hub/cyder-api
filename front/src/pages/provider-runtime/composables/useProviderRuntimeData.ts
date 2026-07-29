import { ref } from "vue";
import { normalizeError } from "@/utils/error";
import * as providerRuntimeService from "@/services/providerRuntime";
import type {
  ProviderRuntimeItem,
  ProviderRuntimeListParams,
  ProviderRuntimeSummary,
} from "@/services/types";
import type { ProviderRuntimeDataApi } from "../types";

export interface UseProviderRuntimeDataOptions {
  api?: ProviderRuntimeDataApi;
}

export function useProviderRuntimeData(
  options: UseProviderRuntimeDataOptions = {},
) {
  const api = options.api ?? providerRuntimeService;
  const items = ref<ProviderRuntimeItem[]>([]);
  const summary = ref<ProviderRuntimeSummary | null>(null);
  const isLoading = ref(false);
  const error = ref<string | null>(null);

  async function refresh(params: ProviderRuntimeListParams) {
    isLoading.value = true;
    error.value = null;
    try {
      const snapshot = await api.getProviderRuntimeSnapshot(params);
      items.value = snapshot.items || [];
      summary.value = snapshot.summary;
      return snapshot;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch provider runtime snapshot:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      isLoading.value = false;
    }
  }

  return {
    error,
    isLoading,
    items,
    refresh,
    summary,
  };
}
