import { defineStore } from "pinia";
import { ref } from "vue";
import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import type { ProviderListItem } from "./types";

// --- Pinia Store Definition ---

export const useProviderStore = defineStore("provider", () => {
  const providers = ref<ProviderListItem[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function fetchProviders() {
    loading.value = true;
    error.value = null;
    try {
      const data = await Api.getProviderDetailList();
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

  return { providers, loading, error, fetchProviders };
});
