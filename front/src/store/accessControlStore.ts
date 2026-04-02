import { defineStore } from "pinia";
import { ref } from "vue";
import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import type { AccessControlPolicyFromAPI } from "./types";

export const useAccessControlStore = defineStore("accessControl", () => {
  const policies = ref<AccessControlPolicyFromAPI[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function fetchPolicies() {
    loading.value = true;
    error.value = null;
    try {
      const response = await Api.getAccessControlList();
      policies.value = response || [];
      return policies.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch policies:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      loading.value = false;
    }
  }


  return { policies, loading, error, fetchPolicies };
});
