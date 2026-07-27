<template>
  <div class="rounded-lg border border-gray-200 bg-white p-5 sm:p-8">
    <div class="mb-6 sm:mb-8">
      <h1 class="text-center text-xl font-semibold tracking-tight text-gray-900 sm:text-2xl">
        {{ $t(titleKey) }}
      </h1>
      <p class="mt-2 text-center text-sm leading-6 text-gray-500">
        {{ $t(descriptionKey) }}
      </p>
    </div>

    <form class="space-y-5 sm:space-y-6" @submit.prevent="$emit('submit')">
      <template v-if="stage === 'password'">
        <PasswordField
          v-model="passwordModel"
          :disabled="isLoading"
          :label="$t('loginPage.passwordLabel')"
          :placeholder="$t('loginPage.passwordPlaceholder')"
        />
      </template>

      <template v-else-if="stage === 'totp'">
        <TotpField
          v-model="totpCodeModel"
          :disabled="isLoading"
          :label="$t('loginPage.totpLabel')"
          :placeholder="$t('loginPage.totpPlaceholder')"
        />
        <p class="text-center text-xs text-gray-500">
          {{ $t("loginPage.challengeExpires", { seconds: remainingSeconds }) }}
        </p>
      </template>

      <template v-else-if="stage === 'recovery_credentials'">
        <PasswordField
          v-model="recoveryPasswordModel"
          :disabled="isLoading"
          :label="$t('loginPage.recoveryPasswordLabel')"
          :placeholder="$t('loginPage.passwordPlaceholder')"
        />
        <div class="space-y-2">
          <label class="block text-sm font-medium text-gray-700">
            {{ $t("loginPage.recoveryCodeLabel") }}
          </label>
          <Input
            v-model="recoveryCodeModel"
            :disabled="isLoading"
            required
            autocomplete="off"
            :placeholder="$t('loginPage.recoveryCodePlaceholder')"
            class="w-full font-mono"
          />
        </div>
      </template>

      <template v-else-if="stage === 'recovery_totp'">
        <div class="space-y-3 border-y border-gray-100 py-4">
          <p class="text-sm text-gray-600">
            {{ $t("loginPage.recoverySetupDescription") }}
          </p>
          <div>
            <p class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("loginPage.manualSecretLabel") }}
            </p>
            <p class="mt-1 break-all font-mono text-xs text-gray-900">
              {{ recoveryManualSecret }}
            </p>
          </div>
          <div>
            <p class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ $t("loginPage.otpauthUriLabel") }}
            </p>
            <p class="mt-1 break-all font-mono text-xs text-gray-600">
              {{ recoveryOtpauthUri }}
            </p>
          </div>
        </div>
        <TotpField
          v-model="totpCodeModel"
          :disabled="isLoading"
          :label="$t('loginPage.newTotpLabel')"
          :placeholder="$t('loginPage.totpPlaceholder')"
        />
        <p class="text-center text-xs text-gray-500">
          {{ $t("loginPage.challengeExpires", { seconds: remainingSeconds }) }}
        </p>
      </template>

      <template v-else>
        <div class="rounded-lg border border-amber-200 bg-amber-50 px-4 py-3">
          <p class="text-sm font-medium text-amber-900">
            {{ $t("securityPage.setup.recoveryWarningTitle") }}
          </p>
          <p class="mt-1 text-sm leading-6 text-amber-800">
            {{ $t("securityPage.setup.recoveryWarning") }}
          </p>
        </div>
        <ul class="space-y-2">
          <li
            v-for="code in recoveryCodes"
            :key="code"
            class="rounded-md border border-gray-200 bg-gray-50 px-3 py-2 font-mono text-xs text-gray-900"
          >
            {{ code }}
          </li>
        </ul>
        <div class="flex flex-col gap-2 sm:flex-row">
          <Button type="button" variant="outline" class="sm:flex-1" @click="copyRecoveryCodes">
            <Copy class="h-4 w-4" />
            {{ $t("securityPage.setup.copyRecovery") }}
          </Button>
          <Button type="button" variant="outline" class="sm:flex-1" @click="downloadRecoveryCodes">
            <Download class="h-4 w-4" />
            {{ $t("securityPage.setup.downloadRecovery") }}
          </Button>
        </div>
        <label class="flex cursor-pointer items-start gap-3 border-t border-gray-100 pt-4">
          <Checkbox v-model="recoveryCodesSavedModel" class="mt-0.5" />
          <span class="text-sm leading-6 text-gray-700">
            {{ $t("securityPage.setup.recoverySaved") }}
          </span>
        </label>
      </template>

      <div
        v-if="error"
        class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-center text-sm text-red-600"
      >
        {{ error }}
      </div>

      <Button
        type="submit"
        class="w-full"
        :disabled="isLoading || (stage === 'recovery_codes' && !recoveryCodesSaved)"
      >
        {{ isLoading ? $t("loginPage.submitting") : $t(submitKey) }}
      </Button>

      <div
        v-if="stage !== 'password' && stage !== 'recovery_codes'"
        class="flex flex-col items-center gap-1 text-sm"
      >
        <Button
          v-if="stage === 'totp'"
          type="button"
          variant="ghost"
          class="text-gray-600"
          :disabled="isLoading"
          @click="$emit('begin-recovery')"
        >
          {{ $t("loginPage.cannotUseAuthenticator") }}
        </Button>
        <Button
          type="button"
          variant="ghost"
          class="text-gray-600"
          :disabled="isLoading"
          @click="$emit('begin-password')"
        >
          {{ $t("loginPage.backToPassword") }}
        </Button>
      </div>

      <Button
        v-else-if="stage === 'password'"
        type="button"
        variant="ghost"
        class="w-full text-gray-600"
        :disabled="isLoading"
        @click="$emit('begin-recovery')"
      >
        {{ $t("loginPage.cannotUseAuthenticator") }}
      </Button>
    </form>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount } from "vue";
