import type {
  DashboardAlertsSection,
  DashboardKpiSection,
  DashboardResourcesSection,
  ProviderRuntimeLevel,
  RuntimeStateBackendName,
  RuntimeStateBackendStatus,
} from "@/services/types";
import type { RuntimeStateBackendScope } from "@/utils/runtimeBackend";

export interface DashboardApiClient {
  getSystemDashboardKpi: () => Promise<DashboardKpiSection>;
  getSystemDashboardResources: () => Promise<DashboardResourcesSection>;
  getSystemDashboardAlerts: () => Promise<DashboardAlertsSection>;
}

export type DashboardTranslator = (
  key: string,
  params?: Record<string, unknown>,
) => string;

export interface DashboardKpiCardItem {
  key: string;
  label: string;
  value: string;
  description: string;
}

export interface DashboardResourceItem {
  key: string;
  label: string;
  value: string;
  description: string;
}

export interface DashboardRuntimeItem {
  key: string;
  label: string;
  value: string;
  description: string;
}

export interface DashboardRuntimeBackendDisplayRow {
  key: RuntimeStateBackendScope;
  label: string;
  configured: RuntimeStateBackendName;
  effective: RuntimeStateBackendName;
  changed: boolean;
}

export interface DashboardRuntimeBackendView {
  status: RuntimeStateBackendStatus;
  headline: string;
  detail: string;
  rows: DashboardRuntimeBackendDisplayRow[];
  badgeLabel: string;
  badgeClass: string;
}

export type DashboardFormatCount = (value: number | null | undefined) => string;
export type DashboardFormatPercentage = (value: number | null | undefined) => string;
export type DashboardFormatLatency = (value: number | null | undefined) => string;
export type DashboardFormatDateTime = (value: number | null | undefined) => string;
export type DashboardFormatCostEntries = (costMap: Record<string, number>) => string[];
export type DashboardRuntimeLevelClass = (level: ProviderRuntimeLevel) => string;
export type DashboardRuntimeLevelLabel = (level: ProviderRuntimeLevel) => string;
