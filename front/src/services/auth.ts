import { useAuthStore } from "@/store/authStore";
import { Api } from "./request";

export async function tryRefreshToken(): Promise<boolean> {
  const refreshToken = localStorage.getItem("auth_token");
  const authStore = useAuthStore();

  if (!refreshToken) {
    return false;
  }

  try {
    const newAccessToken = await Api.refreshToken(refreshToken);
    authStore.setAccessToken(newAccessToken);
    return true;
  } catch {
    localStorage.removeItem("auth_token");
    authStore.setAccessToken(null);
    return false;
  }
}

export async function login(password: string): Promise<boolean> {
  try {
    const refreshToken = await Api.login(password);
    localStorage.setItem("auth_token", refreshToken);
    await tryRefreshToken();
    return true;
  } catch {
    return false;
  }
}

export function logout(): void {
  localStorage.removeItem("auth_token");
  const authStore = useAuthStore();
  authStore.setAccessToken(null);
}
