<template>
  <div class="overflow-hidden rounded-xl border border-gray-200 bg-white">
    <Table>
      <TableHeader>
        <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("providerPage.table.name") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("providerPage.table.key") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("providerPage.table.status") }}
          </TableHead>
          <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("common.actions") }}
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow v-for="provider in providers" :key="provider.id">
          <TableCell>
            <div class="font-medium text-gray-900">{{ provider.name }}</div>
            <div class="mt-0.5 text-xs text-gray-500">
              {{ t("providerPage.subtitle") }}
            </div>
          </TableCell>
          <TableCell class="font-mono text-sm text-gray-700">
            {{ provider.provider_key }}
          </TableCell>
          <TableCell>
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
          </TableCell>
          <TableCell class="text-right">
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
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </div>
</template>

<script setup lang="ts">
import { Activity, Server, Trash2 } from "lucide-vue-next";

import { useAppI18n } from "@/i18n";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
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
