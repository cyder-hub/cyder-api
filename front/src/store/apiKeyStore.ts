import { defineStore } from "pinia";
import { ref } from "vue";
import { Api } from "@/services/request";
import type { ApiKeyItem } from "./types";

const formatTimestamp = (ms: number | undefined | null): string => {
  if (!ms) return "";
  const date = new Date(ms);
  const YYYY = date.getFullYear();
  const MM = String(date.getMonth() + 1).padStart(2, "0");
  const DD = String(date.getDate()).padStart(2, "0");
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  const ss = String(date.getSeconds()).padStart(2, "0");
  return `${YYYY}-${MM}-${DD} ${hh}:${mm}:${ss}`;
};

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

  const refetchApiKeys = fetchApiKeys;
  const loadApiKeys = fetchApiKeys;

  return { apiKeys, fetchApiKeys, refetchApiKeys, loadApiKeys };
});
