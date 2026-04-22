<template>
  <div class="space-y-5 text-sm">
    <section>
      <h3 class="border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
        {{ $t("recordPage.detailDialog.overview.sections.request") }}
      </h3>
      <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-3">
        <div
          v-for="item in generalItems"
          :key="item.label"
          class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5"
        >
          <dt class="text-xs uppercase tracking-wide text-gray-500">{{ item.label }}</dt>
          <dd class="min-w-0 truncate text-right text-sm text-gray-900" :class="item.valueClass">
            {{ item.value }}
          </dd>
        </div>
      </dl>
    </section>

    <section v-if="record.final_error_code || record.final_error_message">
      <h3 class="border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
        {{ $t("recordPage.detailDialog.overview.sections.finalError") }}
      </h3>
      <div class="mt-3 rounded-lg border border-red-100 bg-red-50/70 px-3 py-2 text-red-800">
        <div class="font-mono text-xs font-semibold">
          {{ record.final_error_code || "request_error" }}
        </div>
        <div v-if="record.final_error_message" class="mt-1 break-words">
          {{ record.final_error_message }}
        </div>
      </div>
    </section>

    <section>
      <h3 class="border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
        {{ $t("recordPage.detailDialog.overview.sections.usage") }}
      </h3>
      <dl class="grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-4">
        <div
          v-for="item in usageItems"
          :key="item.label"
          class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5"
        >
          <dt class="text-xs text-gray-500">{{ item.label }}</dt>
          <dd class="font-semibold text-gray-900">{{ item.value }}</dd>
        </div>
      </dl>
    </section>

    <section>
      <h3 class="border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
        {{ $t("recordPage.detailDialog.overview.sections.timings") }}
      </h3>
      <dl class="grid grid-cols-1 gap-x-6 md:grid-cols-2">
        <div
          v-for="item in timingItems"
          :key="item.label"
          class="flex items-center justify-between gap-3 border-b border-gray-100 py-2.5"
        >
          <dt class="text-xs uppercase tracking-wide text-gray-500">{{ item.label }}</dt>
          <dd class="text-right text-sm text-gray-900">{{ item.value }}</dd>
        </div>
      </dl>
    </section>

    <section>
      <h3 class="border-b border-gray-100 pb-2 text-base font-semibold text-gray-900">
        {{ $t("recordPage.detailDialog.overview.sections.costSnapshot") }}
      </h3>
      <div v-if="parsedCostSnapshot" class="mt-3 space-y-4">
        <div class="flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <div class="text-xs text-gray-500">
              {{ $t("recordPage.detailDialog.overview.totalCost") }}
            </div>
            <div class="mt-1 text-lg font-semibold text-gray-900">
              {{
                formatPrice(
                  parsedCostSnapshot.total_cost_nanos,
                  parsedCostSnapshot.currency,
                )
              }}
            </div>
          </div>
          <Badge
            v-if="costIssueCount > 0"
            variant="outline"
            class="w-fit border-amber-200 bg-amber-50 text-amber-800"
          >
            {{ $t("recordPage.detailDialog.overview.warningCount", { count: costIssueCount }) }}
          </Badge>
        </div>

        <div
          v-if="parsedCostSnapshot.detail_lines.length === 0"
          class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
        >
          {{ $t("recordPage.detailDialog.overview.noCostDetailLines") }}
        </div>
        <div v-else class="overflow-hidden border-y border-gray-100">
          <div
            v-for="(line, index) in parsedCostSnapshot.detail_lines"
            :key="`${line.meter_key}-${index}`"
            class="grid grid-cols-1 gap-2 border-t border-gray-100 px-4 py-3 first:border-t-0 md:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)_minmax(0,1fr)_auto]"
          >
            <div class="min-w-0">
              <Badge variant="outline" class="max-w-full font-mono text-[11px]">
                {{ line.meter_key }}
              </Badge>
              <p v-if="line.description" class="mt-1 truncate text-xs text-gray-500">
                {{ line.description }}
              </p>
            </div>
            <div class="text-sm text-gray-700">
              <span class="mr-2 text-[11px] uppercase tracking-wide text-gray-400 md:hidden">
                {{ $t("recordPage.detailDialog.overview.costColumns.quantity") }}
              </span>
              {{ line.quantity }} {{ line.unit }}
            </div>
            <div class="text-sm text-gray-700">
              <span class="mr-2 text-[11px] uppercase tracking-wide text-gray-400 md:hidden">
                {{ $t("recordPage.detailDialog.overview.costColumns.unit") }}
              </span>
              {{ formatUnitPriceLabel(line.meter_key, line.unit_price_nanos, parsedCostSnapshot.currency) }}
            </div>
            <div class="text-sm font-semibold text-gray-900 md:text-right">
              <span class="mr-2 text-[11px] uppercase tracking-wide text-gray-400 md:hidden">
                {{ $t("recordPage.detailDialog.overview.costColumns.amount") }}
              </span>
              {{ formatPrice(line.amount_nanos, parsedCostSnapshot.currency) }}
            </div>
          </div>
        </div>
      </div>
      <pre
        v-else-if="record.cost_snapshot_json"
        class="mt-3 overflow-x-auto rounded-lg bg-gray-950 px-3 py-3 text-xs text-gray-100"
      >{{ record.cost_snapshot_json }}</pre>
      <div
        v-else
        class="mt-3 rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
      >
        {{ $t("recordPage.detailDialog.overview.noCostSnapshot") }}
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { Badge } from "@/components/ui/badge";
import type { CostSnapshot, RecordRequest } from "@/store/types";
import {
  emptyValue,
  formatDate,
  formatDuration,
  formatPrice,
  formatUnitPrice,
} from "./recordFormat";

