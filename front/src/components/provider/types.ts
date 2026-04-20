import type { CustomFieldItem } from "@/store/types";

export interface LocalProviderApiKeyItem {
  id: number | null;
  api_key: string;
  description: string | null;
  isEditing: boolean;
  checkStatus: "unchecked" | "checking" | "success" | "error";
  checkMessage?: string;
}

export interface LocalEditableModelItem {
  id: number | null;
  model_name: string;
  real_model_name: string | null;
  is_enabled: boolean;
  isEditing: boolean;
  checkStatus: "unchecked" | "checking" | "success" | "error";
  checkMessage?: string;
}

export interface EditingProviderData {
  id: number | null;
  name: string;
  provider_key: string;
  provider_type: string;
  endpoint: string;
  use_proxy: boolean;
  models: LocalEditableModelItem[];
  provider_keys: LocalProviderApiKeyItem[];
  custom_fields: CustomFieldItem[];
}
