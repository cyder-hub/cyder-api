<template>
  <div class="app-page">
    <div class="app-page-shell">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold tracking-tight text-gray-900 sm:text-xl">
            {{ $t("sidebar.dashboard") }}
          </h1>
          <p class="mt-1 text-sm text-gray-500">
            {{ $t("dashboard.description") }}
          </p>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
          <Button class="w-full sm:w-auto" :disabled="isRefreshing" @click="fetchDashboard">
            <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isRefreshing }" />
            {{ $t("common.refresh") }}
          </Button>
        </div>
      </div>

      <div class="app-section">
        <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-6">
          <template v-if="kpiLoading">
            <Card
              v-for="placeholder in 6"
              :key="`kpi-loading-${placeholder}`"
              class="border border-gray-200 shadow-none"
            >
              <CardContent class="flex items-center justify-center px-4 py-10">
                <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
                <span class="text-sm text-gray-500">{{ $t("common.loading") }}</span>
              </CardContent>
            </Card>
          </template>
          <Card
            v-else-if="kpiError"
            class="border border-red-200 bg-red-50 shadow-none md:col-span-2 xl:col-span-6"
          >
            <CardContent class="flex flex-col items-center justify-center px-4 py-10 text-center">
              <AlertCircle class="mb-3 h-8 w-8 stroke-1 text-red-500" />
              <p class="text-sm font-medium text-red-500">
                {{ $t("dashboard.errorLoading", { error: kpiError }) }}
              </p>
            </CardContent>
          </Card>
          <template v-else>
            <Card
              v-for="card in kpiCards"
              :key="card.key"
              class="border border-gray-200 shadow-none"
            >
              <CardContent class="px-4 py-4">
                <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ card.label }}
                </p>
                <p class="mt-2 text-2xl font-semibold tracking-tight text-gray-900">
                  {{ card.value }}
                </p>
                <p class="mt-1 text-xs text-gray-500">
                  {{ card.description }}
                </p>
              </CardContent>
            </Card>
          </template>
        </div>

        <div class="grid grid-cols-1 gap-4 xl:grid-cols-2">
          <Card class="border border-gray-200 shadow-none">
            <CardHeader class="px-4 pb-4 sm:px-6">
              <CardTitle class="text-base">{{ $t("dashboard.sections.resources.title") }}</CardTitle>
            </CardHeader>
            <CardContent class="px-4 sm:px-6">
              <div
                v-if="resourcesLoading"
                class="flex items-center justify-center rounded-lg border border-dashed border-gray-200 py-12"
              >
                <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
                <span class="text-sm text-gray-500">{{ $t("common.loading") }}</span>
              </div>
              <div
                v-else-if="resourcesError"
                class="flex flex-col items-center justify-center rounded-lg border border-red-200 bg-red-50 px-4 py-10 text-center"
              >
                <AlertCircle class="mb-3 h-8 w-8 stroke-1 text-red-500" />
                <p class="text-sm font-medium text-red-500">
                  {{ $t("dashboard.errorLoading", { error: resourcesError }) }}
                </p>
              </div>
              <ul v-else class="divide-y divide-gray-100">
                <li
                  v-for="item in resourceItems"
                  :key="item.key"
                  class="flex items-start justify-between gap-4 py-3 first:pt-0 last:pb-0 sm:items-center"
                >
                  <span class="text-sm text-gray-500">{{ item.label }}</span>
                  <div class="text-right">
                    <div class="text-base font-medium text-gray-900 font-mono sm:text-sm">
                      {{ item.value }}
                    </div>
                    <div class="mt-0.5 text-xs text-gray-400">
                      {{ item.description }}
                    </div>
                  </div>
                </li>
              </ul>
            </CardContent>
          </Card>

          <Card class="border border-gray-200 shadow-none">
            <CardHeader class="px-4 pb-4 sm:px-6">
              <CardTitle class="text-base">{{ $t("dashboard.sections.runtime.title") }}</CardTitle>
            </CardHeader>
            <CardContent class="px-4 sm:px-6">
              <div
                v-if="resourcesLoading"
                class="flex items-center justify-center rounded-lg border border-dashed border-gray-200 py-12"
              >
                <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
                <span class="text-sm text-gray-500">{{ $t("common.loading") }}</span>
              </div>
              <div
                v-else-if="resourcesError"
                class="flex flex-col items-center justify-center rounded-lg border border-red-200 bg-red-50 px-4 py-10 text-center"
              >
                <AlertCircle class="mb-3 h-8 w-8 stroke-1 text-red-500" />
                <p class="text-sm font-medium text-red-500">
                  {{ $t("dashboard.errorLoading", { error: resourcesError }) }}
                </p>
              </div>
              <template v-else>
                <div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
                  <div
                    v-for="item in runtimeItems"
                    :key="item.key"
                    class="rounded-lg border border-gray-100 px-3 py-3"
                  >
                    <div class="flex items-center justify-between gap-2">
                      <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                        {{ item.label }}
                      </p>
                      <Badge variant="outline" :class="runtimeBadgeClass(item.key)">
                        {{ item.value }}
                      </Badge>
                    </div>
                    <p class="mt-2 text-xs text-gray-400">
                      {{ item.description }}
                    </p>
                  </div>
                </div>

                <div class="mt-4 flex flex-col gap-2 border-t border-gray-100 pt-4 sm:flex-row">
                  <Button variant="outline" class="w-full sm:w-auto" @click="goToRuntime">
                    {{ $t("dashboard.actions.viewRuntime") }}
                  </Button>
                  <Button variant="outline" class="w-full sm:w-auto" @click="goToRecords">
                    {{ $t("dashboard.actions.viewRecords") }}
                  </Button>
                </div>
              </template>
            </CardContent>
          </Card>
        </div>

        <Card class="border border-gray-200 shadow-none">
          <CardHeader class="px-4 pb-4 sm:px-6">
            <CardTitle class="text-base">{{ $t("dashboard.sections.trends.title") }}</CardTitle>
          </CardHeader>
          <CardContent class="px-0 pb-0">
            <UsageChart />
          </CardContent>
        </Card>

        <div v-if="alertsLoading" class="grid grid-cols-1 gap-4 xl:grid-cols-3">
          <Card
            v-for="placeholder in 3"
            :key="`alerts-loading-${placeholder}`"
            class="border border-gray-200 shadow-none"
          >
            <CardContent class="flex items-center justify-center px-4 py-16">
              <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
              <span class="text-sm text-gray-500">{{ $t("common.loading") }}</span>
            </CardContent>
          </Card>
        </div>
        <Card
          v-else-if="alertsError"
          class="border border-red-200 bg-red-50 shadow-none"
        >
          <CardContent class="flex flex-col items-center justify-center px-4 py-16 text-center">
            <AlertCircle class="mb-3 h-8 w-8 stroke-1 text-red-500" />
            <p class="text-sm font-medium text-red-500">
              {{ $t("dashboard.errorLoading", { error: alertsError }) }}
            </p>
          </CardContent>
        </Card>
        <div v-else class="grid grid-cols-1 gap-4 xl:grid-cols-3">
          <Card class="border border-gray-200 shadow-none">
            <CardHeader class="px-4 pb-4 sm:px-6">
              <CardTitle class="text-base">{{ $t("dashboard.sections.alerts.title") }}</CardTitle>
            </CardHeader>
            <CardContent class="app-stack-sm px-4 sm:px-6">
              <div class="rounded-lg border border-gray-100">
                <div class="border-b border-gray-100 px-4 py-3">
                  <p class="text-sm font-medium text-gray-900">
                    {{ $t("dashboard.alertGroups.unstable") }}
                  </p>
                </div>
                <ul v-if="unstableProviders.length" class="divide-y divide-gray-100">
                  <li
                    v-for="item in unstableProviders"
                    :key="`unstable-${item.provider_id}-${item.runtime_level}`"
                    class="px-4 py-3"
                  >
                    <div class="flex items-start justify-between gap-3">
                      <div class="min-w-0">
                        <p class="truncate text-sm font-medium text-gray-900">
                          {{ item.provider_name || item.provider_key }}
                        </p>
                        <p class="mt-1 font-mono text-xs text-gray-500">{{ item.provider_key }}</p>
                      </div>
                      <Badge :class="runtimeLevelBadgeClass(item.runtime_level)">
                        {{ runtimeLevelLabel(item.runtime_level) }}
                      </Badge>
                    </div>
                    <div class="mt-3">
                      <Button variant="outline" size="sm" @click="goToRuntime">
                        {{ $t("dashboard.actions.viewRuntime") }}
                      </Button>
                    </div>
                  </li>
                </ul>
                <div v-else class="px-4 py-5 text-sm text-gray-500">
                  {{ $t("dashboard.empty.noUnstableProviders") }}
                </div>
              </div>

              <div class="rounded-lg border border-gray-100">
                <div class="border-b border-gray-100 px-4 py-3">
                  <p class="text-sm font-medium text-gray-900">{{ $t("dashboard.alertGroups.topErrors") }}</p>
                </div>
                <ul v-if="alertsSection.alerts.top_error_providers.length" class="divide-y divide-gray-100">
                  <li
                    v-for="item in alertsSection.alerts.top_error_providers"
                    :key="`error-${item.provider_id}`"
                    class="px-4 py-3"
                  >
                    <div class="flex items-start justify-between gap-3">
                      <div class="min-w-0">
                        <p class="truncate text-sm font-medium text-gray-900">
                          {{ item.provider_name || item.provider_key }}
                        </p>
                        <p class="mt-1 text-xs text-gray-500">
                          {{ $t("dashboard.metrics.errors") }} {{ formatCount(item.error_count) }}
                          · {{ $t("dashboard.metrics.successRate") }}
                          {{ formatPercentage(item.success_rate) }}
                        </p>
                        <p class="mt-1 text-xs text-gray-400">
                          {{ $t("dashboard.metrics.lastError") }} {{ formatDateTime(item.last_error_at) }}
                        </p>
                      </div>
                      <Button variant="outline" size="sm" @click="goToRecords">
                        {{ $t("dashboard.actions.viewRecords") }}
                      </Button>
                    </div>
                  </li>
                </ul>
                <div v-else class="px-4 py-5 text-sm text-gray-500">
                  {{ $t("dashboard.empty.noErrors") }}
                </div>
              </div>

              <div class="rounded-lg border border-gray-100">
                <div class="border-b border-gray-100 px-4 py-3">
                  <p class="text-sm font-medium text-gray-900">
                    {{ $t("dashboard.alertGroups.costHotspots") }}
                  </p>
                </div>
                <div v-if="showCostHotspots" class="divide-y divide-gray-100">
                  <div class="px-4 py-3">
                    <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                      {{ $t("dashboard.alertGroups.costProviders") }}
                    </p>
                    <ul
                      v-if="alertsSection.alerts.top_cost_providers.length"
                      class="mt-3 divide-y divide-gray-100"
                    >
                      <li
                        v-for="item in alertsSection.alerts.top_cost_providers"
                        :key="`cost-provider-${item.provider_id}`"
                        class="py-3 first:pt-0 last:pb-0"
                      >
                        <div class="flex items-start justify-between gap-3">
                          <div class="min-w-0">
                            <p class="truncate text-sm font-medium text-gray-900">
                              {{ item.provider_name || item.provider_key }}
                            </p>
                            <p class="mt-1 font-mono text-xs text-gray-500">{{ item.provider_key }}</p>
                            <p class="mt-1 text-xs text-gray-500">
                              {{ $t("dashboard.metrics.requests") }} {{ formatCount(item.request_count) }}
                              · {{ $t("dashboard.metrics.successRate") }}
                              {{ formatPercentage(item.success_rate) }}
                            </p>
                          </div>
                          <div class="text-right">
                            <div
                              v-for="cost in formatCostEntries(item.total_cost)"
                              :key="cost"
                              class="text-xs font-medium text-gray-900"
                            >
                              {{ cost }}
                            </div>
                            <Button
                              variant="outline"
                              size="sm"
                              class="mt-3"
                              @click="goToProvider(item.provider_id)"
                            >
                              {{ $t("common.edit") }}
                            </Button>
                          </div>
                        </div>
                      </li>
                    </ul>
                    <p v-else class="mt-3 text-sm text-gray-500">
                      {{ $t("dashboard.empty.noCostProviders") }}
                    </p>
                  </div>

                  <div class="px-4 py-3">
                    <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                      {{ $t("dashboard.alertGroups.costModels") }}
                    </p>
                    <ul
                      v-if="alertsSection.alerts.top_cost_models.length"
                      class="mt-3 divide-y divide-gray-100"
                    >
                      <li
                        v-for="item in alertsSection.alerts.top_cost_models"
                        :key="`cost-model-${item.provider_id}-${item.model_id}`"
                        class="py-3 first:pt-0 last:pb-0"
                      >
                        <div class="flex items-start justify-between gap-3">
                          <div class="min-w-0">
                            <p class="truncate text-sm font-medium text-gray-900">
                              {{ item.model_name || item.real_model_name || $t("common.notAvailable") }}
                            </p>
                            <p class="mt-1 font-mono text-xs text-gray-500">
                              {{ item.provider_key }} / {{ item.real_model_name || item.model_name }}
                            </p>
                            <p class="mt-1 text-xs text-gray-500">
                              {{ $t("dashboard.metrics.requests") }} {{ formatCount(item.request_count) }}
                              · {{ $t("dashboard.metrics.totalTokens") }}
                              {{ formatCount(item.total_tokens) }}
                            </p>
                          </div>
                          <div class="text-right">
                            <div
                              v-for="cost in formatCostEntries(item.total_cost)"
                              :key="cost"
                              class="text-xs font-medium text-gray-900"
                            >
                              {{ cost }}
                            </div>
                            <Button
                              variant="outline"
                              size="sm"
                              class="mt-3"
                              @click="goToModel(item.model_id)"
                            >
                              {{ $t("common.edit") }}
                            </Button>
                          </div>
                        </div>
                      </li>
                    </ul>
                    <p v-else class="mt-3 text-sm text-gray-500">
                      {{ $t("dashboard.empty.noCostModels") }}
                    </p>
                  </div>
                </div>
                <div v-else class="px-4 py-5 text-sm text-gray-500">
                  {{ $t("dashboard.empty.noCostHotspots") }}
                </div>
              </div>
            </CardContent>
          </Card>

          <Card class="border border-gray-200 shadow-none">
            <CardHeader class="px-4 pb-4 sm:px-6">
              <CardTitle class="text-base">
                {{ $t("dashboard.sections.topProviders.title") }}
              </CardTitle>
            </CardHeader>
            <CardContent class="px-4 sm:px-6">
              <ul v-if="alertsSection.top_providers.length" class="divide-y divide-gray-100">
                <li
                  v-for="item in alertsSection.top_providers"
                  :key="item.provider_id"
                  class="py-3 first:pt-0 last:pb-0"
                >
                  <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0">
                      <p class="truncate text-sm font-medium text-gray-900">
                        {{ item.provider_name || item.provider_key }}
                      </p>
                      <p class="mt-1 font-mono text-xs text-gray-500">{{ item.provider_key }}</p>
                      <p class="mt-1 text-xs text-gray-500">
                        {{ $t("dashboard.metrics.requests") }} {{ formatCount(item.request_count) }}
                        · {{ $t("dashboard.metrics.successRate") }}
                        {{ formatPercentage(item.success_rate) }}
                      </p>
                    </div>
                    <div class="text-right">
                      <div
                        v-for="cost in formatCostEntries(item.total_cost)"
                        :key="cost"
                        class="text-xs font-medium text-gray-900"
                      >
                        {{ cost }}
                      </div>
                      <div class="mt-1 text-xs text-gray-400">
                        {{ formatLatency(item.avg_total_latency_ms) }}
                      </div>
                    </div>
                  </div>
                </li>
              </ul>
              <div v-else class="py-12 text-center text-sm text-gray-500">
                {{ $t("dashboard.empty.noTopProviders") }}
              </div>
            </CardContent>
          </Card>

          <Card class="border border-gray-200 shadow-none">
            <CardHeader class="px-4 pb-4 sm:px-6">
              <CardTitle class="text-base">{{ $t("dashboard.sections.topModels.title") }}</CardTitle>
            </CardHeader>
            <CardContent class="px-4 sm:px-6">
              <ul v-if="alertsSection.top_models.length" class="divide-y divide-gray-100">
                <li
                  v-for="item in alertsSection.top_models"
                  :key="`${item.provider_id}-${item.model_id}`"
                  class="py-3 first:pt-0 last:pb-0"
                >
                  <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0">
                      <p class="truncate text-sm font-medium text-gray-900">
                        {{ item.model_name || item.real_model_name || $t("common.notAvailable") }}
                      </p>
                      <p class="mt-1 font-mono text-xs text-gray-500">
                        {{ item.provider_key }} / {{ item.real_model_name || item.model_name }}
                      </p>
                      <p class="mt-1 text-xs text-gray-500">
                        {{ $t("dashboard.metrics.requests") }} {{ formatCount(item.request_count) }}
                        · {{ $t("dashboard.metrics.totalTokens") }}
                        {{ formatCount(item.total_tokens) }}
                      </p>
                    </div>
                    <div class="text-right">
                      <div
                        v-for="cost in formatCostEntries(item.total_cost)"
                        :key="cost"
                        class="text-xs font-medium text-gray-900"
                      >
                        {{ cost }}
                      </div>
                    </div>
                  </div>
                </li>
              </ul>
              <div v-else class="py-12 text-center text-sm text-gray-500">
                {{ $t("dashboard.empty.noTopModels") }}
              </div>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { AlertCircle, Loader2, RefreshCcw } from "lucide-vue-next";
