import type { Component } from "vue";
import type { DownstreamProtocol, RecordListItem } from "../../services/types";
import type { RecordDetailTab } from "./composables/useRecordDetail";

export type RecordFilters = {
  api_key_id: number;
  provider_id: number;
  model_id: number;
  status: string;
  downstream_protocol: "ALL" | DownstreamProtocol;
  final_error_code: string;
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
  httpStatusDisplay: string;
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
};
