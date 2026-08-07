import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { aggregateProviderRuntimeLevels } from "../src/pages/provider/composables/providerRuntimeAggregation.ts";
import { useRecordList } from "../src/pages/record/composables/useRecordList.ts";
import { parseRecordQueryState } from "../src/pages/record/composables/useRecordQuery.ts";

const ROOT = new URL("../", import.meta.url);

const readSource = (path) => readFile(new URL(path, ROOT), "utf8");

test("provider upstream actions require an explicit Source and keep Source evidence", async () => {
  const [page, check, model, key, source, selector, service] = await Promise.all([
    readSource("src/pages/provider-edit/ProviderEditPage.vue"),
    readSource("src/pages/provider-edit/composables/useProviderCheck.ts"),
    readSource("src/pages/provider-edit/components/ProviderModelList.vue"),
    readSource("src/pages/provider-edit/components/ProviderApiKeyList.vue"),
    readSource("src/pages/provider-edit/components/ProviderSourceList.vue"),
    readSource("src/pages/provider-edit/components/ProviderSourceSelector.vue"),
    readSource("src/services/providers.ts"),
  ]);

  assert.match(page, /ProviderSourceSelector/);
  assert.match(page, /:source-id="selectedSourceId"/);
  assert.match(page, /@check-source="handleSourceCheck"/);
  assert.doesNotMatch(page, /activeSourceProfile|profile-type=/);

  assert.match(check, /selectedSourceId: Ref<number \| null>/);
  assert.match(check, /sourceRequiredForCheck/);
  assert.match(check, /isSourceCheckModalOpen/);
  assert.doesNotMatch(check, /getCheckSourceId/);
  assert.doesNotMatch(check, /is_default\)\?\.id|upstream_sources\[0\]/);

  assert.match(model, /getProviderRemoteModels\(data\.id, props\.sourceId\)/);
  assert.match(model, /response\.profile_type/);
  assert.match(model, /discoveryInheritanceWarning/);
  assert.match(key, /checkProviderConnection\(providerId, props\.sourceId/);
  assert.match(source, /emit\('checkSource', source\.id\)/);
  assert.match(selector, /sourceSelection/);

  assert.match(service, /\/provider\/\$\{providerId\}\/sources\/\$\{sourceId\}\/check/);
  assert.match(service, /\/provider\/\$\{providerId\}\/sources\/\$\{sourceId\}\/remote_models/);
  assert.doesNotMatch(service, /\/provider\/\$\{providerId\}\/check/);
});

test("credential secrets are opaque text while dialog cleanup and reveal governance remain", async () => {
  const [form, key, secretState, security] = await Promise.all([
    readSource("src/pages/provider-edit/components/ProviderBaseInfoForm.vue"),
    readSource("src/pages/provider-edit/components/ProviderApiKeyList.vue"),
    readSource("src/pages/provider-edit/composables/useProviderCredentialSecretState.ts"),
    readSource("tests/sensitive-command-totp.test.mjs"),
  ]);

  assert.match(form, /v-model="quickStart\.api_key"[\s\S]*type="password"/);
  assert.match(key, /v-model="secretInput"[\s\S]*type="password"/);
  assert.doesNotMatch(form, /vertexPlaceholder|vertexHelp/);
  assert.doesNotMatch(key, /isVertex|vertexInvalid|JSON\.parse/);
  assert.match(secretState, /clearDialog/);
  assert.match(security, /ProviderApiKeyList\.vue/);
  assert.match(security, /runWithSecretGovernanceReauth/);
});

test("runtime and dashboard expose Provider and Source counts without provider-row collisions", async () => {
  const [runtimePage, runtimeTable, runtimeCards, dashboard] = await Promise.all([
    readSource("src/pages/provider-runtime/ProviderRuntimePage.vue"),
    readSource("src/pages/provider-runtime/components/ProviderRuntimeTable.vue"),
    readSource("src/pages/provider-runtime/components/ProviderRuntimeCards.vue"),
    readSource("src/pages/dashboard/composables/useDashboardData.ts"),
  ]);

  assert.match(runtimePage, /total_source_count/);
  assert.match(runtimePage, /enabled_source_count/);
  assert.match(runtimeTable, /:key="item\.source_id"/);
  assert.match(runtimeCards, /:key="item\.source_id"/);
  assert.doesNotMatch(runtimeTable, /\$\{item\.provider_id\}:\$\{item\.source_id\}/);
  assert.match(dashboard, /total_source_count/);
  assert.match(dashboard, /enabled_source_count/);
});

