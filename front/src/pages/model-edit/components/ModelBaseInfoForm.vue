<template>
  <Card>
    <CardHeader>
      <CardTitle>{{ t("common.basicInfo") }}</CardTitle>
    </CardHeader>
    <CardContent class="space-y-4">
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
        <div>
          <h3 class="text-sm font-semibold text-gray-900">
            {{ t("modelEditPage.capabilities.title") }}
          </h3>
          <p class="mt-1 text-sm text-gray-500">
            {{ t("modelEditPage.capabilities.description") }}
          </p>
        </div>
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
    </CardContent>
  </Card>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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
