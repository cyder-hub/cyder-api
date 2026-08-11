<script setup lang="ts">
import { FileText, Pencil } from "lucide-vue-next";

import { useAppI18n } from "@/i18n";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { ProviderRuntimeItem, ProviderRuntimeLevel } from "@/services/types";
import { formatSafeSourceEndpoint } from "@/utils/sourceEvidence";

const props = defineProps<{
  items: ProviderRuntimeItem[];
  formatCount: (value: number | null | undefined) => string;
  formatCost: (nanos: number, currency: string) => string;
  formatDateTime: (value: number | null | undefined) => string;
  formatLatencyCoverage: (value: number | null, sampleCount: number) => string;
  formatPercentage: (value: number | null) => string;
  runtimeBadgeClass: (level: ProviderRuntimeLevel) => string;
  runtimeLevelLabel: (level: ProviderRuntimeLevel) => string;
}>();

const emit = defineEmits<{
  editProvider: [providerId: number, sourceId: number];
  viewRecords: [item: ProviderRuntimeItem];
}>();

const { t: $t } = useAppI18n();
</script>

<template>
  <div class="hidden overflow-hidden rounded-xl border border-gray-200 bg-white xl:block">
    <Table>
      <TableHeader>
        <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
          <TableHead>{{ $t("providerRuntimePage.table.provider") }}</TableHead>
          <TableHead>{{ $t("providerRuntimePage.table.source") }}</TableHead>
          <TableHead>{{ $t("providerRuntimePage.table.runtime") }}</TableHead>
          <TableHead>{{ $t("providerRuntimePage.metrics.requests") }}</TableHead>
          <TableHead>{{ $t("providerRuntimePage.metrics.firstResponseBody") }}</TableHead>
          <TableHead>{{ $t("providerRuntimePage.metrics.ttft") }}</TableHead>
          <TableHead>{{ $t("providerRuntimePage.metrics.totalLatency") }}</TableHead>
          <TableHead>{{ $t("providerRuntimePage.metrics.lastError") }}</TableHead>
          <TableHead>{{ $t("providerRuntimePage.metrics.cost") }}</TableHead>
          <TableHead class="text-right">{{ $t("common.actions") }}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow v-for="item in props.items" :key="item.source_id">
          <TableCell class="align-top">
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2">
                <span class="font-medium text-gray-900">{{ item.provider_name }}</span>
              </div>
              <p class="mt-1 font-mono text-xs text-gray-400">
                {{ item.provider_key }}
              </p>
              <p class="mt-1 text-xs text-gray-500">
                {{ $t("providerRuntimePage.metrics.models") }}:
                {{ item.enabled_model_count }}
                · {{ $t("providerRuntimePage.metrics.keys") }}:
                {{ item.enabled_provider_key_count }}
              </p>
              <p
                v-if="item.runtime_state_backend_degraded && item.runtime_state_backend_error"
                class="mt-2 max-w-sm break-words text-xs text-orange-700"
              >
                {{
                  $t("providerRuntimePage.backendStatus.itemError", {
                    error: item.runtime_state_backend_error,
                  })
                }}
              </p>
            </div>
          </TableCell>
          <TableCell class="max-w-xs align-top">
            <div class="flex flex-wrap items-center gap-2">
              <span class="font-mono text-xs text-gray-700">
                {{ item.source_profile_type }}
              </span>
            </div>
            <p
              class="mt-1 truncate font-mono text-[11px] text-gray-500"
              :title="formatSafeSourceEndpoint(item.source_endpoint, '-')"
            >
              {{ formatSafeSourceEndpoint(item.source_endpoint, "-") }}
            </p>
            <p class="mt-1 font-mono text-[11px] text-gray-400">
              #{{ item.source_id }} · {{ $t("providerRuntimePage.metrics.proxy") }}:
              {{ item.source_use_proxy ? $t("common.yes") : $t("common.no") }}
            </p>
          </TableCell>
          <TableCell class="align-top">
            <Badge :class="props.runtimeBadgeClass(item.runtime_level)">
              {{ props.runtimeLevelLabel(item.runtime_level) }}
            </Badge>
          </TableCell>
          <TableCell class="align-top">
            <p class="font-mono text-sm text-gray-900">
              {{ props.formatCount(item.request_count) }}
            </p>
            <p class="mt-1 text-xs text-gray-500">
              {{ $t("providerRuntimePage.metrics.successRate") }}
              {{ props.formatPercentage(item.success_rate) }}
            </p>
            <p class="mt-1 text-xs text-gray-500">
              {{ $t("providerRuntimePage.metrics.errors") }}
              {{ props.formatCount(item.error_count) }}
            </p>
          </TableCell>
          <TableCell class="align-top">
            {{
              props.formatLatencyCoverage(
                item.avg_time_to_first_response_body_ms,
                item.time_to_first_response_body_sample_count,
              )
            }}
          </TableCell>
          <TableCell class="align-top">
            {{ props.formatLatencyCoverage(item.avg_ttft_ms, item.ttft_sample_count) }}
          </TableCell>
          <TableCell class="align-top">
            {{ props.formatLatencyCoverage(item.avg_total_latency_ms, item.total_latency_sample_count) }}
          </TableCell>
          <TableCell class="max-w-xs align-top">
            <p class="text-sm text-gray-900">
              {{ props.formatDateTime(item.last_error_at) }}
            </p>
            <p class="mt-1 break-words text-xs text-gray-500">
              {{
                item.last_error_summary || $t("providerRuntimePage.detail.noError")
              }}
            </p>
            <p class="mt-1 text-xs text-gray-400">
              {{ $t("providerRuntimePage.metrics.lastRequest") }}
              {{ props.formatDateTime(item.last_request_at) }}
            </p>
          </TableCell>
          <TableCell class="align-top">
            <div v-if="item.total_cost.length" class="space-y-1">
              <p v-for="cost in item.total_cost" :key="cost.currency" class="font-mono text-xs">
                {{ props.formatCost(cost.amount_nanos, cost.currency) }}
              </p>
            </div>
            <p v-else class="text-xs text-gray-500">
              {{ $t("providerRuntimePage.detail.noCost") }}
            </p>
          </TableCell>
          <TableCell class="text-right align-top">
            <div class="flex justify-end gap-1">
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-500"
                @click="emit('editProvider', item.provider_id, item.source_id)"
              >
                <Pencil class="mr-1 h-3.5 w-3.5" />
                {{ $t("providerRuntimePage.editProvider") }}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-500"
                @click="emit('viewRecords', item)"
              >
                <FileText class="mr-1 h-3.5 w-3.5" />
                {{ $t("providerRuntimePage.viewRecords") }}
              </Button>
            </div>
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </div>
</template>
