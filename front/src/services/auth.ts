import { useAuthStore } from "@/store/authStore";
import { request } from "./http";
import type { AuthTokenPair, ManagerBootstrapStatus } from "./types";
import { createAuthSessionActions } from "./authSession";
import { registerAuthRestoration } from "./authRuntime";
import {
  clearStoredAuthSessionIfCurrent,
  clearStoredAuthSession,
  persistAuthTokenPair,
  readStoredAuthSession,
} from "./authTokens";

export function refreshToken(refreshToken: string): Promise<AuthTokenPair> {
  return request.post(
    "/ai/manager/api/auth/refresh_token",
    {},
    {
      headers: { Authorization: `Bearer ${refreshToken}` },
    },
  );
}

export function loginWithPassword(password: string): Promise<AuthTokenPair> {
  return request.post("/ai/manager/api/auth/login", { password });
}

export function getBootstrapStatus(): Promise<ManagerBootstrapStatus> {
  return request.get("/ai/manager/api/auth/bootstrap/status");
}

export function bootstrapWithPassword(password: string): Promise<AuthTokenPair> {
  return request.post("/ai/manager/api/auth/bootstrap", { password });
}

export function rotateManagerPassword(
  currentPassword: string,
  newPassword: string,
): Promise<AuthTokenPair> {
  return request.post("/ai/manager/api/auth/password/rotate", {
    current_password: currentPassword,
    new_password: newPassword,
  });
}

export function logoutRequest(): Promise<void> {
  return request.post("/ai/manager/api/auth/logout", {});
}

export function logoutAllRequest(): Promise<void> {
  return request.post("/ai/manager/api/auth/logout_all", {});
}

const authSession = createAuthSessionActions({
  getAuthStore: useAuthStore,
  readStoredAuthSession,
  persistAuthTokenPair,
  clearStoredAuthSession,
  clearStoredAuthSessionIfCurrent,
  refreshToken,
  loginWithPassword,
  bootstrapWithPassword,
  rotateManagerPassword,
  logoutRequest,
  logoutAllRequest,
});

export const {
  restoreStoredSession,
  tryRefreshToken,
  login,
  bootstrap,
  rotatePassword,
  logout,
  logoutAll,
} = authSession;

registerAuthRestoration(restoreStoredSession);
