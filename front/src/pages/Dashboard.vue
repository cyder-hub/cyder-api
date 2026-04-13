<template>
  <div class="app-page">
    <div class="app-page-shell">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold text-gray-900 tracking-tight sm:text-xl">
            {{ $t("sidebar.dashboard") }}
          </h1>
          <p class="mt-1 text-sm text-gray-500">
            {{ $t("dashboard.description") }}
          </p>
        </div>
      </div>

      <div v-if="isLoading" class="flex items-center justify-center py-16">
        <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm font-medium text-gray-500">{{ $t("common.loading") }}</span>
      </div>
      <div v-else-if="error" class="flex flex-col items-center justify-center py-20">
        <AlertCircle class="mb-4 h-10 w-10 stroke-1 text-red-500" />
        <p class="text-sm font-medium text-red-500">
          {{ $t("dashboard.errorLoading", { error: error }) }}
        </p>
      </div>
      <div v-else class="app-section">
        <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <Card class="border border-gray-200 shadow-none">
            <CardHeader class="px-4 pb-4 sm:px-6">
              <CardTitle class="text-base">{{ $t("dashboard.systemOverview.title") }}</CardTitle>
            </CardHeader>
            <CardContent class="px-4 sm:px-6">
              <ul class="grid grid-cols-1 gap-3 sm:gap-4">
                <li
                  class="flex items-start justify-between gap-4 rounded-lg border border-gray-100 px-3 py-3 sm:items-center sm:px-4"
                >
                  <span class="text-sm text-gray-500">{{ $t("dashboard.systemOverview.providers") }}</span>
                  <span class="text-base font-medium text-gray-900 font-mono sm:text-sm">{{
                    overview.providers_count
                  }}</span>
                </li>
                <li
                  class="flex items-start justify-between gap-4 rounded-lg border border-gray-100 px-3 py-3 sm:items-center sm:px-4"
                >
                  <span class="text-sm text-gray-500">{{ $t("dashboard.systemOverview.models") }}</span>
                  <span class="text-base font-medium text-gray-900 font-mono sm:text-sm">{{ overview.models_count }}</span>
                </li>
                <li
                  class="flex items-start justify-between gap-4 rounded-lg border border-gray-100 px-3 py-3 sm:items-center sm:px-4"
                >
                  <span class="text-sm text-gray-500">{{ $t("dashboard.systemOverview.providerKeys") }}</span>
                  <span class="text-base font-medium text-gray-900 font-mono sm:text-sm">{{
                    overview.provider_keys_count
                  }}</span>
                </li>
              </ul>
            </CardContent>
          </Card>

          <Card class="border border-gray-200 shadow-none">
            <CardHeader class="px-4 pb-4 sm:px-6">
              <CardTitle class="text-base">{{ $t("dashboard.todayLogStats.title") }}</CardTitle>
            </CardHeader>
            <CardContent class="px-4 sm:px-6">
              <ul class="grid grid-cols-1 gap-3 sm:gap-4">
                <li
                  class="flex items-start justify-between gap-4 rounded-lg border border-gray-100 px-3 py-3 sm:items-center sm:px-4"
                >
                  <span class="text-sm text-gray-500">{{ $t("dashboard.todayLogStats.requests") }}</span>
                  <span class="text-base font-medium text-gray-900 font-mono sm:text-sm">{{
                    todayStats.requests_count
                  }}</span>
                </li>
                <li
                  class="flex items-start justify-between gap-4 rounded-lg border border-gray-100 px-3 py-3 sm:items-center sm:px-4"
                >
                  <span class="text-sm text-gray-500">{{ $t("dashboard.todayLogStats.promptTokens") }}</span>
                  <span class="text-base font-medium text-gray-900 font-mono sm:text-sm">{{
                    todayStats.total_input_tokens?.toLocaleString()
                  }}</span>
                </li>
                <li
                  class="flex items-start justify-between gap-4 rounded-lg border border-gray-100 px-3 py-3 sm:items-center sm:px-4"
                >
                  <span class="text-sm text-gray-500">{{ $t("dashboard.todayLogStats.completionTokens") }}</span>
                  <span class="text-base font-medium text-gray-900 font-mono sm:text-sm">{{
                    todayStats.total_output_tokens?.toLocaleString()
                  }}</span>
                </li>
                <li
                  class="flex items-start justify-between gap-4 rounded-lg border border-gray-100 px-3 py-3 sm:items-center sm:px-4"
                >
                  <span class="text-sm text-gray-500">{{ $t("dashboard.todayLogStats.reasoningTokens") }}</span>
                  <span class="text-base font-medium text-gray-900 font-mono sm:text-sm">{{
                    todayStats.total_reasoning_tokens?.toLocaleString()
                  }}</span>
                </li>
                <li
                  class="flex items-start justify-between gap-4 rounded-lg border border-gray-100 px-3 py-3 sm:items-center sm:px-4"
                >
                  <span class="text-sm text-gray-500">{{ $t("dashboard.todayLogStats.totalTokens") }}</span>
                  <span class="text-base font-medium text-gray-900 font-mono sm:text-sm">{{
                    todayStats.total_tokens?.toLocaleString()
                  }}</span>
                </li>
                <li
                  class="flex items-start justify-between gap-4 rounded-lg border border-gray-100 px-3 py-3 sm:px-4"
                >
                  <span class="mt-0.5 text-sm text-gray-500">{{ $t("dashboard.todayLogStats.totalCost") }}</span>
                  <div class="min-w-0 text-right">
                    <div
                      v-for="(cost, currency) in todayStats.total_cost"
                      :key="currency"
                      class="text-base font-medium text-gray-900 font-mono sm:text-sm"
                    >
                      {{ formatDashboardCost(cost, currency) }}
                    </div>
                    <div v-if="Object.keys(todayStats.total_cost || {}).length === 0" class="text-base font-medium text-gray-900 font-mono sm:text-sm">
                      0
                    </div>
                  </div>
                </li>
              </ul>
            </CardContent>
          </Card>
        </div>

        <div class="overflow-hidden rounded-xl border border-gray-200 bg-white">
          <UsageChart />
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { Loader2, AlertCircle } from "lucide-vue-next";
import { Api } from "@/services/request";
import type { SystemOverviewStats, TodayRequestLogStats } from "@/store/types";
import UsageChart from "@/components/UsageChart.vue";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { formatPriceFromNanos } from "@/lib/utils";

const { t: $t } = useI18n();

// Types imported from @/services/request

const overview = ref<SystemOverviewStats>({
  providers_count: 0,
  models_count: 0,
  provider_keys_count: 0,
});

const todayStats = ref<TodayRequestLogStats>({
  requests_count: 0,
  total_input_tokens: 0,
  total_output_tokens: 0,
  total_reasoning_tokens: 0,
  total_tokens: 0,
  total_cost: {},
});

const isLoading = ref(true);
const error = ref<string | null>(null);

const formatDashboardCost = (nanos: number, currency: string) =>
  formatPriceFromNanos(nanos, currency, "0");

const fetchData = async () => {
  isLoading.value = true;
  error.value = null;
  try {
    const [overviewData, statsData] = await Promise.all([
      Api.getSystemOverview(),
      Api.getTodayLogStats(),
    ]);

    overview.value = overviewData as any;
    todayStats.value = statsData as any;
  } catch (err: any) {
    console.error("Failed to fetch dashboard data:", err);
    error.value = err.message || $t("common.unknownError", "Unknown Error");
  } finally {
    isLoading.value = false;
  }
};

onMounted(() => {
  fetchData();
});
</script>
