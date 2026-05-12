import { computed, type Ref } from "vue";
import type {
  DashboardAlerts,
  DashboardAlertsSection,
  ProviderRuntimeLevel,
} from "../../../services/types";
import { formatTimestamp } from "../../../utils/datetime.ts";
import { formatPriceFromNanos } from "../../../utils/money.ts";
import { formatNumberValue } from "../../../utils/number.ts";
import type { DashboardTranslator } from "../types";

const identityTranslate: DashboardTranslator = (key) => key;

export function getUnstableProviders(alerts: DashboardAlerts) {
  return [...alerts.open_providers, ...alerts.half_open_providers].sort(
    (left, right) =>
      right.error_count - left.error_count || left.provider_id - right.provider_id,
  );
}

export function hasCostHotspots(alerts: DashboardAlerts) {
  return alerts.top_cost_providers.length > 0 || alerts.top_cost_models.length > 0;
}

export interface UseDashboardAlertsOptions {
  t?: DashboardTranslator;
}

export function useDashboardAlerts(
  alertsSection: Ref<DashboardAlertsSection>,
  options: UseDashboardAlertsOptions = {},
) {
  const t = options.t ?? identityTranslate;

  const unstableProviders = computed(() =>
    getUnstableProviders(alertsSection.value.alerts),
  );
  const showCostHotspots = computed(() => hasCostHotspots(alertsSection.value.alerts));

  const formatCount = (value: number | null | undefined) =>
    formatNumberValue(value ?? 0);

  const formatPercentage = (value: number | null | undefined) =>
    value == null ? "0%" : `${(value * 100).toFixed(1)}%`;

  const formatLatency = (value: number | null | undefined) =>
    value == null
      ? t("dashboard.empty.noLatency")
      : `${formatNumberValue(Math.round(value))} ms`;

  const formatDateTime = (value: number | null | undefined) =>
    formatTimestamp(value) || "-";

  const formatDashboardCost = (nanos: number, currency: string) =>
    formatPriceFromNanos(nanos, currency, "0");

  const formatCostEntries = (costMap: Record<string, number>) => {
    const entries = Object.entries(costMap);
    if (!entries.length) {
      return ["0"];
    }

    return entries.map(([currency, amount]) => formatDashboardCost(amount, currency));
  };

  const runtimeLevelLabel = (level: ProviderRuntimeLevel) =>
    t(`providerRuntimePage.status.${level}`);

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

  return {
    formatCostEntries,
    formatCount,
    formatDateTime,
    formatLatency,
    formatPercentage,
    runtimeLevelBadgeClass,
    runtimeLevelLabel,
    showCostHotspots,
    unstableProviders,
  };
}