import { Api } from "@/services/request";
import type {
  DashboardRuntimeSummary,
  ProviderRuntimeLevel,
} from "@/store/types";
import UsageChart from "@/components/UsageChart.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { formatPriceFromNanos, formatTimestamp } from "@/lib/utils";
import { createDashboardPageState } from "./dashboardPageState";

const { t: $t } = useI18n();
const router = useRouter();

const dashboardState = createDashboardPageState({
  api: Api,
  getUnknownErrorMessage: () => $t("common.unknownError"),
  logError: (message, error) => {
    console.error(message, error);
  },
});

const {
  alertsError,
  alertsLoading,
  alertsSection,
  fetchDashboard,
  isRefreshing,
  kpiError,
  kpiLoading,
  kpiSection,
  resourcesError,
  resourcesLoading,
  resourcesSection,
  showCostHotspots,
  unstableProviders,
} = dashboardState;

const formatCount = (value: number | null | undefined) => (value ?? 0).toLocaleString();
const formatPercentage = (value: number | null | undefined) =>
  value == null ? "0%" : `${(value * 100).toFixed(1)}%`;
const formatLatency = (value: number | null | undefined) =>
  value == null ? $t("dashboard.empty.noLatency") : `${Math.round(value).toLocaleString()} ms`;
const formatDateTime = (value: number | null | undefined) => formatTimestamp(value) || "-";
const formatDashboardCost = (nanos: number, currency: string) =>
  formatPriceFromNanos(nanos, currency, "0");

