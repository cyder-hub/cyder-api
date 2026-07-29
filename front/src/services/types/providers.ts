import type { JsonValue } from "./shared";
import type { ModelDetail, ModelItem } from "./models";
import type { RequestPatchRule } from "./requestPatch";

// ========== Provider Types ==========
export interface ProviderBase {
  id: number;
  provider_key: string;
  name: string;
  endpoint: string;
  use_proxy: boolean;
  provider_type: string;
}

export interface ProviderSummaryItem {
  id: number;
  provider_key: string;
  name: string;
  is_enabled: boolean;
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

export type ProviderRemoteModelsResponse =
  | ProviderRemoteModelItem[]
  | {
      data?: ProviderRemoteModelItem[];
      models?: ProviderRemoteModelItem[];
    };

export interface ProviderCheckPayload {
  model_id?: number;
  model_name?: string;
  provider_api_key_id?: number;
  provider_api_key?: string;
}

export interface ProviderBootstrapPayload {
  endpoint: string;
  api_key: string;
  model_name: string;
  key: string;
  provider_type?: string;
  name?: string;
  real_model_name?: string | null;
  use_proxy?: boolean;
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

export interface ProviderPayload {
  key: string;
  name: string;
  endpoint: string;
  use_proxy: boolean;
  provider_type: string;
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
