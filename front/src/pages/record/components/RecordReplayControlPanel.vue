<template>
  <section class="space-y-4">
    <div
      class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between"
    >
      <div>
        <h3 class="text-base font-semibold text-gray-900">
          {{ $t("recordPage.detailDialog.replay.controlTitle") }}
        </h3>
        <p class="mt-1 text-xs leading-5 text-gray-500">
          {{ $t("recordPage.detailDialog.replay.controlDescription") }}
        </p>
      </div>
      <Badge
        :variant="selectedCapability.available ? 'default' : 'secondary'"
        class="w-fit"
      >
        {{
          selectedCapability.available
            ? $t("recordPage.detailDialog.replay.available")
            : $t("recordPage.detailDialog.replay.unavailable")
        }}
      </Badge>
    </div>

    <RecordArtifactStatePanel
      v-if="artifactViewState === 'unavailable'"
      :title="$t('recordPage.detailDialog.replay.unavailableSummary')"
      :details="selectedUnavailableReasons"
      tone="warning"
    />

    <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-4">
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("recordPage.detailDialog.replay.labels.replayKind") }}
        </span>
        <Select
          :model-value="selectedKind"
          @update:model-value="$emit('update:selectedKind', $event as RecordReplayKind)"
        >
          <SelectTrigger class="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem value="attempt_upstream">
              {{ $t("recordPage.detailDialog.replay.kinds.attemptUpstream") }}
            </SelectItem>
            <SelectItem value="gateway_request">
              {{ $t("recordPage.detailDialog.replay.kinds.gatewayRequest") }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div v-if="selectedKind === 'attempt_upstream'" class="flex flex-col gap-1.5">
        <span class="text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("recordPage.detailDialog.replay.labels.attempt") }}
        </span>
        <Select
          :model-value="selectedAttemptValue"
          @update:model-value="$emit('update:selectedAttemptValue', String($event))"
        >
          <SelectTrigger class="w-full">
            <SelectValue :placeholder="$t('recordPage.detailDialog.replay.selectAttempt')" />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem
              v-for="attempt in replayableAttempts"
              :key="attempt.id"
              :value="String(attempt.id)"
            >
              #{{ attempt.attempt_index }} / {{ attempt.attempt_status }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div v-if="selectedKind === 'attempt_upstream'" class="flex flex-col gap-1.5">
        <span class="text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("recordPage.detailDialog.replay.labels.providerKeyOverride") }}
        </span>
        <Input
          :model-value="providerApiKeyOverride"
          inputmode="numeric"
          :placeholder="$t('recordPage.detailDialog.replay.providerKeyPlaceholder')"
          @update:model-value="$emit('update:providerApiKeyOverride', String($event))"
        />
      </div>

      <label class="flex items-center justify-between gap-3 rounded-lg border border-gray-200 p-3.5">
        <span>
          <span class="block text-sm font-medium text-gray-900">
            {{ $t("recordPage.detailDialog.replay.confirmLiveRequest") }}
          </span>
          <span class="block text-xs text-gray-500">
            {{ $t("recordPage.detailDialog.replay.confirmLiveRequestDescription") }}
          </span>
        </span>
        <Checkbox
          :model-value="confirmLiveRequest"
          @update:model-value="$emit('update:confirmLiveRequest', Boolean($event))"
        />
      </label>
    </div>

    <div
      v-if="overrideInvalid"
      class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700"
    >
      {{ $t("recordPage.detailDialog.replay.overrideInvalid") }}
    </div>

    <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
      <Button
        variant="outline"
        class="w-full sm:w-auto"
        :disabled="!canPreview || previewLoading || executeLoading"
        @click="$emit('preview')"
      >
        {{
          previewLoading
            ? $t("recordPage.detailDialog.replay.actions.previewing")
            : $t("recordPage.detailDialog.replay.actions.preview")
        }}
      </Button>
      <Button
        variant="outline"
        class="w-full sm:w-auto"
        :disabled="!canSaveDryRun || executeLoading"
        @click="$emit('execute', 'dry_run')"
      >
        {{
          executeMode === "dry_run"
            ? $t("recordPage.detailDialog.replay.actions.saving")
            : $t("recordPage.detailDialog.replay.actions.saveDryRun")
        }}
      </Button>
      <Button
        class="w-full sm:w-auto"
        :disabled="!canExecute || executeLoading"
        @click="$emit('execute', 'live')"
      >
        {{
          executeMode === "live"
            ? $t("recordPage.detailDialog.replay.actions.executing")
            : $t("recordPage.detailDialog.replay.actions.executeLive")
        }}
      </Button>
    </div>

    <div
      v-if="actionError"
      class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-red-700"
    >
      {{ actionError }}
    </div>
  </section>
</template>

<script setup lang="ts">
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  RecordAttempt,
  RecordReplayKind,
  RecordReplayKindCapability,
  RecordReplayMode,
} from "@/services/types";
import type { ReplayArtifactViewState } from "../composables/useRecordReplay";
import RecordArtifactStatePanel from "./RecordArtifactStatePanel.vue";

defineProps<{
  artifactViewState: ReplayArtifactViewState;
  selectedCapability: RecordReplayKindCapability;
  selectedUnavailableReasons: string[];
  selectedKind: RecordReplayKind;
  selectedAttemptValue: string;
  replayableAttempts: RecordAttempt[];
  providerApiKeyOverride: string;
  confirmLiveRequest: boolean;
  overrideInvalid: boolean;
  canPreview: boolean;
  canSaveDryRun: boolean;
  canExecute: boolean;
  previewLoading: boolean;
  executeLoading: boolean;
  executeMode: RecordReplayMode | null;
  actionError: string | null;
}>();

defineEmits<{
  "update:selectedKind": [value: RecordReplayKind];
  "update:selectedAttemptValue": [value: string];
  "update:providerApiKeyOverride": [value: string];
  "update:confirmLiveRequest": [value: boolean];
  preview: [];
  execute: [mode: RecordReplayMode];
}>();
</script>
