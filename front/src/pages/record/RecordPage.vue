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

        <RecordFilters
          v-model:search-input="searchInput"
          :filters="filters"
          :filter-summary="filterSummary"
          :is-filter-panel-open="isFilterPanelOpen"
          :is-advanced-filter-open="isAdvancedFilterOpen"
          :has-active-filters="hasActiveFilters"
          :advanced-active-filter-count="advancedActiveFilterCount"
          :api-key-options="apiKeyOptions"
          :provider-options="providerOptions"
          :model-options="modelOptions"
          :status-options="statusOptions"
          :user-api-type-options="userApiTypeOptions"
          :resolved-scope-options="resolvedScopeOptions"
          :boolean-options="booleanOptions"
          @toggle-filter-panel="toggleFilterPanel"
          @toggle-advanced-filters="toggleAdvancedFilters"
          @update-filter="handleFilterChange"
          @update-number-filter="handleNumberFilterChange"
          @update-status-filter="handleStatusFilterChange"
          @apply="handleApplyFilter"
          @clear-search="handleClearSearch"
          @reset="handleResetFilter"
        />

        <RecordTable
          :records="records"
          :loading="isLoading"
          :error-msg="errorMsg"
          :current-page="currentPage"
          :page-size="pageSize"
          :total-pages="totalPages"
          :total-records="totalRecords"
          @view-details="handleViewDetails"
          @page-change="handlePageChange"
          @page-size-change="handlePageSizeChange"
        />
      </div>
    </div>

    <RecordDetailDialog
      v-model:open="isDetailModalOpen"
      v-model:active-tab="selectedTab"
      :loading="isDetailLoading"
      :record="detailedRecord"
      :attempts="detailedAttempts"
      :artifacts="artifacts"
      :artifacts-loading="artifactsLoading"
      :artifacts-error="artifactsError"
      :selected-attempt-id="selectedAttemptId"
      :selected-replay-run-id="selectedReplayRunId"
      :api-key-name="detailApiKeyName"
      :provider-name="detailProviderName"
      @reload-artifacts="loadArtifacts(true)"
      @update:selected-attempt-id="handleSelectedAttemptIdChange"
      @update:selected-replay-run-id="handleSelectedReplayRunIdChange"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import RecordDetailDialog from "./components/RecordDetailDialog.vue";
import RecordFilters from "./components/RecordFilters.vue";
import RecordTable from "./components/RecordTable.vue";
import { toastController } from "../../services/uiFeedback";
import { normalizeError } from "../../utils/error";
import * as recordService from "../../services/records";
import { useApiKeyStore } from "../../store/apiKeyStore";
import { useModelStore } from "../../store/modelStore";
import { useProviderStore } from "../../store/providerStore";
import type { RecordFilters as RecordFiltersState } from "./types";
import {
  RECORD_DETAIL_TABS,
  shouldLoadRecordArtifacts,
} from "./composables/useRecordDetail";
import { useRecordDetail } from "./composables/useRecordDetail";
import { useRecordList } from "./composables/useRecordList";
import {
  DEFAULT_RECORD_FILTERS,
  DEFAULT_RECORD_PAGE,
  VALID_RECORD_STATUSES,
  useRecordQuery,
} from "./composables/useRecordQuery";

const { t: $t } = useI18n();
const route = useRoute();
const router = useRouter();
const providerStore = useProviderStore();
const apiKeyStore = useApiKeyStore();
const modelStore = useModelStore();

const isInitialized = ref(false);
const isFilterPanelOpen = ref(false);

const queryState = useRecordQuery({
  route,
  router,
  validators: {
    hasProviderId: (id) => providerStore.providers.some((item) => item.id === id),
    hasApiKeyId: (id) => apiKeyStore.apiKeys.some((item) => item.id === id),
    hasModelId: (id) => modelStore.modelOptions.some((item) => Number(item.value) === id),
  },
});

const {
  currentPage,
  pageSize,
  searchInput,
  filters,
  isAdvancedFilterOpen,
  selectedRecordId,
  selectedTab,
  selectedAttemptId,
  selectedReplayRunId,
  applyQueryToState,
  syncRouteWithState,
  buildListParams,
  clearDetailSelection,
} = queryState;

const recordList = useRecordList({
  filters,
  currentPage,
  pageSize,
  buildListParams,
  t: $t,
  providerStore,
  apiKeyStore,
  modelStore,
  api: recordService,
});

const {
  records,
  totalRecords,
  totalPages,
  isLoading,
  errorMsg,
  booleanOptions,
  apiKeyOptions,
  providerOptions,
  modelOptions,
  statusOptions,
  userApiTypeOptions,
  resolvedScopeOptions,
  hasActiveFilters,
  advancedActiveFilterCount,
  filterSummary,
  fetchRecords,
  loadFilterOptions,
  getProviderName,
  getApiKeyName,
} = recordList;

const recordDetail = useRecordDetail({
  t: $t,
  getApiKeyName,
  getProviderName,
  api: recordService,
});

