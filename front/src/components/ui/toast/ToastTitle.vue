<script setup lang="ts">
import { ToastTitle } from "radix-vue";
import { cn } from "@/lib/utils";
import type { ToastTitleProps } from "./types";

const props = defineProps<ToastTitleProps>();
</script>

<template>
  <ToastTitle :class="cn('text-sm font-semibold [&+div]:text-xs', props.class)">
    <slot />
  </ToastTitle>
</template>
