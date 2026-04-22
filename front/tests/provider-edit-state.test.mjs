import test from "node:test";
import assert from "node:assert/strict";

import {
  buildProviderUpdatePayload,
  buildProviderBootstrapPayload,
  buildProviderBootstrapPreview,
  createProviderBootstrapFormState,
  createEmptyEditingProviderData,
  hydrateEditingProviderDataFromBootstrap,
  normalizeBootstrapCheckResult,
  syncProviderBootstrapFormState,
} from "../src/pages/providerEditState.ts";

test("buildProviderBootstrapPayload trims values and keeps bootstrap flags", () => {
  const payload = buildProviderBootstrapPayload(
    {
      provider_type: "  VERTEX  ",
      endpoint: "  https://api.example.com/v1  ",
      api_key: "  secret-key  ",
      model_name: "  gemini-1.5-pro  ",
      api_key_description: "  first key  ",
      use_proxy: true,
      provider_name: "  Example Cloud  ",
      provider_key: "  example-cloud  ",
      real_model_name: "  gemini-1.5-pro-latest  ",
    },
    true,
  );

  assert.deepEqual(payload, {
    endpoint: "https://api.example.com/v1",
    api_key: "secret-key",
    model_name: "gemini-1.5-pro",
    provider_type: "VERTEX",
    name: "Example Cloud",
    key: "example-cloud",
    real_model_name: "gemini-1.5-pro-latest",
    use_proxy: true,
    save_and_test: true,
    api_key_description: "first key",
  });
});

test("buildProviderBootstrapPreview falls back to provider type and endpoint host", () => {
  const preview = buildProviderBootstrapPreview({
    provider_type: "openai",
    endpoint: "https://api.example.com/v1",
    api_key: "",
    model_name: "gpt-4o",
    api_key_description: "",
    use_proxy: false,
    provider_name: "",
    provider_key: "",
  });

  assert.deepEqual(preview, {
    provider_name: "Openai",
    provider_key: "openai-api-example-com",
  });
});

test("hydrateEditingProviderDataFromBootstrap merges bootstrap response and preserves model enablement", () => {
  const editingData = createEmptyEditingProviderData();
  editingData.name = "Old Provider";
  editingData.provider_key = "old-provider";
  editingData.provider_type = "OPENAI";
  editingData.endpoint = "https://old.example.com";
  editingData.use_proxy = false;
  editingData.models.push({
    id: 1,
    model_name: "legacy-model",
    real_model_name: "legacy-real",
    is_enabled: true,
    isEditing: false,
    checkStatus: "unchecked",
  });
  editingData.provider_keys.push({
    id: 11,
    api_key: "old-key",
    description: "legacy",
    isEditing: false,
    checkStatus: "unchecked",
  });

  const hydrated = hydrateEditingProviderDataFromBootstrap(editingData, {
    provider: {
      id: 99,
      name: "Bootstrapped Provider",
      provider_key: "boot-key",
      provider_type: "VERTEX",
      endpoint: "https://bootstrap.example.com",
      use_proxy: true,
    },
    created_key: {
      id: 12,
      api_key: "sk-bootstrap",
      description: "bootstrap key",
    },
    created_model: {
      id: 13,
      model_name: "gemini-1.5-pro",
      real_model_name: "gemini-1.5-pro-latest",
      is_enabled: false,
    },
    provider_name: "Bootstrapped Provider",
    provider_key: "boot-key",
    check_result: { ok: true },
  });

  assert.equal(hydrated.id, 99);
  assert.equal(hydrated.name, "Bootstrapped Provider");
  assert.equal(hydrated.provider_key, "boot-key");
  assert.equal(hydrated.provider_type, "VERTEX");
  assert.equal(hydrated.endpoint, "https://bootstrap.example.com");
  assert.equal(hydrated.use_proxy, true);
  assert.equal(hydrated.provider_keys.length, 2);
  assert.deepEqual(hydrated.provider_keys[1], {
    id: 12,
    api_key: "sk-bootstrap",
    description: "bootstrap key",
    isEditing: false,
    checkStatus: "unchecked",
  });
  assert.equal(hydrated.models.length, 2);
  assert.deepEqual(hydrated.models[1], {
    id: 13,
    model_name: "gemini-1.5-pro",
    real_model_name: "gemini-1.5-pro-latest",
    is_enabled: false,
    supports_streaming: true,
    supports_tools: true,
    supports_reasoning: true,
    supports_image_input: true,
    supports_embeddings: true,
    supports_rerank: true,
    isEditing: false,
    checkStatus: "unchecked",
  });
});

test("normalizeBootstrapCheckResult supports mixed bootstrap responses", () => {
  assert.deepEqual(normalizeBootstrapCheckResult(true), {
    ok: true,
    message: "",
  });
  assert.deepEqual(normalizeBootstrapCheckResult("boom"), {
    ok: false,
    message: "boom",
  });
  assert.deepEqual(normalizeBootstrapCheckResult(["timeout", "dns"]), {
    ok: false,
    message: "timeout, dns",
  });
});

test("syncProviderBootstrapFormState copies saved provider identity into the edit form", () => {
  const form = createProviderBootstrapFormState();
  const editingData = createEmptyEditingProviderData();
  editingData.id = 7;
  editingData.name = "Saved Provider";
  editingData.provider_key = "saved-provider";
  editingData.provider_type = "ANTHROPIC";
  editingData.endpoint = "https://anthropic.example.com/v1";
  editingData.use_proxy = true;

  syncProviderBootstrapFormState(form, editingData);

  assert.deepEqual(form, {
    provider_type: "ANTHROPIC",
    endpoint: "https://anthropic.example.com/v1",
    api_key: "",
    model_name: "",
    api_key_description: "",
    use_proxy: true,
    provider_name: "Saved Provider",
    provider_key: "saved-provider",
  });
});

test("buildProviderUpdatePayload keeps the existing provider key immutable", () => {
  const editingData = createEmptyEditingProviderData();
  editingData.id = 11;
  editingData.name = "Existing Provider";
  editingData.provider_key = "existing-provider";
  editingData.provider_type = "OPENAI";
  editingData.endpoint = "https://old.example.com/v1";
  editingData.use_proxy = false;

  const payload = buildProviderUpdatePayload(editingData, {
    provider_type: "RESPONSES",
    endpoint: " https://new.example.com/v1 ",
    api_key: "",
    model_name: "",
    api_key_description: "",
    use_proxy: true,
    provider_name: " Updated Name ",
    provider_key: "should-not-be-used",
  });

  assert.deepEqual(payload, {
    key: "existing-provider",
    name: "Updated Name",
    endpoint: "https://new.example.com/v1",
    use_proxy: true,
    provider_type: "RESPONSES",
    omit_config: null,
    api_keys: [],
  });
});
