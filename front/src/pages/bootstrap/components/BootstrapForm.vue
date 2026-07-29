<script setup lang="ts">
import { computed, ref } from "vue";
import { Eye, EyeOff } from "lucide-vue-next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

const props = defineProps<{
  password: string;
  confirmation: string;
  isLoading: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  (event: "update:password", value: string): void;
  (event: "update:confirmation", value: string): void;
  (event: "submit"): void;
}>();

const showPassword = ref(false);
const passwordModel = computed({
  get: () => props.password,
  set: (value: string) => emit("update:password", value),
});
const confirmationModel = computed({
  get: () => props.confirmation,
  set: (value: string) => emit("update:confirmation", value),
});
</script>

<template>
  <div class="rounded-lg border border-gray-200 bg-white p-5 sm:p-8">
    <div class="mb-6 sm:mb-8">
      <h1 class="text-center text-xl font-semibold tracking-tight text-gray-900 sm:text-2xl">
        {{ $t("bootstrapPage.title") }}
      </h1>
      <p class="mt-2 text-center text-sm leading-6 text-gray-500">
        {{ $t("bootstrapPage.description") }}
      </p>
    </div>

    <div class="mb-5 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2.5 text-sm leading-5 text-amber-800">
      {{ $t("bootstrapPage.firstComeWarning") }}
    </div>

    <form class="space-y-5" @submit.prevent="$emit('submit')">
      <div class="space-y-2">
        <label class="block text-sm font-medium text-gray-700">
          {{ $t("bootstrapPage.passwordLabel") }}
        </label>
        <div class="relative">
          <Input
            v-model="passwordModel"
            :disabled="isLoading"
            :type="showPassword ? 'text' : 'password'"
            required
            autocomplete="new-password"
            class="w-full pr-11"
          />
          <button
            type="button"
            class="absolute inset-y-0 right-0 inline-flex w-11 items-center justify-center text-gray-400 transition-colors hover:text-gray-700"
            :aria-label="$t(showPassword ? 'authPassword.hide' : 'authPassword.show')"
            @click="showPassword = !showPassword"
          >
            <EyeOff v-if="showPassword" class="h-4 w-4" />
            <Eye v-else class="h-4 w-4" />
          </button>
        </div>
        <p class="text-xs leading-5 text-gray-500">
          {{ $t("authPassword.policy") }}
        </p>
      </div>

      <div class="space-y-2">
        <label class="block text-sm font-medium text-gray-700">
          {{ $t("bootstrapPage.confirmLabel") }}
        </label>
        <Input
          v-model="confirmationModel"
          :disabled="isLoading"
          :type="showPassword ? 'text' : 'password'"
          required
          autocomplete="new-password"
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
        {{ isLoading ? $t("bootstrapPage.submitting") : $t("bootstrapPage.submit") }}
      </Button>
    </form>
  </div>
</template>
