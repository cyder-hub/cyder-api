import test from "node:test";
import assert from "node:assert/strict";

import {
  AUTH_SESSION_STORAGE_KEY,
  clearStoredAuthSession,
  clearStoredAuthSessionIfCurrent,
  persistAuthTokenPair,
  readStoredAuthSession,
  readStoredSessionRecord,
  subscribeToAuthSessionStorage,
} from "../src/services/authTokens.ts";
import { applyStoredAuthSession } from "../src/services/authSession.ts";

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

function storageEventTarget() {
  let listener = null;
  return {
    addEventListener(type, callback) {
      assert.equal(type, "storage");
      listener = callback;
    },
    removeEventListener(type, callback) {
      assert.equal(type, "storage");
      if (listener === callback) listener = null;
    },
    dispatch(key, newValue = null) {
      listener?.({ key, newValue });
    },
  };
}

test("token pair is persisted as one strict versioned session record", () => {
  const storage = memoryStorage();

  const first = persistAuthTokenPair(
    { refresh_token: "refresh-old", access_token: "access-old" },
    storage,
    () => 100,
  );
  assert.deepEqual(first, {
    schema_version: 1,
    revision: 100,
    refresh_token: "refresh-old",
    access_token: "access-old",
  });
  assert.deepEqual(JSON.parse(storage.getItem(AUTH_SESSION_STORAGE_KEY)), first);

  const second = persistAuthTokenPair(
    { refresh_token: "refresh-new", access_token: "access-new" },
    storage,
    () => 99,
  );
  assert.equal(second.revision, 101);
  assert.deepEqual(readStoredSessionRecord(storage), second);

  clearStoredAuthSession(storage);
  assert.equal(readStoredAuthSession(storage), null);
});

test("raw refresh token is read only as a legacy session and migrates on write", () => {
  const storage = memoryStorage();
  storage.setItem(AUTH_SESSION_STORAGE_KEY, "legacy-refresh-token");

  assert.deepEqual(readStoredAuthSession(storage), {
    kind: "legacy",
    refresh_token: "legacy-refresh-token",
  });

  const migrated = persistAuthTokenPair(
    { refresh_token: "refresh-v1", access_token: "access-v1" },
    storage,
    () => 200,
  );
  assert.deepEqual(readStoredAuthSession(storage), {
    kind: "record",
    record: migrated,
  });
});

test("parsed malformed records are rejected without exposing their values", () => {
  const storage = memoryStorage();
  const invalidRecords = [
    {},
    { schema_version: 2, revision: 1, refresh_token: "r", access_token: "a" },
    { schema_version: 1, revision: -1, refresh_token: "r", access_token: "a" },
    { schema_version: 1, revision: 1, refresh_token: "", access_token: "a" },
    {
      schema_version: 1,
      revision: 1,
      refresh_token: "r",
      access_token: "a",
      unexpected: true,
    },
  ];

  for (const invalid of invalidRecords) {
    storage.setItem(AUTH_SESSION_STORAGE_KEY, JSON.stringify(invalid));
    assert.equal(readStoredAuthSession(storage), null);
  }
});

test("conditional clear preserves a session rotated by another tab", () => {
  const storage = memoryStorage();
  persistAuthTokenPair(
    { refresh_token: "refresh-current", access_token: "access-current" },
    storage,
    () => 1,
  );

  assert.equal(clearStoredAuthSessionIfCurrent("refresh-stale", storage), false);
  assert.equal(readStoredSessionRecord(storage).refresh_token, "refresh-current");

  assert.equal(clearStoredAuthSessionIfCurrent("refresh-current", storage), true);
  assert.equal(readStoredAuthSession(storage), null);
});

test("storage events reread the current key and deterministically update the store", () => {
  const storage = memoryStorage();
  const events = storageEventTarget();
  const store = {
    accessToken: "access-existing",
    lifecycle: "authenticated",
    setAuthenticated(token) {
      this.accessToken = token;
      this.lifecycle = "authenticated";
    },
    setAnonymous() {
      this.accessToken = null;
      this.lifecycle = "anonymous";
    },
  };
  const seen = [];
  const unsubscribe = subscribeToAuthSessionStorage(
    (session) => {
      seen.push(session);
      applyStoredAuthSession(store, session);
    },
    events,
    storage,
  );

  const record = persistAuthTokenPair(
    { refresh_token: "refresh-current", access_token: "access-current" },
    storage,
    () => 10,
  );
  events.dispatch(AUTH_SESSION_STORAGE_KEY, "stale-event-value");
  assert.equal(store.accessToken, "access-current");
  assert.equal(store.lifecycle, "authenticated");
  assert.deepEqual(seen.at(-1), { kind: "record", record });

  storage.setItem(AUTH_SESSION_STORAGE_KEY, JSON.stringify({ damaged: true }));
  events.dispatch(AUTH_SESSION_STORAGE_KEY);
  assert.equal(store.accessToken, null);
  assert.equal(store.lifecycle, "anonymous");
  assert.equal(seen.at(-1), null);

  storage.removeItem(AUTH_SESSION_STORAGE_KEY);
  events.dispatch(AUTH_SESSION_STORAGE_KEY);
  assert.equal(store.accessToken, null);
  assert.equal(seen.at(-1), null);

  events.dispatch("unrelated-key");
  assert.equal(seen.length, 3);
  unsubscribe();
});
