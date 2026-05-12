<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { AlertCircle, Loader2 } from "lucide-vue-next";

import { cn } from "@/utils/cn";

export interface StatsStripItem {
  key: string | number;
  label: string;
  value: string | number;
  secondary?: string | null;
  tone?: "default" | "success" | "warning" | "danger" | "muted";
  mono?: boolean;
}

const props = withDefaults(
  defineProps<{
    items: StatsStripItem[];
    loading?: boolean;
    error?: string | null;
    emptyText?: string;
    loadingText?: string;
    gridClass?: HTMLAttributes["class"];
    class?: HTMLAttributes["class"];
  }>(),
  {
    loading: false,
    error: null,
    emptyText: "",
    loadingText: "Loading",
    gridClass: "grid-cols-2 sm:grid-cols-4 xl:grid-cols-6",
    class: "",
  },
);

function valueClass(item: StatsStripItem) {
  const toneClass = {
    default: "text-gray-900",
    success: "text-emerald-700",
    warning: "text-amber-700",
    danger: "text-red-700",
    muted: "text-gray-500",
  }[item.tone ?? "default"];

  return cn(
    "mt-1 text-lg font-semibold tracking-tight",
    item.mono ? "font-mono text-base" : "",
    toneClass,
  );
}
</script>

<template>
  <div
    v-if="props.loading"
    :class="
      cn(
        'flex items-center justify-center rounded-lg border border-dashed border-gray-200 bg-white py-10 text-sm text-gray-500',
        props.class,
      )
    "
  >
    <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
    <span>{{ props.loadingText }}</span>
  </div>

  <div
    v-else-if="props.error"
    :class="
      cn(
        'flex items-center gap-2 rounded-lg border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-700',
        props.class,
      )
    "
  >
    <AlertCircle class="h-4 w-4 shrink-0" />
    <span class="min-w-0 break-words">{{ props.error }}</span>
  </div>

  <div
    v-else-if="!props.items.length"
    :class="
      cn(
        'rounded-lg border border-dashed border-gray-200 bg-white px-4 py-8 text-center text-sm text-gray-500',
        props.class,
      )
    "
  >
    <slot name="empty">
      {{ props.emptyText }}
    </slot>
  </div>

  <dl
    v-else
    :class="
      cn(
        'grid gap-px overflow-hidden rounded-lg border border-gray-200 bg-gray-100',
        props.gridClass,
        props.class,
      )
    "
  >
    <div
      v-for="item in props.items"
      :key="item.key"
      class="min-w-0 bg-white px-4 py-3"
    >
      <dt class="truncate text-[11px] font-medium uppercase tracking-wide text-gray-500">
        {{ item.label }}
      </dt>
      <dd :class="valueClass(item)">
        {{ item.value }}
      </dd>
      <dd
        v-if="item.secondary"
        class="mt-0.5 min-w-0 break-words text-xs leading-5 text-gray-500"
      >
        {{ item.secondary }}
      </dd>
    </div>
  </dl>
</template>
