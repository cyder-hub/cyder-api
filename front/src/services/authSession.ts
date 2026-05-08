import type { AuthTokenPair } from "./types";

export interface AuthSessionStore {
  setAccessToken: (token: string | null) => void;
}

export interface AuthSessionDependencies {
  getAuthStore: () => AuthSessionStore;
  readStoredRefreshToken: () => string | null;
  persistAuthTokenPair: (tokenPair: AuthTokenPair) => string;
  clearStoredRefreshToken: () => void;
  clearStoredRefreshTokenIfCurrent: (refreshToken: string) => boolean;
  refreshToken: (refreshToken: string) => Promise<AuthTokenPair>;
  loginWithPassword: (password: string) => Promise<AuthTokenPair>;
  logoutRequest: () => Promise<void>;
}

export function createAuthSessionActions(deps: AuthSessionDependencies) {
  const tryRefreshToken = async (): Promise<boolean> => {
    const storedRefreshToken = deps.readStoredRefreshToken();

    if (!storedRefreshToken) {
      return false;
    }

    try {
      const tokenPair = await deps.refreshToken(storedRefreshToken);
      deps.getAuthStore().setAccessToken(deps.persistAuthTokenPair(tokenPair));
      return true;
    } catch {
      if (deps.clearStoredRefreshTokenIfCurrent(storedRefreshToken)) {
        deps.getAuthStore().setAccessToken(null);
      }
      return false;
    }
  };

  const login = async (password: string): Promise<boolean> => {
    try {
      const tokenPair = await deps.loginWithPassword(password);
      deps.getAuthStore().setAccessToken(deps.persistAuthTokenPair(tokenPair));
      return true;
    } catch {
      return false;
    }
  };

  const logout = async (): Promise<void> => {
    try {
      await deps.logoutRequest();
    } catch {
      // Local logout must still finish if the backend session is already invalid.
    } finally {
      deps.clearStoredRefreshToken();
      deps.getAuthStore().setAccessToken(null);
    }
  };

  return {
    tryRefreshToken,
    login,
    logout,
  };
}
