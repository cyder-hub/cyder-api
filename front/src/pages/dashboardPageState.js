import { computed, ref } from "vue";
import {
  buildEmptyDashboardAlertsSection,
  buildEmptyDashboardKpiSection,
  buildEmptyDashboardResourcesSection,
  getUnstableProviders,
  hasCostHotspots,
} from "./dashboardViewModel.js";

export function createDashboardPageState(options = {}) {
  const api = options.api;
  const getUnknownErrorMessage =
    options.getUnknownErrorMessage || (() => "Unknown error");
  const logError = options.logError || (() => {});

  if (!api) {
    throw new Error("createDashboardPageState requires an api client");
  }

  const kpiSection = ref(buildEmptyDashboardKpiSection());
  const resourcesSection = ref(buildEmptyDashboardResourcesSection());
  const alertsSection = ref(buildEmptyDashboardAlertsSection());

  const kpiLoading = ref(true);
  const resourcesLoading = ref(true);
  const alertsLoading = ref(true);

  const kpiError = ref(null);
  const resourcesError = ref(null);
  const alertsError = ref(null);

  const isRefreshing = computed(
    () => kpiLoading.value || resourcesLoading.value || alertsLoading.value,
  );
  const unstableProviders = computed(() => getUnstableProviders(alertsSection.value.alerts));
  const showCostHotspots = computed(() => hasCostHotspots(alertsSection.value.alerts));

  const toErrorMessage = (err) => err?.message || getUnknownErrorMessage();

  const fetchKpiSection = async () => {
    kpiLoading.value = true;
    kpiError.value = null;
    try {
      kpiSection.value = await api.getSystemDashboardKpi();
    } catch (err) {
      logError("Failed to fetch dashboard KPI section:", err);
      kpiSection.value = buildEmptyDashboardKpiSection();
      kpiError.value = toErrorMessage(err);
    } finally {
      kpiLoading.value = false;
    }
  };

  const fetchResourcesSection = async () => {
    resourcesLoading.value = true;
    resourcesError.value = null;
    try {
      resourcesSection.value = await api.getSystemDashboardResources();
    } catch (err) {
      logError("Failed to fetch dashboard resources section:", err);
      resourcesSection.value = buildEmptyDashboardResourcesSection();
      resourcesError.value = toErrorMessage(err);
    } finally {
      resourcesLoading.value = false;
    }
  };

  const fetchAlertsSection = async () => {
    alertsLoading.value = true;
    alertsError.value = null;
    try {
      alertsSection.value = await api.getSystemDashboardAlerts();
    } catch (err) {
      logError("Failed to fetch dashboard alerts section:", err);
      alertsSection.value = buildEmptyDashboardAlertsSection();
      alertsError.value = toErrorMessage(err);
    } finally {
      alertsLoading.value = false;
    }
  };

  const fetchDashboard = async () => {
    await Promise.allSettled([fetchKpiSection(), fetchResourcesSection(), fetchAlertsSection()]);
  };

  return {
    alertsError,
    alertsLoading,
    alertsSection,
    fetchAlertsSection,
    fetchDashboard,
    fetchKpiSection,
    fetchResourcesSection,
    isRefreshing,
    kpiError,
    kpiLoading,
    kpiSection,
    resourcesError,
    resourcesLoading,
    resourcesSection,
    showCostHotspots,
    unstableProviders,
  };
}
