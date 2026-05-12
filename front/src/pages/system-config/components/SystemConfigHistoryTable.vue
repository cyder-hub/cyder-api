<script setup lang="ts">
import { Loader2, RefreshCcw } from "lucide-vue-next";

import SectionHeader from "@/components/SectionHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { SystemConfigHistoryItem } from "@/services/types";
import type { SystemConfigHistoryRow } from "../types";
import { formatSystemConfigTimestamp } from "../composables/useSystemConfigReport";

defineProps<{
  historyRows: SystemConfigHistoryRow[];
  isHistoryLoading: boolean;
  historyError: string | null;
  hasMoreHistory: boolean;
  historyOperationLabel: (
    operation: SystemConfigHistoryItem["operation"],
  ) => string;
}>();

defineEmits<{
  refresh: [];
  loadMore: [];
}>();
</script>

<template>
  <div class="rounded-xl border border-gray-200 bg-white">
    <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-3 sm:flex-row sm:items-start sm:justify-between">
      <SectionHeader
        :title="$t('systemConfigPage.history.title')"
        :help="$t('systemConfigPage.history.description')"
        :help-label="$t('systemConfigPage.history.title')"
      />
      <Button
        variant="outline"
        class="w-full sm:w-auto"
        :disabled="isHistoryLoading"
        @click="$emit('refresh')"
      >
        <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isHistoryLoading }" />
        {{ $t("systemConfigPage.refresh") }}
      </Button>
    </div>

    <div
      v-if="historyError"
      class="border-b border-red-100 bg-red-50 px-4 py-3 text-sm text-red-600"
    >
      {{ historyError }}
    </div>

    <div v-if="historyRows.length" class="divide-y divide-gray-100">
      <article
        v-for="row in historyRows"
        :key="`${row.item.changed_at}-${row.item.version_after}`"
        class="px-4 py-4"
      >
        <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-2">
              <Badge variant="outline" class="font-mono text-xs">
                {{ historyOperationLabel(row.item.operation) }}
              </Badge>
              <span class="font-mono text-xs text-gray-500">
                v{{ row.item.version_before }} -> v{{ row.item.version_after }}
              </span>
            </div>
            <p class="mt-2 break-words text-sm text-gray-700">
              {{ row.item.reason || $t("systemConfigPage.history.noReason") }}
            </p>
          </div>
          <p class="font-mono text-xs text-gray-500">
            {{ formatSystemConfigTimestamp(row.item.changed_at) }}
          </p>
        </div>
        <div class="mt-3 flex flex-wrap gap-1.5">
          <Badge
            v-for="path in row.item.changed_paths"
            :key="`${row.item.changed_at}-${path}`"
            variant="outline"
            class="max-w-full break-all font-mono text-xs text-gray-500"
          >
            {{ path }}
          </Badge>
        </div>
        <div v-if="row.diff.length" class="mt-3 overflow-hidden rounded-lg border border-gray-200">
          <div class="grid grid-cols-1 divide-y divide-gray-100 md:grid-cols-3 md:divide-x md:divide-y-0">
            <div
              v-for="diff in row.diff"
              :key="`${row.item.changed_at}-${diff.path}`"
              class="contents"
            >
              <div class="px-3 py-2 font-mono text-xs font-medium text-gray-900">
                {{ diff.path }}
              </div>
              <pre class="max-h-24 overflow-auto whitespace-pre-wrap break-all px-3 py-2 font-mono text-xs text-gray-500">{{ diff.oldText }}</pre>
              <pre class="max-h-24 overflow-auto whitespace-pre-wrap break-all px-3 py-2 font-mono text-xs text-gray-900">{{ diff.newText }}</pre>
            </div>
          </div>
        </div>
      </article>
    </div>
    <div v-else class="px-4 py-8 text-center text-sm text-gray-500">
      {{ isHistoryLoading ? $t("systemConfigPage.history.loading") : $t("systemConfigPage.history.empty") }}
    </div>
    <div class="border-t border-gray-100 px-4 py-3">
      <Button
        variant="outline"
        class="w-full"
        :disabled="isHistoryLoading || !hasMoreHistory"
        @click="$emit('loadMore')"
      >
        <Loader2 v-if="isHistoryLoading" class="mr-1.5 h-4 w-4 animate-spin" />
        {{ hasMoreHistory ? $t("systemConfigPage.history.loadMore") : $t("systemConfigPage.history.noMore") }}
      </Button>
    </div>
  </div>
</template>
