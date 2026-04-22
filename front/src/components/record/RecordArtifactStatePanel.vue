<template>
  <div :class="panelClass">
    <div v-if="loading" class="py-4 text-center">
      <div class="mb-2 inline-block h-8 w-8 animate-spin rounded-full border-b-2 border-gray-900"></div>
      <div class="font-medium">{{ title }}</div>
      <div v-if="message" class="mt-1 text-sm">{{ message }}</div>
    </div>

    <template v-else>
      <div class="font-medium">{{ title }}</div>
      <div v-if="message" class="mt-1 text-sm">{{ message }}</div>
      <ul v-if="details.length > 0" class="mt-2 list-inside list-disc space-y-1 text-sm">
        <li v-for="detail in details" :key="detail">
          {{ detail }}
        </li>
      </ul>
      <Button
        v-if="retryable"
        variant="outline"
        size="sm"
        class="mt-3"
        @click="$emit('retry')"
      >
        {{ $t("common.retry") }}
      </Button>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { Button } from "@/components/ui/button";

const props = withDefaults(
  defineProps<{
    title: string;
    message?: string | null;
    details?: string[];
    tone?: "neutral" | "danger" | "warning";
    loading?: boolean;
    retryable?: boolean;
  }>(),
  {
    message: "",
    details: () => [],
    tone: "neutral",
    loading: false,
    retryable: false,
  },
);

defineEmits<{
  retry: [];
}>();

const { t: $t } = useI18n();

const panelClass = computed(() => {
  if (props.tone === "danger") {
    return "rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-red-700";
  }
  if (props.tone === "warning") {
    return "rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-amber-900";
  }
  return "rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-gray-500";
});
</script>
