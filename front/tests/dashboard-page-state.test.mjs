import test from "node:test";
import assert from "node:assert/strict";

import {
  buildEmptyDashboard,
  buildEmptyDashboardOperationsSection,
  useDashboardData,
} from "../src/pages/dashboard/composables/useDashboardData.ts";
import { useDashboardOperations } from "../src/pages/dashboard/composables/useDashboardOperations.ts";

function createApiMock(overrides = {}) {
  return {
    getSystemDashboard: overrides.getSystemDashboard,
  };
}

function buildOperationsSection(overrides = {}) {
  return {
    ...buildEmptyDashboardOperationsSection(),
    operational_signals: {
      ...buildEmptyDashboardOperationsSection().operational_signals,
      open_providers: [
        {
          provider_id: 2,
          provider_key: "open-two",
          provider_name: "Open Two",
          runtime_level: "open",
          request_count: 9,
          error_count: 5,
          success_rate: 0.44,
          avg_total_latency_ms: 900,
          last_error_at: 1700000000000,
          last_error_summary: "boom",
        },
      ],
      half_open_providers: [
        {
          provider_id: 1,
          provider_key: "recovering-one",
          provider_name: "Recovering One",
          runtime_level: "half_open",
          request_count: 4,
          error_count: 6,
          success_rate: 0.25,
          avg_total_latency_ms: 600,
          last_error_at: 1700000001000,
          last_error_summary: "retrying",
        },
      ],
      top_cost_providers: [
        {
          provider_id: 9,
          provider_key: "costly",
          provider_name: "Costly",
          request_count: 10,
          success_rate: 0.8,
          avg_total_latency_ms: 1200,
          total_cost: { USD: 4560000000 },
        },
      ],
      ...overrides.operational_signals,
    },
    top_providers: overrides.top_providers || [
      {
        provider_id: 9,
        provider_key: "costly",
        provider_name: "Costly",
        request_count: 10,
        success_count: 8,
        error_count: 2,
        success_rate: 0.8,
        total_cost: { USD: 4560000000 },
        avg_total_latency_ms: 1200,
      },
    ],
    top_models: overrides.top_models || [
      {
        provider_id: 9,
        provider_key: "costly",
        model_id: 7,
        model_name: "gpt-test",
        real_model_name: null,
        request_count: 10,
        total_tokens: 3000,
        total_cost: { USD: 2000000000 },
      },
    ],
  };
}

function buildDashboardResponse(overrides = {}) {
  const empty = buildEmptyDashboard();
  const operationsSection = buildOperationsSection(overrides);
  return {
    ...empty,
    overview: {
      ...empty.overview,
      provider_count: 5,
      enabled_provider_count: 4,
      ...overrides.overview,
    },
    today: {
      ...empty.today,
      request_count: 42,
      success_count: 40,
      error_count: 2,
      total_cost: { USD: 1230000000 },
      active_provider_count: 3,
      active_model_count: 7,
      active_api_key_count: 2,
      ...overrides.today,
    },
    runtime: {
      ...empty.runtime,
      healthy_count: 3,
      open_count: 1,
      degraded_count: 2,
      ...overrides.runtime,
    },
    runtime_state_backend: {
      ...empty.runtime_state_backend,
      ...overrides.runtime_state_backend,
    },
    operational_signals: operationsSection.operational_signals,
    top_providers: operationsSection.top_providers,
    top_models: operationsSection.top_models,
  };
}

test("dashboard page state loads all sections and derives operational state", async () => {
  const state = useDashboardData({
    api: createApiMock({
      getSystemDashboard: async () => buildDashboardResponse(),
    }),
    getUnknownErrorMessage: () => "unknown",
  });

  await state.fetchDashboard();
  const operations = useDashboardOperations(state.operationsSection);

  assert.equal(state.kpiError.value, null);
  assert.equal(state.resourcesError.value, null);
  assert.equal(state.operationsError.value, null);
  assert.equal(state.kpiSection.value.today.request_count, 42);
  assert.equal(state.resourcesSection.value.overview.provider_count, 5);
  assert.equal(
    state.resourcesSection.value.runtime_state_backend.runtime_effective_backend,
    "memory",
  );
  assert.equal(state.operationsSection.value.top_providers.length, 1);
  assert.equal(operations.showCostHotspots.value, true);
  assert.deepEqual(
    operations.unstableProviders.value.map((item) => [item.provider_id, item.runtime_level]),
    [
      [1, "half_open"],
      [2, "open"],
    ],
  );
  assert.equal(state.isRefreshing.value, false);
});

