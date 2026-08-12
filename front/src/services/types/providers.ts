import type { ModelDetail, ModelDetailModel, ModelKind } from "./models";
import type { RequestPatchVariantAggregate } from "./requestPatch";

export type UpstreamProfileType =
  | "OPENAI"
  | "OPENAI_COMPATIBLE"
  | "GEMINI_OPENAI"
  | "GEMINI"
  | "VERTEX"
  | "OLLAMA"
  | "ANTHROPIC"
  | "RESPONSES";

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

interface UpstreamSourceCommon {
  id: number;
  provider_id: number;
  base_url: string;
  base_url_is_default: boolean;
  use_proxy: boolean;
  is_enabled: boolean;
  is_default: boolean;
  deleted_at: number | null;
  created_at: number;
  updated_at: number;
}

export type UpstreamSource =
  | (UpstreamSourceCommon & {
      profile_type: "OPENAI" | "GEMINI_OPENAI";
      chat_completions_enabled: boolean;
      chat_completions_path_override: string | null;
      embeddings_enabled: boolean;
      embeddings_path_override: string | null;
      rerank_enabled?: never;
      rerank_path_override?: never;
    })
  | (UpstreamSourceCommon & {
      profile_type: "OPENAI_COMPATIBLE";
      chat_completions_enabled: boolean;
      chat_completions_path_override: string | null;
      embeddings_enabled: boolean;
      embeddings_path_override: string | null;
      rerank_enabled: boolean;
      rerank_path_override: string | null;
    })
  | (UpstreamSourceCommon & {
      profile_type: "GEMINI" | "VERTEX" | "OLLAMA" | "ANTHROPIC" | "RESPONSES";
      chat_completions_enabled?: never;
      chat_completions_path_override?: never;
      embeddings_enabled?: never;
      embeddings_path_override?: never;
      rerank_enabled?: never;
      rerank_path_override?: never;
    });

interface SourceLifecyclePayload {
  use_proxy: boolean;
  is_enabled: boolean;
  is_default: boolean;
}

export type UpstreamSourcePayload =
  | (SourceLifecyclePayload & {
      profile_type: "OPENAI" | "GEMINI_OPENAI";
      base_url?: string | null;
      chat_completions_enabled?: boolean;
      chat_completions_path_override?: string;
      embeddings_enabled?: boolean;
      embeddings_path_override?: string;
    })
  | (SourceLifecyclePayload & {
      profile_type: "OPENAI_COMPATIBLE";
      base_url: string;
      chat_completions_enabled?: boolean;
      chat_completions_path_override?: string;
      embeddings_enabled?: boolean;
      embeddings_path_override?: string;
      rerank_enabled?: boolean;
      rerank_path_override?: string;
    })
  | (SourceLifecyclePayload & {
      profile_type: "GEMINI" | "VERTEX" | "OLLAMA" | "ANTHROPIC" | "RESPONSES";
      base_url: string;
    });

export interface UpstreamSourceUpdatePayload {
  base_url?: string | null;
  use_proxy?: boolean;
  chat_completions_enabled?: boolean;
  chat_completions_path_override?: string | null;
  embeddings_enabled?: boolean;
  embeddings_path_override?: string | null;
  rerank_enabled?: boolean;
  rerank_path_override?: string | null;
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
  request_patch_variants: RequestPatchVariantAggregate[];
}


// ========== Provider CRUD Payloads ==========
export type SourceImpactAction =
  | "DISABLE"
  | "DELETE"
  | "SET_DEFAULT"
  | "UNSET_DEFAULT";

export interface SourceImpactProtocolSummary {
  downstream_protocol: string;
  selection_changed_count: number;
  would_become_unselectable_count: number;
}

export interface SourceImpactReport {
  action: SourceImpactAction;
  provider_id: number;
  source_id: number;
  inherit_all_model_count: number;
  explicit_binding_model_count: number;
  explicit_default_model_count: number;
  protocols: SourceImpactProtocolSummary[];
}

export type ProviderCheckPayload = (
  | {
      model_id: number;
      draft_model?: never;
    }
  | {
      model_id?: never;
      draft_model: {
        model_kind: "CHAT";
        upstream_model_name: string;
      };
    }
) & {
  provider_api_key_id?: number;
  provider_api_key?: string;
};

export interface ProviderCheckResponse {
  source_id: number;
  profile_type: string;
  provider_api_key_id: number | null;
}

export interface ProviderBootstrapPayload {
  initial_source: UpstreamSourcePayload;
  api_key: string;
  model_name: string;
  model_kind: ModelKind;
  key: string;
  name?: string;
  real_model_name?: string | null;
  save_and_test?: boolean;
  api_key_description?: string | null;
}

export interface ProviderBootstrapResponse {
  provider?: ProviderBase;
  created_key?: ProviderApiKeySummary | null;
  // The bootstrap API returns the raw Model core row. Source Config is
  // hydrated from the returned Provider Sources by the provider editor.
  created_model?: ModelDetailModel | null;
  provider_name?: string | null;
  provider_key?: string | null;
  check_result?: ProviderBootstrapCheckResult | null;
}

export interface ProviderBootstrapCheckResult {
  status: "success" | "failed" | "check_skipped";
  message: string;
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
