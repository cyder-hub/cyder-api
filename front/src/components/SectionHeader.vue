<script setup lang="ts">
import type { HTMLAttributes } from "vue";

import HelpHint from "@/components/HelpHint.vue";
import { cn } from "@/utils/cn";

const props = withDefaults(
  defineProps<{
    title: string;
    help?: string;
    helpLabel?: string;
    class?: HTMLAttributes["class"];
    titleClass?: HTMLAttributes["class"];
    actionsClass?: HTMLAttributes["class"];
  }>(),
  {
    help: "",
    helpLabel: "Section help",
    class: "",
    titleClass: "",
    actionsClass: "",
  },
);
</script>

<template>
  <div
    :class="
      cn(
        'flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between',
        props.class,
      )
    "
  >
    <div class="min-w-0">
      <div class="flex min-w-0 items-center gap-1.5">
        <h2 :class="cn('text-base font-semibold text-gray-900', props.titleClass)">
          {{ props.title }}
        </h2>
        <HelpHint v-if="$slots.help" :label="props.helpLabel">
          <slot name="help" />
        </HelpHint>
        <HelpHint
          v-else-if="props.help"
          :label="props.helpLabel"
          :content="props.help"
        />
      </div>
      <slot name="meta" />
    </div>
    <div
      v-if="$slots.actions"
      :class="cn('flex flex-wrap items-center gap-2', props.actionsClass)"
    >
      <slot name="actions" />
    </div>
  </div>
</template>
