import type { RuntimeStateBackendName, RuntimeStateBackendStatus } from "@/services/types";

export type RuntimeStateBackendScope = "runtime" | "catalog";

export interface RuntimeStateBackendRow {
  key: RuntimeStateBackendScope;
  configured: RuntimeStateBackendName;
  effective: RuntimeStateBackendName;
  fallback_reason: string | null;
  changed: boolean;
}

export function buildDefaultRuntimeStateBackendStatus(): RuntimeStateBackendStatus {
  return {
    deployment_mode: "single_instance",
    catalog_cache_backend: "memory",
    catalog_cache_configured_backend: "memory",
    catalog_cache_effective_backend: "memory",
    catalog_cache_fallback_reason: null,
    runtime_configured_backend: "memory",
    runtime_effective_backend: "memory",
    runtime_shared: false,
    runtime_degraded: false,
    fallback_reason: null,
    last_error: null,
    last_checked_at: 0,
  };
}

export function buildRuntimeStateBackendRows(
  status: RuntimeStateBackendStatus,
): RuntimeStateBackendRow[] {
  const catalogConfigured =
    status.catalog_cache_configured_backend ?? status.catalog_cache_backend;
  const catalogEffective =
    status.catalog_cache_effective_backend ?? status.catalog_cache_backend;

  return [
    {
      key: "runtime",
      configured: status.runtime_configured_backend,
      effective: status.runtime_effective_backend,
      fallback_reason: status.fallback_reason,
      changed: status.runtime_configured_backend !== status.runtime_effective_backend,
    },
    {
      key: "catalog",
      configured: catalogConfigured,
      effective: catalogEffective,
      fallback_reason: status.catalog_cache_fallback_reason,
      changed: catalogConfigured !== catalogEffective,
    },
  ];
}
