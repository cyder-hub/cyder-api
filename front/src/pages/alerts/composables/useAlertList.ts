import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import * as alertService from "@/services/alerts";
import * as notificationService from "@/services/notifications";
import type {
  AlertEvent,
  AlertScopeType,
  AlertSeverity,
  AlertStatus,
  NotificationDelivery,
} from "@/services/types";
import { normalizeError } from "@/utils/error";
import type { AlertSelectOption, AlertSummaryCard } from "../types";
import {
  buildAlertListParams,
  buildAlertSummaryCounts,
  createDefaultAlertFilters,
  filterAlertsByQuery,
} from "./alertViewModel";

const ALERT_PAGE_SIZE = 50;

export function useAlertList() {
  const { t } = useI18n();

  const alerts = ref<AlertEvent[]>([]);
  const deliveries = ref<NotificationDelivery[]>([]);
  const selectedAlert = ref<AlertEvent | null>(null);
  const isLoading = ref(true);
  const isRefreshing = ref(false);
  const isDetailLoading = ref(false);
  const error = ref<string | null>(null);
  const deliveryError = ref<string | null>(null);
  const filters = ref(createDefaultAlertFilters());
  const offset = ref(0);
  const nextOffset = ref<number | null>(null);

  const statusOptions = computed<AlertSelectOption<AlertStatus | "all">[]>(() => [
    { value: "active", label: t("alertsPage.filter.statusActive") },
    { value: "resolved", label: t("alertsPage.filter.statusResolved") },
    { value: "all", label: t("alertsPage.filter.allStatuses") },
  ]);

  const severityOptions = computed<AlertSelectOption<AlertSeverity | "all">[]>(() => [
    { value: "all", label: t("alertsPage.filter.allSeverities") },
    { value: "critical", label: t("alertsPage.severity.critical") },
    { value: "warning", label: t("alertsPage.severity.warning") },
    { value: "info", label: t("alertsPage.severity.info") },
  ]);

  const scopeOptions = computed<AlertSelectOption<AlertScopeType | "all">[]>(() => [
    { value: "all", label: t("alertsPage.filter.allScopes") },
    { value: "provider", label: t("alertsPage.scope.provider") },
    { value: "model", label: t("alertsPage.scope.model") },
    { value: "api_key", label: t("alertsPage.scope.api_key") },
    { value: "provider_api_key", label: t("alertsPage.scope.provider_api_key") },
    { value: "provider_model", label: t("alertsPage.scope.provider_model") },
    { value: "system", label: t("alertsPage.scope.system") },
    { value: "global", label: t("alertsPage.scope.global") },
  ]);

  const booleanOptions = computed<AlertSelectOption<"all" | "yes" | "no">[]>(() => [
    { value: "all", label: t("alertsPage.filter.all") },
    { value: "yes", label: t("common.yes") },
    { value: "no", label: t("common.no") },
  ]);

  const visibleAlerts = computed(() =>
    filterAlertsByQuery(alerts.value, filters.value.query),
  );

  const summaryCards = computed<AlertSummaryCard[]>(() => {
    const counts = buildAlertSummaryCounts(alerts.value);
    return [
      { key: "active", label: t("alertsPage.summary.active"), value: counts.active },
      { key: "critical", label: t("alertsPage.summary.critical"), value: counts.critical },
      {
        key: "suppressed",
        label: t("alertsPage.summary.suppressed"),
        value: counts.suppressed,
      },
      {
        key: "acknowledged",
        label: t("alertsPage.summary.acknowledged"),
        value: counts.acknowledged,
      },
    ];
  });

  const canGoPrevious = computed(() => offset.value > 0);
  const canGoNext = computed(() => nextOffset.value !== null);

  const loadDeliveries = async (alertId: number) => {
    deliveryError.value = null;
    try {
      const response = await notificationService.getNotificationDeliveries({
        alert_id: alertId,
        limit: 10,
      });
      deliveries.value = response.items;
    } catch (err: unknown) {
      deliveryError.value = normalizeError(err, t("common.unknownError")).message;
      deliveries.value = [];
    }
  };

  const loadAlerts = async (options: { resetOffset?: boolean } = {}) => {
    if (options.resetOffset) {
      offset.value = 0;
    }
    isRefreshing.value = true;
    error.value = null;
    try {
      const response = await alertService.getAlerts(
        buildAlertListParams(filters.value, {
          limit: ALERT_PAGE_SIZE,
          offset: offset.value,
        }),
      );
      alerts.value = response.items;
      nextOffset.value = response.next_offset;

      if (selectedAlert.value) {
        selectedAlert.value =
          response.items.find((item) => item.id === selectedAlert.value?.id) ??
          response.items[0] ??
          null;
      } else {
        selectedAlert.value = response.items[0] ?? null;
      }

      if (selectedAlert.value) {
        await loadDeliveries(selectedAlert.value.id);
      } else {
        deliveries.value = [];
      }
    } catch (err: unknown) {
      error.value = normalizeError(err, t("common.unknownError")).message;
    } finally {
      isLoading.value = false;
      isRefreshing.value = false;
    }
  };

  const goToPreviousPage = async () => {
    if (!canGoPrevious.value) return;
    offset.value = Math.max(0, offset.value - ALERT_PAGE_SIZE);
    await loadAlerts();
  };

  const goToNextPage = async () => {
    if (nextOffset.value === null) return;
    offset.value = nextOffset.value;
    await loadAlerts();
  };

  const selectAlert = async (alert: AlertEvent) => {
    selectedAlert.value = alert;
    await loadDeliveries(alert.id);
  };

  const refreshSelectedAlert = async () => {
    if (!selectedAlert.value) return;
    isDetailLoading.value = true;
    try {
      selectedAlert.value = await alertService.getAlert(selectedAlert.value.id);
      await loadDeliveries(selectedAlert.value.id);
    } finally {
      isDetailLoading.value = false;
    }
  };

  onMounted(() => {
    void loadAlerts();
  });

  return {
    alerts,
    deliveries,
    selectedAlert,
    isLoading,
    isRefreshing,
    isDetailLoading,
    error,
    deliveryError,
    filters,
    statusOptions,
    severityOptions,
    scopeOptions,
    booleanOptions,
    visibleAlerts,
    summaryCards,
    offset,
    nextOffset,
    canGoPrevious,
    canGoNext,
    loadAlerts,
    loadDeliveries,
    selectAlert,
    refreshSelectedAlert,
    goToPreviousPage,
    goToNextPage,
  };
}
