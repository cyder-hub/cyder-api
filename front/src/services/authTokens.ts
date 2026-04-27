import type { AuthTokenPair } from "../store/types";

export const REFRESH_TOKEN_STORAGE_KEY = "auth_token";

type ReadableTokenStorage = Pick<Storage, "getItem">;
type WritableTokenStorage = Pick<Storage, "setItem">;
type RemovableTokenStorage = Pick<Storage, "removeItem">;
type RefreshTokenStorage = Pick<Storage, "getItem" | "removeItem">;

export function readStoredRefreshToken(
  storage: ReadableTokenStorage = localStorage,
): string | null {
  return storage.getItem(REFRESH_TOKEN_STORAGE_KEY);
}

export function persistAuthTokenPair(
  tokenPair: AuthTokenPair,
  storage: WritableTokenStorage = localStorage,
): string {
  storage.setItem(REFRESH_TOKEN_STORAGE_KEY, tokenPair.refresh_token);
  return tokenPair.access_token;
}

export function clearStoredRefreshToken(
  storage: RemovableTokenStorage = localStorage,
): void {
  storage.removeItem(REFRESH_TOKEN_STORAGE_KEY);
}

export function clearStoredRefreshTokenIfCurrent(
  refreshToken: string,
  storage: RefreshTokenStorage = localStorage,
): boolean {
  if (storage.getItem(REFRESH_TOKEN_STORAGE_KEY) !== refreshToken) {
    return false;
  }
  storage.removeItem(REFRESH_TOKEN_STORAGE_KEY);
  return true;
}
