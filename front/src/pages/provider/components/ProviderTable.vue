<template>
  <div class="hidden overflow-hidden rounded-xl border border-gray-200 bg-white md:block">
    <div
      class="grid grid-cols-[1.3fr_1fr_1fr_auto] items-center gap-4 border-b border-gray-200 bg-gray-50/80 px-4 py-3"
    >
      <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
        {{ t("providerPage.table.name") }}
      </span>
      <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
        {{ t("providerPage.table.key") }}
      </span>
      <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
        {{ t("providerPage.table.status") }}
      </span>
      <span class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
        {{ t("common.actions") }}
      </span>
    </div>

    <div
      v-for="provider in providers"
      :key="provider.id"
      class="grid grid-cols-[1.3fr_1fr_1fr_auto] items-center gap-4 border-b border-gray-100 px-4 py-3 transition-colors last:border-0 hover:bg-gray-50/50"
    >
      <div>
        <div class="font-medium text-gray-900">{{ provider.name }}</div>
        <div class="mt-0.5 text-xs text-gray-500">
          {{ t("providerPage.subtitle") }}
        </div>
      </div>
      <div class="font-mono text-sm text-gray-700">{{ provider.provider_key }}</div>
      <div class="flex flex-wrap items-center gap-2">
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
      <div class="flex items-center justify-end gap-1">
        <Button
          variant="ghost"
          size="sm"
          class="h-8 px-2 text-gray-600"
          @click="$emit('edit', provider)"
        >
          <Server class="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="h-8 px-2 text-gray-600"
          @click="$emit('runtime', provider)"
        >
          <Activity class="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="h-8 px-2 text-gray-400 hover:text-red-600"
          @click="$emit('delete', provider)"
        >
          <Trash2 class="h-4 w-4" />
        </Button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Activity, Server, Trash2 } from "lucide-vue-next";

import { useAppI18n } from "@/i18n";
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

const { t } = useAppI18n();
</script>