import { Copy, Download } from "lucide-vue-next";
import { useAppI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { toastController } from "@/services/uiFeedback";
import { copyText } from "@/utils/clipboard";
import type { LoginStage } from "../types";
import PasswordField from "./PasswordField.vue";
import TotpField from "./TotpField.vue";

const props = defineProps<{
  stage: LoginStage;
  password: string;
  totpCode: string;
  recoveryPassword: string;
  recoveryCode: string;
  recoveryManualSecret: string;
  recoveryOtpauthUri: string;
  recoveryCodes: string[];
  recoveryCodesSaved: boolean;
  remainingSeconds: number;
  isLoading: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  (event: "update:password", value: string): void;
  (event: "update:totpCode", value: string): void;
  (event: "update:recoveryPassword", value: string): void;
  (event: "update:recoveryCode", value: string): void;
  (event: "update:recoveryCodesSaved", value: boolean): void;
  (event: "submit"): void;
  (event: "begin-password"): void;
  (event: "begin-recovery"): void;
}>();

const { t } = useAppI18n();
let recoveryDownloadUrl = "";

const passwordModel = computed({
  get: () => props.password,
  set: (value: string) => emit("update:password", value),
});
const totpCodeModel = computed({
  get: () => props.totpCode,
  set: (value: string) => emit("update:totpCode", value),
});
const recoveryPasswordModel = computed({
  get: () => props.recoveryPassword,
  set: (value: string) => emit("update:recoveryPassword", value),
});
const recoveryCodeModel = computed({
  get: () => props.recoveryCode,
  set: (value: string) => emit("update:recoveryCode", value),
});
const recoveryCodesSavedModel = computed({
  get: () => props.recoveryCodesSaved,
  set: (value: boolean) => emit("update:recoveryCodesSaved", value),
});

const revokeDownloadUrl = () => {
  if (!recoveryDownloadUrl) return;
  URL.revokeObjectURL(recoveryDownloadUrl);
  recoveryDownloadUrl = "";
};

const copyRecoveryCodes = async () => {
  const copied = await copyText(props.recoveryCodes.join("\n"));
  if (copied) {
    toastController.success(t("securityPage.setup.recoveryCopied"));
  }
};

const downloadRecoveryCodes = () => {
  revokeDownloadUrl();
  const blob = new Blob([`${props.recoveryCodes.join("\n")}\n`], {
    type: "text/plain;charset=utf-8",
  });
  recoveryDownloadUrl = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = recoveryDownloadUrl;
  link.download = "cyder-manager-recovery-codes.txt";
  link.click();
  setTimeout(revokeDownloadUrl, 0);
};

const titleKey = computed(() => {
  if (props.stage === "password") return "loginPage.title";
  if (props.stage === "totp") return "loginPage.totpTitle";
  if (props.stage === "recovery_credentials") {
    return "loginPage.recoveryTitle";
  }
  if (props.stage === "recovery_codes") {
    return "securityPage.setup.recoveryWarningTitle";
  }
  return "loginPage.recoverySetupTitle";
});

const descriptionKey = computed(() => {
  if (props.stage === "password") return "loginPage.description";
  if (props.stage === "totp") return "loginPage.totpDescription";
  if (props.stage === "recovery_credentials") {
    return "loginPage.recoveryDescription";
  }
  if (props.stage === "recovery_codes") {
    return "securityPage.setup.recovery_codesDescription";
  }
  return "loginPage.recoverySetupLead";
});

const submitKey = computed(() => {
  if (props.stage === "password") return "loginPage.submit";
  if (props.stage === "totp") return "loginPage.verifyTotp";
  if (props.stage === "recovery_credentials") {
    return "loginPage.startRecovery";
  }
  if (props.stage === "recovery_codes") {
    return "securityPage.setup.finish";
  }
  return "loginPage.confirmRecovery";
});

onBeforeUnmount(revokeDownloadUrl);
</script>
