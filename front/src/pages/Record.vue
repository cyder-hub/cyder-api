<template>
  <div class="h-[calc(100vh-3rem)] flex flex-col gap-6">
    <div class="flex justify-between items-start">
      <div>
        <h1 class="text-lg font-semibold text-gray-900 tracking-tight">
          {{ $t("recordPage.title") }}
        </h1>
        <p class="mt-1 text-sm text-gray-500">
          {{ $t("recordPage.description") || $t("recordPage.title") }}
        </p>
      </div>
    </div>

    <!-- Filters -->
    <div
      class="flex flex-wrap items-center gap-4 p-4 flex-shrink-0 bg-white rounded-lg border border-gray-200"
    >
      <div class="flex flex-col space-y-1">
        <Select
          :model-value="String(filters.api_key_id)"
          @update:model-value="handleApiKeyFilterChange"
        >
          <SelectTrigger class="w-[200px]">
            <SelectValue :placeholder="$t('recordPage.filter.allApiKeys')"/>
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="opt in apiKeyOptions"
              :key="opt.value"
              :value="String(opt.value)"
            >
              {{ opt.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div class="flex flex-col space-y-1">
        <Select
          :model-value="String(filters.provider_id)"
          @update:model-value="handleProviderFilterChange"
        >
          <SelectTrigger class="w-[200px]">
            <SelectValue :placeholder="$t('recordPage.filter.allProviders')"/>
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="opt in providerOptions"
              :key="opt.value"
              :value="String(opt.value)"
            >
              {{ opt.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div class="flex flex-col space-y-1">
        <Select
          :model-value="filters.status"
          @update:model-value="handleStatusFilterChange"
        >
          <SelectTrigger class="w-[150px]">
            <SelectValue :placeholder="$t('recordPage.filter.allStatuses')"/>
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="opt in statusOptions"
              :key="opt.value"
              :value="opt.value"
            >
              {{ opt.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div class="relative w-[250px]">
        <Input
          v-model="searchInput"
          :placeholder="$t('recordPage.filter.searchPlaceholder')"
          class="pr-9"
          @keydown.enter="handleApplyFilter"
        />
        <button
          v-if="searchInput"
          type="button"
          class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-gray-400 transition-colors hover:text-gray-600"
          @click="handleClearSearch"
        >
          <X class="h-4 w-4" />
        </button>
      </div>

      <div class="flex gap-2 flex-wrap">
        <Button @click="handleApplyFilter">{{
          $t("recordPage.filter.applyButton")
        }}</Button>
        <Button
          v-if="hasActiveFilters"
          variant="outline"
          @click="handleResetFilter"
        >
          {{ $t("recordPage.filter.resetButton") }}
        </Button>
      </div>
    </div>

    <!-- Table -->
    <div
      v-if="isLoading"
      class="text-center py-10 text-gray-500"
    >
      <div
        class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900 mb-2"
      ></div>
      <div>{{ $t("recordPage.loading") }}</div>
    </div>

    <div
      v-else-if="errorMsg"
      class="text-center py-4 text-red-600 bg-red-100 border border-red-400 rounded p-4"
    >
      {{ $t("recordPage.errorPrefix") }} {{ errorMsg }}
    </div>

    <div
      v-else
      class="flex-1 min-h-0 border border-gray-200 rounded-lg bg-white flex flex-col"
    >
      <div class="flex-1 overflow-auto">
        <Table>
          <TableHeader class="bg-gray-50/80 hover:bg-gray-50/80">
            <TableRow>
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("recordPage.table.modelName") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("recordPage.table.provider") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("recordPage.table.apiKey") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("recordPage.table.status") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("recordPage.table.promptTokens") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("recordPage.table.completionTokens") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("recordPage.table.reasoningTokens") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("recordPage.table.totalTokens") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("recordPage.table.stream") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("recordPage.table.firstResp") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("recordPage.table.totalResp") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("recordPage.table.tps") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("recordPage.table.cost") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("recordPage.table.requestTime") }}</TableHead
              >
              <TableHead
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >{{ $t("recordPage.table.details") }}</TableHead
              >
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow v-if="records.length === 0">
              <TableCell colspan="15"class="text-center py-6">
                {{
                  totalRecords === 0
                    ? $t("recordPage.table.noRecordsMatch")
                    : $t("recordPage.table.noRecordsAvailable")
                }}
              </TableCell>
            </TableRow>
            <TableRow
              v-for="record in records"
              :key="record.id"
              class="hover:bg-gray-50 "
            >
              <TableCell class="font-medium">{{
                record.model_name || "/"
              }}</TableCell>
              <TableCell>{{ record.providerName }}</TableCell>
              <TableCell>{{ record.apiKeyName }}</TableCell>
              <TableCell>
                <Badge :variant="getStatusBadgeVariant(record.status)">
                  {{ record.status || $t("common.notAvailable") }}
                </Badge>
              </TableCell>
              <TableCell class="text-right">{{
                record.input_tokens ?? "/"
              }}</TableCell>
              <TableCell class="text-right">{{
                record.output_tokens ?? "/"
              }}</TableCell>
              <TableCell class="text-right">{{
                record.reasoning_tokens ?? "/"
              }}</TableCell>
              <TableCell class="text-right font-semibold">{{
                record.total_tokens ?? "/"
              }}</TableCell>
              <TableCell>{{ record.isStreamDisplay }}</TableCell>
              <TableCell class="text-right">{{
                record.firstRespTimeDisplay
              }}</TableCell>
              <TableCell class="text-right">{{
                record.totalRespTimeDisplay
              }}</TableCell>
              <TableCell class="text-right text-gray-500">{{
                record.tpsDisplay
              }}</TableCell>
              <TableCell class="text-right font-mono text-gray-600">{{
                record.costDisplay
              }}</TableCell>
              <TableCell class="whitespace-nowrap text-gray-500 text-sm">{{
                record.request_at_formatted
              }}</TableCell>
              <TableCell class="text-right">
                <Button
                  variant="link"
                  size="sm"
                  @click="handleViewDetails(record.id)"
                  class="px-0"
                >
                  {{ $t("recordPage.table.viewDetails") }}
                </Button>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>

      <!-- Pagination -->
      <div
        v-if="totalPages > 0"
        class="flex-shrink-0 mt-auto flex items-center justify-between pt-2 flex-wrap gap-4"
      >
        <div
          class="flex items-center gap-4 flex-wrap order-2 sm:order-1 text-sm text-gray-500"
        >
          <div>
            {{ $t("recordPage.pagination.page") }}
            <span class="font-medium text-gray-900">{{ currentPage }}</span>
            {{ $t("recordPage.pagination.of") }}
            <span class="font-medium text-gray-900">{{ totalPages }}</span>
            (<span class="font-medium text-gray-900">{{ totalRecords }}</span>
            {{ $t("recordPage.pagination.items") }})
          </div>
          <div class="flex items-center space-x-2">
            <label class="whitespace-nowrap">{{
              $t("recordPage.pagination.itemsPerPage")
            }}</label>
            <Select
              :model-value="String(pageSize)"
              @update:model-value="handlePageSizeChange"
            >
              <SelectTrigger class="w-[70px] h-8 text-xs">
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
          @update:page="handlePageChange"
          show-edges
          class="order-1 sm:order-2 mx-0 w-auto"
        >
          <PaginationContent v-slot="{ items }"class="flex items-center gap-1">
            <PaginationFirst />
            <PaginationPrevious />
            <template v-for="(item, index) in items":key="index">
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

    <!-- Detail Dialog -->
    <Dialog v-model:open="isDetailModalOpen">
      <DialogContent
        class="sm:max-w-[85vw] md:max-w-6xl max-h-[90vh] flex flex-col p-0"
      >
        <DialogHeader class="p-6 pb-4 border-b border-gray-100">
          <DialogTitle class="text-lg font-semibold text-gray-900">{{
            $t("recordPage.detailModal.title", "Log Details")
          }}</DialogTitle>
        </DialogHeader>

        <div class="flex-grow overflow-y-auto p-6 pt-2">
          <div v-if="isDetailLoading"class="text-center py-10">
            <div
              class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900  mb-2"
            ></div>
            <div>{{ $t("recordPage.loading") }}</div>
          </div>
          <div v-else-if="detailedRecord"class="space-y-6 text-sm">
            <section>
              <h3
                class="text-base font-semibold text-gray-900  border-b  pb-2 mb-2"
              >
                {{ $t('recordPage.detailModal.general') }}
              </h3>
              <dl class="divide-y divide-gray-100 ">
                <DetailItem label="ID">{{ detailedRecord.id }}</DetailItem>
                <DetailItem label="Status">
                  <Badge
                    :variant="getStatusBadgeVariant(detailedRecord.status)"
                  >
                    {{ detailedRecord.status || $t("common.notAvailable") }}
                  </Badge>
                </DetailItem>
                <DetailItem label="Provider">{{
                  getProviderName(detailedRecord.provider_id)
                }}</DetailItem>
                <DetailItem label="API Key">{{
                  getApiKeyName(detailedRecord.system_api_key_id)
                }}</DetailItem>
                <DetailItem label="Model Name">{{
                  detailedRecord.model_name
                }}</DetailItem>
                <DetailItem label="Real Model Name">{{
                  detailedRecord.real_model_name
                }}</DetailItem>
                <DetailItem label="User API Type">{{
                  detailedRecord.user_api_type || $t("common.notAvailable")
                }}</DetailItem>
                <DetailItem label="LLM API Type">{{
                  detailedRecord.llm_api_type || $t("common.notAvailable")
                }}</DetailItem>
                <DetailItem label="Stream">{{
                  detailedRecord.is_stream ? $t("common.yes") : $t("common.no")
                }}</DetailItem>
              </dl>
            </section>

            <section>
              <h3
                class="text-base font-semibold text-gray-900  border-b  pb-2 mb-2"
              >
                {{ $t('recordPage.detailModal.timings') }}
              </h3>
              <dl class="divide-y divide-gray-100 ">
                <DetailItem label="Request Received">{{
                  formatDate(detailedRecord.request_received_at)
                }}</DetailItem>
                <DetailItem label="LLM Request Sent">{{
                  formatDate(detailedRecord.llm_request_sent_at)
                }}</DetailItem>
                <DetailItem label="LLM First Chunk">{{
                  formatDate(detailedRecord.llm_response_first_chunk_at)
                }}</DetailItem>
                <DetailItem label="LLM Completed">{{
                  formatDate(detailedRecord.llm_response_completed_at)
                }}</DetailItem>
                <DetailItem label="Response to Client">{{
                  formatDate(detailedRecord.response_sent_to_client_at)
                }}</DetailItem>
              </dl>
            </section>

            <section>
              <h3
                class="text-base font-semibold text-gray-900  border-b  pb-2 mb-2"
              >
                Usage & Cost
              </h3>
              <dl class="divide-y divide-gray-100 ">
                <DetailItem label="Prompt Tokens">{{
                  detailedRecord.input_tokens
                }}</DetailItem>
                <DetailItem label="Completion Tokens">{{
                  detailedRecord.output_tokens
                }}</DetailItem>
                <DetailItem
                  v-if="detailedRecord.input_image_tokens"
                  label="Input Image Tokens"
                  >{{ detailedRecord.input_image_tokens }}</DetailItem
                >
                <DetailItem
                  v-if="detailedRecord.output_image_tokens"
                  label="Output Image Tokens"
                  >{{ detailedRecord.output_image_tokens }}</DetailItem
                >
                <DetailItem
                  v-if="detailedRecord.cached_tokens"
                  label="Cached Tokens"
                  >{{ detailedRecord.cached_tokens }}</DetailItem
                >
                <DetailItem
                  v-if="detailedRecord.reasoning_tokens"
                  label="Reasoning Tokens"
                  >{{ detailedRecord.reasoning_tokens }}</DetailItem
                >
                <DetailItem label="Total Tokens"class="font-bold">{{
                  detailedRecord.total_tokens
                }}</DetailItem>
                <DetailItem label="Calculated Cost">{{
                  detailedRecord.calculated_cost != null
                    ? `${detailedRecord.cost_currency || ""} ${(detailedRecord.calculated_cost / 1000000000).toFixed(6)}`
                    : "/"
                }}</DetailItem>
                <DetailItem label="Storage Type">{{
                  detailedRecord.storage_type
                }}</DetailItem>
              </dl>
            </section>

            <section>
              <h3
                class="text-base font-semibold text-gray-900 border-b pb-2 mb-2"
              >
                Payloads
              </h3>
              <div v-if="showPayloads" class="space-y-4">
                <template v-if="detailedRecord.storage_type">
                  <BodyViewer
                    :record-id="detailedRecord.id"
                    :storage-type="detailedRecord.storage_type"
                    :status="detailedRecord.status"
                  />
                </template>
                <template v-else>
                  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <SingleRequestBodyContent
                      :content="detailedRecord.user_request_body"
                      title="User Request Body"
                    />
                    <SingleRequestBodyContent
                      :content="detailedRecord.llm_request_body"
                      title="LLM Request Body"
                    />
                    <SingleResponseBodyContent
                      :content="detailedRecord.llm_response_body"
                      title="LLM Response Body"
                      :status="detailedRecord.status"
                    />
                    <SingleResponseBodyContent
                      :content="detailedRecord.user_response_body"
                      title="User Response Body"
                      :status="detailedRecord.status"
                    />
                  </div>
                </template>
              </div>
              <p v-else class="text-sm text-gray-500">
                Rendering request and response payloads...
              </p>
            </section>
          </div>
        </div>

        <DialogFooter class="p-6 pt-2 border-t ">
          <Button variant="secondary"@click="isDetailModalOpen = false">
            {{ $t("common.close", "Close") }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>

<script setup lang="ts">
import {
  ref,
  reactive,
  computed,
  onMounted,
  watch,
  nextTick,
} from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter, type LocationQuery } from "vue-router";
import { X } from "lucide-vue-next";
import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import { useProviderStore } from "@/store/providerStore";
import { useApiKeyStore } from "@/store/apiKeyStore";
import { toastController } from "@/lib/toastController";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
import { Badge } from "@/components/ui/badge";
import type { RecordListItem, RecordDetail } from "@/store/types";
import DetailItem from "@/components/record/DetailItem.vue";
import BodyViewer from "@/components/record/BodyViewer.vue";
import { formatTimestamp } from "@/lib/utils";

const { t: $t } = useI18n();
const route = useRoute();
const router = useRouter();
const providerStore = useProviderStore();
const apiKeyStore = useApiKeyStore();

const DEFAULT_PAGE = 1;
const FALLBACK_PAGE_SIZE = 10;
const getStoredPageSize = () =>
  Number(localStorage.getItem("pageSize")) || FALLBACK_PAGE_SIZE;
type RecordFilters = {
  api_key_id: number;
  provider_id: number;
  status: string;
  search: string;
};
const DEFAULT_FILTERS: RecordFilters = {
  api_key_id: 0,
  provider_id: 0,
  status: "ALL",
  search: "",
};
const VALID_STATUSES = new Set(["ALL", "SUCCESS", "PENDING", "ERROR"]);

// --- State ---
const records = ref<
  (RecordListItem & {
    providerName: string;
    apiKeyName: string;
    isStreamDisplay: string;
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

const filters = reactive<RecordFilters>({
  api_key_id: DEFAULT_FILTERS.api_key_id,
  provider_id: DEFAULT_FILTERS.provider_id,
  status: DEFAULT_FILTERS.status,
  search: DEFAULT_FILTERS.search,
});

const isDetailModalOpen = ref(false);
const isDetailLoading = ref(false);
const detailedRecord = ref<RecordDetail | null>(null);
const showPayloads = ref(false);

// --- Computed ---
const totalPages = computed(() =>
  Math.ceil(totalRecords.value / pageSize.value),
);

const hasActiveFilters = computed(() => {
  return (
    filters.api_key_id !== 0 ||
    filters.provider_id !== 0 ||
    filters.status !== "ALL"||
    filters.search !== ""
  );
});

const apiKeyOptions = computed(() => {
  const allKey = { value: 0, label: $t("recordPage.filter.allApiKeys") };
  const keys = (apiKeyStore.apiKeys || []).map((k) => ({
    value: k.id,
    label: k.name,
  }));
  return [allKey, ...keys];
});

const providerOptions = computed(() => {
  const allProvider = { value: 0, label: $t("recordPage.filter.allProviders") };
  const providers = (providerStore.providers || []).map((p) => ({
    value: p.provider.id,
    label: p.provider.name,
  }));
  return [allProvider, ...providers];
});

const statusOptions = computed(() => {
  const allStatus = {
    value: DEFAULT_FILTERS.status,
    label: $t("recordPage.filter.allStatuses"),
  };
  const statuses = ["SUCCESS", "PENDING", "ERROR"].map((s) => ({
    value: s,
    label: $t(`recordPage.filter.status.${s}`),
  }));
  return [allStatus, ...statuses];
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

const parseStatusQuery = (value: LocationQuery[string]) => {
  const raw = getSingleQueryValue(value);
  return raw && VALID_STATUSES.has(raw) ? raw : DEFAULT_FILTERS.status;
};

const parseSearchQuery = (value: LocationQuery[string]) => {
  const raw = getSingleQueryValue(value);
  return typeof raw === "string" ? raw : DEFAULT_FILTERS.search;
};

const hasProviderId = (id: number) => {
  return providerStore.providers.some((item) => item.provider.id === id);
};

const hasApiKeyId = (id: number) => {
  return apiKeyStore.apiKeys.some((item) => item.id === id);
};

const applyQueryToState = (query: LocationQuery) => {
  currentPage.value = parsePositiveIntQuery(query.page, DEFAULT_PAGE);
  pageSize.value = parsePositiveIntQuery(query.page_size, getStoredPageSize());
  localStorage.setItem("pageSize", String(pageSize.value));

  const providerId = parsePositiveIntQuery(
    query.provider_id,
    DEFAULT_FILTERS.provider_id,
  );
  const apiKeyId = parsePositiveIntQuery(
    query.api_key_id,
    DEFAULT_FILTERS.api_key_id,
  );

  filters.provider_id =
    providerId > 0 && hasProviderId(providerId)
      ? providerId
      : DEFAULT_FILTERS.provider_id;
  filters.api_key_id =
    apiKeyId > 0 && hasApiKeyId(apiKeyId)
      ? apiKeyId
      : DEFAULT_FILTERS.api_key_id;
  filters.status = parseStatusQuery(query.status);
  filters.search = parseSearchQuery(query.search);
  searchInput.value = filters.search;
};

const buildQueryFromState = () => {
  const query: Record<string, string> = {};
  if (currentPage.value !== DEFAULT_PAGE) {
    query.page = String(currentPage.value);
  }
  if (pageSize.value !== FALLBACK_PAGE_SIZE) {
    query.page_size = String(pageSize.value);
  }
  if (filters.provider_id !== DEFAULT_FILTERS.provider_id) {
    query.provider_id = String(filters.provider_id);
  }
  if (filters.api_key_id !== DEFAULT_FILTERS.api_key_id) {
    query.api_key_id = String(filters.api_key_id);
  }
  if (filters.status !== DEFAULT_FILTERS.status) {
    query.status = filters.status;
  }
  if (filters.search !== DEFAULT_FILTERS.search) {
    query.search = filters.search;
  }
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

// --- Methods ---
const fetchRecords = async () => {
  isLoading.value = true;
  errorMsg.value = null;
  try {
    const params = {
      page: currentPage.value,
      page_size: pageSize.value,
      system_api_key_id: filters.api_key_id || undefined,
      provider_id: filters.provider_id || undefined,
      status:
        filters.status === DEFAULT_FILTERS.status ? undefined : filters.status,
      search: filters.search || undefined,
    };
    const result = await Api.getRecordList(params);

    records.value = (result.list || []).map((r: RecordListItem) => {
      // Formatting and display logic
      const providerName =
        r.provider_id != null
          ? providerStore.providers.find((p) => p.provider.id === r.provider_id)
              ?.provider.name || "/"
          : "/";
      const apiKeyName =
        r.system_api_key_id != null
          ? apiKeyStore.apiKeys.find((k) => k.id === r.system_api_key_id)
              ?.name || "/"
          : "/";
      const isStreamDisplay = r.is_stream ? $t("common.yes") : $t("common.no");

      const firstRespTimeDisplay =
        r.llm_response_first_chunk_at != null && r.llm_request_sent_at != null
          ? (
              (r.llm_response_first_chunk_at - r.llm_request_sent_at) /
              1000
            ).toFixed(3)
          : "/";
      const totalRespTimeDisplay =
        r.llm_response_completed_at != null && r.llm_request_sent_at != null
          ? (
              (r.llm_response_completed_at - r.llm_request_sent_at) /
              1000
            ).toFixed(3)
          : "/";

      let tpsDisplay = "/";
      if (r.output_tokens != null && r.llm_response_completed_at != null) {
        let durationMs;
        if (r.is_stream) {
          if (r.llm_response_first_chunk_at != null) {
            durationMs =
              r.llm_response_completed_at - r.llm_response_first_chunk_at;
          }
        } else {
          if (r.llm_request_sent_at != null) {
            durationMs = r.llm_response_completed_at - r.llm_request_sent_at;
          }
        }
        if (durationMs != null && durationMs > 0) {
          tpsDisplay = (r.output_tokens / (durationMs / 1000)).toFixed(2);
        }
      }

      const costDisplay =
        r.calculated_cost != null
          ? `${r.cost_currency || ""} ${(r.calculated_cost / 1000000000).toFixed(6)}`
          : "/";

      return {
        ...r,
        providerName,
        apiKeyName,
        isStreamDisplay,
        firstRespTimeDisplay,
        totalRespTimeDisplay,
        tpsDisplay,
        costDisplay,
        request_at_formatted: formatDate(r.request_received_at),
      };
    });
    totalRecords.value = result.total || 0;
  } catch (err: unknown) {
    console.error("Failed to fetch records:", err);
    errorMsg.value = (err as Error).message || String(err);
  } finally {
    isLoading.value = false;
  }
};

const handleApplyFilter = () => {
  filters.search = searchInput.value.trim();
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleClearSearch = () => {
  if (!searchInput.value && !filters.search) return;
  searchInput.value = DEFAULT_FILTERS.search;
  filters.search = DEFAULT_FILTERS.search;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleApiKeyFilterChange = (val: unknown) => {
  const nextId = Number(val);
  filters.api_key_id =
    Number.isInteger(nextId) && nextId >= 0 ? nextId : DEFAULT_FILTERS.api_key_id;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleProviderFilterChange = (val: unknown) => {
  const nextId = Number(val);
  filters.provider_id =
    Number.isInteger(nextId) && nextId >= 0 ? nextId : DEFAULT_FILTERS.provider_id;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleStatusFilterChange = (val: unknown) => {
  const nextStatus =
    typeof val === "string" && VALID_STATUSES.has(val)
      ? val
      : DEFAULT_FILTERS.status;
  filters.status = nextStatus;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleResetFilter = () => {
  searchInput.value = DEFAULT_FILTERS.search;
  filters.api_key_id = DEFAULT_FILTERS.api_key_id;
  filters.provider_id = DEFAULT_FILTERS.provider_id;
  filters.status = DEFAULT_FILTERS.status;
  filters.search = DEFAULT_FILTERS.search;
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handlePageChange = (page: number) => {
  currentPage.value = page;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handlePageSizeChange = (val: unknown) => {
  const size = Number(val);
  if (!Number.isInteger(size) || size <= 0) return;
  pageSize.value = size;
  localStorage.setItem("pageSize", String(size));
  currentPage.value = DEFAULT_PAGE;
  void syncRouteWithState().then((updated) => {
    if (!updated) {
      void fetchRecords();
    }
  });
};

const handleViewDetails = async (id: number) => {
  isDetailModalOpen.value = true;
  isDetailLoading.value = true;
  detailedRecord.value = null;
  showPayloads.value = false;
  try {
    detailedRecord.value = await Api.getRecordDetail(id);
    await nextTick();
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (isDetailModalOpen.value && detailedRecord.value?.id === id) {
          showPayloads.value = true;
        }
      });
    });
  } catch (err: unknown) {
    console.error("Failed to fetch record detail:", err);
    toastController.error(
      $t("recordPage.detailModal.fetchFailed", "Failed to fetch record detail"),
      (err as Error).message || String(err),
    );
  } finally {
    isDetailLoading.value = false;
  }
};

watch(isDetailModalOpen, (isOpen) => {
  if (!isOpen) {
    showPayloads.value = false;
  }
});

const formatDate = (timestamp: number | null | undefined) => {
  return formatTimestamp(timestamp) || "/";
};

const getStatusBadgeVariant = (status: string | null) => {
  switch (status) {
    case "SUCCESS":
      return "default"; // Success is usually default/green in Shadcn
    case "ERROR":
      return "destructive";
    case "PENDING":
      return "outline";
    default:
      return "secondary";
  }
};

const getProviderName = (id: number | null) => {
  if (id == null) return "/";
  return (
    providerStore.providers.find((p) => p.provider.id === id)?.provider.name ||
    "/"
  );
};

const getApiKeyName = (id: number | null) => {
  if (id == null) return "/";
  return apiKeyStore.apiKeys.find((k) => k.id === id)?.name || "/";
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
    await Promise.all([providerStore.fetchProviders(), apiKeyStore.fetchApiKeys()]);
    applyQueryToState(route.query);
    const updated = await syncRouteWithState();
    if (!updated) {
      await fetchRecords();
    }
  } catch (error: unknown) {
    errorMsg.value = normalizeError(error, $t("common.unknownError")).message;
  }
});
</script>

<style scoped>
/* Any additional local styles */
</style>
