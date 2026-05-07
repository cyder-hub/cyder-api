import { computed, ref } from "vue";
import type {
  DashboardAlertsSection,
  DashboardKpiSection,
  DashboardResponse,
  DashboardRuntimeSummary,
  ProviderRuntimeLevel,
  RuntimeStateBackendStatus,
} from "../../../services/types";
import {
  buildDefaultRuntimeStateBackendStatus,
  buildRuntimeStateBackendRows,
} from "../../../utils/runtimeBackend.ts";
import { formatTimestamp } from "../../../utils/datetime.ts";
import { formatPriceFromNanos } from "../../../utils/money.ts";
import type {
  DashboardApiClient,
  DashboardTranslator,
} from "../types";

const identityTranslate: DashboardTranslator = (key) => key;

export function buildEmptyDashboard(): DashboardResponse {
  return {
    overview: {
      provider_count: 0,
      enabled_provider_count: 0,
      model_count: 0,
      enabled_model_count: 0,
      provider_key_count: 0,
      enabled_provider_key_count: 0,
      api_key_count: 0,
      enabled_api_key_count: 0,
    },
    today: {
      request_count: 0,
      success_count: 0,
      error_count: 0,
      success_rate: null,
      total_input_tokens: 0,
      total_output_tokens: 0,
      total_reasoning_tokens: 0,
      total_tokens: 0,
      total_cost: {},
      avg_first_byte_ms: null,
      avg_total_latency_ms: null,
      active_provider_count: 0,
      active_model_count: 0,
      active_api_key_count: 0,
    },
    runtime: {
      window: "1h",
      healthy_count: 0,
      degraded_count: 0,
      half_open_count: 0,
      open_count: 0,
      no_traffic_count: 0,
    },
    runtime_state_backend: buildDefaultRuntimeStateBackendStatus(),
    alerts: {
      open_providers: [],
      half_open_providers: [],
      degraded_providers: [],
      top_error_providers: [],
      top_cost_providers: [],
      top_cost_models: [],
    },
    top_providers: [],
    top_models: [],
  };
}

export function buildEmptyDashboardKpiSection(): DashboardKpiSection {
  const dashboard = buildEmptyDashboard();
  return {
    today: dashboard.today,
    runtime: dashboard.runtime,
  };
}

export function buildEmptyDashboardResourcesSection() {
  const dashboard = buildEmptyDashboard();
  return {
    overview: dashboard.overview,
    today: dashboard.today,
    runtime: dashboard.runtime,
    runtime_state_backend: dashboard.runtime_state_backend,
  };
}

export function buildEmptyDashboardAlertsSection(): DashboardAlertsSection {
  const dashboard = buildEmptyDashboard();
  return {
    alerts: dashboard.alerts,
    top_providers: dashboard.top_providers,
    top_models: dashboard.top_models,
  };
}

export interface UseDashboardDataOptions {
  api: DashboardApiClient;
  t?: DashboardTranslator;
  getUnknownErrorMessage?: () => string;
  logError?: (message: string, error: unknown) => void;
}

