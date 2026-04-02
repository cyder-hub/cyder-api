import { defineStore } from "pinia";
import { ref } from "vue";
import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import type { ApiKeyItem } from "./types";
import { formatTimestamp } from "@/lib/utils";

export const useApiKeyStore = defineStore("apiKey", () => {
  const apiKeys = ref<ApiKeyItem[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function fetchApiKeys() {
    loading.value = true;
    error.value = null;
    try {
      const data = await Api.getApiKeyList();
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


  return { apiKeys, loading, error, fetchApiKeys };
});
