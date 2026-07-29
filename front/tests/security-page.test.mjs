import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import {
  MANAGER_TOTP_CODE_HEADER,
  sensitiveTotpRequestConfig,
} from "../src/services/sensitiveTotp.ts";
import {
  effectiveManagerTotpState,
  reconcileManagerTotpStatus,
} from "../src/pages/security/totpStatus.ts";

const ROOT = new URL("../", import.meta.url);

const source = (path) => readFile(new URL(path, ROOT), "utf8");

test("sensitive TOTP request configuration is isolated per request and disables auth replay", () => {
  const first = sensitiveTotpRequestConfig("123456");
  const second = sensitiveTotpRequestConfig("654321");

  assert.equal(MANAGER_TOTP_CODE_HEADER, "X-Cyder-TOTP-Code");
  assert.equal(first.headers[MANAGER_TOTP_CODE_HEADER], "123456");
  assert.equal(second.headers[MANAGER_TOTP_CODE_HEADER], "654321");
  assert.equal(first._skipAuthRetry, true);
  assert.equal(second._skipAuthRetry, true);
  assert.notEqual(first, second);
  assert.notEqual(first.headers, second.headers);
});

test("auth service wires lifecycle endpoints and protects only current-code requests", async () => {
  const auth = await source("src/services/auth.ts");

  for (const endpoint of [
    "totp/enroll/start",
    "totp/enroll/confirm",
    "totp/replace/start",
    "totp/replace/confirm",
    "totp/disable",
  ]) {
    assert.match(auth, new RegExp(endpoint.replaceAll("/", "\\/")));
  }
  assert.match(
    auth,
    /totp\/replace\/start[\s\S]*sensitiveTotpRequestConfig\(currentTotpCode\)/,
  );
  assert.match(
    auth,
    /totp\/disable[\s\S]*sensitiveTotpRequestConfig\(currentTotpCode\)/,
  );
  assert.match(
    auth,
    /password\/rotate[\s\S]*totpCode \? sensitiveTotpRequestConfig\(totpCode\)/,
  );
  assert.match(
    auth,
    /auth\/logout_all[\s\S]*totpCode \? sensitiveTotpRequestConfig\(totpCode\)/,
  );
});

test("Security page exposes disabled, enabled, and unavailable actions in one operator section", async () => {
  const page = await source("src/pages/security/SecurityPage.vue");

  assert.match(page, /app-page-shell app-page-shell--narrow/);
  assert.match(page, /totpState === 'disabled'/);
  assert.match(page, /totpState === 'enabled'/);
  assert.match(page, /totpState === 'unavailable'/);
  assert.match(page, /openSetup\('enroll'\)/);
  assert.match(page, /openSetup\('replace'\)/);
  assert.match(page, /disableOpen = true/);
  assert.match(page, /:disabled="totpState === 'unavailable'"/);
  assert.match(page, /RotatePasswordDialog/);
  assert.match(page, /LogoutAllDialog/);
});

test("Security page reconciles loaded status with refreshed cross-tab auth state", () => {
  const staleDisabled = { state: "disabled" };
  const staleEnabled = { state: "enabled", enabled_at: 1_800_000_000 };

  assert.equal(
    effectiveManagerTotpState("enabled", staleDisabled),
    "enabled",
  );
  assert.deepEqual(
    reconcileManagerTotpStatus("enabled", staleDisabled),
    { state: "enabled" },
  );
  assert.deepEqual(
    reconcileManagerTotpStatus("disabled", staleEnabled),
    { state: "disabled" },
  );
  assert.equal(
    reconcileManagerTotpStatus("enabled", staleEnabled),
    staleEnabled,
  );
});

