<template>
  <div class="flex min-h-[calc(100dvh-(var(--app-page-y)*2))] items-center justify-center">
    <div class="w-full max-w-md">
      <LoginForm
        :stage="stage"
        v-model:password="password"
        v-model:totp-code="totpCode"
        v-model:recovery-password="recoveryPassword"
        v-model:recovery-code="recoveryCode"
        :recovery-manual-secret="recoveryManualSecret"
        :recovery-otpauth-uri="recoveryOtpauthUri"
        :recovery-codes="recoveryCodes"
        v-model:recovery-codes-saved="recoveryCodesSaved"
        :remaining-seconds="remainingSeconds"
        :is-loading="isLoading"
        :error="error"
        @submit="handleLogin"
        @begin-password="beginPasswordStage"
        @begin-recovery="beginRecovery"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { useRouter } from "vue-router";
import { useAppI18n } from "@/i18n";
import {
  completeTotpLogin,
  confirmRecovery,
  getBootstrapStatus,
  login,
  startRecovery,
} from "@/services/auth";
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
  if (code === 1472) return t("loginPage.errors.totpInvalid");
  if (code === 1473 || code === 1474) return t("loginPage.errors.totpWait");
  if (code === 1475 || code === 1476) return t("loginPage.errors.totpRateLimited");
  if (code === 1477) return t("loginPage.errors.challengeExpired");
  if (code === 1478) return t("loginPage.errors.challengeExhausted");
  if (code === 1479) return t("loginPage.errors.totpUnavailable");
  if (code === 1482) return t("loginPage.errors.recoveryInvalid");
  if (code === 1483) return t("loginPage.errors.operationBusy");
  if (code === 1484) return t("loginPage.errors.storageUnavailable");
  if (code === 1485) return t("loginPage.errors.invalidRequest");
  return t("loginPage.loginFailed");
};

const {
  stage,
  password,
  totpCode,
  recoveryPassword,
  recoveryCode,
  recoveryManualSecret,
  recoveryOtpauthUri,
  recoveryCodes,
  recoveryCodesSaved,
  remainingSeconds,
  isLoading,
  error,
  handleLogin,
  beginPasswordStage,
  beginRecovery,
} = useLoginForm({
  login,
  completeTotpLogin,
  startRecovery,
  confirmRecovery,
  errorForCode: loginError,
  onUninitialized: async () => {
    await authStore.resolveBootstrapState(getBootstrapStatus, true);
    await router.replace({ name: "Bootstrap" });
  },
  onSuccess: () => router.replace({ name: "Dashboard" }),
});
</script>
