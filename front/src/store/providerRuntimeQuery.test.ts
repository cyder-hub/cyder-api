import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_PROVIDER_RUNTIME_FILTERS,
  buildProviderRuntimeApiParamsFromFilters,
  buildProviderRuntimeFiltersFromQuery,
  buildProviderRuntimeRouteQueryFromFilters,
} from "./providerRuntimeQuery.ts";

test("buildProviderRuntimeFiltersFromQuery normalizes invalid values", () => {
  const filters = buildProviderRuntimeFiltersFromQuery({
    window: "2h",
    status: "bad",
    search: "  openai  ",
    sort: "latency",
    direction: "sideways",
    only_enabled: "0",
  });

  assert.deepEqual(filters, {
    ...DEFAULT_PROVIDER_RUNTIME_FILTERS,
    search: "openai",
    sort: "latency",
    only_enabled: false,
  });
});

test("buildProviderRuntimeRouteQueryFromFilters omits defaults", () => {
  assert.deepEqual(
    buildProviderRuntimeRouteQueryFromFilters(DEFAULT_PROVIDER_RUNTIME_FILTERS),
    {},
  );

  assert.deepEqual(
    buildProviderRuntimeRouteQueryFromFilters({
      ...DEFAULT_PROVIDER_RUNTIME_FILTERS,
      window: "24h",
      status: "degraded",
      search: "gemini",
      only_enabled: false,
    }),
    {
      window: "24h",
      status: "degraded",
      search: "gemini",
      only_enabled: "false",
    },
  );
});

test("buildProviderRuntimeApiParamsFromFilters removes empty search", () => {
  assert.deepEqual(
    buildProviderRuntimeApiParamsFromFilters(DEFAULT_PROVIDER_RUNTIME_FILTERS),
    {
      window: "1h",
      status: "all",
      search: undefined,
      sort: "health",
      direction: "desc",
      only_enabled: true,
    },
  );
});
