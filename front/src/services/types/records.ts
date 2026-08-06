// Request-level gateway log types. The 1.0 cleanup baseline only retains summary
// fields needed for operations and cost visibility.
export type DownstreamProtocol = "OPENAI" | "RESPONSES" | "ANTHROPIC" | "GEMINI";
export type UpstreamProtocol =
  | "OPENAI"
  | "RESPONSES"
  | "ANTHROPIC"
  | "GEMINI"
  | "OLLAMA";

export interface RecordListItem {
  id: number;
  request_id: string;
  client_request_id: string | null;
  api_key_id: number;
  requested_model_name?: string | null;
  base_requested_model_name?: string | null;
  resolved_reasoning_suffix?: string | null;
  resolved_reasoning_preset?: string | null;
  overall_status: string;
  request_received_at: number;
  upstream_request_sent_at: number | null;
  first_response_body_at: number | null;
  first_token_at: number | null;
  completed_at: number | null;
  is_stream: boolean;
  provider_id: number | null;
  provider_name: string | null;
  model_id: number | null;
  model_name: string | null;
  real_model_name: string | null;
  source_id: number | null;
  source_key: string | null;
  source_profile_type: string | null;
  upstream_http_status: number | null;
  estimated_cost_nanos: number | null;
  estimated_cost_currency: string | null;
  total_input_tokens: number | null;
  total_output_tokens: number | null;
  output_text_tokens: number | null;
  reasoning_tokens: number | null;
  total_tokens: number | null;
}

export interface RecordRequest extends RecordListItem {
  downstream_protocol: DownstreamProtocol;
  final_error_code: string | null;
  final_error_message: string | null;
  client_ip: string | null;
  upstream_response_headers_at: number | null;
  upstream_first_body_chunk_at: number | null;
  max_upstream_response_idle_ms: number | null;
  provider_api_key_id: number | null;
  provider_key: string | null;
  upstream_protocol: UpstreamProtocol | null;
  source_endpoint: string | null;
  cost_catalog_id: number | null;
  cost_catalog_version_id: number | null;
  cost_snapshot_json: string | null;
  input_text_tokens: number | null;
  input_image_tokens: number | null;
  output_image_tokens: number | null;
  cache_read_tokens: number | null;
  cache_write_tokens: number | null;
  created_at: number;
  updated_at: number;
}

export type RecordDetail = RecordRequest;

export interface RecordListParams {
  page?: number;
  page_size?: number;
  api_key_id?: number;
  provider_id?: number;
  source_id?: number;
  model_id?: number;
  status?: string;
  downstream_protocol?: DownstreamProtocol;
  final_error_code?: string;
  latency_ms_min?: number;
  latency_ms_max?: number;
  total_tokens_min?: number;
  total_tokens_max?: number;
  estimated_cost_nanos_min?: number;
  estimated_cost_nanos_max?: number;
  start_time?: number;
  end_time?: number;
  search?: string;
  [key: string]: string | number | boolean | undefined;
}
