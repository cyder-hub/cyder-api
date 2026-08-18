<script setup lang="ts">
import { useI18n } from "vue-i18n";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Label } from "@/components/ui/label";
import type { CheckOption, CheckDialogKind } from "../composables/providerCheckViewModel";

const props = defineProps<{
  open: boolean;
  kind: CheckDialogKind | null;
  targetLabel: string;
  sourceOptions: CheckOption[];
  modelOptions: CheckOption[];
  apiKeyOptions: CheckOption[];
  sourceValue: string | null;
  modelValue: string | null;
  apiKeyValue: string | null;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  confirm: [];
  "update:sourceValue": [value: string | null];
  "update:modelValue": [value: string | null];
  "update:apiKeyValue": [value: string | null];
}>();

const { t } = useI18n();

const updateValue = (field: "source" | "model" | "apiKey", value: unknown) => {
  const next = typeof value === "string" && value.length > 0 ? value : null;
  if (field === "source") emit("update:sourceValue", next);
  if (field === "model") emit("update:modelValue", next);
  if (field === "apiKey") emit("update:apiKeyValue", next);
};
</script>

<template>
  <Dialog :open="open" @update:open="(value) => emit('update:open', value)">
    <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-md">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 text-left sm:px-6">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{ t("providerEditPage.checkDialog.title") }}
        </DialogTitle>
        <DialogDescription class="text-sm leading-5 text-gray-500">
          {{ t("providerEditPage.checkDialog.description") }}
        </DialogDescription>
      </DialogHeader>

      <div class="min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6">
        <p class="rounded-lg border border-gray-200 bg-gray-50 px-3.5 py-3 font-mono text-xs text-gray-700">
          {{ targetLabel }}
        </p>

        <div v-if="kind !== 'source'" class="space-y-1.5">
          <Label class="text-gray-700">{{ t("providerEditPage.checkDialog.sourceLabel") }}</Label>
          <Select
            :model-value="sourceValue ?? undefined"
            @update:model-value="(value) => updateValue('source', value)"
          >
            <SelectTrigger class="w-full">
              <SelectValue :placeholder="t('providerEditPage.checkDialog.sourcePlaceholder')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="option in sourceOptions" :key="option.value" :value="String(option.value)">
                {{ option.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div v-if="kind !== 'model'" class="space-y-1.5">
          <Label class="text-gray-700">{{ t("providerEditPage.checkDialog.modelLabel") }}</Label>
          <Select
            :model-value="modelValue ?? undefined"
            @update:model-value="(value) => updateValue('model', value)"
          >
            <SelectTrigger class="w-full">
              <SelectValue :placeholder="t('providerEditPage.checkDialog.modelPlaceholder')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="option in modelOptions" :key="option.value" :value="String(option.value)">
                {{ option.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div v-if="kind !== 'apiKey'" class="space-y-1.5">
          <Label class="text-gray-700">{{ t("providerEditPage.checkDialog.apiKeyLabel") }}</Label>
          <Select
            :model-value="apiKeyValue ?? undefined"
            @update:model-value="(value) => updateValue('apiKey', value)"
          >
            <SelectTrigger class="w-full">
              <SelectValue :placeholder="t('providerEditPage.checkDialog.apiKeyPlaceholder')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="option in apiKeyOptions" :key="option.value" :value="String(option.value)">
                {{ option.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button variant="ghost" class="w-full text-gray-600 sm:w-auto" @click="emit('update:open', false)">
          {{ t("common.cancel") }}
        </Button>
        <Button
          variant="default"
          class="w-full sm:w-auto"
          :disabled="
            (!sourceValue && kind !== 'source') ||
            (!modelValue && kind !== 'model') ||
            (!apiKeyValue && kind !== 'apiKey')
          "
          @click="emit('confirm')"
        >
          {{ t("common.check") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
