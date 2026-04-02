<script setup lang="ts">
withDefaults(
  defineProps<{
    title: string;
    description?: string;
    loading?: boolean;
    error?: string | null;
    empty?: boolean;
    headerClass?: string;
  }>(),
  {
    description: "",
    loading: false,
    error: null,
    empty: false,
    headerClass: "flex justify-between items-start gap-4",
  },
);
</script>

<template>
  <div class="p-6 space-y-6">
    <div :class="headerClass">
      <div>
        <h1 class="text-lg font-semibold text-gray-900 tracking-tight">
          {{ title }}
        </h1>
        <p v-if="description" class="mt-1 text-sm text-gray-500">
          {{ description }}
        </p>
      </div>
      <slot name="actions" />
    </div>

    <template v-if="loading">
      <slot name="loading" />
    </template>
    <template v-else-if="error">
      <slot name="error" :error="error" />
    </template>
    <template v-else-if="empty">
      <slot name="empty" />
    </template>
    <template v-else>
      <slot />
    </template>

    <slot name="modals" />
  </div>
</template>
