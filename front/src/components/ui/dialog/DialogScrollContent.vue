<script setup lang="ts">
import type { DialogContentEmits, DialogContentProps } from "reka-ui";
import type { HTMLAttributes } from "vue";
import { reactiveOmit } from "@vueuse/core";
import { X } from "lucide-vue-next";
import {
  DialogClose,
  DialogContent,
  DialogOverlay,
  DialogPortal,
  useForwardPropsEmits,
} from "reka-ui";
import { cn } from "@/lib/utils";

defineOptions({
  inheritAttrs: false,
});

const props = defineProps<
  DialogContentProps & { class?: HTMLAttributes["class"] }
>();
const emits = defineEmits<DialogContentEmits>();

const delegatedProps = reactiveOmit(props, "class");

const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <DialogPortal>
    <DialogOverlay
      class="fixed inset-0 z-50 grid place-items-center overflow-y-auto bg-black/80 px-2 py-2 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 sm:px-4 sm:py-6"
    >
      <DialogContent
        :class="
          cn(
            'relative z-50 flex w-full max-w-[calc(100%-1rem)] flex-col gap-4 overflow-y-auto rounded-xl border border-border bg-background p-4 shadow-lg duration-200 [max-height:calc(100dvh-env(safe-area-inset-top)-env(safe-area-inset-bottom)-1rem)] [padding-bottom:calc(env(safe-area-inset-bottom)+1rem)] sm:my-8 sm:max-w-lg sm:gap-5 sm:rounded-lg sm:p-6 sm:[max-height:min(90dvh,48rem)] sm:[padding-bottom:1.5rem] [&>[data-slot=dialog-header]]:shrink-0 [&>[data-slot=dialog-header]]:pr-10 [&>[data-slot=dialog-footer]]:shrink-0',
            props.class,
          )
        "
        v-bind="{ ...$attrs, ...forwarded }"
        @pointer-down-outside="
          (event) => {
            const originalEvent = event.detail.originalEvent;
            const target = originalEvent.target as HTMLElement;
            if (
              originalEvent.offsetX > target.clientWidth ||
              originalEvent.offsetY > target.clientHeight
            ) {
              event.preventDefault();
            }
          }
        "
      >
        <slot />

        <DialogClose
          class="absolute top-3 right-3 rounded-md p-2 transition-colors hover:bg-secondary sm:top-4 sm:right-4 sm:p-1"
        >
          <X class="w-4 h-4" />
          <span class="sr-only">Close</span>
        </DialogClose>
      </DialogContent>
    </DialogOverlay>
  </DialogPortal>
</template>
