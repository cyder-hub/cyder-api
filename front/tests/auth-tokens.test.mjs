import test from "node:test";
import assert from "node:assert/strict";

import {
  REFRESH_TOKEN_STORAGE_KEY,
  clearStoredRefreshTokenIfCurrent,
  clearStoredRefreshToken,
  persistAuthTokenPair,
  readStoredRefreshToken,
} from "../src/services/authTokens.ts";

function memoryStorage() {
  const values = new Map();
  return {
    getItem(key) {
      return values.has(key) ? values.get(key) : null;
    },
    setItem(key, value) {
      values.set(key, value);
    },
    removeItem(key) {
      values.delete(key);
    },
  };
}

test("auth token pair persistence overwrites rotated refresh token", () => {
  const storage = memoryStorage();

  assert.equal(
    persistAuthTokenPair(
      { refresh_token: "refresh-old", access_token: "access-old" },
      storage,
    ),
    "access-old",
  );
  assert.equal(storage.getItem(REFRESH_TOKEN_STORAGE_KEY), "refresh-old");

  assert.equal(
    persistAuthTokenPair(
      { refresh_token: "refresh-new", access_token: "access-new" },
      storage,
    ),
    "access-new",
  );
  assert.equal(readStoredRefreshToken(storage), "refresh-new");

  clearStoredRefreshToken(storage);
  assert.equal(readStoredRefreshToken(storage), null);
});

test("conditional clear preserves refresh token rotated by another tab", () => {
  const storage = memoryStorage();
  persistAuthTokenPair(
    { refresh_token: "refresh-current", access_token: "access-current" },
    storage,
  );

  assert.equal(
    clearStoredRefreshTokenIfCurrent("refresh-stale", storage),
    false,
  );
  assert.equal(readStoredRefreshToken(storage), "refresh-current");

  assert.equal(
    clearStoredRefreshTokenIfCurrent("refresh-current", storage),
    true,
  );
  assert.equal(readStoredRefreshToken(storage), null);
});
