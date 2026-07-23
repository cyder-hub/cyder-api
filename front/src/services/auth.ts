import { useAuthStore } from "@/store/authStore";
import { request } from "./http";
import type {
  LogoutAllResult,
  ManagerAuthAccess,
  ManagerBootstrapStatus,
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

let coordination: ReturnType<typeof createAuthCoordination> | null = null;

export function requestAccess(): Promise<ManagerAuthAccess> {
  return request.post("/ai/manager/api/auth/access", {});
}

export function loginWithPassword(
  password: string,
): Promise<ManagerAuthAccess> {
  return request.post("/ai/manager/api/auth/login", { password });
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
): Promise<ManagerAuthAccess> {
  return request.post("/ai/manager/api/auth/password/rotate", {
    current_password: currentPassword,
    new_password: newPassword,
  });
}

export function logoutRequest(): Promise<void> {
  return request.post("/ai/manager/api/auth/logout", {});
}

export function logoutAllRequest(): Promise<LogoutAllResult> {
  return request.post("/ai/manager/api/auth/logout_all", {});
}

const authSession = createAuthSessionActions({
  getAuthStore: useAuthStore,
  getAccessToken,
  setAccessToken,
  clearAccessToken,
  clearLegacyAuthStorage,
  requestAccess,
  loginWithPassword,
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
  bootstrap,
  rotatePassword,
  logout,
  logoutAll,
} = authSession;

registerAuthRecovery(restoreSession, recoverAccess, revokeAndAnnounce);

export function startAuthCoordination(): void {
  coordination ??= createAuthCoordination({
    recoverAccess,
    invalidateAccessRecovery,
    revokeLocalSession,
  });
}
