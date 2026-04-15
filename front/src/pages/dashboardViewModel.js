export function buildEmptyDashboard() {
  return {
    overview: {
      provider_count: 0,
      enabled_provider_count: 0,
      model_count: 0,
      enabled_model_count: 0,
      provider_key_count: 0,
      enabled_provider_key_count: 0,
      system_api_key_count: 0,
      enabled_system_api_key_count: 0,
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
      active_system_api_key_count: 0,
    },
    runtime: {
      window: "1h",
      healthy_count: 0,
      degraded_count: 0,
      half_open_count: 0,
      open_count: 0,
      no_traffic_count: 0,
    },
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
