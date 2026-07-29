import test from "node:test";
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import { effectScope } from "vue";

import { createAuthSessionActions } from "../src/services/authSession.ts";
import {
  applyManagerAuthBrowserHeaders,
  createHttpAuthRefreshHandler,
  createProtectedManagerRequestGate,
  ManagerAuthenticationRequiredError,
} from "../src/services/httpAuthRefresh.ts";
import { useLoginForm } from "../src/pages/login/composables/useLoginForm.ts";

const ROOT = new URL("../", import.meta.url);
const PASSWORD_REAUTH = {
  scope: "secret_governance",
  method: "password",
  verified_until: 1_900_000_000,
};
const TOTP_REAUTH = {
  scope: "secret_governance",
  method: "totp",
  verified_until: 1_900_000_000,
};

function authError(code, status = 401) {
  return { response: { status, data: { code } } };
}

function createStore(
  lifecycle = "unknown",
  accessToken = null,
  totpState = null,
  reauth = null,
) {
  return {
    accessToken,
    lifecycle,
    totpState,
    reauth,
    setRestoring() {
      this.lifecycle = "restoring";
      this.accessToken = null;
      this.totpState = null;
      this.reauth = null;
    },
    setUnknown() {
      this.lifecycle = "unknown";
      this.accessToken = null;
      this.totpState = null;
      this.reauth = null;
    },
    setAuthenticated(token, managerTotpState, managerReauth) {
      this.lifecycle = "authenticated";
      this.accessToken = token;
      this.totpState = managerTotpState;
      this.reauth = managerReauth;
    },
    setAnonymous() {
      this.lifecycle = "anonymous";
      this.accessToken = null;
      this.totpState = null;
      this.reauth = null;
    },
  };
}

function createAuthHarness(overrides = {}) {
  const store = createStore(
    overrides.lifecycle ?? "unknown",
    overrides.initialAccess ?? null,
    overrides.initialTotpState ??
      (overrides.lifecycle === "authenticated" ? "disabled" : null),
    overrides.initialReauth ?? null,
  );
  let accessToken = overrides.initialAccess ?? null;
  const calls = {
    cleanup: 0,
    requestAccess: 0,
    setAccess: [],
    clearAccess: 0,
    logout: 0,
    logoutAll: 0,
    logoutAllArgs: [],
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
        return {
          access_token: "access-recovered",
          totp_state: "enabled",
          reauth: TOTP_REAUTH,
        };
      }),
    loginWithPassword:
      overrides.loginWithPassword ??
      (async () => ({
        state: "authenticated",
        access_token: "access-login",
        totp_state: "disabled",
        reauth: PASSWORD_REAUTH,
      })),
    loginWithTotp:
      overrides.loginWithTotp ??
      (async () => ({
        access_token: "access-login-totp",
        totp_state: "enabled",
        reauth: TOTP_REAUTH,
      })),
    startTotpRecovery:
      overrides.startTotpRecovery ??
      (async () => ({
        recovery_challenge: "recovery-challenge",
        manual_secret: "RECOVERYSECRET",
        otpauth_uri: "otpauth://totp/Cyder:manager",
        expires_in: 600,
      })),
    confirmTotpRecovery:
      overrides.confirmTotpRecovery ??
      (async () => ({
        access_token: "access-recovery",
        totp_state: "enabled",
        reauth: TOTP_REAUTH,
        recovery_codes: ["RECOVERY-CODE"],
      })),
    confirmTotpEnrollmentRequest:
      overrides.confirmTotpEnrollmentRequest ??
      (async () => ({
        access_token: "access-enrolled",
        totp_state: "enabled",
        reauth: {
          ...TOTP_REAUTH,
          method: "password_totp",
        },
        recovery_codes: ["ENROLL-RECOVERY-CODE"],
      })),
    confirmTotpReplacementRequest:
      overrides.confirmTotpReplacementRequest ??
      (async () => ({
        access_token: "access-replaced",
        totp_state: "enabled",
        reauth: {
          ...TOTP_REAUTH,
          method: "password_totp",
        },
        recovery_codes: ["REPLACE-RECOVERY-CODE"],
      })),
    disableTotpRequest:
      overrides.disableTotpRequest ??
      (async () => ({
        access_token: "access-disabled",
        totp_state: "disabled",
        reauth: {
          ...PASSWORD_REAUTH,
          method: "password_totp",
        },
      })),
    bootstrapWithPassword:
      overrides.bootstrapWithPassword ??
      (async () => ({
        access_token: "access-bootstrap",
        totp_state: "disabled",
        reauth: PASSWORD_REAUTH,
      })),
    rotateManagerPassword:
      overrides.rotateManagerPassword ??
      (async () => ({
        access_token: "access-rotated-password",
        totp_state: "disabled",
        reauth: PASSWORD_REAUTH,
      })),
    logoutRequest:
      overrides.logoutRequest ??
      (async () => {
        calls.logout += 1;
      }),
    logoutAllRequest:
      overrides.logoutAllRequest ??
      (async (currentPassword, totpCode) => {
        calls.logoutAll += 1;
        calls.logoutAllArgs.push([currentPassword, totpCode]);
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
      return {
        access_token: "access-recovered",
        totp_state: "enabled",
        reauth: TOTP_REAUTH,
      };
    },
  });
  assert.equal(harness.calls.cleanup, 1);
  assert.equal(await harness.actions.restoreSession(), true);
  assert.equal(cleanupObserved, true);
  assert.equal(harness.store.totpState, "enabled");
  assert.deepEqual(harness.store.reauth, TOTP_REAUTH);
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
  assert.equal(harness.store.totpState, "disabled");
  assert.deepEqual(harness.store.reauth, PASSWORD_REAUTH);
});

