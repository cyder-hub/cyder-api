export type RuntimeFeatureScopeKind = "provider" | "model" | string;

export type RuntimeFeatureEffectiveSource =
  | "default_false"
  | "provider_default"
  | "model_override"
  | string;

export interface RuntimeFeatureCatalogItem {
  feature_key: string;
  label: string;
  description: string;
  default_enabled: boolean;
  supported_scope_kinds: RuntimeFeatureScopeKind[];
}

export interface RuntimeFeatureCatalog {
  features: RuntimeFeatureCatalogItem[];
}

export interface RuntimeFeatureConfigView {
  id: number;
  scope_kind: RuntimeFeatureScopeKind;
  provider_id: number | null;
  model_id: number | null;
  feature_key: string;
  enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface RuntimeFeatureConfigFeature {
  feature_key: string;
  owner_config: RuntimeFeatureConfigView | null;
  provider_config: RuntimeFeatureConfigView | null;
  effective_enabled: boolean;
  effective_source: RuntimeFeatureEffectiveSource;
}

export interface RuntimeFeatureConfigResponse {
  owner_kind: RuntimeFeatureScopeKind;
  owner_id: number;
  features: RuntimeFeatureConfigFeature[];
}

export interface RuntimeFeatureConfigPayload {
  enabled: boolean;
}
