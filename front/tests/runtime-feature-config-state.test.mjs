import test from "node:test";
import assert from "node:assert/strict";

import {
  createRuntimeFeatureDrafts,
  findRuntimeFeatureResponse,
  runtimeFeatureDraftSnapshot,
} from "../src/components/runtime-feature/runtimeFeatureConfigState.ts";

const catalog = {
  features: [
    {
      feature_key: "openai_reasoning_content_repair",
      label: "OpenAI reasoning_content repair",
      description: "Restore observed reasoning content",
      default_enabled: false,
      supported_scope_kinds: ["provider", "model"],
    },
  ],
};

test("runtime feature drafts prefer owner config over inherited effective values", () => {
  assert.deepEqual(
    createRuntimeFeatureDrafts(catalog, {
      owner_kind: "model",
      owner_id: 42,
      features: [
        {
          feature_key: "openai_reasoning_content_repair",
          owner_config: {
            id: 7,
            scope_kind: "model",
            provider_id: null,
            model_id: 42,
            feature_key: "openai_reasoning_content_repair",
            enabled: false,
            created_at: 1,
            updated_at: 1,
          },
          provider_config: {
            id: 6,
            scope_kind: "provider",
            provider_id: 9,
            model_id: null,
            feature_key: "openai_reasoning_content_repair",
            enabled: true,
            created_at: 1,
            updated_at: 1,
          },
          effective_enabled: false,
          effective_source: "model_override",
        },
      ],
    }),
    {
      openai_reasoning_content_repair: false,
    },
  );
});

test("runtime feature drafts use inherited provider value when model override is missing", () => {
  assert.deepEqual(
    createRuntimeFeatureDrafts(catalog, {
      owner_kind: "model",
      owner_id: 42,
      features: [
        {
          feature_key: "openai_reasoning_content_repair",
          owner_config: null,
          provider_config: {
            id: 6,
            scope_kind: "provider",
            provider_id: 9,
            model_id: null,
            feature_key: "openai_reasoning_content_repair",
            enabled: true,
            created_at: 1,
            updated_at: 1,
          },
          effective_enabled: true,
          effective_source: "provider_default",
        },
      ],
    }),
    {
      openai_reasoning_content_repair: true,
    },
  );
});

test("runtime feature snapshots are stable by feature key order", () => {
  assert.equal(
    runtimeFeatureDraftSnapshot({
      z_feature: false,
      a_feature: true,
    }),
    '[["a_feature",true],["z_feature",false]]',
  );
});

test("runtime feature response lookup returns null for unknown feature", () => {
  const response = {
    owner_kind: "provider",
    owner_id: 9,
    features: [
      {
        feature_key: "openai_reasoning_content_repair",
        owner_config: null,
        provider_config: null,
        effective_enabled: false,
        effective_source: "default_false",
      },
    ],
  };

  assert.equal(
    findRuntimeFeatureResponse(response, "openai_reasoning_content_repair")
      ?.effective_source,
    "default_false",
  );
  assert.equal(findRuntimeFeatureResponse(response, "unknown"), null);
});