test("enabled login and recovery install access only after the TOTP confirmation stage", async () => {
  const harness = createAuthHarness({
    loginWithPassword: async () => ({
      state: "totp_required",
      login_challenge: "login-challenge",
      expires_in: 300,
    }),
  });

  assert.deepEqual(await harness.actions.login("password-stage"), {
    state: "totp_required",
    login_challenge: "login-challenge",
    expires_in: 300,
  });
  assert.equal(harness.accessToken, null);
  assert.equal(harness.store.lifecycle, "unknown");
  assert.deepEqual(harness.calls.events, []);

  await harness.actions.completeTotpLogin("login-challenge", "123456");
  assert.equal(harness.accessToken, "access-login-totp");
  assert.equal(harness.store.totpState, "enabled");
  assert.deepEqual(harness.store.reauth, TOTP_REAUTH);
  assert.deepEqual(harness.calls.events, ["session_changed"]);

  const setup = await harness.actions.startRecovery(
    "re-entered-password",
    "RECOVERY-CODE",
  );
  assert.equal(setup.recovery_challenge, "recovery-challenge");
  assert.equal(harness.accessToken, null);
  assert.equal(harness.store.lifecycle, "anonymous");
  assert.deepEqual(harness.calls.events, [
    "session_changed",
    "session_revoked",
  ]);

  const recovered = await harness.actions.confirmRecovery(
    "recovery-challenge",
    "654321",
  );
  assert.deepEqual(recovered.recovery_codes, ["RECOVERY-CODE"]);
  assert.equal(harness.accessToken, "access-recovery");
  assert.equal(harness.store.totpState, "enabled");
  assert.deepEqual(harness.store.reauth, TOTP_REAUTH);
  assert.deepEqual(harness.calls.events, [
    "session_changed",
    "session_revoked",
    "session_changed",
  ]);
});

