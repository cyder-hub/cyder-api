<script setup lang="ts">
import { ref, watch } from "vue";
import { useRouter } from "vue-router";
import { Loader2 } from "lucide-vue-next";
import { useAppI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import PasswordField from "@/pages/login/components/PasswordField.vue";
import TotpField from "@/pages/login/components/TotpField.vue";
import type { ManagerTotpState } from "@/services/types";
import { logoutAll } from "@/services/auth";
import { authErrorCode } from "@/services/authErrors";
import { toastController } from "@/services/uiFeedback";

const props = defineProps<{
  open: boolean;
  totpState: ManagerTotpState;
}>();
const emit = defineEmits<{ "update:open": [value: boolean] }>();
const { t } = useAppI18n();
const router = useRouter();

const currentPassword = ref("");
const totpCode = ref("");
const isLoading = ref(false);
const error = ref<string | null>(null);

const reset = () => {
  currentPassword.value = "";
  totpCode.value = "";
  error.value = null;
};

watch(
  () => props.open,
  () => reset(),
);

const handleOpenChange = (open: boolean) => {
  if (!isLoading.value) emit("update:open", open);
};

const errorKeyForCode = (code: number | null) => {
  if (code === 1481) return "securityPage.errors.currentPassword";
  if (code === 1472) return "securityPage.errors.totpInvalid";
  if (code === 1473 || code === 1474) return "securityPage.errors.totpWait";
  if (code === 1475 || code === 1476) return "securityPage.errors.rateLimited";
  if (code === 1479) return "securityPage.errors.unavailable";
  return "securityPage.sessions.logoutAllFailed";
};

const submit = async () => {
  if (
    isLoading.value ||
    props.totpState === "unavailable" ||
    currentPassword.value.length === 0 ||
    (props.totpState === "enabled" && totpCode.value.length !== 6)
  ) {
    return;
  }
  isLoading.value = true;
  error.value = null;
  try {
    await logoutAll(
      currentPassword.value,
      props.totpState === "enabled" ? totpCode.value : undefined,
    );
    toastController.success(t("securityPage.sessions.logoutAllSuccess"));
    emit("update:open", false);
    await router.replace({ name: "Login" });
  } catch (caught) {
    currentPassword.value = "";
    totpCode.value = "";
    error.value = t(errorKeyForCode(authErrorCode(caught)));
  } finally {
    isLoading.value = false;
  }
};
</script>

<template>
  <Dialog :open="open" @update:open="handleOpenChange">
    <DialogContent class="border border-gray-200 bg-white p-0 sm:max-w-lg">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DialogTitle>{{ t("securityPage.sessions.logoutAllTitle") }}</DialogTitle>
        <DialogDescription>
          {{ t("securityPage.sessions.logoutAllDescription") }}
        </DialogDescription>
      </DialogHeader>
      <div class="space-y-4 px-4 py-5 sm:px-6">
        <PasswordField
          v-model="currentPassword"
          :disabled="isLoading"
          :label="t('securityPage.currentPassword')"
          :placeholder="t('securityPage.currentPasswordPlaceholder')"
        />
        <TotpField
          v-if="totpState === 'enabled'"
          v-model="totpCode"
          :disabled="isLoading"
          :label="t('securityPage.currentTotpCode')"
          :placeholder="t('securityPage.totpPlaceholder')"
        />
        <div
          v-if="error"
          class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700"
        >
          {{ error }}
        </div>
      </div>
      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:px-6">
        <Button variant="ghost" class="w-full sm:w-auto" :disabled="isLoading" @click="handleOpenChange(false)">
          {{ t("common.cancel") }}
        </Button>
        <Button
          variant="destructive"
          class="w-full sm:w-auto"
          :disabled="
            isLoading ||
            totpState === 'unavailable' ||
            currentPassword.length === 0 ||
            (totpState === 'enabled' && totpCode.length !== 6)
          "
          @click="submit"
        >
          <Loader2 v-if="isLoading" class="h-4 w-4 animate-spin" />
          {{ t("securityPage.sessions.logoutAllAction") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
