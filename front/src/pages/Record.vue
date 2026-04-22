<template>
  <div class="app-page flex h-full min-h-0 flex-col overflow-hidden">
    <div class="app-page-shell flex min-h-0 flex-1 flex-col">
      <div class="flex min-h-0 flex-1 flex-col gap-4 sm:gap-6">
        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0">
            <h1 class="text-lg font-semibold tracking-tight text-gray-900 sm:text-xl">
              {{ $t("recordPage.title") }}
            </h1>
            <p class="mt-1 text-sm text-gray-500">
              {{ $t("recordPage.description") }}
            </p>
          </div>
        </div>

        <div class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
          <div class="flex flex-col gap-3 border-b border-gray-100 pb-4 md:flex-row md:items-start md:justify-between">
            <div class="min-w-0">
              <h2 class="text-base font-semibold text-gray-900">
                {{ $t("recordPage.filter.title") }}
              </h2>
              <p class="mt-1 text-sm text-gray-500">{{ filterSummary }}</p>
            </div>
            <div class="flex w-full flex-col gap-2 sm:flex-row md:w-auto md:items-center">
              <Button
                variant="outline"
                class="w-full justify-between md:hidden"
                @click="toggleFilterPanel"
              >
                <span class="flex items-center gap-2">
                  <SlidersHorizontal class="h-4 w-4" />
                  {{
                    isFilterPanelOpen
                      ? $t("recordPage.filter.hideFilters")
                      : $t("recordPage.filter.showFilters")
                  }}
                </span>
                <ChevronDown
                  class="h-4 w-4 transition-transform"
                  :class="{ 'rotate-180': isFilterPanelOpen }"
                />
              </Button>
              <Button
                v-if="hasActiveFilters"
                variant="outline"
                class="hidden md:inline-flex"
                @click="handleResetFilter"
              >
                {{ $t("recordPage.filter.resetButton") }}
              </Button>
            </div>
          </div>

          <div
            :class="[
              'mt-4 flex-col gap-4 md:flex',
              isFilterPanelOpen ? 'flex' : 'hidden md:flex',
            ]"
          >
            <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-12">
              <FilterSelect
                class="xl:col-span-2"
                :label="$t('recordPage.filter.apiKeyLabel')"
                :model-value="String(filters.api_key_id)"
                :options="apiKeyOptions"
                @update:model-value="handleApiKeyFilterChange"
              />
              <FilterSelect
                class="xl:col-span-2"
                :label="$t('recordPage.filter.providerLabel')"
                :model-value="String(filters.provider_id)"
                :options="providerOptions"
                @update:model-value="handleProviderFilterChange"
              />
              <FilterSelect
                class="xl:col-span-3"
                :label="$t('recordPage.filter.modelLabel')"
                :model-value="String(filters.model_id)"
                :options="modelOptions"
                @update:model-value="handleModelFilterChange"
              />
              <FilterSelect
                class="xl:col-span-2"
                :label="$t('recordPage.filter.statusLabel')"
                :model-value="filters.status"
                :options="statusOptions"
                @update:model-value="handleStatusFilterChange"
              />
              <div class="flex flex-col gap-1.5 xl:col-span-3">
                <span class="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("recordPage.filter.searchLabel") }}
                </span>
                <div class="relative">
                  <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
                  <Input
                    v-model="searchInput"
                    :placeholder="$t('recordPage.filter.searchPlaceholder')"
                    class="w-full pl-9 pr-9"
                    @keydown.enter="handleApplyFilter"
                  />
                  <button
                    v-if="searchInput"
                    type="button"
                    :aria-label="$t('recordPage.filter.clearSearch')"
                    class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-gray-400 transition-colors hover:text-gray-600"
                    @click="handleClearSearch"
                  >
                    <X class="h-4 w-4" />
                  </button>
                </div>
              </div>
            </div>

            <div class="border-t border-gray-100 pt-3">
              <button
                type="button"
                class="flex w-full items-center justify-between gap-3 rounded-md px-1 py-2 text-left text-sm font-medium text-gray-700 transition-colors hover:text-gray-900"
                @click="toggleAdvancedFilters"
              >
                <span class="flex min-w-0 items-center gap-2">
                  <SlidersHorizontal class="h-4 w-4 flex-shrink-0 text-gray-400" />
                  <span>{{ $t("recordPage.filter.advancedFilters") }}</span>
                  <Badge
                    v-if="advancedActiveFilterCount > 0"
                    variant="outline"
                    class="font-mono text-[11px]"
                  >
                    {{ advancedActiveFilterCount }}
                  </Badge>
                </span>
                <span class="flex flex-shrink-0 items-center gap-2 text-xs text-gray-500">
                  {{
                    isAdvancedFilterOpen
                      ? $t("recordPage.filter.collapse")
                      : $t("recordPage.filter.expand")
                  }}
                  <ChevronDown
                    class="h-4 w-4 transition-transform"
                    :class="{ 'rotate-180': isAdvancedFilterOpen }"
                  />
                </span>
              </button>

              <div
                v-if="isAdvancedFilterOpen"
                class="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-12"
              >
                <FilterSelect
                  class="xl:col-span-3"
                  :label="$t('recordPage.filter.userApiLabel')"
                  v-model="filters.user_api_type"
                  :options="userApiTypeOptions"
                />
                <FilterSelect
                  class="xl:col-span-3"
                  :label="$t('recordPage.filter.resolvedScopeLabel')"
                  v-model="filters.resolved_name_scope"
                  :options="resolvedScopeOptions"
                />
                <FilterSelect
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.retryLabel')"
                  v-model="filters.has_retry"
                  :options="booleanOptions"
                />
                <FilterSelect
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.fallbackLabel')"
                  v-model="filters.has_fallback"
                  :options="booleanOptions"
                />
                <FilterSelect
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.diagnosticsLabel')"
                  v-model="filters.has_transform_diagnostics"
                  :options="booleanOptions"
                />
                <FilterInput
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.errorCodeLabel')"
                  v-model="filters.final_error_code"
                />
                <FilterInput
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.latencyMinLabel')"
                  v-model="filters.latency_ms_min"
                  inputmode="numeric"
                  :placeholder="$t('recordPage.filter.msPlaceholder')"
                />
                <FilterInput
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.latencyMaxLabel')"
                  v-model="filters.latency_ms_max"
                  inputmode="numeric"
                  :placeholder="$t('recordPage.filter.msPlaceholder')"
                />
                <FilterInput
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.tokensMinLabel')"
                  v-model="filters.total_tokens_min"
                  inputmode="numeric"
                />
                <FilterInput
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.tokensMaxLabel')"
                  v-model="filters.total_tokens_max"
                  inputmode="numeric"
                />
                <FilterInput
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.costMinLabel')"
                  v-model="filters.estimated_cost_nanos_min"
                  inputmode="numeric"
                  :placeholder="$t('recordPage.filter.nanosPlaceholder')"
                />
                <FilterInput
                  class="xl:col-span-2"
                  :label="$t('recordPage.filter.costMaxLabel')"
                  v-model="filters.estimated_cost_nanos_max"
                  inputmode="numeric"
                  :placeholder="$t('recordPage.filter.nanosPlaceholder')"
                />
                <FilterInput
                  class="xl:col-span-3"
                  :label="$t('recordPage.filter.startTimeLabel')"
                  v-model="filters.start_time"
                  type="datetime-local"
                />
                <FilterInput
                  class="xl:col-span-3"
                  :label="$t('recordPage.filter.endTimeLabel')"
                  v-model="filters.end_time"
                  type="datetime-local"
                />
              </div>
            </div>

            <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <p class="text-xs text-gray-500">
                {{ $t("recordPage.filter.helpText") }}
              </p>
              <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
                <Button class="w-full sm:w-auto" @click="handleApplyFilter">
                  {{ $t("recordPage.filter.applyButton") }}
                </Button>
                <Button
                  v-if="hasActiveFilters"
                  variant="outline"
                  class="w-full md:hidden sm:w-auto"
                  @click="handleResetFilter"
                >
                  {{ $t("recordPage.filter.resetButton") }}
                </Button>
              </div>
            </div>
          </div>
        </div>

        <div v-if="isLoading" class="py-10 text-center text-gray-500">
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
                <Button class="w-full" @click="handleViewDetails(record.id)">
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
                      @click="handleViewDetails(record.id)"
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
                  @update:model-value="handlePageSizeChange"
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
              @update:page="handlePageChange"
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
      </div>
    </div>

    <RecordDetailDialog
      v-model:open="isDetailModalOpen"
      :loading="isDetailLoading"
      :record="detailedRecord"
      :attempts="detailedAttempts"
      :api-key-name="detailApiKeyName"
      :provider-name="detailProviderName"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, defineComponent, h, onMounted, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter, type LocationQuery } from "vue-router";
