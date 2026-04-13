<script setup lang="ts">
withDefaults(
  defineProps<{
    title: string;
    description?: string;
    loading?: boolean;
    error?: string | null;
    empty?: boolean;
    headerClass?: string;
    pageClass?: string;
    shellClass?: string;
    contentClass?: string;
  }>(),
  {
    description: "",
    loading: false,
    error: null,
    empty: false,
    headerClass:
      "flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between",
    pageClass: "",
    shellClass: "",
    contentClass: "",
  },
);
</script>

<template>
  <div class="app-page" :class="pageClass">
    <div class="app-page-shell" :class="shellClass">
      <div :class="headerClass">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold text-gray-900 tracking-tight sm:text-xl">
            {{ title }}
          </h1>
          <p v-if="description" class="mt-1 text-sm text-gray-500">
            {{ description }}
          </p>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center">
          <slot name="actions" />
        </div>
      </div>

      <div class="app-section" :class="contentClass">
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
      </div>

      <slot name="modals" />
    </div>
  </div>
</template>
