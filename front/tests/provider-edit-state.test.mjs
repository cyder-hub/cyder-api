import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const ROOT = new URL("../", import.meta.url);

import {
  buildProviderUpdatePayload,
  buildProviderBootstrapPayload,
  buildProviderBootstrapPreview,
  createProviderBootstrapFormState,
  createEmptyEditingProviderData,
  hydrateEditingProviderDataFromBootstrap,
  normalizeBootstrapCheckResult,
  syncProviderBootstrapFormState,
} from "../src/pages/provider-edit/composables/providerEditState.ts";
import { useProviderCredentialSecretState } from "../src/pages/provider-edit/composables/useProviderCredentialSecretState.ts";

test("buildProviderBootstrapPayload trims values and keeps bootstrap flags", () => {
  const payload = buildProviderBootstrapPayload(
    {
      profile_type: "  VERTEX  ",
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
    upstream_source: {
      profile_type: "VERTEX",
      endpoint: "https://api.example.com/v1",
      use_proxy: true,
    },
    api_key: "secret-key",
    model_name: "gemini-1.5-pro",
    name: "Example Cloud",
    key: "example-cloud",
    real_model_name: "gemini-1.5-pro-latest",
    save_and_test: true,
    api_key_description: "first key",
  });
});

test("buildProviderBootstrapPreview requires explicit provider key", () => {
  const preview = buildProviderBootstrapPreview({
    profile_type: "openai",
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
    provider_key: "",
  });
});

