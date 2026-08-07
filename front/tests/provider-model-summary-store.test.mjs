import test from "node:test";
import assert from "node:assert/strict";

import {
  buildModelNameById,
  buildModelOptions,
  buildModelsByProviderId,
  buildProviderNameById,
  buildProviderOptions,
  getModelById,
  getProviderById,
} from "../src/store/summaryViewModel.ts";

test("provider summary helpers derive options and lookup maps from summary items", () => {
  const providers = [
    {
      id: 1,
      provider_key: "openai",
      name: "OpenAI",
      is_enabled: true,
      source_count: 1,
      enabled_source_count: 1,
      default_source_id: 101,
      default_source_profile_type: "OPENAI",
    },
    {
      id: 2,
      provider_key: "anthropic",
      name: "Anthropic",
      is_enabled: false,
      source_count: 1,
      enabled_source_count: 1,
      default_source_id: 102,
      default_source_profile_type: "ANTHROPIC",
    },
  ];

  assert.deepEqual(buildProviderOptions(providers), [
    { value: 1, label: "OpenAI (openai)", isEnabled: true },
    { value: 2, label: "Anthropic (anthropic)", isEnabled: false },
  ]);
  assert.equal(buildProviderNameById(providers).get(2), "Anthropic (anthropic)");
  assert.equal(getProviderById(providers, "1")?.name, "OpenAI");
});

test("model summary helpers derive options and provider grouping from summary items", () => {
  const models = [
    {
      id: 10,
      provider_id: 1,
      provider_key: "openai",
      provider_name: "OpenAI",
      model_name: "gpt-4o",
      real_model_name: "gpt-4o-2024-08-06",
      is_enabled: true,
    },
    {
      id: 11,
      provider_id: 1,
      provider_key: "openai",
      provider_name: "OpenAI",
      model_name: "gpt-4o-mini",
      real_model_name: null,
      is_enabled: false,
    },
  ];

  assert.deepEqual(buildModelOptions(models), [
    {
      value: 10,
      label: "openai / gpt-4o",
      providerId: 1,
      providerName: "OpenAI",
      isEnabled: true,
    },
    {
      value: 11,
      label: "openai / gpt-4o-mini",
      providerId: 1,
      providerName: "OpenAI",
      isEnabled: false,
    },
  ]);
  assert.equal(buildModelNameById(models).get(10), "openai / gpt-4o");
  assert.equal(getModelById(models, "11")?.model_name, "gpt-4o-mini");
  assert.equal(buildModelsByProviderId(models).get(1)?.length, 2);
});
