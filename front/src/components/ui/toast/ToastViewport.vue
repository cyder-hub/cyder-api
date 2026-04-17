<script setup lang="ts">
import { ToastViewport } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps<{
  class?: string;
}>();
</script>

<template>
  <ToastViewport
    :class="
      cn(
        'pointer-events-none fixed inset-x-0 top-0 z-[100] flex max-h-screen w-full flex-col-reverse gap-2 p-3 [padding-top:calc(env(safe-area-inset-top)+0.75rem)] sm:inset-x-auto sm:bottom-0 sm:right-0 sm:top-auto sm:w-full sm:max-w-[420px] sm:flex-col sm:p-4',
        props.class,
      )
    "
    v-bind="$attrs"
  />
</template>
