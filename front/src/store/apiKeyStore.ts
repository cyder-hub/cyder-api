import { defineStore } from "pinia";
import { ref } from "vue";
import { normalizeError } from "@/utils/error";
import * as apiKeyService from "@/services/apiKeys";
import type { ApiKeyItem, ApiKeyRuntimeSnapshot } from "@/services/types";
import { formatTimestamp } from "@/utils/datetime";

export const useApiKeyStore = defineStore("apiKey", () => {
  const apiKeys = ref<ApiKeyItem[]>([]);
  const runtimeSnapshots = ref<ApiKeyRuntimeSnapshot[]>([]);
  const loading = ref(false);
  const runtimeLoading = ref(false);
  const error = ref<string | null>(null);
  const runtimeError = ref<string | null>(null);

  async function fetchApiKeys() {
    loading.value = true;
    error.value = null;
    try {
      const data = await apiKeyService.getApiKeyList();
      apiKeys.value = (data || []).map((key) => ({
        ...key,
        created_at_formatted: formatTimestamp(key.created_at),
        updated_at_formatted: formatTimestamp(key.updated_at),
      }));
      return apiKeys.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch API keys:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      loading.value = false;
    }
  }

  async function fetchRuntimeSnapshots() {
    runtimeLoading.value = true;
    runtimeError.value = null;
    try {
      runtimeSnapshots.value = await apiKeyService.getApiKeyRuntimeList();
      return runtimeSnapshots.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch API key runtime snapshots:", normalizedError);
      runtimeError.value = normalizedError.message;
      throw normalizedError;
    } finally {
      runtimeLoading.value = false;
    }
  }

  return {
    apiKeys,
    runtimeSnapshots,
    loading,
    runtimeLoading,
    error,
    runtimeError,
    fetchApiKeys,
    fetchRuntimeSnapshots,
  };
});
