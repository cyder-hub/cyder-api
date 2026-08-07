import type { JsonValue } from "./shared";
import type { ModelDetail, ModelItem } from "./models";
import type { RequestPatchRule } from "./requestPatch";

// ========== Provider Types ==========
export interface ProviderBase {
  id: number;
  provider_key: string;
  name: string;
  is_enabled: boolean;
  deleted_at: number | null;
  created_at: number;
  updated_at: number;
  provider_api_key_mode: string;
  upstream_sources: UpstreamSource[];
}

export interface UpstreamSource {
  id: number;
  provider_id: number;
  profile_type: string;
  endpoint: string;
  use_proxy: boolean;
  is_enabled: boolean;
  is_default: boolean;
  deleted_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface UpstreamSourcePayload {
  profile_type: string;
  endpoint: string;
  use_proxy: boolean;
  is_enabled: boolean;
  is_default: boolean;
}

export interface UpstreamSourceUpdatePayload {
  endpoint?: string;
  use_proxy?: boolean;
  is_enabled?: boolean;
  is_default?: boolean;
}

export interface ProviderSummaryItem {
  id: number;
  provider_key: string;
  name: string;
  is_enabled: boolean;
  source_count: number;
  enabled_source_count: number;
  default_source_id: number | null;
  default_source_profile_type: string | null;
}

export interface ProviderApiKeySummary {
  id: number;
  provider_id: number;
  description: string | null;
  key_prefix: string;
  key_last4: string;
  is_enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface ProviderApiKeyReveal extends ProviderApiKeySummary {
  api_key: string;
}

export interface ProviderListItem {
  provider: ProviderBase;
  models: ModelDetail[];
  provider_keys: ProviderApiKeySummary[];
  request_patches: RequestPatchRule[];
}


// ========== Provider CRUD Payloads ==========
export interface ProviderRemoteModelItem {
  [key: string]: JsonValue | undefined;
  id?: string;
  name?: string;
  owned_by?: string;
}

export type ProviderRemoteModelsPayload =
  | ProviderRemoteModelItem[]
  | {
      data?: ProviderRemoteModelItem[];
      models?: ProviderRemoteModelItem[];
    };

export interface ProviderRemoteModelsResponse {
  source_id: number;
  profile_type: string;
  models: ProviderRemoteModelsPayload;
}

export interface ProviderCheckPayload {
  model_id?: number;
  model_name?: string;
  provider_api_key_id?: number;
  provider_api_key?: string;
}

export interface ProviderCheckResponse {
  source_id: number;
  profile_type: string;
}

export interface ProviderBootstrapPayload {
  initial_source: UpstreamSourcePayload;
  api_key: string;
  model_name: string;
  key: string;
  name?: string;
  real_model_name?: string | null;
  save_and_test?: boolean;
  api_key_description?: string | null;
}

export interface ProviderBootstrapResponse {
  provider?: ProviderBase;
  created_key?: ProviderApiKeySummary | null;
  created_model?: ModelItem | null;
  provider_name?: string | null;
  provider_key?: string | null;
  check_result?: unknown;
}

export interface ProviderCreatePayload {
  key: string;
  name: string;
  is_enabled?: boolean;
  provider_api_key_mode?: string;
  initial_source?: UpstreamSourcePayload;
}

export interface ProviderUpdatePayload {
  name: string;
  is_enabled?: boolean;
  provider_api_key_mode?: string;
}

export interface ProviderKeyPayload {
  api_key: string;
  description?: string | null;
}

export interface ProviderKeyUpdatePayload {
  description: string | null;
  is_enabled: boolean;
}

export interface ProviderKeyReplacePayload {
  api_key: string;
}
