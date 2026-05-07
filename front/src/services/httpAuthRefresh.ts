import type { AuthTokenPair } from "./types";

export interface RetriableHttpRequest {
  _retry?: boolean;
  headers?: Record<string, string>;
  [key: string]: unknown;
}

export interface HttpAuthRefreshError {
  config?: RetriableHttpRequest;
  response?: {
    status?: number;
  };
}

export interface HttpAuthRefreshDependencies {
  readStoredRefreshToken: () => string | null;
  persistAuthTokenPair: (tokenPair: AuthTokenPair) => string;
  clearStoredRefreshTokenIfCurrent: (refreshToken: string) => boolean;
  setAccessToken: (token: string | null) => void;
  refreshAccessToken: (refreshToken: string) => Promise<AuthTokenPair>;
  retryRequest: (request: RetriableHttpRequest) => Promise<unknown>;
  redirectToLogin: () => void;
}

interface PendingRefresh {
  resolve: (token: string) => void;
  reject: (reason: unknown) => void;
}

function httpStatus(error: unknown): number | undefined {
  if (!error || typeof error !== "object" || !("response" in error)) {
    return undefined;
  }

  const response = (error as { response?: { status?: unknown } }).response;
  return typeof response?.status === "number" ? response.status : undefined;
}

function setAuthorizationHeader(
  request: RetriableHttpRequest,
  token: string,
): void {
  request.headers ??= {};
  request.headers.Authorization = `Bearer ${token}`;
}

export function createHttpAuthRefreshHandler(
  deps: HttpAuthRefreshDependencies,
) {
  let isRefreshing = false;
  let failedQueue: PendingRefresh[] = [];

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

  return async function handleHttpAuthRefresh(
    error: HttpAuthRefreshError,
  ): Promise<unknown> {
    const originalRequest = error.config;

    if (
      error.response?.status !== 401 ||
      !originalRequest ||
      originalRequest._retry
    ) {
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

    const refreshToken = deps.readStoredRefreshToken();
    if (!refreshToken) {
      throw error;
    }

    originalRequest._retry = true;
    isRefreshing = true;

    try {
      const tokenPair = await deps.refreshAccessToken(refreshToken);
      const newAccessToken = deps.persistAuthTokenPair(tokenPair);

      deps.setAccessToken(newAccessToken);
      setAuthorizationHeader(originalRequest, newAccessToken);
      processQueue(null, newAccessToken);

      return deps.retryRequest(originalRequest);
    } catch (refreshError) {
      processQueue(refreshError, null);

      if (
        httpStatus(refreshError) === 401 &&
        deps.clearStoredRefreshTokenIfCurrent(refreshToken)
      ) {
        deps.setAccessToken(null);
        deps.redirectToLogin();
      }

      throw refreshError;
    } finally {
      isRefreshing = false;
    }
  };
}
