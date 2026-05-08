import type { RuntimeStateBackendStatus } from "./shared";
import type { ProviderRuntimeLevel } from "./providerRuntime";

// ========== System / Dashboard Types ==========
export interface SystemOverviewStats {
  providers_count: number;
  models_count: number;
  provider_keys_count: number;
  runtime_state_backend: RuntimeStateBackendStatus;
}

export interface TodayRequestLogStats {
  requests_count: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_reasoning_tokens: number;
  total_tokens: number;
  total_cost: Record<string, number>;
}

export type DashboardRuntimeWindow = "15m" | "1h" | "6h" | "24h";

export interface DashboardOverviewStats {
  provider_count: number;
  enabled_provider_count: number;
  model_count: number;
  enabled_model_count: number;
  provider_key_count: number;
  enabled_provider_key_count: number;
  api_key_count: number;
  enabled_api_key_count: number;
}

export interface DashboardTodayStats {
  request_count: number;
  success_count: number;
  error_count: number;
  success_rate: number | null;
  total_input_tokens: number;
  total_output_tokens: number;
  total_reasoning_tokens: number;
  total_tokens: number;
  total_cost: Record<string, number>;
  avg_first_byte_ms: number | null;
  avg_total_latency_ms: number | null;
  active_provider_count: number;
  active_model_count: number;
  active_api_key_count: number;
}

export interface DashboardRuntimeSummary {
  window: DashboardRuntimeWindow;
  healthy_count: number;
  degraded_count: number;
  half_open_count: number;
  open_count: number;
  no_traffic_count: number;
}

export interface DashboardProviderAlertItem {
  provider_id: number;
  provider_key: string;
  provider_name: string;
  runtime_level: ProviderRuntimeLevel;
  request_count: number;
  error_count: number;
  success_rate: number | null;
  avg_total_latency_ms: number | null;
  last_error_at: number | null;
  last_error_summary: string | null;
}

export interface DashboardCostProviderAlertItem {
  provider_id: number;
  provider_key: string;
  provider_name: string;
  request_count: number;
  success_rate: number | null;
  avg_total_latency_ms: number | null;
  total_cost: Record<string, number>;
}

export interface DashboardCostModelAlertItem {
  provider_id: number;
  provider_key: string;
  model_id: number;
  model_name: string;
  real_model_name: string | null;
  request_count: number;
  total_tokens: number;
  total_cost: Record<string, number>;
}

export interface DashboardAlerts {
  open_providers: DashboardProviderAlertItem[];
  half_open_providers: DashboardProviderAlertItem[];
  degraded_providers: DashboardProviderAlertItem[];
  top_error_providers: DashboardProviderAlertItem[];
  top_cost_providers: DashboardCostProviderAlertItem[];
  top_cost_models: DashboardCostModelAlertItem[];
}

export interface DashboardTopProviderItem {
  provider_id: number;
  provider_key: string;
  provider_name: string;
  request_count: number;
  success_count: number;
  error_count: number;
  success_rate: number | null;
  total_cost: Record<string, number>;
  avg_total_latency_ms: number | null;
}

export interface DashboardTopModelItem {
  provider_id: number;
  provider_key: string;
  model_id: number;
  model_name: string;
  real_model_name: string | null;
  request_count: number;
  total_tokens: number;
  total_cost: Record<string, number>;
}

export interface DashboardResponse {
  overview: DashboardOverviewStats;
  today: DashboardTodayStats;
  runtime: DashboardRuntimeSummary;
  runtime_state_backend: RuntimeStateBackendStatus;
  alerts: DashboardAlerts;
  top_providers: DashboardTopProviderItem[];
  top_models: DashboardTopModelItem[];
}

export interface DashboardKpiSection {
  today: DashboardTodayStats;
  runtime: DashboardRuntimeSummary;
}

export interface DashboardResourcesSection {
  overview: DashboardOverviewStats;
  today: DashboardTodayStats;
  runtime: DashboardRuntimeSummary;
  runtime_state_backend: RuntimeStateBackendStatus;
}

export interface DashboardAlertsSection {
  alerts: DashboardAlerts;
  top_providers: DashboardTopProviderItem[];
  top_models: DashboardTopModelItem[];
}

export interface UsageStatItem {
  provider_id: number | null;
  model_id: number | null;
  api_key_id: number | null;
  provider_key: string | null;
  model_name: string | null;
  real_model_name: string | null;
  api_key_name: string | null;
  group_key: string;
  group_label: string;
  group_detail: string | null;
  total_input_tokens: number;
  total_output_tokens: number;
  total_reasoning_tokens: number;
  total_tokens: number;
  request_count: number;
  success_count: number;
  error_count: number;
  success_rate: number | null;
  avg_total_latency_ms: number | null;
  latency_sample_count: number;
  total_cost: Record<string, number>;
  is_other: boolean;
}

export interface UsageStatsPeriod {
  time: number;
  data: UsageStatItem[];
}
