export const LEGACY_AUTH_STORAGE_KEY = "auth_token";

type RemovableTokenStorage = Pick<Storage, "removeItem">;

let accessToken: string | null = null;

export function getAccessToken(): string | null {
  return accessToken;
}

export function setAccessToken(token: string): void {
  accessToken = token;
}

export function clearAccessToken(): void {
  accessToken = null;
}

export function clearLegacyAuthStorage(
  local: RemovableTokenStorage = localStorage,
  session: RemovableTokenStorage = sessionStorage,
): void {
  local.removeItem(LEGACY_AUTH_STORAGE_KEY);
  session.removeItem(LEGACY_AUTH_STORAGE_KEY);
}
