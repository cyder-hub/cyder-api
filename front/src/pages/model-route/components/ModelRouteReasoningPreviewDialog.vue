<script setup lang="ts">
import { Loader2 } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type {
  ReasoningRouteCandidatePreview,
  ReasoningRoutePreview,
} from "@/services/types";

const open = defineModel<boolean>("open", { required: true });

defineProps<{
  preview: ReasoningRoutePreview | null;
  loading: boolean;
  error: string | null;
  formatConfigSource: (candidate: ReasoningRouteCandidatePreview) => string;
}>();
</script>

<template>
  <Dialog :open="open" @update:open="(value) => (open = value)">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-5xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{ $t("modelRoutePage.reasoning.title") }}
          <span v-if="preview" class="font-mono text-sm font-normal text-gray-500">
            {{ preview.route_name }}
          </span>
        </DialogTitle>
      </DialogHeader>

      <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
        <div v-if="loading" class="flex items-center justify-center py-16 text-gray-400">
          <Loader2 class="mr-2 h-5 w-5 animate-spin" />
          <span class="text-sm">{{ $t("modelRoutePage.reasoning.loading") }}</span>
        </div>

        <div
          v-else-if="error"
          class="rounded-lg border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600"
        >
          {{ error }}
        </div>

        <div v-else-if="preview" class="space-y-4">
          <div class="space-y-3">
            <div
              v-for="preset in preview.presets"
              :key="preset.preset_key"
              class="rounded-lg border border-gray-200 bg-white"
            >
              <div class="flex flex-col gap-2 border-b border-gray-100 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
                <div class="min-w-0">
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="font-mono text-sm font-semibold text-gray-900">
                      {{ preset.preset_key }} -{{ preset.suffix }}
                    </span>
                    <Badge :variant="preset.stable ? 'secondary' : 'outline'" class="font-mono text-[11px]">
                      {{ preset.stable ? $t("modelRoutePage.reasoning.stable") : $t("modelRoutePage.reasoning.incomplete") }}
                    </Badge>
                    <Badge :variant="preset.requires_reasoning ? 'secondary' : 'outline'" class="font-mono text-[11px]">
                      reasoning: {{ preset.requires_reasoning ? $t("common.yes") : $t("common.no") }}
                    </Badge>
                  </div>
                  <p v-if="preset.reason" class="mt-1 text-xs text-gray-500">
                    {{ preset.reason }}
                  </p>
                </div>
              </div>

              <div class="divide-y divide-gray-100">
                <div
                  v-for="candidate in preset.candidates"
                  :key="`${preset.preset_key}-${candidate.candidate_position}`"
                  class="grid grid-cols-1 gap-2 px-4 py-3 text-sm md:grid-cols-[auto_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)] md:items-center"
                >
                  <div class="flex flex-wrap items-center gap-1">
                    <Badge :variant="candidate.supported ? 'secondary' : 'outline'" class="w-fit font-mono text-[11px]">
                      #{{ candidate.candidate_position }}
                    </Badge>
                    <Badge
                      :variant="candidate.runtime_status === 'stale_skipped' ? 'outline' : candidate.supported ? 'secondary' : 'outline'"
                      class="w-fit font-mono text-[11px]"
                    >
                      {{
                        candidate.runtime_status === "stale_skipped"
                          ? $t("modelRoutePage.reasoning.staleSkipped")
                          : candidate.supported
                            ? $t("modelRoutePage.reasoning.supported")
                            : $t("modelRoutePage.reasoning.unsupported")
                      }}
                    </Badge>
                  </div>
                  <div class="min-w-0">
                    <p class="break-all font-mono text-xs text-gray-700">
                      {{ candidate.provider_key || "missing-provider" }}/{{ candidate.model_name || candidate.model_id }}
                    </p>
                  </div>
                  <div class="min-w-0">
                    <p class="break-all font-mono text-xs text-gray-600">
                      {{ formatConfigSource(candidate) }}
                      <span v-if="candidate.config_id != null">
                        / {{ candidate.config_scope || "scope" }}:{{ candidate.config_id }}
                      </span>
                    </p>
                    <p
                      v-if="candidate.family || candidate.config_preset_id != null"
                      class="mt-0.5 break-all font-mono text-[11px] text-gray-500"
                    >
                      {{ candidate.family || "missing family" }}
                      <span v-if="candidate.config_preset_id != null">
                        / preset row {{ candidate.config_preset_id }}
                      </span>
                    </p>
                  </div>
                  <div class="min-w-0">
                    <p
                      class="text-xs"
                      :class="candidate.runtime_status === 'stale_skipped' ? 'text-amber-700' : candidate.supported ? 'text-gray-600' : 'text-red-600'"
                    >
                      {{
                        candidate.runtime_status === "stale_skipped"
                          ? candidate.reason
                          : candidate.supported
                            ? $t("modelRoutePage.reasoning.supported")
                            : candidate.reason
                      }}
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button
          variant="ghost"
          class="w-full text-gray-600 sm:w-auto"
          @click="open = false"
        >
          {{ $t("common.close") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
