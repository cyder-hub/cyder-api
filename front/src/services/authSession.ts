import type {
  LogoutAllResult,
  ManagerAuthAccess,
  ManagerPasswordLoginResult,
  ManagerReauthStatus,
  ManagerTotpLifecycleResult,
  ManagerTotpRecoveryResult,
  ManagerTotpRecoverySetup,
  ManagerTotpState,
} from "./types";
import { authErrorCode } from "./authErrors.ts";

export interface AuthSessionStore {
  lifecycle: "unknown" | "restoring" | "authenticated" | "anonymous";
  totpState: ManagerTotpState | null;
  reauth: ManagerReauthStatus | null;
  setRestoring: () => void;
  setUnknown: () => void;
  setAuthenticated: (
    token: string,
    totpState: ManagerTotpState,
    reauth: ManagerReauthStatus | null,
  ) => void;
  setAnonymous: () => void;
}

export interface AuthSessionDependencies {
  getAuthStore: () => AuthSessionStore;
  getAccessToken: () => string | null;
  setAccessToken: (token: string) => void;
  clearAccessToken: () => void;
  clearLegacyAuthStorage: () => void;
  requestAccess: () => Promise<ManagerAuthAccess>;
  loginWithPassword: (password: string) => Promise<ManagerPasswordLoginResult>;
  loginWithTotp: (
    loginChallenge: string,
    totpCode: string,
  ) => Promise<ManagerAuthAccess>;
  startTotpRecovery: (
    password: string,
    recoveryCode: string,
  ) => Promise<ManagerTotpRecoverySetup>;
  confirmTotpRecovery: (
    recoveryChallenge: string,
    totpCode: string,
  ) => Promise<ManagerTotpRecoveryResult>;
  confirmTotpEnrollmentRequest: (
    setupChallenge: string,
    totpCode: string,
  ) => Promise<ManagerTotpLifecycleResult>;
  confirmTotpReplacementRequest: (
    setupChallenge: string,
    totpCode: string,
  ) => Promise<ManagerTotpLifecycleResult>;
  disableTotpRequest: (
    currentPassword: string,
    currentTotpCode: string,
  ) => Promise<ManagerTotpLifecycleResult>;
  bootstrapWithPassword: (password: string) => Promise<ManagerAuthAccess>;
  rotateManagerPassword: (
    currentPassword: string,
    newPassword: string,
    totpCode?: string,
  ) => Promise<ManagerAuthAccess>;
  logoutRequest: () => Promise<void>;
  logoutAllRequest: (
    currentPassword: string,
    totpCode?: string,
  ) => Promise<LogoutAllResult>;
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
    deps
      .getAuthStore()
      .setAuthenticated(access.access_token, access.totp_state, access.reauth);
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
          const previousTotpState = store.totpState;
          const previousReauth = store.reauth;
          if (previousTotpState) {
            store.setAuthenticated(
              previousAccess,
              previousTotpState,
              previousReauth,
            );
          } else {
            store.setUnknown();
          }
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

  const login = async (
    password: string,
  ): Promise<ManagerPasswordLoginResult> => {
    const result = await deps.loginWithPassword(password);
    if (result.state === "authenticated") {
      replaceAccess(result);
      deps.announceSessionChanged();
    }
    return result;
  };

  const completeTotpLogin = async (
    loginChallenge: string,
    totpCode: string,
  ): Promise<void> => {
    replaceAccess(await deps.loginWithTotp(loginChallenge, totpCode));
    deps.announceSessionChanged();
  };

  const startRecovery = async (
    password: string,
    recoveryCode: string,
  ): Promise<ManagerTotpRecoverySetup> => {
    const setup = await deps.startTotpRecovery(password, recoveryCode);
    invalidateAccessRecovery();
    clearSession();
    deps.announceSessionRevoked();
    return setup;
  };

  const confirmRecovery = async (
    recoveryChallenge: string,
    totpCode: string,
  ): Promise<ManagerTotpRecoveryResult> => {
    const result = await deps.confirmTotpRecovery(recoveryChallenge, totpCode);
    replaceAccess(result);
    deps.announceSessionChanged();
    return result;
  };

  const installTotpLifecycle = (
    result: ManagerTotpLifecycleResult,
  ): ManagerTotpLifecycleResult => {
    replaceAccess(result);
    deps.announceSessionChanged();
    return result;
  };

  const confirmTotpEnrollment = async (
    setupChallenge: string,
    totpCode: string,
  ): Promise<ManagerTotpLifecycleResult> =>
    installTotpLifecycle(
      await deps.confirmTotpEnrollmentRequest(setupChallenge, totpCode),
    );

  const confirmTotpReplacement = async (
    setupChallenge: string,
    totpCode: string,
  ): Promise<ManagerTotpLifecycleResult> =>
    installTotpLifecycle(
      await deps.confirmTotpReplacementRequest(setupChallenge, totpCode),
    );

  const disableTotp = async (
    currentPassword: string,
    currentTotpCode: string,
  ): Promise<void> => {
    installTotpLifecycle(
      await deps.disableTotpRequest(currentPassword, currentTotpCode),
    );
  };

  const bootstrap = async (password: string): Promise<void> => {
    replaceAccess(await deps.bootstrapWithPassword(password));
    deps.announceSessionChanged();
  };

  const rotatePassword = async (
    currentPassword: string,
    newPassword: string,
    totpCode?: string,
  ): Promise<void> => {
    replaceAccess(
      await deps.rotateManagerPassword(
        currentPassword,
        newPassword,
        totpCode,
      ),
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

  const logoutAll = async (
    currentPassword: string,
    totpCode?: string,
  ): Promise<LogoutAllResult> => {
    try {
      const result = await deps.logoutAllRequest(currentPassword, totpCode);
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
    completeTotpLogin,
    startRecovery,
    confirmRecovery,
    confirmTotpEnrollment,
    confirmTotpReplacement,
    disableTotp,
    bootstrap,
    rotatePassword,
    logout,
    logoutAll,
  };
}