const formatCostEntries = (costMap: Record<string, number>) => {
  const entries = Object.entries(costMap);
  if (!entries.length) {
    return ["0"];
  }

  return entries.map(([currency, amount]) => formatDashboardCost(amount, currency));
};

const runtimeWindowLabel = (window: DashboardRuntimeSummary["window"]) =>
  $t(`providerRuntimePage.window.${window}`);

const runtimeLevelLabel = (level: ProviderRuntimeLevel) =>
  $t(`providerRuntimePage.status.${level}`);

const runtimeLevelBadgeClass = (level: ProviderRuntimeLevel) => {
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
};

const runtimeBadgeClass = (key: string) => {
  switch (key) {
    case "open_count":
      return "border-red-200 bg-red-50 text-red-700 hover:bg-red-50";
    case "half_open_count":
      return "border-amber-200 bg-amber-50 text-amber-700 hover:bg-amber-50";
    case "degraded_count":
      return "border-orange-200 bg-orange-50 text-orange-700 hover:bg-orange-50";
    case "healthy_count":
      return "border-emerald-200 bg-emerald-50 text-emerald-700 hover:bg-emerald-50";
    default:
      return "border-gray-200 bg-gray-50 text-gray-700 hover:bg-gray-50";
  }
};

const kpiCards = computed(() => [
  {
    key: "requests",
    label: $t("dashboard.kpi.requests"),
    value: formatCount(kpiSection.value.today.request_count),
    description: `${$t("dashboard.kpi.success")} ${formatCount(kpiSection.value.today.success_count)} / ${$t("dashboard.kpi.errors")} ${formatCount(kpiSection.value.today.error_count)}`,
  },
  {
    key: "success_rate",
    label: $t("dashboard.kpi.successRate"),
    value: formatPercentage(kpiSection.value.today.success_rate),
    description: $t("dashboard.kpi.windowToday"),
  },
  {
    key: "tokens",
    label: $t("dashboard.kpi.totalTokens"),
    value: formatCount(kpiSection.value.today.total_tokens),
    description: `${$t("dashboard.kpi.inputTokens")} ${formatCount(kpiSection.value.today.total_input_tokens)}`,
  },
  {
    key: "cost",
    label: $t("dashboard.kpi.totalCost"),
    value: formatCostEntries(kpiSection.value.today.total_cost).join(" / "),
    description: $t("dashboard.kpi.multiCurrencyHint"),
  },
  {
    key: "latency",
    label: $t("dashboard.kpi.avgLatency"),
    value: formatLatency(kpiSection.value.today.avg_total_latency_ms),
    description: `${$t("dashboard.kpi.firstByte")} ${formatLatency(kpiSection.value.today.avg_first_byte_ms)}`,
  },
  {
    key: "runtime_issues",
    label: $t("dashboard.kpi.runtimeIssues"),
    value: formatCount(
      kpiSection.value.runtime.open_count +
        kpiSection.value.runtime.half_open_count +
        kpiSection.value.runtime.degraded_count,
    ),
    description: `${$t("dashboard.kpi.runtimeWindow")} ${runtimeWindowLabel(kpiSection.value.runtime.window)}`,
  },
]);

