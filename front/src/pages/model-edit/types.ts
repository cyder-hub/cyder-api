import type { RequestPatchRule } from "@/services/types";
import type { ModelCapabilityItem } from "@/pages/model/types";

export interface EditingModelData {
  id: number;
  provider_id: number;
  cost_catalog_id: number | null;
  model_name: string;
  real_model_name: string;
  supports_streaming: boolean;
  supports_tools: boolean;
  supports_reasoning: boolean;
  supports_image_input: boolean;
  supports_embeddings: boolean;
  supports_rerank: boolean;
  is_enabled: boolean;
  request_patches: RequestPatchRule[];
}

export type { ModelCapabilityItem };
