import test from "node:test";
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";

import { createAuthSessionActions } from "../src/services/authSession.ts";
import {
  createHttpAuthRefreshHandler,
  createProtectedManagerRequestGate,
  ManagerAuthenticationRequiredError,
} from "../src/services/httpAuthRefresh.ts";
import { useLoginForm } from "../src/pages/login/composables/useLoginForm.ts";

const ROOT = new URL("../", import.meta.url);

function sessionRecord(revision = 1, suffix = "current") {
  return {
    schema_version: 1,
    revision,
    refresh_token: `refresh-${suffix}`,
    access_token: `access-${suffix}`,
  };
}

function authError(code) {
  return { response: { status: 401, data: { code } } };
}

function createStore() {
  return {
    accessToken: "access-existing",
    lifecycle: "unknown",
    setRestoring() {
      this.lifecycle = "restoring";
      this.accessToken = null;
    },
    setUnknown() {
      this.lifecycle = "unknown";
      this.accessToken = null;
    },
    setAuthenticated(token) {
      this.lifecycle = "authenticated";
      this.accessToken = token;
    },
    setAnonymous() {
      this.lifecycle = "anonymous";
      this.accessToken = null;
    },
  };
}

function createAuthHarness(overrides = {}) {
  const calls = {
    persisted: [],
    cleared: 0,
    clearIfCurrent: [],
    logout: 0,
    logoutAll: 0,
  };
  const store = createStore();
  let storedRefreshToken = overrides.storedRefreshToken ?? "refresh-current";

  const actions = createAuthSessionActions({
    getAuthStore: () => store,
    readStoredAuthSession: () =>
      storedRefreshToken
        ? {
            kind: "record",
            record: {
              ...sessionRecord(),
              refresh_token: storedRefreshToken,
              access_token: store.accessToken ?? "access-current",
            },
          }
        : null,
    persistAuthTokenPair: (tokenPair) => {
      calls.persisted.push(tokenPair);
      storedRefreshToken = tokenPair.refresh_token;
      return { schema_version: 1, revision: 2, ...tokenPair };
    },
    clearStoredAuthSession: () => {
      calls.cleared += 1;
      storedRefreshToken = null;
    },
    clearStoredAuthSessionIfCurrent: (refreshToken) => {
      calls.clearIfCurrent.push(refreshToken);
      if (storedRefreshToken !== refreshToken) return false;
      storedRefreshToken = null;
      return true;
    },
    refreshToken:
      overrides.refreshToken ??
      (async () => ({
        refresh_token: "refresh-rotated",
        access_token: "access-rotated",
      })),
    loginWithPassword:
      overrides.loginWithPassword ??
      (async () => ({
        refresh_token: "refresh-login",
        access_token: "access-login",
      })),
    bootstrapWithPassword:
      overrides.bootstrapWithPassword ??
      (async () => ({
        refresh_token: "refresh-bootstrap",
        access_token: "access-bootstrap",
      })),
    rotateManagerPassword:
      overrides.rotateManagerPassword ??
      (async () => ({
        refresh_token: "refresh-rotated-password",
        access_token: "access-rotated-password",
      })),
    logoutRequest:
      overrides.logoutRequest ??
      (async () => {
        calls.logout += 1;
      }),
    logoutAllRequest:
      overrides.logoutAllRequest ??
      (async () => {
        calls.logoutAll += 1;
      }),
  });

  return {
    actions,
    calls,
    get storedRefreshToken() {
      return storedRefreshToken;
    },
    store,
    setStoredRefreshToken(value) {
      storedRefreshToken = value;
    },
  };
}

