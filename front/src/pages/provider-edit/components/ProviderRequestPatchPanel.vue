<template>
  <Drawer :open="props.open" :direction="isDesktop ? 'right' : 'bottom'" @update:open="(open) => emit('update:open', open)">
    <DrawerContent class="flex max-h-[94dvh] flex-col border-gray-200 bg-white p-0 outline-none md:h-full md:max-h-full md:max-w-2xl md:rounded-none">
      <RequestPatchVariantManager
        :open="props.open"
        :provider-id="props.providerId"
        :source-id="props.sourceId"
        :title="props.sourceTitle"
        mode="source"
      />
    </DrawerContent>
  </Drawer>
</template>

<script setup lang="ts">
import { useMediaQuery } from "@vueuse/core";
import {
  Drawer,
  DrawerContent,
} from "@/components/ui/drawer";
import RequestPatchVariantManager from "@/components/request-patch/RequestPatchVariantManager.vue";

const props = defineProps<{
  open: boolean;
  providerId: number;
  sourceId: number;
  sourceTitle: string;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
}>();

const isDesktop = useMediaQuery("(min-width: 768px)");
</script>