const props = defineProps<{
  record: RecordRequest;
  apiKeyName: string;
  providerName: string;
}>();

const { t: $t } = useI18n();

const parsedCostSnapshot = computed<CostSnapshot | null>(() => {
  const raw = props.record.cost_snapshot_json;
  if (!raw) return null;
  try {
    return JSON.parse(raw) as CostSnapshot;
  } catch {
    return null;
  }
});

const costIssueCount = computed(() => {
  const snapshot = parsedCostSnapshot.value;
  if (!snapshot) return 0;
  return snapshot.warnings?.length ?? 0 + snapshot.unmatched_items.length;
});

const formatResolvedScopeLabel = (scope: string | null | undefined) => {
  switch (scope) {
    case "direct":
      return $t("recordPage.detailDialog.overview.resolvedScope.direct");
    case "global_route":
      return $t("recordPage.detailDialog.overview.resolvedScope.globalRoute");
    case "api_key_override":
      return $t("recordPage.detailDialog.overview.resolvedScope.apiKeyOverride");
    default:
      return emptyValue;
  }
};

const formatAttemptsSummary = () =>
  $t("recordPage.detailDialog.overview.attemptsSummary", {
    attempts: props.record.attempt_count,
    retries: props.record.retry_count,
    fallbacks: props.record.fallback_count,
  });

const formatUnitPriceLabel = (
  meterKey: string,
  unitPriceNanos: number | null,
  currency?: string | null,
) =>
  formatUnitPrice(meterKey, unitPriceNanos, currency, {
    tokens: $t("recordPage.detailDialog.overview.unitPrice.tokens"),
    unit: $t("recordPage.detailDialog.overview.unitPrice.unit"),
  });

