import { authErrorCode } from "./authErrors.ts";
import type {
  AuthSessionRecord,
  StoredAuthSession,
} from "./authTokens";
import type { AuthTokenPair } from "./types";

export type AuthLifecycle =
  | "unknown"
  | "restoring"
  | "authenticated"
  | "anonymous";

export interface RetriableHttpRequest {
  _retry?: boolean;
  _authRevision?: number;
  _authRefreshToken?: string;
  headers?: Record<string, string>;
  url?: string;
  [key: string]: unknown;
}

export interface HttpAuthRefreshError {
  config?: RetriableHttpRequest;
  response?: {
    status?: number;
    data?: { code?: unknown };
  };
}

export interface HttpAuthRefreshDependencies {
  readStoredAuthSession: () => StoredAuthSession | null;
  persistAuthTokenPair: (tokenPair: AuthTokenPair) => AuthSessionRecord;
  clearStoredAuthSessionIfCurrent: (refreshToken: string) => boolean;
  getLifecycle: () => AuthLifecycle;
  restoreStoredSession: () => Promise<boolean>;
  setAuthenticated: (token: string) => void;
  setAnonymous: () => void;
  refreshAccessToken: (refreshToken: string) => Promise<AuthTokenPair>;
  retryRequest: (request: RetriableHttpRequest) => Promise<unknown>;
  redirectToLogin: () => void;
  subscribeToSessionChanges?: (
    onChange: (session: StoredAuthSession | null) => void,
  ) => () => void;
}

interface PendingRefresh {
  resolve: (token: string) => void;
  reject: (reason: unknown) => void;
}

export class ManagerAuthenticationRequiredError extends Error {
  constructor() {
    super("manager authentication is required");
    this.name = "ManagerAuthenticationRequiredError";
  }
}

function setAuthorizationHeader(
  request: RetriableHttpRequest,
  token: string,
): void {
  request.headers ??= {};
  request.headers.Authorization = `Bearer ${token}`;
}

export function isPublicManagerAuthRequest(request: RetriableHttpRequest): boolean {
  const url = request.url ?? "";
  return [
    "/ai/manager/api/auth/bootstrap/status",
    "/ai/manager/api/auth/bootstrap",
    "/ai/manager/api/auth/login",
    "/ai/manager/api/auth/refresh_token",
  ].some((path) => url === path || url.startsWith(`${path}?`));
}

function isProtectedManagerRequest(request: RetriableHttpRequest): boolean {
  return (
    (request.url ?? "").startsWith("/ai/manager/api/") &&
    !isPublicManagerAuthRequest(request)
  );
}

function recordFrom(session: StoredAuthSession | null): AuthSessionRecord | null {
  return session?.kind === "record" ? session.record : null;
}

function refreshTokenFrom(session: StoredAuthSession | null): string | null {
  if (!session) return null;
  return session.kind === "record"
    ? session.record.refresh_token
    : session.refresh_token;
}

function isNewerRecord(
  record: AuthSessionRecord,
  request: RetriableHttpRequest,
): boolean {
  if (request._authRefreshToken) {
    return record.refresh_token !== request._authRefreshToken;
  }
  if (request._authRevision !== undefined) {
    return record.revision > request._authRevision;
  }
  const authorization = request.headers?.Authorization;
  return authorization !== `Bearer ${record.access_token}`;
}

export function createProtectedManagerRequestGate(
  deps: Pick<
    HttpAuthRefreshDependencies,
    "getLifecycle" | "restoreStoredSession" | "readStoredAuthSession"
  >,
) {
  return async function gateProtectedManagerRequest(
    request: RetriableHttpRequest,
  ): Promise<RetriableHttpRequest> {
    if (!isProtectedManagerRequest(request)) return request;

    const lifecycle = deps.getLifecycle();
    if (
      (lifecycle === "unknown" || lifecycle === "restoring") &&
      !(await deps.restoreStoredSession())
    ) {
      throw new ManagerAuthenticationRequiredError();
    }
    if (deps.getLifecycle() === "anonymous") {
      throw new ManagerAuthenticationRequiredError();
    }

    const record = recordFrom(deps.readStoredAuthSession());
    if (!record) throw new ManagerAuthenticationRequiredError();
    request._authRevision = record.revision;
    request._authRefreshToken = record.refresh_token;
    setAuthorizationHeader(request, record.access_token);
    return request;
  };
}

