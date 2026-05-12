<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { computed, ref } from "vue";
import { Info } from "lucide-vue-next";

import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/utils/cn";

const props = withDefaults(
  defineProps<{
    label?: string;
    content?: string;
    side?: "top" | "right" | "bottom" | "left";
    align?: "start" | "center" | "end";
    class?: HTMLAttributes["class"];
    contentClass?: HTMLAttributes["class"];
  }>(),
  {
    label: "Help",
    content: "",
    side: "top",
    align: "center",
    class: "",
    contentClass: "",
  },
);

const open = ref(false);
const hasFinePointer = computed(() => {
  if (typeof window === "undefined" || !window.matchMedia) {
    return false;
  }
  return window.matchMedia("(hover: hover) and (pointer: fine)").matches;
});

function showOnHover() {
  if (hasFinePointer.value) {
    open.value = true;
  }
}

function hideOnHover() {
  if (hasFinePointer.value) {
    open.value = false;
  }
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    open.value = false;
  }
}
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <button
        type="button"
        :aria-label="props.label"
        :class="
          cn(
            'inline-flex size-7 shrink-0 items-center justify-center rounded-md text-gray-400 transition hover:bg-gray-100 hover:text-gray-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-gray-300',
            props.class,
          )
        "
        @focus="open = true"
        @mouseenter="showOnHover"
        @mouseleave="hideOnHover"
        @keydown="handleKeydown"
      >
        <Info class="h-4 w-4" aria-hidden="true" />
      </button>
    </PopoverTrigger>
    <PopoverContent
      :side="props.side"
      :align="props.align"
      :class="
        cn(
          'w-72 rounded-lg border-gray-200 bg-white p-3 text-sm leading-6 text-gray-600 shadow-sm',
          props.contentClass,
        )
      "
      @mouseenter="showOnHover"
      @mouseleave="hideOnHover"
    >
      <slot>
        {{ props.content }}
      </slot>
    </PopoverContent>
  </Popover>
</template>
