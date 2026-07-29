<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { Loader2 } from "lucide-vue-next";
import { useAppI18n } from "@/i18n";
import PasswordField from "@/pages/login/components/PasswordField.vue";
import TotpField from "@/pages/login/components/TotpField.vue";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  announceAuthorizationChanged,
  reauthenticateManager,
  recoverAccess,
} from "@/services/auth";
import { authErrorCode } from "@/services/authErrors";
import {
  MANAGER_REAUTH_METHOD_CHANGED_CODE,
  registerManagerReauthPresenter,
} from "@/services/managerReauth";
import { useAuthStore } from "@/store/authStore";

const { t } = useAppI18n();
const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const open = ref(false);
const password = ref("");
const totpCode = ref("");
const error = ref<string | null>(null);
const isLoading = ref(false);
let resolvePrompt: ((accepted: boolean) => void) | null = null;
let unregisterPresenter: (() => void) | null = null;

const canSubmit = computed(
  () =>
    !isLoading.value &&
    authStore.totpState !== "unavailable" &&
    (authStore.totpState === "enabled"
      ? totpCode.value.length === 6
      : password.value.length > 0),
);

const resetCredentials = () => {
  password.value = "";
  totpCode.value = "";
  error.value = null;
};

const finish = (accepted: boolean) => {
  const resolve = resolvePrompt;
  resolvePrompt = null;
  open.value = false;
  resetCredentials();
  resolve?.(accepted);
};

const present = (): Promise<boolean> => {
  resetCredentials();
  open.value = true;
  return new Promise<boolean>((resolve) => {
    resolvePrompt = resolve;
  });
};

const handleOpenChange = (nextOpen: boolean) => {
  if (isLoading.value) return;
  if (!nextOpen) finish(false);
};

const errorKeyForCode = (code: number | null) => {
  if (code === 1481) return "managerReauth.errors.passwordInvalid";
  if (code === 1472) return "managerReauth.errors.totpInvalid";
  if (code === 1473 || code === 1474) return "managerReauth.errors.totpWait";
  if (code === 1475 || code === 1476) {
    return "managerReauth.errors.rateLimited";
  }
  if (code === 1479) return "managerReauth.errors.unavailable";
  if (code === MANAGER_REAUTH_METHOD_CHANGED_CODE) {
    return "managerReauth.errors.methodChanged";
  }
  return "managerReauth.errors.failed";
};

const submit = async () => {
  if (!canSubmit.value) return;
  isLoading.value = true;
  error.value = null;
  try {
    const reauth =
      authStore.totpState === "enabled"
        ? await reauthenticateManager({
            method: "totp",
            totp_code: totpCode.value,
          })
        : await reauthenticateManager({
            method: "password",
            password: password.value,
          });
    authStore.setReauth(reauth);
    announceAuthorizationChanged();
    finish(true);
  } catch (caught) {
    const code = authErrorCode(caught);
    resetCredentials();
    if (code === MANAGER_REAUTH_METHOD_CHANGED_CODE) {
      try {
        await recoverAccess();
      } catch {
        // The auth recovery flow owns session failure classification.
      }
    }
    error.value = t(errorKeyForCode(code));
  } finally {
    isLoading.value = false;
  }
};

const openSecurity = async () => {
  finish(false);
  await router.push({ name: "Security" });
};

watch(
  () => authStore.lifecycle,
  (lifecycle) => {
    if (lifecycle === "anonymous" && open.value) finish(false);
  },
);

watch(
  () => route.fullPath,
  () => {
    if (resolvePrompt) finish(false);
  },
);

onMounted(() => {
  unregisterPresenter = registerManagerReauthPresenter(present);
});

onBeforeUnmount(() => {
  unregisterPresenter?.();
  unregisterPresenter = null;
  if (resolvePrompt) finish(false);
});
</script>

<template>
  <Dialog :open="open" @update:open="handleOpenChange">
    <DialogContent
      overlay-class="z-[200]"
      class="z-[201] border border-gray-200 bg-white p-0 sm:max-w-lg"
    >
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DialogTitle>{{ t("managerReauth.title") }}</DialogTitle>
        <DialogDescription>
          {{ t("managerReauth.description") }}
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-4 px-4 py-5 sm:px-6">
        <PasswordField
          v-if="authStore.totpState === 'disabled'"
          v-model="password"
          :disabled="isLoading"
          :label="t('managerReauth.passwordLabel')"
          :placeholder="t('managerReauth.passwordPlaceholder')"
        />
        <TotpField
          v-else-if="authStore.totpState === 'enabled'"
          v-model="totpCode"
          :disabled="isLoading"
          :label="t('managerReauth.totpLabel')"
          :placeholder="t('managerReauth.totpPlaceholder')"
        />
        <div
          v-else
          class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm leading-6 text-red-700"
        >
          {{ t("managerReauth.unavailable") }}
        </div>
        <div
          v-if="error"
          class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700"
        >
          {{ error }}
        </div>
      </div>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:px-6">
        <Button
          variant="ghost"
          class="w-full text-gray-600 sm:w-auto"
          :disabled="isLoading"
          @click="handleOpenChange(false)"
        >
          {{ t("common.cancel") }}
        </Button>
        <Button
          v-if="authStore.totpState !== 'unavailable'"
          class="w-full sm:w-auto"
          :disabled="!canSubmit"
          @click="submit"
        >
          <Loader2 v-if="isLoading" class="h-4 w-4 animate-spin" />
          {{ t("managerReauth.confirm") }}
        </Button>
        <Button
          v-else
          variant="outline"
          class="w-full sm:w-auto"
          @click="openSecurity"
        >
          {{ t("managerReauth.openSecurity") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
