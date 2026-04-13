<template>
  <div class="py-3 sm:grid sm:grid-cols-3 sm:gap-4">
    <dt class="text-xs font-medium uppercase tracking-wide text-gray-500 sm:text-sm sm:normal-case sm:tracking-normal">
      {{ label }}
    </dt>
    <dd class="mt-1 break-words text-sm text-gray-900 sm:col-span-2 sm:mt-0">
      <slot></slot>
      <template v-if="!$slots.default">/</template>
    </dd>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  label: string;
}>();
</script>
