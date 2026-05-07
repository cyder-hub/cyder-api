import { reactive, ref } from "vue";
import type { LocationQuery, Router, RouteLocationNormalizedLoaded } from "vue-router";
import type { RecordListParams } from "../../../services/types";
import type { BooleanFilter, RecordFilters } from "../types";
import {
  RECORD_DETAIL_TABS,
  type RecordDetailTab,
} from "./useRecordDetail.ts";

export const DEFAULT_RECORD_PAGE = 1;
export const FALLBACK_RECORD_PAGE_SIZE = 10;

export const DEFAULT_RECORD_FILTERS: RecordFilters = {
  api_key_id: 0,
  provider_id: 0,
  model_id: 0,
  status: "ALL",
  user_api_type: "ALL",
  resolved_name_scope: "ALL",
  final_error_code: "",
  has_retry: "ALL",
  has_fallback: "ALL",
  has_transform_diagnostics: "ALL",
  latency_ms_min: "",
  latency_ms_max: "",
  total_tokens_min: "",
  total_tokens_max: "",
  estimated_cost_nanos_min: "",
  estimated_cost_nanos_max: "",
  start_time: "",
  end_time: "",
  search: "",
};

export const VALID_RECORD_STATUSES = new Set([
  "ALL",
  "SUCCESS",
  "PENDING",
  "ERROR",
  "CANCELLED",
]);

export const VALID_BOOLEAN_FILTERS = new Set(["ALL", "true", "false"]);

export const RECORD_ADVANCED_FILTER_KEYS: Array<keyof RecordFilters> = [
  "user_api_type",
  "resolved_name_scope",
  "final_error_code",
  "has_retry",
  "has_fallback",
  "has_transform_diagnostics",
  "latency_ms_min",
  "latency_ms_max",
  "total_tokens_min",
  "total_tokens_max",
  "estimated_cost_nanos_min",
  "estimated_cost_nanos_max",
  "start_time",
  "end_time",
];

const validDetailTabs = new Set(RECORD_DETAIL_TABS.map((tab) => tab.value));

export type RecordQueryEntityValidators = {
  hasProviderId?: (id: number) => boolean;
  hasApiKeyId?: (id: number) => boolean;
  hasModelId?: (id: number) => boolean;
};

export const getSingleRecordQueryValue = (value: LocationQuery[string]) => {
  if (Array.isArray(value)) return value[0];
  return value;
};

export const parsePositiveIntRecordQuery = (
  value: LocationQuery[string],
  fallback: number,
) => {
  const raw = getSingleRecordQueryValue(value);
  if (raw == null || raw === "") return fallback;
  const parsed = Number(raw);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
};

export const parseNullablePositiveIntRecordQuery = (
  value: LocationQuery[string],
) => {
  const parsed = parsePositiveIntRecordQuery(value, 0);
  return parsed > 0 ? parsed : null;
};

export const parseStringRecordQuery = (
  value: LocationQuery[string],
  fallback = "",
) => {
  const raw = getSingleRecordQueryValue(value);
  return typeof raw === "string" ? raw : fallback;
};

export const parseRecordStatusQuery = (value: LocationQuery[string]) => {
  const raw = getSingleRecordQueryValue(value);
  return raw && VALID_RECORD_STATUSES.has(raw) ? raw : DEFAULT_RECORD_FILTERS.status;
};

export const parseRecordBooleanFilterQuery = (
  value: LocationQuery[string],
): BooleanFilter => {
  const raw = getSingleRecordQueryValue(value);
  return raw && VALID_BOOLEAN_FILTERS.has(raw) ? (raw as BooleanFilter) : "ALL";
};

export const parseRecordDetailTabQuery = (
  value: LocationQuery[string],
): RecordDetailTab => {
  const raw = getSingleRecordQueryValue(value);
  return raw && validDetailTabs.has(raw as RecordDetailTab)
    ? (raw as RecordDetailTab)
    : "overview";
};

export const getStoredRecordPageSize = (
  storage: Pick<Storage, "getItem"> | null | undefined = globalThis.localStorage,
) =>
  Number(storage?.getItem("pageSize")) || FALLBACK_RECORD_PAGE_SIZE;

const acceptsEntityId = (
  id: number,
  validate: ((id: number) => boolean) | undefined,
) => id > 0 && (validate ? validate(id) : true);

