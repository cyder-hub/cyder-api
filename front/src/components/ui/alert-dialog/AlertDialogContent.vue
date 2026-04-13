<script setup lang="ts">
import type { AlertDialogContentEmits, AlertDialogContentProps } from "reka-ui"
import type { HTMLAttributes } from "vue"
import { reactiveOmit } from "@vueuse/core"
import {
  AlertDialogContent,
  AlertDialogOverlay,
  AlertDialogPortal,
  useForwardPropsEmits,
} from "reka-ui"
import { cn } from "@/lib/utils"

defineOptions({
  inheritAttrs: false,
})

const props = defineProps<AlertDialogContentProps & { class?: HTMLAttributes["class"] }>()
const emits = defineEmits<AlertDialogContentEmits>()

const delegatedProps = reactiveOmit(props, "class")

const forwarded = useForwardPropsEmits(delegatedProps, emits)
</script>

<template>
  <AlertDialogPortal>
    <AlertDialogOverlay
      data-slot="alert-dialog-overlay"
      class="data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 fixed inset-0 z-50 bg-black/80"
    />
    <AlertDialogContent
      data-slot="alert-dialog-content"
      v-bind="{ ...$attrs, ...forwarded }"
      :class="
        cn(
          'bg-background data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 fixed top-[50%] left-[50%] z-50 flex w-[calc(100%-1rem)] max-w-[calc(100%-1rem)] translate-x-[-50%] translate-y-[-50%] flex-col gap-4 overflow-y-auto rounded-xl border p-4 shadow-lg duration-200 [max-height:calc(100dvh-env(safe-area-inset-top)-env(safe-area-inset-bottom)-1rem)] [padding-bottom:calc(env(safe-area-inset-bottom)+1rem)] sm:w-full sm:max-w-lg sm:gap-5 sm:rounded-lg sm:p-6 sm:[max-height:min(90dvh,40rem)] sm:[padding-bottom:1.5rem] [&>[data-slot=alert-dialog-header]]:shrink-0 [&>[data-slot=alert-dialog-footer]]:shrink-0',
          props.class,
        )
      "
    >
      <slot />
    </AlertDialogContent>
  </AlertDialogPortal>
</template>
