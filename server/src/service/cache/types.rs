use bincode::{Decode, Encode};
// Cache-specific types optimized for caching layer
// These structures contain only the fields needed for cache operations,
// reducing memory footprint and improving cache performance.

use crate::database::model_route::{ApiKeyModelOverride, ModelRouteDetail};
use crate::database::reasoning_profile::{
    ReasoningPatchFamily, ReasoningPreset, ReasoningProfileWithPresets,
};
use crate::database::{api_key::ApiKey, api_key_acl_rule::ApiKeyAclRule};
use crate::schema::enum_def::{
    Action, ProviderApiKeyMode, ProviderType, RequestPatchOperation, RequestPatchPlacement,
    RuleScope,
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

/// Cached model with only essential fields
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheModel {
    pub id: i64,
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub cost_catalog_id: Option<i64>,
    pub reasoning_profile_override_id: Option<i64>,
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub supports_reasoning: bool,
    pub supports_image_input: bool,
    pub supports_embeddings: bool,
    pub supports_rerank: bool,
    pub is_enabled: bool,
}

/// Cached provider with only essential fields
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheProvider {
    pub id: i64,
    pub provider_key: String,
    pub name: String,
    pub endpoint: String,
    pub use_proxy: bool,
    pub provider_type: ProviderType,
    pub provider_api_key_mode: ProviderApiKeyMode,
    pub default_reasoning_profile_id: Option<i64>,
    pub is_enabled: bool,
}

/// Cached enabled reasoning profile and enabled presets.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheReasoningProfile {
    pub id: i64,
    pub profile_key: String,
    pub name: String,
    pub description: Option<String>,
    pub family: ReasoningPatchFamily,
    pub is_enabled: bool,
    pub presets: Vec<CacheReasoningProfilePreset>,
}

/// Cached enabled preset metadata derived from the built-in preset key.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheReasoningProfilePreset {
    pub id: i64,
    pub profile_id: i64,
    pub preset: ReasoningPreset,
    pub suffix: String,
    pub requires_reasoning: bool,
    pub expose_in_models: bool,
    pub is_enabled: bool,
}

/// Cached model route candidate ordered by runtime priority.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheModelRouteCandidate {
    pub route_id: i64,
    pub model_id: i64,
    pub provider_id: i64,
    pub priority: i32,
    pub is_enabled: bool,
}

/// Cached logical model route used by request resolution.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheModelRoute {
    pub id: i64,
    pub route_name: String,
    pub description: Option<String>,
    pub is_enabled: bool,
    pub expose_in_models: bool,
    pub candidates: Vec<CacheModelRouteCandidate>,
}

/// Cached API key scoped name override definition.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheApiKeyModelOverride {
    pub id: i64,
    pub api_key_id: i64,
    pub source_name: String,
    pub target_route_id: i64,
    pub description: Option<String>,
    pub is_enabled: bool,
}

/// Cached aggregate catalog used by `/models` style listing endpoints
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheModelsCatalog {
    pub providers: Vec<CacheProvider>,
    pub models: Vec<CacheModel>,
    pub routes: Vec<CacheModelRoute>,
    pub api_key_overrides: Vec<CacheApiKeyModelOverride>,
    pub reasoning_profiles: Vec<CacheReasoningProfile>,
}