function createHttpHarness(overrides = {}) {
  let record = overrides.record === undefined ? sessionRecord() : overrides.record;
  let lifecycle = overrides.lifecycle ?? "authenticated";
  let storageListener = null;
  const calls = { refresh: 0, retry: [], cleared: [], redirects: 0 };
  const deps = {
    readStoredAuthSession: () =>
      record ? { kind: "record", record } : null,
    persistAuthTokenPair: (tokenPair) => {
      record = { schema_version: 1, revision: (record?.revision ?? 0) + 1, ...tokenPair };
      return record;
    },
    clearStoredAuthSessionIfCurrent: (refreshToken) => {
      calls.cleared.push(refreshToken);
      if (record?.refresh_token !== refreshToken) return false;
      record = null;
      return true;
    },
    getLifecycle: () => lifecycle,
    restoreStoredSession: overrides.restoreStoredSession ?? (async () => true),
    setAuthenticated: (token) => {
      lifecycle = "authenticated";
      if (record) record = { ...record, access_token: token };
    },
    setAnonymous: () => {
      lifecycle = "anonymous";
    },
    refreshAccessToken:
      overrides.refreshAccessToken ??
      (async () => {
        calls.refresh += 1;
        return {
          refresh_token: "refresh-rotated",
          access_token: "access-rotated",
        };
      }),
    retryRequest: async (request) => {
      calls.retry.push({ ...request, headers: { ...request.headers } });
      return { retried: true };
    },
    redirectToLogin: () => {
      calls.redirects += 1;
    },
    subscribeToSessionChanges: (listener) => {
      storageListener = listener;
      return () => {};
    },
  };
  const handler = createHttpAuthRefreshHandler(deps);
  return {
    calls,
    deps,
    handler,
    emitStorage(session) {
      record = session?.kind === "record" ? session.record : null;
      storageListener?.(session);
    },
    get lifecycle() {
      return lifecycle;
    },
    get record() {
      return record;
    },
    set record(value) {
      record = value;
    },
  };
}

function accessFailure(code, request = {}) {
  return {
    response: { status: 401, data: { code } },
    config: {
      url: "/ai/manager/api/system/dashboard",
      headers: { Authorization: "Bearer access-current" },
      _authRevision: 1,
      _authRefreshToken: "refresh-current",
      ...request,
    },
  };
}

test("login, bootstrap, and password rotation share one persistence owner", async () => {
  const harness = createAuthHarness();
  await harness.actions.login("secret");
  assert.equal(harness.store.lifecycle, "authenticated");
  await harness.actions.bootstrap("new administrator password");
  await harness.actions.rotatePassword("current password", "new password");
  assert.equal(harness.store.accessToken, "access-rotated-password");
  assert.equal(harness.storedRefreshToken, "refresh-rotated-password");
  assert.equal(harness.calls.persisted.length, 3);
});

test("restoration is shared, rotates the pair, and enters authenticated", async () => {
  let refreshCalls = 0;
  let resolveRefresh;
  const pending = new Promise((resolve) => {
    resolveRefresh = resolve;
  });
  const harness = createAuthHarness({
    refreshToken: async () => {
      refreshCalls += 1;
      return pending;
    },
  });

  const first = harness.actions.restoreStoredSession();
  const second = harness.actions.restoreStoredSession();
  assert.equal(harness.store.lifecycle, "restoring");
  resolveRefresh({ refresh_token: "refresh-rotated", access_token: "access-rotated" });
  assert.deepEqual(await Promise.all([first, second]), [true, true]);
  assert.equal(refreshCalls, 1);
  assert.equal(harness.store.lifecycle, "authenticated");
});

test("definite refresh invalidity clears session while transient failure preserves it", async () => {
  const invalid = createAuthHarness({ refreshToken: async () => { throw authError(1441); } });
  assert.equal(await invalid.actions.restoreStoredSession(), false);
  assert.equal(invalid.store.lifecycle, "anonymous");
  assert.equal(invalid.storedRefreshToken, null);

  const transient = createAuthHarness({ refreshToken: async () => { throw authError(1443); } });
  assert.equal(await transient.actions.restoreStoredSession(), false);
  assert.equal(transient.store.lifecycle, "unknown");
  assert.equal(transient.storedRefreshToken, "refresh-current");
});

test("startup restoration adopts a cross-tab winner after refresh replay", async () => {
  let harness;
  harness = createAuthHarness({
    refreshToken: async () => {
      harness.setStoredRefreshToken("refresh-winner");
      harness.store.accessToken = "access-winner";
      throw authError(1444);
    },
  });

  assert.equal(await harness.actions.restoreStoredSession(), true);
  assert.equal(harness.store.lifecycle, "authenticated");
  assert.equal(harness.store.accessToken, "access-winner");
  assert.deepEqual(harness.calls.clearIfCurrent, []);
});

