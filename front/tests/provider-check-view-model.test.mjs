import test from "node:test";
import assert from "node:assert/strict";

import {
  buildCheckOptions,
  buildEnabledApiKeyOptions,
  buildEnabledModelOptions,
  buildEnabledSourceOptions,
  formatCheckSourceEvidence,
  modelAllowsSource,
  resolveAutomaticSource,
} from "../src/pages/provider-edit/composables/providerCheckViewModel.ts";

test("buildCheckOptions keeps selection empty until the user chooses a target", () => {
  const result = buildCheckOptions(["first", "second"], (item, index) => {
    return `${item}-${index}`;
  });

  assert.deepEqual(result, {
    options: [
      { value: 0, label: "#1 first-0" },
      { value: 1, label: "#2 second-1" },
    ],
    defaultSelectedValue: null,
  });
});

test("formatCheckSourceEvidence exposes the exact Source used", () => {
  assert.equal(
    formatCheckSourceEvidence({
      source_id: 42,
      profile_type: "RESPONSES",
    }),
    "RESPONSES · source #42",
  );
});

const source = (id, { enabled = true, defaultSource = false, deleted = null } = {}) => ({
  id,
  provider_id: 1,
  profile_type: id === 1 ? "OPENAI" : "GEMINI",
  endpoint: `https://source-${id}.test`,
  use_proxy: false,
  is_enabled: enabled,
  is_default: defaultSource,
  deleted_at: deleted,
  created_at: 1,
  updated_at: 1,
});

test("resolveAutomaticSource covers zero, one, provider default, and prompt cases", () => {
  assert.deepEqual(resolveAutomaticSource([source(1, { enabled: false })]), {
    status: "none",
    sourceId: null,
  });
  assert.deepEqual(resolveAutomaticSource([source(1)]), {
    status: "selected",
    sourceId: 1,
  });
  assert.deepEqual(
    resolveAutomaticSource([source(1), source(2, { defaultSource: true })]),
    { status: "selected", sourceId: 2 },
  );
  assert.deepEqual(resolveAutomaticSource([source(1), source(2)]), {
    status: "prompt",
    sourceId: null,
  });
});

test("automatic check options exclude disabled targets while preserving source indexes", () => {
  const models = [
    {
      id: 10,
      model_name: "disabled-model",
      real_model_name: null,
      is_enabled: false,
    },
    {
      id: 11,
      model_name: "enabled-model",
      real_model_name: null,
      is_enabled: true,
    },
  ];
  const keys = [
    {
      id: 20,
      description: "disabled-key",
      key_last4: "0000",
      is_enabled: false,
    },
    {
      id: 21,
      description: "enabled-key",
      key_last4: "1111",
      is_enabled: true,
    },
  ];

  assert.deepEqual(buildEnabledSourceOptions([source(1), source(2, { enabled: false })]), [
    { value: 1, label: "OPENAI · #1" },
  ]);
  assert.deepEqual(
    buildEnabledModelOptions(models, (item) => item.model_name),
    [{ value: 1, label: "#1 enabled-model" }],
  );
  assert.deepEqual(
    buildEnabledApiKeyOptions(keys, (item) => item.description),
    [{ value: 1, label: "#1 enabled-key" }],
  );
});

test("explicit models constrain check Sources and fixed-Source model candidates", () => {
  const sources = [source(1), source(2)];
  const models = [
    {
      id: 10,
      model_name: "explicit-one",
      real_model_name: null,
      is_enabled: true,
      source_config: {
        source_selection_mode: "EXPLICIT",
        bindings: [{ source_id: 2, is_default: true }],
        declared_source_count: 1,
        enabled_source_count: 1,
        model_default_source_id: 2,
        warnings: [],
      },
    },
    {
      id: 11,
      model_name: "inherited",
      real_model_name: null,
      is_enabled: true,
      source_config: {
        source_selection_mode: "INHERIT_ALL",
        bindings: [],
        declared_source_count: 2,
        enabled_source_count: 2,
        model_default_source_id: null,
        warnings: [],
      },
    },
  ];

  assert.equal(modelAllowsSource(models[0], 1), false);
  assert.deepEqual(buildEnabledSourceOptions(sources, models[0]), [
    { value: 2, label: "GEMINI · #2" },
  ]);
  assert.deepEqual(
    buildEnabledModelOptions(models, (item) => item.model_name, 1),
    [{ value: 1, label: "#1 inherited" }],
  );
  assert.deepEqual(resolveAutomaticSource(sources, models[0]), {
    status: "selected",
    sourceId: 2,
  });
});
