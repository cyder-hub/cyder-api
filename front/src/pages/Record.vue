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
          @update:model-value="(val) => (filters.api_key_id = Number(val))"
        >
          <SelectTrigger class="w-[200px]">
            <SelectValue :placeholder="$t('recordPage.filter.allApiKeys')" />
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
          @update:model-value="(val) => (filters.provider_id = Number(val))"
        >
          <SelectTrigger class="w-[200px]">
            <SelectValue :placeholder="$t('recordPage.filter.allProviders')" />
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
        <Select v-model="filters.status">
          <SelectTrigger class="w-[150px]">
            <SelectValue :placeholder="$t('recordPage.filter.allStatuses')" />
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

      <Input
        v-model="searchInput"
        :placeholder="$t('recordPage.filter.searchPlaceholder')"
        class="w-[250px]"
      />

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
      class="text-center py-10 text-gray-500 dark:text-gray-400"
    >
      <div
        class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900 dark:border-gray-100 mb-2"
      ></div>
      <div>{{ $t("recordPage.loading") }}</div>
    </div>

    <div
      v-else-if="errorMsg"
      class="text-center py-4 text-red-600 bg-red-100 dark:bg-red-900/30 border border-red-400 dark:border-red-800 rounded p-4"
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
              <TableCell colspan="15" class="text-center py-6">
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
              class="hover:bg-gray-50 dark:hover:bg-zinc-800/50"
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
          <PaginationContent v-slot="{ items }" class="flex items-center gap-1">
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
          <div v-if="isDetailLoading" class="text-center py-10">
            <div
              class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900 dark:border-gray-100 mb-2"
            ></div>
            <div>{{ $t("recordPage.loading") }}</div>
          </div>
          <div v-else-if="detailedRecord" class="space-y-6 text-sm">
            <section>
              <h3
                class="text-base font-semibold text-gray-900 dark:text-gray-100 border-b dark:border-zinc-700 pb-2 mb-2"
              >
                General
              </h3>
              <dl class="divide-y divide-gray-100 dark:divide-zinc-800">
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
                <DetailItem label="Stream">{{
                  detailedRecord.is_stream ? $t("common.yes") : $t("common.no")
                }}</DetailItem>
              </dl>
            </section>

            <section>
              <h3
                class="text-base font-semibold text-gray-900 dark:text-gray-100 border-b dark:border-zinc-700 pb-2 mb-2"
              >
                Timings (UTC)
              </h3>
              <dl class="divide-y divide-gray-100 dark:divide-zinc-800">
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
                class="text-base font-semibold text-gray-900 dark:text-gray-100 border-b dark:border-zinc-700 pb-2 mb-2"
              >
                Usage & Cost
              </h3>
              <dl class="divide-y divide-gray-100 dark:divide-zinc-800">
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
                <DetailItem label="Total Tokens" class="font-bold">{{
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
                class="text-base font-semibold text-gray-900 dark:text-gray-100 border-b dark:border-zinc-700 pb-2 mb-2"
              >
                Payloads
              </h3>
              <div class="space-y-4">
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
            </section>
          </div>
        </div>

        <DialogFooter class="p-6 pt-2 border-t dark:border-zinc-700">
          <Button variant="secondary" @click="isDetailModalOpen = false">
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
  h,
  defineComponent,
} from "vue";
import { useI18n } from "vue-i18n";
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
import { parseSse } from "@/utils/sse";
import * as msgpack from "@msgpack/msgpack";
import { applyPatch } from "fast-json-patch";
import type { RecordListItem, RecordDetail } from "@/store/types";

const { t: $t } = useI18n();
const providerStore = useProviderStore();
const apiKeyStore = useApiKeyStore();

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

const currentPage = ref(1);
const pageSize = ref(Number(localStorage.getItem("pageSize")) || 10);
const searchInput = ref("");
let searchDebounceTimer: any = null;

const filters = reactive({
  api_key_id: 0,
  provider_id: 0,
  status: "ALL",
  search: "",
});

const isDetailModalOpen = ref(false);
const isDetailLoading = ref(false);
const detailedRecord = ref<RecordDetail | null>(null);

// --- Computed ---
const totalPages = computed(() =>
  Math.ceil(totalRecords.value / pageSize.value),
);

const hasActiveFilters = computed(() => {
  return (
    filters.api_key_id !== 0 ||
    filters.provider_id !== 0 ||
    filters.status !== "ALL" ||
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
    value: "ALL",
    label: $t("recordPage.filter.allStatuses"),
  };
  const statuses = ["SUCCESS", "PENDING", "ERROR"].map((s) => ({
    value: s,
    label: $t(`recordPage.filter.status.${s}`),
  }));
  return [allStatus, ...statuses];
});

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
      status: filters.status === "ALL" ? undefined : filters.status,
      search: filters.search || undefined,
    };
    const result = await Api.getRecordList(params);

    records.value = (result.list || []).map((r: any) => {
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
  } catch (err: any) {
    console.error("Failed to fetch records:", err);
    errorMsg.value = err.message || String(err);
  } finally {
    isLoading.value = false;
  }
};

