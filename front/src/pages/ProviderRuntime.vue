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
            :disabled="store.isLoading"
            @click="handleReset"
          >
            {{ $t("providerRuntimePage.reset") }}
          </Button>
          <Button class="w-full sm:w-auto" :disabled="store.isLoading" @click="handleRefresh">
            <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': store.isLoading }" />
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

      <div class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
        <div class="flex flex-col gap-3 border-b border-gray-100 pb-4 md:flex-row md:items-start md:justify-between">
          <div class="min-w-0">
            <h2 class="text-base font-semibold text-gray-900">
              {{ $t("providerRuntimePage.filter.title") }}
            </h2>
            <p class="mt-1 text-sm text-gray-500">
              {{ activeFilterSummary }}
            </p>
          </div>
          <div class="flex flex-wrap gap-2">
            <Button
              v-for="windowOption in windowOptions"
              :key="windowOption.value"
              :variant="store.filters.window === windowOption.value ? 'default' : 'outline'"
              size="sm"
              @click="handleWindowChange(windowOption.value)"
            >
              {{ windowOption.label }}
            </Button>
          </div>
        </div>

        <div class="mt-4 grid grid-cols-1 gap-3 lg:grid-cols-12">
          <div class="lg:col-span-4">
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerRuntimePage.searchPlaceholder") }}
            </span>
            <div class="relative">
              <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
              <Input
                v-model="searchInput"
                class="w-full pl-9 pr-9"
                :placeholder="$t('providerRuntimePage.searchPlaceholder')"
                @keydown.enter="handleSearchApply"
              />
              <button
                v-if="searchInput"
                type="button"
                class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-gray-400 transition-colors hover:text-gray-600"
                @click="handleSearchClear"
              >
                <X class="h-4 w-4" />
              </button>
            </div>
          </div>

          <div class="lg:col-span-3">
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerRuntimePage.filter.status") }}
            </span>
            <Select
              :model-value="store.filters.status"
              @update:model-value="handleStatusChange"
            >
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent :body-lock="false">
                <SelectItem
                  v-for="statusOption in statusOptions"
                  :key="statusOption.value"
                  :value="statusOption.value"
                >
                  {{ statusOption.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div class="lg:col-span-3">
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerRuntimePage.filter.sort") }}
            </span>
            <Select
              :model-value="store.filters.sort"
              @update:model-value="handleSortChange"
            >
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent :body-lock="false">
                <SelectItem
                  v-for="sortOption in sortOptions"
                  :key="sortOption.value"
                  :value="sortOption.value"
                >
                  {{ sortOption.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div class="lg:col-span-2">
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerRuntimePage.filter.direction") }}
            </span>
            <Button
              variant="outline"
              class="w-full justify-between"
              @click="handleToggleDirection"
            >
              <span>
                {{
                  $t(
                    `providerRuntimePage.filter.${store.filters.direction}`,
                  )
                }}
              </span>
              <ArrowUpDown class="h-4 w-4 text-gray-400" />
            </Button>
          </div>
        </div>

        <div class="mt-4 flex flex-col gap-3 border-t border-gray-100 pt-4 sm:flex-row sm:items-center sm:justify-between">
          <label class="inline-flex items-center gap-2 text-sm text-gray-600">
            <Checkbox
              :model-value="store.filters.only_enabled"
              @update:model-value="handleOnlyEnabledChange"
            />
            <span>{{ $t("providerRuntimePage.activeOnly") }}</span>
          </label>
          <Button variant="outline" class="sm:hidden" @click="handleSearchApply">
            {{ $t("recordPage.filter.applyButton") }}
          </Button>
          <Button variant="outline" class="hidden sm:inline-flex" @click="handleSearchApply">
            {{ $t("recordPage.filter.applyButton") }}
          </Button>
        </div>
      </div>

      <div
        v-if="store.isLoading && !store.items.length"
        class="flex items-center justify-center rounded-xl border border-gray-200 bg-white py-16"
      >
        <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm text-gray-500">{{ $t("providerRuntimePage.loading") }}</span>
      </div>

      <div
        v-else-if="store.error"
        class="rounded-xl border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600"
      >
        {{ store.error }}
      </div>

      <div
        v-else-if="!store.items.length"
        class="rounded-xl border border-gray-200 bg-white px-4 py-16 text-center text-sm text-gray-500"
      >
        {{ store.hasActiveFilters ? $t("providerRuntimePage.empty") : $t("providerRuntimePage.noData") }}
      </div>

      <div v-else class="grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card
          v-for="item in store.items"
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
                  <Badge :class="runtimeBadgeClass(item.runtime_level)">
                    {{ runtimeLevelLabel(item.runtime_level) }}
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
                  @click="router.push(`/provider/edit/${item.provider_id}`)"
                >
                  <Pencil class="mr-1 h-3.5 w-3.5" />
                  {{ $t("providerRuntimePage.editProvider") }}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-7 px-2 text-xs text-gray-500"
                  @click="openProviderRecords(item)"
                >
                  <FileText class="mr-1 h-3.5 w-3.5" />
                  {{ $t("providerRuntimePage.viewRecords") }}
                </Button>
                <Badge variant="outline" class="bg-gray-50 text-[11px] text-gray-500">
                  {{ $t("providerRuntimePage.metrics.models") }}: {{ item.enabled_model_count }}
                </Badge>
                <Badge variant="outline" class="bg-gray-50 text-[11px] text-gray-500">
                  {{ $t("providerRuntimePage.metrics.keys") }}: {{ item.enabled_provider_key_count }}
                </Badge>
                <Badge variant="outline" class="bg-gray-50 text-[11px] text-gray-500">
                  {{ $t("providerRuntimePage.metrics.proxy") }}:
                  {{ item.use_proxy ? $t("common.yes") : $t("common.no") }}
                </Badge>
              </div>
            </div>
          </CardHeader>

          <CardContent class="space-y-4 px-4 pb-4 sm:px-5">
            <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
              <div
                v-for="metric in buildPrimaryMetrics(item)"
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
                <p class="mt-1 text-gray-900">{{ formatDateTime(item.last_request_at) }}</p>
              </div>
              <div class="rounded-lg border border-gray-100 px-3 py-3">
                <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("providerRuntimePage.metrics.lastSuccess") }}
                </p>
                <p class="mt-1 text-gray-900">{{ formatDateTime(item.last_success_at) }}</p>
              </div>
              <div class="rounded-lg border border-gray-100 px-3 py-3 sm:col-span-2">
                <div class="flex items-start justify-between gap-3">
                  <div class="min-w-0">
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ $t("providerRuntimePage.metrics.lastError") }}
                    </p>
                    <p class="mt-1 text-gray-900">{{ formatDateTime(item.last_error_at) }}</p>
                  </div>
                  <Badge variant="outline" class="shrink-0 bg-gray-50 text-[11px] text-gray-500">
                    {{ $t("providerRuntimePage.metrics.failures") }}:
                    {{ item.consecutive_failures }}
                  </Badge>
                </div>
                <p class="mt-2 break-words text-xs text-gray-500">
                  {{ item.last_error_summary || item.last_error || $t("providerRuntimePage.detail.noError") }}
                </p>
              </div>
            </div>

            <div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
              <div class="rounded-lg border border-gray-100 px-3 py-3">
                <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("providerRuntimePage.detail.statusCode") }}
                </p>
                <div
                  v-if="item.status_code_breakdown.length"
                  class="mt-2 flex flex-wrap gap-2"
                >
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
                    {{ formatCost(cost.amount_nanos, cost.currency) }}
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
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import type { LocationQuery } from "vue-router";
import {
  ArrowUpDown,
  FileText,
  Loader2,
  Pencil,
  RefreshCcw,
  Search,
  X,
} from "lucide-vue-next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { formatPriceFromNanos, formatTimestamp } from "@/lib/utils";
import { useProviderRuntimeStore } from "@/store/providerRuntimeStore";
import type {
  ProviderRuntimeItem,
  ProviderRuntimeLevel,
  ProviderRuntimeSortField,
  ProviderRuntimeStatusFilter,
  ProviderRuntimeWindow,
  SortDirection,
} from "@/store/types";

