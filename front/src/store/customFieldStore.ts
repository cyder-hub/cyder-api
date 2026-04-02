import { defineStore } from "pinia";
import { ref } from "vue";
import { Api } from "@/services/request";
import type { CustomFieldDefinition } from "./types";

export const useCustomFieldStore = defineStore("customField", () => {
  const customFields = ref<CustomFieldDefinition[]>([]);

  async function fetchCustomFields() {
    try {
      const response = await Api.getCustomFieldList();
      customFields.value = response.list || [];
    } catch (error) {
      console.error("Failed to fetch custom fields:", error);
      customFields.value = [];
    }
  }


  return {
    customFields,
    fetchCustomFields,
  };
});
