import type {
  DashboardAlerts,
  DashboardAlertsSection,
  DashboardKpiSection,
  DashboardProviderAlertItem,
  DashboardResourcesSection,
  DashboardResponse,
} from "@/store/types";

export function buildEmptyDashboard(): DashboardResponse;
export function buildEmptyDashboardKpiSection(): DashboardKpiSection;
export function buildEmptyDashboardResourcesSection(): DashboardResourcesSection;
export function buildEmptyDashboardAlertsSection(): DashboardAlertsSection;
export function getUnstableProviders(alerts: DashboardAlerts): DashboardProviderAlertItem[];
export function hasCostHotspots(alerts: DashboardAlerts): boolean;
