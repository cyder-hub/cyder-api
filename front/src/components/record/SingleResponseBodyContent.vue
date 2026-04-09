<template>
  <div
    v-if="contentToDisplay.type !== 'empty'"
    class="rounded-xl border border-gray-200 bg-white p-3 sm:p-4"
  >
    <div class="mb-2 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
      <h4 class="text-sm font-medium text-gray-700">{{ title }}</h4>
      <Button
        type="button"
        variant="outline"
        size="sm"
        class="h-8 gap-1.5 px-2 text-xs"
        @click="handleCopy"
      >
        <Check v-if="isCopied" class="h-3.5 w-3.5" />
        <Copy v-else class="h-3.5 w-3.5" />
        {{ isCopied ? $t("recordPage.copy.copied") : $t("recordPage.copy.action") }}
      </Button>
    </div>
    <div
      class="mt-1 max-h-[26rem] overflow-auto rounded-lg border border-gray-200 bg-gray-50 p-3 text-[11px] sm:max-h-[30rem]"
    >
      <template v-if="contentToDisplay.type === 'sse'">
        <SseEventViewer
          v-for="(ev, idx) in contentToDisplay.content"
          :key="idx"
          :event="ev"
        />
      </template>
      <pre
        v-else
        class="whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-gray-700"
      >{{ contentToDisplay.content }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from "vue-i18n";
import { Check, Copy } from "lucide-vue-next";
import { parseSse } from '@/utils/sse';
import SseEventViewer from './SseEventViewer.vue';
import { Button } from "@/components/ui/button";
import { copyText } from "@/lib/clipboard";
import { toastController } from "@/lib/toastController";

const props = defineProps<{
  content: string | null;
  title: string;
  status?: string | null;
}>();
const { t: $t } = useI18n();
const isCopied = ref(false);
let copiedResetTimer: ReturnType<typeof setTimeout> | null = null;

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

const handleCopy = async () => {
  const copied = await copyText(props.content ?? "");
  if (!copied) {
    toastController.error($t("recordPage.copy.failed"));
    return;
  }

  isCopied.value = true;
  if (copiedResetTimer) {
    clearTimeout(copiedResetTimer);
  }
  copiedResetTimer = setTimeout(() => {
    isCopied.value = false;
  }, 2000);
};
</script>
