import test from "node:test";
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";

import { createAuthSessionActions } from "../src/services/authSession.ts";
import { createHttpAuthRefreshHandler } from "../src/services/httpAuthRefresh.ts";
import { useLoginForm } from "../src/pages/login/composables/useLoginForm.ts";

const ROOT = new URL("../", import.meta.url);

function createAuthHarness(overrides = {}) {
  const calls = {
    persisted: [],
    cleared: 0,
    clearIfCurrent: [],
    logout: 0,
  };
  const store = {
    accessToken: "access-existing",
    setAccessToken(token) {
      this.accessToken = token;
    },
  };
  let storedRefreshToken = overrides.storedRefreshToken ?? "refresh-current";

  const actions = createAuthSessionActions({
    getAuthStore: () => store,
    readStoredRefreshToken: () => storedRefreshToken,
    persistAuthTokenPair: (tokenPair) => {
      calls.persisted.push(tokenPair);
      storedRefreshToken = tokenPair.refresh_token;
      return tokenPair.access_token;
    },
    clearStoredRefreshToken: () => {
      calls.cleared += 1;
      storedRefreshToken = null;
    },
    clearStoredRefreshTokenIfCurrent: (refreshToken) => {
      calls.clearIfCurrent.push(refreshToken);
      if (storedRefreshToken !== refreshToken) {
        return false;
      }
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
    logoutRequest:
      overrides.logoutRequest ??
      (async () => {
        calls.logout += 1;
      }),
  });

  return {
    actions,
    calls,
    get storedRefreshToken() {
      return storedRefreshToken;
    },
    store,
  };
}

test("login success persists tokens and updates the access token", async () => {
  const harness = createAuthHarness();

  assert.equal(await harness.actions.login("secret"), true);
  assert.equal(harness.store.accessToken, "access-login");
  assert.equal(harness.storedRefreshToken, "refresh-login");
  assert.deepEqual(harness.calls.persisted, [
    { refresh_token: "refresh-login", access_token: "access-login" },
  ]);
});

test("login failure leaves the existing session state unchanged", async () => {
  const harness = createAuthHarness({
    loginWithPassword: async () => {
      throw new Error("bad password");
    },
  });

  assert.equal(await harness.actions.login("wrong"), false);
  assert.equal(harness.store.accessToken, "access-existing");
  assert.equal(harness.storedRefreshToken, "refresh-current");
  assert.deepEqual(harness.calls.persisted, []);
});

test("stored refresh token success rotates the session token pair", async () => {
  const harness = createAuthHarness();

  assert.equal(await harness.actions.tryRefreshToken(), true);
  assert.equal(harness.store.accessToken, "access-rotated");
  assert.equal(harness.storedRefreshToken, "refresh-rotated");
});

test("stored refresh token failure clears the current local session", async () => {
  const harness = createAuthHarness({
    refreshToken: async () => {
      throw new Error("expired refresh");
    },
  });

  assert.equal(await harness.actions.tryRefreshToken(), false);
  assert.equal(harness.store.accessToken, null);
  assert.equal(harness.storedRefreshToken, null);
  assert.deepEqual(harness.calls.clearIfCurrent, ["refresh-current"]);
});

test("logout clears local tokens even when server logout fails", async () => {
  const harness = createAuthHarness({
    logoutRequest: async () => {
      throw new Error("already logged out");
    },
  });

  await harness.actions.logout();

  assert.equal(harness.calls.cleared, 1);
  assert.equal(harness.store.accessToken, null);
  assert.equal(harness.storedRefreshToken, null);
});

test("login form redirects on success and reports failure without token storage access", async () => {
  const redirects = [];
  const form = useLoginForm({
    login: async (password) => password === "correct",
    translate: (key) => `translated:${key}`,
    onSuccess: () => {
      redirects.push("Dashboard");
    },
  });

  form.password.value = "correct";
  await form.handleLogin();
  assert.deepEqual(redirects, ["Dashboard"]);
  assert.equal(form.error.value, null);
  assert.equal(form.isLoading.value, false);

  form.password.value = "wrong";
  await form.handleLogin();
  assert.equal(form.error.value, "translated:loginPage.loginFailed");
  assert.equal(form.isLoading.value, false);

  const loginPageSource = await readFile(
    new URL("src/pages/login/LoginPage.vue", ROOT),
    "utf8",
  );
  const loginFormSource = await readFile(
    new URL("src/pages/login/components/LoginForm.vue", ROOT),
    "utf8",
  );
  assert.doesNotMatch(loginPageSource, /localStorage|authTokens|refresh_token/);
  assert.doesNotMatch(loginFormSource, /localStorage|authTokens|refresh_token/);
});

test("401 refresh queue retries pending requests with a single rotated token", async () => {
  let refreshCalls = 0;
  let resolveRefresh;
  const refreshPromise = new Promise((resolve) => {
    resolveRefresh = resolve;
  });
  const retried = [];
  const handler = createHttpAuthRefreshHandler({
    readStoredRefreshToken: () => "refresh-current",
    persistAuthTokenPair: (tokenPair) => tokenPair.access_token,
    clearStoredRefreshTokenIfCurrent: () => true,
    setAccessToken: () => {},
    refreshAccessToken: async () => {
      refreshCalls += 1;
      return refreshPromise;
    },
    retryRequest: async (request) => {
      retried.push({ ...request, headers: { ...request.headers } });
      return { retried: true };
    },
    redirectToLogin: () => {},
  });

  const first = handler({ response: { status: 401 }, config: { headers: {} } });
  const second = handler({
    response: { status: 401 },
    config: { headers: { "X-Request": "queued" } },
  });

  resolveRefresh({
    refresh_token: "refresh-rotated",
    access_token: "access-rotated",
  });

  await Promise.all([first, second]);

  assert.equal(refreshCalls, 1);
  assert.equal(retried.length, 2);
  assert.deepEqual(
    retried.map((request) => request.headers.Authorization),
    ["Bearer access-rotated", "Bearer access-rotated"],
  );
  assert.equal(retried[0]._retry, true);
  assert.equal(retried[1]._retry, true);
});

test("401 refresh failure clears current token and redirects to login", async () => {
  const refreshError = { response: { status: 401 } };
  const redirects = [];
  const cleared = [];
  let accessToken = "access-current";

  const handler = createHttpAuthRefreshHandler({
    readStoredRefreshToken: () => "refresh-current",
    persistAuthTokenPair: (tokenPair) => tokenPair.access_token,
    clearStoredRefreshTokenIfCurrent: (refreshToken) => {
      cleared.push(refreshToken);
      return true;
    },
    setAccessToken: (token) => {
      accessToken = token;
    },
    refreshAccessToken: async () => {
      throw refreshError;
    },
    retryRequest: async () => {
      throw new Error("retry should not run after refresh failure");
    },
    redirectToLogin: () => {
      redirects.push("Login");
    },
  });

  await assert.rejects(
    handler({ response: { status: 401 }, config: { headers: {} } }),
    (error) => error === refreshError,
  );

  assert.deepEqual(cleared, ["refresh-current"]);
  assert.equal(accessToken, null);
  assert.deepEqual(redirects, ["Login"]);
});

test("login route uses the page directory entry and removes the legacy top-level page", async () => {
  const routerSource = await readFile(new URL("src/router/index.ts", ROOT), "utf8");

  assert.match(routerSource, /pages\/login\/LoginPage\.vue/);
  assert.equal(routerSource.includes("@/pages/Login.vue"), false);
  await assert.rejects(
    access(new URL("src/pages/Login.vue", ROOT)),
    /ENOENT/,
  );
});
