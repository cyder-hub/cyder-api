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
  bootstrapWithPassword: (password: string) => Promise<AuthTokenPair>;
  rotateManagerPassword: (
    currentPassword: string,
    newPassword: string,
  ) => Promise<AuthTokenPair>;
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

  const persistSession = (tokenPair: AuthTokenPair): void => {
    deps.getAuthStore().setAccessToken(deps.persistAuthTokenPair(tokenPair));
  };

  const login = async (password: string): Promise<void> => {
    persistSession(await deps.loginWithPassword(password));
  };

  const bootstrap = async (password: string): Promise<void> => {
    persistSession(await deps.bootstrapWithPassword(password));
  };

  const rotatePassword = async (
    currentPassword: string,
    newPassword: string,
  ): Promise<void> => {
    persistSession(await deps.rotateManagerPassword(currentPassword, newPassword));
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
    bootstrap,
    rotatePassword,
    logout,
  };
}
