import test from "node:test";
import assert from "node:assert/strict";

import { buildModelPageState } from "../src/pages/modelViewModel.ts";

const models = [
  {
    id: 2,
    provider_id: 2,
    provider_key: "anthropic",
    provider_name: "Anthropic",
    model_name: "claude-3-5-sonnet",
    real_model_name: null,
    is_enabled: true,
  },
  {
    id: 1,
    provider_id: 1,
    provider_key: "openai",
    provider_name: "OpenAI",
    model_name: "gpt-4o",
    real_model_name: "gpt-4o-2024-08-06",
    is_enabled: true,
  },
];

test("model page state keeps page chrome visible when search has no matches", () => {
  const state = buildModelPageState(models, "does-not-exist");

  assert.equal(state.isPageEmpty, false);
  assert.equal(state.isSearchEmpty, true);
  assert.deepEqual(state.filteredItems, []);
});

test("model page state sorts models and matches provider and real model fields", () => {
  const state = buildModelPageState(models, "2024-08");

  assert.equal(state.isPageEmpty, false);
  assert.equal(state.isSearchEmpty, false);
  assert.deepEqual(
    state.filteredItems.map((item) => item.id),
    [1],
  );

  const unfilteredState = buildModelPageState(models, "");
  assert.deepEqual(
    unfilteredState.filteredItems.map((item) => item.id),
    [2, 1],
  );
});