export const parseRecordQueryState = (
  query: LocationQuery,
  fallbackPageSize = FALLBACK_RECORD_PAGE_SIZE,
  validators: RecordQueryEntityValidators = {},
) => {
  const providerId = parsePositiveIntRecordQuery(
    query.provider_id,
    DEFAULT_RECORD_FILTERS.provider_id,
  );
  const apiKeyId = parsePositiveIntRecordQuery(
    query.api_key_id,
    DEFAULT_RECORD_FILTERS.api_key_id,
  );
  const modelId = parsePositiveIntRecordQuery(
    query.model_id,
    DEFAULT_RECORD_FILTERS.model_id,
  );

  const filters: RecordFilters = {
    api_key_id: acceptsEntityId(apiKeyId, validators.hasApiKeyId) ? apiKeyId : 0,
    provider_id: acceptsEntityId(providerId, validators.hasProviderId)
      ? providerId
      : 0,
    model_id: acceptsEntityId(modelId, validators.hasModelId) ? modelId : 0,
    status: parseRecordStatusQuery(query.status),
    user_api_type: parseStringRecordQuery(query.user_api_type, "ALL") || "ALL",
    resolved_name_scope:
      parseStringRecordQuery(query.resolved_name_scope, "ALL") || "ALL",
    final_error_code: parseStringRecordQuery(query.final_error_code),
    has_retry: parseRecordBooleanFilterQuery(query.has_retry),
    has_fallback: parseRecordBooleanFilterQuery(query.has_fallback),
    has_transform_diagnostics: parseRecordBooleanFilterQuery(
      query.has_transform_diagnostics,
    ),
    latency_ms_min: parseStringRecordQuery(query.latency_ms_min),
    latency_ms_max: parseStringRecordQuery(query.latency_ms_max),
    total_tokens_min: parseStringRecordQuery(query.total_tokens_min),
    total_tokens_max: parseStringRecordQuery(query.total_tokens_max),
    estimated_cost_nanos_min: parseStringRecordQuery(
      query.estimated_cost_nanos_min,
    ),
    estimated_cost_nanos_max: parseStringRecordQuery(
      query.estimated_cost_nanos_max,
    ),
    start_time: parseStringRecordQuery(query.start_time),
    end_time: parseStringRecordQuery(query.end_time),
    search: parseStringRecordQuery(query.search),
  };

  return {
    page: parsePositiveIntRecordQuery(query.page, DEFAULT_RECORD_PAGE),
    pageSize: parsePositiveIntRecordQuery(query.page_size, fallbackPageSize),
    filters,
    recordId: parseNullablePositiveIntRecordQuery(query.record_id),
    tab: parseRecordDetailTabQuery(query.tab),
    attemptId: parseNullablePositiveIntRecordQuery(query.attempt_id),
    replayRunId: parseNullablePositiveIntRecordQuery(query.replay_run_id),
    hasAdvancedFilters: RECORD_ADVANCED_FILTER_KEYS.some(
      (key) => filters[key] !== DEFAULT_RECORD_FILTERS[key],
    ),
  };
};

export const buildRecordQueryFromState = (state: {
  page: number;
  pageSize: number;
  filters: RecordFilters;
  recordId?: number | null;
  tab?: RecordDetailTab;
  attemptId?: number | null;
  replayRunId?: number | null;
}) => {
  const query: Record<string, string> = {};

  if (state.page !== DEFAULT_RECORD_PAGE) query.page = String(state.page);
  if (state.pageSize !== FALLBACK_RECORD_PAGE_SIZE) {
    query.page_size = String(state.pageSize);
  }

  (Object.keys(DEFAULT_RECORD_FILTERS) as Array<keyof RecordFilters>).forEach(
    (key) => {
      const value = state.filters[key];
      if (value !== DEFAULT_RECORD_FILTERS[key]) {
        query[key] = String(value);
      }
    },
  );

  if (state.recordId != null) {
    query.record_id = String(state.recordId);
    if (state.tab && state.tab !== "overview") {
      query.tab = state.tab;
    }
    if (state.attemptId != null) query.attempt_id = String(state.attemptId);
    if (state.replayRunId != null) query.replay_run_id = String(state.replayRunId);
  }

  return query;
};

export const isSameRecordQuery = (
  currentQuery: LocationQuery,
  nextQuery: Record<string, string>,
) => {
  const currentEntries = Object.entries(currentQuery)
    .map(([key, value]) => [key, getSingleRecordQueryValue(value) ?? ""])
    .filter(([, value]) => value !== "")
    .sort(([left], [right]) => left.localeCompare(right));
  const nextEntries = Object.entries(nextQuery).sort(([left], [right]) =>
    left.localeCompare(right),
  );

  if (currentEntries.length !== nextEntries.length) return false;
  return currentEntries.every(([key, value], index) => {
    const [nextKey, nextValue] = nextEntries[index];
    return key === nextKey && value === nextValue;
  });
};

