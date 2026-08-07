import type { RequestPatchRule } from "@/services/types";

export interface LocalProviderApiKeyItem {
  id: number;
  provider_id: number;
  description: string | null;
  key_prefix: string;
  key_last4: string;
  is_enabled: boolean;
  created_at: number;
  updated_at: number;
  checkStatus: "unchecked" | "checking" | "success" | "error";
  checkMessage?: string;
}

export interface LocalEditableModelItem {
  id: number | null;
  model_name: string;
  real_model_name: string | null;
  supports_streaming: boolean;
  supports_tools: boolean;
  supports_reasoning: boolean;
  supports_image_input: boolean;
  supports_embeddings: boolean;
  supports_rerank: boolean;
  is_enabled: boolean;
  isEditing: boolean;
  checkStatus: "unchecked" | "checking" | "success" | "error";
  checkMessage?: string;
}

export interface EditingProviderData {
  id: number | null;
  name: string;
  provider_key: string;
  is_enabled: boolean;
  provider_api_key_mode: string;
  upstream_sources: EditingProviderSource[];
  models: LocalEditableModelItem[];
  provider_keys: LocalProviderApiKeyItem[];
  request_patches: RequestPatchRule[];
}

export interface EditingProviderSource {
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
