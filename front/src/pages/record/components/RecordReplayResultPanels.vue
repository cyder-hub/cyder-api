<template>
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
    <ReplaySnapshotBlock
      :title="$t('recordPage.detailDialog.replay.inputBaseline')"
      :value="preview.input_snapshot"
    />
    <ReplaySnapshotBlock
      :title="$t('recordPage.detailDialog.replay.executionPreview')"
      :value="preview.execution_preview"
    />
    <ReplaySnapshotBlock
      :title="$t('recordPage.detailDialog.replay.historicalBaseline')"
      :value="preview.baseline"
    />
  </section>

  <section v-if="run" class="space-y-4">
    <div
      class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between"
    >
      <h3 class="text-base font-semibold text-gray-900">
        {{ $t("recordPage.detailDialog.replay.runTitle") }}
      </h3>
      <Badge :variant="getStatusBadgeVariant(run.status)" class="w-fit font-mono text-xs">
        {{ run.status }}
      </Badge>
    </div>
    <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-4">
      <div
        v-for="item in runSummaryItems"
        :key="item.label"
        class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5"
      >
        <dt class="text-xs uppercase tracking-wide text-gray-500">{{ item.label }}</dt>
        <dd class="truncate text-right text-sm text-gray-900" :class="item.valueClass">
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

    <ReplaySnapshotBlock
      v-if="artifact.input_snapshot"
      :title="$t('recordPage.detailDialog.replay.persistedInputBaseline')"
      :value="artifact.input_snapshot"
    />
    <ReplaySnapshotBlock
      v-if="artifact.execution_preview"
      :title="$t('recordPage.detailDialog.replay.persistedExecutionPreview')"
      :value="artifact.execution_preview"
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
          <dt class="text-xs uppercase tracking-wide text-gray-500">{{ item.label }}</dt>
          <dd class="truncate text-right text-sm text-gray-900" :class="item.valueClass">
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
        <summary class="cursor-pointer text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("recordPage.detailDialog.replay.responseBody") }}
        </summary>
        <pre class="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-gray-700">{{
          summarizeReplayBodyLabelled(artifact.result.response_body)
        }}</pre>
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
          <dt class="text-xs uppercase tracking-wide text-gray-500">{{ item.label }}</dt>
          <dd class="truncate text-right text-sm text-gray-900" :class="item.valueClass">
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

<script setup lang="ts">
import { computed, defineComponent, h } from "vue";
import { useI18n } from "vue-i18n";
import { Badge } from "@/components/ui/badge";
import type {
  RecordReplayArtifact,
  RecordReplayPreviewResponse,
  RecordReplayRun,
} from "@/services/types";
import NameValueBlock from "./NameValueBlock.vue";
import {
  emptyValue,
  formatCompactMetrics,
  formatDate,
  formatJsonText,
  formatPrice,
  getStatusBadgeVariant,
  summarizeReplayBody,
} from "../composables/recordFormat";

const props = defineProps<{
  preview: RecordReplayPreviewResponse | null;
  run: RecordReplayRun | null;
  artifact: RecordReplayArtifact | null;
}>();

const { t: $t } = useI18n();

const formatBooleanLabel = (value: boolean | null | undefined) => {
  if (value == null) return emptyValue;
  return value ? $t("common.yes") : $t("common.no");
};

const summarizeReplayBodyLabelled = (
  body: Parameters<typeof summarizeReplayBody>[0],
) => summarizeReplayBody(body, $t("recordPage.detailDialog.replay.capture"));

