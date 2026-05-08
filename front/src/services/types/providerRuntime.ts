import type { RuntimeStateBackendStatus } from "./shared";

export type ProviderRuntimeWindow = "15m" | "1h" | "6h" | "24h";

export type ProviderRuntimeHealthStatus = "healthy" | "open" | "half_open";

export type ProviderRuntimeLevel =
  | "healthy"
  | "degraded"
  | "open"
  | "half_open"
  | "no_traffic";

export type ProviderRuntimeStatusFilter =
  | "all"
  | "healthy"
  | "degraded"
  | "open"
  | "half_open"
  | "no_traffic";

export type ProviderRuntimeSortField =
  | "health"
  | "error_rate"
  | "latency"
  | "last_error_at"
  | "request_count";

export type SortDirection = "asc" | "desc";

export interface ProviderRuntimeStatusCodeStat {
  status_code: number;
  count: number;
}

export interface ProviderRuntimeCostStat {
  currency: string;
  amount_nanos: number;
}

export interface ProviderRuntimeItem {
  provider_id: number;
  provider_key: string;
  provider_name: string;
  provider_type: string;
  is_enabled: boolean;
  use_proxy: boolean;
  enabled_model_count: number;
  enabled_provider_key_count: number;
  health_status: ProviderRuntimeHealthStatus;
  runtime_level: ProviderRuntimeLevel;
  consecutive_failures: number;
  half_open_probe_in_flight: boolean;
  opened_at: number | null;
  last_failure_at: number | null;
  last_recovered_at: number | null;
  last_error: string | null;
  runtime_state_backend_degraded: boolean;
  runtime_state_backend_error: string | null;
  request_count: number;
  success_count: number;
  error_count: number;
  success_rate: number | null;
  avg_first_byte_ms: number | null;
  avg_total_latency_ms: number | null;
  last_request_at: number | null;
  last_success_at: number | null;
  last_error_at: number | null;
  last_error_summary: string | null;
  status_code_breakdown: ProviderRuntimeStatusCodeStat[];
  total_cost: ProviderRuntimeCostStat[];
  sort_score: number;
}

export interface ProviderRuntimeSummary {
  total_provider_count: number;
  healthy_count: number;
  degraded_count: number;
  half_open_count: number;
  open_count: number;
  no_traffic_count: number;
  window: ProviderRuntimeWindow;
  generated_at: number;
  runtime_state_backend: RuntimeStateBackendStatus;
}

export interface ProviderRuntimeListParams {
  window?: ProviderRuntimeWindow;
  status?: ProviderRuntimeStatusFilter;
  search?: string;
  sort?: ProviderRuntimeSortField;
  direction?: SortDirection;
  only_enabled?: boolean;
}
