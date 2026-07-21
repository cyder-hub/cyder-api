import axios from "axios";
import { useAuthStore } from "@/store/authStore";
import type { AuthTokenPair } from "./types";
import { navigateToManagerLogin, restoreManagerSession } from "./authRuntime";
import {
  createHttpAuthRefreshHandler,
  createProtectedManagerRequestGate,
  type HttpAuthRefreshDependencies,
  type HttpAuthRefreshError,
  type RetriableHttpRequest,
} from "./httpAuthRefresh";
import {
  clearStoredAuthSessionIfCurrent,
  persistAuthTokenPair,
  readStoredAuthSession,
  subscribeToAuthSessionChanges,
} from "./authTokens";

const apiClient = axios.create({
  headers: {
    "Content-Type": "application/json",
  },
});

const authHttpDependencies = {
  readStoredAuthSession,
  persistAuthTokenPair,
  clearStoredAuthSessionIfCurrent,
  getLifecycle: () => useAuthStore().lifecycle,
  restoreStoredSession: restoreManagerSession,
  setAuthenticated: (token: string) => useAuthStore().setAuthenticated(token),
  setAnonymous: () => useAuthStore().setAnonymous(),
  refreshAccessToken: async (refreshToken: string) => {
    const response = await axios.post(
      "/ai/manager/api/auth/refresh_token",
      {},
      {
        headers: { Authorization: `Bearer ${refreshToken}` },
      },
    );
    return response.data.data as AuthTokenPair;
  },
  retryRequest: (originalRequest: RetriableHttpRequest) =>
    apiClient(originalRequest),
  redirectToLogin: navigateToManagerLogin,
  subscribeToSessionChanges: subscribeToAuthSessionChanges,
} satisfies HttpAuthRefreshDependencies;

const gateProtectedManagerRequest = createProtectedManagerRequestGate(
  authHttpDependencies,
);
const handleAuthRefresh = createHttpAuthRefreshHandler(authHttpDependencies);

apiClient.interceptors.request.use(
  async (config) => {
    await gateProtectedManagerRequest(config as unknown as RetriableHttpRequest);
    return config;
  },
  (error) => Promise.reject(error),
);

apiClient.interceptors.response.use(
  (response) => {
    // If responseType is arraybuffer, blob, etc., return response.data directly
    if (
      response.config.responseType &&
      response.config.responseType !== "json"
    ) {
      return response.data;
    }
    // For JSON, handle optional .data wrapper if it exists and matches our API structure
    if (
      response.data &&
      typeof response.data === "object" &&
      "data" in response.data
    ) {
      return response.data.data;
    }
    return response.data;
  },
  (error) => handleAuthRefresh(error as HttpAuthRefreshError),
);

export const request = apiClient;