test("TOTP lifecycle confirmations rotate in-memory access and return one-time recovery codes", async () => {
  const harness = createAuthHarness({
    lifecycle: "authenticated",
    initialAccess: "access-existing",
    initialTotpState: "disabled",
  });

  const enrolled = await harness.actions.confirmTotpEnrollment(
    "enroll-challenge",
    "123456",
  );
  assert.deepEqual(enrolled.recovery_codes, ["ENROLL-RECOVERY-CODE"]);
  assert.equal(harness.accessToken, "access-enrolled");
  assert.equal(harness.store.totpState, "enabled");
  assert.equal(harness.store.reauth.method, "password_totp");

  const replaced = await harness.actions.confirmTotpReplacement(
    "replace-challenge",
    "234567",
  );
  assert.deepEqual(replaced.recovery_codes, ["REPLACE-RECOVERY-CODE"]);
  assert.equal(harness.accessToken, "access-replaced");
  assert.equal(harness.store.totpState, "enabled");
  assert.equal(harness.store.reauth.method, "password_totp");

  await harness.actions.disableTotp("current-password", "345678");
  assert.equal(harness.accessToken, "access-disabled");
  assert.equal(harness.store.totpState, "disabled");
  assert.equal(harness.store.reauth.method, "password_totp");
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
  resolveAccess({
    access_token: "access-shared",
    totp_state: "enabled",
    reauth: TOTP_REAUTH,
  });
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
      settleAccess({
        access_token: "access-stale",
        totp_state: "enabled",
        reauth: TOTP_REAUTH,
      });
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

  resolvers[0]({
    access_token: "access-stale",
    totp_state: "enabled",
    reauth: TOTP_REAUTH,
  });
  await assert.rejects(staleRecovery, /lifecycle changed/);
  assert.deepEqual(harness.calls.setAccess, []);

  resolvers[1]({
    access_token: "access-current-generation",
    totp_state: "enabled",
    reauth: TOTP_REAUTH,
  });
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
  assert.deepEqual(
    await all.actions.logoutAll("current-password", "123456"),
    { revoked_sessions: 2 },
  );
  assert.deepEqual(all.calls.logoutAllArgs, [
    ["current-password", "123456"],
  ]);
  assert.equal(all.store.lifecycle, "anonymous");
  assert.deepEqual(all.calls.events, ["session_revoked"]);

  const unavailable = createAuthHarness({
    lifecycle: "authenticated",
    initialAccess: "access-existing",
    logoutAllRequest: async () => {
      throw authError(1451, 503);
    },
  });
  await assert.rejects(
    unavailable.actions.logoutAll("current-password", "123456"),
  );
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
    "/ai/manager/api/auth/login/password",
    "/ai/manager/api/auth/login/totp",
    "/ai/manager/api/auth/reauth",
    "/ai/manager/api/auth/recovery/start",
    "/ai/manager/api/auth/recovery/confirm",
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

test("TOTP login and recovery requests use the split endpoints without auth replay", async () => {
  const authService = await readFile(
    new URL("src/services/auth.ts", ROOT),
    "utf8",
  );
  assert.match(authService, /auth\/login\/password/);
  assert.match(authService, /auth\/login\/totp[\s\S]*noAuthRetry/);
  assert.match(authService, /auth\/recovery\/start[\s\S]*noAuthRetry/);
  assert.match(authService, /auth\/recovery\/confirm[\s\S]*noAuthRetry/);
  assert.doesNotMatch(authService, /auth\/login",/);
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
      return {
        state: "authenticated",
        access_token: "access-login",
        totp_state: "disabled",
      };
    },
    completeTotpLogin: async () => {},
    startRecovery: async () => {
      throw new Error("not used");
    },
    confirmRecovery: async () => ({
      access_token: "access-recovery",
      totp_state: "enabled",
      recovery_codes: ["RECOVERY-CODE"],
    }),
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
  form.dispose();

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

test("login flow keeps TOTP and recovery challenges local and handles terminal errors", async () => {
  let totpFailure = authError(1472);
  const calls = { recoveryPassword: null, recoveryCode: null, redirects: 0 };
  const form = useLoginForm({
    login: async () => ({
      state: "totp_required",
      login_challenge: "local-login-challenge",
      expires_in: 300,
    }),
    completeTotpLogin: async () => {
      if (totpFailure) throw totpFailure;
    },
    startRecovery: async (password, recoveryCode) => {
      calls.recoveryPassword = password;
      calls.recoveryCode = recoveryCode;
      return {
        recovery_challenge: "local-recovery-challenge",
        manual_secret: "LOCAL-SECRET",
        otpauth_uri: "otpauth://totp/Cyder:manager",
        expires_in: 600,
      };
    },
    confirmRecovery: async () => ({
      access_token: "access-recovery",
      totp_state: "enabled",
      recovery_codes: ["NEW-RECOVERY-CODE"],
    }),
    errorForCode: (code) => `error:${code}`,
    onUninitialized: () => {},
    onSuccess: () => {
      calls.redirects += 1;
    },
  });

  form.password.value = "password-stage";
  await form.handleLogin();
  assert.equal(form.stage.value, "totp");
  assert.equal(form.password.value, "");
  assert.equal(form.remainingSeconds.value, 300);
  assert.equal(form.loginChallenge, undefined);

  form.totpCode.value = "111111";
  await form.handleLogin();
  assert.equal(form.stage.value, "totp");
  assert.equal(form.totpCode.value, "");
  assert.equal(form.error.value, "error:1472");

  totpFailure = authError(1475, 429);
  await form.handleLogin();
  assert.equal(form.stage.value, "totp");
  assert.equal(form.error.value, "error:1475");

  totpFailure = authError(1477);
  await form.handleLogin();
  assert.equal(form.stage.value, "password");
  assert.equal(form.remainingSeconds.value, 0);
  assert.equal(form.error.value, "error:1477");

  form.beginRecovery();
  form.recoveryPassword.value = "re-entered-password";
  form.recoveryCode.value = "RECOVERY-CODE";
  await form.handleLogin();
  assert.equal(calls.recoveryPassword, "re-entered-password");
  assert.equal(calls.recoveryCode, "RECOVERY-CODE");
  assert.equal(form.recoveryPassword.value, "");
  assert.equal(form.recoveryCode.value, "");
  assert.equal(form.stage.value, "recovery_totp");
  assert.equal(form.recoveryManualSecret.value, "LOCAL-SECRET");
  assert.equal(form.recoveryOtpauthUri.value, "otpauth://totp/Cyder:manager");
  assert.equal(form.recoveryChallenge, undefined);

  form.totpCode.value = "222222";
  await form.handleLogin();
  assert.equal(calls.redirects, 0);
  assert.equal(form.stage.value, "recovery_codes");
  assert.deepEqual(form.recoveryCodes.value, ["NEW-RECOVERY-CODE"]);
  assert.equal(form.recoveryCodesSaved.value, false);
  assert.equal(form.recoveryManualSecret.value, "");
  assert.equal(form.recoveryOtpauthUri.value, "");
  await form.handleLogin();
  assert.equal(calls.redirects, 0);
  form.recoveryCodesSaved.value = true;
  await form.handleLogin();
  assert.equal(calls.redirects, 1);
  assert.deepEqual(form.recoveryCodes.value, []);
  form.dispose();
});

test("login and recovery challenge countdowns start after setup and expire in scope", async (t) => {
  t.mock.timers.enable({
    apis: ["Date", "setInterval"],
    now: 1_000_000,
  });

  const scope = effectScope();
  const form = scope.run(() =>
    useLoginForm({
      login: async () => ({
        state: "totp_required",
        login_challenge: "expiring-login-challenge",
        expires_in: 2,
      }),
      completeTotpLogin: async () => {},
      startRecovery: async () => ({
        recovery_challenge: "expiring-recovery-challenge",
        manual_secret: "LOCAL-SECRET",
        otpauth_uri: "otpauth://totp/Cyder:manager",
        expires_in: 2,
      }),
      confirmRecovery: async () => ({
        access_token: "access-recovery",
        totp_state: "enabled",
        recovery_codes: ["RECOVERY-CODE"],
      }),
      errorForCode: (code) => `error:${code}`,
      onUninitialized: () => {},
      onSuccess: () => {},
    }),
  );
  assert.ok(form);

  await form.handleLogin();
  assert.equal(form.stage.value, "totp");
  assert.equal(form.remainingSeconds.value, 2);
  t.mock.timers.tick(2_000);
  assert.equal(form.stage.value, "password");
  assert.equal(form.remainingSeconds.value, 0);
  assert.equal(form.error.value, "error:1477");

  form.beginRecovery();
  await form.handleLogin();
  assert.equal(form.stage.value, "recovery_totp");
  assert.equal(form.remainingSeconds.value, 2);
  t.mock.timers.tick(2_000);
  assert.equal(form.stage.value, "recovery_credentials");
  assert.equal(form.remainingSeconds.value, 0);
  assert.equal(form.error.value, "error:1477");

  scope.stop();
});

test("unavailable and exhausted login TOTP states remain fail-closed", async () => {
  const unavailable = useLoginForm({
    login: async () => {
      throw authError(1479, 503);
    },
    completeTotpLogin: async () => {},
    startRecovery: async () => {
      throw new Error("not used");
    },
    confirmRecovery: async () => ({
      access_token: "access-recovery",
      totp_state: "enabled",
      recovery_codes: ["RECOVERY-CODE"],
    }),
    errorForCode: (code) => `error:${code}`,
    onUninitialized: () => {},
    onSuccess: () => {},
  });
  unavailable.password.value = "password";
  await unavailable.handleLogin();
  assert.equal(unavailable.stage.value, "password");
  assert.equal(unavailable.error.value, "error:1479");
  unavailable.dispose();

  const exhausted = useLoginForm({
    login: async () => ({
      state: "totp_required",
      login_challenge: "attempts-challenge",
      expires_in: 300,
    }),
    completeTotpLogin: async () => {
      throw authError(1478, 429);
    },
    startRecovery: async () => {
      throw new Error("not used");
    },
    confirmRecovery: async () => ({
      access_token: "access-recovery",
      totp_state: "enabled",
      recovery_codes: ["RECOVERY-CODE"],
    }),
    errorForCode: (code) => `error:${code}`,
    onUninitialized: () => {},
    onSuccess: () => {},
  });
  await exhausted.handleLogin();
  exhausted.totpCode.value = "000000";
  await exhausted.handleLogin();
  assert.equal(exhausted.stage.value, "password");
  assert.equal(exhausted.error.value, "error:1478");
  exhausted.dispose();
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
      "src/store/authStore.ts",
      "src/pages/login/LoginPage.vue",
      "src/pages/login/composables/useLoginForm.ts",
    ].map((path) => readFile(new URL(path, ROOT), "utf8")),
  );
  const runtime = sources.join("\n");
  assert.doesNotMatch(runtime, /refresh_token|auth\/refresh_token/);
  assert.doesNotMatch(runtime, /localStorage|sessionStorage|setItem|getItem/);
  assert.doesNotMatch(runtime, /_authRevision|_authRefreshToken/);
  assert.match(runtime, /auth\/access/);

  const authStore = await readFile(
    new URL("src/store/authStore.ts", ROOT),
    "utf8",
  );
  const coordination = await readFile(
    new URL("src/services/authCoordination.ts", ROOT),
    "utf8",
  );
  assert.doesNotMatch(
    authStore,
    /loginChallenge|recoveryChallenge|manualSecret|otpauth|recoveryCode/,
  );
  assert.doesNotMatch(
    coordination,
    /login_challenge|recovery_challenge|manual_secret|otpauth_uri|totp_code/,
  );
});

test("desktop and mobile layouts keep current logout while centralizing sensitive actions under Security", async () => {
  const layout = await readFile(
    new URL("src/layouts/DefaultLayout.vue", ROOT),
    "utf8",
  );
  const router = await readFile(new URL("src/router/index.ts", ROOT), "utf8");
  const navItems = await readFile(
    new URL("src/router/nav-items.ts", ROOT),
    "utf8",
  );
  assert.match(layout, /logout as logoutSession/);
  assert.match(layout, /logoutLocalOnlyTitle/);
  assert.equal((layout.match(/@click="handleLogout"/g) ?? []).length, 2);
  assert.doesNotMatch(layout, /handleLogoutAll|RotatePasswordDialog/);
  assert.match(layout, /authStore\.totpState === 'disabled'/);
  assert.match(router, /path: "security"[\s\S]*name: "Security"/);
  assert.match(navItems, /path: "\/security"[\s\S]*navKey: "security"/);
  assert.doesNotMatch(layout, /window\.confirm/);
});
