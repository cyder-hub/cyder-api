<template>
  <div
    class="flex items-center justify-center min-h-screen bg-gray-100 dark:bg-zinc-900"
  >
    <div
      class="p-8 bg-white dark:bg-zinc-800 rounded-lg shadow-md w-full max-w-sm"
    >
      <h2
        class="text-2xl font-semibold text-center text-gray-800 dark:text-gray-100 mb-6"
      >
        Admin Login
      </h2>
      <form @submit.prevent="handleLogin" class="space-y-6">
        <div>
          <label
            class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
          >
            Password
          </label>
          <Input
            v-model="password"
            :disabled="isLoading"
            type="password"
            required
            placeholder="Enter your password"
          />
        </div>

        <div v-if="error" class="text-red-600 text-sm text-center">
          {{ error }}
        </div>

        <Button type="submit" class="w-full" :disabled="isLoading">
          {{ isLoading ? "Logging in..." : "Confirm" }}
        </Button>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { login } from "@/services/auth";

const password = ref("");
const isLoading = ref(false);
const error = ref<string | null>(null);
const router = useRouter();

const handleLogin = async () => {
  isLoading.value = true;
  error.value = null;
  console.log("Attempting login...");

  const success = await login(password.value);

  if (success) {
    console.log("Login successful, navigating to dashboard...");
    router.replace("/dashboard");
  } else {
    console.error("Login failed.");
    error.value = "Login failed. Please check your password.";
  }
  isLoading.value = false;
};
</script>
