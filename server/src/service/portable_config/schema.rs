use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;

use crate::{
    database::reasoning_config::ReasoningConfigMode,
    schema::enum_def::{
        Action, ProviderApiKeyMode, ProviderType, RequestPatchOperation, RequestPatchPlacement,
        RuleScope,
    },
};

pub const PORTABLE_SCHEMA_VERSION: &str = "cyder.portable.v1";
pub const PORTABLE_MODULE_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PortableModuleId {
    ProviderProfile,
    ApiKeys,
    CostCatalogs,
    CostBindings,
    Unknown(String),
}

impl PortableModuleId {
    pub fn from_wire(value: &str) -> Self {
        match value {
            "provider_profile" => Self::ProviderProfile,
            "api_keys" => Self::ApiKeys,
            "cost_catalogs" => Self::CostCatalogs,
            "cost_bindings" => Self::CostBindings,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::ProviderProfile => "provider_profile",
            Self::ApiKeys => "api_keys",
            Self::CostCatalogs => "cost_catalogs",
            Self::CostBindings => "cost_bindings",
            Self::Unknown(value) => value.as_str(),
        }
    }

    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

impl fmt::Display for PortableModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for PortableModuleId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PortableModuleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_wire(&value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PortableSubrangeId {
    ProviderCore,
    ProviderKeys,
    ProviderModels,
    ProviderRequestPatches,
    ProviderReasoningConfig,
    ApiKeyCore,
    ApiKeyAcl,
    ApiKeyModelOverride,
    CostCatalogCore,
    CostCatalogVersions,
    CostComponents,
    CostModelBindings,
    Unknown(String),
}

impl PortableSubrangeId {
    pub fn from_wire(value: &str) -> Self {
        match value {
            "provider_core" => Self::ProviderCore,
            "provider_keys" => Self::ProviderKeys,
            "provider_models" => Self::ProviderModels,
            "provider_request_patches" => Self::ProviderRequestPatches,
            "provider_reasoning_config" => Self::ProviderReasoningConfig,
            "api_key_core" => Self::ApiKeyCore,
            "api_key_acl" => Self::ApiKeyAcl,
            "api_key_model_override" => Self::ApiKeyModelOverride,
            "cost_catalog_core" => Self::CostCatalogCore,
            "cost_catalog_versions" => Self::CostCatalogVersions,
            "cost_components" => Self::CostComponents,
            "cost_model_bindings" => Self::CostModelBindings,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::ProviderCore => "provider_core",
            Self::ProviderKeys => "provider_keys",
            Self::ProviderModels => "provider_models",
            Self::ProviderRequestPatches => "provider_request_patches",
            Self::ProviderReasoningConfig => "provider_reasoning_config",
            Self::ApiKeyCore => "api_key_core",
            Self::ApiKeyAcl => "api_key_acl",
            Self::ApiKeyModelOverride => "api_key_model_override",
            Self::CostCatalogCore => "cost_catalog_core",
            Self::CostCatalogVersions => "cost_catalog_versions",
            Self::CostComponents => "cost_components",
            Self::CostModelBindings => "cost_model_bindings",
            Self::Unknown(value) => value.as_str(),
        }
    }
}

impl fmt::Display for PortableSubrangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for PortableSubrangeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PortableSubrangeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_wire(&value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileProtectionMode {
    Plaintext,
    PasswordEncrypted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    FailOnConflict,
    SkipExisting,
    OverwriteExisting,
}

impl Default for ConflictStrategy {
    fn default() -> Self {
        Self::FailOnConflict
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortableReferenceStatus {
    ResolvedInBundle,
    ResolvedInTarget,
    MissingDependency,
    Conflict,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableBundle {
    pub schema_version: String,
    pub exported_at: i64,
    pub cyder_version: String,
    pub modules: Vec<PortableBundleModule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableBundleModule {
    pub module_id: PortableModuleId,
    pub module_version: u32,
    #[serde(default)]
    pub subranges: Vec<PortableSubrangeId>,
    #[serde(default)]
    pub summary: PortableModuleSummary,
    #[serde(default)]
    pub items: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableModuleSummary {
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub create: u64,
    #[serde(default)]
    pub update: u64,
    #[serde(default)]
    pub skip: u64,
    #[serde(default)]
    pub blocked: u64,
    #[serde(default)]
    pub conflict: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PortableProviderProfileItems {
    #[serde(default)]
    pub providers: Vec<PortableProviderItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_patches: Vec<PortableProviderRequestPatchItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_configs: Vec<PortableProviderReasoningConfigItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableProviderItem {
    pub provider_key: String,
    pub name: String,
    pub endpoint: String,
    pub use_proxy: bool,
    pub is_enabled: bool,
    pub provider_type: ProviderType,
    pub provider_api_key_mode: ProviderApiKeyMode,
    #[serde(default)]
    pub keys: Vec<PortableProviderApiKeyItem>,
    #[serde(default)]
    pub models: Vec<PortableProviderModelItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableProviderApiKeyItem {
    pub description: Option<String>,
    pub is_enabled: bool,
    pub api_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableModelRef {
    pub provider_key: String,
    pub model_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableProviderModelItem {
    pub provider_ref: String,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub supports_reasoning: bool,
    pub supports_image_input: bool,
    pub supports_embeddings: bool,
    pub supports_rerank: bool,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableProviderOwnerRef {
    pub scope: RuleScope,
    pub provider_ref: Option<String>,
    pub model_ref: Option<PortableModelRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableProviderRequestPatchItem {
    pub owner: PortableProviderOwnerRef,
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    pub value_json: Option<Value>,
    pub description: Option<String>,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableReasoningConfigPresetItem {
    pub preset_key: String,
    pub expose_in_models: bool,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableProviderReasoningConfigItem {
    pub owner: PortableProviderOwnerRef,
    pub mode: ReasoningConfigMode,
    pub family_key: Option<String>,
    #[serde(default)]
    pub presets: Vec<PortableReasoningConfigPresetItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableApiKeyItem {
    pub name: String,
    pub description: Option<String>,
    pub default_action: Action,
    pub is_enabled: bool,
    pub expires_at: Option<i64>,
    pub rate_limit_rpm: Option<i32>,
    pub max_concurrent_requests: Option<i32>,
    pub quota_daily_requests: Option<i64>,
    pub quota_daily_tokens: Option<i64>,
    pub quota_monthly_tokens: Option<i64>,
    pub budget_daily_nanos: Option<i64>,
    pub budget_daily_currency: Option<String>,
    pub budget_monthly_nanos: Option<i64>,
    pub budget_monthly_currency: Option<String>,
    pub api_key: String,
    #[serde(default)]
    pub acl_rules: Vec<PortableApiKeyAclRuleItem>,
    #[serde(default)]
    pub model_overrides: Vec<PortableApiKeyModelOverrideItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableApiKeyAclRuleItem {
    pub effect: Action,
    pub scope: RuleScope,
    pub provider_ref: Option<String>,
    pub model_ref: Option<PortableModelRef>,
    pub priority: i32,
    pub is_enabled: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableApiKeyModelOverrideItem {
    pub source_name: String,
    pub target_route_ref: String,
    pub description: Option<String>,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PortableCostCatalogItems {
    #[serde(default)]
    pub catalogs: Vec<PortableCostCatalogItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableCostCatalogItem {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub versions: Vec<PortableCostCatalogVersionItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableCostCatalogVersionItem {
    pub catalog_ref: String,
    pub version: String,
    pub currency: String,
    pub source: Option<String>,
    pub effective_from: i64,
    pub effective_until: Option<i64>,
    pub is_enabled: bool,
    pub is_archived: bool,
    #[serde(default)]
    pub components: Vec<PortableCostComponentItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableCostComponentItem {
    pub meter_key: String,
    pub charge_kind: String,
    pub unit_price_nanos: Option<i64>,
    pub flat_fee_nanos: Option<i64>,
    pub tier_config_json: Option<Value>,
    pub match_attributes_json: Option<Value>,
    pub priority: i32,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableCostBindingItem {
    pub target_kind: String,
    pub model_ref: Option<PortableModelRef>,
    pub provider_ref: Option<String>,
    pub cost_catalog_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableBlockedItem {
    pub code: String,
    pub message: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub module_id: Option<PortableModuleId>,
    pub subrange_id: Option<PortableSubrangeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableDependencyStatus {
    pub module_id: PortableModuleId,
    pub status: PortableReferenceStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableFileProtectionStatus {
    pub mode: FileProtectionMode,
    pub requires_password: bool,
    pub decrypted: bool,
    pub integrity_checked: bool,
    pub integrity_valid: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortablePreviewModule {
    pub module_id: PortableModuleId,
    pub module_version: u32,
    pub label: String,
    pub supported: bool,
    pub available: bool,
    pub selected_by_default: bool,
    pub contains_secrets: bool,
    pub deferred: bool,
    pub dependencies: Vec<PortableDependencyStatus>,
    pub subranges: Vec<PortableSubrangeId>,
    pub summary: PortableModuleSummary,
    pub warnings: Vec<String>,
    pub blocking_issues: Vec<PortableBlockedItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortablePreviewResponse {
    pub schema_version: String,
    pub exported_at: i64,
    pub cyder_version: String,
    pub bundle_digest: String,
    pub file_protection: PortableFileProtectionStatus,
    pub modules: Vec<PortablePreviewModule>,
    pub default_selected_modules: Vec<PortableModuleId>,
    pub unsupported_modules: Vec<PortableModuleId>,
    #[serde(default)]
    pub blocking_issues: Vec<PortableBlockedItem>,
    pub excluded_data_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableModuleSelection {
    pub module_id: PortableModuleId,
    #[serde(default)]
    pub subranges: Vec<PortableSubrangeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableExportRequest {
    #[serde(default)]
    pub selected_modules: Vec<PortableModuleSelection>,
    pub file_protection: FileProtectionMode,
    pub password: Option<String>,
    #[serde(default)]
    pub auto_generate_password: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableExportResponse {
    pub filename: String,
    pub content: String,
    pub file_protection: FileProtectionMode,
    pub generated_password: Option<String>,
    pub bundle_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableImportPreviewRequest {
    pub content: String,
    pub password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableDangerousPatchConfirmation {
    pub path: String,
    pub target: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableApplyRequest {
    pub content: String,
    pub password: Option<String>,
    pub bundle_digest: String,
    #[serde(default)]
    pub selected_modules: Vec<PortableModuleSelection>,
    #[serde(default)]
    pub conflict_strategy: ConflictStrategy,
    pub reason: String,
    #[serde(default)]
    pub dangerous_patch_confirmations: Vec<PortableDangerousPatchConfirmation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortableApplyModuleStatus {
    Applied,
    Skipped,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableApplyModuleResult {
    pub module_id: PortableModuleId,
    pub status: PortableApplyModuleStatus,
    pub summary: PortableModuleSummary,
    pub messages: Vec<String>,
    pub blocking_issues: Vec<PortableBlockedItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableApplyResult {
    pub bundle_digest: String,
    pub conflict_strategy: ConflictStrategy,
    pub modules: Vec<PortableApplyModuleResult>,
    pub summary: PortableModuleSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPortableBundle {
    pub bundle: PortableBundle,
    pub unsupported_modules: Vec<PortableModuleId>,
}

#[derive(Debug, Error)]
pub enum PortableBundleError {
    #[error("failed to parse portable bundle JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported portable schema version `{actual}`, expected `{expected}`")]
    UnsupportedSchemaVersion { expected: String, actual: String },
    #[error("duplicate portable module `{module_id}`")]
    DuplicateModuleId { module_id: String },
}

pub fn parse_portable_bundle_str(input: &str) -> Result<ParsedPortableBundle, PortableBundleError> {
    let bundle = serde_json::from_str::<PortableBundle>(input)?;
    validate_portable_bundle(bundle)
}

pub fn validate_portable_bundle(
    bundle: PortableBundle,
) -> Result<ParsedPortableBundle, PortableBundleError> {
    if bundle.schema_version != PORTABLE_SCHEMA_VERSION {
        return Err(PortableBundleError::UnsupportedSchemaVersion {
            expected: PORTABLE_SCHEMA_VERSION.to_string(),
            actual: bundle.schema_version,
        });
    }

    let mut seen = BTreeSet::<String>::new();
    let mut unsupported_modules = Vec::new();

    for module in &bundle.modules {
        let module_id = module.module_id.as_str().to_string();
        if !seen.insert(module_id.clone()) {
            return Err(PortableBundleError::DuplicateModuleId { module_id });
        }
        if !module.module_id.is_known() {
            unsupported_modules.push(module.module_id.clone());
        }
    }

    Ok(ParsedPortableBundle {
        bundle,
        unsupported_modules,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        PORTABLE_SCHEMA_VERSION, PortableBundleError, PortableModuleId, parse_portable_bundle_str,
    };

    #[test]
    fn unknown_schema_version_blocks_whole_bundle() {
        let raw = json!({
            "schema_version": "cyder.portable.v2",
            "exported_at": 1778236800000_i64,
            "cyder_version": "1.0.0",
            "modules": []
        })
        .to_string();

        let err = parse_portable_bundle_str(&raw).expect_err("schema v2 must be rejected");

        assert!(matches!(
            err,
            PortableBundleError::UnsupportedSchemaVersion { actual, .. }
                if actual == "cyder.portable.v2"
        ));
    }

    #[test]
    fn duplicate_module_id_blocks_whole_bundle() {
        let raw = json!({
            "schema_version": PORTABLE_SCHEMA_VERSION,
            "exported_at": 1778236800000_i64,
            "cyder_version": "1.0.0",
            "modules": [
                {
                    "module_id": "provider_profile",
                    "module_version": 1,
                    "summary": {},
                    "items": {}
                },
                {
                    "module_id": "provider_profile",
                    "module_version": 1,
                    "summary": {},
                    "items": {}
                }
            ]
        })
        .to_string();

        let err = parse_portable_bundle_str(&raw).expect_err("duplicate module must be rejected");

        assert!(matches!(
            err,
            PortableBundleError::DuplicateModuleId { module_id }
                if module_id == "provider_profile"
        ));
    }

    #[test]
    fn unknown_v1_module_and_unknown_fields_are_tolerated() {
        let raw = json!({
            "schema_version": PORTABLE_SCHEMA_VERSION,
            "exported_at": 1778236800000_i64,
            "cyder_version": "1.0.0",
            "future_top_level": true,
            "modules": [
                {
                    "module_id": "provider_profile",
                    "module_version": 1,
                    "future_module_field": "ignored",
                    "subranges": ["provider_core", "provider_keys"],
                    "summary": {},
                    "items": {
                        "providers": [],
                        "future_items_field": true
                    }
                },
                {
                    "module_id": "future_module",
                    "module_version": 1,
                    "summary": {},
                    "items": {
                        "opaque": true
                    }
                }
            ]
        })
        .to_string();

        let parsed = parse_portable_bundle_str(&raw).expect("v1 forward-compatible bundle");

        assert_eq!(parsed.bundle.modules.len(), 2);
        assert_eq!(
            parsed.bundle.modules[0].module_id,
            PortableModuleId::ProviderProfile
        );
        assert_eq!(
            parsed.unsupported_modules,
            vec![PortableModuleId::Unknown("future_module".to_string())]
        );
    }
}
