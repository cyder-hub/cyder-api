<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { cn } from "@/lib/utils";

const props = defineProps<{
  class?: HTMLAttributes["class"];
}>();
</script>

<template>
  <div
    data-slot="table-container"
    class="app-scroll-x relative w-full overflow-x-auto overflow-y-hidden overscroll-x-contain"
  >
    <table
      data-slot="table"
      :class="cn('min-w-full w-max caption-bottom text-sm', props.class)"
    >
      <slot />
    </table>
  </div>
</template>
