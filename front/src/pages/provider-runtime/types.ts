import type { ProviderRuntimeListParams, ProviderRuntimeSnapshot } from "@/services/types";
import type { RuntimeStateBackendScope } from "@/utils/runtimeBackend";

export type ProviderRuntimeFilters = Required<ProviderRuntimeListParams>;

export interface ProviderRuntimeSummaryCard {
  key: string;
  label: string;
  value: number;
}

export interface ProviderRuntimeOption<T extends string> {
  value: T;
  label: string;
}

export interface ProviderRuntimeMetric {
  label: string;
  value: string;
}

export interface ProviderRuntimeBackendDisplayRow {
  key: RuntimeStateBackendScope;
  label: string;
  configured: string;
  effective: string;
  changed: boolean;
}

export interface ProviderRuntimeDataApi {
  getProviderRuntimeSnapshot: (
    params?: ProviderRuntimeListParams,
  ) => Promise<ProviderRuntimeSnapshot>;
}

export type ProviderRuntimeTranslator = (
  key: string,
  params?: Record<string, unknown>,
) => string;
