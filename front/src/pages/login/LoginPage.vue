<template>
  <div class="flex min-h-[calc(100dvh-(var(--app-page-y)*2))] items-center justify-center">
    <div class="w-full max-w-md">
      <LoginForm
        v-model:password="password"
        :is-loading="isLoading"
        :error="error"
        @submit="handleLogin"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { useRouter } from "vue-router";
import { useAppI18n } from "@/i18n";
import { getBootstrapStatus, login } from "@/services/auth";
import { useAuthStore } from "@/store/authStore";
import LoginForm from "./components/LoginForm.vue";
import { useLoginForm } from "./composables/useLoginForm";

const { t } = useAppI18n();
const router = useRouter();
const authStore = useAuthStore();

const loginError = (code: number | null) => {
  if (code === 1413) return t("loginPage.errors.busy");
  if (code === 1414 || code === 1415) return t("loginPage.errors.unavailable");
  if (code === 1416) return t("loginPage.errors.invalidRequest");
  return t("loginPage.loginFailed");
};

const { password, isLoading, error, handleLogin } = useLoginForm({
  login,
  errorForCode: loginError,
  onUninitialized: async () => {
    await authStore.resolveBootstrapState(getBootstrapStatus, true);
    await router.replace({ name: "Bootstrap" });
  },
  onSuccess: () => router.replace({ name: "Dashboard" }),
});
</script>
