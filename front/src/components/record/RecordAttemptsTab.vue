<template>
  <section class="space-y-4 text-sm">
    <div class="flex flex-col gap-2 border-b border-gray-100 pb-2 sm:flex-row sm:items-center sm:justify-between">
      <h3 class="text-base font-semibold text-gray-900">
        {{ $t("recordPage.detailDialog.attempts.title") }}
      </h3>
      <Badge variant="outline" class="w-fit font-mono text-xs">
        {{ $t("recordPage.detailDialog.attempts.count", { count: attempts.length }) }}
      </Badge>
    </div>

    <div
      v-if="attempts.length === 0"
      class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
    >
      {{ $t("recordPage.detailDialog.attempts.empty") }}
    </div>

    <ol v-else class="divide-y divide-gray-100">
      <li
        v-for="attempt in attempts"
        :key="attempt.id"
        class="grid grid-cols-[auto_minmax(0,1fr)] gap-3 py-4 first:pt-0"
      >
        <div class="pt-1">
          <component
            :is="getStatusMeta(attempt.attempt_status).icon"
            class="h-4 w-4"
            :class="getStatusMeta(attempt.attempt_status).className"
          />
        </div>
        <div class="min-w-0 space-y-3">
          <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2">
                <h4 class="text-sm font-semibold text-gray-900">
                  {{ $t("recordPage.detailDialog.attempts.attemptTitle", { index: attempt.attempt_index }) }}
                </h4>
                <Badge :variant="getStatusBadgeVariant(attempt.attempt_status)">
                  {{ attempt.attempt_status }}
                </Badge>
                <Badge variant="outline" class="font-mono text-[11px]">
                  {{ attempt.scheduler_action }}
                </Badge>
                <Badge
                  v-if="attempt.http_status != null"
                  variant="outline"
                  class="font-mono text-[11px]"
                >
                  HTTP {{ attempt.http_status }}
                </Badge>
              </div>
              <p class="mt-1 break-all font-mono text-xs text-gray-600">
                {{ formatAttemptProviderModel(attempt) }}
              </p>
            </div>
            <div class="font-mono text-xs text-gray-500 sm:text-right">
              <div>{{ $t("recordPage.detailDialog.attempts.candidate", { position: attempt.candidate_position }) }}</div>
              <div>{{ formatDuration(attempt.started_at, attempt.completed_at) }}</div>
            </div>
          </div>

          <div
            v-if="attempt.error_code || attempt.error_message"
            class="rounded-lg border border-red-100 bg-red-50/70 px-3 py-2 text-sm text-red-800"
          >
            <div class="font-mono text-xs font-semibold">
              {{ attempt.error_code || "attempt_error" }}
            </div>
            <div v-if="attempt.error_message" class="mt-1 break-words">
              {{ attempt.error_message }}
            </div>
          </div>

          <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-3">
            <div
              v-for="item in attemptMetricItems(attempt)"
              :key="item.label"
              class="flex items-center justify-between gap-3 border-b border-gray-100 py-2"
            >
              <dt class="text-xs uppercase tracking-wide text-gray-500">{{ item.label }}</dt>
              <dd class="truncate text-right text-sm text-gray-900" :class="item.valueClass">
                {{ item.value }}
              </dd>
            </div>
          </dl>

          <div v-if="attempt.request_uri" class="space-y-1 border-b border-gray-100 pb-3">
            <div class="text-xs uppercase tracking-wide text-gray-500">
              {{ $t("recordPage.detailDialog.attempts.requestUri") }}
            </div>
            <div class="break-all font-mono text-xs text-gray-700">
              {{ attempt.request_uri }}
            </div>
          </div>

          <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
            <details
              v-for="block in detailBlocks(attempt)"
              :key="block.label"
              class="rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-2"
            >
              <summary class="cursor-pointer text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ block.label }}
              </summary>
              <pre class="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-gray-700">{{ block.value }}</pre>
            </details>
          </div>
        </div>
      </li>
    </ol>
  </section>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { CircleAlert, CircleCheckBig, CircleHelp, Clock3 } from "lucide-vue-next";
