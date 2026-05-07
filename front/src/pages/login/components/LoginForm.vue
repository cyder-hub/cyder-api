<template>
  <div class="rounded-lg border border-gray-200 bg-white p-5 sm:p-8">
    <div class="mb-6 sm:mb-8">
      <h1 class="text-center text-xl font-semibold tracking-tight text-gray-900 sm:text-2xl">
        {{ $t("loginPage.title") }}
      </h1>
      <p class="mt-2 text-center text-sm leading-6 text-gray-500">
        {{ $t("loginPage.description") }}
      </p>
    </div>

    <form class="space-y-5 sm:space-y-6" @submit.prevent="$emit('submit')">
      <div class="space-y-2">
        <label class="block text-sm font-medium text-gray-700">
          {{ $t("loginPage.passwordLabel") }}
        </label>
        <Input
          v-model="passwordModel"
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
</template>

<script setup lang="ts">
import { computed } from "vue";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

const props = defineProps<{
  password: string;
  isLoading: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  (event: "update:password", value: string): void;
  (event: "submit"): void;
}>();

const passwordModel = computed({
  get: () => props.password,
  set: (value: string) => emit("update:password", value),
});
</script>