/// Cached provider API key
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct CacheProviderKey {
    pub id: i64,
    pub provider_id: i64,
    pub api_key: String,
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
pub enum RequestPatchRuleOrigin {
    ProviderDirect,
    ModelDirect,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequestPatchSource {
    ProviderRule {
        rule_id: i64,
    },
    ModelRule {
        rule_id: i64,
    },
    ReasoningPreset {
        profile_id: i64,
        profile_preset_id: i64,
        family: ReasoningPatchFamily,
        preset: ReasoningPreset,
        suffix: String,
    },
}

impl RequestPatchSource {
    pub fn rule_id(&self) -> Option<i64> {
        match self {
            Self::ProviderRule { rule_id } | Self::ModelRule { rule_id } => Some(*rule_id),
            Self::ReasoningPreset { .. } => None,
        }
    }

    pub fn legacy_origin(&self) -> Option<RequestPatchRuleOrigin> {
        match self {
            Self::ProviderRule { .. } => Some(RequestPatchRuleOrigin::ProviderDirect),
            Self::ModelRule { .. } => Some(RequestPatchRuleOrigin::ModelDirect),
            Self::ReasoningPreset { .. } => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::ProviderRule { rule_id } => format!("provider request patch rule {rule_id}"),
            Self::ModelRule { rule_id } => format!("model request patch rule {rule_id}"),
            Self::ReasoningPreset {
                profile_id,
                profile_preset_id,
                family,
                preset,
                suffix,
            } => format!(
                "reasoning preset patch profile={profile_id} preset_row={profile_preset_id} family={family} preset={preset} suffix={suffix}"
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub enum RequestPatchExplainStatus {
    Effective,
    Overridden,
    Conflicted,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheRequestPatchRule {
    pub id: i64,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    pub value_json: Option<String>,
    pub description: Option<String>,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheInheritedRequestPatch {
    pub rule: CacheRequestPatchRule,
    pub overridden_by_rule_id: Option<i64>,
    pub conflict_with_rule_ids: Vec<i64>,
    pub is_effective: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheResolvedRequestPatch {
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    pub value_json: Option<String>,
    pub source_rule_id: i64,
    pub source_origin: RequestPatchRuleOrigin,
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
    pub source_origin: Option<RequestPatchRuleOrigin>,
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
        let source = match rule.source_origin {
            RequestPatchRuleOrigin::ProviderDirect => RequestPatchSource::ProviderRule {
                rule_id: rule.source_rule_id,
            },
            RequestPatchRuleOrigin::ModelDirect => RequestPatchSource::ModelRule {
                rule_id: rule.source_rule_id,
            },
        };
        let overridden_sources = match rule.source_origin {
            RequestPatchRuleOrigin::ProviderDirect => Vec::new(),
            RequestPatchRuleOrigin::ModelDirect => rule
                .overridden_rule_ids
                .iter()
                .map(|rule_id| RequestPatchSource::ProviderRule { rule_id: *rule_id })
                .collect(),
        };

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
    pub provider_rule_id: i64,
    pub model_rule_id: i64,
    pub placement: RequestPatchPlacement,
    pub provider_target: String,
    pub model_target: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheRequestPatchExplainEntry {
    pub rule: CacheRequestPatchRule,
    pub origin: RequestPatchRuleOrigin,
    pub status: RequestPatchExplainStatus,
    pub effective_rule_id: Option<i64>,
    pub conflict_with_rule_ids: Vec<i64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct CacheResolvedModelRequestPatches {
    pub provider_id: i64,
    pub model_id: i64,
    pub direct_rules: Vec<CacheRequestPatchRule>,
    pub inherited_rules: Vec<CacheInheritedRequestPatch>,
    pub effective_rules: Vec<CacheResolvedRequestPatch>,
    pub explain: Vec<CacheRequestPatchExplainEntry>,
    pub conflicts: Vec<CacheRequestPatchConflict>,
    pub has_conflicts: bool,
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
            api_key_hash: row
                .api_key_hash
                .unwrap_or_else(|| crate::database::api_key::hash_api_key(&row.api_key)),
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

impl From<crate::database::model::Model> for CacheModel {
    fn from(db: crate::database::model::Model) -> Self {
        Self {
            id: db.id,
            provider_id: db.provider_id,
            real_model_name: db.real_model_name,
            model_name: db.model_name,
            cost_catalog_id: db.cost_catalog_id,
            reasoning_profile_override_id: db.reasoning_profile_override_id,
            supports_streaming: db.supports_streaming,
            supports_tools: db.supports_tools,
            supports_reasoning: db.supports_reasoning,
            supports_image_input: db.supports_image_input,
            supports_embeddings: db.supports_embeddings,
            supports_rerank: db.supports_rerank,
            is_enabled: db.is_enabled,
        }
    }
}

impl CacheModelRoute {
    pub fn from_detail(detail: &ModelRouteDetail) -> Self {
        Self {
            id: detail.route.id,
            route_name: detail.route.route_name.clone(),
            description: detail.route.description.clone(),
            is_enabled: detail.route.is_enabled,
            expose_in_models: detail.route.expose_in_models,
            candidates: detail
                .candidates
                .iter()
                .map(|candidate| CacheModelRouteCandidate {
                    route_id: candidate.candidate.route_id,
                    model_id: candidate.candidate.model_id,
                    provider_id: candidate.provider_id,
                    priority: candidate.candidate.priority,
                    is_enabled: candidate.candidate.is_enabled,
                })
                .collect(),
        }
    }
}

impl From<ApiKeyModelOverride> for CacheApiKeyModelOverride {
    fn from(db: ApiKeyModelOverride) -> Self {
        Self {
            id: db.id,
            api_key_id: db.api_key_id,
            source_name: db.source_name,
            target_route_id: db.target_route_id,
            description: db.description,
            is_enabled: db.is_enabled,
        }
    }
}

impl From<crate::database::provider::ProviderApiKey> for CacheProviderKey {
    fn from(db: crate::database::provider::ProviderApiKey) -> Self {
        Self {
            id: db.id,
            provider_id: db.provider_id,
            api_key: db.api_key,
        }
    }
}

impl From<crate::database::provider::Provider> for CacheProvider {
    fn from(db: crate::database::provider::Provider) -> Self {
        Self {
            id: db.id,
            provider_key: db.provider_key,
            name: db.name,
            endpoint: db.endpoint,
            use_proxy: db.use_proxy,
            provider_type: db.provider_type,
            provider_api_key_mode: db.provider_api_key_mode,
            default_reasoning_profile_id: db.default_reasoning_profile_id,
            is_enabled: db.is_enabled,
        }
    }
}

impl From<ReasoningProfileWithPresets> for CacheReasoningProfile {
    fn from(db: ReasoningProfileWithPresets) -> Self {
        Self {
            id: db.profile.id,
            profile_key: db.profile.profile_key,
            name: db.profile.name,
            description: db.profile.description,
            family: db.family,
            is_enabled: db.profile.is_enabled,
            presets: db
                .presets
                .into_iter()
                .map(|preset| CacheReasoningProfilePreset {
                    id: preset.preset.id,
                    profile_id: preset.preset.profile_id,
                    preset: preset.preset_key,
                    suffix: preset.suffix,
                    requires_reasoning: preset.requires_reasoning,
                    expose_in_models: preset.preset.expose_in_models,
                    is_enabled: preset.preset.is_enabled,
                })
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

impl TryFrom<crate::database::request_patch::RequestPatchRuleResponse> for CacheRequestPatchRule {
    type Error = serde_json::Error;

    fn try_from(
        db: crate::database::request_patch::RequestPatchRuleResponse,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            id: db.id,
            provider_id: db.provider_id,
            model_id: db.model_id,
            placement: db.placement,
            target: db.target,
            operation: db.operation,
            value_json: db
                .value_json
                .map(|value| serde_json::to_string(&value))
                .transpose()?,
            description: db.description,
            is_enabled: db.is_enabled,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
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
