<template>
  <div class="space-y-5 text-sm">
    <RecordArtifactStatePanel
      v-if="artifactViewState === 'loading'"
      :title="$t('recordPage.detailDialog.replay.loading')"
      loading
    />

    <RecordArtifactStatePanel
      v-else-if="artifactViewState === 'error'"
      :title="$t('recordPage.detailDialog.replay.failed')"
      :message="artifactLoadError"
      tone="danger"
      retryable
      @retry="$emit('reload')"
    />

    <RecordArtifactStatePanel
      v-else-if="artifactViewState === 'lazy'"
      :title="$t('recordPage.detailDialog.replay.lazyEmpty')"
    />

    <RecordArtifactStatePanel
      v-else-if="artifactViewState === 'no_bundle'"
      :title="$t('recordPage.detailDialog.replay.noBundle')"
      :message="$t('recordPage.detailDialog.replay.noBundleDescription')"
      :details="selectedUnavailableReasons"
    />

    <template v-else>
      <RecordReplayControlPanel
        v-model:selected-kind="selectedKind"
        :selected-attempt-value="selectedAttemptValue"
        :provider-api-key-override="providerApiKeyOverride"
        :confirm-live-request="confirmLiveRequest"
        :artifact-view-state="artifactViewState"
        :selected-capability="selectedCapability"
        :selected-unavailable-reasons="selectedUnavailableReasons"
        :replayable-attempts="replayableAttempts"
        :override-invalid="overrideInvalid"
        :can-preview="canPreview"
        :can-save-dry-run="canSaveDryRun"
        :can-execute="canExecute"
        :preview-loading="previewLoading"
        :execute-loading="executeLoading"
        :execute-mode="executeMode"
        :action-error="actionError"
        @update:selected-attempt-value="setSelectedAttemptValue"
        @update:provider-api-key-override="providerApiKeyOverride = $event"
        @update:confirm-live-request="confirmLiveRequest = $event"
        @preview="handlePreview"
        @execute="handleExecute"
      />

      <RecordReplayHistory
        :replay-runs="replayRuns"
        :history-loading="historyLoading"
        :history-selection-loading="historySelectionLoading"
        :history-error="historyError"
        :selected-replay-run-id="selectedReplayRunId"
        @reload="loadReplayHistory(true)"
        @open="openReplayRun"
      />

      <RecordReplayResultPanels
        :preview="preview"
        :run="run"
        :artifact="artifact"
      />
    </template>
  </div>
</template>

<script setup lang="ts">
import { toRef } from "vue";
import { useI18n } from "vue-i18n";
import type {
  RecordArtifactResponse,
  RecordAttempt,
} from "@/services/types";
import * as recordService from "@/services/records";
import { useRecordReplay } from "../composables/useRecordReplay";
import RecordArtifactStatePanel from "./RecordArtifactStatePanel.vue";
import RecordReplayControlPanel from "./RecordReplayControlPanel.vue";
import RecordReplayHistory from "./RecordReplayHistory.vue";
import RecordReplayResultPanels from "./RecordReplayResultPanels.vue";

const props = defineProps<{
  recordId: number;
  attempts: RecordAttempt[];
  artifacts: RecordArtifactResponse | null;
  loading: boolean;
  error: string | null;
  selectedAttemptId: number | null;
  selectedReplayRunId: number | null;
}>();

const emit = defineEmits<{
  reload: [];
  "update:selectedAttemptId": [value: number | null];
  "update:selectedReplayRunId": [value: number | null];
}>();

const { t: $t, te } = useI18n();

const recordId = toRef(props, "recordId");
const attempts = toRef(props, "attempts");
const artifacts = toRef(props, "artifacts");
const loading = toRef(props, "loading");
const error = toRef(props, "error");
const selectedAttemptId = toRef(props, "selectedAttemptId");
const selectedReplayRunId = toRef(props, "selectedReplayRunId");

const replay = useRecordReplay({
  recordId,
  attempts,
  artifacts,
  loading,
  error,
  selectedAttemptId,
  selectedReplayRunId,
  i18n: { t: $t, te },
  api: recordService,
  emitSelectedAttemptId: (value) => emit("update:selectedAttemptId", value),
  emitSelectedReplayRunId: (value) => emit("update:selectedReplayRunId", value),
});

const {
  selectedKind,
  selectedAttemptValue,
  providerApiKeyOverride,
  confirmLiveRequest,
  previewLoading,
  executeLoading,
  executeMode,
  actionError,
  preview,
  replayRuns,
  historyLoading,
  historySelectionLoading,
  historyError,
  run,
  artifact,
  selectedCapability,
  selectedUnavailableReasons,
  artifactLoadError,
  artifactViewState,
  replayableAttempts,
  overrideInvalid,
  canPreview,
  canExecute,
  canSaveDryRun,
  setSelectedAttemptValue,
  loadReplayHistory,
  handlePreview,
  handleExecute,
  openReplayRun,
} = replay;

</script>