test("setup drawer renders a raw QR matrix without HTML injection and clears ephemeral material", async () => {
  const drawer = await source(
    "src/pages/security/components/TotpSetupDrawer.vue",
  );
  const matrix = await source(
    "src/pages/security/components/TotpQrMatrix.vue",
  );

  assert.match(
    drawer,
    /encodeQR\(result\.otpauth_uri, "raw", \{[\s\S]*ecc: "medium",[\s\S]*border: 4/,
  );
  assert.doesNotMatch(drawer, /qr\/decode|qr\/dom|decodeQR|DOMParser|v-html/);
  assert.doesNotMatch(matrix, /v-html|DOMParser/);
  assert.match(matrix, /shape-rendering="crispEdges"/);
  assert.match(drawer, /manualSecret\.value = ""/);
  assert.match(drawer, /otpauthUri\.value = ""/);
  assert.match(drawer, /qrMatrix\.value = \[\]/);
  assert.match(drawer, /recoveryCodes\.value = \[\]/);
  assert.match(drawer, /onBeforeRouteLeave\(\(\) => reset\(\)\)/);
  assert.match(drawer, /onBeforeUnmount\(reset\)/);
  assert.match(drawer, /isDesktop \? 'right' : 'bottom'/);
  assert.match(drawer, /min-h-0 flex-1 overflow-y-auto/);
});

test("recovery plaintext requires saved confirmation and download URLs are promptly revoked", async () => {
  const drawer = await source(
    "src/pages/security/components/TotpSetupDrawer.vue",
  );

  assert.match(drawer, /<Checkbox v-model="recoveryCodesSaved"/);
  assert.match(drawer, /:disabled="!recoveryCodesSaved"/);
  assert.match(
    drawer,
    /stage\.value === "recovery_codes" && !recoveryCodesSaved\.value/,
  );
  assert.match(drawer, /new Blob\(/);
  assert.match(drawer, /URL\.createObjectURL/);
  assert.match(drawer, /URL\.revokeObjectURL/);
  assert.match(drawer, /setTimeout\(revokeDownloadUrl, 0\)/);
  assert.doesNotMatch(
    drawer,
    /localStorage|sessionStorage|console\.|v-html/,
  );
});

test("lost-device recovery shows new codes before navigation and requires saved confirmation", async () => {
  const form = await source("src/pages/login/components/LoginForm.vue");
  const state = await source(
    "src/pages/login/composables/useLoginForm.ts",
  );

  assert.match(state, /stage\.value = "recovery_codes"/);
  assert.match(state, /recoveryCodes\.value = \[\.\.\.result\.recovery_codes\]/);
  assert.match(state, /if \(!recoveryCodesSaved\.value\) return/);
  assert.match(form, /v-for="code in recoveryCodes"/);
  assert.match(form, /<Checkbox v-model="recoveryCodesSavedModel"/);
  assert.match(
    form,
    /stage === 'recovery_codes' && !recoveryCodesSaved/,
  );
  assert.match(form, /copyRecoveryCodes/);
  assert.match(form, /downloadRecoveryCodes/);
  assert.match(form, /URL\.revokeObjectURL/);
  assert.doesNotMatch(form, /localStorage|sessionStorage|console\./);
});

test("password and all-session dialogs collect TOTP only when enabled and fail closed when unavailable", async () => {
  const password = await source(
    "src/components/manager/RotatePasswordDialog.vue",
  );
  const logoutAll = await source(
    "src/pages/security/components/LogoutAllDialog.vue",
  );

  for (const dialog of [password, logoutAll]) {
    assert.match(dialog, /totpState === "unavailable"|totpState === 'unavailable'/);
    assert.match(dialog, /totpState === "enabled"|totpState === 'enabled'/);
    assert.match(dialog, /totpCode\.value = ""/);
    assert.doesNotMatch(dialog, /localStorage|sessionStorage|console\./);
  }
  assert.match(
    password,
    /props\.totpState === "enabled" \? totpCode\.value : undefined/,
  );
  assert.match(
    logoutAll,
    /props\.totpState === "enabled" \? totpCode\.value : undefined/,
  );
});

test("shared TOTP field preserves native autofill and paste behind six styled slots", async () => {
  const field = await source("src/pages/login/components/TotpField.vue");

  assert.match(field, /autocomplete="one-time-code"/);
  assert.match(field, /name="totp"/);
  assert.match(field, /inputmode="numeric"/);
  assert.doesNotMatch(field, /opacity-0/);
  assert.match(field, /v-for="\(digit, index\) in digits"/);
  assert.match(field, /activeIndex === index/);
  assert.match(field, /Array\.from\(/);
  assert.match(field, /@paste="handlePaste"/);
  assert.match(field, /const handlePaste = \(event: ClipboardEvent\)/);
  assert.match(field, /event\.clipboardData\?\.getData\("text"\)/);
  assert.match(field, /event\.preventDefault\(\)/);
  assert.match(field, /const normalize = \(event: Event\)/);
  assert.match(field, /event\.target as HTMLInputElement/);
  assert.match(field, /value\.replace\(\/\\D\/g, ""\)\.slice\(0, CODE_LENGTH\)/);
  assert.match(field, /input\.value = normalized/);
  assert.match(field, /model\.value = normalized/);
});
