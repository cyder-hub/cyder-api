<script setup lang="ts">
import { Inbox } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import type { AlertEvent } from "@/services/types";
import {
  formatAlertDateTime,
  isAlertSuppressed,
  severityBadgeClass,
  statusBadgeClass,
} from "../composables/alertViewModel";

defineProps<{
  alerts: AlertEvent[];
  selectedAlertId: number | null;
}>();

defineEmits<{
  select: [alert: AlertEvent];
}>();
</script>

<template>
  <div class="overflow-hidden rounded-lg border border-gray-200 bg-white">
    <div v-if="!alerts.length" class="flex flex-col items-center justify-center py-20 text-gray-500">
      <Inbox class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
      <p class="text-sm font-medium">{{ $t("alertsPage.empty") }}</p>
    </div>
    <div v-else class="divide-y divide-gray-100">
      <button
        v-for="alert in alerts"
        :key="alert.id"
        type="button"
        class="block w-full px-4 py-4 text-left transition-colors hover:bg-gray-50"
        :class="selectedAlertId === alert.id ? 'bg-gray-50' : 'bg-white'"
        @click="$emit('select', alert)"
      >
        <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-2">
              <Badge :class="severityBadgeClass(alert.severity)" class="font-mono text-[11px]">
                {{ $t(`alertsPage.severity.${alert.severity}`) }}
              </Badge>
              <Badge :class="statusBadgeClass(alert.status)" class="font-mono text-[11px]">
                {{ $t(`alertsPage.status.${alert.status}`) }}
              </Badge>
              <Badge
                v-if="isAlertSuppressed(alert)"
                class="border-gray-200 bg-gray-100 font-mono text-[11px] text-gray-600"
              >
                {{ $t("alertsPage.flags.suppressed") }}
              </Badge>
              <Badge
                v-if="alert.acknowledged_at"
                class="border-gray-200 bg-white font-mono text-[11px] text-gray-600"
              >
                {{ $t("alertsPage.flags.acknowledged") }}
              </Badge>
            </div>
            <p class="mt-2 truncate text-sm font-medium text-gray-900">
              {{ alert.title }}
            </p>
            <p class="mt-1 line-clamp-2 text-sm text-gray-500">
              {{ alert.summary }}
            </p>
          </div>
          <div class="shrink-0 text-left text-xs text-gray-500 sm:text-right">
            <p class="font-mono">{{ alert.rule_key }}</p>
            <p class="mt-1">{{ formatAlertDateTime(alert.last_seen_at) }}</p>
          </div>
        </div>
      </button>
    </div>
  </div>
</template>
