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
import { login } from "@/services/auth";
import LoginForm from "./components/LoginForm.vue";
import { useLoginForm } from "./composables/useLoginForm";

const { t } = useAppI18n();
const router = useRouter();

const { password, isLoading, error, handleLogin } = useLoginForm({
  login,
  translate: t,
  onSuccess: () => router.replace({ name: "Dashboard" }),
});
</script>
