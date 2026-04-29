import type {
  DashboardAlerts,
  DashboardAlertsSection,
  DashboardKpiSection,
  DashboardProviderAlertItem,
  DashboardResourcesSection,
  DashboardResponse,
  RuntimeStateBackendStatus,
} from "@/store/types";

export interface RuntimeStateBackendRow {
  key: "runtime" | "catalog";
  configured: string;
  effective: string;
  fallback_reason: string | null;
  changed: boolean;
}

export function buildEmptyDashboard(): DashboardResponse;
export function buildEmptyDashboardKpiSection(): DashboardKpiSection;
export function buildEmptyDashboardResourcesSection(): DashboardResourcesSection;
export function buildEmptyDashboardAlertsSection(): DashboardAlertsSection;
export function buildRuntimeStateBackendRows(
  status: RuntimeStateBackendStatus,
): RuntimeStateBackendRow[];
export function getUnstableProviders(alerts: DashboardAlerts): DashboardProviderAlertItem[];
export function hasCostHotspots(alerts: DashboardAlerts): boolean;
