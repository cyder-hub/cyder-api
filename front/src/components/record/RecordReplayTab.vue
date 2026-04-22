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
      <RecordArtifactStatePanel
        v-if="artifactViewState === 'unavailable'"
        :title="$t('recordPage.detailDialog.replay.unavailableSummary')"
        :details="selectedUnavailableReasons"
        tone="warning"
      />

      <section class="space-y-4">
        <div
          class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between"
        >
          <div>
            <h3 class="text-base font-semibold text-gray-900">
              {{ $t("recordPage.detailDialog.replay.controlTitle") }}
            </h3>
            <p class="mt-1 text-sm text-gray-500">
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

        <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-4">
          <div class="flex flex-col gap-1.5">
            <span
              class="text-xs font-medium uppercase tracking-wide text-gray-500"
            >
              {{ $t("recordPage.detailDialog.replay.labels.replayKind") }}
            </span>
            <Select v-model="selectedKind">
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent :body-lock="false">
                <SelectItem value="attempt_upstream">
                  {{
                    $t("recordPage.detailDialog.replay.kinds.attemptUpstream")
                  }}
                </SelectItem>
                <SelectItem value="gateway_request">
                  {{
                    $t("recordPage.detailDialog.replay.kinds.gatewayRequest")
                  }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div
            v-if="selectedKind === 'attempt_upstream'"
            class="flex flex-col gap-1.5"
          >
            <span
              class="text-xs font-medium uppercase tracking-wide text-gray-500"
            >
              {{ $t("recordPage.detailDialog.replay.labels.attempt") }}
            </span>
            <Select v-model="selectedAttemptValue">
              <SelectTrigger class="w-full">
                <SelectValue
                  :placeholder="
                    $t('recordPage.detailDialog.replay.selectAttempt')
                  "
                />
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

          <div
            v-if="selectedKind === 'attempt_upstream'"
            class="flex flex-col gap-1.5"
          >
            <span
              class="text-xs font-medium uppercase tracking-wide text-gray-500"
            >
              {{
                $t("recordPage.detailDialog.replay.labels.providerKeyOverride")
              }}
            </span>
            <Input
              v-model="providerApiKeyOverride"
              inputmode="numeric"
              :placeholder="
                $t('recordPage.detailDialog.replay.providerKeyPlaceholder')
              "
            />
          </div>

          <label
            class="flex items-center justify-between gap-3 rounded-lg border border-gray-200 p-3.5"
          >
            <span>
              <span class="block text-sm font-medium text-gray-900">
                {{ $t("recordPage.detailDialog.replay.confirmLiveRequest") }}
              </span>
              <span class="block text-xs text-gray-500">
                {{
                  $t(
                    "recordPage.detailDialog.replay.confirmLiveRequestDescription",
                  )
                }}
              </span>
            </span>
            <Checkbox v-model="confirmLiveRequest" />
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
            @click="handlePreview"
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
            @click="handleExecute('dry_run')"
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
            @click="handleExecute('live')"
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

      <section class="space-y-4">
        <div
          class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between"
        >
          <div>
            <h3 class="text-base font-semibold text-gray-900">
              {{ $t("recordPage.detailDialog.replay.historyTitle") }}
            </h3>
            <p class="mt-1 text-sm text-gray-500">
              {{ $t("recordPage.detailDialog.replay.historyDescription") }}
            </p>
          </div>
          <Button
            variant="outline"
            class="w-full sm:w-auto"
            :disabled="historyLoading || historySelectionLoading"
            @click="loadReplayHistory(true)"
          >
            {{
              historyLoading
                ? $t("recordPage.detailDialog.replay.actions.loadingHistory")
                : $t("recordPage.detailDialog.replay.actions.reloadHistory")
            }}
          </Button>
        </div>

        <div
          v-if="historyLoading && replayRuns.length === 0"
          class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
        >
          {{ $t("recordPage.detailDialog.replay.historyLoading") }}
        </div>

        <div
          v-else-if="historyError"
          class="space-y-3 rounded-lg border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-700"
        >
          <div>{{ historyError }}</div>
          <Button
            variant="outline"
            class="w-full border-red-200 bg-white text-red-700 hover:bg-red-50 sm:w-auto"
            @click="loadReplayHistory(true)"
          >
            {{ $t("recordPage.detailDialog.replay.actions.retryHistory") }}
          </Button>
        </div>

        <div
          v-else-if="replayRuns.length === 0"
          class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
        >
          {{ $t("recordPage.detailDialog.replay.historyEmpty") }}
        </div>

        <div v-else class="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <button
            v-for="replayRun in replayRuns"
            :key="replayRun.id"
            type="button"
            class="w-full border-b border-gray-100 px-4 py-3 text-left transition-colors last:border-b-0"
            :class="
              selectedReplayRunId === replayRun.id
                ? 'bg-gray-50'
                : 'hover:bg-gray-50/70'
            "
            :disabled="historySelectionLoading"
            @click="openReplayRun(replayRun.id)"
          >
            <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
              <div class="min-w-0 space-y-2">
                <div class="flex flex-wrap items-center gap-2">
                  <Badge variant="outline" class="font-mono text-xs">
                    #{{ replayRun.id }}
                  </Badge>
                  <Badge
                    :variant="getStatusBadgeVariant(replayRun.status)"
                    class="font-mono text-xs"
                  >
                    {{ replayRun.status }}
                  </Badge>
                  <Badge variant="secondary" class="font-mono text-xs">
                    {{ replayRun.replay_kind }}
                  </Badge>
                  <Badge variant="secondary" class="font-mono text-xs">
                    {{ replayRun.replay_mode }}
                  </Badge>
                </div>
                <div class="grid grid-cols-1 gap-1 text-xs text-gray-500 sm:grid-cols-2 xl:grid-cols-4">
                  <div>
                    {{ $t("recordPage.detailDialog.replay.labels.created") }}:
                    <span class="font-mono text-gray-700">{{ formatDate(replayRun.created_at) }}</span>
                  </div>
                  <div>
                    {{ $t("recordPage.detailDialog.replay.labels.route") }}:
                    <span class="font-mono text-gray-700">{{ replayRun.executed_route_name || emptyValue }}</span>
                  </div>
                  <div>
                    {{ $t("recordPage.detailDialog.replay.labels.http") }}:
                    <span class="font-mono text-gray-700">{{ replayRun.http_status ?? emptyValue }}</span>
                  </div>
                  <div>
                    {{ $t("recordPage.detailDialog.replay.labels.errorCode") }}:
                    <span class="font-mono text-gray-700">{{ replayRun.error_code || emptyValue }}</span>
                  </div>
                </div>
              </div>
              <div class="text-xs text-gray-500 sm:text-right">
                {{
                  historySelectionLoading && selectedReplayRunId === replayRun.id
                    ? $t("recordPage.detailDialog.replay.actions.openingHistory")
                    : $t("recordPage.detailDialog.replay.actions.openHistory")
                }}
              </div>
            </div>
          </button>
        </div>
      </section>

      <section v-if="preview" class="space-y-4">
        <div
          class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between"
        >
          <h3 class="text-base font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.replay.previewTitle") }}
          </h3>
          <Badge variant="outline" class="w-fit font-mono text-xs">
            {{ preview.replay_kind }}
          </Badge>
        </div>
        <ReplayInputSnapshot :snapshot="preview.input_snapshot" />
        <ReplayExecutionPreview :preview="preview.execution_preview" />
        <ReplayBaseline :preview="preview" />
      </section>

      <section v-if="run" class="space-y-4">
        <div
          class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between"
        >
          <h3 class="text-base font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.replay.runTitle") }}
          </h3>
          <Badge
            :variant="getStatusBadgeVariant(run.status)"
            class="w-fit font-mono text-xs"
          >
            {{ run.status }}
          </Badge>
        </div>
        <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-4">
          <div
            v-for="item in runSummaryItems"
            :key="item.label"
            class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5"
          >
            <dt class="text-xs uppercase tracking-wide text-gray-500">
              {{ item.label }}
            </dt>
            <dd
              class="truncate text-right text-sm text-gray-900"
              :class="item.valueClass"
            >
              {{ item.value }}
            </dd>
          </div>
        </dl>
        <pre
          v-if="run.diff_summary_json"
          class="max-h-56 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-gray-950 px-3 py-3 font-mono text-xs text-gray-100"
          >{{ formatJsonText(run.diff_summary_json) }}</pre
        >
      </section>

      <section v-if="artifact" class="space-y-4">
        <div
          class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between"
        >
          <h3 class="text-base font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.replay.artifactTitle") }}
          </h3>
          <Badge variant="outline" class="w-fit font-mono text-xs">
            v{{ artifact.version }}
          </Badge>
        </div>

        <ReplayInputSnapshot
          v-if="artifact.input_snapshot"
          :snapshot="artifact.input_snapshot"
          :title="$t('recordPage.detailDialog.replay.persistedInputBaseline')"
        />
        <ReplayExecutionPreview
          v-if="artifact.execution_preview"
          :preview="artifact.execution_preview"
          :title="
            $t('recordPage.detailDialog.replay.persistedExecutionPreview')
          "
        />

        <div v-if="artifact.result" class="space-y-3">
          <h4 class="text-sm font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.replay.resultSummary") }}
          </h4>
          <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-4">
            <div
              v-for="item in artifactResultItems"
              :key="item.label"
              class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5"
            >
              <dt class="text-xs uppercase tracking-wide text-gray-500">
                {{ item.label }}
              </dt>
              <dd
                class="truncate text-right text-sm text-gray-900"
                :class="item.valueClass"
              >
                {{ item.value }}
              </dd>
            </div>
          </dl>
          <NameValueBlock
            :title="$t('recordPage.detailDialog.replay.responseHeaders')"
            :items="artifact.result.response_headers"
          />
          <details
            v-if="artifact.result.response_body"
            class="rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-2"
          >
            <summary
              class="cursor-pointer text-xs font-medium uppercase tracking-wide text-gray-500"
            >
              {{ $t("recordPage.detailDialog.replay.responseBody") }}
            </summary>
            <pre
              class="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-gray-700"
              >{{
                summarizeReplayBodyLabelled(artifact.result.response_body)
              }}</pre
            >
          </details>
        </div>

        <div v-if="artifact.diff" class="space-y-3">
          <h4 class="text-sm font-semibold text-gray-900">
            {{ $t("recordPage.detailDialog.replay.diffSummary") }}
          </h4>
          <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-3">
            <div
              v-for="item in artifactDiffItems"
              :key="item.label"
              class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5"
            >
              <dt class="text-xs uppercase tracking-wide text-gray-500">
                {{ item.label }}
              </dt>
              <dd
                class="truncate text-right text-sm text-gray-900"
                :class="item.valueClass"
              >
                {{ item.value }}
              </dd>
            </div>
          </dl>
          <ul
            v-if="artifact.diff.summary_lines.length > 0"
            class="space-y-2 rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-2 text-sm text-gray-700"
          >
            <li v-for="line in artifact.diff.summary_lines" :key="line">
              {{ line }}
            </li>
          </ul>
        </div>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, defineComponent, h, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
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
import { Api } from "@/services/request";
import type {
  RecordArtifactResponse,
  RecordAttempt,
  RecordReplayArtifact,
  RecordReplayExecutionPreview,
  RecordReplayInputSnapshot,
  RecordReplayKind,
  RecordReplayMode,
  RecordReplayPreviewResponse,
  RecordReplayRun,
} from "@/store/types";
import NameValueBlock from "./NameValueBlock.vue";
import RecordArtifactStatePanel from "./RecordArtifactStatePanel.vue";
import {
  buildAttemptReplayPayload,
  canExecuteReplay,
  canSaveReplayDryRun,
  canPreviewReplay,
  getReplayCapability,
  hasReplayArtifactBundle,
  initialReplayAttemptId,
  resolveReplayArtifactViewState,
  normalizeProviderApiKeyOverrideForKind,
  replayableAttempts as getReplayableAttempts,
  shouldShowProviderApiKeyOverrideError,
} from "./recordReplayState";
import {
  emptyValue,
  formatCompactMetrics,
  formatDate,
  formatJsonText,
  formatPrice,
  getStatusBadgeVariant,
  summarizeReplayBody,
} from "./recordFormat";

