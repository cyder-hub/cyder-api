<template>
  <div class="mb-2 border-b border-gray-200 pb-2 last:border-b-0 last:pb-0">
    <p class="break-all text-xs font-semibold text-gray-600">
      {{ $t("recordPage.detailDialog.sse.event", { name: event.event }) }}
    </p>
    <pre
      v-if="eventData.type !== 'empty'"
      class="mt-1 whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-gray-700"
    >{{ eventData.content }}</pre>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from "vue-i18n";

const props = defineProps<{
  event: any;
}>();

const { t: $t } = useI18n();

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
