<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <SectionHeader :title="t('common.basicInfo')" />
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div class="grid gap-1.5">
          <Label for="model_name" class="text-gray-700">
            {{ t("modelEditPage.labelModelName") }}
            <span class="text-red-500 ml-0.5">*</span>
          </Label>
          <Input id="model_name" v-model="editingData.model_name" />
        </div>

        <div class="grid gap-1.5">
          <Label for="real_model_name" class="text-gray-700">
            {{ t("modelEditPage.labelRealModelName") }}
          </Label>
          <Input id="real_model_name" v-model="editingData.real_model_name" />
        </div>
      </div>

      <div class="flex items-center justify-between p-3.5 border border-gray-200 rounded-lg">
        <Label for="is_enabled" class="cursor-pointer text-gray-700">
          {{ t("modelEditPage.labelEnabled") }}
        </Label>
        <Checkbox id="is_enabled" v-model="editingData.is_enabled" />
      </div>

      <div class="space-y-3">
        <SectionHeader
          :title="t('modelEditPage.capabilities.title')"
          :help="t('modelEditPage.capabilities.description')"
          :help-label="t('modelEditPage.capabilities.title')"
          title-class="text-sm"
        />
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div
            v-for="capability in capabilityItems"
            :key="capability.key"
            class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5"
          >
            <Label class="cursor-pointer text-gray-700">
              {{ t(capability.labelKey) }}
            </Label>
            <Checkbox
              :model-value="editingData[capability.key]"
              @update:model-value="
                (value: boolean | 'indeterminate') =>
                  (editingData[capability.key] = value === true)
              "
            />
          </div>
        </div>
      </div>
  </section>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";

import SectionHeader from "@/components/SectionHeader.vue";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { EditingModelData, ModelCapabilityItem } from "../types";

const editingData = defineModel<EditingModelData>("editingData", {
  required: true,
});

defineProps<{
  capabilityItems: ModelCapabilityItem[];
}>();

const { t } = useI18n();
</script>