test("logout clears local session even when server revocation fails", async () => {
  const harness = createAuthHarness({
    logoutRequest: async () => { throw new Error("offline"); },
  });
  const outcome = await harness.actions.logout();
  assert.equal(harness.calls.cleared, 1);
  assert.equal(harness.store.lifecycle, "anonymous");
  assert.deepEqual(outcome, { serverRevocationConfirmed: false });
});

test("logout all clears only on success or a definite invalid-session response", async () => {
  const success = createAuthHarness();
  await success.actions.logoutAll();
  assert.equal(success.storedRefreshToken, null);
  assert.equal(success.store.lifecycle, "anonymous");

  const invalid = createAuthHarness({
    logoutAllRequest: async () => { throw authError(1435); },
  });
  await invalid.actions.logoutAll();
  assert.equal(invalid.storedRefreshToken, null);
  assert.equal(invalid.store.lifecycle, "anonymous");

  const unavailable = createAuthHarness({
    logoutAllRequest: async () => {
      throw { response: { status: 503, data: { code: 1436 } } };
    },
  });
  await assert.rejects(unavailable.actions.logoutAll());
  assert.equal(unavailable.storedRefreshToken, "refresh-current");
  assert.equal(unavailable.store.lifecycle, "unknown");
});

test("protected request gate waits for restore and rejects anonymous locally", async () => {
  const harness = createHttpHarness({ lifecycle: "restoring" });
  const gate = createProtectedManagerRequestGate(harness.deps);
  const request = await gate({ url: "/ai/manager/api/system/dashboard", headers: {} });
  assert.equal(request.headers.Authorization, "Bearer access-current");
  assert.equal(request._authRevision, 1);

  harness.emitStorage(null);
  await assert.rejects(
    gate({ url: "/ai/manager/api/system/dashboard", headers: {} }),
    ManagerAuthenticationRequiredError,
  );
  await assert.doesNotReject(
    gate({ url: "/ai/manager/api/auth/login", headers: {} }),
  );
});

test("concurrent 1432 responses run one refresh and retry each request once", async () => {
  let resolveRefresh;
  let refreshCalls = 0;
  const pending = new Promise((resolve) => { resolveRefresh = resolve; });
  const harness = createHttpHarness({
    refreshAccessToken: async () => {
      refreshCalls += 1;
      return pending;
    },
  });
  const first = harness.handler(accessFailure(1432));
  const second = harness.handler(accessFailure(1432, { headers: { "X-Queued": "yes" } }));
  resolveRefresh({ refresh_token: "refresh-rotated", access_token: "access-rotated" });
  await Promise.all([first, second]);
  assert.equal(refreshCalls, 1);
  assert.equal(harness.calls.retry.length, 2);
  assert.deepEqual(
    harness.calls.retry.map((request) => request.headers.Authorization),
    ["Bearer access-rotated", "Bearer access-rotated"],
  );
  assert.equal(harness.calls.retry.every((request) => request._retry), true);
});

test("1431 never refreshes; 1434 and 1436 retain the session", async () => {
  for (const code of [1431, 1434, 1436]) {
    const harness = createHttpHarness();
    const failure = accessFailure(code);
    await assert.rejects(harness.handler(failure), (error) => error === failure);
    assert.equal(harness.calls.refresh, 0);
    assert.equal(harness.record.refresh_token, "refresh-current");
    assert.equal(harness.calls.redirects, 0);
  }
});

test("1433 and 1435 adopt a newer record or invalidate the attempted session", async () => {
  for (const code of [1433, 1435]) {
    const winner = createHttpHarness();
    winner.record = sessionRecord(2, "winner");
    await winner.handler(accessFailure(code));
    assert.equal(winner.calls.retry[0].headers.Authorization, "Bearer access-winner");
    assert.equal(winner.calls.refresh, 0);

    const stale = createHttpHarness();
    const failure = accessFailure(code);
    await assert.rejects(stale.handler(failure), (error) => error === failure);
    assert.equal(stale.record, null);
    assert.equal(stale.lifecycle, "anonymous");
    assert.equal(stale.calls.redirects, 1);
  }
});

