<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    title: string;
    description?: string | null;
    selected?: boolean;
    class?: HTMLAttributes["class"];
  }>(),
  {
    description: "",
    selected: false,
  },
);
</script>

<template>
  <article
    :class="
      cn(
        'rounded-xl border bg-white p-4',
        props.selected
          ? 'border-gray-900 bg-gray-50/70 ring-1 ring-gray-200'
          : 'border-gray-200',
        props.class,
      )
    "
  >
    <div class="flex items-start justify-between gap-3">
      <div class="min-w-0">
        <h3 class="truncate text-sm font-semibold text-gray-900">
          {{ props.title }}
        </h3>
        <p
          v-if="props.description"
          class="mt-1 break-words text-xs leading-5 text-gray-500"
        >
          {{ props.description }}
        </p>
      </div>
      <div
        v-if="$slots.header"
        class="flex shrink-0 flex-wrap justify-end gap-1.5"
      >
        <slot name="header" />
      </div>
    </div>

    <div v-if="$slots.default" class="mt-3 space-y-3">
      <slot />
    </div>

    <div
      v-if="$slots.actions"
      class="mt-4 flex flex-col gap-2 border-t border-gray-100 pt-3"
    >
      <slot name="actions" />
    </div>
  </article>
</template>
