<script setup lang="ts">
import type { PaginationFirstProps } from "reka-ui";
import type { HTMLAttributes } from "vue";
import type { ButtonVariants } from "@/components/ui/button";
import { reactiveOmit } from "@vueuse/core";
import { ChevronLeftIcon } from "lucide-vue-next";
import { PaginationFirst, useForwardProps } from "reka-ui";
import { cn } from "@/utils/cn";
import { buttonVariants } from "@/components/ui/button";
import { useAppI18n } from "@/i18n";

const props = withDefaults(
  defineProps<
    PaginationFirstProps & {
      size?: ButtonVariants["size"];
      class?: HTMLAttributes["class"];
    }
  >(),
  {
    size: "default",
  },
);

const delegatedProps = reactiveOmit(props, "class", "size");
const forwarded = useForwardProps(delegatedProps);
const { t } = useAppI18n();
</script>

<template>
  <PaginationFirst
    data-slot="pagination-first"
    :class="
      cn(
        buttonVariants({ variant: 'ghost', size }),
        'min-w-10 gap-1 px-2.5 sm:min-w-0 sm:pr-2.5',
        props.class,
      )
    "
    v-bind="forwarded"
  >
    <slot>
      <ChevronLeftIcon />
      <span class="hidden sm:block">{{ t("ui.pagination.firstPage") }}</span>
    </slot>
  </PaginationFirst>
</template>
