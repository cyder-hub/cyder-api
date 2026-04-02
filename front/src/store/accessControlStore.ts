import { defineStore } from "pinia";
import { ref } from "vue";
import { Api } from "@/services/request";
import type { AccessControlPolicyFromAPI } from "./types";

export const useAccessControlStore = defineStore("accessControl", () => {
  const policies = ref<AccessControlPolicyFromAPI[]>([]);

  async function fetchPolicies() {
    try {
      const response = await Api.getAccessControlList();
      policies.value = response || [];
    } catch (error) {
      console.error("Failed to fetch policies:", error);
      policies.value = [];
    }
  }


  return { policies, fetchPolicies };
});
