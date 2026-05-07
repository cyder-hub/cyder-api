<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { FileText, Pencil } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { ProviderRuntimeItem, ProviderRuntimeLevel } from "@/services/types";
import type { ProviderRuntimeMetric } from "../types";

const props = defineProps<{
  items: ProviderRuntimeItem[];
  buildPrimaryMetrics: (item: ProviderRuntimeItem) => ProviderRuntimeMetric[];
  formatCost: (nanos: number, currency: string) => string;
  formatDateTime: (value: number | null | undefined) => string;
  runtimeBadgeClass: (level: ProviderRuntimeLevel) => string;
  runtimeLevelLabel: (level: ProviderRuntimeLevel) => string;
}>();

const emit = defineEmits<{
  editProvider: [providerId: number];
  viewRecords: [item: ProviderRuntimeItem];
}>();

const { t: $t } = useI18n();
</script>

<template>
  <div class="grid grid-cols-1 gap-4 xl:hidden">
    <Card
      v-for="item in props.items"
      :key="item.provider_id"
      class="border border-gray-200 shadow-none"
    >
      <CardHeader class="flex flex-col gap-4 px-4 py-4 sm:px-5">
        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-2">
              <CardTitle class="text-base text-gray-900">
                {{ item.provider_name }}
              </CardTitle>
              <Badge :class="props.runtimeBadgeClass(item.runtime_level)">
                {{ props.runtimeLevelLabel(item.runtime_level) }}
              </Badge>
              <Badge variant="outline" class="text-[11px]">
                {{ item.provider_type }}
              </Badge>
            </div>
            <p class="mt-1 truncate font-mono text-xs text-gray-400" :title="item.provider_key">
              {{ item.provider_key }}
            </p>
          </div>
          <div class="flex flex-wrap gap-1.5">
            <Button
              variant="ghost"
              size="sm"
              class="h-7 px-2 text-xs text-gray-500"
              @click="emit('editProvider', item.provider_id)"
            >
              <Pencil class="mr-1 h-3.5 w-3.5" />
              {{ $t("providerRuntimePage.editProvider") }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="h-7 px-2 text-xs text-gray-500"
              @click="emit('viewRecords', item)"
            >
              <FileText class="mr-1 h-3.5 w-3.5" />
              {{ $t("providerRuntimePage.viewRecords") }}
            </Button>
            <Badge variant="outline" class="bg-gray-50 text-[11px] text-gray-500">
              {{ $t("providerRuntimePage.metrics.models") }}: {{ item.enabled_model_count }}
            </Badge>
            <Badge variant="outline" class="bg-gray-50 text-[11px] text-gray-500">
              {{ $t("providerRuntimePage.metrics.keys") }}:
              {{ item.enabled_provider_key_count }}
            </Badge>
            <Badge variant="outline" class="bg-gray-50 text-[11px] text-gray-500">
              {{ $t("providerRuntimePage.metrics.proxy") }}:
              {{ item.use_proxy ? $t("common.yes") : $t("common.no") }}
            </Badge>
          </div>
        </div>
      </CardHeader>

      <CardContent class="space-y-4 px-4 pb-4 sm:px-5">
        <div
          v-if="item.runtime_state_backend_degraded && item.runtime_state_backend_error"
          class="rounded-lg border border-orange-200 bg-orange-50 px-3 py-2 text-xs text-orange-700"
        >
          {{
            $t("providerRuntimePage.backendStatus.itemError", {
              error: item.runtime_state_backend_error,
            })
          }}
        </div>

        <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <div
            v-for="metric in props.buildPrimaryMetrics(item)"
            :key="metric.label"
            class="rounded-lg border border-gray-100 bg-gray-50/70 px-3 py-3"
          >
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              {{ metric.label }}
            </p>
            <p class="mt-1 text-sm font-semibold text-gray-900">
              {{ metric.value }}
            </p>
          </div>
        </div>

        <div class="grid grid-cols-1 gap-3 text-sm text-gray-600 sm:grid-cols-2">
          <div class="rounded-lg border border-gray-100 px-3 py-3">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerRuntimePage.metrics.lastRequest") }}
            </p>
            <p class="mt-1 text-gray-900">{{ props.formatDateTime(item.last_request_at) }}</p>
          </div>
          <div class="rounded-lg border border-gray-100 px-3 py-3">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerRuntimePage.metrics.lastSuccess") }}
            </p>
            <p class="mt-1 text-gray-900">{{ props.formatDateTime(item.last_success_at) }}</p>
          </div>
          <div class="rounded-lg border border-gray-100 px-3 py-3 sm:col-span-2">
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("providerRuntimePage.metrics.lastError") }}
                </p>
                <p class="mt-1 text-gray-900">{{ props.formatDateTime(item.last_error_at) }}</p>
              </div>
              <Badge variant="outline" class="shrink-0 bg-gray-50 text-[11px] text-gray-500">
                {{ $t("providerRuntimePage.metrics.failures") }}:
                {{ item.consecutive_failures }}
              </Badge>
            </div>
            <p class="mt-2 break-words text-xs text-gray-500">
              {{
                item.last_error_summary ||
                item.last_error ||
                $t("providerRuntimePage.detail.noError")
              }}
            </p>
          </div>
        </div>

        <div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
          <div class="rounded-lg border border-gray-100 px-3 py-3">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerRuntimePage.detail.statusCode") }}
            </p>
            <div v-if="item.status_code_breakdown.length" class="mt-2 flex flex-wrap gap-2">
              <Badge
                v-for="statusCode in item.status_code_breakdown"
                :key="statusCode.status_code"
                variant="secondary"
                class="font-mono text-[11px]"
              >
                {{ statusCode.status_code }} · {{ statusCode.count }}
              </Badge>
            </div>
            <p v-else class="mt-2 text-xs text-gray-500">
              {{ $t("providerRuntimePage.detail.noStatusCode") }}
            </p>
          </div>

          <div class="rounded-lg border border-gray-100 px-3 py-3">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerRuntimePage.metrics.cost") }}
            </p>
            <div v-if="item.total_cost.length" class="mt-2 space-y-1 text-sm text-gray-900">
              <p v-for="cost in item.total_cost" :key="cost.currency" class="font-mono">
                {{ props.formatCost(cost.amount_nanos, cost.currency) }}
              </p>
            </div>
            <p v-else class="mt-2 text-xs text-gray-500">
              {{ $t("providerRuntimePage.detail.noCost") }}
            </p>
          </div>
        </div>
      </CardContent>
    </Card>
  </div>
</template>