export function useDashboardData(options: UseDashboardDataOptions) {
  const api = options.api;
  const t = options.t ?? identityTranslate;
  const getUnknownErrorMessage =
    options.getUnknownErrorMessage ?? (() => "Unknown error");
  const logError = options.logError ?? (() => {});

  if (!api) {
    throw new Error("useDashboardData requires an api client");
  }

  const kpiSection = ref(buildEmptyDashboardKpiSection());
  const resourcesSection = ref(buildEmptyDashboardResourcesSection());
  const alertsSection = ref(buildEmptyDashboardAlertsSection());

  const kpiLoading = ref(true);
  const resourcesLoading = ref(true);
  const alertsLoading = ref(true);

  const kpiError = ref<string | null>(null);
  const resourcesError = ref<string | null>(null);
  const alertsError = ref<string | null>(null);

  const isRefreshing = computed(
    () => kpiLoading.value || resourcesLoading.value || alertsLoading.value,
  );

  const toErrorMessage = (error: unknown) =>
    error instanceof Error && error.message ? error.message : getUnknownErrorMessage();

  const fetchKpiSection = async () => {
    kpiLoading.value = true;
    kpiError.value = null;
    try {
      kpiSection.value = await api.getSystemDashboardKpi();
    } catch (error) {
      logError("Failed to fetch dashboard KPI section:", error);
      kpiSection.value = buildEmptyDashboardKpiSection();
      kpiError.value = toErrorMessage(error);
    } finally {
      kpiLoading.value = false;
    }
  };

  const fetchResourcesSection = async () => {
    resourcesLoading.value = true;
    resourcesError.value = null;
    try {
      resourcesSection.value = await api.getSystemDashboardResources();
    } catch (error) {
      logError("Failed to fetch dashboard resources section:", error);
      resourcesSection.value = buildEmptyDashboardResourcesSection();
      resourcesError.value = toErrorMessage(error);
    } finally {
      resourcesLoading.value = false;
    }
  };

  const fetchAlertsSection = async () => {
    alertsLoading.value = true;
    alertsError.value = null;
    try {
      alertsSection.value = await api.getSystemDashboardAlerts();
    } catch (error) {
      logError("Failed to fetch dashboard alerts section:", error);
      alertsSection.value = buildEmptyDashboardAlertsSection();
      alertsError.value = toErrorMessage(error);
    } finally {
      alertsLoading.value = false;
    }
  };

  const fetchDashboard = async () => {
    await Promise.allSettled([
      fetchKpiSection(),
      fetchResourcesSection(),
      fetchAlertsSection(),
    ]);
  };

  const formatCount = (value: number | null | undefined) =>
    (value ?? 0).toLocaleString();

  const formatPercentage = (value: number | null | undefined) =>
    value == null ? "0%" : `${(value * 100).toFixed(1)}%`;

  const formatLatency = (value: number | null | undefined) =>
    value == null
      ? t("dashboard.empty.noLatency")
      : `${Math.round(value).toLocaleString()} ms`;

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

  const runtimeWindowLabel = (window: DashboardRuntimeSummary["window"]) =>
    t(`providerRuntimePage.window.${window}`);

  const backendLabel = (
    backend: RuntimeStateBackendStatus["runtime_effective_backend"],
  ) =>
    backend === "memory" || backend === "redis"
      ? t(`dashboard.runtimeState.backend.${backend}`)
      : backend;

  const deploymentModeLabel = (mode: RuntimeStateBackendStatus["deployment_mode"]) =>
    mode === "single_instance" || mode === "multi_instance"
      ? t(`dashboard.runtimeState.deployment.${mode}`)
      : mode;

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

  const runtimeBackendStatus = computed(
    () => resourcesSection.value.runtime_state_backend,
  );

  const runtimeBackendHeadline = computed(() =>
    t("dashboard.runtimeState.description", {
      deployment: deploymentModeLabel(runtimeBackendStatus.value.deployment_mode),
      runtime: backendLabel(runtimeBackendStatus.value.runtime_effective_backend),
      catalog: backendLabel(runtimeBackendStatus.value.catalog_cache_backend),
    }),
  );

  const runtimeBackendRows = computed(() =>
    buildRuntimeStateBackendRows(runtimeBackendStatus.value).map((row) => ({
      key: row.key,
      label: t(`dashboard.runtimeState.scope.${row.key}`),
      configured: backendLabel(row.configured),
      effective: backendLabel(row.effective),
      changed: row.changed,
    })),
  );

  const runtimeBackendBadgeLabel = computed(() => {
    if (runtimeBackendStatus.value.runtime_degraded) {
      return t("dashboard.runtimeState.status.degraded");
    }
    if (
      runtimeBackendStatus.value.deployment_mode === "single_instance" &&
      runtimeBackendStatus.value.runtime_effective_backend === "memory" &&
      !runtimeBackendStatus.value.fallback_reason
    ) {
      return t("dashboard.runtimeState.status.recommended");
    }
    return runtimeBackendStatus.value.runtime_shared
      ? t("dashboard.runtimeState.status.shared")
      : t("dashboard.runtimeState.status.nonShared");
  });

  const runtimeBackendBadgeClass = computed(() => {
    if (runtimeBackendStatus.value.runtime_degraded) {
      return "border-red-200 bg-red-50 text-red-700 hover:bg-red-50";
    }
    if (runtimeBackendStatus.value.runtime_shared) {
      return "border-emerald-200 bg-emerald-50 text-emerald-700 hover:bg-emerald-50";
    }
    return "border-gray-200 bg-gray-50 text-gray-700 hover:bg-gray-50";
  });

  const runtimeBackendDetail = computed(() => {
    if (runtimeBackendStatus.value.fallback_reason) {
      return t("dashboard.runtimeState.fallback", {
        reason: runtimeBackendStatus.value.fallback_reason,
      });
    }
    if (runtimeBackendStatus.value.catalog_cache_fallback_reason) {
      return t("dashboard.runtimeState.catalogFallback", {
        reason: runtimeBackendStatus.value.catalog_cache_fallback_reason,
      });
    }
    if (
      runtimeBackendStatus.value.deployment_mode === "single_instance" &&
      runtimeBackendStatus.value.runtime_effective_backend === "memory"
    ) {
      return t("dashboard.runtimeState.recommendedHint");
    }
    return runtimeBackendStatus.value.runtime_shared
      ? t("dashboard.runtimeState.sharedHint")
      : t("dashboard.runtimeState.nonSharedHint");
  });

  const kpiCards = computed(() => [
    {
      key: "requests",
      label: t("dashboard.kpi.requests"),
      value: formatCount(kpiSection.value.today.request_count),
      description: `${t("dashboard.kpi.success")} ${formatCount(
        kpiSection.value.today.success_count,
      )} / ${t("dashboard.kpi.errors")} ${formatCount(
        kpiSection.value.today.error_count,
      )}`,
    },
    {
      key: "success_rate",
      label: t("dashboard.kpi.successRate"),
      value: formatPercentage(kpiSection.value.today.success_rate),
      description: t("dashboard.kpi.windowToday"),
    },
    {
      key: "tokens",
      label: t("dashboard.kpi.totalTokens"),
      value: formatCount(kpiSection.value.today.total_tokens),
      description: `${t("dashboard.kpi.inputTokens")} ${formatCount(
        kpiSection.value.today.total_input_tokens,
      )}`,
    },
    {
      key: "cost",
      label: t("dashboard.kpi.totalCost"),
      value: formatCostEntries(kpiSection.value.today.total_cost).join(" / "),
      description: t("dashboard.kpi.multiCurrencyHint"),
    },
    {
      key: "latency",
      label: t("dashboard.kpi.avgLatency"),
      value: formatLatency(kpiSection.value.today.avg_total_latency_ms),
      description: `${t("dashboard.kpi.firstByte")} ${formatLatency(
        kpiSection.value.today.avg_first_byte_ms,
      )}`,
    },
    {
      key: "runtime_issues",
      label: t("dashboard.kpi.runtimeIssues"),
      value: formatCount(
        kpiSection.value.runtime.open_count +
          kpiSection.value.runtime.half_open_count +
          kpiSection.value.runtime.degraded_count,
      ),
      description: `${t("dashboard.kpi.runtimeWindow")} ${runtimeWindowLabel(
        kpiSection.value.runtime.window,
      )}`,
    },
  ]);

  const resourceItems = computed(() => [
    {
      key: "providers",
      label: t("dashboard.resources.providers"),
      value: `${formatCount(
        resourcesSection.value.overview.enabled_provider_count,
      )} / ${formatCount(resourcesSection.value.overview.provider_count)}`,
      description: t("dashboard.resources.enabledTotal"),
    },
    {
      key: "models",
      label: t("dashboard.resources.models"),
      value: `${formatCount(
        resourcesSection.value.overview.enabled_model_count,
      )} / ${formatCount(resourcesSection.value.overview.model_count)}`,
      description: t("dashboard.resources.enabledTotal"),
    },
    {
      key: "provider_keys",
      label: t("dashboard.resources.providerKeys"),
      value: `${formatCount(
        resourcesSection.value.overview.enabled_provider_key_count,
      )} / ${formatCount(resourcesSection.value.overview.provider_key_count)}`,
      description: t("dashboard.resources.enabledTotal"),
    },
    {
      key: "api_keys",
      label: t("dashboard.resources.apiKeys"),
      value: `${formatCount(
        resourcesSection.value.overview.enabled_api_key_count,
      )} / ${formatCount(resourcesSection.value.overview.api_key_count)}`,
      description: `${t("dashboard.resources.activeToday")} ${formatCount(
        resourcesSection.value.today.active_api_key_count,
      )}`,
    },
  ]);

  const runtimeItems = computed(() => [
    {
      key: "healthy_count",
      label: t("providerRuntimePage.summary.healthy"),
      value: formatCount(resourcesSection.value.runtime.healthy_count),
      description: t("dashboard.runtime.windowDetail", {
        window: runtimeWindowLabel(resourcesSection.value.runtime.window),
      }),
    },
    {
      key: "degraded_count",
      label: t("providerRuntimePage.summary.degraded"),
      value: formatCount(resourcesSection.value.runtime.degraded_count),
      description: t("dashboard.runtime.degradedHint"),
    },
    {
      key: "half_open_count",
      label: t("providerRuntimePage.summary.halfOpen"),
      value: formatCount(resourcesSection.value.runtime.half_open_count),
      description: t("dashboard.runtime.halfOpenHint"),
    },
    {
      key: "open_count",
      label: t("providerRuntimePage.summary.open"),
      value: formatCount(resourcesSection.value.runtime.open_count),
      description: t("dashboard.runtime.openHint"),
    },
    {
      key: "no_traffic_count",
      label: t("providerRuntimePage.summary.noTraffic"),
      value: formatCount(resourcesSection.value.runtime.no_traffic_count),
      description: t("dashboard.runtime.noTrafficHint"),
    },
    {
      key: "active_provider_count",
      label: t("dashboard.runtime.activeProviders"),
      value: formatCount(resourcesSection.value.today.active_provider_count),
      description: `${t("dashboard.runtime.activeModels")} ${formatCount(
        resourcesSection.value.today.active_model_count,
      )}`,
    },
  ]);

  return {
    alertsError,
    alertsLoading,
    alertsSection,
    fetchAlertsSection,
    fetchDashboard,
    fetchKpiSection,
    fetchResourcesSection,
    formatCount,
    formatCostEntries,
    formatDateTime,
    formatLatency,
    formatPercentage,
    isRefreshing,
    kpiCards,
    kpiError,
    kpiLoading,
    kpiSection,
    resourceItems,
    resourcesError,
    resourcesLoading,
    resourcesSection,
    runtimeBackendBadgeClass,
    runtimeBackendBadgeLabel,
    runtimeBackendDetail,
    runtimeBackendHeadline,
    runtimeBackendRows,
    runtimeBackendStatus,
    runtimeBadgeClass,
    runtimeItems,
    runtimeLevelBadgeClass,
  };
}
