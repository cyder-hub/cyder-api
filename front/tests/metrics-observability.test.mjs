import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";

import {
  usageMetricSampleCount,
  usageMetricValue,
  weightedUsageAverage,
} from "../src/components/usageMetrics.ts";

const usageItem = (overrides = {}) => ({
  total_input_tokens: 0,
  total_output_tokens: 0,
  total_reasoning_tokens: 0,
  total_tokens: 0,
  request_count: 0,
  success_count: 0,
  error_count: 0,
  success_rate: null,
  avg_time_to_first_response_body_ms: null,
  time_to_first_response_body_sample_count: 0,
  avg_ttft_ms: null,
  ttft_sample_count: 0,
  avg_total_latency_ms: null,
  total_latency_sample_count: 0,
  total_cost: {},
  ...overrides,
});

test("usage latency aggregation weights backend averages by sample count", () => {
  const items = [
    usageItem({
      avg_time_to_first_response_body_ms: 100,
      time_to_first_response_body_sample_count: 1,
      avg_ttft_ms: 80,
      ttft_sample_count: 1,
      avg_total_latency_ms: 250,
      total_latency_sample_count: 1,
    }),
    usageItem({
      avg_time_to_first_response_body_ms: 300,
      time_to_first_response_body_sample_count: 3,
      avg_ttft_ms: 600,
      ttft_sample_count: 3,
      avg_total_latency_ms: 300,
      total_latency_sample_count: 3,
    }),
  ];

  assert.deepEqual(weightedUsageAverage(items, "avg_time_to_first_response_body"), {
    average: 250,
    sampleCount: 4,
  });
  assert.deepEqual(weightedUsageAverage(items, "avg_ttft"), {
    average: 470,
    sampleCount: 4,
  });
  assert.deepEqual(weightedUsageAverage(items, "avg_latency"), {
    average: 287.5,
    sampleCount: 4,
  });
});

test("zero-duration samples remain distinct from an unobserved metric", () => {
  const zero = usageItem({
    avg_ttft_ms: 0,
    ttft_sample_count: 1,
  });
  const missing = usageItem();

  assert.equal(usageMetricValue(zero, "avg_ttft"), 0);
  assert.equal(usageMetricSampleCount(zero, "avg_ttft"), 1);
  assert.equal(usageMetricValue(missing, "avg_ttft"), null);
  assert.equal(usageMetricSampleCount(missing, "avg_ttft"), 0);
  assert.deepEqual(weightedUsageAverage([missing], "avg_ttft"), {
    average: null,
    sampleCount: 0,
  });
});

test("dashboard, usage chart, and provider runtime expose the three latency contracts", () => {
  const dashboard = fs.readFileSync(
    new URL("../src/pages/dashboard/composables/useDashboardData.ts", import.meta.url),
    "utf8",
  );
  const usageChart = fs.readFileSync(
    new URL("../src/components/UsageChart.vue", import.meta.url),
    "utf8",
  );
  const providerRuntime = fs.readFileSync(
    new URL("../src/pages/provider-runtime/ProviderRuntimePage.vue", import.meta.url),
    "utf8",
  );
  const providerTable = fs.readFileSync(
    new URL("../src/pages/provider-runtime/components/ProviderRuntimeTable.vue", import.meta.url),
    "utf8",
  );

  for (const field of [
    "avg_time_to_first_response_body_ms",
    "avg_ttft_ms",
    "avg_total_latency_ms",
  ]) {
    assert.match(dashboard, new RegExp(field));
    assert.match(providerRuntime, new RegExp(field));
    assert.match(providerTable, new RegExp(field));
  }
  assert.match(dashboard, /common\.noSamples/);
  assert.match(dashboard, /dashboard\.kpi\.latencyTooltip/);
  assert.match(usageChart, /weightedUsageAverage/);
  assert.match(usageChart, /avg_time_to_first_response_body/);
  assert.match(usageChart, /avg_ttft/);
  assert.match(usageChart, /row\.value\[2\]/);
  assert.match(providerRuntime, /time_to_first_response_body/);
  assert.match(providerRuntime, /ttft/);
  assert.match(providerTable, /formatLatencyCoverage/);
  assert.match(usageChart, /v-if="isLoading"/);
  assert.match(usageChart, /v-else-if="error"/);
  assert.match(providerRuntime, /v-else-if="!items\.length"/);
});
