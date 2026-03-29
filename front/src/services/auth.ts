import { useAuthStore } from "@/store/authStore";
import { Api } from "./request";

export async function tryRefreshToken(): Promise<boolean> {
  console.debug("Attempting to refresh token...");
  const refreshToken = localStorage.getItem("auth_token");
  const authStore = useAuthStore();

  if (!refreshToken) {
    console.log("No refresh token found in localStorage.");
    return false;
  }

  try {
    const newAccessToken = await Api.refreshToken(refreshToken);
    authStore.setAccessToken(newAccessToken);
    return true;
  } catch (error) {
    console.error("Error during token refresh:", error);
    localStorage.removeItem("auth_token");
    authStore.setAccessToken(null);
    return false;
  }
}

export async function login(password: string): Promise<boolean> {
  console.debug("Attempting login...");
  try {
    const refreshToken = await Api.login(password);
    localStorage.setItem("auth_token", refreshToken);
    console.log("Login successful, token stored.");
    await tryRefreshToken();
    return true;
  } catch (error) {
    console.error("Error during login:", error);
    return false;
  }
}

export function logout(): void {
  localStorage.removeItem("auth_token");
  const authStore = useAuthStore();
  authStore.setAccessToken(null);
  console.log("Logged out, token removed.");
}
