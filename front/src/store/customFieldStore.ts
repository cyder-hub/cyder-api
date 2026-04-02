import { defineStore } from "pinia";
import { ref } from "vue";
import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import type { CustomFieldDefinition } from "./types";

export const useCustomFieldStore = defineStore("customField", () => {
  const customFields = ref<CustomFieldDefinition[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function fetchCustomFields() {
    loading.value = true;
    error.value = null;
    try {
      const response = await Api.getCustomFieldList();
      customFields.value = response.list || [];
      return customFields.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch custom fields:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      loading.value = false;
    }
  }


  return {
    customFields,
    loading,
    error,
    fetchCustomFields,
  };
});
