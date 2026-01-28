// Cache-specific types optimized for caching layer
// These structures contain only the fields needed for cache operations,
// reducing memory footprint and improving cache performance.

use serde::{de, Deserialize, Serialize};
use std::sync::Arc;
use serde_with::serde_as;
use crate::schema::enum_def::{Action, RuleScope, ProviderType, FieldPlacement, FieldType};

/// Represents an entry in the cache, which can either be a value (Positive)
/// or a marker indicating the value does not exist (Negative).
#[serde_as]
#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub enum CacheEntry<T: Clone + Serialize + de::DeserializeOwned> {
    Positive(#[serde_as(as = "Arc<serde_with::Same>")] Arc<T>),
    Negative,
}

/// Cached system API key with only essential fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSystemApiKey {
    pub id: i64,
    pub name: String,
    pub ref_: Option<String>,
    pub access_control_policy_id: Option<i64>,
}

/// Cached model with only essential fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheModel {
    pub id: i64,
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub billing_plan_id: Option<i64>,
}

/// Cached provider with only essential fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheProvider {
    pub id: i64,
    pub provider_key: String,
    pub name: String,
    pub endpoint: String,
    pub use_proxy: bool,
    pub provider_type: ProviderType,
}

/// Cached provider API key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheProviderKey {
    pub id: i64,
    pub provider_id: i64,
    pub api_key: String,
}

/// Cached access control rule (part of CacheAccessControl)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheAccessControlRule {
    pub id: i64,
    pub rule_type: Action,
    pub priority: i32, 
    pub scope: RuleScope,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
}

/// Cached access control policy with embedded rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheAccessControl {
    pub id: i64,
    pub name: String,
    pub default_action: Action,
    pub rules: Vec<CacheAccessControlRule>,
}

/// Cached custom field definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheCustomField {
    pub id: i64,
    pub field_name: String,
    pub field_placement: FieldPlacement,
    pub field_type: FieldType,
    pub string_value: Option<String>,
    pub integer_value: Option<i64>,
    pub number_value: Option<f32>,
    pub boolean_value: Option<bool>,
}

/// Cached price rule (part of CacheBillingPlan)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachePriceRule {
    pub effective_from: i64,
    pub effective_until: Option<i64>,
    pub period_start_seconds_utc: Option<i32>,
    pub period_end_seconds_utc: Option<i32>,
    pub usage_type: String,
    pub media_type: String,
    pub condition_had_reasoning: Option<i32>,
    pub tier_from_tokens: Option<i32>,
    pub tier_to_tokens: Option<i32>,
    pub price_in_micro_units: Option<i64>,
}

/// Cached billing plan with embedded price rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheBillingPlan {
    pub id: i64,
    pub name: String,
    pub currency: String,
    pub price_rules: Vec<CachePriceRule>,
}

// Conversion implementations from database types to cache types

impl From<crate::database::system_api_key::SystemApiKey> for CacheSystemApiKey {
    fn from(db: crate::database::system_api_key::SystemApiKey) -> Self {
        Self {
            id: db.id,
            name: db.name,
            ref_: db.ref_,
            access_control_policy_id: db.access_control_policy_id,
        }
    }
}

impl From<crate::database::model::Model> for CacheModel {
    fn from(db: crate::database::model::Model) -> Self {
        let model_name = db.model_name.clone();
        Self {
            id: db.id,
            provider_id: db.provider_id,
            real_model_name: db.real_model_name,
            model_name,
            billing_plan_id: db.billing_plan_id,
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

impl From<crate::database::access_control::AccessControlRule> for CacheAccessControlRule {
    fn from(db: crate::database::access_control::AccessControlRule) -> Self {
        Self {
            id: db.id,
            rule_type: db.rule_type,
            priority: db.priority,
            scope: db.scope,
            provider_id: db.provider_id,
            model_id: db.model_id,
        }
    }
}

impl From<crate::database::access_control::ApiAccessControlPolicy> for CacheAccessControl {
    fn from(db: crate::database::access_control::ApiAccessControlPolicy) -> Self {
        Self {
            id: db.id,
            name: db.name,
            default_action: db.default_action,
            rules: db.rules.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<crate::database::custom_field::CustomFieldDefinition> for CacheCustomField {
    fn from(db: crate::database::custom_field::CustomFieldDefinition) -> Self {
        Self {
            id: db.id,
            field_name: db.field_name,
            field_placement: db.field_placement,
            field_type: db.field_type,
            string_value: db.string_value,
            integer_value: db.integer_value,
            number_value: db.number_value,
            boolean_value: db.boolean_value,
        }
    }
}

impl From<crate::database::price::PriceRule> for CachePriceRule {
    fn from(db: crate::database::price::PriceRule) -> Self {
        Self {
            effective_from: db.effective_from,
            effective_until: db.effective_until,
            period_start_seconds_utc: db.period_start_seconds_utc,
            period_end_seconds_utc: db.period_end_seconds_utc,
            usage_type: db.usage_type,
            media_type: db.media_type.unwrap_or_default(),
            condition_had_reasoning: db.condition_had_reasoning,
            tier_from_tokens: db.tier_from_tokens,
            tier_to_tokens: db.tier_to_tokens,
            price_in_micro_units: Some(db.price_in_micro_units),
        }
    }
}

// Note: CacheBillingPlan conversion requires both BillingPlan and its PriceRules
// This will be handled by a dedicated function in the cache layer
impl CacheBillingPlan {
    /// Create a CacheBillingPlan from database BillingPlan and PriceRules
    pub fn from_db_with_rules(
        plan: crate::database::price::BillingPlan,
        rules: Vec<crate::database::price::PriceRule>,
    ) -> Self {
        Self {
            id: plan.id,
            name: plan.name,
            currency: plan.currency,
            price_rules: rules.into_iter().map(Into::into).collect(),
        }
    }
}