const handleApplyFilter = () => {
  currentPage.value = 1;
  fetchRecords();
};

const handleResetFilter = () => {
  searchInput.value = "";
  filters.api_key_id = 0;
  filters.provider_id = 0;
  filters.status = "ALL";
  filters.search = "";
  currentPage.value = 1;
  fetchRecords();
};

const handlePageChange = (page: number) => {
  currentPage.value = page;
  fetchRecords();
};

const handlePageSizeChange = (val: any) => {
  const size = Number(val);
  pageSize.value = size;
  localStorage.setItem("pageSize", String(size));
  currentPage.value = 1;
  fetchRecords();
};

const handleViewDetails = async (id: number) => {
  isDetailModalOpen.value = true;
  isDetailLoading.value = true;
  detailedRecord.value = null;
  try {
    detailedRecord.value = await Api.getRecordDetail(id);
  } catch (err: any) {
    console.error("Failed to fetch record detail:", err);
    toastController.error(
      $t("recordPage.detailModal.fetchFailed", "Failed to fetch record detail"),
      err.message || String(err),
    );
  } finally {
    isDetailLoading.value = false;
  }
};

const formatDate = (timestamp: number | null | undefined) => {
  if (!timestamp) return "/";
  try {
    const date = new Date(timestamp);
    if (isNaN(date.getTime())) return "/";
    return date.toISOString().replace("T", " ").substring(0, 19);
  } catch (e) {
    return "/";
  }
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

// --- Watchers ---
watch(searchInput, (newVal) => {
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
  searchDebounceTimer = setTimeout(() => {
    filters.search = newVal;
  }, 600);
});

onMounted(async () => {
  await Promise.all([providerStore.loadProviders(), apiKeyStore.loadApiKeys()]);
  fetchRecords();
});

// --- Internal Components using render functions (h()) ---
// Note: template strings don't work with Vue 3 runtime-only build (default with Vite).
// Using h() render functions instead.

// Detail Item Component
const DetailItem = defineComponent({
  props: ["label"],
  setup(props, { slots }) {
    return () =>
      h("div", { class: "py-2 sm:grid sm:grid-cols-3 sm:gap-4" }, [
        h(
          "dt",
          { class: "text-sm font-medium text-gray-500 dark:text-gray-400" },
          props.label,
        ),
        h(
          "dd",
          {
            class:
              "mt-1 text-sm text-gray-900 dark:text-gray-100 sm:mt-0 sm:col-span-2",
          },
          slots.default?.() ?? "/",
        ),
      ]);
  },
});

// SSE Event Viewer
const SseEventViewer = defineComponent({
  props: ["event"],
  setup(props) {
    const eventData = computed(() => {
      if (!props.event.data) return { type: "empty" };
      try {
        return {
          type: "json",
          content: JSON.stringify(JSON.parse(props.event.data), null, 2),
        };
      } catch (e) {
        return { type: "text", content: props.event.data };
      }
    });
    return () =>
      h(
        "div",
        {
          class:
            "mb-2 border-b border-gray-200 dark:border-zinc-700 pb-2 last:border-b-0 last:pb-0",
        },
        [
          h(
            "p",
            { class: "font-semibold text-gray-600 dark:text-gray-400" },
            `event: ${props.event.event}`,
          ),
          eventData.value.type !== "empty"
            ? h(
                "pre",
                { class: "mt-1 whitespace-pre-wrap break-all text-[10px]" },
                eventData.value.content,
              )
            : null,
        ],
      );
  },
});

// Single Request Body Content
const SingleRequestBodyContent = defineComponent({
  props: ["content", "title"],
  setup(props, { slots }) {
    const displayContent = computed(() => {
      if (!props.content) return { type: "empty" };
      try {
        return {
          type: "json",
          content: JSON.stringify(JSON.parse(props.content), null, 2),
        };
      } catch (e) {
        return { type: "text", content: props.content };
      }
    });
    return () =>
      h("div", null, [
        h("div", { class: "flex items-center justify-between mb-1" }, [
          h(
            "h4",
            {
              class:
                "text-sm font-medium text-gray-700 dark:text-gray-300 py-1",
            },
            props.title,
          ),
          slots.action?.(),
        ]),
        displayContent.value.type !== "empty"
          ? h(
              "div",
              {
                class:
                  "mt-1 text-[10px] bg-gray-50 dark:bg-zinc-900/50 p-2 rounded-md max-h-[30rem] overflow-y-auto border dark:border-zinc-800",
              },
              [
                h(
                  "pre",
                  { class: "whitespace-pre-wrap break-all" },
                  displayContent.value.content,
                ),
              ],
            )
          : null,
      ]);
  },
});

// Single Response Body Content
const SingleResponseBodyContent = defineComponent({
  props: ["content", "title", "status"],
  setup(props) {
    const contentToDisplay = computed(() => {
      if (!props.content) return { type: "empty" as const };
      try {
        return {
          type: "json" as const,
          content: JSON.stringify(JSON.parse(props.content), null, 2),
        };
      } catch (e) {}

      if (props.status === "SUCCESS") {
        try {
          const sseEvents = parseSse(props.content);
          if (sseEvents.some((e: any) => e.data && e.data.trim() !== "")) {
            return { type: "sse" as const, content: sseEvents };
          }
        } catch (e) {}
      }
      return { type: "text" as const, content: props.content };
    });

    return () => {
      if (contentToDisplay.value.type === "empty") return null;
      return h("div", null, [
        h(
          "h4",
          {
            class: "text-sm font-medium text-gray-700 dark:text-gray-300 mb-1",
          },
          props.title,
        ),
        h(
          "div",
          {
            class:
              "mt-1 text-[10px] bg-gray-50 dark:bg-zinc-900/50 p-2 rounded-md max-h-[30rem] overflow-y-auto border dark:border-zinc-800",
          },
          contentToDisplay.value.type === "sse"
            ? (contentToDisplay.value.content as any[]).map(
                (ev: any, idx: number) =>
                  h(SseEventViewer, { key: idx, event: ev }),
              )
            : [
                h(
                  "pre",
                  { class: "whitespace-pre-wrap break-all" },
                  (contentToDisplay.value as any).content,
                ),
              ],
        ),
      ]);
    };
  },
});

// Body Viewer (Handles Msgpack Decoding and JSON Patching)
const BodyViewer = defineComponent({
  props: ["recordId", "storageType", "status"],
  setup(props) {
    const bodies = ref<any>(null);
    const isLoadingBodies = ref(false);
    const showPatched = ref(true);

    const fetchAndDecodeBody = async () => {
      if (!props.storageType || !props.recordId) return;
      isLoadingBodies.value = true;
      try {
        const buffer = await Api.getRecordContent(props.recordId);
        const decoded = msgpack.decode(new Uint8Array(buffer)) as any;
        const textDecoder = new TextDecoder();
        bodies.value = {
          user_request_body: decoded.user_request_body
            ? textDecoder.decode(decoded.user_request_body)
            : null,
          llm_request_body: decoded.llm_request_body
            ? textDecoder.decode(decoded.llm_request_body)
            : null,
          user_response_body: decoded.user_response_body
            ? textDecoder.decode(decoded.user_response_body)
            : null,
          llm_response_body: decoded.llm_response_body
            ? textDecoder.decode(decoded.llm_response_body)
            : null,
        };
      } catch (error) {
        console.error("Failed to fetch or decode body content:", error);
      } finally {
        isLoadingBodies.value = false;
      }
    };

    const patchInfo = computed(() => {
      const userContent = bodies.value?.user_request_body;
      const llmContent = bodies.value?.llm_request_body;
      if (!userContent || !llmContent || userContent === llmContent) {
        return { isPatch: false, patchedContent: null };
      }
      try {
        const userJson = JSON.parse(userContent);
        const patch = JSON.parse(llmContent);
        if (
          Array.isArray(patch) &&
          patch.every((op) => "op" in op && "path" in op)
        ) {
          const { newDocument } = applyPatch(userJson, patch, true, false);
          return {
            isPatch: true,
            patchedContent: JSON.stringify(newDocument, null, 2),
          };
        }
      } catch (e) {}
      return { isPatch: false, patchedContent: null };
    });

    onMounted(fetchAndDecodeBody);

    return () => {
      if (isLoadingBodies.value) {
        return h("div", { class: "text-center py-4" }, "Loading bodies...");
      }
      if (!bodies.value) return null;

      const requestSection = (() => {
        const b = bodies.value;
        if (
          b.user_request_body !== b.llm_request_body &&
          b.user_request_body &&
          b.llm_request_body
        ) {
          const llmContent =
            patchInfo.value.isPatch && showPatched.value
              ? patchInfo.value.patchedContent
              : b.llm_request_body;
          const llmTitle =
            patchInfo.value.isPatch && showPatched.value
              ? "LLM Request Body (Patched)"
              : "LLM Request Body (Raw Patch)";
          return h("div", { class: "grid grid-cols-1 md:grid-cols-2 gap-4" }, [
            h(SingleRequestBodyContent, {
              content: b.user_request_body,
              title: "User Request Body",
            }),
            h(
              SingleRequestBodyContent,
              { content: llmContent, title: llmTitle },
              {
                action: patchInfo.value.isPatch
                  ? () =>
                      h(
                        Button,
                        {
                          size: "sm",
                          variant: "ghost",
                          onClick: () => {
                            showPatched.value = !showPatched.value;
                          },
                          class: "h-6 text-[10px]",
                        },
                        () =>
                          showPatched.value
                            ? "Show Raw Patch"
                            : "Show Patched Body",
                      )
                  : undefined,
              },
            ),
          ]);
        }
        return h(SingleRequestBodyContent, {
          content: b.user_request_body || b.llm_request_body,
          title: "Request Body",
        });
      })();

      const responseSection = (() => {
        const b = bodies.value;
        if (
          b.user_response_body !== b.llm_response_body &&
          b.user_response_body &&
          b.llm_response_body
        ) {
          return h("div", { class: "grid grid-cols-1 md:grid-cols-2 gap-4" }, [
            h(SingleResponseBodyContent, {
              content: b.llm_response_body,
              title: "LLM Response Body",
              status: props.status,
            }),
            h(SingleResponseBodyContent, {
              content: b.user_response_body,
              title: "User Response Body",
              status: props.status,
            }),
          ]);
        }
        return h(SingleResponseBodyContent, {
          content: b.user_response_body || b.llm_response_body,
          title: "Response Body",
          status: props.status,
        });
      })();

      return h("div", { class: "space-y-4" }, [
        requestSection,
        responseSection,
      ]);
    };
  },
});
</script>

<style scoped>
/* Any additional local styles */
</style>