import {
  ChevronDown,
  CircleAlert,
  CircleCheckBig,
  CircleHelp,
  Clock3,
  Search,
  SlidersHorizontal,
  X,
} from "lucide-vue-next";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import RecordDetailDialog from "@/components/record/RecordDetailDialog.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { Api } from "@/services/request";
import { useApiKeyStore } from "@/store/apiKeyStore";
import { useModelStore } from "@/store/modelStore";
import { useProviderStore } from "@/store/providerStore";
import type { RecordAttempt, RecordListItem, RecordListParams, RecordRequest } from "@/store/types";
import {
  emptyValue,
  formatCompactMetrics,
  formatDate,
  formatDuration,
  formatLossLevel,
  formatPrice,
  getStatusBadgeVariant,
} from "@/components/record/recordFormat";

const { t: $t } = useI18n();
const route = useRoute();
const router = useRouter();
const providerStore = useProviderStore();
const apiKeyStore = useApiKeyStore();
const modelStore = useModelStore();

const DEFAULT_PAGE = 1;
const FALLBACK_PAGE_SIZE = 10;
const getStoredPageSize = () =>
  Number(localStorage.getItem("pageSize")) || FALLBACK_PAGE_SIZE;

type BooleanFilter = "ALL" | "true" | "false";

type RecordFilters = {
  api_key_id: number;
  provider_id: number;
  model_id: number;
  status: string;
  user_api_type: string;
  resolved_name_scope: string;
  final_error_code: string;
  has_retry: BooleanFilter;
  has_fallback: BooleanFilter;
  has_transform_diagnostics: BooleanFilter;
  latency_ms_min: string;
  latency_ms_max: string;
  total_tokens_min: string;
  total_tokens_max: string;
  estimated_cost_nanos_min: string;
  estimated_cost_nanos_max: string;
  start_time: string;
  end_time: string;
  search: string;
};

