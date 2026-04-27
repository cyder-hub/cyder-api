import { useAuthStore } from "@/store/authStore";
import { Api } from "./request";
import {
  clearStoredRefreshTokenIfCurrent,
  clearStoredRefreshToken,
  persistAuthTokenPair,
  readStoredRefreshToken,
} from "./authTokens";

export async function tryRefreshToken(): Promise<boolean> {
  const refreshToken = readStoredRefreshToken();
  const authStore = useAuthStore();

  if (!refreshToken) {
    return false;
  }

  try {
    const tokenPair = await Api.refreshToken(refreshToken);
    authStore.setAccessToken(persistAuthTokenPair(tokenPair));
    return true;
  } catch {
    if (clearStoredRefreshTokenIfCurrent(refreshToken)) {
      authStore.setAccessToken(null);
    }
    return false;
  }
}

export async function login(password: string): Promise<boolean> {
  try {
    const tokenPair = await Api.login(password);
    const authStore = useAuthStore();
    authStore.setAccessToken(persistAuthTokenPair(tokenPair));
    return true;
  } catch {
    return false;
  }
}

export async function logout(): Promise<void> {
  const authStore = useAuthStore();
  try {
    await Api.logout();
  } catch {
    // Local logout must complete even when the server-side instance is already gone
    // or the access token has expired.
  } finally {
    clearStoredRefreshToken();
    authStore.setAccessToken(null);
  }
}