const generalItems = computed(() => [
  { label: $t("recordPage.detailDialog.overview.labels.id"), value: props.record.id, valueClass: "font-mono text-xs" },
  { label: $t("recordPage.detailDialog.overview.labels.apiKey"), value: props.apiKeyName, valueClass: "" },
  {
    label: $t("recordPage.detailDialog.overview.labels.requestedModel"),
    value: props.record.requested_model_name || props.record.final_model_name_snapshot || emptyValue,
    valueClass: "font-mono text-xs",
  },
  { label: $t("recordPage.detailDialog.overview.labels.resolvedScope"), value: formatResolvedScopeLabel(props.record.resolved_name_scope), valueClass: "" },
  { label: $t("recordPage.detailDialog.overview.labels.resolvedRoute"), value: props.record.resolved_route_name || emptyValue, valueClass: "font-mono text-xs" },
  { label: $t("recordPage.detailDialog.overview.labels.finalProvider"), value: props.providerName, valueClass: "" },
  { label: $t("recordPage.detailDialog.overview.labels.selectedModel"), value: props.record.final_model_name_snapshot || emptyValue, valueClass: "font-mono text-xs" },
  { label: $t("recordPage.detailDialog.overview.labels.realModel"), value: props.record.final_real_model_name_snapshot || emptyValue, valueClass: "font-mono text-xs" },
  {
    label: $t("recordPage.detailDialog.overview.labels.attempts"),
    value: formatAttemptsSummary(),
    valueClass: "font-mono text-xs",
  },
  { label: $t("recordPage.detailDialog.overview.labels.userApiType"), value: props.record.user_api_type || emptyValue, valueClass: "" },
  { label: $t("recordPage.detailDialog.overview.labels.llmApiType"), value: props.record.final_llm_api_type || emptyValue, valueClass: "" },
  { label: $t("recordPage.detailDialog.overview.labels.clientIp"), value: props.record.client_ip || emptyValue, valueClass: "font-mono text-xs" },
]);

const usageItems = computed(() => [
  { label: $t("recordPage.detailDialog.overview.usage.totalInputTokens"), value: props.record.total_input_tokens },
  { label: $t("recordPage.detailDialog.overview.usage.totalOutputTokens"), value: props.record.total_output_tokens },
  { label: $t("recordPage.detailDialog.overview.usage.reasoningTokens"), value: props.record.reasoning_tokens },
  { label: $t("recordPage.detailDialog.overview.usage.totalTokens"), value: props.record.total_tokens },
  { label: $t("recordPage.detailDialog.overview.usage.inputTextTokens"), value: props.record.input_text_tokens },
  { label: $t("recordPage.detailDialog.overview.usage.outputTextTokens"), value: props.record.output_text_tokens },
  { label: $t("recordPage.detailDialog.overview.usage.inputImageTokens"), value: props.record.input_image_tokens },
  { label: $t("recordPage.detailDialog.overview.usage.outputImageTokens"), value: props.record.output_image_tokens },
  { label: $t("recordPage.detailDialog.overview.usage.cacheReadTokens"), value: props.record.cache_read_tokens },
  { label: $t("recordPage.detailDialog.overview.usage.cacheWriteTokens"), value: props.record.cache_write_tokens },
].filter((item) => item.value != null));

const timingItems = computed(() => [
  { label: $t("recordPage.detailDialog.overview.timings.requestReceived"), value: formatDate(props.record.request_received_at) },
  { label: $t("recordPage.detailDialog.overview.timings.firstAttemptStarted"), value: formatDate(props.record.first_attempt_started_at) },
  { label: $t("recordPage.detailDialog.overview.timings.firstByteToClient"), value: formatDate(props.record.response_started_to_client_at) },
  { label: $t("recordPage.detailDialog.overview.timings.completed"), value: formatDate(props.record.completed_at) },
  {
    label: $t("recordPage.detailDialog.overview.timings.firstByteLatency"),
    value: formatDuration(
      props.record.first_attempt_started_at,
      props.record.response_started_to_client_at,
    ),
  },
  {
    label: $t("recordPage.detailDialog.overview.timings.totalLatency"),
    value: formatDuration(props.record.request_received_at, props.record.completed_at),
  },
]);
</script>