const DEFAULT_FILTERS: RecordFilters = {
  api_key_id: 0,
  provider_id: 0,
  model_id: 0,
  status: "ALL",
  user_api_type: "ALL",
  resolved_name_scope: "ALL",
  final_error_code: "",
  has_retry: "ALL",
  has_fallback: "ALL",
  has_transform_diagnostics: "ALL",
  latency_ms_min: "",
  latency_ms_max: "",
  total_tokens_min: "",
  total_tokens_max: "",
  estimated_cost_nanos_min: "",
  estimated_cost_nanos_max: "",
  start_time: "",
  end_time: "",
  search: "",
};

const VALID_STATUSES = new Set(["ALL", "SUCCESS", "PENDING", "ERROR", "CANCELLED"]);
const VALID_BOOLEAN_FILTERS = new Set(["ALL", "true", "false"]);
const ADVANCED_FILTER_KEYS: Array<keyof RecordFilters> = [
  "user_api_type",
  "resolved_name_scope",
  "final_error_code",
  "has_retry",
  "has_fallback",
  "has_transform_diagnostics",
  "latency_ms_min",
  "latency_ms_max",
  "total_tokens_min",
  "total_tokens_max",
  "estimated_cost_nanos_min",
  "estimated_cost_nanos_max",
  "start_time",
  "end_time",
];

