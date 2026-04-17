<script setup lang="ts">
import { ToastDescription } from "reka-ui";
import { cn } from "@/lib/utils";
import type { ToastDescriptionProps } from "./types";

const props = defineProps<ToastDescriptionProps>();
</script>

<template>
  <ToastDescription :class="cn('text-sm opacity-90', props.class)">
    <slot />
  </ToastDescription>
</template>
