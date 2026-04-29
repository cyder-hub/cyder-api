import test from "node:test";
import assert from "node:assert/strict";

import {
  buildEmptyDashboard,
  buildRuntimeStateBackendRows,
  getUnstableProviders,
  hasCostHotspots,
} from "../src/pages/dashboardViewModel.js";

test("buildEmptyDashboard returns stable zero-state dashboard data", () => {
  const dashboard = buildEmptyDashboard();

  assert.equal(dashboard.today.request_count, 0);
  assert.equal(dashboard.today.success_rate, null);
  assert.equal(dashboard.runtime.window, "1h");
  assert.equal(dashboard.runtime_state_backend.runtime_effective_backend, "memory");
  assert.equal(dashboard.runtime_state_backend.runtime_shared, false);
  assert.deepEqual(dashboard.alerts.open_providers, []);
  assert.deepEqual(dashboard.alerts.top_cost_models, []);
});

test("buildRuntimeStateBackendRows preserves catalog configured and effective backends", () => {
  const rows = buildRuntimeStateBackendRows({
    deployment_mode: "single_instance",
    catalog_cache_backend: "memory",
    catalog_cache_configured_backend: "redis",
    catalog_cache_effective_backend: "memory",
    catalog_cache_fallback_reason: "redis_config_missing",
    runtime_configured_backend: "memory",
    runtime_effective_backend: "memory",
    runtime_shared: false,
    runtime_degraded: false,
    fallback_reason: null,
    last_error: null,
    last_checked_at: 0,
  });

  assert.deepEqual(rows, [
    {
      key: "runtime",
      configured: "memory",
      effective: "memory",
      fallback_reason: null,
      changed: false,
    },
    {
      key: "catalog",
      configured: "redis",
      effective: "memory",
      fallback_reason: "redis_config_missing",
      changed: true,
    },
  ]);
});

test("getUnstableProviders merges open and half-open providers in error order", () => {
  const alerts = {
    open_providers: [
      { provider_id: 3, error_count: 8, runtime_level: "open" },
      { provider_id: 1, error_count: 8, runtime_level: "open" },
    ],
    half_open_providers: [{ provider_id: 2, error_count: 3, runtime_level: "half_open" }],
    degraded_providers: [],
    top_error_providers: [],
    top_cost_providers: [],
    top_cost_models: [],
  };

  const items = getUnstableProviders(alerts);

  assert.deepEqual(
    items.map((item) => [item.provider_id, item.runtime_level]),
    [
      [1, "open"],
      [3, "open"],
      [2, "half_open"],
    ],
  );
});

test("hasCostHotspots is true when either provider or model cost alerts exist", () => {
  assert.equal(
    hasCostHotspots({
      top_cost_providers: [{ provider_id: 1 }],
      top_cost_models: [],
    }),
    true,
  );
  assert.equal(
    hasCostHotspots({
      top_cost_providers: [],
      top_cost_models: [{ model_id: 10 }],
    }),
    true,
  );
  assert.equal(
    hasCostHotspots({
      top_cost_providers: [],
      top_cost_models: [],
    }),
    false,
  );
});
