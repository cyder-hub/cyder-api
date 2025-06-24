import { Button } from "@kobalte/core/button";
import { TextField } from "@kobalte/core/text-field";
import { createSignal } from "solid-js";
import { useNavigate } from "@solidjs/router"; // Import useNavigate
import { login } from "../services/auth"; // Import the login function

export default function Login() {
  const [password, setPassword] = createSignal("");
  const [isLoading, setIsLoading] = createSignal(false); // Add loading state
  const [error, setError] = createSignal<string | null>(null); // Add error state
  const navigate = useNavigate(); // Initialize navigate

  const handleLogin = async (e: Event) => {
    e.preventDefault(); // Prevent default form submission
    setIsLoading(true); // Set loading state
    setError(null); // Clear previous errors
    console.log("Attempting login with password:", password());

    const success = await login(password()); // Call the login service function

    if (success) {
      console.log("Login successful, navigating to dashboard...");
      // Navigate to the dashboard or a default route upon successful login
      // Ensure the path matches your router configuration and base path
      navigate("/dashboard", { replace: true });
    } else {
      console.error("Login failed.");
      setError("Login failed. Please check your password."); // Set error message
    }
    setIsLoading(false); // Reset loading state
  };

  return (
    <div class="flex items-center justify-center min-h-screen bg-gray-100">
      <div class="p-8 bg-white rounded-lg shadow-md w-full max-w-sm">
        <h2 class="text-2xl font-semibold text-center text-gray-800 mb-6">
          Admin Login
        </h2>
        <form onSubmit={handleLogin} class="space-y-6">
          <TextField
            value={password()}
            onChange={setPassword}
            class="flex flex-col space-y-1"
            disabled={isLoading()} // Disable input while loading
          >
            <TextField.Label class="text-sm font-medium text-gray-700">
              Password
            </TextField.Label>
            <TextField.Input
              type="password"
              required
              class="w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 sm:text-sm disabled:bg-gray-50"
              placeholder="Enter your password"
            />
            {/* Optional: Add TextField.ErrorMessage for validation feedback */}
          </TextField>

          {error() && ( // Display error message if present
            <div class="text-red-600 text-sm text-center">{error()}</div>
          )}

          <Button
            type="submit"
            class="w-full flex justify-center py-2 px-4 border border-transparent rounded-md shadow-sm text-sm font-medium text-white bg-indigo-600 hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50 disabled:bg-indigo-400"
            disabled={isLoading()} // Disable button while loading
          >
            {isLoading() ? "Logging in..." : "Confirm"} {/* Show loading text */}
          </Button>
        </form>
      </div>
    </div>
  );
}