const props = defineProps<{
  recordId: number;
  attempts: RecordAttempt[];
  artifacts: RecordArtifactResponse | null;
  loading: boolean;
  error: string | null;
}>();

defineEmits<{
  reload: [];
}>();

const { t: $t, te } = useI18n();

const selectedKind = ref<RecordReplayKind>("attempt_upstream");
const selectedAttemptValue = ref("");
const providerApiKeyOverride = ref("");
const confirmLiveRequest = ref(false);
const previewLoading = ref(false);
const executeLoading = ref(false);
const executeMode = ref<RecordReplayMode | null>(null);
const actionError = ref<string | null>(null);
const preview = ref<RecordReplayPreviewResponse | null>(null);
const replayRuns = ref<RecordReplayRun[]>([]);
const historyLoading = ref(false);
const historySelectionLoading = ref(false);
const historyError = ref<string | null>(null);
const selectedReplayRunId = ref<number | null>(null);
const run = ref<RecordReplayRun | null>(null);
const artifact = ref<RecordReplayArtifact | null>(null);

const selectedCapability = computed(() => {
  return getReplayCapability(props.artifacts, selectedKind.value);
});

const selectedUnavailableReasons = computed(() => {
  return selectedCapability.value.reasons.map(formatReplayUnavailableReason);
});

