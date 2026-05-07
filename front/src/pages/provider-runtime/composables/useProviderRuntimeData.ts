import { computed, ref } from "vue";
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
  const loadingList = ref(false);
  const loadingSummary = ref(false);
  const error = ref<string | null>(null);

  const isLoading = computed(() => loadingList.value || loadingSummary.value);

  async function fetchList(params: ProviderRuntimeListParams) {
    loadingList.value = true;
    error.value = null;
    try {
      const data = await api.getProviderRuntimeList(params);
      items.value = data || [];
      return items.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch provider runtime list:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      loadingList.value = false;
    }
  }

  async function fetchSummary(window?: ProviderRuntimeListParams["window"]) {
    loadingSummary.value = true;
    error.value = null;
    try {
      summary.value = await api.getProviderRuntimeSummary(window);
      return summary.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch provider runtime summary:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      loadingSummary.value = false;
    }
  }

  async function refresh(params: ProviderRuntimeListParams) {
    await Promise.all([fetchSummary(params.window), fetchList(params)]);
  }

  return {
    error,
    fetchList,
    fetchSummary,
    isLoading,
    items,
    loadingList,
    loadingSummary,
    refresh,
    summary,
  };
}