type QueryValue = string | null | Array<string | null> | undefined;

const { t: $t } = useI18n();
const route = useRoute();
const router = useRouter();
const store = useProviderRuntimeStore();
const searchInput = ref("");

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

const summaryCards = computed(() => {
  const summary = store.summary;
  return [
    {
      key: "total",
      label: $t("providerRuntimePage.summary.total"),
      value: summary?.total_provider_count ?? 0,
    },
    {
      key: "healthy",
      label: $t("providerRuntimePage.summary.healthy"),
      value: summary?.healthy_count ?? 0,
    },
    {
      key: "degraded",
      label: $t("providerRuntimePage.summary.degraded"),
      value: summary?.degraded_count ?? 0,
    },
    {
      key: "half_open",
      label: $t("providerRuntimePage.summary.halfOpen"),
      value: summary?.half_open_count ?? 0,
    },
    {
      key: "open",
      label: $t("providerRuntimePage.summary.open"),
      value: summary?.open_count ?? 0,
    },
    {
      key: "no_traffic",
      label: $t("providerRuntimePage.summary.noTraffic"),
      value: summary?.no_traffic_count ?? 0,
    },
  ];
});

const activeFilterSummary = computed(() => {
  const parts = [
    windowOptions.find((item) => item.value === store.filters.window)?.label ??
      store.filters.window,
    statusOptions.find((item) => item.value === store.filters.status)?.label ??
      store.filters.status,
    sortOptions.find((item) => item.value === store.filters.sort)?.label ??
      store.filters.sort,
    $t(`providerRuntimePage.filter.${store.filters.direction}`),
  ];

  if (store.filters.only_enabled) {
    parts.push($t("providerRuntimePage.activeOnly"));
  }
  if (store.filters.search) {
    parts.push(store.filters.search);
  }

  return parts.join(" · ");
});