const {
  isDetailLoading,
  detailedRecord,
  detailedAttempts,
  detailApiKeyName,
  detailProviderName,
  artifacts,
  artifactsLoading,
  artifactsError,
  loadDetail,
  loadArtifacts,
  resetDetail,
} = recordDetail;

const isDetailModalOpen = computed({
  get: () => selectedRecordId.value != null,
  set: (open: boolean) => {
    if (open) return;
    clearDetailSelection();
    resetDetail();
    void syncRouteWithState();
  },
});

const closeMobileFilterPanel = () => {
  isFilterPanelOpen.value = false;
};

const refreshFromState = () => {
  void syncRouteWithState().then((updated) => {
    if (!updated) void fetchRecords();
  });
};

const handleApplyFilter = () => {
  filters.search = searchInput.value.trim();
  currentPage.value = DEFAULT_RECORD_PAGE;
  closeMobileFilterPanel();
  refreshFromState();
};

const handleClearSearch = () => {
  if (!searchInput.value && !filters.search) return;
  searchInput.value = DEFAULT_RECORD_FILTERS.search;
  filters.search = DEFAULT_RECORD_FILTERS.search;
  currentPage.value = DEFAULT_RECORD_PAGE;
  closeMobileFilterPanel();
  refreshFromState();
};

const handleFilterChange = (key: keyof RecordFiltersState, value: string) => {
  if (key === "api_key_id" || key === "provider_id" || key === "model_id") {
    handleNumberFilterChange(key, value);
    return;
  }
  filters[key] = value as never;
  currentPage.value = DEFAULT_RECORD_PAGE;
  refreshFromState();
};

const handleNumberFilterChange = (
  key: "api_key_id" | "provider_id" | "model_id",
  value: string,
) => {
  const nextId = Number(value);
  filters[key] = Number.isInteger(nextId) && nextId >= 0 ? nextId : 0;
  currentPage.value = DEFAULT_RECORD_PAGE;
  refreshFromState();
};

const handleStatusFilterChange = (value: string) => {
  filters.status = VALID_RECORD_STATUSES.has(value)
    ? value
    : DEFAULT_RECORD_FILTERS.status;
  currentPage.value = DEFAULT_RECORD_PAGE;
  refreshFromState();
};

const handleResetFilter = () => {
  searchInput.value = DEFAULT_RECORD_FILTERS.search;
  Object.assign(filters, DEFAULT_RECORD_FILTERS);
  currentPage.value = DEFAULT_RECORD_PAGE;
  closeMobileFilterPanel();
  isAdvancedFilterOpen.value = false;
  refreshFromState();
};

const handlePageChange = (page: number) => {
  currentPage.value = page;
  refreshFromState();
};

const handlePageSizeChange = (value: string) => {
  const size = Number(value);
  if (!Number.isInteger(size) || size <= 0) return;
  pageSize.value = size;
  localStorage.setItem("pageSize", String(size));
  currentPage.value = DEFAULT_RECORD_PAGE;
  refreshFromState();
};

const handleViewDetails = (id: number) => {
  selectedRecordId.value = id;
  selectedTab.value = "overview";
  selectedAttemptId.value = null;
  selectedReplayRunId.value = null;
  void syncRouteWithState();
};

const handleSelectedAttemptIdChange = (attemptId: number | null) => {
  selectedAttemptId.value = attemptId;
  void syncRouteWithState();
};

const handleSelectedReplayRunIdChange = (replayRunId: number | null) => {
  selectedReplayRunId.value = replayRunId;
  void syncRouteWithState();
};

const toggleFilterPanel = () => {
  isFilterPanelOpen.value = !isFilterPanelOpen.value;
};

const toggleAdvancedFilters = () => {
  isAdvancedFilterOpen.value = !isAdvancedFilterOpen.value;
};

watch(
  () => route.query,
  async (query) => {
    if (!isInitialized.value) return;
    applyQueryToState(query);
    const updated = await syncRouteWithState();
    if (!updated) {
      await fetchRecords();
    }
  },
);

watch(selectedRecordId, async (recordId) => {
  if (recordId == null) {
    resetDetail();
    return;
  }

  try {
    await loadDetail(recordId);
  } catch (err: unknown) {
    toastController.error(
      $t("recordPage.detailModal.fetchFailed"),
      normalizeError(err, $t("recordPage.detailModal.fetchFailed")).message,
    );
  }
});

watch(
  [selectedTab, detailedRecord],
  ([tab]) => {
    if (shouldLoadRecordArtifacts(tab)) {
      void loadArtifacts();
    }
  },
);

watch(selectedTab, (tab) => {
  if (!RECORD_DETAIL_TABS.some((item) => item.value === tab)) {
    selectedTab.value = "overview";
  }
  if (tab !== "replay") {
    selectedReplayRunId.value = null;
  }
  void syncRouteWithState();
});

onMounted(async () => {
  try {
    await loadFilterOptions();
    applyQueryToState(route.query);
    isInitialized.value = true;
    const updated = await syncRouteWithState();
    if (!updated) {
      await fetchRecords();
    }
  } catch (error: unknown) {
    errorMsg.value = normalizeError(error, $t("recordPage.loadFailed")).message;
  }
});
</script>
