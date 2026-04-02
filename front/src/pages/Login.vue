<template>
  <div class="flex min-h-screen items-center justify-center bg-gray-100 px-4">
    <div class="w-full max-w-sm rounded-lg border border-gray-200 bg-white p-8 shadow-sm">
      <h2 class="mb-2 text-center text-2xl font-semibold text-gray-900">
        {{ $t("loginPage.title") }}
      </h2>
      <p class="mb-6 text-center text-sm text-gray-500">
        {{ $t("loginPage.description") }}
      </p>
      <form @submit.prevent="handleLogin" class="space-y-6">
        <div>
          <label class="mb-1 block text-sm font-medium text-gray-700">
            {{ $t("loginPage.passwordLabel") }}
          </label>
          <Input
            v-model="password"
            :disabled="isLoading"
            type="password"
            required
            :placeholder="$t('loginPage.passwordPlaceholder')"
          />
        </div>

        <div v-if="error" class="text-red-600 text-sm text-center">
          {{ error }}
        </div>

        <Button type="submit" class="w-full" :disabled="isLoading">
          {{
            isLoading
              ? $t("loginPage.submitting")
              : $t("loginPage.submit")
          }}
        </Button>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { login } from "@/services/auth";

const { t } = useI18n();
const password = ref("");
const isLoading = ref(false);
const error = ref<string | null>(null);
const router = useRouter();

const handleLogin = async () => {
  isLoading.value = true;
  error.value = null;

  const success = await login(password.value);

  if (success) {
    router.replace("/dashboard");
  } else {
    error.value = t("loginPage.loginFailed");
  }
  isLoading.value = false;
};
</script>
