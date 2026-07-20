import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { decideAuthRoute } from "../src/router/auth-state.ts";
import { authErrorCode } from "../src/services/authErrors.ts";
import { validateManagerPassword } from "../src/services/managerPassword.ts";

const ROOT = new URL("../", import.meta.url);

test("manager auth route matrix prioritizes bootstrap state before session restoration", () => {
  const decide = (bootstrapState, routeKind, hasRefreshToken = false, hasAccessToken = false) =>
    decideAuthRoute({ bootstrapState, routeKind, hasRefreshToken, hasAccessToken });

  assert.equal(decide("uninitialized", "protected"), "bootstrap");
  assert.equal(decide("uninitialized", "login"), "bootstrap");
  assert.equal(decide("uninitialized", "bootstrap"), "allow");
  assert.equal(decide("error", "protected"), "bootstrap");
  assert.equal(decide("error", "bootstrap"), "allow");
  assert.equal(decide("ready", "protected"), "login");
  assert.equal(decide("ready", "protected", true), "restore");
  assert.equal(decide("ready", "protected", true, true), "allow");
  assert.equal(decide("ready", "login", true), "restore");
  assert.equal(decide("ready", "login", true, true), "dashboard");
  assert.equal(decide("ready", "bootstrap"), "login");
  assert.equal(decide("ready", "bootstrap", true), "restore");
});

test("manager password policy counts normalized Unicode code points without trimming", () => {
  assert.deepEqual(validateManagerPassword("a".repeat(14)), {
    valid: false,
    reason: "tooShort",
  });
  assert.equal(validateManagerPassword("界".repeat(15)).valid, true);
  assert.equal(validateManagerPassword("界".repeat(128)).valid, true);
  assert.deepEqual(validateManagerPassword("界".repeat(129)), {
    valid: false,
    reason: "tooLong",
  });
  assert.equal(
    validateManagerPassword(`${"a".repeat(14)}e\u0301`).normalized,
    validateManagerPassword(`${"a".repeat(14)}é`).normalized,
  );
  assert.equal(validateManagerPassword(" ".repeat(15)).valid, true);
});

test("auth errors are routed by numeric code rather than server messages", () => {
  assert.equal(authErrorCode({ response: { data: { code: 1401, msg: "changed" } } }), 1401);
  assert.equal(authErrorCode({ response: { data: { code: "1401" } } }), null);
  assert.equal(authErrorCode(new Error("offline")), null);
});

test("bootstrap and rotate views delegate token storage to auth services", async () => {
  const bootstrapPage = await readFile(
    new URL("src/pages/bootstrap/BootstrapPage.vue", ROOT),
    "utf8",
  );
  const rotateDialog = await readFile(
    new URL("src/components/manager/RotatePasswordDialog.vue", ROOT),
    "utf8",
  );
  const authService = await readFile(new URL("src/services/auth.ts", ROOT), "utf8");

  assert.doesNotMatch(bootstrapPage, /localStorage|refresh_token|authTokens/);
  assert.doesNotMatch(rotateDialog, /localStorage|refresh_token|authTokens/);
  assert.match(authService, /auth\/login", \{ password \}/);
  assert.match(authService, /auth\/bootstrap/);
  assert.match(authService, /auth\/password\/rotate/);
  assert.doesNotMatch(authService, /\{ key: password \}/);
  assert.match(
    bootstrapPage,
    /code === 1401[\s\S]*state === "ready"[\s\S]*router\.replace\(\{ name: "Login" \}\)/,
  );
});
