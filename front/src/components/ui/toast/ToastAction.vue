<script setup lang="ts">
import { ToastAction } from "radix-vue";
import { cn } from "@/lib/utils";
import { buttonVariants } from "@/components/ui/button";
import type { ToastActionProps } from "./types";

const props = defineProps<ToastActionProps>();
</script>

<template>
  <ToastAction
    :class="
      cn(
        buttonVariants({ variant: 'secondary' }),
        'h-8 px-3 text-xs font-semibold',
        props.class,
      )
    "
    v-bind="props"
  >
    <slot />
  </ToastAction>
</template>