const artifactLoadError = computed(() => props.error ?? "");

const artifactViewState = computed(() => {
  return resolveReplayArtifactViewState({
    kind: selectedKind.value,
    artifacts: props.artifacts,
    loading: props.loading,
    error: props.error,
  });
});

const hasReadyArtifacts = computed(() => {
  return props.artifacts != null && hasReplayArtifactBundle(props.artifacts);
});

const replayableAttempts = computed(() => {
  return getReplayableAttempts(props.attempts, props.artifacts);
});

const selectedAttemptId = computed(() => {
  const value = Number(selectedAttemptValue.value);
  return Number.isInteger(value) && value > 0 ? value : null;
});

const overrideInvalid = computed(() => {
  return shouldShowProviderApiKeyOverrideError(
    selectedKind.value,
    providerApiKeyOverride.value,
  );
});

const canPreview = computed(() => {
  if (!hasReadyArtifacts.value) return false;
  return canPreviewReplay({
    kind: selectedKind.value,
    artifacts: props.artifacts,
    selectedAttemptId: selectedAttemptId.value,
    providerApiKeyOverride: providerApiKeyOverride.value,
  });
});

const canExecute = computed(() => {
  return canExecuteReplay({
    hasPreview: Boolean(preview.value),
    previewFingerprint: preview.value?.preview_fingerprint,
    canPreview: canPreview.value,
    confirmLiveRequest: confirmLiveRequest.value,
  });
});

