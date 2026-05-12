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

test("legacy custom fields route and page are removed from the manager UI", async () => {
  const [routerSource, navSource, providerEditSource] = await Promise.all([
    readSource("src/router/index.ts"),
    readSource("src/router/nav-items.ts"),
    readSource("src/pages/provider-edit/ProviderEditPage.vue"),
  ]);

  assert.equal(routerSource.includes("custom_fields"), false);
  assert.equal(navSource.includes("/custom_fields"), false);
  assert.match(providerEditSource, /ProviderRequestPatchPanel/);
  assert.match(providerEditSource, /ReasoningConfigPanel/);
  assert.equal(providerEditSource.includes("ProviderCustomFieldList"), false);
});

test("provider and model resource pages use page-local entry points", async () => {
  const routerSource = await readSource("src/router/index.ts");

  assert.match(routerSource, /pages\/provider\/ProviderPage\.vue/);
  assert.match(routerSource, /pages\/provider-edit\/ProviderEditPage\.vue/);
  assert.match(routerSource, /pages\/model\/ModelPage\.vue/);
  assert.match(routerSource, /pages\/model-edit\/ModelEditPage\.vue/);
  assert.equal(routerSource.includes("@/pages/Provider.vue"), false);
  assert.equal(routerSource.includes("@/pages/ProviderEdit.vue"), false);
  assert.equal(routerSource.includes("@/pages/Model.vue"), false);
  assert.equal(routerSource.includes("@/pages/ModelEdit.vue"), false);
});

test("shared request patch and reasoning components do not call services directly", async () => {
  const [requestPatchPanelSource, reasoningPanelSource] = await Promise.all([
    readSource("src/components/request-patch/RequestPatchRulesPanel.vue"),
    readSource("src/components/reasoning/ReasoningConfigPanel.vue"),
  ]);

  assert.equal(requestPatchPanelSource.includes("@/services/requestPatch"), false);
  assert.equal(requestPatchPanelSource.includes("requestPatchService"), false);
  assert.equal(reasoningPanelSource.includes("@/services/requestPatch"), false);
  assert.equal(reasoningPanelSource.includes("requestPatchService"), false);
  assert.match(requestPatchPanelSource, /actions\.createRule/);
  assert.match(reasoningPanelSource, /actions\.updateConfig/);
});

test("legacy custom field i18n sections are removed after request patch migration", async () => {
  const [enMessages, zhMessages] = await Promise.all([
    readMessages("en"),
    readMessages("zh"),
  ]);

  assert.equal("customFieldsPage" in enMessages, false);
  assert.equal("customFieldsPage" in zhMessages, false);
  assert.match(JSON.stringify(enMessages), /requestPatch/);
  assert.match(JSON.stringify(zhMessages), /requestPatch/);
});
