import axios from "axios";
import { useAuthStore } from "@/store/authStore";
import {
  recoverManagerAccess,
  restoreManagerSession,
  revokeManagerSession,
} from "./authRuntime";
import {
  applyManagerAuthBrowserHeaders,
  createHttpAuthRefreshHandler,
  createProtectedManagerRequestGate,
  type HttpAuthRefreshDependencies,
  type HttpAuthRefreshError,
  type RetriableHttpRequest,
} from "./httpAuthRefresh";
import { getAccessToken } from "./authTokens";

const apiClient = axios.create({
  headers: {
    "Content-Type": "application/json",
  },
});

const authHttpDependencies = {
  getAccessToken,
  getLifecycle: () => useAuthStore().lifecycle,
  restoreSession: restoreManagerSession,
  recoverAccess: recoverManagerAccess,
  revokeSession: revokeManagerSession,
  retryRequest: (originalRequest: RetriableHttpRequest) =>
    apiClient(originalRequest),
} satisfies HttpAuthRefreshDependencies;

const gateProtectedManagerRequest = createProtectedManagerRequestGate(
  authHttpDependencies,
);
const handleAuthRefresh = createHttpAuthRefreshHandler(authHttpDependencies);

apiClient.interceptors.request.use(
  async (config) => {
    const request = config as unknown as RetriableHttpRequest;
    applyManagerAuthBrowserHeaders(request);
    await gateProtectedManagerRequest(request);
    return config;
  },
  (error) => Promise.reject(error),
);

apiClient.interceptors.response.use(
  (response) => {
    if (
      response.config.responseType &&
      response.config.responseType !== "json"
    ) {
      return response.data;
    }
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
