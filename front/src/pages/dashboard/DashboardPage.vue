<script setup lang="ts">
import { onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { RefreshCcw } from "lucide-vue-next";

import PageHeader from "@/components/PageHeader.vue";
import SectionHeader from "@/components/SectionHeader.vue";
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
      <PageHeader :title="$t('sidebar.dashboard')">
        <template #actions>
          <Button class="w-full sm:w-auto" :disabled="isRefreshing" @click="fetchDashboard">
            <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isRefreshing }" />
            {{ $t("common.refresh") }}
          </Button>
        </template>
      </PageHeader>

      <div class="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 xl:gap-6">
        <div class="md:col-span-2 lg:col-span-3">
          <DashboardKpiGrid
            :loading="kpiLoading"
            :error="kpiError"
            :cards="kpiCards"
          />
        </div>

        <div class="md:col-span-2 lg:col-span-2">
          <section class="flex h-full flex-col rounded-lg border border-gray-200 bg-white">
            <SectionHeader
              :title="$t('dashboard.sections.trends.title')"
              class="border-b border-gray-100 px-4 py-4 sm:px-6"
            />
            <div class="flex-1 px-0 pb-0">
              <UsageChart class="h-full min-h-[400px]" />
            </div>
          </section>
        </div>

        <div class="md:col-span-2 lg:col-span-1">
          <DashboardAlertPanel
            class="h-full"
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

        <div class="md:col-span-2 lg:col-span-3">
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
        </div>
      </div>
    </div>
  </div>
</template>