test("1444 adopts the cross-tab winner without a second refresh", async () => {
  const harness = createHttpHarness({
    refreshAccessToken: async () => {
      harness.record = sessionRecord(2, "winner");
      throw authError(1444);
    },
  });
  await harness.handler(accessFailure(1432));
  assert.equal(harness.calls.retry.length, 1);
  assert.equal(harness.calls.retry[0].headers.Authorization, "Bearer access-winner");
  assert.equal(harness.record.refresh_token, "refresh-winner");
  assert.equal(harness.calls.redirects, 0);
});

test("1444 without a winner invalidates, while storage deletion rejects the queue", async () => {
  const replay = createHttpHarness({
    refreshAccessToken: async () => { throw authError(1444); },
  });
  await assert.rejects(replay.handler(accessFailure(1432)), (error) =>
    error === undefined ? false : true,
  );
  assert.equal(replay.record, null);
  assert.equal(replay.calls.redirects, 1);

  let resolveRefresh;
  const pending = new Promise((resolve) => { resolveRefresh = resolve; });
  const deleted = createHttpHarness({ refreshAccessToken: async () => pending });
  const first = deleted.handler(accessFailure(1432));
  const queued = deleted.handler(accessFailure(1432));
  deleted.emitStorage(null);
  await assert.rejects(queued, ManagerAuthenticationRequiredError);
  resolveRefresh({ refresh_token: "refresh-late", access_token: "access-late" });
  await assert.rejects(first, ManagerAuthenticationRequiredError);
  assert.equal(deleted.calls.retry.length, 0);
  assert.equal(deleted.lifecycle, "anonymous");
});

test("a retried request cannot enter another refresh loop", async () => {
  const harness = createHttpHarness();
  const failure = accessFailure(1432, { _retry: true });
  await assert.rejects(harness.handler(failure), (error) => error === failure);
  assert.equal(harness.calls.refresh, 0);
});

test("login form delegates session ownership and reports failures", async () => {
  const redirects = [];
  const form = useLoginForm({
    login: async (password) => {
      if (password !== "correct") throw new Error("invalid");
    },
    errorForCode: () => "translated:loginPage.loginFailed",
    onUninitialized: () => {},
    onSuccess: () => redirects.push("Dashboard"),
  });
  form.password.value = "correct";
  await form.handleLogin();
  assert.deepEqual(redirects, ["Dashboard"]);
  form.password.value = "wrong";
  await form.handleLogin();
  assert.equal(form.error.value, "translated:loginPage.loginFailed");

  const loginPageSource = await readFile(new URL("src/pages/login/LoginPage.vue", ROOT), "utf8");
  const loginFormSource = await readFile(new URL("src/pages/login/components/LoginForm.vue", ROOT), "utf8");
  assert.doesNotMatch(loginPageSource, /localStorage|authTokens|refresh_token/);
  assert.doesNotMatch(loginFormSource, /localStorage|authTokens|refresh_token/);
});

test("login route uses page entries and removes the legacy top-level page", async () => {
  const routerSource = await readFile(new URL("src/router/index.ts", ROOT), "utf8");
  assert.match(routerSource, /pages\/login\/LoginPage\.vue/);
  assert.match(routerSource, /pages\/bootstrap\/BootstrapPage\.vue/);
  assert.equal(routerSource.includes("@/pages/Login.vue"), false);
  await assert.rejects(access(new URL("src/pages/Login.vue", ROOT)), /ENOENT/);
});

test("desktop and mobile layouts expose guarded current and all-session logout", async () => {
  const layout = await readFile(new URL("src/layouts/DefaultLayout.vue", ROOT), "utf8");
  assert.match(layout, /logout as logoutSession, logoutAll/);
  assert.match(layout, /confirm\(\{[\s\S]*logoutAllConfirmTitle/);
  assert.match(layout, /logoutLocalOnlyTitle/);
  assert.match(layout, /logoutAllFailedTitle/);
  assert.equal((layout.match(/@click="handleLogoutAll"/g) ?? []).length, 2);
  assert.equal((layout.match(/:disabled="isLoggingOut \|\| isLoggingOutAll"/g) ?? []).length, 4);
  assert.doesNotMatch(layout, /window\.confirm/);
});
