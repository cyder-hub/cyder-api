import { defineStore } from "pinia";
import { ref } from "vue";
import { Api } from "@/services/request";
import type { ApiKeyItem } from "./types";
import { formatTimestamp } from "@/lib/utils";

export const useApiKeyStore = defineStore("apiKey", () => {
  const apiKeys = ref<ApiKeyItem[]>([]);

  async function fetchApiKeys() {
    try {
      const data = await Api.getApiKeyList();
      apiKeys.value = (data || []).map((key) => ({
        ...key,
        created_at_formatted: formatTimestamp(key.created_at),
        updated_at_formatted: formatTimestamp(key.updated_at),
      }));
    } catch (error) {
      console.error("Failed to fetch API keys:", error);
      apiKeys.value = [];
    }
  }


  return { apiKeys, fetchApiKeys };
});
