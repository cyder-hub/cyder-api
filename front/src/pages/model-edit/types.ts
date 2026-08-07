import type { RequestPatchRule } from "@/services/types";
import type { ModelSourceConfigSummary } from "@/services/types";

export interface EditingModelData {
  id: number;
  provider_id: number;
  cost_catalog_id: number | null;
  model_name: string;
  real_model_name: string;
  is_enabled: boolean;
  request_patches: RequestPatchRule[];
  source_config?: ModelSourceConfigSummary;
}
