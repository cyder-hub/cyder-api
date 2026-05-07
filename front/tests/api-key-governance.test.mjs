import test from "node:test";
import assert from "node:assert/strict";

import {
  buildApiKeySummaryCards,
  buildBudgetPayload,
  buildRuntimeRejectionView,
  emptyRuntimeSnapshot,
  getApiKeyLifecycle,
  hasApiKeyGovernanceLimits,
  maskedApiKey,
} from "../src/pages/api-key/composables/apiKeyViewModel.ts";

const t = (key) => key;

function apiKey(overrides = {}) {
  return {
    id: 1,
    key_prefix: "ck_live",
    key_last4: "1234",
    name: "ops",
    description: null,
    default_action: "ALLOW",
    is_enabled: true,
    expires_at: null,
    rate_limit_rpm: null,
    max_concurrent_requests: null,
    quota_daily_requests: null,
    quota_daily_tokens: null,
    quota_monthly_tokens: null,
    budget_daily_nanos: null,
    budget_daily_currency: null,
    budget_monthly_nanos: null,
    budget_monthly_currency: null,
    created_at: 0,
    updated_at: 0,
    ...overrides,
  };
}

test("api key lifecycle and summary cards reflect governance state", () => {
  const now = Date.UTC(2026, 4, 8);
  const expiring = apiKey({
    id: 2,
    expires_at: now + 3 * 24 * 60 * 60 * 1000,
    rate_limit_rpm: 120,
  });
  const disabled = apiKey({ id: 3, is_enabled: false });

  assert.equal(getApiKeyLifecycle(expiring, now), "expiringSoon");
  assert.equal(getApiKeyLifecycle(disabled, now), "disabled");
  assert.equal(hasApiKeyGovernanceLimits(expiring), true);
  assert.equal(maskedApiKey(expiring), "ck_live...1234");

  const summary = buildApiKeySummaryCards(
    [apiKey(), expiring, disabled],
    [
      { ...emptyRuntimeSnapshot(1), current_concurrency: 2 },
      { ...emptyRuntimeSnapshot(2), current_concurrency: 3 },
    ],
    t,
    now,
  );

  assert.deepEqual(
    summary.map((item) => [item.key, item.value]),
    [
      ["total", 3],
      ["enabled", 2],
      ["governed", 1],
      ["concurrency", 5],
      ["expiring", 1],
    ],
  );
});

test("runtime rejection view reports the first active governance limit", () => {
  const key = apiKey({
    max_concurrent_requests: 2,
    rate_limit_rpm: 10,
  });
  const runtime = {
    ...emptyRuntimeSnapshot(key.id),
    current_concurrency: 2,
    current_minute_request_count: 10,
  };

  assert.deepEqual(buildRuntimeRejectionView(key, runtime, t), {
    reason: "concurrency",
    label: "apiKeyPage.runtimeRejection.concurrency",
    tone: "warning",
  });

  assert.equal(
    buildRuntimeRejectionView(apiKey({ is_enabled: false }), runtime, t).reason,
    "disabled",
  );
});

test("budget payload converts major currency units and requires currency", () => {
  assert.deepEqual(buildBudgetPayload("", "", "Daily", t), {
    nanos: null,
    currency: null,
  });
  assert.deepEqual(buildBudgetPayload("1.25", "usd", "Daily", t), {
    nanos: 125_000_000_000,
    currency: "USD",
  });
  assert.throws(
    () => buildBudgetPayload("1.25", "", "Daily", t),
    /apiKeyEditModal\.alert\.budgetCurrencyRequired/,
  );
});
