<script setup lang="ts">
import { computed, onMounted, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import { RefreshCcw } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { formatTimestamp } from "@/utils/datetime";
import { formatPriceFromNanos } from "@/utils/money";
import { buildRuntimeStateBackendRows } from "@/utils/runtimeBackend";
import type {
  ProviderRuntimeItem,
  ProviderRuntimeLevel,
  ProviderRuntimeSortField,
  ProviderRuntimeStatusFilter,
  ProviderRuntimeWindow,
  RuntimeStateBackendStatus,
  SortDirection,
} from "@/services/types";
import ProviderRuntimeCards from "./components/ProviderRuntimeCards.vue";
import ProviderRuntimeFilters from "./components/ProviderRuntimeFilters.vue";
import ProviderRuntimeTable from "./components/ProviderRuntimeTable.vue";
import { useProviderRuntimeData } from "./composables/useProviderRuntimeData";
import {
  useProviderRuntimeFilters,
  type ProviderRuntimeQueryValue,
} from "./composables/useProviderRuntimeFilters";

const { t: $t } = useI18n();
const route = useRoute();
const router = useRouter();

const runtimeData = useProviderRuntimeData();
const {
  error,
  isLoading,
  items,
  refresh,
  summary,
} = runtimeData;

const routeQuery = computed(() => route.query);
const runtimeFilters = useProviderRuntimeFilters({
  routeQuery,
  router,
});
const {
  applyRouteQuery,
  filters,
  hasActiveFilters,
  resetFilters,
  searchInput,
  setFilters,
  syncRouteWithFilters,
  toApiParams,
} = runtimeFilters;

const windowOptions: Array<{ value: ProviderRuntimeWindow; label: string }> = [
  { value: "15m", label: $t("providerRuntimePage.window.15m") },
  { value: "1h", label: $t("providerRuntimePage.window.1h") },
  { value: "6h", label: $t("providerRuntimePage.window.6h") },
  { value: "24h", label: $t("providerRuntimePage.window.24h") },
];

const statusOptions: Array<{ value: ProviderRuntimeStatusFilter; label: string }> = [
  { value: "all", label: $t("providerRuntimePage.filter.all") },
  { value: "healthy", label: $t("providerRuntimePage.status.healthy") },
  { value: "degraded", label: $t("providerRuntimePage.status.degraded") },
  { value: "open", label: $t("providerRuntimePage.status.open") },
  { value: "half_open", label: $t("providerRuntimePage.status.half_open") },
  { value: "no_traffic", label: $t("providerRuntimePage.status.no_traffic") },
];

const sortOptions: Array<{ value: ProviderRuntimeSortField; label: string }> = [
  { value: "health", label: $t("providerRuntimePage.sort.health") },
  { value: "error_rate", label: $t("providerRuntimePage.sort.error_rate") },
  { value: "latency", label: $t("providerRuntimePage.sort.latency") },
  { value: "last_error_at", label: $t("providerRuntimePage.sort.last_error_at") },
  { value: "request_count", label: $t("providerRuntimePage.sort.request_count") },
];

const summaryCards = computed(() => [
  {
    key: "total",
    label: $t("providerRuntimePage.summary.total"),
    value: summary.value?.total_provider_count ?? 0,
  },
  {
    key: "healthy",
    label: $t("providerRuntimePage.summary.healthy"),
    value: summary.value?.healthy_count ?? 0,
  },
  {
    key: "degraded",
    label: $t("providerRuntimePage.summary.degraded"),
    value: summary.value?.degraded_count ?? 0,
  },
  {
    key: "half_open",
    label: $t("providerRuntimePage.summary.halfOpen"),
    value: summary.value?.half_open_count ?? 0,
  },
  {
    key: "open",
    label: $t("providerRuntimePage.summary.open"),
    value: summary.value?.open_count ?? 0,
  },
  {
    key: "no_traffic",
    label: $t("providerRuntimePage.summary.noTraffic"),
    value: summary.value?.no_traffic_count ?? 0,
  },
]);

const activeFilterSummary = computed(() => {
  const parts = [
    windowOptions.find((item) => item.value === filters.value.window)?.label ??
      filters.value.window,
    statusOptions.find((item) => item.value === filters.value.status)?.label ??
      filters.value.status,
    sortOptions.find((item) => item.value === filters.value.sort)?.label ??
      filters.value.sort,
    $t(`providerRuntimePage.filter.${filters.value.direction}`),
  ];

  if (filters.value.only_enabled) {
    parts.push($t("providerRuntimePage.activeOnly"));
  }
  if (filters.value.search) {
    parts.push(filters.value.search);
  }

  return parts.join(" · ");
});

const runtimeBackendStatus = computed(
  () => summary.value?.runtime_state_backend ?? null,
);

function backendLabel(backend: RuntimeStateBackendStatus["runtime_effective_backend"]) {
  return backend === "memory" || backend === "redis"
    ? $t(`dashboard.runtimeState.backend.${backend}`)
    : backend;
}

function deploymentModeLabel(mode: RuntimeStateBackendStatus["deployment_mode"]) {
  return mode === "single_instance" || mode === "multi_instance"
    ? $t(`dashboard.runtimeState.deployment.${mode}`)
    : mode;
}

const runtimeBackendHeadline = computed(() => {
  const status = runtimeBackendStatus.value;
  if (!status) {
    return "";
  }
  return $t("dashboard.runtimeState.description", {
    deployment: deploymentModeLabel(status.deployment_mode),
    runtime: backendLabel(status.runtime_effective_backend),
    catalog: backendLabel(status.catalog_cache_backend),
  });
});

const runtimeBackendRows = computed(() => {
  const status = runtimeBackendStatus.value;
  if (!status) {
    return [];
  }
  return buildRuntimeStateBackendRows(status).map((row) => ({
    key: row.key,
    label: $t(`dashboard.runtimeState.scope.${row.key}`),
    configured: backendLabel(row.configured),
    effective: backendLabel(row.effective),
    changed: row.changed,
  }));
});

const runtimeBackendBadgeLabel = computed(() => {
  const status = runtimeBackendStatus.value;
  if (!status) {
    return "";
  }
  if (status.runtime_degraded) {
    return $t("dashboard.runtimeState.status.degraded");
  }
  if (
    status.deployment_mode === "single_instance" &&
    status.runtime_effective_backend === "memory" &&
    !status.fallback_reason
  ) {
    return $t("dashboard.runtimeState.status.recommended");
  }
  return status.runtime_shared
    ? $t("dashboard.runtimeState.status.shared")
    : $t("dashboard.runtimeState.status.nonShared");
});

const runtimeBackendBadgeClass = computed(() => {
  const status = runtimeBackendStatus.value;
  if (status?.runtime_degraded) {
    return "border-red-200 bg-red-50 text-red-700 hover:bg-red-50";
  }
  if (status?.runtime_shared) {
    return "border-emerald-200 bg-emerald-50 text-emerald-700 hover:bg-emerald-50";
  }
  return "border-gray-200 bg-white text-gray-700 hover:bg-white";
});

const runtimeBackendPanelClass = computed(() => {
  if (runtimeBackendStatus.value?.runtime_degraded) {
    return "border-red-200 bg-red-50";
  }
  return "border-gray-200 bg-white";
});

const runtimeBackendDetail = computed(() => {
  const status = runtimeBackendStatus.value;
  if (!status) {
    return "";
  }
  if (status.fallback_reason) {
    return $t("dashboard.runtimeState.fallback", {
      reason: status.fallback_reason,
    });
  }
  if (status.catalog_cache_fallback_reason) {
    return $t("dashboard.runtimeState.catalogFallback", {
      reason: status.catalog_cache_fallback_reason,
    });
  }
  if (
    status.deployment_mode === "single_instance" &&
    status.runtime_effective_backend === "memory"
  ) {
    return $t("dashboard.runtimeState.recommendedHint");
  }
  return status.runtime_shared
    ? $t("dashboard.runtimeState.sharedHint")
    : $t("dashboard.runtimeState.nonSharedHint");
});

async function applyRouteQueryAndFetch() {
  applyRouteQuery();
  await refresh(toApiParams());
}

function formatPercentage(value: number | null) {
  if (value == null) {
    return "-";
  }
  return `${(value * 100).toFixed(1)}%`;
}

function formatLatency(value: number | null) {
  if (value == null) {
    return "-";
  }
  return `${Math.round(value).toLocaleString()} ms`;
}

function formatCount(value: number | null | undefined) {
  if (value == null) {
    return "-";
  }
  return value.toLocaleString();
}

function formatDateTime(value: number | null | undefined) {
  return formatTimestamp(value) || "-";
}

function formatCost(nanos: number, currency: string) {
  return formatPriceFromNanos(nanos, currency, "-");
}

function runtimeLevelLabel(level: ProviderRuntimeLevel) {
  return $t(`providerRuntimePage.status.${level}`);
}

function runtimeBadgeClass(level: ProviderRuntimeLevel) {
  switch (level) {
    case "open":
      return "border-red-200 bg-red-50 text-red-700 hover:bg-red-50";
    case "half_open":
      return "border-amber-200 bg-amber-50 text-amber-700 hover:bg-amber-50";
    case "degraded":
      return "border-orange-200 bg-orange-50 text-orange-700 hover:bg-orange-50";
    case "healthy":
      return "border-emerald-200 bg-emerald-50 text-emerald-700 hover:bg-emerald-50";
    case "no_traffic":
      return "border-gray-200 bg-gray-100 text-gray-600 hover:bg-gray-100";
  }
}

function buildPrimaryMetrics(item: ProviderRuntimeItem) {
  return [
    {
      label: $t("providerRuntimePage.metrics.requests"),
      value: formatCount(item.request_count),
    },
    {
      label: $t("providerRuntimePage.metrics.successRate"),
      value: formatPercentage(item.success_rate),
    },
    {
      label: $t("providerRuntimePage.metrics.firstByte"),
      value: formatLatency(item.avg_first_byte_ms),
    },
    {
      label: $t("providerRuntimePage.metrics.totalLatency"),
      value: formatLatency(item.avg_total_latency_ms),
    },
    {
      label: $t("providerRuntimePage.metrics.errors"),
      value: formatCount(item.error_count),
    },
    {
      label: $t("providerRuntimePage.metrics.failures"),
      value: formatCount(item.consecutive_failures),
    },
    {
      label: $t("providerRuntimePage.metrics.sortScore"),
      value: item.sort_score.toFixed(2),
    },
    {
      label: $t("providerRuntimePage.metrics.proxy"),
      value: item.use_proxy ? $t("common.yes") : $t("common.no"),
    },
  ];
}

function openProviderRecords(item: ProviderRuntimeItem) {
  const query: Record<string, string> = {
    provider_id: String(item.provider_id),
  };

  if (
    item.runtime_level === "open" ||
    item.runtime_level === "half_open" ||
    item.runtime_level === "degraded"
  ) {
    query.status = "ERROR";
  }

  void router.push({
    path: "/record",
    query,
  });
}

async function pushFilters(next: {
  window?: ProviderRuntimeWindow;
  status?: ProviderRuntimeStatusFilter;
  sort?: ProviderRuntimeSortField;
  direction?: SortDirection;
  only_enabled?: boolean;
  search?: string;
}) {
  setFilters(next);
  const routeUpdated = await syncRouteWithFilters();
  if (!routeUpdated) {
    await refresh(toApiParams());
  }
}

function handleWindowChange(window: ProviderRuntimeWindow) {
  if (window === filters.value.window) {
    return;
  }
  void pushFilters({ window });
}

function handleStatusChange(value: ProviderRuntimeStatusFilter) {
  void pushFilters({ status: value });
}

function handleSortChange(value: ProviderRuntimeSortField) {
  void pushFilters({ sort: value });
}

function handleToggleDirection() {
  void pushFilters({
    direction: filters.value.direction === "desc" ? "asc" : "desc",
  });
}

function handleOnlyEnabledChange(value: boolean) {
  void pushFilters({ only_enabled: value });
}

function handleSearchApply() {
  void pushFilters({ search: searchInput.value.trim() });
}

function handleSearchClear() {
  if (!searchInput.value && !filters.value.search) {
    return;
  }
  searchInput.value = "";
  void pushFilters({ search: "" });
}

function handleRefresh() {
  searchInput.value = filters.value.search;
  void refresh(toApiParams());
}

function handleReset() {
  resetFilters();
  void syncRouteWithFilters().then((updated) => {
    if (!updated) {
      void refresh(toApiParams());
    }
  });
}

function goToProviderEdit(providerId: number) {
  void router.push(`/provider/edit/${providerId}`);
}

watch(
  () => route.query as Record<string, ProviderRuntimeQueryValue>,
  () => {
    void applyRouteQueryAndFetch();
  },
);

onMounted(() => {
  void applyRouteQueryAndFetch();
});
</script>

<template>
  <div class="app-page">
    <div class="app-page-shell">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold tracking-tight text-gray-900 sm:text-xl">
            {{ $t("providerRuntimePage.title") }}
          </h1>
          <p class="mt-1 text-sm text-gray-500">
            {{ $t("providerRuntimePage.description") }}
          </p>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
          <Button
            variant="outline"
            class="w-full sm:w-auto"
            :disabled="isLoading"
            @click="handleReset"
          >
            {{ $t("providerRuntimePage.reset") }}
          </Button>
          <Button class="w-full sm:w-auto" :disabled="isLoading" @click="handleRefresh">
            <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isLoading }" />
            {{ $t("providerRuntimePage.refresh") }}
          </Button>
        </div>
      </div>

      <div class="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
        <Card
          v-for="summaryCard in summaryCards"
          :key="summaryCard.key"
          class="border border-gray-200 shadow-none"
        >
          <CardContent class="px-4 py-4">
            <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ summaryCard.label }}
            </p>
            <p class="mt-2 text-2xl font-semibold tracking-tight text-gray-900">
              {{ summaryCard.value }}
            </p>
          </CardContent>
        </Card>
      </div>

      <div
        v-if="runtimeBackendStatus"
        class="rounded-lg border px-4 py-3"
        :class="runtimeBackendPanelClass"
      >
        <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0">
            <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("dashboard.runtimeState.title") }}
            </p>
            <p class="mt-1 text-sm font-medium text-gray-900">
              {{ runtimeBackendHeadline }}
            </p>
            <p class="mt-1 text-xs text-gray-500">
              {{ runtimeBackendDetail }}
            </p>
            <dl class="mt-3 grid grid-cols-1 gap-x-4 gap-y-2 text-xs sm:grid-cols-2">
              <div
                v-for="row in runtimeBackendRows"
                :key="`provider-runtime-backend-${row.key}`"
                class="min-w-0"
              >
                <dt class="font-medium uppercase tracking-wide text-gray-400">
                  {{ row.label }}
                </dt>
                <dd class="mt-1 flex flex-wrap items-center gap-1.5 text-gray-600">
                  <span>{{ $t("dashboard.runtimeState.configured") }}</span>
                  <span class="font-mono font-medium text-gray-900">
                    {{ row.configured }}
                  </span>
                  <span class="text-gray-300">/</span>
                  <span>{{ $t("dashboard.runtimeState.effective") }}</span>
                  <span
                    class="font-mono font-medium"
                    :class="row.changed ? 'text-amber-700' : 'text-gray-900'"
                  >
                    {{ row.effective }}
                  </span>
                </dd>
              </div>
            </dl>
          </div>
          <Badge :class="runtimeBackendBadgeClass">
            {{ runtimeBackendBadgeLabel }}
          </Badge>
        </div>
        <p
          v-if="runtimeBackendStatus.last_error"
          class="mt-2 break-words text-xs text-red-600"
        >
          {{ $t("dashboard.runtimeState.lastError", { error: runtimeBackendStatus.last_error }) }}
        </p>
      </div>

      <ProviderRuntimeFilters
        v-model:search-input="searchInput"
        :filters="filters"
        :active-filter-summary="activeFilterSummary"
        :window-options="windowOptions"
        :status-options="statusOptions"
        :sort-options="sortOptions"
        @apply-search="handleSearchApply"
        @clear-search="handleSearchClear"
        @select-window="handleWindowChange"
        @select-status="handleStatusChange"
        @select-sort="handleSortChange"
        @toggle-direction="handleToggleDirection"
        @update-only-enabled="handleOnlyEnabledChange"
      />

      <div
        v-if="isLoading && !items.length"
        class="flex items-center justify-center rounded-xl border border-gray-200 bg-white py-16"
      >
        <span class="text-sm text-gray-500">{{ $t("providerRuntimePage.loading") }}</span>
      </div>

      <div
        v-else-if="error"
        class="rounded-xl border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600"
      >
        {{ error }}
      </div>

      <div
        v-else-if="!items.length"
        class="rounded-xl border border-gray-200 bg-white px-4 py-16 text-center text-sm text-gray-500"
      >
        {{ hasActiveFilters ? $t("providerRuntimePage.empty") : $t("providerRuntimePage.noData") }}
      </div>

      <template v-else>
        <ProviderRuntimeTable
          :items="items"
          :format-count="formatCount"
          :format-cost="formatCost"
          :format-date-time="formatDateTime"
          :format-latency="formatLatency"
          :format-percentage="formatPercentage"
          :runtime-badge-class="runtimeBadgeClass"
          :runtime-level-label="runtimeLevelLabel"
          @edit-provider="goToProviderEdit"
          @view-records="openProviderRecords"
        />
        <ProviderRuntimeCards
          :items="items"
          :build-primary-metrics="buildPrimaryMetrics"
          :format-cost="formatCost"
          :format-date-time="formatDateTime"
          :runtime-badge-class="runtimeBadgeClass"
          :runtime-level-label="runtimeLevelLabel"
          @edit-provider="goToProviderEdit"
          @view-records="openProviderRecords"
        />
      </template>
    </div>
  </div>
</template>