const records = ref<
  (RecordListItem & {
    providerName: string;
    apiKeyName: string;
    displayRequestedModelName: string;
    attemptsDisplay: string;
    diagnosticsDisplay: string;
    firstRespTimeDisplay: string;
    totalRespTimeDisplay: string;
    tpsDisplay: string;
    costDisplay: string;
    request_at_formatted: string;
  })[]
>([]);
const totalRecords = ref(0);
const isLoading = ref(false);
const errorMsg = ref<string | null>(null);
const currentPage = ref(DEFAULT_PAGE);
const pageSize = ref(getStoredPageSize());
const searchInput = ref("");
const isFilterPanelOpen = ref(false);
const isAdvancedFilterOpen = ref(false);

const filters = reactive<RecordFilters>({ ...DEFAULT_FILTERS });

const isDetailModalOpen = ref(false);
const isDetailLoading = ref(false);
const detailedRecord = ref<RecordRequest | null>(null);
const detailedAttempts = ref<RecordAttempt[]>([]);

const totalPages = computed(() => Math.ceil(totalRecords.value / pageSize.value));

const allOption = (label: string) => ({ value: "ALL", label });
const booleanOptions = computed(() => [
  allOption($t("recordPage.filter.all")),
  { value: "true", label: $t("common.yes") },
  { value: "false", label: $t("common.no") },
]);

const apiKeyOptions = computed(() => [
  { value: "0", label: $t("recordPage.filter.allApiKeys") },
  ...(apiKeyStore.apiKeys || []).map((key) => ({
    value: String(key.id),
    label: key.name,
  })),
]);

const providerOptions = computed(() => [
  { value: "0", label: $t("recordPage.filter.allProviders") },
  ...(providerStore.providers || []).map((provider) => ({
    value: String(provider.id),
    label: provider.name,
  })),
]);

const modelOptions = computed(() => [
  { value: "0", label: $t("recordPage.filter.allModels") },
  ...modelStore.modelOptions.map((model) => ({
    value: String(model.value),
    label: model.label,
  })),
]);

const statusOptions = computed(() => [
  allOption($t("recordPage.filter.allStatuses")),
  { value: "SUCCESS", label: $t("recordPage.filter.status.SUCCESS") },
  { value: "PENDING", label: $t("recordPage.filter.status.PENDING") },
  { value: "ERROR", label: $t("recordPage.filter.status.ERROR") },
  { value: "CANCELLED", label: $t("recordPage.filter.status.CANCELLED") },
]);

const userApiTypeOptions = computed(() => [
  allOption($t("recordPage.filter.allApis")),
  { value: "OPENAI", label: "OpenAI" },
  { value: "RESPONSES", label: "Responses" },
  { value: "ANTHROPIC", label: "Anthropic" },
  { value: "GEMINI", label: "Gemini" },
  { value: "OLLAMA", label: "Ollama" },
  { value: "GEMINI_OPENAI", label: "Gemini OpenAI" },
]);

const resolvedScopeOptions = computed(() => [
  allOption($t("recordPage.filter.allScopes")),
  { value: "direct", label: $t("recordPage.filter.resolvedScopes.direct") },
  { value: "global_route", label: $t("recordPage.filter.resolvedScopes.globalRoute") },
  {
    value: "api_key_override",
    label: $t("recordPage.filter.resolvedScopes.apiKeyOverride"),
  },
]);

const hasActiveFilters = computed(() =>
  (Object.keys(DEFAULT_FILTERS) as Array<keyof RecordFilters>).some(
    (key) => filters[key] !== DEFAULT_FILTERS[key],
  ),
);

const activeFilterCount = computed(() =>
  (Object.keys(DEFAULT_FILTERS) as Array<keyof RecordFilters>).filter(
    (key) => filters[key] !== DEFAULT_FILTERS[key],
  ).length,
);

const advancedActiveFilterCount = computed(() =>
  ADVANCED_FILTER_KEYS.filter((key) => filters[key] !== DEFAULT_FILTERS[key]).length,
);

