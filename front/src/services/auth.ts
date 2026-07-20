import { useAuthStore } from "@/store/authStore";
import { request } from "./http";
import type { AuthTokenPair, ManagerBootstrapStatus } from "./types";
import { createAuthSessionActions } from "./authSession";
import {
  clearStoredRefreshTokenIfCurrent,
  clearStoredRefreshToken,
  persistAuthTokenPair,
  readStoredRefreshToken,
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

const authSession = createAuthSessionActions({
  getAuthStore: useAuthStore,
  readStoredRefreshToken,
  persistAuthTokenPair,
  clearStoredRefreshToken,
  clearStoredRefreshTokenIfCurrent,
  refreshToken,
  loginWithPassword,
  bootstrapWithPassword,
  rotateManagerPassword,
  logoutRequest,
});

export const { tryRefreshToken, login, bootstrap, rotatePassword, logout } = authSession;
