<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import { Loader2 } from "lucide-vue-next";
import { useAppI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { authErrorCode } from "@/services/authErrors";
import { bootstrap, getBootstrapStatus } from "@/services/auth";
import { validateManagerPassword } from "@/services/managerPassword";
import { useAuthStore } from "@/store/authStore";
import BootstrapForm from "./components/BootstrapForm.vue";

const router = useRouter();
const authStore = useAuthStore();
const { t } = useAppI18n();
const password = ref("");
const confirmation = ref("");
const isLoading = ref(false);
const error = ref<string | null>(null);

const errorKeyForCode = (code: number | null) => {
  if (code === 1401) return "bootstrapPage.errors.alreadyInitialized";
  if (code === 1402) return "bootstrapPage.errors.policy";
  if (code === 1403) return "bootstrapPage.errors.busy";
  if (code === 1404 || code === 1405) return "bootstrapPage.errors.unavailable";
  return "bootstrapPage.errors.failed";
};

const retryStatus = async () => {
  const state = await authStore.resolveBootstrapState(getBootstrapStatus, true);
  if (state === "ready") {
    await router.replace({ name: "Login" });
  }
};

const handleSubmit = async () => {
  if (isLoading.value) return;
  error.value = null;
  const validated = validateManagerPassword(password.value);
  if (!validated.valid) {
    error.value = t("bootstrapPage.errors.policy");
    return;
  }
  if (validated.normalized !== confirmation.value.normalize("NFC")) {
    error.value = t("bootstrapPage.errors.confirmation");
    return;
  }

  isLoading.value = true;
  try {
    await bootstrap(validated.normalized);
    authStore.markBootstrapReady();
    await router.replace({ name: "Dashboard" });
  } catch (caught) {
    const code = authErrorCode(caught);
    error.value = t(errorKeyForCode(code));
    if (code === 1401) {
      const state = await authStore.resolveBootstrapState(getBootstrapStatus, true);
      if (state === "ready") {
        await router.replace({ name: "Login" });
      }
    }
  } finally {
    isLoading.value = false;
  }
};
</script>

<template>
  <div class="flex min-h-[calc(100dvh-(var(--app-page-y)*2))] items-center justify-center">
    <div class="w-full max-w-md">
      <div
        v-if="authStore.bootstrapState === 'loading' || authStore.bootstrapState === 'unknown'"
        class="flex items-center justify-center rounded-lg border border-gray-200 bg-white py-16"
      >
        <Loader2 class="h-5 w-5 animate-spin text-gray-500" />
      </div>
      <div
        v-else-if="authStore.bootstrapState === 'error'"
        class="rounded-lg border border-gray-200 bg-white p-5 text-center sm:p-8"
      >
        <h1 class="text-lg font-semibold text-gray-900">
          {{ t("bootstrapPage.statusErrorTitle") }}
        </h1>
        <p class="mt-2 text-sm leading-6 text-gray-500">
          {{ t("bootstrapPage.statusErrorDescription") }}
        </p>
        <Button class="mt-6 w-full" variant="outline" @click="retryStatus">
          {{ t("common.retry") }}
        </Button>
      </div>
      <BootstrapForm
        v-else
        v-model:password="password"
        v-model:confirmation="confirmation"
        :is-loading="isLoading"
        :error="error"
        @submit="handleSubmit"
      />
    </div>
  </div>
</template>
