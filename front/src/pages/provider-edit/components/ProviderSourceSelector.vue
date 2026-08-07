<template>
  <section class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div>
        <h2 class="text-sm font-semibold text-gray-900">
          {{ $t("providerEditPage.sourceSelection.title") }}
        </h2>
        <p class="mt-1 text-xs leading-5 text-gray-500">
          {{ $t("providerEditPage.sourceSelection.description") }}
        </p>
      </div>
      <Badge v-if="selectedSource" variant="outline" class="w-fit font-mono text-[11px]">
        {{ selectedSource.profile_type }} · #{{ selectedSource.id }}
      </Badge>
    </div>

    <div v-if="!props.sources.length" class="mt-4 rounded-lg border border-dashed border-amber-200 bg-amber-50/60 px-3.5 py-3 text-xs leading-5 text-amber-800">
      {{ $t("providerEditPage.sourceSelection.empty") }}
    </div>

    <div v-else class="mt-4 space-y-1.5">
      <Label class="text-gray-700">
        {{ $t("providerEditPage.sourceSelection.label") }}
      </Label>
      <Select :model-value="selectedValue" @update:model-value="handleUpdate">
        <SelectTrigger class="w-full">
          <SelectValue :placeholder="$t('providerEditPage.sourceSelection.placeholder')" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem
            v-for="source in props.sources"
            :key="source.id"
            :value="String(source.id)"
          >
            <span class="font-mono text-xs">
              {{ source.profile_type }} · #{{ source.id }}
            </span>
          </SelectItem>
        </SelectContent>
      </Select>
      <p class="text-xs leading-5 text-gray-500">
        {{ $t("providerEditPage.sourceSelection.warning") }}
      </p>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { EditingProviderSource } from "../types";

const props = defineProps<{
  sources: EditingProviderSource[];
  modelValue: number | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: number | null];
}>();

const { t: $t } = useI18n();

const selectedValue = computed(() =>
  props.modelValue == null ? undefined : String(props.modelValue),
);

const selectedSource = computed(() =>
  props.sources.find((source) => source.id === props.modelValue) ?? null,
);

const handleUpdate = (value: unknown) => {
  const sourceId = typeof value !== "string" || value === "" ? null : Number(value);
  emit("update:modelValue", Number.isInteger(sourceId) ? sourceId : null);
};
</script>
