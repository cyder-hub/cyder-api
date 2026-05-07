<script setup lang="ts">
import { onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { RefreshCcw } from "lucide-vue-next";

import UsageChart from "@/components/UsageChart.vue";
import { Button } from "@/components/ui/button";
import * as dashboardService from "@/services/dashboard";
import DashboardAlertPanel from "./components/DashboardAlertPanel.vue";
import DashboardKpiGrid from "./components/DashboardKpiGrid.vue";
import DashboardRuntimePanel from "./components/DashboardRuntimePanel.vue";
import { useDashboardAlerts } from "./composables/useDashboardAlerts";
import { useDashboardData } from "./composables/useDashboardData";

const { t: $t } = useI18n();
const router = useRouter();

const dashboardData = useDashboardData({
  api: dashboardService,
  t: $t,
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
  formatDateTime,
  isRefreshing,
  kpiCards,
  kpiError,
  kpiLoading,
  resourceItems,
  resourcesError,
  resourcesLoading,
  runtimeBackendBadgeClass,
  runtimeBackendBadgeLabel,
  runtimeBackendDetail,
  runtimeBackendHeadline,
  runtimeBackendRows,
  runtimeBackendStatus,
  runtimeBadgeClass,
  runtimeItems,
} = dashboardData;

const dashboardAlerts = useDashboardAlerts(alertsSection, { t: $t });
const {
  formatCostEntries,
  formatCount,
  formatDateTime: formatAlertDateTime,
  formatLatency,
  formatPercentage,
  runtimeLevelBadgeClass,
  runtimeLevelLabel,
  showCostHotspots,
  unstableProviders,
} = dashboardAlerts;

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
        <DashboardKpiGrid
          :loading="kpiLoading"
          :error="kpiError"
          :cards="kpiCards"
        />

        <DashboardRuntimePanel
          :loading="resourcesLoading"
          :error="resourcesError"
          :resource-items="resourceItems"
          :runtime-items="runtimeItems"
          :runtime-backend-status="runtimeBackendStatus"
          :runtime-backend-headline="runtimeBackendHeadline"
          :runtime-backend-detail="runtimeBackendDetail"
          :runtime-backend-rows="runtimeBackendRows"
          :runtime-backend-badge-label="runtimeBackendBadgeLabel"
          :runtime-backend-badge-class="runtimeBackendBadgeClass"
          :runtime-badge-class="runtimeBadgeClass"
          :format-date-time="formatDateTime"
          @view-runtime="goToRuntime"
          @view-records="goToRecords"
        />

        <div class="border border-gray-200 bg-white rounded-lg">
          <div class="px-4 py-4 sm:px-6">
            <h2 class="text-base font-semibold text-gray-900">
              {{ $t("dashboard.sections.trends.title") }}
            </h2>
          </div>
          <div class="px-0 pb-0">
            <UsageChart />
          </div>
        </div>

        <DashboardAlertPanel
          :loading="alertsLoading"
          :error="alertsError"
          :alerts-section="alertsSection"
          :unstable-providers="unstableProviders"
          :show-cost-hotspots="showCostHotspots"
          :format-count="formatCount"
          :format-percentage="formatPercentage"
          :format-latency="formatLatency"
          :format-date-time="formatAlertDateTime"
          :format-cost-entries="formatCostEntries"
          :runtime-level-badge-class="runtimeLevelBadgeClass"
          :runtime-level-label="runtimeLevelLabel"
          @view-runtime="goToRuntime"
          @view-records="goToRecords"
          @edit-provider="goToProvider"
          @edit-model="goToModel"
        />
      </div>
    </div>
  </div>
</template>
