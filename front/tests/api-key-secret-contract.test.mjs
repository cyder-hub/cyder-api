import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { copyText } from "../src/utils/clipboard.ts";
import { useApiKeySecretState } from "../src/pages/api-key/composables/useApiKeySecretState.ts";
import { createHttpAuthRefreshHandler } from "../src/services/httpAuthRefresh.ts";

const ROOT = new URL("../", import.meta.url);

function secret(id = 1, canReveal = false) {
  return {
    id,
    name: `key-${id}`,
    key_prefix: "cyder-ab",
    key_last4: "1234",
    api_key: `cyder-secret-${id}`,
    updated_at: 0,
    can_reveal: canReveal,
  };
}

test("component-scoped API key plaintext obeys every cleanup boundary", () => {
  const state = useApiKeySecretState();

  state.setIssuedSecret(secret(1));
  state.setRevealedSecret(secret(1));
  assert.equal(state.issuedSecret.value?.api_key, "cyder-secret-1");
  assert.equal(state.revealedSecret.value?.api_key, "cyder-secret-1");

  // Reveal remains available while the same drawer stays open.
  state.selectKey(1);
  assert.equal(state.revealedSecret.value?.api_key, "cyder-secret-1");

  state.selectKey(2);
  assert.equal(state.revealedSecret.value, null, "key switch clears reveal");

  state.setRevealedSecret(secret(2));
  state.closeDrawer();
  assert.equal(state.revealedSecret.value, null, "drawer close clears reveal");

  state.setIssuedSecret(secret(2));
  state.setRevealedSecret(secret(2));
  state.leaveRoute();
  assert.equal(state.issuedSecret.value, null, "route leave clears issued secret");
  assert.equal(state.revealedSecret.value, null, "route leave clears reveal");

  state.setIssuedSecret(secret(3));
  state.setRevealedSecret(secret(3));
  state.logout();
  assert.equal(state.issuedSecret.value, null, "logout clears issued secret");
  assert.equal(state.revealedSecret.value, null, "logout clears reveal");
});

test("issued secret can be acknowledged without a copy prerequisite", () => {
  const state = useApiKeySecretState();
  state.setIssuedSecret(secret());
  state.setIssuedSecret(null);
  assert.equal(state.issuedSecret.value, null);
});

test("clipboard helper reports copy success and failure", async () => {
  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const writes = [];
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: { clipboard: { writeText: async (value) => writes.push(value) } },
  });

  try {
    assert.equal(await copyText("cyder-secret"), true);
    assert.deepEqual(writes, ["cyder-secret"]);
  } finally {
    if (originalNavigator) {
      Object.defineProperty(globalThis, "navigator", originalNavigator);
    } else {
      delete globalThis.navigator;
    }
  }

  assert.equal(await copyText(""), false);
});

test("Create and Rotate opt out of authentication replay", async () => {
  let refreshCalls = 0;
  let retryCalls = 0;
  const error = {
    config: { _skipAuthRetry: true },
    response: { status: 401, data: { code: 1432 } },
  };
  const handler = createHttpAuthRefreshHandler({
    getAccessToken: () => "access-current",
    getLifecycle: () => "authenticated",
    restoreSession: async () => true,
    recoverAccess: async () => {
      refreshCalls += 1;
      throw new Error("not called");
    },
    revokeSession: () => {},
    retryRequest: async () => {
      retryCalls += 1;
    },
  });

  await assert.rejects(handler(error), (caught) => caught === error);
  assert.equal(refreshCalls, 0);
  assert.equal(retryCalls, 0);
});

test("API key UI and service preserve the mode-aware secret contract", async () => {
  const [dialog, drawer, page, service, state, en, zh] = await Promise.all([
    readFile(new URL("src/pages/api-key/components/ApiKeySecretDialog.vue", ROOT), "utf8"),
    readFile(new URL("src/pages/api-key/components/ApiKeyDetailDrawer.vue", ROOT), "utf8"),
    readFile(new URL("src/pages/api-key/ApiKeyPage.vue", ROOT), "utf8"),
    readFile(new URL("src/services/apiKeys.ts", ROOT), "utf8"),
    readFile(new URL("src/pages/api-key/composables/useApiKeySecretState.ts", ROOT), "utf8"),
    readFile(new URL("src/i18n/locales/en/messages.json", ROOT), "utf8"),
    readFile(new URL("src/i18n/locales/zh/messages.json", ROOT), "utf8"),
  ]);

  assert.match(dialog, /:show-close-button="false"/);
  assert.match(dialog, /@escape-key-down\.prevent/);
  assert.match(dialog, /@pointer-down-outside\.prevent/);
  assert.match(dialog, /@interact-outside\.prevent/);
  assert.match(dialog, /@click="\$emit\('acknowledge'\)"/);
  assert.match(dialog, /v-if="!secret\.can_reveal"/);
  assert.match(dialog, /secret\?\.can_reveal/);
  assert.match(dialog, /apiKeyPage\.issuedSecret\.recoverableDescription/);
  assert.doesNotMatch(dialog, /download|setTimeout|copied.*disabled/i);

  assert.match(drawer, /v-if="detail\.can_reveal"/);
  assert.match(drawer, /apiKeyPage\.secret\.unavailable/);
  assert.match(service, /api_key\/\$\{id\}\/reveal/);
  assert.doesNotMatch(service, /request\.get\([^\n]*\/reveal/);
  assert.doesNotMatch(service, /sensitiveTotpRequestConfig|totpCode/);

  assert.match(page, /onBeforeRouteLeave/);
  assert.match(page, /lifecycle === "anonymous"/);
  assert.doesNotMatch(`${page}\n${state}`, /localStorage|sessionStorage|setTimeout/);
  assert.doesNotMatch(`${page}\n${state}`, /use[A-Za-z]+Store\([^)]*secret/i);

  for (const messages of [JSON.parse(en), JSON.parse(zh)]) {
    assert.ok(messages.apiKeyPage.issuedSecret.responseLoss);
    assert.ok(messages.apiKeyPage.issuedSecret.acknowledge);
    assert.ok(messages.apiKeyPage.issuedSecret.recoverableTitle);
    assert.ok(messages.apiKeyPage.issuedSecret.recoverableDescription);
    assert.ok(messages.apiKeyPage.issuedSecret.close);
    assert.ok(messages.apiKeyPage.secret.unavailable);
  }
});
