import test from "node:test";
import assert from "node:assert/strict";
import { ref } from "vue";

import {
  ensureReasoningProfiles,
  fetchReasoningProfiles,
  shouldFetchReasoningProfiles,
} from "../src/store/reasoningProfileLoadState.ts";

const normalizeError = (error) => {
  if (error instanceof Error) {
    return error;
  }
  return new Error(String(error));
};

const createState = () => ({
  profiles: ref([]),
  catalog: ref(null),
  loaded: ref(false),
  loading: ref(false),
  error: ref(null),
});

const createCatalog = () => ({
  families: [],
  presets: [],
});

test("reasoning profile ensureLoaded does not refetch an empty loaded list", async () => {
  const state = createState();
  const calls = {
    catalog: 0,
    list: 0,
  };
  const client = {
    async getReasoningProfileCatalog() {
      calls.catalog += 1;
      return createCatalog();
    },
    async getReasoningProfileList() {
      calls.list += 1;
      return [];
    },
  };

  const first = await ensureReasoningProfiles(state, client, normalizeError);

  assert.equal(state.loaded.value, true);
  assert.equal(state.loading.value, false);
  assert.equal(state.error.value, null);
  assert.deepEqual(first.profiles, []);
  assert.equal(calls.catalog, 1);
  assert.equal(calls.list, 1);
  assert.equal(shouldFetchReasoningProfiles(state), false);

  const second = await ensureReasoningProfiles(state, client, normalizeError);

  assert.deepEqual(second.profiles, []);
  assert.equal(calls.catalog, 1);
  assert.equal(calls.list, 1);
});

test("reasoning profile ensureLoaded retries after a failed initial load", async () => {
  const state = createState();
  let shouldFail = true;
  const calls = {
    catalog: 0,
    list: 0,
  };
  const client = {
    async getReasoningProfileCatalog() {
      calls.catalog += 1;
      if (shouldFail) {
        throw new Error("catalog unavailable");
      }
      return createCatalog();
    },
    async getReasoningProfileList() {
      calls.list += 1;
      return [];
    },
  };

  await assert.rejects(
    () => ensureReasoningProfiles(state, client, normalizeError),
    /catalog unavailable/,
  );

  assert.equal(state.loaded.value, false);
  assert.equal(state.loading.value, false);
  assert.equal(state.error.value, "catalog unavailable");
  assert.equal(shouldFetchReasoningProfiles(state), true);

  shouldFail = false;
  await ensureReasoningProfiles(state, client, normalizeError);

  assert.equal(state.loaded.value, true);
  assert.equal(state.error.value, null);
  assert.equal(calls.catalog, 2);
  assert.equal(calls.list, 2);
});

test("reasoning profile fetchAll marks loaded only after a successful response", async () => {
  const state = createState();
  const client = {
    async getReasoningProfileCatalog() {
      return createCatalog();
    },
    async getReasoningProfileList() {
      return null;
    },
  };

  const profiles = await fetchReasoningProfiles(state, client, normalizeError);

  assert.equal(state.loaded.value, true);
  assert.deepEqual(profiles, []);
  assert.deepEqual(state.profiles.value, []);
});
