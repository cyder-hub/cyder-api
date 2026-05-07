<script setup lang="ts">
import { Checkbox } from "@/components/ui/checkbox";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type {
  CandidateMoveDelta,
  EditingCandidate,
  EditingRoute,
  ModelRouteOption,
} from "../types";
import ModelRouteQueueEditor from "./ModelRouteQueueEditor.vue";

const open = defineModel<boolean>("open", { required: true });
const route = defineModel<EditingRoute>("route", { required: true });

defineProps<{
  saving?: boolean;
  providerOptions: ModelRouteOption[];
  getModelOptions: (providerId: string | null) => ModelRouteOption[];
  getCandidateSummary: (candidate: EditingCandidate) => string;
}>();

const emit = defineEmits<{
  save: [];
  close: [];
  addCandidate: [];
  removeCandidate: [index: number];
  moveCandidate: [index: number, delta: CandidateMoveDelta];
  candidateProviderChange: [index: number, value: unknown];
  candidateModelChange: [index: number, value: unknown];
  candidateEnabledChange: [index: number, isEnabled: boolean];
}>();
</script>

<template>
  <Dialog :open="open" @update:open="(value) => (open = value)">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-4xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{ route.id ? $t("modelRoutePage.modal.titleEdit") : $t("modelRoutePage.modal.titleAdd") }}
        </DialogTitle>
      </DialogHeader>

      <div class="flex-1 space-y-5 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
        <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ $t("modelRoutePage.modal.labelRouteName") }}
              <span class="ml-0.5 text-red-500">*</span>
            </Label>
            <Input
              v-model="route.route_name"
              :placeholder="$t('modelRoutePage.modal.placeholderRouteName')"
              class="font-mono text-sm"
            />
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ $t("modelRoutePage.modal.labelDescription") }}
            </Label>
            <Input
              v-model="route.description"
              :placeholder="$t('modelRoutePage.modal.placeholderDescription')"
            />
          </div>
        </div>

        <div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
          <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
            <Label class="cursor-pointer text-gray-700">
              {{ $t("modelRoutePage.modal.labelExposeInModels") }}
            </Label>
            <Checkbox v-model="route.expose_in_models" />
          </div>

          <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
            <Label class="cursor-pointer text-gray-700">
              {{ $t("modelRoutePage.modal.labelEnabled") }}
            </Label>
            <Checkbox v-model="route.is_enabled" />
          </div>
        </div>

        <ModelRouteQueueEditor
          :route="route"
          :provider-options="providerOptions"
          :get-model-options="getModelOptions"
          :get-candidate-summary="getCandidateSummary"
          @add-candidate="emit('addCandidate')"
          @remove-candidate="(index) => emit('removeCandidate', index)"
          @move-candidate="(index, delta) => emit('moveCandidate', index, delta)"
          @candidate-provider-change="(index, value) => emit('candidateProviderChange', index, value)"
          @candidate-model-change="(index, value) => emit('candidateModelChange', index, value)"
          @candidate-enabled-change="(index, isEnabled) => emit('candidateEnabledChange', index, isEnabled)"
        />
      </div>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button
          variant="ghost"
          class="w-full text-gray-600 sm:w-auto"
          :disabled="saving"
          @click="emit('close')"
        >
          {{ $t("common.cancel") }}
        </Button>
        <Button
          variant="default"
          class="w-full sm:w-auto"
          :disabled="saving"
          @click="emit('save')"
        >
          {{ saving ? $t("common.saving") : $t("common.save") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
