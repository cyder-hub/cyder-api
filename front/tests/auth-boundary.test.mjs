import test from "node:test";
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";

import { createAuthSessionActions } from "../src/services/authSession.ts";
import {
  applyManagerAuthBrowserHeaders,
  createHttpAuthRefreshHandler,
  createProtectedManagerRequestGate,
  ManagerAuthenticationRequiredError,
} from "../src/services/httpAuthRefresh.ts";
import { useLoginForm } from "../src/pages/login/composables/useLoginForm.ts";

const ROOT = new URL("../", import.meta.url);

function authError(code, status = 401) {
  return { response: { status, data: { code } } };
}

function createStore(lifecycle = "unknown", accessToken = null) {
  return {
    accessToken,
    lifecycle,
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
  const store = createStore(
    overrides.lifecycle ?? "unknown",
    overrides.initialAccess ?? null,
  );
  let accessToken = overrides.initialAccess ?? null;
  const calls = {
    cleanup: 0,
    requestAccess: 0,
    setAccess: [],
    clearAccess: 0,
    logout: 0,
    logoutAll: 0,
    events: [],
    navigations: 0,
  };

  const actions = createAuthSessionActions({
    getAuthStore: () => store,
    getAccessToken: () => accessToken,
    setAccessToken: (token) => {
      accessToken = token;
      calls.setAccess.push(token);
    },
    clearAccessToken: () => {
      accessToken = null;
      calls.clearAccess += 1;
    },
    clearLegacyAuthStorage: () => {
      calls.cleanup += 1;
    },
    requestAccess:
      overrides.requestAccess ??
      (async () => {
        calls.requestAccess += 1;
        return { access_token: "access-recovered" };
      }),
    loginWithPassword:
      overrides.loginWithPassword ??
      (async () => ({ access_token: "access-login" })),
    bootstrapWithPassword:
      overrides.bootstrapWithPassword ??
      (async () => ({ access_token: "access-bootstrap" })),
    rotateManagerPassword:
      overrides.rotateManagerPassword ??
      (async () => ({ access_token: "access-rotated-password" })),
    logoutRequest:
      overrides.logoutRequest ??
      (async () => {
        calls.logout += 1;
      }),
    logoutAllRequest:
      overrides.logoutAllRequest ??
      (async () => {
        calls.logoutAll += 1;
        return { revoked_sessions: 2 };
      }),
    announceSessionChanged: () => calls.events.push("session_changed"),
    announceSessionRevoked: () => calls.events.push("session_revoked"),
    onSessionRevoked: () => {
      calls.navigations += 1;
    },
  });

  return {
    actions,
    calls,
    store,
    get accessToken() {
      return accessToken;
    },
  };
}

function createHttpHarness(overrides = {}) {
  let lifecycle = overrides.lifecycle ?? "authenticated";
  let accessToken =
    overrides.accessToken === undefined
      ? "access-current"
      : overrides.accessToken;
  const calls = { recover: 0, retry: [], cleared: 0 };
  const deps = {
    getAccessToken: () => accessToken,
    getLifecycle: () => lifecycle,
    restoreSession:
      overrides.restoreSession ??
      (async () => {
        lifecycle = "authenticated";
        accessToken ??= "access-restored";
        return true;
      }),
    recoverAccess:
      overrides.recoverAccess ??
      (async () => {
        calls.recover += 1;
        accessToken = "access-recovered";
        lifecycle = "authenticated";
        return accessToken;
      }),
    revokeSession: () => {
      calls.cleared += 1;
      accessToken = null;
      lifecycle = "anonymous";
    },
    retryRequest: async (request) => {
      calls.retry.push({ ...request, headers: { ...request.headers } });
      return { retried: true };
    },
  };
  return {
    calls,
    deps,
    handler: createHttpAuthRefreshHandler(deps),
    get lifecycle() {
      return lifecycle;
    },
    get accessToken() {
      return accessToken;
    },
  };
}

function accessFailure(code, request = {}) {
  return {
    response: { status: 401, data: { code } },
    config: {
      url: "/ai/manager/api/system/dashboard",
      headers: { Authorization: "Bearer access-current" },
      ...request,
    },
  };
}

test("startup cleanup runs once before any access recovery", async () => {
  let cleanupObserved = false;
  const harness = createAuthHarness({
    requestAccess: async () => {
      cleanupObserved = harness.calls.cleanup === 1;
      return { access_token: "access-recovered" };
    },
  });
  assert.equal(harness.calls.cleanup, 1);
  assert.equal(await harness.actions.restoreSession(), true);
  assert.equal(cleanupObserved, true);
});

test("login, bootstrap, and password rotation install access-only responses in memory", async () => {
  const harness = createAuthHarness();
  await harness.actions.login("secret");
  await harness.actions.bootstrap("new administrator password");
  await harness.actions.rotatePassword("current password", "new password");
  assert.equal(harness.store.lifecycle, "authenticated");
  assert.equal(harness.accessToken, "access-rotated-password");
  assert.deepEqual(harness.calls.setAccess, [
    "access-login",
    "access-bootstrap",
    "access-rotated-password",
  ]);
  assert.deepEqual(harness.calls.events, [
    "session_changed",
    "session_changed",
    "session_changed",
  ]);
});

test("restoration and forced recovery share one in-tab access promise", async () => {
  let requestCalls = 0;
  let resolveAccess;
  const pending = new Promise((resolve) => {
    resolveAccess = resolve;
  });
  const harness = createAuthHarness({
    requestAccess: async () => {
      requestCalls += 1;
      return pending;
    },
  });

  const first = harness.actions.restoreSession();
  const second = harness.actions.recoverAccess();
  assert.equal(harness.store.lifecycle, "restoring");
  resolveAccess({ access_token: "access-shared" });
  assert.deepEqual(await Promise.all([first, second]), [true, "access-shared"]);
  assert.equal(requestCalls, 1);
  assert.equal(harness.accessToken, "access-shared");
});

test("revocation discards pending access success and failure without restoring stale state", async () => {
  for (const outcome of ["success", "failure"]) {
    let settleAccess;
    const pending = new Promise((resolve, reject) => {
      settleAccess = outcome === "success" ? resolve : reject;
    });
    const harness = createAuthHarness({
      lifecycle: "authenticated",
      initialAccess: "access-existing",
      requestAccess: async () => pending,
    });

    const recovery = harness.actions.recoverAccess();
    harness.actions.revokeLocalSession();
    if (outcome === "success") {
      settleAccess({ access_token: "access-stale" });
      await assert.rejects(recovery, /lifecycle changed/);
    } else {
      const failure = authError(1443, 503);
      settleAccess(failure);
      await assert.rejects(recovery, (error) => error === failure);
    }

    assert.equal(harness.store.lifecycle, "anonymous");
    assert.equal(harness.accessToken, null);
    assert.deepEqual(harness.calls.setAccess, []);
  }
});

test("session change starts a new recovery and stale completion cannot clear it", async () => {
  const resolvers = [];
  const harness = createAuthHarness({
    lifecycle: "authenticated",
    initialAccess: "access-existing",
    requestAccess: () =>
      new Promise((resolve) => {
        resolvers.push(resolve);
      }),
  });

  const staleRecovery = harness.actions.recoverAccess();
  harness.actions.invalidateAccessRecovery();
  const currentRecovery = harness.actions.recoverAccess();
  assert.equal(resolvers.length, 2);

  resolvers[0]({ access_token: "access-stale" });
  await assert.rejects(staleRecovery, /lifecycle changed/);
  assert.deepEqual(harness.calls.setAccess, []);

  resolvers[1]({ access_token: "access-current-generation" });
  assert.equal(await currentRecovery, "access-current-generation");
  assert.equal(harness.store.lifecycle, "authenticated");
  assert.equal(harness.accessToken, "access-current-generation");
});

test("definitive mediator failure becomes anonymous while 503 preserves lifecycle", async () => {
  const invalid = createAuthHarness({
    requestAccess: async () => {
      throw authError(1441);
    },
  });
  assert.equal(await invalid.actions.restoreSession(), false);
  assert.equal(invalid.store.lifecycle, "anonymous");
  assert.equal(invalid.accessToken, null);

  const transient = createAuthHarness({
    requestAccess: async () => {
      throw authError(1443, 503);
    },
  });
  assert.equal(await transient.actions.restoreSession(), false);
  assert.equal(transient.store.lifecycle, "unknown");

  const authenticated = createAuthHarness({
    lifecycle: "authenticated",
    initialAccess: "access-existing",
    requestAccess: async () => {
      throw authError(1443, 503);
    },
  });
  await assert.rejects(authenticated.actions.recoverAccess());
  assert.equal(authenticated.store.lifecycle, "authenticated");
  assert.equal(authenticated.accessToken, "access-existing");
});

test("current logout always clears memory; logout all clears only after success", async () => {
  const local = createAuthHarness({
    lifecycle: "authenticated",
    initialAccess: "access-existing",
    logoutRequest: async () => {
      throw new Error("offline");
    },
  });
  assert.deepEqual(await local.actions.logout(), {
    serverRevocationConfirmed: false,
  });
  assert.equal(local.store.lifecycle, "anonymous");
  assert.equal(local.accessToken, null);
  assert.deepEqual(local.calls.events, ["session_revoked"]);

  const all = createAuthHarness({
    lifecycle: "authenticated",
    initialAccess: "access-existing",
  });
  assert.deepEqual(await all.actions.logoutAll(), { revoked_sessions: 2 });
  assert.equal(all.store.lifecycle, "anonymous");
  assert.deepEqual(all.calls.events, ["session_revoked"]);

  const unavailable = createAuthHarness({
    lifecycle: "authenticated",
    initialAccess: "access-existing",
    logoutAllRequest: async () => {
      throw authError(1451, 503);
    },
  });
  await assert.rejects(unavailable.actions.logoutAll());
  assert.equal(unavailable.store.lifecycle, "authenticated");
  assert.equal(unavailable.accessToken, "access-existing");
  assert.deepEqual(unavailable.calls.events, []);
});

test("protected request gate restores memory access and bypasses mediator endpoints", async () => {
  const harness = createHttpHarness({
    lifecycle: "restoring",
    accessToken: null,
  });
  const gate = createProtectedManagerRequestGate(harness.deps);
  const request = await gate({
    url: "/ai/manager/api/system/dashboard",
    headers: {},
  });
  assert.equal(request.headers.Authorization, "Bearer access-restored");

  const anonymous = createHttpHarness({
    lifecycle: "anonymous",
    accessToken: null,
  });
  await assert.rejects(
    createProtectedManagerRequestGate(anonymous.deps)({
      url: "/ai/manager/api/system/dashboard",
      headers: {},
    }),
    ManagerAuthenticationRequiredError,
  );
  for (const url of [
    "/ai/manager/api/auth/login",
    "/ai/manager/api/auth/access",
    "/ai/manager/api/auth/logout",
  ]) {
    await assert.doesNotReject(gate({ url, headers: {} }));
  }
});

test("manager auth POST requests receive the browser boundary headers", () => {
  const authRequest = {
    method: "post",
    url: "/ai/manager/api/auth/access",
    headers: {},
  };
  applyManagerAuthBrowserHeaders(authRequest);
  assert.equal(authRequest.headers["Content-Type"], "application/json");
  assert.equal(authRequest.headers["X-Cyder-Manager-Auth"], "1");

  const ordinary = {
    method: "get",
    url: "/ai/manager/api/system/dashboard",
    headers: {},
  };
  applyManagerAuthBrowserHeaders(ordinary);
  assert.deepEqual(ordinary.headers, {});
});

test("concurrent 1432 responses perform one recovery and replay each request once", async () => {
  let resolveAccess;
  let recoverCalls = 0;
  const pending = new Promise((resolve) => {
    resolveAccess = resolve;
  });
  const harness = createHttpHarness({
    recoverAccess: async () => {
      recoverCalls += 1;
      return pending;
    },
  });
  const first = harness.handler(accessFailure(1432));
  const second = harness.handler(
    accessFailure(1432, { headers: { "X-Queued": "yes" } }),
  );
  resolveAccess("access-recovered");
  await Promise.all([first, second]);
  assert.equal(recoverCalls, 1);
  assert.equal(harness.calls.retry.length, 2);
  assert.deepEqual(
    harness.calls.retry.map((request) => request.headers.Authorization),
    ["Bearer access-recovered", "Bearer access-recovered"],
  );
  assert.equal(harness.calls.retry.every((request) => request._retry), true);
});

test("stale access 401 recovers through the mediator and replays the request", async () => {
  for (const code of [1433, 1435]) {
    const harness = createHttpHarness();
    assert.deepEqual(await harness.handler(accessFailure(code)), {
      retried: true,
    });
    assert.equal(harness.calls.recover, 1);
    assert.equal(harness.calls.cleared, 0);
    assert.equal(harness.lifecycle, "authenticated");
    assert.equal(
      harness.calls.retry[0].headers.Authorization,
      "Bearer access-recovered",
    );
  }
});

test("mediator recovery only revokes stale access after a definitive failure", async () => {
  for (const recoveryCode of [1441, 1444]) {
    const recoveryFailure = authError(recoveryCode);
    const invalid = createHttpHarness({
      recoverAccess: async () => {
        throw recoveryFailure;
      },
    });
    await assert.rejects(
      invalid.handler(accessFailure(recoveryCode === 1441 ? 1433 : 1435)),
      (error) => error === recoveryFailure,
    );
    assert.equal(invalid.lifecycle, "anonymous");
    assert.equal(invalid.calls.cleared, 1);
  }

  for (const accessCode of [1432, 1433, 1435]) {
    const unavailable = createHttpHarness({
      recoverAccess: async () => {
        throw authError(1443, 503);
      },
    });
    await assert.rejects(unavailable.handler(accessFailure(accessCode)));
    assert.equal(unavailable.lifecycle, "authenticated");
    assert.equal(unavailable.calls.cleared, 0);
  }
});

test("a retried or auth endpoint request cannot enter an access recovery loop", async () => {
  const harness = createHttpHarness();
  const retried = accessFailure(1432, { _retry: true });
  await assert.rejects(harness.handler(retried), (error) => error === retried);
  const accessEndpoint = accessFailure(1432, {
    url: "/ai/manager/api/auth/access",
  });
  await assert.rejects(
    harness.handler(accessEndpoint),
    (error) => error === accessEndpoint,
  );
  assert.equal(harness.calls.recover, 0);
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

test("login route uses page entries and removes the legacy top-level page", async () => {
  const routerSource = await readFile(
    new URL("src/router/index.ts", ROOT),
    "utf8",
  );
  assert.match(routerSource, /pages\/login\/LoginPage\.vue/);
  assert.match(routerSource, /pages\/bootstrap\/BootstrapPage\.vue/);
  assert.equal(routerSource.includes("@/pages/Login.vue"), false);
  await assert.rejects(access(new URL("src/pages/Login.vue", ROOT)), /ENOENT/);
});

test("runtime auth sources contain no refresh bearer or persistent access contract", async () => {
  const sources = await Promise.all(
    [
      "src/services/auth.ts",
      "src/services/authSession.ts",
      "src/services/http.ts",
      "src/services/httpAuthRefresh.ts",
      "src/router/index.ts",
    ].map((path) => readFile(new URL(path, ROOT), "utf8")),
  );
  const runtime = sources.join("\n");
  assert.doesNotMatch(runtime, /refresh_token|auth\/refresh_token/);
  assert.doesNotMatch(runtime, /localStorage|sessionStorage|setItem|getItem/);
  assert.doesNotMatch(runtime, /_authRevision|_authRefreshToken/);
  assert.match(runtime, /auth\/access/);
});

test("desktop and mobile layouts expose guarded current and all-session logout", async () => {
  const layout = await readFile(
    new URL("src/layouts/DefaultLayout.vue", ROOT),
    "utf8",
  );
  assert.match(layout, /logout as logoutSession, logoutAll/);
  assert.match(layout, /confirm\(\{[\s\S]*logoutAllConfirmTitle/);
  assert.match(layout, /logoutLocalOnlyTitle/);
  assert.match(layout, /logoutAllFailedTitle/);
  assert.equal((layout.match(/@click="handleLogoutAll"/g) ?? []).length, 2);
  assert.equal(
    (layout.match(/:disabled="isLoggingOut \|\| isLoggingOutAll"/g) ?? [])
      .length,
    4,
  );
  assert.doesNotMatch(layout, /window\.confirm/);
});
