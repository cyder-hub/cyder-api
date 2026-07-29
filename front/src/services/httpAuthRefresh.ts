import { authErrorCode } from "./authErrors.ts";

export type AuthLifecycle =
  | "unknown"
  | "restoring"
  | "authenticated"
  | "anonymous";

type MutableHeaders = Record<string, string> & {
  set?: (name: string, value: string) => void;
};

export interface RetriableHttpRequest {
  _retry?: boolean;
  _skipAuthRetry?: boolean;
  headers?: MutableHeaders;
  method?: string;
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
  getAccessToken: () => string | null;
  getLifecycle: () => AuthLifecycle;
  restoreSession: () => Promise<boolean>;
  recoverAccess: () => Promise<string>;
  revokeSession: () => void;
  retryRequest: (request: RetriableHttpRequest) => Promise<unknown>;
}

export class ManagerAuthenticationRequiredError extends Error {
  constructor() {
    super("manager authentication is required");
    this.name = "ManagerAuthenticationRequiredError";
  }
}

function setRequestHeader(
  request: RetriableHttpRequest,
  name: string,
  value: string,
): void {
  request.headers ??= {};
  if (typeof request.headers.set === "function") {
    request.headers.set(name, value);
  } else {
    request.headers[name] = value;
  }
}

function setAuthorizationHeader(
  request: RetriableHttpRequest,
  token: string,
): void {
  setRequestHeader(request, "Authorization", `Bearer ${token}`);
}

const PUBLIC_MANAGER_AUTH_PATHS = [
  "/ai/manager/api/auth/bootstrap/status",
  "/ai/manager/api/auth/bootstrap",
  "/ai/manager/api/auth/login/password",
  "/ai/manager/api/auth/login/totp",
  "/ai/manager/api/auth/recovery/start",
  "/ai/manager/api/auth/recovery/confirm",
  "/ai/manager/api/auth/access",
  "/ai/manager/api/auth/logout",
];

export function isPublicManagerAuthRequest(
  request: RetriableHttpRequest,
): boolean {
  const url = request.url ?? "";
  return PUBLIC_MANAGER_AUTH_PATHS.some(
    (path) => url === path || url.startsWith(`${path}?`),
  );
}

function isManagerAuthPost(request: RetriableHttpRequest): boolean {
  return (
    (request.method ?? "get").toLowerCase() === "post" &&
    (request.url ?? "").startsWith("/ai/manager/api/auth/")
  );
}

export function applyManagerAuthBrowserHeaders(
  request: RetriableHttpRequest,
): void {
  if (!isManagerAuthPost(request)) return;
  setRequestHeader(request, "Content-Type", "application/json");
  setRequestHeader(request, "X-Cyder-Manager-Auth", "1");
}

function isProtectedManagerRequest(request: RetriableHttpRequest): boolean {
  return (
    (request.url ?? "").startsWith("/ai/manager/api/") &&
    !isPublicManagerAuthRequest(request)
  );
}

export function createProtectedManagerRequestGate(
  deps: Pick<
    HttpAuthRefreshDependencies,
    "getAccessToken" | "getLifecycle" | "restoreSession"
  >,
) {
  return async function gateProtectedManagerRequest(
    request: RetriableHttpRequest,
  ): Promise<RetriableHttpRequest> {
    if (!isProtectedManagerRequest(request)) return request;

    const lifecycle = deps.getLifecycle();
    if (
      (lifecycle === "unknown" || lifecycle === "restoring") &&
      !(await deps.restoreSession())
    ) {
      throw new ManagerAuthenticationRequiredError();
    }
    if (deps.getLifecycle() === "anonymous") {
      throw new ManagerAuthenticationRequiredError();
    }

    const token = deps.getAccessToken();
    if (!token) throw new ManagerAuthenticationRequiredError();
    setAuthorizationHeader(request, token);
    return request;
  };
}

export function createHttpAuthRefreshHandler(
  deps: HttpAuthRefreshDependencies,
) {
  let recoveryPromise: Promise<string> | null = null;

  const invalidateSession = (): void => {
    deps.revokeSession();
  };

  const sharedRecovery = (): Promise<string> => {
    if (recoveryPromise) return recoveryPromise;
    recoveryPromise = deps.recoverAccess().finally(() => {
      recoveryPromise = null;
    });
    return recoveryPromise;
  };

  return async function handleHttpAuthRefresh(
    error: HttpAuthRefreshError,
  ): Promise<unknown> {
    const originalRequest = error.config;
    const code = authErrorCode(error);

    if (
      error.response?.status !== 401 ||
      !originalRequest ||
      originalRequest._retry ||
      originalRequest._skipAuthRetry ||
      isPublicManagerAuthRequest(originalRequest)
    ) {
      throw error;
    }

    if (![1432, 1433, 1435].includes(code ?? -1)) throw error;

    originalRequest._retry = true;
    try {
      const token = await sharedRecovery();
      setAuthorizationHeader(originalRequest, token);
      return deps.retryRequest(originalRequest);
    } catch (recoveryError) {
      if ([1441, 1444].includes(authErrorCode(recoveryError) ?? -1)) {
        invalidateSession();
      }
      throw recoveryError;
    }
  };
}
