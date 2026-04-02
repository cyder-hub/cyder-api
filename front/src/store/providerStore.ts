import { defineStore } from "pinia";
import { ref } from "vue";
import { Api } from "@/services/request";
import type { ProviderListItem } from "./types";

// --- Pinia Store Definition ---

export const useProviderStore = defineStore("provider", () => {
  const providers = ref<ProviderListItem[]>([]);

  async function fetchProviders() {
    try {
      providers.value = await Api.getProviderDetailList();
    } catch (error) {
      console.error("Failed to fetch global providers:", error);
      providers.value = [];
    }
  }

  return { providers, fetchProviders };
});