const resourceItems = computed(() => [
  {
    key: "providers",
    label: $t("dashboard.resources.providers"),
    value: `${formatCount(resourcesSection.value.overview.enabled_provider_count)} / ${formatCount(resourcesSection.value.overview.provider_count)}`,
    description: $t("dashboard.resources.enabledTotal"),
  },
  {
    key: "models",
    label: $t("dashboard.resources.models"),
    value: `${formatCount(resourcesSection.value.overview.enabled_model_count)} / ${formatCount(resourcesSection.value.overview.model_count)}`,
    description: $t("dashboard.resources.enabledTotal"),
  },
  {
    key: "provider_keys",
    label: $t("dashboard.resources.providerKeys"),
    value: `${formatCount(resourcesSection.value.overview.enabled_provider_key_count)} / ${formatCount(resourcesSection.value.overview.provider_key_count)}`,
    description: $t("dashboard.resources.enabledTotal"),
  },
  {
    key: "system_api_keys",
    label: $t("dashboard.resources.systemApiKeys"),
    value: `${formatCount(resourcesSection.value.overview.enabled_system_api_key_count)} / ${formatCount(resourcesSection.value.overview.system_api_key_count)}`,
    description: `${$t("dashboard.resources.activeToday")} ${formatCount(resourcesSection.value.today.active_system_api_key_count)}`,
  },
]);