test("hydrateEditingProviderDataFromBootstrap merges bootstrap response and preserves model enablement", () => {
  const editingData = createEmptyEditingProviderData();
  editingData.name = "Old Provider";
  editingData.provider_key = "old-provider";
  editingData.upstream_source.profile_type = "OPENAI";
  editingData.upstream_source.endpoint = "https://old.example.com";
  editingData.upstream_source.use_proxy = false;
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
    provider_id: 99,
    description: "legacy",
    key_prefix: "old-",
    key_last4: "-key",
    is_enabled: true,
    created_at: 1,
    updated_at: 1,
    checkStatus: "unchecked",
  });

  const hydrated = hydrateEditingProviderDataFromBootstrap(editingData, {
    provider: {
      id: 99,
      name: "Bootstrapped Provider",
      provider_key: "boot-key",
      is_enabled: true,
      deleted_at: null,
      created_at: 1,
      updated_at: 2,
      provider_api_key_mode: "ROUND_ROBIN",
      upstream_source: {
        id: 100,
        provider_id: 99,
        source_key: "primary",
        profile_type: "VERTEX",
        endpoint: "https://bootstrap.example.com",
        use_proxy: true,
        deleted_at: null,
        created_at: 1,
        updated_at: 2,
      },
    },
    created_key: {
      id: 12,
      provider_id: 99,
      description: "bootstrap key",
      key_prefix: "sk-b",
      key_last4: "trap",
      is_enabled: true,
      created_at: 2,
      updated_at: 2,
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
  assert.deepEqual(hydrated.upstream_source, {
    id: 100,
    provider_id: 99,
    source_key: "primary",
    profile_type: "VERTEX",
    endpoint: "https://bootstrap.example.com",
    use_proxy: true,
  });
  assert.equal(hydrated.provider_keys.length, 2);
  assert.deepEqual(hydrated.provider_keys[1], {
    id: 12,
    provider_id: 99,
    description: "bootstrap key",
    key_prefix: "sk-b",
    key_last4: "trap",
    is_enabled: true,
    created_at: 2,
    updated_at: 2,
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
  editingData.upstream_source.profile_type = "ANTHROPIC";
  editingData.upstream_source.endpoint = "https://anthropic.example.com/v1";
  editingData.upstream_source.use_proxy = true;

  syncProviderBootstrapFormState(form, editingData);

  assert.deepEqual(form, {
    profile_type: "ANTHROPIC",
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
  editingData.upstream_source.profile_type = "OPENAI";
  editingData.upstream_source.endpoint = "https://old.example.com/v1";
  editingData.upstream_source.use_proxy = false;

  const payload = buildProviderUpdatePayload(editingData, {
    profile_type: "RESPONSES",
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
    upstream_source: {
      profile_type: "RESPONSES",
      endpoint: "https://new.example.com/v1",
      use_proxy: true,
    },
  });
});

test("provider state requires the nested Primary Source contract without legacy fallbacks", async () => {
  const [state, edit, baseForm] = await Promise.all([
    readFile(
      new URL("src/pages/provider-edit/composables/providerEditState.ts", ROOT),
      "utf8",
    ),
    readFile(
      new URL("src/pages/provider-edit/composables/useProviderEdit.ts", ROOT),
      "utf8",
    ),
    readFile(
      new URL("src/pages/provider-edit/components/ProviderBaseInfoForm.vue", ROOT),
      "utf8",
    ),
  ]);

  assert.match(state, /upstream_source:\s*\{/);
  assert.match(state, /source_key: "primary"/);
  assert.match(edit, /detail\.provider\.upstream_source\.profile_type/);
  assert.doesNotMatch(state, /provider_type/);
  assert.doesNotMatch(edit, /detail\.provider\.provider_type/);
  assert.match(baseForm, /labelSourceKey/);
  assert.doesNotMatch(baseForm, /source.*(?:create|delete|enable|default)/i);
});

test("provider credential plaintext stays in dialog-local state and clears on every boundary", () => {
  const state = useProviderCredentialSecretState();
  state.draftSecret.value = "draft-sensitive-marker";
  state.openReplace(41);
  state.draftSecret.value = "replacement-sensitive-marker";
  state.setRevealed({
    id: 41,
    provider_id: 7,
    description: null,
    key_prefix: "sk-a",
    key_last4: "last",
    is_enabled: true,
    created_at: 1,
    updated_at: 2,
    api_key: "revealed-sensitive-marker",
  });

  state.providerChanged();
  assert.equal(state.draftSecret.value, "");
  assert.equal(state.replacementKeyId.value, null);
  assert.equal(state.revealedSecret.value, null);

  state.draftSecret.value = "route-sensitive-marker";
  state.leaveRoute();
  assert.equal(state.draftSecret.value, "");

  state.draftSecret.value = "logout-sensitive-marker";
  state.logout();
  assert.equal(state.draftSecret.value, "");
});

test("provider credential service and UI keep saved summaries plaintext-free", async () => {
  const [types, service, component, bootstrap, check] = await Promise.all([
    readFile(new URL("src/services/types/providers.ts", ROOT), "utf8"),
    readFile(new URL("src/services/providers.ts", ROOT), "utf8"),
    readFile(
      new URL("src/pages/provider-edit/components/ProviderApiKeyList.vue", ROOT),
      "utf8",
    ),
    readFile(
      new URL("src/pages/provider-edit/components/ProviderBaseInfoForm.vue", ROOT),
      "utf8",
    ),
    readFile(
      new URL("src/pages/provider-edit/composables/useProviderCheck.ts", ROOT),
      "utf8",
    ),
  ]);

  const summaryContract = types.match(
    /interface ProviderApiKeySummary \{[\s\S]*?\n\}/,
  )?.[0];
  assert.ok(summaryContract);
  assert.doesNotMatch(summaryContract, /api_key\s*:/);
  assert.match(summaryContract, /key_prefix: string/);
  assert.match(summaryContract, /key_last4: string/);
  assert.match(summaryContract, /is_enabled: boolean/);

  assert.match(service, /provider\/\$\{id\}\/provider_keys`/);
  assert.match(service, /provider_keys\/\$\{keyId\}\/replace/);
  assert.match(service, /provider_keys\/\$\{keyId\}\/reveal/);
  assert.doesNotMatch(service, /sensitiveTotpRequestConfig|totpCode/);
  assert.doesNotMatch(service, /provider_key\/\$\{keyId\}/);

  assert.match(component, /provider_api_key: secretInput\.value/);
  assert.match(check, /provider_api_key_id: keyItem\.id/);
  assert.doesNotMatch(check, /provider_api_key: keyItem/);
  assert.doesNotMatch(component, /localStorage|sessionStorage|console\.error/);
  assert.doesNotMatch(bootstrap, /localStorage|sessionStorage|console\.error/);
});

test("remote model discovery returns explicit Primary Source evidence without a legacy payload fallback", async () => {
  const [types, component] = await Promise.all([
    readFile(new URL("src/services/types/providers.ts", ROOT), "utf8"),
    readFile(
      new URL("src/pages/provider-edit/components/ProviderModelList.vue", ROOT),
      "utf8",
    ),
  ]);

  const responseContract = types.match(
    /interface ProviderRemoteModelsResponse \{[\s\S]*?\n\}/,
  )?.[0];
  assert.ok(responseContract);
  assert.match(responseContract, /source_id: number/);
  assert.match(responseContract, /source_key: string/);
  assert.match(responseContract, /profile_type: string/);
  assert.match(responseContract, /models: ProviderRemoteModelsPayload/);
  assert.match(component, /const discoveredModels = response\.models/);
  assert.doesNotMatch(component, /Array\.isArray\(response\)/);
});
