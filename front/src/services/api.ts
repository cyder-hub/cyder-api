import { getAccessToken, tryRefreshToken } from './auth';
// Optional: Import router or navigation functions if needed for redirects
// import { useNavigate } from "@solidjs/router";

export async function request(url: string, options: RequestInit = {}, isRetry: boolean = false): Promise<any> {
  // const navigate = useNavigate(); // If using router for redirects
  const token = getAccessToken(); // Get token from global signal

  const headers = new Headers(options.headers);

  // Set a default Content-Type if one isn't provided by the caller.
  if (!headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }

  // Add Authorization header only if token exists, potentially overwriting.
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }

  try {
    const response = await fetch(url, { ...options, headers });

    if (response.ok) {
      const contentType = response.headers.get('content-type');
      if (contentType && contentType.includes('application/json')) {
        const data = await response.json();
        // Assuming API response structure is { data: [...] } or { data: {...} }
        // Or sometimes { code: ..., message: ..., data: ... }
        if (data && typeof data.data !== 'undefined') {
          return data.data;
        }
        // Handle cases where response might be just the data directly or different structure
        return data; // Adjust as needed based on your API conventions
      } else {
        return response.text(); // Return as plain text if not JSON
      }
    }

    // --- Token Refresh Logic ---
    if ((response.status === 401 || response.status === 403) && !isRetry) {
      console.log("Access token expired or invalid, attempting refresh...");
      const refreshed = await tryRefreshToken();

      if (refreshed) {
        console.log("Token refreshed, retrying original request...");
        // Retry the request with the new token (obtained via getAccessToken inside the recursive call)
        return request(url, options, true); // Pass isRetry = true to prevent infinite loops
      } else {
        console.error("Token refresh failed. Logging out or redirecting.");
        // Optional: Clear tokens and redirect to login
        // logout(); // Assuming logout clears tokens
        // navigate('/login', { replace: true });
        throw new Error(`Authentication error: Refresh failed (${response.status})`);
      }
    }
    // --- End Token Refresh Logic ---

    // Handle other non-ok responses
    console.error("API request failed:", response.status, response.statusText);
    const errorBody = await response.text();
    console.error("Error body:", errorBody);
    throw new Error(`API request failed: ${response.status} ${response.statusText}`);

  } catch (error) {
    console.error("Network or other error during API request:", error);
    // Re-throw the error so calling code can handle it
    throw error;
  }
}
