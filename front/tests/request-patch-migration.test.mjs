import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const ROOT = new URL("../", import.meta.url);

async function readSource(path) {
  return readFile(new URL(path, ROOT), "utf8");
}

test("legacy custom fields route and page are removed from the manager UI", async () => {
  const [routerSource, navSource, providerEditSource] = await Promise.all([
    readSource("src/router/index.ts"),
    readSource("src/lib/nav-items.ts"),
    readSource("src/pages/ProviderEdit.vue"),
  ]);

  assert.equal(routerSource.includes("custom_fields"), false);
  assert.equal(navSource.includes("/custom_fields"), false);
  assert.match(providerEditSource, /ProviderRequestPatchPanel/);
  assert.equal(providerEditSource.includes("ProviderCustomFieldList"), false);
});

test("legacy custom field i18n sections are removed after request patch migration", async () => {
  const [enSource, zhSource] = await Promise.all([
    readSource("src/i18n/en.ts"),
    readSource("src/i18n/zh.ts"),
  ]);

  assert.equal(enSource.includes("customFieldsPage"), false);
  assert.equal(zhSource.includes("customFieldsPage"), false);
  assert.match(enSource, /requestPatch/);
  assert.match(zhSource, /requestPatch/);
});
