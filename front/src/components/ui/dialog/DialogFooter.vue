<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { DialogClose } from "reka-ui";
import { cn } from "@/utils/cn";
import { Button } from "@/components/ui/button";
import { useAppI18n } from "@/i18n";

const props = withDefaults(
  defineProps<{
    class?: HTMLAttributes["class"];
    showCloseButton?: boolean;
  }>(),
  {
    showCloseButton: false,
  },
);
const { t } = useAppI18n();
</script>

<template>
  <div
    data-slot="dialog-footer"
    :class="
      cn(
        'sticky bottom-0 z-10 -mx-4 mt-auto flex flex-col-reverse gap-2 border-t border-gray-100 bg-background/95 px-4 pt-3 pb-[calc(env(safe-area-inset-bottom)+0.25rem)] backdrop-blur supports-[backdrop-filter]:bg-background/80 sm:static sm:mx-0 sm:border-0 sm:bg-transparent sm:px-0 sm:pt-0 sm:pb-0 sm:backdrop-blur-none sm:flex-row sm:justify-end',
        props.class,
      )
    "
  >
    <slot />
    <DialogClose v-if="showCloseButton" as-child>
      <Button variant="outline">{{ t("ui.dialog.close") }}</Button>
    </DialogClose>
  </div>
</template>