const runtimeItems = computed(() => [
  {
    key: "healthy_count",
    label: $t("providerRuntimePage.summary.healthy"),
    value: formatCount(resourcesSection.value.runtime.healthy_count),
    description: $t("dashboard.runtime.windowDetail", {
      window: runtimeWindowLabel(resourcesSection.value.runtime.window),
    }),
  },
  {
    key: "degraded_count",
    label: $t("providerRuntimePage.summary.degraded"),
    value: formatCount(resourcesSection.value.runtime.degraded_count),
    description: $t("dashboard.runtime.degradedHint"),
  },
  {
    key: "half_open_count",
    label: $t("providerRuntimePage.summary.halfOpen"),
    value: formatCount(resourcesSection.value.runtime.half_open_count),
    description: $t("dashboard.runtime.halfOpenHint"),
  },
  {
    key: "open_count",
    label: $t("providerRuntimePage.summary.open"),
    value: formatCount(resourcesSection.value.runtime.open_count),
    description: $t("dashboard.runtime.openHint"),
  },
  {
    key: "no_traffic_count",
    label: $t("providerRuntimePage.summary.noTraffic"),
    value: formatCount(resourcesSection.value.runtime.no_traffic_count),
    description: $t("dashboard.runtime.noTrafficHint"),
  },
  {
    key: "active_provider_count",
    label: $t("dashboard.runtime.activeProviders"),
    value: formatCount(resourcesSection.value.today.active_provider_count),
    description: `${$t("dashboard.runtime.activeModels")} ${formatCount(resourcesSection.value.today.active_model_count)}`,
  },
]);

const goToRuntime = () => {
  router.push({ name: "ProviderRuntime" });
};

const goToRecords = () => {
  router.push({ name: "Record" });
};

const goToProvider = (providerId: number) => {
  router.push({ name: "ProviderEdit", params: { id: providerId } });
};

const goToModel = (modelId: number) => {
  router.push({ name: "ModelEdit", params: { id: modelId } });
};

onMounted(() => {
  fetchDashboard();
});
</script>
