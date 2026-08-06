import { computed, ref } from "vue";
import type {
  PaginatedResponse,
  RecordListItem,
  RecordListParams,
} from "../../../services/types";
import { normalizeError } from "../../../utils/error.ts";
import type { EnrichedRecordListItem, FilterOption, RecordFilters } from "../types";
import {
  emptyValue,
  formatDate,
  formatDuration,
  formatPrice,
} from "./recordFormat.ts";
import { formatSourceIdentity } from "../../../utils/sourceEvidence.ts";
import { DEFAULT_RECORD_FILTERS, RECORD_ADVANCED_FILTER_KEYS } from "./useRecordQuery.ts";

export type RecordTpsDurationKind = "stream_tail" | "effective";

export interface RecordTpsInput {
  total_output_tokens?: number | null;
  output_text_tokens?: number | null;
  reasoning_tokens?: number | null;
  upstream_request_sent_at?: number | null;
  first_token_at?: number | null;
  completed_at?: number | null;
  is_stream?: boolean | null;
}

export interface RecordTpsCalculation {
  value: number;
  tokens: number;
  durationMs: number;
  durationKind: RecordTpsDurationKind;
}

export const STREAM_TPS_MIN_TAIL_MS = 750;
export const STREAM_TPS_MIN_TAIL_RATIO = 0.05;
export const STREAM_TPS_MIN_OUTPUT_TOKENS = 8;

const finiteNonNegativeNumber = (value: number | null | undefined) =>
  typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;

const finiteTimestamp = (value: number | null | undefined) =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

export const resolveVisibleOutputTokens = (record: RecordTpsInput) => {
  const outputTextTokens = finiteNonNegativeNumber(record.output_text_tokens);
  if (outputTextTokens != null) {
    return outputTextTokens;
  }

  const totalOutputTokens = finiteNonNegativeNumber(record.total_output_tokens);
  if (totalOutputTokens == null) {
    return null;
  }

  const reasoningTokens = finiteNonNegativeNumber(record.reasoning_tokens);
  if (reasoningTokens != null) {
    return Math.max(totalOutputTokens - reasoningTokens, 0);
  }

  return totalOutputTokens;
};

export const calculateRecordTps = (
  record: RecordTpsInput,
): RecordTpsCalculation | null => {
  const tokens = resolveVisibleOutputTokens(record);
  const startedAt = finiteTimestamp(record.upstream_request_sent_at);
  const completedAt = finiteTimestamp(record.completed_at);

  if (tokens == null || tokens <= 0 || startedAt == null || completedAt == null) {
    return null;
  }

  const totalMs = completedAt - startedAt;
  if (totalMs <= 0) {
    return null;
  }

  let durationMs = totalMs;
  let durationKind: RecordTpsDurationKind = "effective";
  const firstTokenAt = finiteTimestamp(record.first_token_at);
  const streamTailMs = firstTokenAt == null ? null : completedAt - firstTokenAt;
  const canUseStreamTail =
    record.is_stream === true &&
    streamTailMs != null &&
    streamTailMs > 0 &&
    streamTailMs >= STREAM_TPS_MIN_TAIL_MS &&
    streamTailMs / totalMs >= STREAM_TPS_MIN_TAIL_RATIO &&
    tokens >= STREAM_TPS_MIN_OUTPUT_TOKENS;

  if (canUseStreamTail && streamTailMs != null) {
    durationMs = streamTailMs;
    durationKind = "stream_tail";
  }

  return {
    value: tokens / (durationMs / 1000),
    tokens,
    durationMs,
    durationKind,
  };
};

export type RecordListTranslator = (
  key: string,
  params?: Record<string, unknown>,
) => string;

type NamedEntity = {
  id: number;
  name: string;
};

type ProviderEntity = NamedEntity & {
  upstream_source: {
    id: number;
    source_key: string;
    profile_type: string;
  };
};

type ModelOption = {
  value: number | string;
  label: string;
};

export interface UseRecordListOptions {
  filters: RecordFilters;
  currentPage: { value: number };
  pageSize: { value: number };
  buildListParams: () => RecordListParams;
  t: RecordListTranslator;
  providerStore: {
    providers: ProviderEntity[];
    fetchProviders: () => Promise<unknown>;
  };
  apiKeyStore: {
    apiKeys: NamedEntity[];
    fetchApiKeys: () => Promise<unknown>;
  };
  modelStore: {
    modelOptions: ModelOption[];
    fetchModels: () => Promise<unknown>;
  };
  api: {
    getRecordList: (
      params: RecordListParams,
    ) => Promise<PaginatedResponse<RecordListItem>>;
  };
}

const allOption = (label: string): FilterOption => ({ value: "ALL", label });