export function createHttpAuthRefreshHandler(
  deps: HttpAuthRefreshDependencies,
) {
  let isRefreshing = false;
  let failedQueue: PendingRefresh[] = [];
  let sessionGeneration = 0;

  const processQueue = (error: unknown, token: string | null): void => {
    failedQueue.forEach((pending) => {
      if (error) {
        pending.reject(error);
      } else if (token) {
        pending.resolve(token);
      }
    });
    failedQueue = [];
  };

  const redirectToLogin = (): void => {
    deps.setAnonymous();
    deps.redirectToLogin();
  };

  const invalidateIfCurrent = (refreshToken: string): boolean => {
    if (!deps.clearStoredAuthSessionIfCurrent(refreshToken)) return false;
    processQueue(new ManagerAuthenticationRequiredError(), null);
    redirectToLogin();
    return true;
  };

  const adoptAndRetry = (
    request: RetriableHttpRequest,
    record: AuthSessionRecord,
  ): Promise<unknown> => {
    request._retry = true;
    deps.setAuthenticated(record.access_token);
    setAuthorizationHeader(request, record.access_token);
    return deps.retryRequest(request);
  };

  deps.subscribeToSessionChanges?.((session) => {
    sessionGeneration += 1;
    const record = recordFrom(session);
    if (record) {
      deps.setAuthenticated(record.access_token);
      return;
    }
    processQueue(new ManagerAuthenticationRequiredError(), null);
    redirectToLogin();
  });

  return async function handleHttpAuthRefresh(
    error: HttpAuthRefreshError,
  ): Promise<unknown> {
    const originalRequest = error.config;
    const code = authErrorCode(error);

    if (
      error.response?.status !== 401 ||
      !originalRequest ||
      originalRequest._retry ||
      isPublicManagerAuthRequest(originalRequest)
    ) {
      throw error;
    }

    if (![1432, 1433, 1435].includes(code ?? -1)) throw error;

    const currentRecord = recordFrom(deps.readStoredAuthSession());
    if (currentRecord && isNewerRecord(currentRecord, originalRequest)) {
      return adoptAndRetry(originalRequest, currentRecord);
    }

    const attemptedRefreshToken =
      originalRequest._authRefreshToken ??
      refreshTokenFrom(deps.readStoredAuthSession());

    if (code === 1433 || code === 1435) {
      if (attemptedRefreshToken) invalidateIfCurrent(attemptedRefreshToken);
      else redirectToLogin();
      throw error;
    }

    if (!attemptedRefreshToken) {
      redirectToLogin();
      throw error;
    }

    if (isRefreshing) {
      originalRequest._retry = true;
      return new Promise<string>((resolve, reject) => {
        failedQueue.push({ resolve, reject });
      }).then((token) => {
        setAuthorizationHeader(originalRequest, token);
        return deps.retryRequest(originalRequest);
      });
    }

    originalRequest._retry = true;
    isRefreshing = true;
    const refreshGeneration = sessionGeneration;

    try {
      const tokenPair = await deps.refreshAccessToken(attemptedRefreshToken);
      if (sessionGeneration !== refreshGeneration) {
        const winner = recordFrom(deps.readStoredAuthSession());
        if (winner && winner.refresh_token !== attemptedRefreshToken) {
          deps.setAuthenticated(winner.access_token);
          processQueue(null, winner.access_token);
          setAuthorizationHeader(originalRequest, winner.access_token);
          return deps.retryRequest(originalRequest);
        }
        throw new ManagerAuthenticationRequiredError();
      }
      const newRecord = deps.persistAuthTokenPair(tokenPair);

      deps.setAuthenticated(newRecord.access_token);
      setAuthorizationHeader(originalRequest, newRecord.access_token);
      processQueue(null, newRecord.access_token);

      return deps.retryRequest(originalRequest);
    } catch (refreshError) {
      const refreshCode = authErrorCode(refreshError);
      if (refreshCode === 1444) {
        const winner = recordFrom(deps.readStoredAuthSession());
        if (winner && winner.refresh_token !== attemptedRefreshToken) {
          deps.setAuthenticated(winner.access_token);
          processQueue(null, winner.access_token);
          setAuthorizationHeader(originalRequest, winner.access_token);
          return deps.retryRequest(originalRequest);
        }
      }

      processQueue(refreshError, null);
      if ([1441, 1442, 1444].includes(refreshCode ?? -1)) {
        invalidateIfCurrent(attemptedRefreshToken);
      }
      throw refreshError;
    } finally {
      isRefreshing = false;
    }
  };
}