const filterSummary = computed(() => {
  if (!hasActiveFilters.value) {
    return $t("recordPage.filter.summaryAll");
  }
  return $t("recordPage.filter.summaryActive", { count: activeFilterCount.value });
});

const detailApiKeyName = computed(() =>
  detailedRecord.value ? getApiKeyName(detailedRecord.value.api_key_id) : emptyValue,
);

const detailProviderName = computed(() => {
  const record = detailedRecord.value;
  if (!record) return emptyValue;
  return (
    record.final_provider_name_snapshot ||
    getProviderName(record.final_provider_id)
  );
});

const getSingleQueryValue = (value: LocationQuery[string]) => {
  if (Array.isArray(value)) return value[0];
  return value;
};

const parsePositiveIntQuery = (value: LocationQuery[string], fallback: number) => {
  const raw = getSingleQueryValue(value);
  if (raw == null || raw === "") return fallback;
  const parsed = Number(raw);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
};

const parseStringQuery = (value: LocationQuery[string], fallback = "") => {
  const raw = getSingleQueryValue(value);
  return typeof raw === "string" ? raw : fallback;
};

const parseStatusQuery = (value: LocationQuery[string]) => {
  const raw = getSingleQueryValue(value);
  return raw && VALID_STATUSES.has(raw) ? raw : DEFAULT_FILTERS.status;
};

const parseBooleanFilterQuery = (value: LocationQuery[string]): BooleanFilter => {
  const raw = getSingleQueryValue(value);
  return raw && VALID_BOOLEAN_FILTERS.has(raw) ? (raw as BooleanFilter) : "ALL";
};

const hasProviderId = (id: number) => providerStore.providers.some((item) => item.id === id);
const hasApiKeyId = (id: number) => apiKeyStore.apiKeys.some((item) => item.id === id);
const hasModelId = (id: number) => modelStore.models.some((item) => item.id === id);
const hasActiveAdvancedFilters = () =>
  ADVANCED_FILTER_KEYS.some((key) => filters[key] !== DEFAULT_FILTERS[key]);

const applyQueryToState = (query: LocationQuery) => {
  currentPage.value = parsePositiveIntQuery(query.page, DEFAULT_PAGE);
  pageSize.value = parsePositiveIntQuery(query.page_size, getStoredPageSize());
  localStorage.setItem("pageSize", String(pageSize.value));

  const providerId = parsePositiveIntQuery(query.provider_id, DEFAULT_FILTERS.provider_id);
  const apiKeyId = parsePositiveIntQuery(query.api_key_id, DEFAULT_FILTERS.api_key_id);
  const modelId = parsePositiveIntQuery(query.model_id, DEFAULT_FILTERS.model_id);

  filters.provider_id = providerId > 0 && hasProviderId(providerId) ? providerId : 0;
  filters.api_key_id = apiKeyId > 0 && hasApiKeyId(apiKeyId) ? apiKeyId : 0;
  filters.model_id = modelId > 0 && hasModelId(modelId) ? modelId : 0;
  filters.status = parseStatusQuery(query.status);
  filters.user_api_type = parseStringQuery(query.user_api_type, "ALL") || "ALL";
  filters.resolved_name_scope = parseStringQuery(query.resolved_name_scope, "ALL") || "ALL";
  filters.final_error_code = parseStringQuery(query.final_error_code);
  filters.has_retry = parseBooleanFilterQuery(query.has_retry);
  filters.has_fallback = parseBooleanFilterQuery(query.has_fallback);
  filters.has_transform_diagnostics = parseBooleanFilterQuery(
    query.has_transform_diagnostics,
  );
  filters.latency_ms_min = parseStringQuery(query.latency_ms_min);
  filters.latency_ms_max = parseStringQuery(query.latency_ms_max);
  filters.total_tokens_min = parseStringQuery(query.total_tokens_min);
  filters.total_tokens_max = parseStringQuery(query.total_tokens_max);
  filters.estimated_cost_nanos_min = parseStringQuery(query.estimated_cost_nanos_min);
  filters.estimated_cost_nanos_max = parseStringQuery(query.estimated_cost_nanos_max);
  filters.start_time = parseStringQuery(query.start_time);
  filters.end_time = parseStringQuery(query.end_time);
  filters.search = parseStringQuery(query.search);
  searchInput.value = filters.search;
  if (hasActiveAdvancedFilters()) {
    isAdvancedFilterOpen.value = true;
  }
};

