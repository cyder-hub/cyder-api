import axios from "axios";
import { useAuthStore } from "@/store/authStore";
import router from "@/router";

const apiClient = axios.create({
  headers: {
    "Content-Type": "application/json",
  },
});

apiClient.interceptors.request.use(
  (config) => {
    const authStore = useAuthStore();
    const token = authStore.accessToken;
    if (token) {
      config.headers.Authorization = `Bearer ${token}`;
    }
    return config;
  },
  (error) => {
    return Promise.reject(error);
  },
);

let isRefreshing = false;
let failedQueue: {
  resolve: (value?: any) => void;
  reject: (reason?: any) => void;
}[] = [];

const processQueue = (error: any, token: string | null = null) => {
  failedQueue.forEach((prom) => {
    if (error) {
      prom.reject(error);
    } else {
      prom.resolve(token);
    }
  });
  failedQueue = [];
};

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
  async (error) => {
    const originalRequest = error.config;
    const authStore = useAuthStore();

    if (error.response?.status === 401 && !originalRequest._retry) {
      if (isRefreshing) {
        return new Promise((resolve, reject) => {
          failedQueue.push({ resolve, reject });
        })
          .then((token) => {
            originalRequest.headers["Authorization"] = "Bearer " + token;
            return apiClient(originalRequest);
          })
          .catch((err) => {
            return Promise.reject(err);
          });
      }

      originalRequest._retry = true;
      isRefreshing = true;

      const refreshToken = localStorage.getItem("auth_token");
      if (!refreshToken) {
        // Handle logout
        return Promise.reject(error);
      }

      try {
        const response = await axios.post(
          "/ai/manager/api/auth/refresh_token",
          {},
          {
            headers: { Authorization: `Bearer ${refreshToken}` },
          },
        );
        const newAccessToken = response.data.data;
        authStore.setAccessToken(newAccessToken);
        originalRequest.headers.Authorization = `Bearer ${newAccessToken}`;
        processQueue(null, newAccessToken);
        return apiClient(originalRequest);
      } catch (refreshError: any) {
        processQueue(refreshError, null);
        if (refreshError.response?.status === 401) {
          authStore.setAccessToken(null);
          localStorage.removeItem("auth_token");
          router.push({ name: "Login" });
        }
        return Promise.reject(refreshError);
      } finally {
        isRefreshing = false;
      }
    }

    return Promise.reject(error);
  },
);

export const request = apiClient;
