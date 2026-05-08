import { request } from "./http";
import type {
  DashboardAlertsSection,
  DashboardKpiSection,
  DashboardResourcesSection,
  DashboardResponse,
  SystemOverviewStats,
  TodayRequestLogStats,
  UsageStatsPeriod,
} from "./types";

export function getSystemOverview(): Promise<SystemOverviewStats> {
  return request.get("/ai/manager/api/system/overview");
}

export function getTodayLogStats(): Promise<TodayRequestLogStats> {
  return request.get("/ai/manager/api/system/today_log_stats");
}

export function getSystemDashboard(): Promise<DashboardResponse> {
  return request.get("/ai/manager/api/system/dashboard");
}

export function getSystemDashboardKpi(): Promise<DashboardKpiSection> {
  return request.get("/ai/manager/api/system/dashboard/kpi");
}

export function getSystemDashboardResources(): Promise<DashboardResourcesSection> {
  return request.get("/ai/manager/api/system/dashboard/resources");
}

export function getSystemDashboardAlerts(): Promise<DashboardAlertsSection> {
  return request.get("/ai/manager/api/system/dashboard/alerts");
}

export function getUsageStats(params: URLSearchParams): Promise<UsageStatsPeriod[]> {
  return request(`/ai/manager/api/system/usage_stats?${params.toString()}`);
}