const canSaveDryRun = computed(() => {
  return canSaveReplayDryRun({
    hasPreview: Boolean(preview.value),
    previewFingerprint: preview.value?.preview_fingerprint,
    canPreview: canPreview.value,
  });
});

const resetReplayResult = () => {
  preview.value = null;
  run.value = null;
  artifact.value = null;
  selectedReplayRunId.value = null;
  actionError.value = null;
  confirmLiveRequest.value = false;
};

watch(
  () => props.artifacts,
  (artifacts) => {
    const firstAttemptId = initialReplayAttemptId(artifacts);
    selectedAttemptValue.value =
      firstAttemptId != null ? String(firstAttemptId) : "";
  },
  { immediate: true },
);

watch(selectedKind, (kind) => {
  providerApiKeyOverride.value = normalizeProviderApiKeyOverrideForKind(
    kind,
    providerApiKeyOverride.value,
  );
  resetReplayResult();
});

watch(selectedAttemptValue, resetReplayResult);
watch(providerApiKeyOverride, resetReplayResult);

const loadReplayHistory = async (force = false) => {
  if (historyLoading.value) return;
  if (replayRuns.value.length > 0 && !force) return;
  historyLoading.value = true;
  historyError.value = null;
  try {
    replayRuns.value = await Api.getRecordReplayRuns(props.recordId);
  } catch (err) {
    historyError.value = normalizeErrorMessage(err);
  } finally {
    historyLoading.value = false;
  }
};

watch(
  () => props.recordId,
  () => {
    replayRuns.value = [];
    historyError.value = null;
    preview.value = null;
    run.value = null;
    artifact.value = null;
    selectedReplayRunId.value = null;
    actionError.value = null;
    confirmLiveRequest.value = false;
    void loadReplayHistory(true);
  },
  { immediate: true },
);

