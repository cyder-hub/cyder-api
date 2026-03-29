<template>
  <div class="p-6 space-y-6 bg-gray-50 min-h-screen">
    <!-- 页面头部 -->
    <div class="flex justify-between items-start">
      <div>
        <h1 class="text-lg font-semibold text-gray-900 tracking-tight">
          {{ $t("sidebar.dashboard") }}
        </h1>
        <p class="mt-1 text-sm text-gray-500">
          {{ $t("dashboard.description") }}
        </p>
      </div>
    </div>

    <div v-if="isLoading" class="flex items-center justify-center py-16">
      <Loader2 class="h-5 w-5 animate-spin mr-2 text-gray-400" />
      <span class="text-sm font-medium text-gray-500">{{ $t("loading") }}</span>
    </div>
    <div v-else-if="error" class="flex flex-col items-center justify-center py-20">
      <AlertCircle class="h-10 w-10 text-red-500 mb-4 stroke-1" />
      <p class="text-sm font-medium text-red-500">
        {{ $t("dashboard.errorLoading", { error: error }) }}
      </p>
    </div>
    <div v-else class="space-y-6">
      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <!-- System Overview Card -->
        <Card class="shadow-none border border-gray-200">
          <CardHeader>
            <CardTitle class="text-base">{{ $t("dashboard.systemOverview.title") }}</CardTitle>
          </CardHeader>
          <CardContent>
            <ul class="space-y-4">
              <li class="flex justify-between items-center pb-4 border-b border-gray-100 last:border-0 last:pb-0">
                <span class="text-sm text-gray-500">{{ $t("dashboard.systemOverview.providers") }}</span>
                <span class="text-sm font-medium text-gray-900 font-mono">{{
                  overview.providers_count
                }}</span>
              </li>
              <li class="flex justify-between items-center pb-4 border-b border-gray-100 last:border-0 last:pb-0">
                <span class="text-sm text-gray-500">{{ $t("dashboard.systemOverview.models") }}</span>
                <span class="text-sm font-medium text-gray-900 font-mono">{{ overview.models_count }}</span>
              </li>
              <li class="flex justify-between items-center pb-4 border-b border-gray-100 last:border-0 last:pb-0">
                <span class="text-sm text-gray-500">{{ $t("dashboard.systemOverview.providerKeys") }}</span>
                <span class="text-sm font-medium text-gray-900 font-mono">{{
                  overview.provider_keys_count
                }}</span>
              </li>
            </ul>
          </CardContent>
        </Card>

        <!-- Today's Log Stats Card -->
        <Card class="shadow-none border border-gray-200">
          <CardHeader>
            <CardTitle class="text-base">{{ $t("dashboard.todayLogStats.title") }}</CardTitle>
          </CardHeader>
          <CardContent>
            <ul class="space-y-4">
              <li class="flex justify-between items-center pb-4 border-b border-gray-100 last:border-0 last:pb-0">
                <span class="text-sm text-gray-500">{{ $t("dashboard.todayLogStats.requests") }}</span>
                <span class="text-sm font-medium text-gray-900 font-mono">{{
                  todayStats.requests_count
                }}</span>
              </li>
              <li class="flex justify-between items-center pb-4 border-b border-gray-100 last:border-0 last:pb-0">
                <span class="text-sm text-gray-500">{{ $t("dashboard.todayLogStats.promptTokens") }}</span>
                <span class="text-sm font-medium text-gray-900 font-mono">{{
                  todayStats.total_input_tokens?.toLocaleString()
                }}</span>
              </li>
              <li class="flex justify-between items-center pb-4 border-b border-gray-100 last:border-0 last:pb-0">
                <span class="text-sm text-gray-500"
                  >{{ $t("dashboard.todayLogStats.completionTokens") }}</span
                >
                <span class="text-sm font-medium text-gray-900 font-mono">{{
                  todayStats.total_output_tokens?.toLocaleString()
                }}</span>
              </li>
              <li class="flex justify-between items-center pb-4 border-b border-gray-100 last:border-0 last:pb-0">
                <span class="text-sm text-gray-500"
                  >{{ $t("dashboard.todayLogStats.reasoningTokens") }}</span
                >
                <span class="text-sm font-medium text-gray-900 font-mono">{{
                  todayStats.total_reasoning_tokens?.toLocaleString()
                }}</span>
              </li>
              <li class="flex justify-between items-center pb-4 border-b border-gray-100 last:border-0 last:pb-0">
                <span class="text-sm text-gray-500">{{ $t("dashboard.todayLogStats.totalTokens") }}</span>
                <span class="text-sm font-medium text-gray-900 font-mono">{{
                  todayStats.total_tokens?.toLocaleString()
                }}</span>
              </li>
              <li class="flex justify-between items-start pb-4 border-b border-gray-100 last:border-0 last:pb-0">
                <span class="text-sm text-gray-500 mt-0.5">{{ $t("dashboard.todayLogStats.totalCost") }}</span>
                <div class="text-right">
                  <div
                    v-for="(cost, currency) in todayStats.total_cost"
                    :key="currency"
                    class="text-sm font-medium text-gray-900 font-mono"
                  >
                    {{ cost / 1000000000 }}
                    <span class="text-xs text-gray-500 font-sans ml-1">{{ $t(`currencies.${currency}`, {}, currency) }}</span>
                  </div>
                  <div v-if="Object.keys(todayStats.total_cost || {}).length === 0" class="text-sm font-medium text-gray-900 font-mono">
                    0
                  </div>
                </div>
              </li>
            </ul>
          </CardContent>
        </Card>
      </div>

      <!-- Usage Stats Chart Card (ensure proper styling without shadow) -->
      <div class="border border-gray-200 rounded-lg bg-white overflow-hidden">
        <UsageChart />
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
    error.value = err.message || $t("unknownError", "Unknown Error");
  } finally {
    isLoading.value = false;
  }
};

onMounted(() => {
  fetchData();
});
</script>
