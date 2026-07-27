<template>
  <div class="space-y-2">
    <label class="block text-sm font-medium text-gray-700">{{ label }}</label>
    <div class="relative">
      <Input
        v-model="model"
        :disabled="disabled"
        :type="showPassword ? 'text' : 'password'"
        required
        autocomplete="current-password"
        :placeholder="placeholder"
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
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { Eye, EyeOff } from "lucide-vue-next";
import { Input } from "@/components/ui/input";

defineProps<{
  label: string;
  placeholder: string;
  disabled: boolean;
}>();

const model = defineModel<string>({ required: true });
const showPassword = ref(false);
</script>
