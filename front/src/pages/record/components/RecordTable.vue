<template>
  <div v-if="loading" class="py-10 text-center text-gray-500">
    <div class="mb-2 inline-block h-8 w-8 animate-spin rounded-full border-b-2 border-gray-900"></div>
    <div>{{ $t("recordPage.loading") }}</div>
  </div>

  <div
    v-else-if="errorMsg"
    class="rounded-lg border border-red-400 bg-red-100 p-4 py-4 text-center text-red-600"
  >
    {{ $t("recordPage.errorPrefix") }} {{ errorMsg }}
  </div>

  <div
    v-else
    class="flex min-h-0 flex-1 flex-col rounded-xl border border-gray-200 bg-white"
  >
    <div v-if="records.length === 0" class="px-4 py-10 text-center text-sm text-gray-500">
      {{ $t("recordPage.table.noRecordsMatch") }}
    </div>

    <div v-else class="space-y-3 p-3 md:hidden">
      <MobileCrudCard
        v-for="record in records"
        :key="record.id"
        :title="record.displayRequestedModelName"
        :description="record.request_at_formatted"
      >
        <div class="grid grid-cols-1 gap-3 text-sm min-[360px]:grid-cols-2">
          <MobileField :label="$t('recordPage.table.provider')" :value="record.providerName" />
          <MobileField :label="$t('recordPage.table.apiKey')" :value="record.apiKeyName" />
          <div class="space-y-1">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              {{ $t("recordPage.table.status") }}
            </p>
            <Badge :variant="getStatusBadgeVariant(record.overall_status)">
              {{ record.overall_status || "/" }}
            </Badge>
          </div>
          <MobileField :label="$t('recordPage.table.attempts')" :value="record.attemptsDisplay" mono />
        </div>

        <div class="grid grid-cols-1 gap-3 rounded-lg bg-gray-50 p-3 min-[360px]:grid-cols-2">
          <MobileField :label="$t('recordPage.table.tokens')" :value="record.total_tokens ?? '/'" />
          <MobileField :label="$t('recordPage.table.cost')" :value="record.costDisplay" mono />
          <MobileField :label="$t('recordPage.table.firstByte')" :value="record.firstRespTimeDisplay" />
          <MobileField :label="$t('recordPage.table.diagnostics')" :value="record.diagnosticsDisplay" mono />
        </div>

        <template #actions>
          <Button class="w-full" @click="$emit('viewDetails', record.id)">
            {{ $t("recordPage.table.viewDetails") }}
          </Button>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden flex-1 overflow-auto md:block">
      <Table>
        <TableHeader class="bg-gray-50/80 hover:bg-gray-50/80">
          <TableRow>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.model") }}
            </TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.provider") }}
            </TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.apiKey") }}
            </TableHead>
            <TableHead class="w-14 text-center text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.status") }}
            </TableHead>
            <TableHead class="min-w-[220px] text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.tokens") }}
            </TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.attempts") }}
            </TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.diagnostics") }}
            </TableHead>
            <TableHead class="min-w-[200px] text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.performance") }}
            </TableHead>
            <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.cost") }}
            </TableHead>
            <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.time") }}
            </TableHead>
            <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("recordPage.table.details") }}
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow
            v-for="record in records"
            :key="record.id"
            class="hover:bg-gray-50"
          >
            <TableCell class="font-medium">{{ record.displayRequestedModelName }}</TableCell>
            <TableCell>{{ record.providerName }}</TableCell>
            <TableCell>{{ record.apiKeyName }}</TableCell>
            <TableCell class="w-14 text-center">
              <div
                class="flex justify-center"
                :title="getStatusMeta(record.overall_status).label"
                :aria-label="getStatusMeta(record.overall_status).label"
              >
                <component
                  :is="getStatusMeta(record.overall_status).icon"
                  class="h-4 w-4"
                  :class="getStatusMeta(record.overall_status).className"
                />
                <span class="sr-only">{{ getStatusMeta(record.overall_status).label }}</span>
              </div>
            </TableCell>
            <TableCell class="font-mono text-xs text-gray-700">
              {{
                formatCompactMetrics([
                  record.total_input_tokens,
                  record.total_output_tokens,
                  record.reasoning_tokens,
                  record.total_tokens,
                ])
              }}
            </TableCell>
            <TableCell class="font-mono text-xs text-gray-700">
              {{ record.attemptsDisplay }}
            </TableCell>
            <TableCell class="font-mono text-xs text-gray-700">
              <Badge
                :variant="record.has_transform_diagnostics ? 'outline' : 'secondary'"
                class="font-mono text-[11px]"
              >
                {{ record.diagnosticsDisplay }}
              </Badge>
            </TableCell>
            <TableCell class="font-mono text-xs text-gray-700">
              {{
                formatCompactMetrics([
                  record.firstRespTimeDisplay,
                  record.totalRespTimeDisplay,
                  record.tpsDisplay,
                ])
              }}
            </TableCell>
            <TableCell class="text-right font-mono text-gray-600">
              {{ record.costDisplay }}
            </TableCell>
            <TableCell class="whitespace-nowrap text-sm text-gray-500">
              {{ record.request_at_formatted }}
            </TableCell>
            <TableCell class="text-right">
              <Button
                variant="link"
                size="sm"
                class="px-0"
                @click="$emit('viewDetails', record.id)"
              >
                {{ $t("recordPage.table.view") }}
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <div
      v-if="totalPages > 0"
      class="mt-auto flex flex-shrink-0 flex-col gap-4 border-t border-gray-100 px-4 py-4 sm:px-5 md:flex-row md:items-center md:justify-between"
    >
      <div class="order-2 flex flex-col gap-3 text-sm text-gray-500 md:order-1 md:flex-row md:items-center md:gap-4">
        <div>
          {{
            $t("recordPage.pagination.summary", {
              page: currentPage,
              totalPages,
              total: totalRecords,
            })
          }}
        </div>
        <div class="flex items-center justify-between gap-2 sm:justify-start">
          <label class="whitespace-nowrap">{{ $t("recordPage.pagination.itemsPerPage") }}</label>
          <Select
            :model-value="String(pageSize)"
            @update:model-value="$emit('pageSizeChange', String($event))"
          >
            <SelectTrigger class="h-8 w-[70px] text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="size in [10, 25, 50, 100]"
                :key="size"
                :value="String(size)"
                class="text-xs"
              >
                {{ size }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      <Pagination
        v-if="totalPages > 1"
        :total="totalRecords"
        :sibling-count="1"
        :items-per-page="pageSize"
        :page="currentPage"
        show-edges
        class="order-1 mx-0 w-full md:order-2 md:w-auto"
        @update:page="$emit('pageChange', $event)"
      >
        <PaginationContent
          v-slot="{ items }"
          class="flex flex-wrap items-center justify-center gap-1 md:justify-end"
        >
          <PaginationFirst />
          <PaginationPrevious />
          <template v-for="(item, index) in items" :key="index">
            <PaginationItem
              v-if="item.type === 'page'"
              :value="item.value"
              :is-active="item.value === currentPage"
            >
              {{ item.value }}
            </PaginationItem>
            <PaginationEllipsis v-else />
          </template>
          <PaginationNext />
          <PaginationLast />
        </PaginationContent>
      </Pagination>
    </div>
  </div>
</template>

<script setup lang="ts">
import { defineComponent, h } from "vue";
import { useI18n } from "vue-i18n";
import {
  CircleAlert,
  CircleCheckBig,
  CircleHelp,
  Clock3,
} from "lucide-vue-next";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Pagination,
  PaginationContent,
  PaginationEllipsis,
  PaginationFirst,
  PaginationItem,
  PaginationLast,
  PaginationNext,
  PaginationPrevious,
} from "@/components/ui/pagination";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { EnrichedRecordListItem, RecordStatusMeta } from "../types";
import {
  emptyValue,
  formatCompactMetrics,
  getStatusBadgeVariant,
} from "../composables/recordFormat";