test("runtime provider badges aggregate the worst Source state instead of overwriting it", () => {
  assert.deepEqual(
    aggregateProviderRuntimeLevels([
      { provider_id: 7, runtime_level: "healthy" },
      { provider_id: 7, runtime_level: "open" },
      { provider_id: 8, runtime_level: "degraded" },
      { provider_id: 8, runtime_level: "half_open" },
    ]),
    { 7: "open", 8: "half_open" },
  );
});

test("retired Source deep links remain selectable in the historical record filter", () => {
  const parsed = parseRecordQueryState({ source_id: "999" });
  assert.equal(parsed.filters.source_id, 999);

  const filters = {
    search: "",
    provider_id: 0,
    source_id: 999,
    model_id: 0,
    api_key_id: 0,
    status: "ALL",
    downstream_protocol: "ALL",
    final_error_code: "",
    latency_ms_min: "",
    latency_ms_max: "",
    total_tokens_min: "",
    total_tokens_max: "",
    estimated_cost_nanos_min: "",
    estimated_cost_nanos_max: "",
    start_time: "",
    end_time: "",
  };
  const state = useRecordList({
    filters,
    currentPage: { value: 1 },
    pageSize: { value: 20 },
    buildListParams: () => ({}),
    t: (key, params) =>
      params?.id ? `${key}:${params.id}` : key,
    providerStore: {
      providers: [],
      sources: [{ id: 101, provider_id: 1, profile_type: "OPENAI", deleted_at: null }],
      fetchProviders: async () => undefined,
      fetchProviderSources: async () => undefined,
    },
    apiKeyStore: { apiKeys: [], fetchApiKeys: async () => undefined },
    modelStore: { modelOptions: [], fetchModels: async () => undefined },
    api: { getRecordList: async () => ({ list: [], total: 0 }) },
  });

  assert.deepEqual(
    state.sourceOptions.value.map((item) => item.value),
    ["0", "101", "999"],
  );
  assert.match(state.sourceOptions.value[2].label, /999/);
});

test("reasoning loading keeps the editor available when saved preview is ambiguous", async () => {
  const panel = await readSource("src/components/reasoning/ReasoningConfigPanel.vue");
  assert.match(panel, /const \[catalogResponse, configResponse\] = await Promise\.all/);
  assert.match(panel, /savedPreviewError/);
  assert.match(panel, /props\.actions\.previewSaved\(props\.ownerId\)/);
  assert.match(panel, /savedPreviewFailed/);
});

test("Source mutations reconcile committed state after post-commit failures", async () => {
  const [sources, sourceList] = await Promise.all([
    readSource("src/pages/provider-edit/composables/useProviderSources.ts"),
    readSource("src/pages/provider-edit/components/ProviderSourceList.vue"),
  ]);
  assert.match(sources, /const recoverAfterMutationFailure = \(\) => refreshSources\(\)\.catch/);
  assert.equal((sources.match(/await recoverAfterMutationFailure\(\)/g) || []).length, 3);
  assert.match(sourceList, /getProviderRuntimeSnapshot/);
  assert.match(sourceList, /sourceRuntime\(source\.id\)/);
  assert.match(sourceList, /providerEditPage\.sources\.circuitUnavailable/);
});

test("Record Source options flatten the aggregate Source catalog", () => {
  const filters = {
    search: "",
    provider_id: 0,
    source_id: 0,
    model_id: 0,
    api_key_id: 0,
    status: "ALL",
    downstream_protocol: "ALL",
    min_input_tokens: null,
    max_input_tokens: null,
    min_output_tokens: null,
    max_output_tokens: null,
    min_total_tokens: null,
    max_total_tokens: null,
    min_cost_nanos: null,
    max_cost_nanos: null,
    min_latency_ms: null,
    max_latency_ms: null,
  };
  const state = useRecordList({
    filters,
    currentPage: { value: 1 },
    pageSize: { value: 20 },
    buildListParams: () => ({}),
    t: (key) => key,
    providerStore: {
      providers: [
        { id: 1, name: "Gateway A", default_source_id: 101, default_source_profile_type: "OPENAI" },
      ],
      sources: [
        { id: 101, provider_id: 1, profile_type: "OPENAI", deleted_at: null },
        { id: 102, provider_id: 1, profile_type: "GEMINI", deleted_at: null },
      ],
      fetchProviders: async () => undefined,
      fetchProviderSources: async () => undefined,
    },
    apiKeyStore: { apiKeys: [], fetchApiKeys: async () => undefined },
    modelStore: { modelOptions: [], fetchModels: async () => undefined },
    api: { getRecordList: async () => ({ list: [], total: 0 }) },
  });

  assert.deepEqual(
    state.sourceOptions.value.map((item) => item.value),
    ["0", "101", "102"],
  );
  assert.match(state.sourceOptions.value[2].label, /GEMINI/);
});
