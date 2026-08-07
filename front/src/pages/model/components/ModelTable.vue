<template>
  <div class="overflow-hidden rounded-xl border border-gray-200 bg-white">
    <Table>
      <TableHeader>
        <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("modelPage.table.provider") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("modelPage.table.model") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("modelPage.table.realModel") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("modelPage.table.sourceConfig") }}
          </TableHead>
          <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("modelPage.table.enabled") }}
          </TableHead>
          <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
            {{ t("common.actions") }}
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow v-for="model in models" :key="model.id">
          <TableCell>
            <div class="min-w-0">
              <div class="font-medium text-gray-900">{{ model.provider_name }}</div>
              <div class="mt-0.5 font-mono text-xs text-gray-500">
                {{ model.provider_key }}
              </div>
            </div>
          </TableCell>
          <TableCell class="font-mono text-sm text-gray-800">
            {{ model.model_name }}
          </TableCell>
          <TableCell class="font-mono text-sm text-gray-700">
            {{ model.real_model_name || t("common.notAvailable") }}
          </TableCell>
          <TableCell>
            <div class="flex max-w-md flex-wrap items-center gap-1.5">
              <Badge variant="secondary" class="font-mono text-[11px]">
                {{ model.source_config.source_selection_mode }}
              </Badge>
              <span class="font-mono text-xs text-gray-500">
                {{ model.source_config.declared_source_count }}
              </span>
              <Badge
                v-if="model.source_config.warnings.length > 0"
                variant="outline"
                class="font-mono text-[11px] text-gray-600"
              >
                {{ t("modelPage.sourceWarnings", { count: model.source_config.warnings.length }) }}
              </Badge>
            </div>
          </TableCell>
          <TableCell>
            <Badge :variant="model.is_enabled ? 'secondary' : 'outline'" class="font-mono text-[11px]">
              {{ model.is_enabled ? t("common.yes") : t("common.no") }}
            </Badge>
          </TableCell>
          <TableCell class="text-right">
            <Button variant="ghost" size="sm" @click="$emit('open', model.id)">
              <Pencil class="mr-1.5 h-3.5 w-3.5" />
              {{ t("common.edit") }}
            </Button>
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Pencil } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import type { ModelSummaryItem } from "@/services/types";

defineProps<{
  models: ModelSummaryItem[];
}>();

defineEmits<{
  open: [id: number];
}>();

const { t } = useI18n();
</script>
