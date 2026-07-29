import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const ROOT = new URL("../", import.meta.url);
const source = (path) => readFile(new URL(path, ROOT), "utf8");

function functionSource(contents, name) {
  const start = contents.indexOf(`export function ${name}`);
  assert.notEqual(start, -1, `${name} must exist`);
  const next = contents.indexOf("\nexport function ", start + 1);
  return contents.slice(start, next === -1 ? contents.length : next);
}

test("API key and Provider business services carry no password or TOTP proof", async () => {
  const [apiKeys, providers] = await Promise.all([
    source("src/services/apiKeys.ts"),
    source("src/services/providers.ts"),
  ]);

  for (const name of [
    "createApiKey",
    "rotateApiKey",
    "revealApiKey",
    "deleteApiKey",
  ]) {
    const body = functionSource(apiKeys, name);
    assert.doesNotMatch(body, /totp|password|sensitiveTotpRequestConfig/i);
  }

  const reveal = functionSource(providers, "revealProviderKey");
  assert.doesNotMatch(reveal, /totp|password|sensitiveTotpRequestConfig/i);
  assert.doesNotMatch(
    `${apiKeys}\n${providers}`,
    /X-Cyder-TOTP-Code|totp_code|[?&]totp/i,
  );
});

test("exactly the low-risk Secret governance actions use the shared reauthentication gate", async () => {
  const [governance, detail, provider] = await Promise.all([
    source("src/pages/api-key/composables/useApiKeyGovernance.ts"),
    source("src/pages/api-key/composables/useApiKeyDetail.ts"),
    source("src/pages/provider-edit/components/ProviderApiKeyList.vue"),
  ]);

  for (const call of [
    "createApiKey",
    "rotateApiKey",
    "deleteApiKey",
  ]) {
    assert.match(
      governance,
      new RegExp(
        `runWithSecretGovernanceReauth\\(\\(\\) =>[\\s\\S]*?${call}\\(`,
      ),
    );
  }
  assert.match(
    detail,
    /runWithSecretGovernanceReauth\(\(\) =>[\s\S]*?revealApiKey\(/,
  );
  assert.match(
    provider,
    /runWithSecretGovernanceReauth\(\(\) =>[\s\S]*?revealProviderKey\(/,
  );

  assert.doesNotMatch(
    functionSource(governance, "useApiKeyEditDialog"),
    /runWithSecretGovernanceReauth[\s\S]*updateApiKey\(/,
  );
  for (const mutation of [
    "createProviderKey",
    "replaceProviderKey",
    "updateProviderKey",
    "deleteProviderKey",
  ]) {
    const call = provider.match(
      new RegExp(`${mutation}\\([\\s\\S]*?\\n\\s*\\)`),
    )?.[0];
    assert.ok(call, `${mutation} call must remain present`);
    assert.doesNotMatch(call, /runWithSecretGovernanceReauth|totp/i);
  }
});

test("the action gate accepts a current grant and retries at most once after explicit reauthentication", async () => {
  const gate = await source("src/services/managerReauth.ts");

  assert.match(gate, /reauth\?\.scope === "secret_governance"/);
  assert.match(gate, /reauth\.verified_until \* 1000 > nowMs/);
  assert.match(gate, /pendingPrompt \?\?= presenter\(\)/);
  assert.match(
    gate,
    /if \(authErrorCode\(error\) !== MANAGER_REAUTH_REQUIRED_CODE\) throw error/,
  );
  assert.match(
    gate,
    /store\.clearReauth\(\)[\s\S]*await ensureSecretGovernanceReauth\(\)[\s\S]*return action\(\)/,
  );
  assert.equal(
    (gate.match(/return action\(\)/g) ?? []).length,
    1,
    "the post-reauth branch must replay the action exactly once",
  );
  assert.doesNotMatch(gate, /setTimeout|setInterval|localStorage|sessionStorage/);
});

test("the global dialog stays above business modals, enforces state-selected credentials, and clears every exit", async () => {
  const [dialog, dialogContent, layout, auth] = await Promise.all([
    source("src/components/manager/ManagerReauthDialog.vue"),
    source("src/components/ui/dialog/DialogContent.vue"),
    source("src/layouts/DefaultLayout.vue"),
    source("src/services/auth.ts"),
  ]);

  assert.match(layout, /<ManagerReauthDialog/);
  assert.match(dialogContent, /overlayClass\?: HTMLAttributes\["class"\]/);
  assert.match(dialogContent, /<DialogOverlay :class="props\.overlayClass"/);
  assert.match(dialog, /overlay-class="z-\[200\]"/);
  assert.match(dialog, /class="z-\[201\]/);
  assert.match(dialog, /registerManagerReauthPresenter\(present\)/);
  assert.match(dialog, /const route = useRoute\(\)/);
  assert.match(
    dialog,
    /\(\) => route\.fullPath,[\s\S]*if \(resolvePrompt\) finish\(false\)/,
  );
  assert.match(
    dialog,
    /authStore\.totpState === "enabled"[\s\S]*method: "totp"[\s\S]*method: "password"/,
  );
  assert.match(dialog, /authStore\.totpState !== "unavailable"/);
  assert.match(dialog, /password\.value = ""/);
  assert.match(dialog, /totpCode\.value = ""/);
  assert.match(dialog, /announceAuthorizationChanged\(\)/);
  assert.match(
    dialog,
    /MANAGER_REAUTH_METHOD_CHANGED_CODE[\s\S]*await recoverAccess\(\)/,
  );
  assert.match(auth, /auth\/reauth/);
  assert.match(auth, /auth\/reauth[\s\S]*noAuthRetry/);
  assert.doesNotMatch(
    `${dialog}\n${layout}`,
    /localStorage|sessionStorage|console\./,
  );
});

test("high-risk auth commands keep direct per-command TOTP while Logout All always carries the password", async () => {
  const [auth, passwordDialog, logoutAllDialog] = await Promise.all([
    source("src/services/auth.ts"),
    source("src/components/manager/RotatePasswordDialog.vue"),
    source("src/pages/security/components/LogoutAllDialog.vue"),
  ]);

  assert.match(
    auth,
    /password\/rotate[\s\S]*totpCode \? sensitiveTotpRequestConfig\(totpCode\)/,
  );
  assert.match(
    auth,
    /auth\/logout_all[\s\S]*current_password: currentPassword[\s\S]*totpCode \? sensitiveTotpRequestConfig\(totpCode\)/,
  );
  assert.match(passwordDialog, /totpState === "enabled"/);
  assert.match(logoutAllDialog, /<PasswordField/);
  assert.match(logoutAllDialog, /currentPassword\.value\.length === 0/);
  assert.match(logoutAllDialog, /totpState === "enabled"/);
  assert.match(logoutAllDialog, /currentPassword\.value = ""/);
  assert.match(logoutAllDialog, /totpCode\.value = ""/);
});
