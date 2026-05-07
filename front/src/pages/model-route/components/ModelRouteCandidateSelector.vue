<script setup lang="ts">
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { EditingCandidate, ModelRouteOption } from "../types";

defineProps<{
  candidate: EditingCandidate;
  index: number;
  providerOptions: ModelRouteOption[];
  getModelOptions: (providerId: string | null) => ModelRouteOption[];
}>();

const emit = defineEmits<{
  providerChange: [index: number, value: unknown];
  modelChange: [index: number, value: unknown];
  enabledChange: [index: number, isEnabled: boolean];
}>();
</script>

<template>
  <div class="grid grid-cols-1 gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
    <div class="space-y-1.5">
      <Label class="text-gray-700">
        {{ $t("modelRoutePage.modal.candidateProvider") }}
      </Label>
      <Select
        :model-value="candidate.provider_id"
        @update:model-value="(value) => emit('providerChange', index, value)"
      >
        <SelectTrigger class="w-full">
          <SelectValue :placeholder="$t('modelRoutePage.modal.placeholderProvider')" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem
            v-for="provider in providerOptions"
            :key="provider.value"
            :value="provider.value"
          >
            {{ provider.label }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <div class="space-y-1.5">
      <Label class="text-gray-700">
        {{ $t("modelRoutePage.modal.candidateModel") }}
      </Label>
      <Select
        :model-value="candidate.model_id"
        :disabled="!candidate.provider_id"
        @update:model-value="(value) => emit('modelChange', index, value)"
      >
        <SelectTrigger class="w-full">
          <SelectValue :placeholder="$t('modelRoutePage.modal.placeholderModel')" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem
            v-for="model in getModelOptions(candidate.provider_id)"
            :key="model.value"
            :value="model.value"
          >
            {{ model.label }}
            <span v-if="model.is_enabled === false" class="text-gray-400">
              ({{ $t("modelRoutePage.queue.disabledModel") }})
            </span>
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <div class="flex items-end">
      <div class="flex w-full items-center justify-between rounded-lg border border-gray-200 bg-white px-3 py-2.5 lg:min-w-[140px]">
        <Label class="cursor-pointer text-gray-700">
          {{ $t("modelRoutePage.modal.candidateEnabled") }}
        </Label>
        <Checkbox
          :model-value="candidate.is_enabled"
          @update:model-value="(value) => emit('enabledChange', index, value === true)"
        />
      </div>
    </div>
  </div>
</template>
