<template>
  <div>
    <div class="flex items-center justify-between mb-1">
      <h4 class="text-sm font-medium text-gray-700 py-1">{{ title }}</h4>
      <div class="flex items-center gap-2">
        <slot name="action"></slot>
        <Button
          v-if="displayContent.type !== 'empty'"
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
    </div>
    <div v-if="displayContent.type !== 'empty'" class="mt-1 overflow-auto rounded-md border bg-gray-50 p-2 text-[10px] max-h-[30rem]">
      <pre class="whitespace-pre font-mono">{{ displayContent.content }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from "vue-i18n";
import { Check, Copy } from "lucide-vue-next";
import { Button } from "@/components/ui/button";
import { copyText } from "@/lib/clipboard";
import { toastController } from "@/lib/toastController";

const props = defineProps<{
  content: string | null;
  title: string;
}>();
const { t: $t } = useI18n();
const isCopied = ref(false);
let copiedResetTimer: ReturnType<typeof setTimeout> | null = null;

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

const copyableContent = computed(() =>
  displayContent.value.type === "empty" ? "" : displayContent.value.content ?? "",
);

const handleCopy = async () => {
  if (!copyableContent.value) {
    return;
  }

  const copied = await copyText(copyableContent.value);
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
