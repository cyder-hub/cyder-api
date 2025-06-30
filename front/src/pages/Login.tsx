import { Button } from "../components/ui/Button";
import { TextField } from "../components/ui/Input";
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
            disabled={isLoading()} // Disable input while loading
            label="Password"
            type="password"
            required
            placeholder="Enter your password"
          />

          {error() && ( // Display error message if present
            <div class="text-red-600 text-sm text-center">{error()}</div>
          )}

          <Button
            type="submit"
            variant="primary"
            class="w-full"
            disabled={isLoading()} // Disable button while loading
          >
            {isLoading() ? "Logging in..." : "Confirm"} {/* Show loading text */}
          </Button>
        </form>
      </div>
    </div>
  );
}
