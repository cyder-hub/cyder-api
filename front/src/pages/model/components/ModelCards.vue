<template>
  <div class="grid grid-cols-1 gap-3 md:hidden">
    <MobileCrudCard
      v-for="model in models"
      :key="model.id"
      :title="model.model_name"
      :description="model.real_model_name || t('modelPage.noMappedModel')"
    >
      <template #header>
        <Badge :variant="model.is_enabled ? 'secondary' : 'outline'" class="font-mono text-[11px]">
          {{ model.is_enabled ? t("common.yes") : t("common.no") }}
        </Badge>
      </template>

      <div class="grid grid-cols-1 gap-2 text-xs text-gray-500">
        <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
          <span>{{ t("modelPage.table.provider") }}</span>
          <span class="max-w-[12rem] truncate font-mono text-gray-700">
            {{ model.provider_name }} / {{ model.provider_key }}
          </span>
        </div>
        <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
          <span>{{ t("modelPage.table.realModel") }}</span>
          <span class="max-w-[12rem] truncate font-mono text-gray-700">
            {{ model.real_model_name || t("common.notAvailable") }}
          </span>
        </div>
        <div class="rounded-lg border border-gray-100 px-3 py-2.5">
          <span>{{ t("modelPage.table.capabilities") }}</span>
          <div class="mt-2 flex flex-wrap gap-1.5">
            <Badge
              v-for="capability in capabilityItems"
              :key="capability.key"
              :variant="model[capability.key] ? 'secondary' : 'outline'"
              class="font-mono text-[11px]"
            >
              {{ t(capability.labelKey) }}
            </Badge>
          </div>
        </div>
      </div>

      <template #actions>
        <Button variant="ghost" size="sm" class="w-full justify-center" @click="$emit('open', model.id)">
          <Pencil class="mr-1.5 h-3.5 w-3.5" />
          {{ t("common.edit") }}
        </Button>
      </template>
    </MobileCrudCard>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Pencil } from "lucide-vue-next";

import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ModelSummaryItem } from "@/services/types";
import type { ModelCapabilityItem } from "../types";

defineProps<{
  models: ModelSummaryItem[];
  capabilityItems: ModelCapabilityItem[];
}>();

defineEmits<{
  open: [id: number];
}>();

const { t } = useI18n();
</script>
