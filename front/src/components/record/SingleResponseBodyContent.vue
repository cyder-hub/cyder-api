<template>
  <div v-if="contentToDisplay.type !== 'empty'">
    <h4 class="text-sm font-medium text-gray-700 mb-1">{{ title }}</h4>
    <div class="mt-1 text-[10px] bg-gray-50 p-2 rounded-md max-h-[30rem] overflow-y-auto border">
      <template v-if="contentToDisplay.type === 'sse'">
        <SseEventViewer v-for="(ev, idx) in contentToDisplay.content" :key="idx" :event="ev" />
      </template>
      <pre v-else class="whitespace-pre-wrap break-all">{{ contentToDisplay.content }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { parseSse } from '@/utils/sse';
import SseEventViewer from './SseEventViewer.vue';

const props = defineProps<{
  content: string | null;
  title: string;
  status?: string | null;
}>();

const contentToDisplay = computed(() => {
  if (!props.content) return { type: "empty" as const };
  try {
    return {
      type: "json" as const,
      content: JSON.stringify(JSON.parse(props.content), null, 2),
    };
  } catch (e) {}

  if (props.status === "SUCCESS") {
    try {
      const sseEvents = parseSse(props.content);
      if (sseEvents.some((e: any) => e.data && e.data.trim() !== "")) {
        return { type: "sse" as const, content: sseEvents };
      }
    } catch (e) {}
  }
  return { type: "text" as const, content: props.content };
});
</script>
