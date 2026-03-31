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

  // In Pinia, refetching is often just calling the fetch action again.
  // We can alias it for clarity if we want.
  const refetchPolicies = fetchPolicies;

  // The concept of 'loadPolicies' which seems to be a deferred fetch
  // can be handled by simply calling fetchPolicies when needed.
  // In SolidJS it was used to trigger the resource fetch.
  // In Vue, we can call this action from any component.
  const loadPolicies = fetchPolicies;

  return { policies, fetchPolicies, refetchPolicies, loadPolicies };
});
