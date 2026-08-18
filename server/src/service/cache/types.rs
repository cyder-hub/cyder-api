use bincode::{Decode, Encode};
// Cache-specific types optimized for caching layer
// These structures contain only the fields needed for cache operations,
// reducing memory footprint and improving cache performance.

use crate::database::model_source_binding::ModelSourceBinding;
use crate::database::request_patch::RequestPatchVariantAggregate;
use crate::database::{api_key::ApiKey, api_key_acl_rule::ApiKeyAclRule};
use crate::schema::enum_def::{
    Action, ModelKind, ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement, RuleScope,
    UpstreamProfileType,
};
use serde::{Deserialize, Serialize, de};
use serde_with::serde_as;
use std::sync::Arc;

/// Represents an entry in the cache, which can either be a value (Positive)
/// or a marker indicating the value does not exist (Negative).
#[serde_as]
#[derive(PartialEq, Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum CacheEntry<T: Clone + Serialize + de::DeserializeOwned> {
    Positive(#[serde_as(as = "Arc<serde_with::Same>")] Arc<T>),
    Negative,
}

/// Unified API key cache snapshot used by request admission.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheApiKey {
    pub id: i64,
    pub api_key_hash: String,
    pub key_prefix: String,
    pub key_last4: String,
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
    pub acl_rules: Vec<CacheApiKeyAclRule>,
}

/// Source binding frozen into the model cache snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheModelSourceBinding {
    pub source_id: i64,
    pub is_default: bool,
}

/// Cached model with only fields needed by selection and execution.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheModel {
    pub id: i64,
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub model_kind: ModelKind,
    pub cost_catalog_id: Option<i64>,
    pub source_selection_mode: String,
    pub source_bindings: Vec<CacheModelSourceBinding>,
    pub is_enabled: bool,
}

/// Cached provider with only essential fields
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheProvider {
    pub id: i64,
    pub provider_key: String,
    pub name: String,
    pub provider_api_key_mode: ProviderApiKeyMode,
    pub is_enabled: bool,
    pub upstream_sources: Vec<CacheUpstreamSource>,
}

/// Immutable execution entry nested under a cached logical provider.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheUpstreamSource {
    pub id: i64,
    pub profile_type: UpstreamProfileType,
    pub base_url: String,
    pub use_proxy: bool,
    pub chat_completions_enabled: Option<bool>,
    pub chat_completions_path_override: Option<String>,
    pub embeddings_enabled: Option<bool>,
    pub embeddings_path_override: Option<String>,
    pub rerank_enabled: Option<bool>,
    pub rerank_path_override: Option<String>,
    pub is_enabled: bool,
    pub is_default: bool,
}

impl From<crate::database::upstream_source::UpstreamSource> for CacheUpstreamSource {
    fn from(source: crate::database::upstream_source::UpstreamSource) -> Self {
        Self {
            id: source.id,
            profile_type: source.profile_type,
            base_url: source.base_url,
            use_proxy: source.use_proxy,
            chat_completions_enabled: source.chat_completions_enabled,
            chat_completions_path_override: source.chat_completions_path_override,
            embeddings_enabled: source.embeddings_enabled,
            embeddings_path_override: source.embeddings_path_override,
            rerank_enabled: source.rerank_enabled,
            rerank_path_override: source.rerank_path_override,
            is_enabled: source.is_enabled,
            is_default: source.is_default,
        }
    }
}

/// Immutable Source-bound Variant snapshot carried by the shared catalog.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheRequestPatchVariant {
    pub id: i64,
    pub source_id: i64,
    pub model_id: Option<i64>,
    pub suffix: Option<String>,
    pub enabled: bool,
    pub expose_in_models: bool,
    pub rules: Vec<CacheRequestPatchRule>,
}

/// Cached aggregate catalog used by `/models` style listing endpoints
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheModelsCatalog {
    pub providers: Vec<CacheProvider>,
    pub models: Vec<CacheModel>,
    pub request_patch_variants: Vec<CacheRequestPatchVariant>,
}

