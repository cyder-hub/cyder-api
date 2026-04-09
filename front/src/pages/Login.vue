<template>
  <div class="flex min-h-[calc(100dvh-(var(--app-page-y)*2))] items-center justify-center">
    <div class="w-full max-w-md">
      <div class="rounded-2xl border border-gray-200 bg-white p-5 sm:p-8">
        <div class="mb-6 sm:mb-8">
          <h1 class="text-center text-xl font-semibold tracking-tight text-gray-900 sm:text-2xl">
            {{ $t("loginPage.title") }}
          </h1>
          <p class="mt-2 text-center text-sm leading-6 text-gray-500">
            {{ $t("loginPage.description") }}
          </p>
        </div>

        <form @submit.prevent="handleLogin" class="space-y-5 sm:space-y-6">
          <div class="space-y-2">
            <label class="block text-sm font-medium text-gray-700">
              {{ $t("loginPage.passwordLabel") }}
            </label>
            <Input
              v-model="password"
              :disabled="isLoading"
              type="password"
              required
              autocomplete="current-password"
              :placeholder="$t('loginPage.passwordPlaceholder')"
              class="w-full"
            />
          </div>

          <div
            v-if="error"
            class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-center text-sm text-red-600"
          >
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
