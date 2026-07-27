<script setup lang="ts">
import { ref, watch } from "vue";
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
import { disableTotp } from "@/services/auth";
import { authErrorCode } from "@/services/authErrors";
import { toastController } from "@/services/uiFeedback";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{
  "update:open": [value: boolean];
  completed: [];
}>();
const { t } = useAppI18n();

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

const errorKeyForCode = (code: number | null) => {
  if (code === 1472) return "securityPage.errors.totpInvalid";
  if (code === 1473 || code === 1474) return "securityPage.errors.totpWait";
  if (code === 1475 || code === 1476) return "securityPage.errors.rateLimited";
  if (code === 1479) return "securityPage.errors.unavailable";
  if (code === 1480) return "securityPage.errors.stateChanged";
  if (code === 1481) return "securityPage.errors.currentPassword";
  if (code === 1483) return "securityPage.errors.busy";
  if (code === 1484) return "securityPage.errors.storage";
  return "securityPage.errors.failed";
};

const handleOpenChange = (open: boolean) => {
  if (!isLoading.value) emit("update:open", open);
};

const submit = async () => {
  if (isLoading.value || totpCode.value.length !== 6) return;
  isLoading.value = true;
  error.value = null;
  try {
    await disableTotp(currentPassword.value, totpCode.value);
    toastController.success(t("securityPage.disable.success"));
    emit("completed");
    emit("update:open", false);
    reset();
  } catch (caught) {
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
        <DialogTitle>{{ t("securityPage.disable.title") }}</DialogTitle>
        <DialogDescription>{{ t("securityPage.disable.description") }}</DialogDescription>
      </DialogHeader>
      <form class="space-y-4 px-4 py-5 sm:px-6" @submit.prevent="submit">
        <PasswordField
          v-model="currentPassword"
          :disabled="isLoading"
          :label="t('securityPage.currentPassword')"
          :placeholder="t('securityPage.currentPasswordPlaceholder')"
        />
        <TotpField
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
      </form>
      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:px-6">
        <Button variant="ghost" class="w-full sm:w-auto" :disabled="isLoading" @click="handleOpenChange(false)">
          {{ t("common.cancel") }}
        </Button>
        <Button variant="destructive" class="w-full sm:w-auto" :disabled="isLoading || !currentPassword || totpCode.length !== 6" @click="submit">
          <Loader2 v-if="isLoading" class="h-4 w-4 animate-spin" />
          {{ t("securityPage.disable.action") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