const buildQueryFromState = () => {
  const query: Record<string, string> = {};

  if (currentPage.value !== DEFAULT_PAGE) query.page = String(currentPage.value);
  if (pageSize.value !== FALLBACK_PAGE_SIZE) query.page_size = String(pageSize.value);

  (Object.keys(DEFAULT_FILTERS) as Array<keyof RecordFilters>).forEach((key) => {
    const value = filters[key];
    if (value !== DEFAULT_FILTERS[key]) {
      query[key] = String(value);
    }
  });

  return query;
};

const isSameQuery = (
  currentQuery: LocationQuery,
  nextQuery: Record<string, string>,
) => {
  const currentEntries = Object.entries(currentQuery)
    .map(([key, value]) => [key, getSingleQueryValue(value) ?? ""])
    .filter(([, value]) => value !== "")
    .sort(([left], [right]) => left.localeCompare(right));
  const nextEntries = Object.entries(nextQuery).sort(([left], [right]) =>
    left.localeCompare(right),
  );

  if (currentEntries.length !== nextEntries.length) return false;
  return currentEntries.every(([key, value], index) => {
    const [nextKey, nextValue] = nextEntries[index];
    return key === nextKey && value === nextValue;
  });
};

const syncRouteWithState = async () => {
  const nextQuery = buildQueryFromState();
  if (isSameQuery(route.query, nextQuery)) {
    return false;
  }
  await router.replace({ query: nextQuery });
  return true;
};

const numberParam = (value: string) => {
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : undefined;
};

const booleanParam = (value: BooleanFilter) => {
  if (value === "ALL") return undefined;
  return value === "true";
};

const timestampParam = (value: string) => {
  if (!value) return undefined;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : undefined;
};

const buildListParams = (): RecordListParams => ({
  page: currentPage.value,
  page_size: pageSize.value,
  api_key_id: filters.api_key_id || undefined,
  provider_id: filters.provider_id || undefined,
  model_id: filters.model_id || undefined,
  status: filters.status === "ALL" ? undefined : filters.status,
  user_api_type: filters.user_api_type === "ALL" ? undefined : filters.user_api_type,
  resolved_name_scope:
    filters.resolved_name_scope === "ALL" ? undefined : filters.resolved_name_scope,
  final_error_code: filters.final_error_code.trim() || undefined,
  has_retry: booleanParam(filters.has_retry),
  has_fallback: booleanParam(filters.has_fallback),
  has_transform_diagnostics: booleanParam(filters.has_transform_diagnostics),
  latency_ms_min: numberParam(filters.latency_ms_min),
  latency_ms_max: numberParam(filters.latency_ms_max),
  total_tokens_min: numberParam(filters.total_tokens_min),
  total_tokens_max: numberParam(filters.total_tokens_max),
  estimated_cost_nanos_min: numberParam(filters.estimated_cost_nanos_min),
  estimated_cost_nanos_max: numberParam(filters.estimated_cost_nanos_max),
  start_time: timestampParam(filters.start_time),
  end_time: timestampParam(filters.end_time),
  search: filters.search || undefined,
});

const fetchRecords = async () => {
  isLoading.value = true;
  errorMsg.value = null;

  try {
    const result = await Api.getRecordList(buildListParams());
    records.value = (result.list || []).map(enrichRecord);
    totalRecords.value = result.total || 0;
  } catch (err: unknown) {
    errorMsg.value = normalizeError(err, $t("recordPage.fetchFailed")).message;
  } finally {
    isLoading.value = false;
  }
};

