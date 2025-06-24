import { createSignal } from 'solid-js';

const [accessTokenSignal, setAccessTokenSignal] = createSignal<string | null>(null);
export const getAccessToken = accessTokenSignal;

export async function tryRefreshToken(): Promise<boolean> {
  const refreshToken = localStorage.getItem("auth_token");

  if (!refreshToken) {
    console.log("No refresh token found in localStorage.");
    return false;
  }

  try {
    const response = await fetch("/ai/manager/api/auth/refresh_token", {
      method: "POST",
      headers: {
        Authorization: `Bearer ${refreshToken}`,
      },
    });

    if (response.ok) {
      const data = await response.json();
      const newAccessToken = data.data; // Assuming the endpoint returns { data: "new_access_token" }
      setAccessTokenSignal(newAccessToken);
      return true;
    } else {
      console.error("Failed to refresh token:", response.status, await response.text());
      // Clear the invalid token if refresh fails
      localStorage.removeItem("auth_token");
      return false;
    }
  } catch (error) {
    console.error("Error during token refresh:", error);
    return false;
  }
}

// Placeholder for login function if needed later
export async function login(password: string): Promise<boolean> {
    try {
        const response = await fetch("/ai/manager/api/auth/login", {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify({ key: password }),
        });

        if (response.ok) {
            const data = await response.json();
            const refreshToken = data.data;
            localStorage.setItem("auth_token", refreshToken);
            console.log("Login successful, token stored.");
            await tryRefreshToken(); // Fetch initial access token
            return true;
        } else {
            console.error("Login failed:", response.status, await response.text());
            return false;
        }
    } catch (error) {
        console.error("Error during login:", error);
        return false;
    }
}

// Placeholder for logout function
export function logout(): void {
    localStorage.removeItem("auth_token");
    setAccessTokenSignal(null); // Clear the global access token
    console.log("Logged out, token removed.");
    // Usually followed by a redirect to login page
}
