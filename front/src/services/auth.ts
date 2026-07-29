import { useAuthStore } from "@/store/authStore";
import type { AxiosRequestConfig } from "axios";
import { request } from "./http";
import type {
  LogoutAllResult,
  ManagerAuthAccess,
  ManagerBootstrapStatus,
  ManagerPasswordLoginResult,
  ManagerReauthRequest,
  ManagerReauthStatus,
  ManagerTotpLifecycleResult,
  ManagerTotpRecoveryResult,
  ManagerTotpRecoverySetup,
  ManagerTotpSetup,
  ManagerTotpStatus,
} from "./types";
import { createAuthSessionActions } from "./authSession";
import { createAuthCoordination } from "./authCoordination";
import { navigateToManagerLogin, registerAuthRecovery } from "./authRuntime";
import {
  clearAccessToken,
  clearLegacyAuthStorage,
  getAccessToken,
  setAccessToken,
} from "./authTokens";
import { sensitiveTotpRequestConfig } from "./sensitiveTotp";

let coordination: ReturnType<typeof createAuthCoordination> | null = null;

export function requestAccess(): Promise<ManagerAuthAccess> {
  return request.post("/ai/manager/api/auth/access", {});
}

export function reauthenticateManager(
  payload: ManagerReauthRequest,
): Promise<ManagerReauthStatus> {
  return request.post(
    "/ai/manager/api/auth/reauth",
    payload,
    noAuthRetry,
  );
}

export function loginWithPassword(
  password: string,
): Promise<ManagerPasswordLoginResult> {
  return request.post("/ai/manager/api/auth/login/password", { password });
}

export function loginWithTotp(
  loginChallenge: string,
  totpCode: string,
): Promise<ManagerAuthAccess> {
  return request.post(
    "/ai/manager/api/auth/login/totp",
    {
      login_challenge: loginChallenge,
      totp_code: totpCode,
    },
    noAuthRetry,
  );
}

export function startTotpRecovery(
  password: string,
  recoveryCode: string,
): Promise<ManagerTotpRecoverySetup> {
  return request.post(
    "/ai/manager/api/auth/recovery/start",
    {
      password,
      recovery_code: recoveryCode,
    },
    noAuthRetry,
  );
}

export function confirmTotpRecovery(
  recoveryChallenge: string,
  totpCode: string,
): Promise<ManagerTotpRecoveryResult> {
  return request.post(
    "/ai/manager/api/auth/recovery/confirm",
    {
      recovery_challenge: recoveryChallenge,
      totp_code: totpCode,
    },
    noAuthRetry,
  );
}

export function getTotpStatus(): Promise<ManagerTotpStatus> {
  return request.get("/ai/manager/api/auth/totp/status");
}

export function startTotpEnrollment(
  currentPassword: string,
): Promise<ManagerTotpSetup> {
  return request.post("/ai/manager/api/auth/totp/enroll/start", {
    current_password: currentPassword,
  });
}

export function confirmTotpEnrollmentRequest(
  setupChallenge: string,
  totpCode: string,
): Promise<ManagerTotpLifecycleResult> {
  return request.post(
    "/ai/manager/api/auth/totp/enroll/confirm",
    {
      setup_challenge: setupChallenge,
      totp_code: totpCode,
    },
    noAuthRetry,
  );
}

export function startTotpReplacement(
  currentPassword: string,
  currentTotpCode: string,
): Promise<ManagerTotpSetup> {
  return request.post(
    "/ai/manager/api/auth/totp/replace/start",
    { current_password: currentPassword },
    sensitiveTotpRequestConfig(currentTotpCode),
  );
}

export function confirmTotpReplacementRequest(
  setupChallenge: string,
  totpCode: string,
): Promise<ManagerTotpLifecycleResult> {
  return request.post(
    "/ai/manager/api/auth/totp/replace/confirm",
    {
      setup_challenge: setupChallenge,
      totp_code: totpCode,
    },
    noAuthRetry,
  );
}

export function disableTotpRequest(
  currentPassword: string,
  currentTotpCode: string,
): Promise<ManagerTotpLifecycleResult> {
  return request.post(
    "/ai/manager/api/auth/totp/disable",
    { current_password: currentPassword },
    sensitiveTotpRequestConfig(currentTotpCode),
  );
}

export function getBootstrapStatus(): Promise<ManagerBootstrapStatus> {
  return request.get("/ai/manager/api/auth/bootstrap/status");
}

export function bootstrapWithPassword(
  password: string,
): Promise<ManagerAuthAccess> {
  return request.post("/ai/manager/api/auth/bootstrap", { password });
}

export function rotateManagerPassword(
  currentPassword: string,
  newPassword: string,
  totpCode?: string,
): Promise<ManagerAuthAccess> {
  return request.post(
    "/ai/manager/api/auth/password/rotate",
    {
      current_password: currentPassword,
      new_password: newPassword,
    },
    totpCode ? sensitiveTotpRequestConfig(totpCode) : undefined,
  );
}

export function logoutRequest(): Promise<void> {
  return request.post("/ai/manager/api/auth/logout", {});
}

export function logoutAllRequest(
  currentPassword: string,
  totpCode?: string,
): Promise<LogoutAllResult> {
  return request.post(
    "/ai/manager/api/auth/logout_all",
    { current_password: currentPassword },
    totpCode ? sensitiveTotpRequestConfig(totpCode) : noAuthRetry,
  );
}

const authSession = createAuthSessionActions({
  getAuthStore: useAuthStore,
  getAccessToken,
  setAccessToken,
  clearAccessToken,
  clearLegacyAuthStorage,
  requestAccess,
  loginWithPassword,
  loginWithTotp,
  startTotpRecovery,
  confirmTotpRecovery,
  confirmTotpEnrollmentRequest,
  confirmTotpReplacementRequest,
  disableTotpRequest,
  bootstrapWithPassword,
  rotateManagerPassword,
  logoutRequest,
  logoutAllRequest,
  announceSessionChanged: () => coordination?.announceSessionChanged(),
  announceSessionRevoked: () => coordination?.announceSessionRevoked(),
  onSessionRevoked: navigateToManagerLogin,
});

export const {
  restoreSession,
  recoverAccess,
  invalidateAccessRecovery,
  revokeLocalSession,
  revokeAndAnnounce,
  login,
  completeTotpLogin,
  startRecovery,
  confirmRecovery,
  confirmTotpEnrollment,
  confirmTotpReplacement,
  disableTotp,
  bootstrap,
  rotatePassword,
  logout,
  logoutAll,
} = authSession;

const noAuthRetry = {
  _skipAuthRetry: true,
} as AxiosRequestConfig;

registerAuthRecovery(restoreSession, recoverAccess, revokeAndAnnounce);

export function startAuthCoordination(): void {
  coordination ??= createAuthCoordination({
    recoverAccess,
    invalidateAccessRecovery,
    revokeLocalSession,
  });
}

export function announceAuthorizationChanged(): void {
  coordination?.announceAuthorizationChanged();
}