const enrichRecord = (record: RecordListItem) => {
  const providerName =
    record.final_provider_name_snapshot ||
    getProviderName(record.final_provider_id);
  const apiKeyName = getApiKeyName(record.api_key_id);
  const firstRespTimeDisplay = formatDuration(
    record.first_attempt_started_at,
    record.response_started_to_client_at,
  );
  const totalRespTimeDisplay = formatDuration(
    record.first_attempt_started_at,
    record.completed_at,
  );
  const attemptsDisplay = `${record.attempt_count ?? 0} / ${record.retry_count ?? 0} / ${
    record.fallback_count ?? 0
  }`;
  const diagnosticsDisplay = record.has_transform_diagnostics
    ? `${record.transform_diagnostic_count}${
        record.transform_diagnostic_max_loss_level
          ? ` / ${formatLossLevel(record.transform_diagnostic_max_loss_level)}`
          : ""
      }`
    : "0";

  return {
    ...record,
    providerName,
    apiKeyName,
    displayRequestedModelName:
      record.final_model_name_snapshot || record.requested_model_name || emptyValue,
    attemptsDisplay,
    diagnosticsDisplay,
    firstRespTimeDisplay,
    totalRespTimeDisplay,
    tpsDisplay: calculateTps(record),
    costDisplay: formatPrice(record.estimated_cost_nanos, record.estimated_cost_currency),
    request_at_formatted: formatDate(record.request_received_at),
  };
};

const calculateTps = (record: RecordListItem) => {
  if (
    record.total_output_tokens == null ||
    record.completed_at == null ||
    record.first_attempt_started_at == null
  ) {
    return emptyValue;
  }
  const durationMs = record.completed_at - record.first_attempt_started_at;
  if (durationMs <= 0) return emptyValue;
  return (record.total_output_tokens / (durationMs / 1000)).toFixed(2);
};

const closeMobileFilterPanel = () => {
  isFilterPanelOpen.value = false;
};

const handleApplyFilter = () => {
  filters.search = searchInput.value.trim();
  currentPage.value = DEFAULT_PAGE;
  closeMobileFilterPanel();
  void syncRouteWithState().then((updated) => {
    if (!updated) void fetchRecords();
  });
};

const handleClearSearch = () => {
  if (!searchInput.value && !filters.search) return;
  searchInput.value = DEFAULT_FILTERS.search;
  filters.search = DEFAULT_FILTERS.search;
  currentPage.value = DEFAULT_PAGE;
  closeMobileFilterPanel();
  void syncRouteWithState().then((updated) => {
    if (!updated) void fetchRecords();
  });
};

const setNumberFilter = (key: "api_key_id" | "provider_id" | "model_id", val: unknown) => {
  const nextId = Number(val);
  filters[key] = Number.isInteger(nextId) && nextId >= 0 ? nextId : 0;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) void fetchRecords();
  });
};

const handleApiKeyFilterChange = (val: unknown) => setNumberFilter("api_key_id", val);
const handleProviderFilterChange = (val: unknown) => setNumberFilter("provider_id", val);
const handleModelFilterChange = (val: unknown) => setNumberFilter("model_id", val);

const handleStatusFilterChange = (val: unknown) => {
  const nextStatus =
    typeof val === "string" && VALID_STATUSES.has(val)
      ? val
      : DEFAULT_FILTERS.status;
  filters.status = nextStatus;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) void fetchRecords();
  });
};

const handleResetFilter = () => {
  searchInput.value = DEFAULT_FILTERS.search;
  Object.assign(filters, DEFAULT_FILTERS);
  currentPage.value = DEFAULT_PAGE;
  closeMobileFilterPanel();
  isAdvancedFilterOpen.value = false;
  void syncRouteWithState().then((updated) => {
    if (!updated) void fetchRecords();
  });
};

const handlePageChange = (page: number) => {
  currentPage.value = page;
  void syncRouteWithState().then((updated) => {
    if (!updated) void fetchRecords();
  });
};

