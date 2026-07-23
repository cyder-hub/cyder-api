import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import {
  LEGACY_AUTH_STORAGE_KEY,
  clearAccessToken,
  clearLegacyAuthStorage,
  getAccessToken,
  setAccessToken,
} from "../src/services/authTokens.ts";

const ROOT = new URL("../", import.meta.url);

function memoryStorage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem(key) {
      return values.has(key) ? values.get(key) : null;
    },
    removeItem(key) {
      values.delete(key);
    },
  };
}

test("manager access token has one process-memory owner", () => {
  clearAccessToken();
  assert.equal(getAccessToken(), null);

  setAccessToken("access-current");
  assert.equal(getAccessToken(), "access-current");

  setAccessToken("access-rotated");
  assert.equal(getAccessToken(), "access-rotated");

  clearAccessToken();
  assert.equal(getAccessToken(), null);
});

test("startup cleanup removes only the legacy auth key from both storage scopes", () => {
  const local = memoryStorage({
    [LEGACY_AUTH_STORAGE_KEY]: "legacy-local-token",
    preference: "keep-local",
  });
  const session = memoryStorage({
    [LEGACY_AUTH_STORAGE_KEY]: "legacy-session-token",
    draft: "keep-session",
  });

  clearLegacyAuthStorage(local, session);

  assert.equal(local.getItem(LEGACY_AUTH_STORAGE_KEY), null);
  assert.equal(session.getItem(LEGACY_AUTH_STORAGE_KEY), null);
  assert.equal(local.getItem("preference"), "keep-local");
  assert.equal(session.getItem("draft"), "keep-session");
});

test("runtime auth token module cannot persist or parse token records", async () => {
  const source = await readFile(
    new URL("src/services/authTokens.ts", ROOT),
    "utf8",
  );
  assert.doesNotMatch(source, /setItem|getItem|JSON\.parse|revision|refresh_token/);
  assert.match(source, /local\.removeItem\(LEGACY_AUTH_STORAGE_KEY\)/);
  assert.match(source, /session\.removeItem\(LEGACY_AUTH_STORAGE_KEY\)/);
});