function getSingleQueryValue(value: QueryValue): string | undefined {
  if (Array.isArray(value)) {
    return value[0] ?? undefined;
  }
  return value ?? undefined;
}

function isSameQuery(
  currentQuery: LocationQuery | Record<string, QueryValue>,
  nextQuery: LocationQuery | Record<string, QueryValue>,
): boolean {
  const currentEntries = Object.entries(currentQuery)
    .map(([key, value]) => [key, getSingleQueryValue(value)] as const)
    .filter(([, value]) => value !== undefined)
    .sort(([left], [right]) => left.localeCompare(right));
  const nextEntries = Object.entries(nextQuery)
    .map(([key, value]) => [key, getSingleQueryValue(value)] as const)
    .filter(([, value]) => value !== undefined)
    .sort(([left], [right]) => left.localeCompare(right));

  if (currentEntries.length !== nextEntries.length) {
    return false;
  }

  return currentEntries.every(([key, value], index) => {
    const [nextKey, nextValue] = nextEntries[index];
    return key === nextKey && value === nextValue;
  });
}

async function syncRouteWithStore() {
  const nextQuery = store.toRouteQuery() as Record<string, QueryValue>;
  if (isSameQuery(route.query, nextQuery)) {
    return false;
  }
  await router.replace({ query: nextQuery });
  return true;
}

async function applyRouteQueryAndFetch() {
  store.applyQuery(route.query);
  searchInput.value = store.filters.search;
  await store.refresh();
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

  if (item.runtime_level === "open" || item.runtime_level === "half_open" || item.runtime_level === "degraded") {
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
  store.setFilters(next);
  const routeUpdated = await syncRouteWithStore();
  if (!routeUpdated) {
    await store.refresh();
  }
}

function handleWindowChange(window: ProviderRuntimeWindow) {
  if (window === store.filters.window) {
    return;
  }
  void pushFilters({ window });
}

function handleStatusChange(value: unknown) {
  if (typeof value !== "string") {
    return;
  }
  void pushFilters({ status: value as ProviderRuntimeStatusFilter });
}

function handleSortChange(value: unknown) {
  if (typeof value !== "string") {
    return;
  }
  void pushFilters({ sort: value as ProviderRuntimeSortField });
}

function handleToggleDirection() {
  void pushFilters({
    direction: store.filters.direction === "desc" ? "asc" : "desc",
  });
}

function handleOnlyEnabledChange(value: boolean | "indeterminate") {
  void pushFilters({ only_enabled: value === true });
}

function handleSearchApply() {
  void pushFilters({ search: searchInput.value.trim() });
}

function handleSearchClear() {
  if (!searchInput.value && !store.filters.search) {
    return;
  }
  searchInput.value = "";
  void pushFilters({ search: "" });
}

function handleRefresh() {
  searchInput.value = store.filters.search;
  void store.refresh();
}

function handleReset() {
  store.resetFilters();
  searchInput.value = store.filters.search;
  void syncRouteWithStore().then((updated) => {
    if (!updated) {
      void store.refresh();
    }
  });
}

watch(
  () => route.query,
  () => {
    void applyRouteQueryAndFetch();
  },
);

onMounted(() => {
  void applyRouteQueryAndFetch();
});
</script>
