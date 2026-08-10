import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const ROOT = new URL("../", import.meta.url);

async function readSource(path) {
  return readFile(new URL(path, ROOT), "utf8");
}

async function readMessages(locale) {
  const source = await readSource(`src/i18n/locales/${locale}/messages.json`);
  return JSON.parse(source);
}

test("legacy custom fields and global Request Patch products are absent", async () => {
  const [routerSource, navSource, providerEditSource, modelEditSource] = await Promise.all([
    readSource("src/router/index.ts"),
    readSource("src/router/nav-items.ts"),
    readSource("src/pages/provider-edit/ProviderEditPage.vue"),
    readSource("src/pages/model-edit/ModelEditPage.vue"),
  ]);

  assert.equal(routerSource.includes("custom_fields"), false);
  assert.equal(navSource.includes("/custom_fields"), false);
  assert.match(providerEditSource, /ProviderRequestPatchPanel/);
  assert.match(providerEditSource, /@request-patch/);
  assert.equal(providerEditSource.includes("ReasoningConfigPanel"), false);
  assert.equal(modelEditSource.includes("ReasoningConfigPanel"), false);
  assert.equal(providerEditSource.includes("RuntimeFeatureConfigPanel"), false);
  assert.equal(modelEditSource.includes("RuntimeFeatureConfigPanel"), false);
});

test("Source and Model Request Patch services expose only aggregate Variant routes", async () => {
  const [serviceSource, typesSource, managerSource, modelPanelSource, overviewSource] = await Promise.all([
    readSource("src/services/requestPatch.ts"),
    readSource("src/services/types/requestPatch.ts"),
    readSource("src/components/request-patch/RequestPatchVariantManager.vue"),
    readSource("src/pages/model-edit/components/ModelRequestPatchPanel.vue"),
    readSource("src/pages/model-edit/components/PatchVariantOverview.vue"),
  ]);

  assert.match(serviceSource, /listSourceRequestPatchVariants/);
  assert.match(serviceSource, /listModelSourceRequestPatchVariants/);
  assert.match(serviceSource, /previewModelSourceRequestPatchVariant/);
  assert.equal(serviceSource.includes("reasoning_config"), false);
  assert.equal(serviceSource.includes("/provider/\${providerId}/request_patch"), false);
  assert.match(typesSource, /source_id: number;/);
  assert.match(typesSource, /export interface RequestPatchVariantInput/);
  assert.equal(typesSource.includes("RequestPatchScopeKind"), false);
  assert.match(managerSource, /RequestPatchVariantEditor/);
  assert.match(modelPanelSource, /addModelSuffix/);
  assert.match(modelPanelSource, /:require-suffix="editingRequiresSuffix"/);
  assert.match(modelPanelSource, /:on-delete="requestDelete"/);
  assert.match(overviewSource, /onDelete\(state\.source\.id, modelVariant\)/);
});

test("reasoning and runtime repair configuration modules are removed", async () => {
  const [indexSource, dynamicCandidates, enMessages, zhMessages] = await Promise.all([
    readSource("src/services/types/index.ts"),
    readSource("src/i18n/dynamic-key-candidates.ts"),
    readMessages("en"),
    readMessages("zh"),
  ]);

  assert.equal(indexSource.includes("runtimeFeatureConfig"), false);
  assert.equal(dynamicCandidates.includes("ReasoningConfigPanel"), false);
  assert.equal(dynamicCandidates.includes("RuntimeFeatureConfigPanel"), false);
  assert.equal("reasoningConfigPanel" in enMessages, false);
  assert.equal("runtimeFeatureConfigPanel" in enMessages, false);
  assert.equal("reasoningConfigPanel" in zhMessages, false);
  assert.equal("runtimeFeatureConfigPanel" in zhMessages, false);
  assert.ok("requestPatchVariant" in enMessages);
  assert.ok("requestPatchVariant" in zhMessages);
});