const previewPayload = () => {
  return buildAttemptReplayPayload(providerApiKeyOverride.value);
};

const handlePreview = async () => {
  if (!canPreview.value) return;
  previewLoading.value = true;
  actionError.value = null;
  run.value = null;
  artifact.value = null;

  try {
    if (selectedKind.value === "attempt_upstream") {
      const attemptId = selectedAttemptId.value;
      if (attemptId == null) return;
      preview.value = await Api.previewAttemptReplay(
        props.recordId,
        attemptId,
        previewPayload(),
      );
    } else {
      preview.value = await Api.previewGatewayReplay(props.recordId, {});
    }
  } catch (err) {
    actionError.value = normalizeErrorMessage(err);
  } finally {
    previewLoading.value = false;
  }
};

const handleExecute = async (mode: RecordReplayMode) => {
  if (mode === "live" && !canExecute.value) return;
  if (mode === "dry_run" && !canSaveDryRun.value) return;
  executeLoading.value = true;
  executeMode.value = mode;
  actionError.value = null;

  try {
    const executedRun =
      selectedKind.value === "attempt_upstream"
        ? await executeAttemptReplay(mode)
        : await Api.executeGatewayReplay(props.recordId, {
            replay_mode: mode,
            confirm_live_request: mode === "live",
            preview_fingerprint: preview.value?.preview_fingerprint ?? "",
          });
    run.value = await Api.getRecordReplayRun(props.recordId, executedRun.id);
    artifact.value = await Api.getRecordReplayArtifacts(
      props.recordId,
      executedRun.id,
    );
    selectedReplayRunId.value = executedRun.id;
    await loadReplayHistory(true);
  } catch (err) {
    actionError.value = normalizeErrorMessage(err);
  } finally {
    executeLoading.value = false;
    executeMode.value = null;
  }
};

const openReplayRun = async (replayRunId: number) => {
  if (historySelectionLoading.value) return;
  historySelectionLoading.value = true;
  selectedReplayRunId.value = replayRunId;
  actionError.value = null;
  run.value = null;
  artifact.value = null;
  try {
    const [loadedRun, loadedArtifact] = await Promise.all([
      Api.getRecordReplayRun(props.recordId, replayRunId),
      Api.getRecordReplayArtifacts(props.recordId, replayRunId),
    ]);
    run.value = loadedRun;
    artifact.value = loadedArtifact;
  } catch (err) {
    actionError.value = normalizeErrorMessage(err);
  } finally {
    historySelectionLoading.value = false;
  }
};

const executeAttemptReplay = async (mode: RecordReplayMode) => {
  const attemptId = selectedAttemptId.value;
  if (attemptId == null) {
    throw new Error($t("recordPage.detailDialog.replay.noAttemptSelected"));
  }
  return Api.executeAttemptReplay(props.recordId, attemptId, {
    ...previewPayload(),
    replay_mode: mode,
    confirm_live_request: mode === "live",
    preview_fingerprint: preview.value?.preview_fingerprint ?? "",
  });
};

