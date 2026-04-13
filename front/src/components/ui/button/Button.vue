<script setup lang="ts">
import type { PrimitiveProps } from "reka-ui"
import type { ButtonHTMLAttributes, HTMLAttributes } from "vue"
import type { ButtonVariants } from "."
import { computed } from "vue"
import { Primitive } from "reka-ui"
import { cn } from "@/lib/utils"
import { buttonVariants } from "."

interface Props extends PrimitiveProps {
  variant?: ButtonVariants["variant"]
  size?: ButtonVariants["size"]
  class?: HTMLAttributes["class"]
  type?: ButtonHTMLAttributes["type"]
}

const props = withDefaults(defineProps<Props>(), {
  as: "button",
})

const resolvedType = computed(() => {
  if (props.asChild || props.as !== "button") {
    return undefined
  }

  return props.type ?? "button"
})
</script>

<template>
  <Primitive
    data-slot="button"
    :data-variant="variant"
    :data-size="size"
    :as="as"
    :as-child="asChild"
    :class="cn(buttonVariants({ variant, size }), props.class)"
    :type="resolvedType"
  >
    <slot />
  </Primitive>
</template>
