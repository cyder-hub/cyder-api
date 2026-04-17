<script setup lang="ts">
import { ToastProvider } from "reka-ui";
import type { ToastProps } from "./types";

const props = defineProps<ToastProps>();
</script>

<template>
  <ToastProvider>
    <slot />
  </ToastProvider>
</template>
