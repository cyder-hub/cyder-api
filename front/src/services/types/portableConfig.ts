import type { JsonValue } from "./shared";

export const PORTABLE_SCHEMA_VERSION = "cyder.portable.v1";

export type KnownPortableModuleId =
  | "provider_profile"
  | "api_keys"
  | "cost_catalogs"
  | "cost_bindings";

export type PortableModuleId = KnownPortableModuleId | (string & {});

export type KnownPortableSubrangeId =
  | "provider_core"
  | "provider_keys"
  | "provider_models"
  | "provider_request_patches"
  | "provider_reasoning_config"
  | "api_key_core"
  | "api_key_acl"
  | "api_key_model_override"
  | "cost_catalog_core"
  | "cost_catalog_versions"
  | "cost_components"
  | "cost_model_bindings";

export type PortableSubrangeId = KnownPortableSubrangeId | (string & {});

export type FileProtectionMode = "plaintext" | "password_encrypted";

export type ConflictStrategy =
  | "fail_on_conflict"
  | "skip_existing"
  | "overwrite_existing";

export type PortableReferenceStatus =
  | "resolved_in_bundle"
  | "resolved_in_target"
  | "missing_dependency"
  | "conflict"
  | "blocked";

export type PortableApplyModuleStatus =
  | "applied"
  | "skipped"
  | "blocked"
  | "failed";

export interface PortableModuleSummary {
  total: number;
  create: number;
  update: number;
  skip: number;
  blocked: number;
  conflict: number;
}

export interface PortableBundleModule {
  module_id: PortableModuleId;
  module_version: number;
  subranges?: PortableSubrangeId[];
  summary?: Partial<PortableModuleSummary>;
  items?: JsonValue;
  [key: string]: JsonValue | PortableModuleId | PortableSubrangeId[] | Partial<PortableModuleSummary> | undefined;
}

export interface PortableBundle {
  schema_version: typeof PORTABLE_SCHEMA_VERSION;
  exported_at: number;
  cyder_version: string;
  modules: PortableBundleModule[];
  [key: string]: JsonValue | PortableBundleModule[] | undefined;
}

export interface PortableCostCatalogItems {
  catalogs: PortableCostCatalogItem[];
}

export interface PortableCostCatalogItem {
  name: string;
  description?: string | null;
  versions: PortableCostCatalogVersionItem[];
}

export interface PortableCostCatalogVersionItem {
  catalog_ref: string;
  version: string;
  currency: string;
  source?: string | null;
  effective_from: number;
  effective_until?: number | null;
  is_enabled: boolean;
  is_archived: boolean;
  components: PortableCostComponentItem[];
}

export interface PortableCostComponentItem {
  meter_key: string;
  charge_kind: string;
  unit_price_nanos?: number | null;
  flat_fee_nanos?: number | null;
  tier_config_json?: JsonValue | null;
  match_attributes_json?: JsonValue | null;
  priority: number;
  description?: string | null;
}

export interface PortableCostBindingItem {
  target_kind: string;
  model_ref?: {
    provider_key: string;
    model_name: string;
  } | null;
  provider_ref?: string | null;
  cost_catalog_ref: string;
}

export interface PortableModuleDependency {
  module_id: PortableModuleId;
  required_for_export: boolean;
  required_for_fresh_import: boolean;
  reason: string;
}

export interface PortableSubrangeRegistryItem {
  subrange_id: PortableSubrangeId;
  label: string;
  default_selected: boolean;
  required: boolean;
  contains_secrets: boolean;
  deferred: boolean;
  deferred_reason: string | null;
}

export interface PortableModuleRegistryItem {
  module_id: PortableModuleId;
  label: string;
  description: string;
  module_version: number;
  default_selected: boolean;
  contains_secrets: boolean;
  deferred: boolean;
  deferred_reason: string | null;
  dependencies: PortableModuleDependency[];
  subranges: PortableSubrangeRegistryItem[];
  conflict_strategies: ConflictStrategy[];
}

export interface PortableModuleRegistryResponse {
  schema_version: typeof PORTABLE_SCHEMA_VERSION;
  modules: PortableModuleRegistryItem[];
  default_selected_modules: PortableModuleId[];
  apply_order: PortableModuleId[];
}

export interface PortableBlockedItem {
  code: string;
  message: string;
  path: string;
  target?: string | null;
  module_id: PortableModuleId | null;
  subrange_id: PortableSubrangeId | null;
}

export interface PortableDependencyStatus {
  module_id: PortableModuleId;
  status: PortableReferenceStatus;
  message: string | null;
}

export interface PortableFileProtectionStatus {
  mode: FileProtectionMode;
  requires_password: boolean;
  decrypted: boolean;
  integrity_checked: boolean;
  integrity_valid: boolean | null;
}

export interface PortablePreviewModule {
  module_id: PortableModuleId;
  module_version: number;
  label: string;
  supported: boolean;
  available: boolean;
  selected_by_default: boolean;
  contains_secrets: boolean;
  deferred: boolean;
  dependencies: PortableDependencyStatus[];
  subranges: PortableSubrangeId[];
  summary: PortableModuleSummary;
  warnings: string[];
  blocking_issues: PortableBlockedItem[];
}

export interface PortablePreviewResponse {
  schema_version: typeof PORTABLE_SCHEMA_VERSION;
  exported_at: number;
  cyder_version: string;
  bundle_digest: string;
  file_protection: PortableFileProtectionStatus;
  modules: PortablePreviewModule[];
  default_selected_modules: PortableModuleId[];
  unsupported_modules: PortableModuleId[];
  blocking_issues: PortableBlockedItem[];
  excluded_data_types: string[];
}

export interface PortableModuleSelection {
  module_id: PortableModuleId;
  subranges: PortableSubrangeId[];
}

export interface PortableDangerousPatchConfirmation {
  path: string;
  target: string;
  confirmed: boolean;
}

export interface PortableExportRequest {
  selected_modules: PortableModuleSelection[];
  file_protection: FileProtectionMode;
  password?: string | null;
  auto_generate_password?: boolean;
}

export interface PortableExportResponse {
  filename: string;
  content: string;
  file_protection: FileProtectionMode;
  generated_password: string | null;
  bundle_digest: string;
}

export interface PortableImportPreviewRequest {
  content: string;
  password?: string | null;
}

export interface PortableApplyRequest {
  content: string;
  password?: string | null;
  bundle_digest: string;
  selected_modules: PortableModuleSelection[];
  conflict_strategy: ConflictStrategy;
  reason: string;
  dangerous_patch_confirmations: PortableDangerousPatchConfirmation[];
}

export interface PortableApplyModuleResult {
  module_id: PortableModuleId;
  status: PortableApplyModuleStatus;
  summary: PortableModuleSummary;
  messages: string[];
  blocking_issues: PortableBlockedItem[];
}

export interface PortableApplyResult {
  bundle_digest: string;
  conflict_strategy: ConflictStrategy;
  modules: PortableApplyModuleResult[];
  summary: PortableModuleSummary;
}
