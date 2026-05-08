<script setup lang="ts">
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type {
  CandidateMoveDelta,
  EditingCandidate,
  EditingRoute,
  ModelRouteOption,
} from "../types";
import ModelRouteCandidateSelector from "./ModelRouteCandidateSelector.vue";

defineProps<{
  route: EditingRoute;
  providerOptions: ModelRouteOption[];
  getModelOptions: (providerId: string | null) => ModelRouteOption[];
  getCandidateSummary: (candidate: EditingCandidate) => string;
}>();

const emit = defineEmits<{
  addCandidate: [];
  removeCandidate: [index: number];
  moveCandidate: [index: number, delta: CandidateMoveDelta];
  candidateProviderChange: [index: number, value: unknown];
  candidateModelChange: [index: number, value: unknown];
  candidateEnabledChange: [index: number, isEnabled: boolean];
}>();
</script>

<template>
  <section class="space-y-3">
    <div class="flex flex-col gap-3 border-b border-gray-100 pb-3 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <h3 class="text-base font-semibold text-gray-900">
          {{ $t("modelRoutePage.modal.candidatesTitle") }}
        </h3>
        <p class="mt-1 text-sm text-gray-500">
          {{ $t("modelRoutePage.queue.description") }}
        </p>
      </div>
      <Button variant="outline" class="w-full sm:w-auto" @click="emit('addCandidate')">
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("modelRoutePage.modal.addCandidate") }}
      </Button>
    </div>

    <div
      v-if="route.candidates.length === 0"
      class="rounded-lg border border-dashed border-gray-300 px-4 py-8 text-center text-sm text-gray-500"
    >
      {{ $t("modelRoutePage.modal.emptyCandidates") }}
    </div>

    <div v-else class="space-y-3">
      <div
        v-for="(candidate, index) in route.candidates"
        :key="candidate.local_id"
        class="rounded-lg border border-gray-200 bg-gray-50/40 p-3 sm:p-4"
      >
        <div class="flex flex-col gap-3">
          <div class="flex items-center justify-between gap-3">
            <div class="flex flex-wrap items-center gap-2">
              <Badge variant="outline" class="font-mono text-xs">
                {{ $t("modelRoutePage.modal.candidateOrder") }} #{{ index + 1 }}
              </Badge>
              <Badge variant="outline" class="font-mono text-xs">
                {{ $t("modelRoutePage.queue.priority") }} {{ index * 10 }}
              </Badge>
              <Badge :variant="candidate.is_enabled ? 'secondary' : 'outline'" class="font-mono text-xs">
                {{ candidate.is_enabled ? $t("common.yes") : $t("common.no") }}
              </Badge>
            </div>
            <div class="flex items-center gap-1">
              <Button
                variant="ghost"
                size="sm"
                :disabled="index === 0"
                :aria-label="$t('modelRoutePage.modal.candidateMoveUp')"
                @click="emit('moveCandidate', index, -1)"
              >
                <ArrowUp class="h-3.5 w-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                :disabled="index === route.candidates.length - 1"
                :aria-label="$t('modelRoutePage.modal.candidateMoveDown')"
                @click="emit('moveCandidate', index, 1)"
              >
                <ArrowDown class="h-3.5 w-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-400 hover:text-red-600"
                :aria-label="$t('modelRoutePage.modal.candidateRemove')"
                @click="emit('removeCandidate', index)"
              >
                <Trash2 class="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          <ModelRouteCandidateSelector
            :candidate="candidate"
            :index="index"
            :provider-options="providerOptions"
            :get-model-options="getModelOptions"
            @provider-change="(candidateIndex, value) => emit('candidateProviderChange', candidateIndex, value)"
            @model-change="(candidateIndex, value) => emit('candidateModelChange', candidateIndex, value)"
            @enabled-change="(candidateIndex, isEnabled) => emit('candidateEnabledChange', candidateIndex, isEnabled)"
          />

          <div class="rounded-lg border border-gray-200 bg-white px-3 py-2.5 text-xs text-gray-500">
            {{ getCandidateSummary(candidate) }}
          </div>
        </div>
      </div>
    </div>
  </section>
</template>
