<template>
  <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-2">
    <div class="text-xs font-medium uppercase tracking-wide text-gray-500">
      {{ title }}
    </div>
    <div v-if="normalizedItems.length === 0" class="mt-2 text-sm text-gray-500">
      /
    </div>
    <dl v-else class="mt-2 max-h-56 space-y-1 overflow-auto text-xs">
      <div
        v-for="(item, index) in normalizedItems"
        :key="`${item.name}-${index}`"
        class="grid grid-cols-[minmax(0,0.45fr)_minmax(0,1fr)] gap-2"
      >
        <dt class="truncate font-mono text-gray-500">{{ item.name }}</dt>
        <dd class="break-all font-mono text-gray-800">{{ formatValue(item) }}</dd>
      </div>
    </dl>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type {
  RecordNameValue,
  RecordReplayNameValue,
  RecordReplayQueryParam,
} from "@/store/types";

type DisplayNameValue = RecordNameValue | RecordReplayNameValue | RecordReplayQueryParam;

const props = withDefaults(defineProps<{
  title: string;
  items?: DisplayNameValue[] | null;
}>(), {
  items: () => [],
});

const normalizedItems = computed(() => props.items ?? []);

const formatValue = (item: DisplayNameValue) => {
  if ("value_present" in item && item.value_present === false && item.value == null) {
    return "(flag)";
  }
  if (item.value === "") {
    return "(empty)";
  }
  return item.value ?? "";
};
</script>
