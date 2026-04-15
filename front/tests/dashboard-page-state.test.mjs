import test from "node:test";
import assert from "node:assert/strict";

import { createDashboardPageState } from "../src/pages/dashboardPageState.js";
import {
  buildEmptyDashboardAlertsSection,
  buildEmptyDashboardKpiSection,
  buildEmptyDashboardResourcesSection,
} from "../src/pages/dashboardViewModel.js";

function createApiMock(overrides = {}) {
  return {
    getSystemDashboardKpi: overrides.getSystemDashboardKpi,
    getSystemDashboardResources: overrides.getSystemDashboardResources,
    getSystemDashboardAlerts: overrides.getSystemDashboardAlerts,
  };
}

function buildKpiSection(overrides = {}) {
  return {
    ...buildEmptyDashboardKpiSection(),
    today: {
      ...buildEmptyDashboardKpiSection().today,
      request_count: 42,
      success_count: 40,
      error_count: 2,
      total_cost: { USD: 1230000000 },
      ...overrides.today,
    },
    runtime: {
      ...buildEmptyDashboardKpiSection().runtime,
      open_count: 1,
      degraded_count: 2,
      ...overrides.runtime,
    },
  };
}

function buildResourcesSection(overrides = {}) {
  return {
    ...buildEmptyDashboardResourcesSection(),
    overview: {
      ...buildEmptyDashboardResourcesSection().overview,
      provider_count: 5,
      enabled_provider_count: 4,
      ...overrides.overview,
    },
    today: {
      ...buildEmptyDashboardResourcesSection().today,
      active_provider_count: 3,
      active_model_count: 7,
      active_system_api_key_count: 2,
      ...overrides.today,
    },
    runtime: {
      ...buildEmptyDashboardResourcesSection().runtime,
      healthy_count: 3,
      ...overrides.runtime,
    },
  };
}

function buildAlertsSection(overrides = {}) {
  return {
    ...buildEmptyDashboardAlertsSection(),
    alerts: {
      ...buildEmptyDashboardAlertsSection().alerts,
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
      ...overrides.alerts,
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

test("dashboard page state loads all sections and derives alert-focused state", async () => {
  const state = createDashboardPageState({
    api: createApiMock({
      getSystemDashboardKpi: async () => buildKpiSection(),
      getSystemDashboardResources: async () => buildResourcesSection(),
      getSystemDashboardAlerts: async () => buildAlertsSection(),
    }),
    getUnknownErrorMessage: () => "unknown",
  });

  await state.fetchDashboard();

  assert.equal(state.kpiError.value, null);
  assert.equal(state.resourcesError.value, null);
  assert.equal(state.alertsError.value, null);
  assert.equal(state.kpiSection.value.today.request_count, 42);
  assert.equal(state.resourcesSection.value.overview.provider_count, 5);
  assert.equal(state.alertsSection.value.top_providers.length, 1);
  assert.equal(state.showCostHotspots.value, true);
  assert.deepEqual(
    state.unstableProviders.value.map((item) => [item.provider_id, item.runtime_level]),
    [
      [1, "half_open"],
      [2, "open"],
    ],
  );
  assert.equal(state.isRefreshing.value, false);
});

test("dashboard page state preserves stable empty sections without errors", async () => {
  const state = createDashboardPageState({
    api: createApiMock({
      getSystemDashboardKpi: async () => buildEmptyDashboardKpiSection(),
      getSystemDashboardResources: async () => buildEmptyDashboardResourcesSection(),
      getSystemDashboardAlerts: async () => buildEmptyDashboardAlertsSection(),
    }),
  });

  await state.fetchDashboard();

  assert.equal(state.kpiSection.value.today.request_count, 0);
  assert.equal(state.resourcesSection.value.overview.provider_count, 0);
  assert.deepEqual(state.alertsSection.value.alerts.top_error_providers, []);
  assert.equal(state.showCostHotspots.value, false);
  assert.deepEqual(state.unstableProviders.value, []);
});

test("dashboard page state degrades only the failed section", async () => {
  const state = createDashboardPageState({
    api: createApiMock({
      getSystemDashboardKpi: async () => buildKpiSection({ today: { request_count: 8 } }),
      getSystemDashboardResources: async () => {
        throw new Error("resources failed");
      },
      getSystemDashboardAlerts: async () => buildAlertsSection({
        alerts: { top_cost_providers: [], top_cost_models: [] },
      }),
    }),
    getUnknownErrorMessage: () => "unknown",
  });

  await state.fetchDashboard();

  assert.equal(state.kpiError.value, null);
  assert.equal(state.alertsError.value, null);
  assert.equal(state.resourcesError.value, "resources failed");
  assert.equal(state.kpiSection.value.today.request_count, 8);
  assert.equal(state.resourcesSection.value.overview.provider_count, 0);
  assert.equal(state.alertsSection.value.top_models.length, 1);
  assert.equal(state.showCostHotspots.value, false);
});

test("dashboard page state clears stale section errors after a successful refresh", async () => {
  let alertsCallCount = 0;
  const state = createDashboardPageState({
    api: createApiMock({
      getSystemDashboardKpi: async () => buildKpiSection(),
      getSystemDashboardResources: async () => buildResourcesSection(),
      getSystemDashboardAlerts: async () => {
        alertsCallCount += 1;
        if (alertsCallCount === 1) {
          throw new Error("alerts failed");
        }
        return buildAlertsSection({
          alerts: {
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
  assert.equal(state.alertsError.value, "alerts failed");
  assert.equal(state.alertsSection.value.top_providers.length, 0);

  await state.fetchDashboard();

  assert.equal(state.alertsError.value, null);
  assert.equal(state.alertsSection.value.top_providers.length, 0);
  assert.equal(state.showCostHotspots.value, false);
  assert.deepEqual(state.unstableProviders.value, []);
});
