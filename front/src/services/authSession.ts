import type { AuthTokenPair } from "./types";
import type { AuthSessionRecord, StoredAuthSession } from "./authTokens";
import { authErrorCode } from "./authErrors.ts";

export interface AuthSessionStore {
  lifecycle: "unknown" | "restoring" | "authenticated" | "anonymous";
  setRestoring: () => void;
  setUnknown: () => void;
  setAuthenticated: (token: string) => void;
  setAnonymous: () => void;
}

export function applyStoredAuthSession(
  store: AuthSessionStore,
  session: StoredAuthSession | null,
): void {
  if (session?.kind === "record") {
    store.setAuthenticated(session.record.access_token);
  } else {
    store.setAnonymous();
  }
}

export interface AuthSessionDependencies {
  getAuthStore: () => AuthSessionStore;
  readStoredAuthSession: () => StoredAuthSession | null;
  persistAuthTokenPair: (tokenPair: AuthTokenPair) => AuthSessionRecord;
  clearStoredAuthSession: () => void;
  clearStoredAuthSessionIfCurrent: (refreshToken: string) => boolean;
  refreshToken: (refreshToken: string) => Promise<AuthTokenPair>;
  loginWithPassword: (password: string) => Promise<AuthTokenPair>;
  bootstrapWithPassword: (password: string) => Promise<AuthTokenPair>;
  rotateManagerPassword: (
    currentPassword: string,
    newPassword: string,
  ) => Promise<AuthTokenPair>;
  logoutRequest: () => Promise<void>;
  logoutAllRequest: () => Promise<void>;
}

export interface LogoutOutcome {
  serverRevocationConfirmed: boolean;
}

export function createAuthSessionActions(deps: AuthSessionDependencies) {
  let restorePromise: Promise<boolean> | null = null;

  const restoreStoredSession = async (): Promise<boolean> => {
    const store = deps.getAuthStore();
    if (store.lifecycle === "authenticated") return true;
    if (restorePromise) return restorePromise;

    const storedSession = deps.readStoredAuthSession();
    const storedRefreshToken =
      storedSession?.kind === "record"
        ? storedSession.record.refresh_token
        : storedSession?.refresh_token;

    if (!storedRefreshToken) {
      store.setAnonymous();
      return false;
    }

    store.setRestoring();
    restorePromise = (async () => {
      try {
        const tokenPair = await deps.refreshToken(storedRefreshToken);
        store.setAuthenticated(deps.persistAuthTokenPair(tokenPair).access_token);
        return true;
      } catch (error) {
        const winner = deps.readStoredAuthSession();
        if (
          winner?.kind === "record" &&
          winner.record.refresh_token !== storedRefreshToken
        ) {
          store.setAuthenticated(winner.record.access_token);
          return true;
        }
        if ([1441, 1442, 1444].includes(authErrorCode(error) ?? -1)) {
          if (deps.clearStoredAuthSessionIfCurrent(storedRefreshToken)) {
            store.setAnonymous();
          }
        } else {
          store.setUnknown();
        }
        return false;
      } finally {
        restorePromise = null;
      }
    })();
    return restorePromise;
  };

  const persistSession = (tokenPair: AuthTokenPair): void => {
    deps.getAuthStore().setAuthenticated(
      deps.persistAuthTokenPair(tokenPair).access_token,
    );
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

  const logout = async (): Promise<LogoutOutcome> => {
    let serverRevocationConfirmed = false;
    try {
      await deps.logoutRequest();
      serverRevocationConfirmed = true;
    } catch (error) {
      serverRevocationConfirmed = [1433, 1435].includes(
        authErrorCode(error) ?? -1,
      );
      // Local logout must still finish if the backend session is already invalid.
    } finally {
      deps.clearStoredAuthSession();
      deps.getAuthStore().setAnonymous();
    }
    return { serverRevocationConfirmed };
  };

  const logoutAll = async (): Promise<void> => {
    try {
      await deps.logoutAllRequest();
    } catch (error) {
      if (![1433, 1435].includes(authErrorCode(error) ?? -1)) throw error;
    }
    deps.clearStoredAuthSession();
    deps.getAuthStore().setAnonymous();
  };

  return {
    restoreStoredSession,
    tryRefreshToken: restoreStoredSession,
    login,
    bootstrap,
    rotatePassword,
    logout,
    logoutAll,
  };
}
