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
    helpLabel: "Page help",
    class: "",
    titleClass: "",
    actionsClass: "",
  },
);
</script>

<template>
  <header
    :class="
      cn(
        'flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between',
        props.class,
      )
    "
  >
    <div class="min-w-0">
      <div class="flex min-w-0 items-center gap-1.5">
        <h1
          :class="
            cn(
              'min-w-0 text-lg font-semibold text-gray-900 tracking-tight sm:text-xl',
              props.titleClass,
            )
          "
        >
          {{ props.title }}
        </h1>
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
      :class="
        cn(
          'flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center',
          props.actionsClass,
        )
      "
    >
      <slot name="actions" />
    </div>
  </header>
</template>
