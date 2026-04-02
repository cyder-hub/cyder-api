<template>
  <div class="py-2 sm:grid sm:grid-cols-3 sm:gap-4">
    <dt class="text-sm font-medium text-gray-500">
      {{ label }}
    </dt>
    <dd class="mt-1 text-sm text-gray-900 sm:mt-0 sm:col-span-2">
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