const runSummaryItems = computed(() => {
  const value = props.run;
  if (!value) return [];
  return [
    { label: $t("recordPage.detailDialog.replay.labels.runId"), value: value.id, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.kind"), value: value.replay_kind, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.mode"), value: value.replay_mode, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.http"), value: value.http_status ?? emptyValue, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.route"), value: value.executed_route_name || emptyValue, valueClass: "" },
    { label: $t("recordPage.detailDialog.replay.labels.provider"), value: value.executed_provider_id ?? emptyValue, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.model"), value: value.executed_model_id ?? emptyValue, valueClass: "font-mono text-xs" },
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
      value: formatPrice(value.estimated_cost_nanos, value.estimated_cost_currency),
      valueClass: "font-mono text-xs",
    },
    { label: $t("recordPage.detailDialog.replay.labels.started"), value: formatDate(value.started_at), valueClass: "" },
    { label: $t("recordPage.detailDialog.replay.labels.created"), value: formatDate(value.created_at), valueClass: "" },
    { label: $t("recordPage.detailDialog.replay.labels.firstByte"), value: formatDate(value.first_byte_at), valueClass: "" },
    { label: $t("recordPage.detailDialog.replay.labels.completed"), value: formatDate(value.completed_at), valueClass: "" },
    { label: $t("recordPage.detailDialog.replay.labels.artifact"), value: value.artifact_storage_key || emptyValue, valueClass: "font-mono text-xs" },
  ];
});

const artifactResultItems = computed(() => {
  const result = props.artifact?.result;
  if (!result) return [];
  const capture = result.response_body_capture;
  return [
    { label: $t("recordPage.detailDialog.replay.labels.status"), value: result.status, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.http"), value: result.http_status ?? emptyValue, valueClass: "font-mono text-xs" },
    {
      label: $t("recordPage.detailDialog.replay.labels.bodyCapture"),
      value: capture
        ? `${capture.state}${capture.truncated ? ` / ${$t("recordPage.detailDialog.replay.labels.captureTruncated")}` : ""}`
        : result.response_body_capture_state || emptyValue,
      valueClass: "font-mono text-xs",
    },
    { label: $t("recordPage.detailDialog.replay.labels.capturedBytes"), value: capture?.bytes_captured ?? emptyValue, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.captureLimit"), value: capture?.capture_limit_bytes ?? emptyValue, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.originalSize"), value: capture?.original_size_known ? (capture.original_size_bytes ?? emptyValue) : emptyValue, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.captureEncoding"), value: capture?.body_encoding ?? emptyValue, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.transformDiagnostics"), value: result.transform_diagnostics?.length ?? emptyValue, valueClass: "font-mono text-xs" },
  ];
});

const artifactDiffItems = computed(() => {
  const diff = props.artifact?.diff;
  if (!diff) return [];
  return [
    { label: $t("recordPage.detailDialog.replay.labels.baseline"), value: diff.baseline_kind, valueClass: "font-mono text-xs" },
    { label: $t("recordPage.detailDialog.replay.labels.statusChanged"), value: formatBooleanLabel(diff.status_changed), valueClass: "" },
    { label: $t("recordPage.detailDialog.replay.labels.headersChanged"), value: formatBooleanLabel(diff.headers_changed), valueClass: "" },
    { label: $t("recordPage.detailDialog.replay.labels.bodyChanged"), value: formatBooleanLabel(diff.body_changed), valueClass: "" },
    { label: $t("recordPage.detailDialog.replay.labels.tokenDelta"), value: diff.token_delta ?? emptyValue, valueClass: "font-mono text-xs" },
    {
      label: $t("recordPage.detailDialog.replay.labels.costDelta"),
      value:
        diff.cost_delta != null
          ? $t("recordPage.detailDialog.replay.costDeltaNanos", { value: diff.cost_delta })
          : emptyValue,
      valueClass: "font-mono text-xs",
    },
  ];
});

const ReplaySnapshotBlock = defineComponent({
  props: {
    title: { type: String, required: true },
    value: { type: Object, required: true },
  },
  setup(componentProps) {
    return () =>
      h(
        "details",
        { class: "rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-2" },
        [
          h(
            "summary",
            { class: "cursor-pointer text-xs font-medium uppercase tracking-wide text-gray-500" },
            componentProps.title,
          ),
          h(
            "pre",
            { class: "mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-gray-700" },
            formatJsonText(componentProps.value),
          ),
        ],
      );
  },
});
</script>