defineProps<{
  records: EnrichedRecordListItem[];
  loading: boolean;
  errorMsg: string | null;
  currentPage: number;
  pageSize: number;
  totalPages: number;
  totalRecords: number;
}>();

defineEmits<{
  viewDetails: [id: number];
  pageChange: [page: number];
  pageSizeChange: [value: string];
}>();

const { t: $t } = useI18n();

const getStatusMeta = (status: string | null): RecordStatusMeta => {
  switch (status) {
    case "SUCCESS":
      return {
        icon: CircleCheckBig,
        className: "text-emerald-600",
        label: $t("recordPage.filter.status.SUCCESS"),
      };
    case "ERROR":
      return {
        icon: CircleAlert,
        className: "text-red-600",
        label: $t("recordPage.filter.status.ERROR"),
      };
    case "PENDING":
      return {
        icon: Clock3,
        className: "text-amber-600",
        label: $t("recordPage.filter.status.PENDING"),
      };
    case "CANCELLED":
      return {
        icon: CircleHelp,
        className: "text-gray-500",
        label: $t("recordPage.filter.status.CANCELLED"),
      };
    default:
      return {
        icon: CircleHelp,
        className: "text-gray-400",
        label: status || emptyValue,
      };
  }
};

const MobileField = defineComponent({
  props: {
    label: { type: String, required: true },
    value: { type: [String, Number], required: true },
    mono: { type: Boolean, default: false },
  },
  setup(props) {
    return () =>
      h("div", { class: "space-y-1" }, [
        h(
          "p",
          { class: "text-[11px] font-medium uppercase tracking-wide text-gray-500" },
          props.label,
        ),
        h(
          "p",
          {
            class: [
              "break-words text-sm text-gray-900",
              props.mono ? "font-mono text-xs text-gray-700" : "",
            ],
          },
          String(props.value),
        ),
      ]);
  },
});
</script>
