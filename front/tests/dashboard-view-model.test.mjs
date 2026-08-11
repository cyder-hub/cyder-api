import test from "node:test";
import assert from "node:assert/strict";

import {
  buildEmptyDashboard,
} from "../src/pages/dashboard/composables/useDashboardData.ts";
import {
  getDegradedProviders,
  hasCostHotspots,
} from "../src/pages/dashboard/composables/useDashboardOperations.ts";
import { buildRuntimeStateBackendRows } from "../src/utils/runtimeBackend.ts";

test("buildEmptyDashboard returns stable zero-state dashboard data", () => {
  const dashboard = buildEmptyDashboard();

  assert.equal(dashboard.today.request_count, 0);
  assert.equal(dashboard.today.success_rate, null);
  assert.equal(dashboard.runtime.window, "1h");
  assert.equal(dashboard.runtime_state_backend.runtime_effective_backend, "memory");
  assert.deepEqual(dashboard.operational_signals.degraded_providers, []);
  assert.deepEqual(dashboard.operational_signals.top_cost_models, []);
});

test("buildRuntimeStateBackendRows preserves catalog configured and effective backends", () => {
  const rows = buildRuntimeStateBackendRows({
    catalog_cache_backend: "memory",
    catalog_cache_configured_backend: "redis",
    catalog_cache_effective_backend: "memory",
    catalog_cache_fallback_reason: "redis_config_missing",
    runtime_configured_backend: "memory",
    runtime_effective_backend: "memory",
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

test("getDegradedProviders sorts observed degradation by error count", () => {
  const signals = {
    degraded_providers: [
      { provider_id: 3, error_count: 8, runtime_level: "degraded" },
      { provider_id: 1, error_count: 8, runtime_level: "degraded" },
      { provider_id: 2, error_count: 3, runtime_level: "degraded" },
    ],
    top_error_providers: [],
    top_cost_providers: [],
    top_cost_models: [],
  };

  const items = getDegradedProviders(signals);

  assert.deepEqual(
    items.map((item) => [item.provider_id, item.runtime_level]),
    [
      [1, "degraded"],
      [3, "degraded"],
      [2, "degraded"],
    ],
  );
});

test("hasCostHotspots is true when either provider or model cost hotspots exist", () => {
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
