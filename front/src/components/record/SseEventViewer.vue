<template>
  <div class="mb-2 border-b border-gray-200 pb-2 last:border-b-0 last:pb-0">
    <p class="font-semibold text-gray-600">event: {{ event.event }}</p>
    <pre v-if="eventData.type !== 'empty'" class="mt-1 whitespace-pre text-[10px] font-mono">{{ eventData.content }}</pre>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

const props = defineProps<{
  event: any;
}>();

const eventData = computed(() => {
  if (!props.event.data) return { type: "empty" };
  try {
    return {
      type: "json",
      content: JSON.stringify(JSON.parse(props.event.data), null, 2),
    };
  } catch (e) {
    return { type: "text", content: props.event.data };
  }
});
</script>