const handlePageSizeChange = (val: unknown) => {
  const size = Number(val);
  if (!Number.isInteger(size) || size <= 0) return;
  pageSize.value = size;
  localStorage.setItem("pageSize", String(size));
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) void fetchRecords();
  });
};

const handleViewDetails = async (id: number) => {
  isDetailModalOpen.value = true;
  isDetailLoading.value = true;
  detailedRecord.value = null;
  detailedAttempts.value = [];

  try {
    const detail = await Api.getRecordDetail(id);
    detailedRecord.value = detail.request;
    detailedAttempts.value = [...(detail.attempts ?? [])].sort(
      (left, right) => left.attempt_index - right.attempt_index,
    );
  } catch (err: unknown) {
    toastController.error(
      $t("recordPage.detailModal.fetchFailed"),
      normalizeError(err, $t("recordPage.detailModal.fetchFailed")).message,
    );
  } finally {
    isDetailLoading.value = false;
  }
};

const toggleFilterPanel = () => {
  isFilterPanelOpen.value = !isFilterPanelOpen.value;
};

const toggleAdvancedFilters = () => {
  isAdvancedFilterOpen.value = !isAdvancedFilterOpen.value;
};

const getProviderName = (id: number | null) => {
  if (id == null) return emptyValue;
  return providerStore.providers.find((provider) => provider.id === id)?.name || emptyValue;
};

const getApiKeyName = (id: number | null) => {
  if (id == null) return emptyValue;
  return apiKeyStore.apiKeys.find((key) => key.id === id)?.name || emptyValue;
};

const getStatusMeta = (status: string | null) => {
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

watch(
  () => route.query,
  async (query) => {
    applyQueryToState(query);
    const updated = await syncRouteWithState();
    if (!updated) {
      await fetchRecords();
    }
  },
);

onMounted(async () => {
  try {
    await Promise.all([
      providerStore.fetchProviders(),
      apiKeyStore.fetchApiKeys(),
      modelStore.fetchModels(),
    ]);
    applyQueryToState(route.query);
    const updated = await syncRouteWithState();
    if (!updated) {
      await fetchRecords();
    }
  } catch (error: unknown) {
    errorMsg.value = normalizeError(error, $t("recordPage.loadFailed")).message;
  }
});

const FilterSelect = defineComponent({
  props: {
    label: { type: String, required: true },
    modelValue: { type: String, required: true },
    options: {
      type: Array as () => Array<{ value: string; label: string }>,
      required: true,
    },
  },
  emits: ["update:modelValue"],
  setup(props, { emit, attrs }) {
    return () =>
      h("div", { class: ["flex flex-col gap-1.5", attrs.class] }, [
        h("span", { class: "text-xs font-medium uppercase tracking-wide text-gray-500" }, props.label),
        h(
          Select,
          {
            modelValue: props.modelValue,
            "onUpdate:modelValue": (value: unknown) => emit("update:modelValue", String(value)),
          },
          () => [
            h(SelectTrigger, { class: "w-full" }, () => h(SelectValue)),
            h(
              SelectContent,
              { bodyLock: false },
              () =>
                props.options.map((option) =>
                  h(SelectItem, { key: option.value, value: option.value }, () => option.label),
                ),
            ),
          ],
        ),
      ]);
  },
});

const FilterInput = defineComponent({
  props: {
    label: { type: String, required: true },
    modelValue: { type: String, required: true },
    type: { type: String, default: "text" },
    inputmode: { type: String, default: undefined },
    placeholder: { type: String, default: "" },
  },
  emits: ["update:modelValue"],
  setup(props, { emit, attrs }) {
    return () =>
      h("div", { class: ["flex flex-col gap-1.5", attrs.class] }, [
        h("span", { class: "text-xs font-medium uppercase tracking-wide text-gray-500" }, props.label),
        h(Input, {
          modelValue: props.modelValue,
          type: props.type,
          inputmode: props.inputmode,
          placeholder: props.placeholder,
          "onUpdate:modelValue": (value: string | number) =>
            emit("update:modelValue", String(value)),
        }),
      ]);
  },
});

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
