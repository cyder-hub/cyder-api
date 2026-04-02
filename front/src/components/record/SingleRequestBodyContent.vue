<template>
  <div>
    <div class="flex items-center justify-between mb-1">
      <h4 class="text-sm font-medium text-gray-700 py-1">{{ title }}</h4>
      <slot name="action"></slot>
    </div>
    <div v-if="displayContent.type !== 'empty'" class="mt-1 overflow-auto rounded-md border bg-gray-50 p-2 text-[10px] max-h-[30rem]">
      <pre class="whitespace-pre font-mono">{{ displayContent.content }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

const props = defineProps<{
  content: string | null;
  title: string;
}>();

const displayContent = computed(() => {
  if (!props.content) return { type: "empty" };
  try {
    return {
      type: "json",
      content: JSON.stringify(JSON.parse(props.content), null, 2),
    };
  } catch (e) {
    return { type: "text", content: props.content };
  }
});
</script>
