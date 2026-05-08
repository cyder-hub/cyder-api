import type { Component } from "vue";
import type { RecordListItem } from "../../services/types";
import type { RecordDetailTab } from "./composables/useRecordDetail";

export type BooleanFilter = "ALL" | "true" | "false";

export type RecordFilters = {
  api_key_id: number;
  provider_id: number;
  model_id: number;
  status: string;
  user_api_type: string;
  resolved_name_scope: string;
  final_error_code: string;
  has_retry: BooleanFilter;
  has_fallback: BooleanFilter;
  has_transform_diagnostics: BooleanFilter;
  latency_ms_min: string;
  latency_ms_max: string;
  total_tokens_min: string;
  total_tokens_max: string;
  estimated_cost_nanos_min: string;
  estimated_cost_nanos_max: string;
  start_time: string;
  end_time: string;
  search: string;
};

export type FilterOption = {
  value: string;
  label: string;
};

export type EnrichedRecordListItem = RecordListItem & {
  providerName: string;
  apiKeyName: string;
  displayRequestedModelName: string;
  attemptsDisplay: string;
  diagnosticsDisplay: string;
  firstRespTimeDisplay: string;
  totalRespTimeDisplay: string;
  tpsDisplay: string;
  costDisplay: string;
  request_at_formatted: string;
};

export type RecordStatusMeta = {
  icon: Component;
  className: string;
  label: string;
};

export type RecordWorkbenchDeepLink = {
  recordId: number | null;
  tab: RecordDetailTab;
  attemptId: number | null;
  replayRunId: number | null;
};
