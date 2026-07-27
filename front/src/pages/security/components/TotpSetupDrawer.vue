<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { onBeforeRouteLeave } from "vue-router";
import { useMediaQuery } from "@vueuse/core";
import { encodeQR } from "qr";
import { Check, Copy, Download, Loader2, X } from "lucide-vue-next";
import { useAppI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Drawer,
  DrawerContent,
  DrawerDescription,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer";
import TotpField from "@/pages/login/components/TotpField.vue";
import PasswordField from "@/pages/login/components/PasswordField.vue";
import {
  confirmTotpEnrollment,
  confirmTotpReplacement,
  startTotpEnrollment,
  startTotpReplacement,
} from "@/services/auth";
import { authErrorCode } from "@/services/authErrors";
import { copyText } from "@/utils/clipboard";
import { toastController } from "@/services/uiFeedback";
import TotpQrMatrix from "./TotpQrMatrix.vue";

export type TotpSetupMode = "enroll" | "replace";
type SetupStage = "credentials" | "setup" | "recovery_codes";

const props = defineProps<{
  open: boolean;
  mode: TotpSetupMode;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  completed: [];
}>();

const { t } = useAppI18n();
const isDesktop = useMediaQuery("(min-width: 768px)");
const stage = ref<SetupStage>("credentials");
const currentPassword = ref("");
const currentTotpCode = ref("");
const newTotpCode = ref("");
const manualSecret = ref("");
const otpauthUri = ref("");
const qrMatrix = ref<boolean[][]>([]);
const recoveryCodes = ref<string[]>([]);
const recoveryCodesSaved = ref(false);
const remainingSeconds = ref(0);
const isLoading = ref(false);
const error = ref<string | null>(null);

let setupChallenge = "";
let expiresAt = 0;
let countdownTimer: ReturnType<typeof setInterval> | null = null;
let recoveryDownloadUrl = "";

const title = computed(() =>
  t(
    props.mode === "enroll"
      ? "securityPage.setup.enrollTitle"
      : "securityPage.setup.replaceTitle",
  ),
);

const revokeDownloadUrl = () => {
  if (!recoveryDownloadUrl) return;
  URL.revokeObjectURL(recoveryDownloadUrl);
  recoveryDownloadUrl = "";
};

const stopCountdown = () => {
  if (countdownTimer !== null) {
    clearInterval(countdownTimer);
    countdownTimer = null;
  }
  expiresAt = 0;
  remainingSeconds.value = 0;
};

const clearSetupMaterial = () => {
  stopCountdown();
  setupChallenge = "";
  manualSecret.value = "";
  otpauthUri.value = "";
  qrMatrix.value = [];
  newTotpCode.value = "";
};

const reset = () => {
  clearSetupMaterial();
  revokeDownloadUrl();
  currentPassword.value = "";
  currentTotpCode.value = "";
  recoveryCodes.value = [];
  recoveryCodesSaved.value = false;
  stage.value = "credentials";
  isLoading.value = false;
  error.value = null;
};

const errorKeyForCode = (code: number | null) => {
  if (code === 1471) return "securityPage.errors.totpRequired";
  if (code === 1472) return "securityPage.errors.totpInvalid";
  if (code === 1473 || code === 1474) return "securityPage.errors.totpWait";
  if (code === 1475 || code === 1476) return "securityPage.errors.rateLimited";
  if (code === 1477) return "securityPage.errors.challengeExpired";
  if (code === 1478) return "securityPage.errors.challengeExhausted";
  if (code === 1479) return "securityPage.errors.unavailable";
  if (code === 1480) return "securityPage.errors.stateChanged";
  if (code === 1481) return "securityPage.errors.currentPassword";
  if (code === 1483) return "securityPage.errors.busy";
  if (code === 1484) return "securityPage.errors.storage";
  if (code === 1485) return "securityPage.errors.invalidRequest";
  return "securityPage.errors.failed";
};

const expireSetup = () => {
  clearSetupMaterial();
  stage.value = "credentials";
  error.value = t("securityPage.errors.challengeExpired");
};

const updateCountdown = () => {
  if (!expiresAt) return;
  remainingSeconds.value = Math.max(
    0,
    Math.ceil((expiresAt - Date.now()) / 1_000),
  );
  if (remainingSeconds.value === 0) expireSetup();
};

const startCountdown = (expiresIn: number) => {
  stopCountdown();
  remainingSeconds.value = Math.max(1, Math.floor(expiresIn));
  expiresAt = Date.now() + remainingSeconds.value * 1_000;
  countdownTimer = setInterval(updateCountdown, 1_000);
};

const start = async () => {
  if (isLoading.value) return;
  error.value = null;
  isLoading.value = true;
  try {
    const result =
      props.mode === "enroll"
        ? await startTotpEnrollment(currentPassword.value)
        : await startTotpReplacement(
            currentPassword.value,
            currentTotpCode.value,
          );
    currentPassword.value = "";
    currentTotpCode.value = "";
    setupChallenge = result.setup_challenge;
    manualSecret.value = result.manual_secret;
    otpauthUri.value = result.otpauth_uri;
    qrMatrix.value = encodeQR(result.otpauth_uri, "raw", {
      ecc: "medium",
      border: 4,
    });
    stage.value = "setup";
    startCountdown(result.expires_in);
  } catch (caught) {
    currentTotpCode.value = "";
    error.value = t(errorKeyForCode(authErrorCode(caught)));
  } finally {
    isLoading.value = false;
  }
};

const confirm = async () => {
  if (isLoading.value || !setupChallenge) return;
  error.value = null;
  isLoading.value = true;
  try {
    const result =
      props.mode === "enroll"
        ? await confirmTotpEnrollment(setupChallenge, newTotpCode.value)
        : await confirmTotpReplacement(setupChallenge, newTotpCode.value);
    const issuedCodes = [...(result.recovery_codes ?? [])];
    clearSetupMaterial();
    recoveryCodes.value = issuedCodes;
    recoveryCodesSaved.value = false;
    stage.value = "recovery_codes";
    emit("completed");
  } catch (caught) {
    const code = authErrorCode(caught);
    newTotpCode.value = "";
    if (code === 1477 || code === 1478) {
      clearSetupMaterial();
      stage.value = "credentials";
    }
    error.value = t(errorKeyForCode(code));
  } finally {
    isLoading.value = false;
  }
};

const copyManualSecret = async () => {
  const copied = await copyText(manualSecret.value);
  if (copied) toastController.success(t("securityPage.setup.secretCopied"));
};

const copyRecoveryCodes = async () => {
  const copied = await copyText(recoveryCodes.value.join("\n"));
  if (copied) {
    toastController.success(t("securityPage.setup.recoveryCopied"));
  }
};

const downloadRecoveryCodes = () => {
  revokeDownloadUrl();
  const blob = new Blob([`${recoveryCodes.value.join("\n")}\n`], {
    type: "text/plain;charset=utf-8",
  });
  recoveryDownloadUrl = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = recoveryDownloadUrl;
  link.download = "cyder-manager-recovery-codes.txt";
  link.click();
  setTimeout(revokeDownloadUrl, 0);
};

const finish = () => {
  if (!recoveryCodesSaved.value) return;
  emit("update:open", false);
  reset();
};

const handleOpenChange = (open: boolean) => {
  if (open) {
    emit("update:open", true);
    return;
  }
  if (
    isLoading.value ||
    (stage.value === "recovery_codes" && !recoveryCodesSaved.value)
  ) {
    return;
  }
  emit("update:open", false);
  reset();
};

watch(
  () => props.open,
  (open) => {
    if (open) reset();
    else reset();
  },
);

onBeforeRouteLeave(() => reset());
onBeforeUnmount(reset);
defineExpose({ reset });
</script>

<template>
  <Drawer
    :open="open"
    :direction="isDesktop ? 'right' : 'bottom'"
    :dismissible="stage !== 'recovery_codes' || recoveryCodesSaved"
    @update:open="handleOpenChange"
  >
    <DrawerContent
      class="flex max-h-[92dvh] flex-col border-gray-200 bg-white p-0 outline-none md:h-full md:max-h-full md:max-w-xl md:rounded-none"
    >
      <DrawerHeader class="border-b border-gray-100 px-4 py-4 text-left sm:px-6">
        <div class="flex items-start gap-3">
          <div class="min-w-0 flex-1">
            <DrawerTitle class="text-lg font-semibold text-gray-900">
              {{ title }}
            </DrawerTitle>
            <DrawerDescription class="mt-1 text-sm leading-6 text-gray-500">
              {{ t(`securityPage.setup.${stage}Description`) }}
            </DrawerDescription>
          </div>
          <Button
            v-if="stage !== 'recovery_codes' || recoveryCodesSaved"
            type="button"
            variant="ghost"
            size="icon-sm"
            :aria-label="t('common.close')"
            :disabled="isLoading"
            @click="handleOpenChange(false)"
          >
            <X class="h-4 w-4" />
          </Button>
        </div>
      </DrawerHeader>

      <div class="min-h-0 flex-1 overflow-y-auto px-4 py-5 sm:px-6">
        <form
          v-if="stage === 'credentials'"
          class="space-y-5"
          @submit.prevent="start"
        >
          <PasswordField
            v-model="currentPassword"
            :disabled="isLoading"
            :label="t('securityPage.currentPassword')"
            :placeholder="t('securityPage.currentPasswordPlaceholder')"
          />
          <TotpField
            v-if="mode === 'replace'"
            v-model="currentTotpCode"
            :disabled="isLoading"
            :label="t('securityPage.currentTotpCode')"
            :placeholder="t('securityPage.totpPlaceholder')"
          />
        </form>

        <div v-else-if="stage === 'setup'" class="space-y-5">
          <div class="mx-auto w-full max-w-[18rem] border border-gray-200 bg-white p-3">
            <TotpQrMatrix
              :matrix="qrMatrix"
              :label="t('securityPage.setup.qrLabel')"
            />
          </div>
          <div class="space-y-2 border-y border-gray-100 py-4">
            <p class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ t("securityPage.setup.manualSecret") }}
            </p>
            <div class="flex items-start gap-2">
              <code class="min-w-0 flex-1 break-all font-mono text-sm text-gray-900">
                {{ manualSecret }}
              </code>
              <Button
                type="button"
                variant="outline"
                size="sm"
                :aria-label="t('securityPage.setup.copySecret')"
                @click="copyManualSecret"
              >
                <Copy class="h-4 w-4" />
                {{ t("securityPage.setup.copy") }}
              </Button>
            </div>
          </div>
          <TotpField
            v-model="newTotpCode"
            :disabled="isLoading"
            :label="t('securityPage.newTotpCode')"
            :placeholder="t('securityPage.totpPlaceholder')"
          />
          <p class="text-xs text-gray-500">
            {{ t("securityPage.setup.expires", { seconds: remainingSeconds }) }}
          </p>
        </div>

        <div v-else class="space-y-5">
          <div class="rounded-lg border border-amber-200 bg-amber-50 px-4 py-3">
            <p class="text-sm font-medium text-amber-900">
              {{ t("securityPage.setup.recoveryWarningTitle") }}
            </p>
            <p class="mt-1 text-sm leading-6 text-amber-800">
              {{ t("securityPage.setup.recoveryWarning") }}
            </p>
          </div>
          <ul class="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <li
              v-for="code in recoveryCodes"
              :key="code"
              class="rounded-md border border-gray-200 bg-gray-50 px-3 py-2 font-mono text-sm text-gray-900"
            >
              {{ code }}
            </li>
          </ul>
          <div class="flex flex-col gap-2 sm:flex-row">
            <Button type="button" variant="outline" class="sm:flex-1" @click="copyRecoveryCodes">
              <Copy class="h-4 w-4" />
              {{ t("securityPage.setup.copyRecovery") }}
            </Button>
            <Button type="button" variant="outline" class="sm:flex-1" @click="downloadRecoveryCodes">
              <Download class="h-4 w-4" />
              {{ t("securityPage.setup.downloadRecovery") }}
            </Button>
          </div>
          <label class="flex cursor-pointer items-start gap-3 border-t border-gray-100 pt-4">
            <Checkbox v-model="recoveryCodesSaved" class="mt-0.5" />
            <span class="text-sm leading-6 text-gray-700">
              {{ t("securityPage.setup.recoverySaved") }}
            </span>
          </label>
        </div>

        <div
          v-if="error"
          class="mt-5 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700"
        >
          {{ error }}
        </div>
      </div>

      <DrawerFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button
          v-if="stage !== 'recovery_codes'"
          type="button"
          variant="ghost"
          class="w-full text-gray-600 sm:w-auto"
          :disabled="isLoading"
          @click="handleOpenChange(false)"
        >
          {{ t("common.cancel") }}
        </Button>
        <Button
          v-if="stage === 'credentials'"
          type="button"
          class="w-full sm:w-auto"
          :disabled="isLoading || !currentPassword || (mode === 'replace' && currentTotpCode.length !== 6)"
          @click="start"
        >
          <Loader2 v-if="isLoading" class="h-4 w-4 animate-spin" />
          {{ t("securityPage.setup.continue") }}
        </Button>
        <Button
          v-else-if="stage === 'setup'"
          type="button"
          class="w-full sm:w-auto"
          :disabled="isLoading || newTotpCode.length !== 6"
          @click="confirm"
        >
          <Loader2 v-if="isLoading" class="h-4 w-4 animate-spin" />
          {{ t("securityPage.setup.confirm") }}
        </Button>
        <Button
          v-else
          type="button"
          class="w-full sm:w-auto"
          :disabled="!recoveryCodesSaved"
          @click="finish"
        >
          <Check class="h-4 w-4" />
          {{ t("securityPage.setup.finish") }}
        </Button>
      </DrawerFooter>
    </DrawerContent>
  </Drawer>
</template>