import { Badge } from "@/components/ui/badge";
import type { RecordAttempt } from "@/store/types";
import {
  emptyValue,
  formatCompactMetrics,
  formatDate,
  formatDuration,
  formatJsonText,
  formatPrice,
  getStatusBadgeVariant,
  hasText,
} from "./recordFormat";

defineProps<{
  attempts: RecordAttempt[];
}>();

const { t: $t } = useI18n();

const getStatusMeta = (status: string | null) => {
  switch (status) {
    case "SUCCESS":
      return {
        icon: CircleCheckBig,
        className: "text-emerald-600",
      };
    case "ERROR":
      return {
        icon: CircleAlert,
        className: "text-red-600",
      };
    case "PENDING":
      return {
        icon: Clock3,
        className: "text-amber-600",
      };
    default:
      return {
        icon: CircleHelp,
        className: "text-gray-500",
      };
  }
};

const formatAttemptProviderModel = (attempt: RecordAttempt) => {
  const provider = attempt.provider_name_snapshot || emptyValue;
  const model = attempt.model_name_snapshot || emptyValue;
  const realModel = attempt.real_model_name_snapshot;
  return realModel ? `${provider} / ${model} -> ${realModel}` : `${provider} / ${model}`;
};

const formatAttemptTokens = (attempt: RecordAttempt) =>
  formatCompactMetrics([
    attempt.total_input_tokens,
    attempt.total_output_tokens,
    attempt.reasoning_tokens,
    attempt.total_tokens,
  ]);

const formatBooleanLabel = (value: boolean | null | undefined) => {
  if (value == null) return emptyValue;
  return value ? $t("common.yes") : $t("common.no");
};

const attemptMetricItems = (attempt: RecordAttempt) => [
  { label: $t("recordPage.detailDialog.attempts.labels.started"), value: formatDate(attempt.started_at), valueClass: "" },
  { label: $t("recordPage.detailDialog.attempts.labels.firstByte"), value: formatDate(attempt.first_byte_at), valueClass: "" },
  { label: $t("recordPage.detailDialog.attempts.labels.completed"), value: formatDate(attempt.completed_at), valueClass: "" },
  {
    label: $t("recordPage.detailDialog.attempts.labels.firstByteLatency"),
    value: formatDuration(attempt.started_at, attempt.first_byte_at),
    valueClass: "font-mono text-xs",
  },
  {
    label: $t("recordPage.detailDialog.attempts.labels.clientVisible"),
    value: formatBooleanLabel(attempt.response_started_to_client),
    valueClass: "",
  },
  {
    label: $t("recordPage.detailDialog.attempts.labels.backoff"),
    value: attempt.backoff_ms != null ? `${attempt.backoff_ms} ms` : emptyValue,
    valueClass: "font-mono text-xs",
  },
  { label: $t("recordPage.detailDialog.attempts.labels.llmApi"), value: attempt.llm_api_type || emptyValue, valueClass: "" },
  { label: $t("recordPage.detailDialog.attempts.labels.tokens"), value: formatAttemptTokens(attempt), valueClass: "font-mono text-xs" },
  {
    label: $t("recordPage.detailDialog.attempts.labels.cost"),
    value: formatPrice(attempt.estimated_cost_nanos, attempt.estimated_cost_currency),
    valueClass: "font-mono text-xs",
  },
];

const detailBlocks = (attempt: RecordAttempt) =>
  [
    { label: $t("recordPage.detailDialog.attempts.blocks.requestHeaders"), raw: attempt.request_headers_json },
    { label: $t("recordPage.detailDialog.attempts.blocks.responseHeaders"), raw: attempt.response_headers_json },
    { label: $t("recordPage.detailDialog.attempts.blocks.patchTrace"), raw: attempt.request_patch_summary_json },
    { label: $t("recordPage.detailDialog.attempts.blocks.appliedPatchRules"), raw: attempt.applied_request_patch_ids_json },
  ]
    .filter((block) => hasText(block.raw))
    .map((block) => ({
      label: block.label,
      value: formatJsonText(block.raw),
    }));
</script>