const normalizeErrorMessage = (err: unknown) => {
  if (err instanceof Error) return err.message;
  if (typeof err === "object" && err !== null && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
};

const formatBooleanLabel = (value: boolean | null | undefined) => {
  if (value == null) return emptyValue;
  return value ? $t("common.yes") : $t("common.no");
};

const formatReplayUnavailableReason = (reason: string) => {
  const key = `recordPage.detailDialog.replay.reasons.${reason}`;
  return te(key) ? $t(key) : reason;
};

const summarizeReplayBodyLabelled = (
  body: Parameters<typeof summarizeReplayBody>[0],
) => summarizeReplayBody(body, $t("recordPage.detailDialog.replay.capture"));

const runSummaryItems = computed(() => {
  const value = run.value;
  if (!value) return [];
  return [
    {
      label: $t("recordPage.detailDialog.replay.labels.runId"),
      value: value.id,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.kind"),
      value: value.replay_kind,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.mode"),
      value: value.replay_mode,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.http"),
      value: value.http_status ?? emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.route"),
      value: value.executed_route_name || emptyValue,
      valueClass: "",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.provider"),
      value: value.executed_provider_id ?? emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.model"),
      value: value.executed_model_id ?? emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.tokens"),
      value: formatCompactMetrics([
        value.total_input_tokens,
        value.total_output_tokens,
        value.reasoning_tokens,
        value.total_tokens,
      ]),
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.cost"),
      value: formatPrice(
        value.estimated_cost_nanos,
        value.estimated_cost_currency,
      ),
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.started"),
      value: formatDate(value.started_at),
      valueClass: "",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.created"),
      value: formatDate(value.created_at),
      valueClass: "",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.firstByte"),
      value: formatDate(value.first_byte_at),
      valueClass: "",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.completed"),
      value: formatDate(value.completed_at),
      valueClass: "",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.artifact"),
      value: value.artifact_storage_key || emptyValue,
      valueClass: "font-mono text-xs",
    },
  ];
});

const artifactResultItems = computed(() => {
  const result = artifact.value?.result;
  if (!result) return [];
  const capture = result.response_body_capture;
  return [
    {
      label: $t("recordPage.detailDialog.replay.labels.status"),
      value: result.status,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.http"),
      value: result.http_status ?? emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.bodyCapture"),
      value: capture
        ? `${capture.state}${capture.truncated ? ` / ${$t("recordPage.detailDialog.replay.labels.captureTruncated")}` : ""}`
        : result.response_body_capture_state || emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.capturedBytes"),
      value: capture?.bytes_captured ?? emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.captureLimit"),
      value: capture?.capture_limit_bytes ?? emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.originalSize"),
      value: capture?.original_size_known
        ? (capture.original_size_bytes ?? emptyValue)
        : emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.captureEncoding"),
      value: capture?.body_encoding ?? emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.transformDiagnostics"),
      value: result.transform_diagnostics?.length ?? emptyValue,
      valueClass: "font-mono text-xs",
    },
  ];
});

const artifactDiffItems = computed(() => {
  const diff = artifact.value?.diff;
  if (!diff) return [];
  return [
    {
      label: $t("recordPage.detailDialog.replay.labels.baseline"),
      value: diff.baseline_kind,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.statusChanged"),
      value: formatBooleanLabel(diff.status_changed),
      valueClass: "",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.headersChanged"),
      value: formatBooleanLabel(diff.headers_changed),
      valueClass: "",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.bodyChanged"),
      value: formatBooleanLabel(diff.body_changed),
      valueClass: "",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.tokenDelta"),
      value: diff.token_delta ?? emptyValue,
      valueClass: "font-mono text-xs",
    },
    {
      label: $t("recordPage.detailDialog.replay.labels.costDelta"),
      value:
        diff.cost_delta != null
          ? $t("recordPage.detailDialog.replay.costDeltaNanos", {
              value: diff.cost_delta,
            })
          : emptyValue,
      valueClass: "font-mono text-xs",
    },
  ];
});

const ReplayInputSnapshot = defineComponent({
  props: {
    snapshot: {
      type: Object as () => RecordReplayInputSnapshot,
      required: true,
    },
    title: {
      type: String,
      default: "",
    },
  },
  setup(componentProps) {
    return () =>
      h("div", { class: "space-y-3" }, [
        h(
          "h4",
          { class: "text-sm font-semibold text-gray-900" },
          componentProps.title ||
            $t("recordPage.detailDialog.replay.inputBaseline"),
        ),
        componentProps.snapshot.kind === "attempt_upstream"
          ? h("div", { class: "space-y-3" }, [
              h(DefinitionGrid, {
                items: [
                  [
                    $t("recordPage.detailDialog.replay.labels.requestUri"),
                    componentProps.snapshot.request_uri,
                  ],
                  [
                    $t("recordPage.detailDialog.replay.labels.provider"),
                    componentProps.snapshot.provider?.provider_name ??
                      emptyValue,
                  ],
                  [
                    $t("recordPage.detailDialog.replay.labels.model"),
                    componentProps.snapshot.model?.model_name ?? emptyValue,
                  ],
                  [
                    $t("recordPage.detailDialog.replay.labels.realModel"),
                    componentProps.snapshot.model?.real_model_name ??
                      emptyValue,
                  ],
                ],
              }),
              h(NameValueBlock, {
                title: $t(
                  "recordPage.detailDialog.replay.sanitizedRequestHeaders",
                ),
                items: componentProps.snapshot.sanitized_request_headers,
              }),
              h(PreBlock, {
                title: $t("recordPage.detailDialog.replay.llmRequestBody"),
                value: summarizeReplayBodyLabelled(
                  componentProps.snapshot.llm_request_body,
                ),
              }),
            ])
          : h("div", { class: "space-y-3" }, [
              h(DefinitionGrid, {
                items: [
                  [
                    $t("recordPage.detailDialog.replay.labels.requestPath"),
                    componentProps.snapshot.request_path,
                  ],
                ],
              }),
              h(NameValueBlock, {
                title: $t("recordPage.detailDialog.replay.queryParams"),
                items: componentProps.snapshot.query_params,
              }),
              h(NameValueBlock, {
                title: $t(
                  "recordPage.detailDialog.replay.sanitizedOriginalHeaders",
                ),
                items: componentProps.snapshot.sanitized_original_headers,
              }),
              h(PreBlock, {
                title: $t("recordPage.detailDialog.replay.userRequestBody"),
                value: summarizeReplayBodyLabelled(
                  componentProps.snapshot.user_request_body,
                ),
              }),
            ]),
      ]);
  },
});

const ReplayExecutionPreview = defineComponent({
  props: {
    preview: {
      type: Object as () => RecordReplayExecutionPreview,
      required: true,
    },
    title: {
      type: String,
      default: "",
    },
  },
  setup(componentProps) {
    return () =>
      h("div", { class: "space-y-3" }, [
        h(
          "h4",
          { class: "text-sm font-semibold text-gray-900" },
          componentProps.title ||
            $t("recordPage.detailDialog.replay.executionPreview"),
        ),
        h(DefinitionGrid, {
          items: [
            [
              $t("recordPage.detailDialog.replay.labels.semanticBasis"),
              componentProps.preview.semantic_basis,
            ],
            [
              $t("recordPage.detailDialog.replay.labels.route"),
              componentProps.preview.resolved_route?.route_name ?? emptyValue,
            ],
            [
              $t("recordPage.detailDialog.replay.labels.candidate"),
              componentProps.preview.resolved_candidate?.candidate_position ??
                emptyValue,
            ],
            [
              $t("recordPage.detailDialog.replay.labels.provider"),
              componentProps.preview.resolved_candidate?.provider_id ??
                emptyValue,
            ],
            [
              $t("recordPage.detailDialog.replay.labels.model"),
              componentProps.preview.resolved_candidate?.model_id ?? emptyValue,
            ],
            [
              $t("recordPage.detailDialog.replay.labels.llmApi"),
              componentProps.preview.resolved_candidate?.llm_api_type ??
                emptyValue,
            ],
            [
              $t("recordPage.detailDialog.replay.labels.finalUri"),
              componentProps.preview.final_request_uri ?? emptyValue,
            ],
          ],
        }),
        h(NameValueBlock, {
          title: $t("recordPage.detailDialog.replay.finalRequestHeaders"),
          items: componentProps.preview.final_request_headers,
        }),
        h(PreBlock, {
          title: $t("recordPage.detailDialog.replay.finalRequestBody"),
          value: summarizeReplayBodyLabelled(
            componentProps.preview.final_request_body,
          ),
        }),
        componentProps.preview.candidate_decisions?.length
          ? h("div", { class: "space-y-2" }, [
              h(
                "h5",
                {
                  class:
                    "text-xs font-semibold uppercase tracking-wide text-gray-500",
                },
                $t("recordPage.detailDialog.replay.candidateDecisions"),
              ),
              h(
                "div",
                {
                  class:
                    "divide-y divide-gray-100 rounded-md border border-gray-200",
                },
                componentProps.preview.candidate_decisions.map((decision) =>
                  h(
                    "div",
                    {
                      class:
                        "grid grid-cols-2 gap-2 px-3 py-2 text-xs sm:grid-cols-5",
                    },
                    [
                      h(
                        "span",
                        { class: "font-mono text-gray-900" },
                        `#${decision.candidate_position}`,
                      ),
                      h(
                        "span",
                        { class: "font-mono text-gray-700" },
                        decision.attempt_status,
                      ),
                      h(
                        "span",
                        { class: "font-mono text-gray-700" },
                        decision.scheduler_action,
                      ),
                      h(
                        "span",
                        { class: "font-mono text-gray-700" },
                        decision.provider_id ?? emptyValue,
                      ),
                      h(
                        "span",
                        { class: "min-w-0 truncate font-mono text-gray-500" },
                        decision.error_code ??
                          decision.request_uri ??
                          emptyValue,
                      ),
                    ],
                  ),
                ),
              ),
            ])
          : null,
        componentProps.preview.applied_request_patch_summary != null
          ? h(PreBlock, {
              title: $t(
                "recordPage.detailDialog.replay.appliedRequestPatchSummary",
              ),
              value: formatJsonText(
                componentProps.preview.applied_request_patch_summary,
              ),
            })
          : null,
      ]);
  },
});

const ReplayBaseline = defineComponent({
  props: {
    preview: {
      type: Object as () => RecordReplayPreviewResponse,
      required: true,
    },
  },
  setup(componentProps) {
    return () => {
      const baseline = componentProps.preview.baseline;
      const items: Array<[string, string | number | boolean]> =
        "attempt_status" in baseline
          ? [
              [
                $t("recordPage.detailDialog.replay.labels.attemptStatus"),
                baseline.attempt_status,
              ],
              [
                $t("recordPage.detailDialog.replay.labels.http"),
                baseline.http_status ?? emptyValue,
              ],
              [
                $t("recordPage.detailDialog.replay.labels.bodyCapture"),
                baseline.response_body_capture_state ?? emptyValue,
              ],
              [
                $t("recordPage.detailDialog.replay.labels.tokens"),
                baseline.total_tokens ?? emptyValue,
              ],
              [
                $t("recordPage.detailDialog.replay.labels.cost"),
                formatPrice(
                  baseline.estimated_cost_nanos,
                  baseline.estimated_cost_currency,
                ),
              ],
            ]
          : [
              [
                $t("recordPage.detailDialog.replay.labels.overallStatus"),
                baseline.overall_status,
              ],
              [
                $t("recordPage.detailDialog.replay.labels.errorCode"),
                baseline.final_error_code ?? emptyValue,
              ],
              [
                $t("recordPage.detailDialog.replay.labels.bodyCapture"),
                baseline.user_response_body_capture_state ?? emptyValue,
              ],
              [
                $t("recordPage.detailDialog.replay.labels.tokens"),
                baseline.total_tokens ?? emptyValue,
              ],
              [
                $t("recordPage.detailDialog.replay.labels.cost"),
                formatPrice(
                  baseline.estimated_cost_nanos,
                  baseline.estimated_cost_currency,
                ),
              ],
            ];
      return h("div", { class: "space-y-3" }, [
        h(
          "h4",
          { class: "text-sm font-semibold text-gray-900" },
          $t("recordPage.detailDialog.replay.historicalBaseline"),
        ),
        h(DefinitionGrid, { items }),
      ]);
    };
  },
});

const DefinitionGrid = defineComponent({
  props: {
    items: {
      type: Array as () => Array<[string, string | number | boolean]>,
      required: true,
    },
  },
  setup(componentProps) {
    return () =>
      h(
        "dl",
        { class: "grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-3" },
        componentProps.items.map(([label, value]) =>
          h(
            "div",
            {
              class:
                "flex items-center justify-between gap-3 border-b border-gray-100 py-2.5",
            },
            [
              h(
                "dt",
                { class: "text-xs uppercase tracking-wide text-gray-500" },
                label,
              ),
              h(
                "dd",
                {
                  class:
                    "min-w-0 truncate text-right font-mono text-xs text-gray-900",
                },
                String(value),
              ),
            ],
          ),
        ),
      );
  },
});

const PreBlock = defineComponent({
  props: {
    title: {
      type: String,
      required: true,
    },
    value: {
      type: String,
      required: true,
    },
  },
  setup(componentProps) {
    return () =>
      h(
        "details",
        { class: "rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-2" },
        [
          h(
            "summary",
            {
              class:
                "cursor-pointer text-xs font-medium uppercase tracking-wide text-gray-500",
            },
            componentProps.title,
          ),
          h(
            "pre",
            {
              class:
                "mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-gray-700",
            },
            componentProps.value,
          ),
        ],
      );
  },
});
</script>