test("dashboard page state preserves catalog configured effective fallback status", async () => {
  const state = useDashboardData({
    api: createApiMock({
      getSystemDashboard: async () =>
        buildDashboardResponse({
          runtime_state_backend: {
            catalog_cache_backend: "memory",
            catalog_cache_configured_backend: "redis",
            catalog_cache_effective_backend: "memory",
            catalog_cache_fallback_reason: "redis_config_missing",
          },
        }),
    }),
  });

  await state.fetchDashboard();

  const backend = state.resourcesSection.value.runtime_state_backend;
  assert.equal(backend.catalog_cache_backend, "memory");
  assert.equal(backend.catalog_cache_configured_backend, "redis");
  assert.equal(backend.catalog_cache_effective_backend, "memory");
  assert.equal(backend.catalog_cache_fallback_reason, "redis_config_missing");
});

test("dashboard runtime backend presents redis as restart persistence, not deployment sharing", async () => {
  const state = useDashboardData({
    api: createApiMock({
      getSystemDashboard: async () =>
        buildDashboardResponse({
          runtime_state_backend: {
            runtime_configured_backend: "redis",
            runtime_effective_backend: "redis",
          },
        }),
    }),
  });

  await state.fetchDashboard();

  assert.equal(
    state.runtimeBackendBadgeLabel.value,
    "dashboard.runtimeState.backend.redis",
  );
  assert.equal(state.runtimeBackendDetail.value, "dashboard.runtimeState.redisHint");
});

test("dashboard page state preserves stable empty sections without errors", async () => {
  const state = useDashboardData({
    api: createApiMock({
      getSystemDashboard: async () => buildEmptyDashboard(),
    }),
  });

  await state.fetchDashboard();
  const operations = useDashboardOperations(state.operationsSection);

  assert.equal(state.kpiSection.value.today.request_count, 0);
  assert.equal(state.resourcesSection.value.overview.provider_count, 0);
  assert.deepEqual(
    state.operationsSection.value.operational_signals.top_error_providers,
    [],
  );
  assert.equal(operations.showCostHotspots.value, false);
  assert.deepEqual(operations.unstableProviders.value, []);
});

test("dashboard page state degrades the failed snapshot as a unit", async () => {
  const state = useDashboardData({
    api: createApiMock({
      getSystemDashboard: async () => {
        throw new Error("dashboard failed");
      },
    }),
    getUnknownErrorMessage: () => "unknown",
  });

  await state.fetchDashboard();
  const operations = useDashboardOperations(state.operationsSection);

  assert.equal(state.kpiError.value, "dashboard failed");
  assert.equal(state.operationsError.value, "dashboard failed");
  assert.equal(state.resourcesError.value, "dashboard failed");
  assert.equal(state.kpiSection.value.today.request_count, 0);
  assert.equal(state.resourcesSection.value.overview.provider_count, 0);
  assert.equal(state.operationsSection.value.top_models.length, 0);
  assert.equal(operations.showCostHotspots.value, false);
});

test("dashboard page state clears stale snapshot errors after a successful refresh", async () => {
  let callCount = 0;
  const state = useDashboardData({
    api: createApiMock({
      getSystemDashboard: async () => {
        callCount += 1;
        if (callCount === 1) {
          throw new Error("dashboard failed");
        }
        return buildDashboardResponse({
          operational_signals: {
            open_providers: [],
            half_open_providers: [],
            top_cost_providers: [],
            top_cost_models: [],
          },
          top_providers: [],
          top_models: [],
        });
      },
    }),
  });

  await state.fetchDashboard();
  assert.equal(state.operationsError.value, "dashboard failed");
  assert.equal(state.operationsSection.value.top_providers.length, 0);

  await state.fetchDashboard();
  const operations = useDashboardOperations(state.operationsSection);

  assert.equal(state.operationsError.value, null);
  assert.equal(state.operationsSection.value.top_providers.length, 0);
  assert.equal(operations.showCostHotspots.value, false);
  assert.deepEqual(operations.unstableProviders.value, []);
});
