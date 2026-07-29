<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  matrix: boolean[][];
  label: string;
}>();

const size = computed(() => props.matrix.length);
const modules = computed(() =>
  props.matrix.flatMap((row, y) =>
    row.flatMap((filled, x) => (filled ? [{ x, y }] : [])),
  ),
);
</script>

<template>
  <svg
    v-if="size > 0"
    :viewBox="`0 0 ${size} ${size}`"
    role="img"
    :aria-label="label"
    shape-rendering="crispEdges"
    class="aspect-square w-full bg-white"
  >
    <rect :width="size" :height="size" fill="white" />
    <rect
      v-for="module in modules"
      :key="`${module.x}:${module.y}`"
      :x="module.x"
      :y="module.y"
      width="1"
      height="1"
      fill="black"
    />
  </svg>
</template>
