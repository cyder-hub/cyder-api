import type { ComputedRef, Ref } from "vue";
import type {
  DashboardAlertsSection,
  DashboardKpiSection,
  DashboardProviderAlertItem,
  DashboardResourcesSection,
} from "@/store/types";

export interface DashboardPageStateApi {
  getSystemDashboardKpi(): Promise<DashboardKpiSection>;
  getSystemDashboardResources(): Promise<DashboardResourcesSection>;
  getSystemDashboardAlerts(): Promise<DashboardAlertsSection>;
}

export interface CreateDashboardPageStateOptions {
  api: DashboardPageStateApi;
  getUnknownErrorMessage?: () => string;
  logError?: (message: string, error: unknown) => void;
}

export interface DashboardPageState {
  alertsError: Ref<string | null>;
  alertsLoading: Ref<boolean>;
  alertsSection: Ref<DashboardAlertsSection>;
  fetchAlertsSection(): Promise<void>;
  fetchDashboard(): Promise<void>;
  fetchKpiSection(): Promise<void>;
  fetchResourcesSection(): Promise<void>;
  isRefreshing: ComputedRef<boolean>;
  kpiError: Ref<string | null>;
  kpiLoading: Ref<boolean>;
  kpiSection: Ref<DashboardKpiSection>;
  resourcesError: Ref<string | null>;
  resourcesLoading: Ref<boolean>;
  resourcesSection: Ref<DashboardResourcesSection>;
  showCostHotspots: ComputedRef<boolean>;
  unstableProviders: ComputedRef<DashboardProviderAlertItem[]>;
}

export function createDashboardPageState(
  options?: CreateDashboardPageStateOptions,
): DashboardPageState;
