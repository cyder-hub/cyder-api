import type { LogoutAllResult, ManagerAuthAccess } from "./types";
import { authErrorCode } from "./authErrors.ts";

export interface AuthSessionStore {
  lifecycle: "unknown" | "restoring" | "authenticated" | "anonymous";
  setRestoring: () => void;
  setUnknown: () => void;
  setAuthenticated: (token: string) => void;
  setAnonymous: () => void;
}

export interface AuthSessionDependencies {
  getAuthStore: () => AuthSessionStore;
  getAccessToken: () => string | null;
  setAccessToken: (token: string) => void;
  clearAccessToken: () => void;
  clearLegacyAuthStorage: () => void;
  requestAccess: () => Promise<ManagerAuthAccess>;
  loginWithPassword: (password: string) => Promise<ManagerAuthAccess>;
  bootstrapWithPassword: (password: string) => Promise<ManagerAuthAccess>;
  rotateManagerPassword: (
    currentPassword: string,
    newPassword: string,
  ) => Promise<ManagerAuthAccess>;
  logoutRequest: () => Promise<void>;
  logoutAllRequest: () => Promise<LogoutAllResult>;
  announceSessionChanged: () => void;
  announceSessionRevoked: () => void;
  onSessionRevoked: () => void;
}

export interface LogoutOutcome {
  serverRevocationConfirmed: boolean;
}

class AuthLifecycleSupersededError extends Error {
  constructor() {
    super("manager authentication lifecycle changed during access recovery");
    this.name = "AuthLifecycleSupersededError";
  }
}

function isDefinitiveSessionFailure(error: unknown): boolean {
  return [1441, 1444].includes(authErrorCode(error) ?? -1);
}

export function createAuthSessionActions(deps: AuthSessionDependencies) {
  let accessPromise: Promise<string> | null = null;
  let lifecycleGeneration = 0;
  deps.clearLegacyAuthStorage();

  const installAccess = (access: ManagerAuthAccess): string => {
    deps.setAccessToken(access.access_token);
    deps.getAuthStore().setAuthenticated(access.access_token);
    return access.access_token;
  };

  const invalidateAccessRecovery = (): void => {
    lifecycleGeneration += 1;
    accessPromise = null;
  };

  const replaceAccess = (access: ManagerAuthAccess): string => {
    invalidateAccessRecovery();
    return installAccess(access);
  };

  const clearSession = (): void => {
    deps.clearAccessToken();
    deps.getAuthStore().setAnonymous();
  };

  const revokeLocalSession = (announce = false): void => {
    invalidateAccessRecovery();
    clearSession();
    if (announce) deps.announceSessionRevoked();
    deps.onSessionRevoked();
  };

  const recoverAccess = async (): Promise<string> => {
    if (accessPromise) return accessPromise;

    const store = deps.getAuthStore();
    const previousLifecycle = store.lifecycle;
    const previousAccess = deps.getAccessToken();
    if (previousLifecycle !== "authenticated") {
      store.setRestoring();
    }

    const recoveryGeneration = lifecycleGeneration;
    const currentPromise = deps
      .requestAccess()
      .then((access) => {
        if (recoveryGeneration !== lifecycleGeneration) {
          throw new AuthLifecycleSupersededError();
        }
        return installAccess(access);
      })
      .catch((error: unknown) => {
        if (recoveryGeneration !== lifecycleGeneration) {
          throw error;
        }
        if (isDefinitiveSessionFailure(error)) {
          revokeLocalSession(true);
        } else if (
          previousLifecycle === "authenticated" &&
          previousAccess !== null
        ) {
          store.setAuthenticated(previousAccess);
        } else {
          store.setUnknown();
        }
        throw error;
      })
      .finally(() => {
        if (accessPromise === currentPromise) {
          accessPromise = null;
        }
      });
    accessPromise = currentPromise;
    return currentPromise;
  };

  const restoreSession = async (): Promise<boolean> => {
    const store = deps.getAuthStore();
    if (
      store.lifecycle === "authenticated" &&
      deps.getAccessToken() !== null
    ) {
      return true;
    }
    try {
      await recoverAccess();
      return true;
    } catch {
      return false;
    }
  };

  const login = async (password: string): Promise<void> => {
    replaceAccess(await deps.loginWithPassword(password));
    deps.announceSessionChanged();
  };

  const bootstrap = async (password: string): Promise<void> => {
    replaceAccess(await deps.bootstrapWithPassword(password));
    deps.announceSessionChanged();
  };

  const rotatePassword = async (
    currentPassword: string,
    newPassword: string,
  ): Promise<void> => {
    replaceAccess(
      await deps.rotateManagerPassword(currentPassword, newPassword),
    );
    deps.announceSessionChanged();
  };

  const logout = async (): Promise<LogoutOutcome> => {
    let serverRevocationConfirmed = false;
    try {
      await deps.logoutRequest();
      serverRevocationConfirmed = true;
    } catch {
      serverRevocationConfirmed = false;
    } finally {
      revokeLocalSession(true);
    }
    return { serverRevocationConfirmed };
  };

  const logoutAll = async (): Promise<LogoutAllResult> => {
    try {
      const result = await deps.logoutAllRequest();
      revokeLocalSession(true);
      return result;
    } catch (error) {
      if ([1433, 1435, 1441].includes(authErrorCode(error) ?? -1)) {
        revokeLocalSession(true);
        return { revoked_sessions: 0 };
      }
      throw error;
    }
  };

  return {
    restoreSession,
    recoverAccess,
    invalidateAccessRecovery,
    revokeLocalSession: () => revokeLocalSession(false),
    revokeAndAnnounce: () => revokeLocalSession(true),
    login,
    bootstrap,
    rotatePassword,
    logout,
    logoutAll,
  };
}
