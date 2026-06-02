<template>
  <div class="grid grid-cols-1 gap-3">
    <MobileCrudCard
      v-for="provider in providers"
      :key="provider.id"
      :title="provider.name"
      :description="provider.provider_key"
    >
      <template #header>
        <div class="flex items-center gap-2">
          <Badge :class="providerStateClass(provider)" class="font-mono text-[11px]">
            {{ providerStateLabel(provider) }}
          </Badge>
          <Badge
            v-if="runtimeLevels[provider.id]"
            :class="runtimeBadgeClass(runtimeLevels[provider.id])"
            class="font-mono text-[11px]"
          >
            {{ runtimeLevelLabel(runtimeLevels[provider.id]) }}
          </Badge>
        </div>
      </template>

      <div class="grid grid-cols-1 gap-2 text-xs text-gray-500">
        <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
          <span>{{ t("providerPage.table.name") }}</span>
          <span class="max-w-[12rem] truncate font-mono text-gray-700">
            {{ provider.name }}
          </span>
        </div>
        <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
          <span>{{ t("providerPage.table.key") }}</span>
          <span class="max-w-[12rem] truncate font-mono text-gray-700">
            {{ provider.provider_key }}
          </span>
        </div>
      </div>

      <template #actions>
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-center"
          @click="$emit('edit', provider)"
        >
          <Server class="mr-1.5 h-3.5 w-3.5" />
          {{ t("common.edit") }}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-center text-gray-600 hover:text-gray-900"
          @click="$emit('runtime', provider)"
        >
          <Activity class="mr-1.5 h-3.5 w-3.5" />
          {{ t("providerPage.viewRuntime") }}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-center text-gray-400 hover:text-red-600"
          @click="$emit('delete', provider)"
        >
          <Trash2 class="mr-1.5 h-3.5 w-3.5" />
          {{ t("common.delete") }}
        </Button>
      </template>
    </MobileCrudCard>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Activity, Server, Trash2 } from "lucide-vue-next";

import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ProviderSummaryItem } from "@/services/types";
import type {
  ProviderBadgeClassResolver,
  ProviderLabelResolver,
  ProviderRuntimeLevelMap,
  RuntimeBadgeClassResolver,
  RuntimeLabelResolver,
} from "../types";

defineProps<{
  providers: ProviderSummaryItem[];
  runtimeLevels: ProviderRuntimeLevelMap;
  providerStateLabel: ProviderLabelResolver;
  providerStateClass: ProviderBadgeClassResolver;
  runtimeLevelLabel: RuntimeLabelResolver;
  runtimeBadgeClass: RuntimeBadgeClassResolver;
}>();

defineEmits<{
  edit: [provider: ProviderSummaryItem];
  runtime: [provider: ProviderSummaryItem];
  delete: [provider: ProviderSummaryItem];
}>();

const { t } = useI18n();
</script>
