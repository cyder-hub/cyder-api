import axios from "axios";
import { useAuthStore } from "@/store/authStore";
import router from "@/router";
import type { AuthTokenPair } from "./types";
import {
  createHttpAuthRefreshHandler,
  type HttpAuthRefreshError,
  type RetriableHttpRequest,
} from "./httpAuthRefresh";
import {
  clearStoredRefreshTokenIfCurrent,
  persistAuthTokenPair,
  readStoredRefreshToken,
} from "./authTokens";

const apiClient = axios.create({
  headers: {
    "Content-Type": "application/json",
  },
});

apiClient.interceptors.request.use(
  (config) => {
    const authStore = useAuthStore();
    const token = authStore.accessToken;
    if (token && !config.headers.Authorization) {
      config.headers.Authorization = `Bearer ${token}`;
    }
    return config;
  },
  (error) => {
    return Promise.reject(error);
  },
);

const handleAuthRefresh = createHttpAuthRefreshHandler({
  readStoredRefreshToken,
  persistAuthTokenPair,
  clearStoredRefreshTokenIfCurrent,
  setAccessToken: (token) => useAuthStore().setAccessToken(token),
  refreshAccessToken: async (refreshToken) => {
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
  redirectToLogin: () => {
    void router.push({ name: "Login" });
  },
});

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