const numberRecordParam = (value: string) => {
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : undefined;
};

const booleanRecordParam = (value: BooleanFilter) => {
  if (value === "ALL") return undefined;
  return value === "true";
};

const timestampRecordParam = (value: string) => {
  if (!value) return undefined;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : undefined;
};

export const buildRecordListParams = (
  page: number,
  pageSize: number,
  filters: RecordFilters,
): RecordListParams => ({
  page,
  page_size: pageSize,
  api_key_id: filters.api_key_id || undefined,
  provider_id: filters.provider_id || undefined,
  model_id: filters.model_id || undefined,
  status: filters.status === "ALL" ? undefined : filters.status,
  user_api_type: filters.user_api_type === "ALL" ? undefined : filters.user_api_type,
  resolved_name_scope:
    filters.resolved_name_scope === "ALL" ? undefined : filters.resolved_name_scope,
  final_error_code: filters.final_error_code.trim() || undefined,
  has_retry: booleanRecordParam(filters.has_retry),
  has_fallback: booleanRecordParam(filters.has_fallback),
  has_transform_diagnostics: booleanRecordParam(filters.has_transform_diagnostics),
  latency_ms_min: numberRecordParam(filters.latency_ms_min),
  latency_ms_max: numberRecordParam(filters.latency_ms_max),
  total_tokens_min: numberRecordParam(filters.total_tokens_min),
  total_tokens_max: numberRecordParam(filters.total_tokens_max),
  estimated_cost_nanos_min: numberRecordParam(filters.estimated_cost_nanos_min),
  estimated_cost_nanos_max: numberRecordParam(filters.estimated_cost_nanos_max),
  start_time: timestampRecordParam(filters.start_time),
  end_time: timestampRecordParam(filters.end_time),
  search: filters.search || undefined,
});

export function useRecordQuery(options: {
  route: RouteLocationNormalizedLoaded;
  router: Router;
  validators?: RecordQueryEntityValidators;
}) {
  const pageSizeStorage = globalThis.localStorage;
  const currentPage = ref(DEFAULT_RECORD_PAGE);
  const pageSize = ref(getStoredRecordPageSize(pageSizeStorage));
  const searchInput = ref("");
  const filters = reactive<RecordFilters>({ ...DEFAULT_RECORD_FILTERS });
  const isAdvancedFilterOpen = ref(false);
  const selectedRecordId = ref<number | null>(null);
  const selectedTab = ref<RecordDetailTab>("overview");
  const selectedAttemptId = ref<number | null>(null);
  const selectedReplayRunId = ref<number | null>(null);

  const applyQueryToState = (query: LocationQuery) => {
    const parsed = parseRecordQueryState(
      query,
      getStoredRecordPageSize(pageSizeStorage),
      options.validators ?? {},
    );
    currentPage.value = parsed.page;
    pageSize.value = parsed.pageSize;
    pageSizeStorage?.setItem("pageSize", String(pageSize.value));
    Object.assign(filters, parsed.filters);
    searchInput.value = filters.search;
    selectedRecordId.value = parsed.recordId;
    selectedTab.value = parsed.tab;
    selectedAttemptId.value = parsed.attemptId;
    selectedReplayRunId.value = parsed.replayRunId;
    if (parsed.hasAdvancedFilters) {
      isAdvancedFilterOpen.value = true;
    }
  };

  const buildQueryFromState = () =>
    buildRecordQueryFromState({
      page: currentPage.value,
      pageSize: pageSize.value,
      filters,
      recordId: selectedRecordId.value,
      tab: selectedTab.value,
      attemptId: selectedAttemptId.value,
      replayRunId: selectedReplayRunId.value,
    });

  const syncRouteWithState = async () => {
    const nextQuery = buildQueryFromState();
    if (isSameRecordQuery(options.route.query, nextQuery)) {
      return false;
    }
    await options.router.replace({ query: nextQuery });
    return true;
  };

  const params = () => buildRecordListParams(currentPage.value, pageSize.value, filters);

  const clearDetailSelection = () => {
    selectedRecordId.value = null;
    selectedTab.value = "overview";
    selectedAttemptId.value = null;
    selectedReplayRunId.value = null;
  };

  return {
    currentPage,
    pageSize,
    searchInput,
    filters,
    isAdvancedFilterOpen,
    selectedRecordId,
    selectedTab,
    selectedAttemptId,
    selectedReplayRunId,
    applyQueryToState,
    buildQueryFromState,
    syncRouteWithState,
    buildListParams: params,
    clearDetailSelection,
  };
}
