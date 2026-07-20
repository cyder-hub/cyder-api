<script setup lang="ts">
import { ref, watch } from "vue";
import { Eye, EyeOff } from "lucide-vue-next";
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
import { Input } from "@/components/ui/input";
import { rotatePassword } from "@/services/auth";
import { authErrorCode } from "@/services/authErrors";
import { validateManagerPassword } from "@/services/managerPassword";
import { toastController } from "@/services/uiFeedback";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (event: "update:open", value: boolean): void }>();
const { t } = useAppI18n();

const currentPassword = ref("");
const newPassword = ref("");
const confirmation = ref("");
const showPasswords = ref(false);
const isLoading = ref(false);
const error = ref<string | null>(null);

const reset = () => {
  currentPassword.value = "";
  newPassword.value = "";
  confirmation.value = "";
  showPasswords.value = false;
  error.value = null;
};

watch(
  () => props.open,
  (open) => {
    if (open) reset();
  },
);

const handleOpenChange = (open: boolean) => {
  if (!isLoading.value) emit("update:open", open);
};

const errorKeyForCode = (code: number | null) => {
  if (code === 1421) return "rotatePassword.errors.currentPassword";
  if (code === 1422) return "rotatePassword.errors.policy";
  if (code === 1423) return "rotatePassword.errors.samePassword";
  if (code === 1424) return "rotatePassword.errors.busy";
  if (code === 1425) return "rotatePassword.errors.conflict";
  if (code === 1426) return "rotatePassword.errors.unavailable";
  return "rotatePassword.errors.failed";
};

const submit = async () => {
  if (isLoading.value) return;
  error.value = null;
  const validated = validateManagerPassword(newPassword.value);
  if (!validated.valid) {
    error.value = t("rotatePassword.errors.policy");
    return;
  }
  if (validated.normalized !== confirmation.value.normalize("NFC")) {
    error.value = t("rotatePassword.errors.confirmation");
    return;
  }

  isLoading.value = true;
  try {
    await rotatePassword(currentPassword.value, validated.normalized);
    toastController.success(t("rotatePassword.success"));
    emit("update:open", false);
  } catch (caught) {
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
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{ t("rotatePassword.title") }}
        </DialogTitle>
        <DialogDescription class="text-sm text-gray-500">
          {{ t("rotatePassword.description") }}
        </DialogDescription>
      </DialogHeader>

      <form class="space-y-4 px-4 sm:px-6" @submit.prevent="submit">
        <div class="space-y-2">
          <label class="text-sm font-medium text-gray-700">
            {{ t("rotatePassword.currentPassword") }}
          </label>
          <Input
            v-model="currentPassword"
            :type="showPasswords ? 'text' : 'password'"
            :disabled="isLoading"
            autocomplete="current-password"
            required
            class="w-full"
          />
        </div>
        <div class="space-y-2">
          <label class="text-sm font-medium text-gray-700">
            {{ t("rotatePassword.newPassword") }}
          </label>
          <div class="relative">
            <Input
              v-model="newPassword"
              :type="showPasswords ? 'text' : 'password'"
              :disabled="isLoading"
              autocomplete="new-password"
              required
              class="w-full pr-11"
            />
            <button
              type="button"
              class="absolute inset-y-0 right-0 inline-flex w-11 items-center justify-center text-gray-400 transition-colors hover:text-gray-700"
              :aria-label="t(showPasswords ? 'authPassword.hide' : 'authPassword.show')"
              @click="showPasswords = !showPasswords"
            >
              <EyeOff v-if="showPasswords" class="h-4 w-4" />
              <Eye v-else class="h-4 w-4" />
            </button>
          </div>
          <p class="text-xs leading-5 text-gray-500">{{ t("authPassword.policy") }}</p>
        </div>
        <div class="space-y-2">
          <label class="text-sm font-medium text-gray-700">
            {{ t("rotatePassword.confirmPassword") }}
          </label>
          <Input
            v-model="confirmation"
            :type="showPasswords ? 'text' : 'password'"
            :disabled="isLoading"
            autocomplete="new-password"
            required
            class="w-full"
          />
        </div>
        <div
          v-if="error"
          class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-600"
        >
          {{ error }}
        </div>
      </form>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:px-6">
        <Button variant="ghost" class="w-full text-gray-600 sm:w-auto" :disabled="isLoading" @click="handleOpenChange(false)">
          {{ t("common.cancel") }}
        </Button>
        <Button class="w-full sm:w-auto" :disabled="isLoading" @click="submit">
          {{ isLoading ? t("common.saving") : t("rotatePassword.submit") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