/// Cached provider API key selection. Secret material remains encrypted at rest and in cache.
#[derive(Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheProviderKey {
    pub id: i64,
    pub provider_id: i64,
    pub secret_ciphertext: Vec<u8>,
    pub secret_nonce: Vec<u8>,
    pub secret_format_version: i32,
    pub secret_key_fingerprint: String,
}

impl std::fmt::Debug for CacheProviderKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "CacheProviderKey {{ id: {}, provider_id: {}, secret_material: <redacted> }}",
            self.id, self.provider_id
        )
    }
}

impl CacheProviderKey {
    pub fn encrypted_secret(
        &self,
    ) -> Result<
        crate::service::secret_encryption::EncryptedSecret,
        crate::service::secret_encryption::SecretEncryptionError,
    > {
        crate::service::secret_encryption::EncryptedSecret::from_parts(
            self.secret_ciphertext.clone(),
            self.secret_nonce.clone(),
            self.secret_format_version,
            self.secret_key_fingerprint.clone(),
        )
    }
}

/// Embedded ACL rule carried by `CacheApiKey`.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheApiKeyAclRule {
    pub id: i64,
    pub effect: Action,
    pub priority: i32,
    pub scope: RuleScope,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub is_enabled: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub enum RequestPatchVariantOrigin {
    SourceBase,
    ModelBase,
    SourceSuffix,
    ModelSuffix,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequestPatchSource {
    Variant {
        variant_id: i64,
        origin: RequestPatchVariantOrigin,
    },
}

impl RequestPatchSource {
    pub fn rule_id(&self) -> Option<i64> {
        match self {
            Self::Variant { .. } => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Variant { variant_id, origin } => {
                format!("request patch Variant {variant_id} ({origin:?})")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub enum RequestPatchExplainStatus {
    Effective,
    Overridden,
    Conflicted,
    Masked,
    Dormant,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheRequestPatchRule {
    pub id: i64,
    pub variant_id: i64,
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    pub value_json: Option<String>,
    pub description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheResolvedRequestPatch {
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    pub value_json: Option<String>,
    pub source_variant_id: i64,
    pub source_rule_id: i64,
    pub source_origin: RequestPatchVariantOrigin,
    pub overridden_rule_ids: Vec<i64>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct RuntimeResolvedRequestPatch {
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    pub value_json: Option<String>,
    pub source: RequestPatchSource,
    pub source_rule_id: Option<i64>,
    pub source_origin: Option<RequestPatchVariantOrigin>,
    pub overridden_rule_ids: Vec<i64>,
    pub overridden_sources: Vec<RequestPatchSource>,
    pub description: Option<String>,
}

impl RuntimeResolvedRequestPatch {
    pub fn source_label(&self) -> String {
        self.source.label()
    }
}

impl From<CacheResolvedRequestPatch> for RuntimeResolvedRequestPatch {
    fn from(rule: CacheResolvedRequestPatch) -> Self {
        let source = RequestPatchSource::Variant {
            variant_id: rule.source_variant_id,
            origin: rule.source_origin.clone(),
        };
        let overridden_sources = Vec::new();

        Self {
            placement: rule.placement,
            target: rule.target,
            operation: rule.operation,
            value_json: rule.value_json,
            source_rule_id: Some(rule.source_rule_id),
            source_origin: Some(rule.source_origin),
            source,
            overridden_rule_ids: rule.overridden_rule_ids,
            overridden_sources,
            description: rule.description,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct RuntimeRequestPatchConflict {
    pub placement: RequestPatchPlacement,
    pub lower_priority_source: RequestPatchSource,
    pub higher_priority_source: RequestPatchSource,
    pub lower_priority_target: String,
    pub higher_priority_target: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheRequestPatchConflict {
    pub lower_priority_variant_id: i64,
    pub higher_priority_variant_id: i64,
    pub lower_priority_origin: RequestPatchVariantOrigin,
    pub higher_priority_origin: RequestPatchVariantOrigin,
    pub placement: RequestPatchPlacement,
    pub lower_priority_target: String,
    pub higher_priority_target: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheRequestPatchExplainEntry {
    pub rule: CacheRequestPatchRule,
    pub origin: RequestPatchVariantOrigin,
    pub status: RequestPatchExplainStatus,
    pub effective_rule_id: Option<i64>,
    pub conflict_with_rule_ids: Vec<i64>,
    pub message: Option<String>,
}

impl From<RequestPatchVariantAggregate> for CacheRequestPatchVariant {
    fn from(aggregate: RequestPatchVariantAggregate) -> Self {
        Self {
            id: aggregate.variant.id,
            source_id: aggregate.variant.source_id,
            model_id: aggregate.variant.model_id,
            suffix: aggregate.variant.suffix,
            enabled: aggregate.variant.enabled,
            expose_in_models: aggregate.variant.expose_in_models,
            rules: aggregate
                .rules
                .into_iter()
                .map(|rule| CacheRequestPatchRule {
                    id: rule.id,
                    variant_id: rule.variant_id,
                    placement: rule.placement,
                    target: rule.target,
                    operation: rule.operation,
                    value_json: rule.value_json,
                    description: rule.description,
                    created_at: rule.created_at,
                    updated_at: rule.updated_at,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheCostComponent {
    pub id: i64,
    pub catalog_version_id: i64,
    pub meter_key: String,
    pub charge_kind: String,
    pub unit_price_nanos: Option<i64>,
    pub flat_fee_nanos: Option<i64>,
    pub tier_config_json: Option<String>,
    pub match_attributes_json: Option<String>,
    pub priority: i32,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheCostCatalogVersion {
    pub id: i64,
    pub catalog_id: i64,
    pub version: String,
    pub currency: String,
    pub source: Option<String>,
    pub effective_from: i64,
    pub effective_until: Option<i64>,
    pub is_enabled: bool,
    pub components: Vec<CacheCostComponent>,
}

// Conversion implementations from database types to cache types

impl CacheApiKey {
    pub fn from_db(row: ApiKey, acl_rules: Vec<ApiKeyAclRule>) -> Self {
        Self {
            id: row.id,
            api_key_hash: row.api_key_hash,
            key_prefix: row.key_prefix,
            key_last4: row.key_last4,
            name: row.name,
            description: row.description,
            default_action: row.default_action,
            is_enabled: row.is_enabled,
            expires_at: row.expires_at,
            rate_limit_rpm: row.rate_limit_rpm,
            max_concurrent_requests: row.max_concurrent_requests,
            quota_daily_requests: row.quota_daily_requests,
            quota_daily_tokens: row.quota_daily_tokens,
            quota_monthly_tokens: row.quota_monthly_tokens,
            budget_daily_nanos: row.budget_daily_nanos,
            budget_daily_currency: row.budget_daily_currency,
            budget_monthly_nanos: row.budget_monthly_nanos,
            budget_monthly_currency: row.budget_monthly_currency,
            acl_rules: acl_rules.into_iter().map(Into::into).collect(),
        }
    }

    pub fn is_active_at(&self, now_ms: i64) -> bool {
        self.is_enabled && self.expires_at.is_none_or(|expires_at| expires_at > now_ms)
    }
}

impl CacheModel {
    pub fn from_db_with_bindings(
        db: crate::database::model::Model,
        source_bindings: Vec<ModelSourceBinding>,
    ) -> Self {
        Self {
            id: db.id,
            provider_id: db.provider_id,
            real_model_name: db.real_model_name,
            model_name: db.model_name,
            model_kind: db.model_kind,
            cost_catalog_id: db.cost_catalog_id,
            source_selection_mode: db.source_selection_mode,
            source_bindings: source_bindings
                .into_iter()
                .map(|binding| CacheModelSourceBinding {
                    source_id: binding.source_id,
                    is_default: binding.is_default,
                })
                .collect(),
            is_enabled: db.is_enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CacheModel, CacheModelSourceBinding};

    #[test]
    fn cache_model_roundtrip_preserves_source_selection_snapshot() {
        let model = CacheModel {
            id: 1,
            provider_id: 2,
            model_name: "model".to_string(),
            real_model_name: Some("real-model".to_string()),
            model_kind: crate::schema::enum_def::ModelKind::Chat,
            cost_catalog_id: None,
            source_selection_mode: "EXPLICIT".to_string(),
            source_bindings: vec![CacheModelSourceBinding {
                source_id: 3,
                is_default: true,
            }],
            is_enabled: true,
        };
        let encoded = bincode::encode_to_vec(&model, bincode::config::standard())
            .expect("cache model should encode");
        let (decoded, consumed) =
            bincode::decode_from_slice::<CacheModel, _>(&encoded, bincode::config::standard())
                .expect("cache model should decode");

        assert_eq!(decoded, model);
        assert_eq!(consumed, encoded.len());
    }
}

impl From<crate::database::provider::ProviderApiKeySelection> for CacheProviderKey {
    fn from(db: crate::database::provider::ProviderApiKeySelection) -> Self {
        Self {
            id: db.id,
            provider_id: db.provider_id,
            secret_ciphertext: db.secret_ciphertext,
            secret_nonce: db.secret_nonce,
            secret_format_version: db.secret_format_version,
            secret_key_fingerprint: db.secret_key_fingerprint,
        }
    }
}

impl From<crate::database::provider::ProviderAggregate> for CacheProvider {
    fn from(db: crate::database::provider::ProviderAggregate) -> Self {
        Self {
            id: db.id,
            provider_key: db.provider_key.clone(),
            name: db.name.clone(),
            provider_api_key_mode: db.provider_api_key_mode.clone(),
            is_enabled: db.is_enabled,
            upstream_sources: db
                .upstream_sources
                .into_iter()
                .map(CacheUpstreamSource::from)
                .collect(),
        }
    }
}

impl From<ApiKeyAclRule> for CacheApiKeyAclRule {
    fn from(db: ApiKeyAclRule) -> Self {
        Self {
            id: db.id,
            effect: db.effect,
            priority: db.priority,
            scope: db.scope,
            provider_id: db.provider_id,
            model_id: db.model_id,
            is_enabled: db.is_enabled,
            description: db.description,
        }
    }
}

impl From<crate::database::request_patch::RequestPatchRule> for CacheRequestPatchRule {
    fn from(db: crate::database::request_patch::RequestPatchRule) -> Self {
        Self {
            id: db.id,
            variant_id: db.variant_id,
            placement: db.placement,
            target: db.target,
            operation: db.operation,
            value_json: db.value_json,
            description: db.description,
            created_at: db.created_at,
            updated_at: db.updated_at,
        }
    }
}

impl From<crate::database::cost::CostComponent> for CacheCostComponent {
    fn from(db: crate::database::cost::CostComponent) -> Self {
        Self {
            id: db.id,
            catalog_version_id: db.catalog_version_id,
            meter_key: db.meter_key,
            charge_kind: db.charge_kind,
            unit_price_nanos: db.unit_price_nanos,
            flat_fee_nanos: db.flat_fee_nanos,
            tier_config_json: db.tier_config_json,
            match_attributes_json: db.match_attributes_json,
            priority: db.priority,
            description: db.description,
        }
    }
}

impl CacheCostCatalogVersion {
    pub fn from_db_with_components(
        version: crate::database::cost::CostCatalogVersion,
        components: Vec<crate::database::cost::CostComponent>,
    ) -> Self {
        Self {
            id: version.id,
            catalog_id: version.catalog_id,
            version: version.version,
            currency: version.currency,
            source: version.source,
            effective_from: version.effective_from,
            effective_until: version.effective_until,
            is_enabled: version.is_enabled,
            components: components.into_iter().map(Into::into).collect(),
        }
    }
}
