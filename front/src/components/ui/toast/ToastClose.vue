<script setup lang="ts">
import { ToastClose } from "reka-ui";
import { XIcon } from "lucide-vue-next";
import { cn } from "@/lib/utils";
import type { ToastCloseProps } from "./types";

const props = defineProps<ToastCloseProps>();
</script>

<template>
  <ToastClose
    :class="
      cn(
        'absolute right-1 top-1 rounded-md p-1 text-foreground/50 opacity-0 transition-opacity hover:text-foreground focus:opacity-100 focus:outline-none focus:ring-1 group-hover:opacity-100',
        props.class,
      )
    "
    v-bind="$attrs"
  >
    <XIcon class="h-4 w-4" />
  </ToastClose>
</template>
