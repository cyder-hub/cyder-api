import { computed, ref } from "vue";

import * as modelRouteService from "@/services/modelRoutes";
import { normalizeError } from "@/utils/error";
import { useApiKeyStore } from "@/store/apiKeyStore";
import { useModelStore } from "@/store/modelStore";
import { useProviderStore } from "@/store/providerStore";
import type { ApiKeyRuntimeSnapshot, ModelRouteListItem } from "@/services/types";
import { buildApiKeySummaryCards } from "./apiKeyViewModel";

type TranslateFn = (key: string, named?: Record<string, unknown>) => string;

export function useApiKeyList(t: TranslateFn) {
  const apiKeyStore = useApiKeyStore();
  const modelStore = useModelStore();
  const providerStore = useProviderStore();

  const loading = ref(true);
  const error = ref<string | null>(null);
  const modelRoutes = ref<ModelRouteListItem[]>([]);

  const apiKeys = computed(() => apiKeyStore.apiKeys);
  const runtimeSnapshots = computed(() => apiKeyStore.runtimeSnapshots);

  const runtimeById = computed(() => {
    const map = new Map<number, ApiKeyRuntimeSnapshot>();
    for (const snapshot of runtimeSnapshots.value) {
      map.set(snapshot.api_key_id, snapshot);
    }
    return map;
  });

  const routeNameById = computed(() => {
    const map = new Map<number, string>();
    for (const item of modelRoutes.value) {
      map.set(item.route.id, item.route.route_name);
    }
    return map;
  });

  const summaryCards = computed(() =>
    buildApiKeySummaryCards(apiKeys.value, runtimeSnapshots.value, t),
  );

  async function fetchData(preferredSelectedId: number | null): Promise<number | null> {
    loading.value = true;
    error.value = null;
    try {
      await Promise.all([
        apiKeyStore.fetchApiKeys(),
        apiKeyStore.fetchRuntimeSnapshots(),
        providerStore.fetchProviders(),
        modelStore.fetchModels(),
        modelRouteService.getModelRouteList().then((items) => {
          modelRoutes.value = items;
        }),
      ]);

      if (!apiKeys.value.length) {
        return null;
      }

      return preferredSelectedId &&
        apiKeys.value.some((key) => key.id === preferredSelectedId)
        ? preferredSelectedId
        : apiKeys.value[0].id;
    } catch (err: unknown) {
      error.value = normalizeError(err, t("common.unknownError")).message;
      return null;
    } finally {
      loading.value = false;
    }
  }

  return {
    apiKeyStore,
    modelStore,
    providerStore,
    apiKeys,
    runtimeSnapshots,
    runtimeById,
    routeNameById,
    modelRoutes,
    loading,
    error,
    summaryCards,
    fetchData,
  };
}
