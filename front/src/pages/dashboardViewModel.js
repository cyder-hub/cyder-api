function buildDefaultRuntimeStateBackendStatus() {
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

export function buildRuntimeStateBackendRows(status) {
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

export function buildEmptyDashboard() {
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

export function buildEmptyDashboardKpiSection() {
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

export function buildEmptyDashboardAlertsSection() {
  const dashboard = buildEmptyDashboard();
  return {
    alerts: dashboard.alerts,
    top_providers: dashboard.top_providers,
    top_models: dashboard.top_models,
  };
}

export function getUnstableProviders(alerts) {
  return [...alerts.open_providers, ...alerts.half_open_providers].sort(
    (left, right) => right.error_count - left.error_count || left.provider_id - right.provider_id,
  );
}

export function hasCostHotspots(alerts) {
  return alerts.top_cost_providers.length > 0 || alerts.top_cost_models.length > 0;
}
