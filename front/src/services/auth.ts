import { useAuthStore } from "@/store/authStore";
import { request } from "./http";
import type { AuthTokenPair } from "./types";
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
  return request.post("/ai/manager/api/auth/login", { key: password });
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
  logoutRequest,
});

export const { tryRefreshToken, login, logout } = authSession;