export function useRecordList(options: UseRecordListOptions) {
  const api = options.api;
  const records = ref<EnrichedRecordListItem[]>([]);
  const totalRecords = ref(0);
  const isLoading = ref(false);
  const errorMsg = ref<string | null>(null);

  const totalPages = computed(() =>
    Math.ceil(totalRecords.value / options.pageSize.value),
  );

  const apiKeyOptions = computed<FilterOption[]>(() => [
    { value: "0", label: options.t("recordPage.filter.allApiKeys") },
    ...(options.apiKeyStore.apiKeys || []).map((key) => ({
      value: String(key.id),
      label: key.name,
    })),
  ]);

  const providerOptions = computed<FilterOption[]>(() => [
    { value: "0", label: options.t("recordPage.filter.allProviders") },
    ...(options.providerStore.providers || []).map((provider) => ({
      value: String(provider.id),
      label: provider.name,
    })),
  ]);

  const sourceOptions = computed<FilterOption[]>(() => [
    { value: "0", label: options.t("recordPage.filter.allSources") },
    ...(options.providerStore.providers || []).map((provider) => ({
      value: String(provider.upstream_source.id),
      label: `${provider.name} / ${provider.upstream_source.source_key} / ${provider.upstream_source.profile_type}`,
    })),
  ]);

  const modelOptions = computed<FilterOption[]>(() => [
    { value: "0", label: options.t("recordPage.filter.allModels") },
    ...options.modelStore.modelOptions.map((model) => ({
      value: String(model.value),
      label: model.label,
    })),
  ]);

  const statusOptions = computed<FilterOption[]>(() => [
    allOption(options.t("recordPage.filter.allStatuses")),
    { value: "SUCCESS", label: options.t("recordPage.filter.status.SUCCESS") },
    { value: "PENDING", label: options.t("recordPage.filter.status.PENDING") },
    { value: "ERROR", label: options.t("recordPage.filter.status.ERROR") },
    { value: "CANCELLED", label: options.t("recordPage.filter.status.CANCELLED") },
  ]);

  const downstreamProtocolOptions = computed<FilterOption[]>(() => [
    allOption(options.t("recordPage.filter.allDownstreamProtocols")),
    { value: "OPENAI", label: "OpenAI" },
    { value: "RESPONSES", label: "Responses" },
    { value: "ANTHROPIC", label: "Anthropic" },
    { value: "GEMINI", label: "Gemini" },
  ]);

  const hasActiveFilters = computed(() =>
    (Object.keys(DEFAULT_RECORD_FILTERS) as Array<keyof RecordFilters>).some(
      (key) => options.filters[key] !== DEFAULT_RECORD_FILTERS[key],
    ),
  );

  const activeFilterCount = computed(() =>
    (Object.keys(DEFAULT_RECORD_FILTERS) as Array<keyof RecordFilters>).filter(
      (key) => options.filters[key] !== DEFAULT_RECORD_FILTERS[key],
    ).length,
  );

  const advancedActiveFilterCount = computed(() =>
    RECORD_ADVANCED_FILTER_KEYS.filter(
      (key) => options.filters[key] !== DEFAULT_RECORD_FILTERS[key],
    ).length,
  );

  const filterSummary = computed(() => {
    if (!hasActiveFilters.value) {
      return options.t("recordPage.filter.summaryAll");
    }
    return options.t("recordPage.filter.summaryActive", {
      count: activeFilterCount.value,
    });
  });

  const getProviderName = (id: number | null) => {
    if (id == null) return emptyValue;
    return (
      options.providerStore.providers.find((provider) => provider.id === id)?.name ||
      emptyValue
    );
  };

  const getApiKeyName = (id: number | null) => {
    if (id == null) return emptyValue;
    return (
      options.apiKeyStore.apiKeys.find((key) => key.id === id)?.name || emptyValue
    );
  };

  const formatTps = (record: RecordListItem) =>
    calculateRecordTps(record)?.value.toFixed(2) ?? emptyValue;

  const formatTtft = (record: RecordListItem) => {
    if (!record.is_stream) return options.t("recordPage.detailDialog.timeline.notApplicable");
    if (record.first_token_at == null) {
      return options.t("recordPage.detailDialog.timeline.notObserved");
    }
    return formatDuration(record.upstream_request_sent_at, record.first_token_at);
  };

  const enrichRecord = (record: RecordListItem): EnrichedRecordListItem => {
    const providerName =
      record.provider_name || getProviderName(record.provider_id);
    const apiKeyName = getApiKeyName(record.api_key_id);
    const firstResponseBodyTimeDisplay = formatDuration(
      record.upstream_request_sent_at,
      record.first_response_body_at,
    );
    const totalLatencyDisplay = formatDuration(
      record.upstream_request_sent_at,
      record.completed_at,
    );
    return {
      ...record,
      providerName,
      sourceDisplay: formatSourceIdentity(
        record,
        options.t("recordPage.source.unselected"),
      ),
      apiKeyName,
      displayRequestedModelName:
        record.model_name || record.requested_model_name || emptyValue,
      httpStatusDisplay: record.upstream_http_status?.toString() ?? emptyValue,
      firstResponseBodyTimeDisplay,
      ttftDisplay: formatTtft(record),
      totalLatencyDisplay,
      tpsDisplay: formatTps(record),
      costDisplay: formatPrice(
        record.estimated_cost_nanos,
        record.estimated_cost_currency,
      ),
      request_at_formatted: formatDate(record.request_received_at),
    };
  };

  const fetchRecords = async () => {
    isLoading.value = true;
    errorMsg.value = null;

    try {
      const result: PaginatedResponse<RecordListItem> = await api.getRecordList(
        options.buildListParams(),
      );
      records.value = (result.list || []).map(enrichRecord);
      totalRecords.value = result.total || 0;
    } catch (err: unknown) {
      errorMsg.value = normalizeError(err, options.t("recordPage.fetchFailed")).message;
    } finally {
      isLoading.value = false;
    }
  };

  const loadFilterOptions = async () => {
    await Promise.all([
      options.providerStore.fetchProviders(),
      options.apiKeyStore.fetchApiKeys(),
      options.modelStore.fetchModels(),
    ]);
  };

  return {
    records,
    totalRecords,
    totalPages,
    isLoading,
    errorMsg,
    apiKeyOptions,
    providerOptions,
    sourceOptions,
    modelOptions,
    statusOptions,
    downstreamProtocolOptions,
    hasActiveFilters,
    activeFilterCount,
    advancedActiveFilterCount,
    filterSummary,
    fetchRecords,
    loadFilterOptions,
    getProviderName,
    getApiKeyName,
  };
}
