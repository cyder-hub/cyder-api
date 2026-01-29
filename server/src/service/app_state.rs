use std::sync::Arc;
use std::fmt::Debug;
use std::collections::{HashSet, HashMap};
use std::time::Duration;

use axum::Router;
use cyder_tools::log::{debug, info};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use sha2::{Sha256, Digest};

use crate::database::system_api_key::SystemApiKey;
use crate::database::provider::{Provider, ProviderApiKey};
use crate::database::model::Model;
use crate::database::model_alias::ModelAlias;
use crate::database::price::{BillingPlan, PriceRule};
use crate::database::access_control::{AccessControlPolicy as DbAccessControlPolicy};
use crate::database::custom_field::CustomFieldDefinition;

use super::cache::types::{
    CacheProvider, CacheProviderKey, CacheModel, CacheSystemApiKey, 
    CacheAccessControl, CacheBillingPlan, CacheCustomField, CacheEntry
};
use super::cache::{CacheError, memory::MemoryCacheBackend};
use super::cache::repository::CacheRepository;

pub enum GroupItemSelectionStrategy {
    Random,
    Queue,
}

use crate::config::{CacheBackendType, CONFIG};
use crate::service::redis::{self, RedisPool};
use super::cache::redis::RedisCacheBackend;

enum CacheKey<'a> {
    SystemApiKey(&'a str),
    ModelAlias(&'a str),
    ProviderById(i64),
    ProviderByKey(&'a str),
    ModelById(i64),
    ModelByName(&'a str, &'a str),
    AccessControlPolicy(i64),
    ProviderApiKeys(i64),
    CustomFieldsAssignment(i64),
    CustomField(i64),
    BillingPlan(i64),
}

impl<'a> ToString for CacheKey<'a> {
    fn to_string(&self) -> String {
        match self {
            CacheKey::SystemApiKey(key) => format!("sys_api_key:key:{}", key),
            CacheKey::ModelAlias(alias) => format!("alias:{}", alias),
            CacheKey::ProviderById(id) => format!("provider:id:{}", id),
            CacheKey::ProviderByKey(key) => format!("provider:key:{}", key),
            CacheKey::ModelById(id) => format!("model:id:{}", id),
            CacheKey::ModelByName(provider_key, model_name) => format!("model:name:{}/{}", provider_key, model_name),
            CacheKey::AccessControlPolicy(id) => format!("acp:id:{}", id),
            CacheKey::ProviderApiKeys(provider_id) => format!("provider_keys:{}", provider_id),
            CacheKey::CustomFieldsAssignment(id) => format!("cfa:{}", id),
            CacheKey::CustomField(id) => format!("custom_field:id:{}", id),
            CacheKey::BillingPlan(id) => format!("billing_plan:id:{}", id),
        }
    }
}

#[derive(Clone)]
enum AnyCacheRepo<T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static> {
    Memory(CacheRepository<T, MemoryCacheBackend<T>>),
    Redis(CacheRepository<T, RedisCacheBackend<T>>),
}

impl<T> AnyCacheRepo<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    async fn get(&self, key: &str) -> Result<Option<Arc<T>>, CacheError> {
        match self {
            AnyCacheRepo::Memory(repo) => repo.get(key).await,
            AnyCacheRepo::Redis(repo) => repo.get(key).await,
        }
    }

    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<Arc<T>>>, CacheError> {
        match self {
            AnyCacheRepo::Memory(repo) => repo.mget(keys).await,
            AnyCacheRepo::Redis(repo) => repo.mget(keys).await,
        }
    }

    async fn get_entry(&self, key: &str) -> Result<Option<Arc<CacheEntry<T>>>, CacheError> {
        match self {
            AnyCacheRepo::Memory(repo) => repo.get_entry(key).await,
            AnyCacheRepo::Redis(repo) => repo.get_entry(key).await,
        }
    }

    async fn set_positive(&self, key: &str, value: &T) -> Result<(), CacheError> {
        match self {
            AnyCacheRepo::Memory(repo) => repo.set_positive(key, value).await,
            AnyCacheRepo::Redis(repo) => repo.set_positive(key, value).await,
        }
    }

    async fn set_negative(&self, key: &str, ttl: Duration) -> Result<(), CacheError> {
        match self {
            AnyCacheRepo::Memory(repo) => repo.set_negative(key, ttl).await,
            AnyCacheRepo::Redis(repo) => repo.set_negative(key, ttl).await,
        }
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        match self {
            AnyCacheRepo::Memory(repo) => repo.delete(key).await,
            AnyCacheRepo::Redis(repo) => repo.delete(key).await,
        }
    }

    async fn clear(&self) -> Result<(), CacheError> {
        match self {
            AnyCacheRepo::Memory(repo) => repo.clear().await,
            AnyCacheRepo::Redis(repo) => repo.clear().await,
        }
    }
}


/// Simplified cache repository type
type CacheRepo<T> = AnyCacheRepo<T>;

#[derive(Clone)]
pub struct AppState {
    // 1. api_key(key) -> CacheSystemApiKey
    system_api_key_cache: CacheRepo<CacheSystemApiKey>,

    // 2. alias_name(key) -> model_id (i64)
    alias_to_model_id_cache: CacheRepo<i64>,

    // 3, 4. provider_id(id)/provider_key(key) -> CacheProvider
    provider_cache: CacheRepo<CacheProvider>,

    // 5. model_name(key) -> CacheModel (Also keyed by ID for internal resolution)
    model_cache: CacheRepo<CacheModel>,

    // 6. access_control_policy_id(id) -> CacheAccessControl
    access_control_policy_cache: CacheRepo<CacheAccessControl>,

    // 7. provider_id(id) -> CacheProviderKey[]
    provider_api_keys_cache: CacheRepo<Vec<CacheProviderKey>>,

    // 8. entity_id(id) -> custom_field_definition_id Set
    custom_fields_assignment_cache: CacheRepo<HashSet<i64>>,

    // 9. custom_field_definition_id(id) Set -> CacheCustomField[]
    custom_field_cache: CacheRepo<CacheCustomField>,

    // 10. billing_plan_id(id) -> CacheBillingPlan
    billing_plan_cache: CacheRepo<CacheBillingPlan>,

    // Config for negative caching TTL
    negative_cache_ttl: Duration,
}

impl AppState {
    pub async fn new() -> Self {
        let negative_cache_ttl = CONFIG.cache.negative_ttl();
        let ttl = Some(CONFIG.cache.ttl());

        let redis_pool = redis::get_pool().await;
        let use_redis = CONFIG.cache.backend == CacheBackendType::Redis && redis_pool.is_some();

        if use_redis {
            info!("Using Redis cache backend.");
        } else {
            if CONFIG.cache.backend == CacheBackendType::Redis {
                info!("Redis is configured, but connection failed. Falling back to in-memory cache.");
            } else {
                info!("Using in-memory cache backend.");
            }
        }

        let pool = redis_pool.as_ref();

        Self {
            system_api_key_cache: Self::create_repo(ttl, pool),
            alias_to_model_id_cache: Self::create_repo(ttl, pool),
            provider_cache: Self::create_repo(ttl, pool),
            model_cache: Self::create_repo(ttl, pool),
            access_control_policy_cache: Self::create_repo(ttl, pool),
            provider_api_keys_cache: Self::create_repo(ttl, pool),
            custom_fields_assignment_cache: Self::create_repo(ttl, pool),
            custom_field_cache: Self::create_repo(ttl, pool),
            billing_plan_cache: Self::create_repo(ttl, pool),
            negative_cache_ttl,
        }
    }

    fn create_repo<T>(ttl: Option<Duration>, pool: Option<&RedisPool>) -> CacheRepo<T>
    where
        T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    {
        if let Some(pool) = pool {
            let redis_config = CONFIG.redis.as_ref().expect("Redis config should exist if pool exists");
            let key_prefix = format!("{}{}", redis_config.key_prefix, CONFIG.cache.redis.key_prefix);
            let backend = RedisCacheBackend::new(pool.clone(), key_prefix);
            AnyCacheRepo::Redis(CacheRepository::new(backend, ttl))
        } else {
            AnyCacheRepo::Memory(CacheRepository::new(MemoryCacheBackend::new(), ttl))
        }
    }

    /// Hash an API key with SHA256 and return the hex digest.
    fn hash_api_key(key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub async fn reload(&self) {
        info!("Reloading AppState: Starting cache refresh...");
        let mut stats: HashMap<&'static str, usize> = HashMap::new();
        
        // 1. SystemApiKeys
        if let Ok(keys) = SystemApiKey::list_all() {
            stats.insert("System API Keys", keys.len());
            for key in keys {
                let cache_item = CacheSystemApiKey::from(key.clone());

                // Cache by api_key hash
                let hashed_api_key = Self::hash_api_key(&key.api_key);
                let api_key_cache_key = CacheKey::SystemApiKey(&hashed_api_key).to_string();
                let _ = self.system_api_key_cache.set_positive(&api_key_cache_key, &cache_item).await;

                // Cache by ref if present
                if let Some(ref_val) = &key.ref_ {
                    let hashed_ref = Self::hash_api_key(ref_val);
                    let ref_cache_key = CacheKey::SystemApiKey(&hashed_ref).to_string();
                    let _ = self.system_api_key_cache.set_positive(&ref_cache_key, &cache_item).await;
                }
            }
        }

        // 3, 4. Providers
        let mut provider_id_to_key: HashMap<i64, String> = HashMap::new();
        if let Ok(providers) = Provider::list_all() {
            stats.insert("Providers", providers.len());
            for provider in providers {
                provider_id_to_key.insert(provider.id, provider.provider_key.clone());
                let cache_item = CacheProvider::from(provider);
                let _ = self.provider_cache.set_positive(&CacheKey::ProviderById(cache_item.id).to_string(), &cache_item).await;
                let _ = self.provider_cache.set_positive(&CacheKey::ProviderByKey(&cache_item.provider_key).to_string(), &cache_item).await;
            }
        }

        // 5. Models
        if let Ok(models) = Model::list_all() {
            stats.insert("Models", models.len());
            for model in models {
                let cache_item = CacheModel::from(model);
                let _ = self.model_cache.set_positive(&CacheKey::ModelById(cache_item.id).to_string(), &cache_item).await;
                if let Some(provider_key) = provider_id_to_key.get(&cache_item.provider_id) {
                    let _ = self.model_cache.set_positive(&CacheKey::ModelByName(provider_key, &cache_item.model_name).to_string(), &cache_item).await;
                }
            }
        }

        // 2. Model Aliases
        if let Ok(aliases) = ModelAlias::list_all() {
            stats.insert("Model Aliases", aliases.len());
            for alias in aliases {
                let _ = self.alias_to_model_id_cache.set_positive(&CacheKey::ModelAlias(&alias.alias_name).to_string(), &alias.target_model_id).await;
            }
        }

        // 6. Access Control Policies
        if let Ok(policies) = DbAccessControlPolicy::list_all() {
            stats.insert("Access Control Policies", policies.len());
            for policy in policies {
                let cache_item = CacheAccessControl::from(policy);
                let _ = self.access_control_policy_cache.set_positive(&CacheKey::AccessControlPolicy(cache_item.id).to_string(), &cache_item).await;
            }
        }

        // 7. Provider API Keys
        if let Ok(keys) = ProviderApiKey::list_all() {
            stats.insert("Provider API Keys", keys.len());
            let mut by_provider: HashMap<i64, Vec<CacheProviderKey>> = HashMap::new();
            for key in keys {
                by_provider.entry(key.provider_id)
                    .or_default()
                    .push(CacheProviderKey::from(key));
            }
            stats.insert("Provider API Key Groups", by_provider.len());
            for (provider_id, provider_keys) in by_provider {
                let _ = self.provider_api_keys_cache.set_positive(&CacheKey::ProviderApiKeys(provider_id).to_string(), &provider_keys).await;
            }
        }

        // 9. Custom Fields Definitions
        if let Ok(defs) = CustomFieldDefinition::list_all_active() {
            stats.insert("Custom Field Definitions", defs.len());
            for def in defs {
                let cache_item = CacheCustomField::from(def);
                let _ = self.custom_field_cache.set_positive(&CacheKey::CustomField(cache_item.id).to_string(), &cache_item).await;
            }
        }

        // 8. Custom Field Assignments
        if let Ok(assignments) = CustomFieldDefinition::list_all_enabled_model_assignments() {
            let mut by_model: HashMap<i64, HashSet<i64>> = HashMap::new();
            for a in assignments {
                by_model.entry(a.model_id).or_default().insert(a.custom_field_definition_id);
            }
            stats.insert("Model Custom Field Assignments", by_model.len());
            for (model_id, field_ids) in by_model {
                let _ = self.custom_fields_assignment_cache.set_positive(&CacheKey::CustomFieldsAssignment(model_id).to_string(), &field_ids).await;
            }
        }
        if let Ok(assignments) = CustomFieldDefinition::list_all_enabled_provider_assignments() {
            let mut by_provider: HashMap<i64, HashSet<i64>> = HashMap::new();
            for a in assignments {
                by_provider.entry(a.provider_id).or_default().insert(a.custom_field_definition_id);
            }
            stats.insert("Provider Custom Field Assignments", by_provider.len());
            for (provider_id, field_ids) in by_provider {
                let _ = self.custom_fields_assignment_cache.set_positive(&CacheKey::CustomFieldsAssignment(provider_id).to_string(), &field_ids).await;
            }
        }

        // 10. Billing Plans
        if let Ok(plans) = BillingPlan::list_all() {
            stats.insert("Billing Plans", plans.len());
            let all_rules = PriceRule::list_all().unwrap_or_default();
            let mut rules_by_plan: HashMap<i64, Vec<PriceRule>> = HashMap::new();
            for rule in all_rules {
                rules_by_plan.entry(rule.plan_id).or_default().push(rule);
            }

            for plan in plans {
                let rules = rules_by_plan.remove(&plan.id).unwrap_or_default();
                let cache_item = CacheBillingPlan::from_db_with_rules(plan, rules);
                let _ = self.billing_plan_cache.set_positive(&CacheKey::BillingPlan(cache_item.id).to_string(), &cache_item).await;
            }
        }

        info!("AppState reloaded successfully. Cache details:\n{:#?}", stats);
    }

    pub async fn clear_cache(&self) {
        info!("Clearing app cache...");
        // Since all redis keys share the same prefix, we only need to clear one of the repos
        // to clear all of them. For memory cache, each repo is a separate instance.
        if let Err(e) = self.system_api_key_cache.clear().await {
            cyder_tools::log::error!("Failed to clear system_api_key_cache: {}", e);
        }
        if let Err(e) = self.alias_to_model_id_cache.clear().await {
            cyder_tools::log::error!("Failed to clear alias_to_model_id_cache: {}", e);
        }
        if let Err(e) = self.provider_cache.clear().await {
            cyder_tools::log::error!("Failed to clear provider_cache: {}", e);
        }
        if let Err(e) = self.model_cache.clear().await {
            cyder_tools::log::error!("Failed to clear model_cache: {}", e);
        }
        if let Err(e) = self.access_control_policy_cache.clear().await {
            cyder_tools::log::error!("Failed to clear access_control_policy_cache: {}", e);
        }
        if let Err(e) = self.provider_api_keys_cache.clear().await {
            cyder_tools::log::error!("Failed to clear provider_api_keys_cache: {}", e);
        }
        if let Err(e) = self.custom_fields_assignment_cache.clear().await {
            cyder_tools::log::error!("Failed to clear custom_fields_assignment_cache: {}", e);
        }
        if let Err(e) = self.custom_field_cache.clear().await {
            cyder_tools::log::error!("Failed to clear custom_field_cache: {}", e);
        }
        if let Err(e) = self.billing_plan_cache.clear().await {
            cyder_tools::log::error!("Failed to clear billing_plan_cache: {}", e);
        }
        info!("App cache cleared.");
    }

    // ============================================================================================
    // 1. api_key(key) -> CacheSystemApiKey
    // ============================================================================================
    pub async fn get_system_api_key(&self, key: &str) -> Result<Option<Arc<CacheSystemApiKey>>, AppStoreError> {
        let hashed_key = Self::hash_api_key(key);
        let cache_key = CacheKey::SystemApiKey(&hashed_key).to_string();

        if let Some(entry) = self.system_api_key_cache.get_entry(&cache_key).await? {
            return match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", &cache_key);
                    Ok(Some(value.clone()))
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &cache_key);
                    Ok(None)
                }
            };
        }

        debug!("cache miss: {}", &cache_key);

        // DB lookup: try by key first, then by ref.
        let db_key_result = SystemApiKey::get_by_key(key).or_else(|_| SystemApiKey::get_by_ref(key));

        if let Ok(db_key) = db_key_result {
            let cache_item = Arc::new(CacheSystemApiKey::from(db_key.clone()));
            
            // Cache by api_key
            let hashed_api_key = Self::hash_api_key(&db_key.api_key);
            self.system_api_key_cache.set_positive(&CacheKey::SystemApiKey(&hashed_api_key).to_string(), &cache_item).await?;

            // Cache by ref if present
            if let Some(ref_val) = &db_key.ref_ {
                let hashed_ref = Self::hash_api_key(ref_val);
                self.system_api_key_cache.set_positive(&CacheKey::SystemApiKey(&hashed_ref).to_string(), &cache_item).await?;
            }
            
            Ok(Some(cache_item))
        } else {
            self.system_api_key_cache.set_negative(&cache_key, self.negative_cache_ttl).await?;
            Ok(None)
        }
    }

    pub async fn invalidate_system_api_key(&self, key: &str) -> Result<(), AppStoreError> {
        let hashed_key_to_find = Self::hash_api_key(key);
        let cache_key_to_find = CacheKey::SystemApiKey(&hashed_key_to_find).to_string();
        debug!("invalidate: {}", &cache_key_to_find);
        
        // Try to get from cache to find the full object.
        if let Ok(Some(cached_entry)) = self.system_api_key_cache.get(&cache_key_to_find).await {
            if let Some(ref_val) = &cached_entry.ref_ {
                let hashed_ref = Self::hash_api_key(ref_val);
                self.system_api_key_cache.delete(&CacheKey::SystemApiKey(&hashed_ref).to_string()).await?;
            }
        }
        self.system_api_key_cache.delete(&cache_key_to_find).await?;
        
        Ok(())
    }

    // ============================================================================================
    // 2. alias_name(key) -> CacheModel
    // ============================================================================================
    pub async fn get_model_by_alias(&self, alias: &str) -> Result<Option<Arc<CacheModel>>, AppStoreError> {
        let alias_key = CacheKey::ModelAlias(alias).to_string();
        
        let model_id = match self.alias_to_model_id_cache.get_entry(&alias_key).await? {
            Some(entry) => match *entry {
                CacheEntry::Positive(ref id) => {
                    debug!("cache hit (positive): {}", &alias_key);
                    Some(**id)
                },
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &alias_key);
                    return Ok(None)
                }
            },
            None => {
                debug!("cache miss: {}", &alias_key);
                if let Ok(Some(db_alias)) = ModelAlias::get_by_alias_name(alias) {
                    self.alias_to_model_id_cache.set_positive(&alias_key, &db_alias.target_model_id).await?;
                    Some(db_alias.target_model_id)
                } else {
                    self.alias_to_model_id_cache.set_negative(&alias_key, self.negative_cache_ttl).await?;
                    None
                }
            }
        };

        match model_id {
            Some(id) => self.get_model_by_id(id).await,
            None => Ok(None),
        }
    }

    pub async fn invalidate_model_alias(&self, alias: &str) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ModelAlias(alias).to_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.alias_to_model_id_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 3. provider_id(id) -> CacheProvider
    // ============================================================================================
    pub async fn get_provider_by_id(&self, id: i64) -> Result<Option<Arc<CacheProvider>>, AppStoreError> {
        let cache_key = CacheKey::ProviderById(id).to_string();
        
        match self.provider_cache.get_entry(&cache_key).await? {
            Some(entry) => match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", &cache_key);
                    return Ok(Some(value.clone()));
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &cache_key);
                    return Ok(None);
                }
            },
            None => {
                debug!("cache miss: {}", &cache_key);
                if let Ok(db_provider) = Provider::get_by_id(id) {
                    let cache_item = CacheProvider::from(db_provider.clone());
                    self.provider_cache.set_positive(&cache_key, &cache_item).await?;
                    self.provider_cache.set_positive(&CacheKey::ProviderByKey(&db_provider.provider_key).to_string(), &cache_item).await?;
                    return Ok(Some(Arc::new(cache_item)));
                } else {
                    self.provider_cache.set_negative(&cache_key, self.negative_cache_ttl).await?;
                    return Ok(None);
                }
            }
        }
    }

    pub async fn invalidate_provider_by_id(&self, id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ProviderById(id).to_string();
        debug!("invalidate: {}", &cache_key);
        if let Some(provider) = self.get_provider_by_id(id).await? {
            let _ = self.invalidate_provider_by_key(&provider.provider_key).await;
        }
        Ok(self.provider_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 4. provider_key(key) -> CacheProvider
    // ============================================================================================
    pub async fn get_provider_by_key(&self, key: &str) -> Result<Option<Arc<CacheProvider>>, AppStoreError> {
        let cache_key = CacheKey::ProviderByKey(key).to_string();
        
        match self.provider_cache.get_entry(&cache_key).await? {
            Some(entry) => match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", &cache_key);
                    return Ok(Some(value.clone()));
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &cache_key);
                    return Ok(None);
                }
            },
            None => {
                debug!("cache miss: {}", &cache_key);
                if let Ok(Some(db_provider)) = Provider::get_by_key(key) {
                    let cache_item = CacheProvider::from(db_provider.clone());
                    self.provider_cache.set_positive(&cache_key, &cache_item).await?;
                    self.provider_cache.set_positive(&CacheKey::ProviderById(db_provider.id).to_string(), &cache_item).await?;
                    return Ok(Some(Arc::new(cache_item)));
                } else {
                    self.provider_cache.set_negative(&cache_key, self.negative_cache_ttl).await?;
                    return Ok(None);
                }
            }
        }
    }

    pub async fn invalidate_provider_by_key(&self, key: &str) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ProviderByKey(key).to_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.provider_cache.delete(&cache_key).await?)
    }

    pub async fn invalidate_provider(&self, id: i64, key: Option<&str>) -> Result<(), AppStoreError> {
        debug!("invalidate provider: id={}, key={:?}", id, key);
        if let Some(k) = key {
            let _ = self.invalidate_provider_by_key(k).await;
        } else if let Some(p) = self.get_provider_by_id(id).await? {
            let _ = self.invalidate_provider_by_key(&p.provider_key).await;
        }
        self.invalidate_provider_by_id(id).await
    }

    // ============================================================================================
    // 5. model_name(key) -> CacheModel
    // ============================================================================================
    pub async fn get_model_by_name(&self, provider_key: &str, model_name: &str) -> Result<Option<Arc<CacheModel>>, AppStoreError> {
        let cache_key = CacheKey::ModelByName(provider_key, model_name).to_string();

        match self.model_cache.get_entry(&cache_key).await? {
            Some(entry) => match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", &cache_key);
                    return Ok(Some(value.clone()));
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &cache_key);
                    return Ok(None);
                }
            },
            None => {
                debug!("cache miss: {}", &cache_key);
                if let Some(provider) = self.get_provider_by_key(provider_key).await? {
                    // Here we assume that a function with this name exists, which can find a model based on the provider ID and model name.
                    // This is a reasonable assumption given the context of needing to uniquely identify models.
                    if let Ok(Some(db_model)) = Model::get_by_name_and_provider_id(model_name, provider.id) {
                        let cache_item = CacheModel::from(db_model.clone());
                        self.model_cache.set_positive(&cache_key, &cache_item).await?;
                        self.model_cache.set_positive(&CacheKey::ModelById(db_model.id).to_string(), &cache_item).await?;
                        return Ok(Some(Arc::new(cache_item)));
                    }
                }
                
                // If provider or model not found, set negative cache
                self.model_cache.set_negative(&cache_key, self.negative_cache_ttl).await?;
                return Ok(None);
            }
        }
    }
    
    // Internal helper for alias resolution + Lazy load by ID
    pub async fn get_model_by_id(&self, id: i64) -> Result<Option<Arc<CacheModel>>, AppStoreError> {
        let cache_key = CacheKey::ModelById(id).to_string();

        match self.model_cache.get_entry(&cache_key).await? {
            Some(entry) => match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", &cache_key);
                    return Ok(Some(value.clone()));
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &cache_key);
                    return Ok(None);
                }
            },
            None => {
                debug!("cache miss: {}", &cache_key);
                if let Ok(db_model) = Model::get_by_id(id) {
                    let cache_item = CacheModel::from(db_model.clone());
                    self.model_cache.set_positive(&cache_key, &cache_item).await?;
                    if let Ok(Some(provider)) = self.get_provider_by_id(db_model.provider_id).await {
                        self.model_cache.set_positive(&CacheKey::ModelByName(&provider.provider_key, &db_model.model_name).to_string(), &cache_item).await?;
                    }
                    return Ok(Some(Arc::new(cache_item)));
                } else {
                    self.model_cache.set_negative(&cache_key, self.negative_cache_ttl).await?;
                    return Ok(None);
                }
            }
        }
    }

    pub async fn invalidate_model_by_name(&self, provider_key: &str, model_name: &str) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ModelByName(provider_key, model_name).to_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.model_cache.delete(&cache_key).await?)
    }
    
    pub async fn invalidate_model(&self, id: i64, name: Option<&str>) -> Result<(), AppStoreError> {
        debug!("invalidate model: id={}, name={:?}", id, name);
        if let Some(n) = name {
            let parts: Vec<&str> = n.splitn(2, '/').collect();
            if parts.len() == 2 {
                let _ = self.invalidate_model_by_name(parts[0], parts[1]).await;
            }
        } else if let Some(m) = self.get_model_by_id(id).await? {
            if let Ok(Some(p)) = self.get_provider_by_id(m.provider_id).await {
                let _ = self.invalidate_model_by_name(&p.provider_key, &m.model_name).await;
            }
        }
        Ok(self.model_cache.delete(&CacheKey::ModelById(id).to_string()).await?)
    }

    // ============================================================================================
    // 6. access_control_policy_id(id) -> CacheAccessControl
    // ============================================================================================
    pub async fn get_access_control_policy(&self, id: i64) -> Result<Option<Arc<CacheAccessControl>>, AppStoreError> {
        let cache_key = CacheKey::AccessControlPolicy(id).to_string();
        
        match self.access_control_policy_cache.get_entry(&cache_key).await? {
            Some(entry) => match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", &cache_key);
                    return Ok(Some(value.clone()));
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &cache_key);
                    return Ok(None);
                }
            },
            None => {
                debug!("cache miss: {}", &cache_key);
                if let Ok(db_policy) = DbAccessControlPolicy::get_by_id(id) {
                    let cache_item = CacheAccessControl::from(db_policy);
                    self.access_control_policy_cache.set_positive(&cache_key, &cache_item).await?;
                    return Ok(Some(Arc::new(cache_item)));
                } else {
                    self.access_control_policy_cache.set_negative(&cache_key, self.negative_cache_ttl).await?;
                    return Ok(None);
                }
            }
        }
    }

    pub async fn invalidate_access_control_policy(&self, id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::AccessControlPolicy(id).to_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.access_control_policy_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 7. provider_id(id) -> CacheProviderKey[]
    // ============================================================================================
    pub async fn get_provider_api_keys(&self, provider_id: i64) -> Result<Vec<Arc<CacheProviderKey>>, AppStoreError> {
        let cache_key = CacheKey::ProviderApiKeys(provider_id).to_string();
        
        if let Some(keys) = self.provider_api_keys_cache.get(&cache_key).await? {
            debug!("cache hit: {}", &cache_key);
            Ok(keys.iter().map(|k| Arc::new(k.clone())).collect())
        } else {
             debug!("cache miss: {}", &cache_key);
             if let Ok(db_keys) = ProviderApiKey::list_by_provider_id(provider_id) {
                 let cache_keys: Vec<CacheProviderKey> = db_keys.into_iter().map(CacheProviderKey::from).collect();
                 self.provider_api_keys_cache.set_positive(&cache_key, &cache_keys).await?;
                 Ok(cache_keys.into_iter().map(Arc::new).collect())
             } else {
                 Ok(Vec::new())
             }
        }
    }
    
    pub async fn get_one_provider_api_key_by_provider(&self, provider_id: i64, _strategy: GroupItemSelectionStrategy) -> Result<Option<Arc<CacheProviderKey>>, AppStoreError> {
        let keys = self.get_provider_api_keys(provider_id).await?;
        // TODO: Implement strategy
        // For now, we return the first key
        Ok(keys.into_iter().next())
    }

    pub async fn invalidate_provider_api_keys(&self, provider_id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ProviderApiKeys(provider_id).to_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.provider_api_keys_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 8. entity_id(id) -> custom_field_definition_id Set
    // ============================================================================================
    pub async fn get_model_custom_field_ids(&self, model_id: i64) -> Result<Option<Arc<HashSet<i64>>>, AppStoreError> {
        let cache_key = CacheKey::CustomFieldsAssignment(model_id).to_string();

        if let Some(entry) = self.custom_fields_assignment_cache.get_entry(&cache_key).await? {
            return match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", &cache_key);
                    Ok(Some(value.clone()))
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &cache_key);
                    Ok(None)
                }
            };
        }

        debug!("cache miss: {}", &cache_key);
        match CustomFieldDefinition::list_enabled_model_assignments_by_model_id(model_id) {
            Ok(assignments) if !assignments.is_empty() => {
                let field_ids: HashSet<i64> = assignments.into_iter().map(|a| a.custom_field_definition_id).collect();
                let cache_item = Arc::new(field_ids);
                self.custom_fields_assignment_cache.set_positive(&cache_key, &cache_item).await?;
                Ok(Some(cache_item))
            }
            _ => {
                self.custom_fields_assignment_cache.set_negative(&cache_key, self.negative_cache_ttl).await?;
                Ok(None)
            }
        }
    }

    pub async fn get_provider_custom_field_ids(&self, provider_id: i64) -> Result<Option<Arc<HashSet<i64>>>, AppStoreError> {
        let cache_key = CacheKey::CustomFieldsAssignment(provider_id).to_string();

        if let Some(entry) = self.custom_fields_assignment_cache.get_entry(&cache_key).await? {
            return match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", &cache_key);
                    Ok(Some(value.clone()))
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &cache_key);
                    Ok(None)
                }
            };
        }

        debug!("cache miss: {}", &cache_key);
        match CustomFieldDefinition::list_enabled_provider_assignments_by_provider_id(provider_id) {
            Ok(assignments) if !assignments.is_empty() => {
                let field_ids: HashSet<i64> = assignments.into_iter().map(|a| a.custom_field_definition_id).collect();
                let cache_item = Arc::new(field_ids);
                self.custom_fields_assignment_cache.set_positive(&cache_key, &cache_item).await?;
                Ok(Some(cache_item))
            }
            _ => {
                self.custom_fields_assignment_cache.set_negative(&cache_key, self.negative_cache_ttl).await?;
                Ok(None)
            }
        }
    }

    pub async fn get_custom_fields_by_model_id(&self, model_id: i64) -> Result<Vec<Arc<CacheCustomField>>, AppStoreError> {
        match self.get_model_custom_field_ids(model_id).await? {
            Some(ids) if !ids.is_empty() => self.get_custom_fields(&ids).await,
            _ => Ok(Vec::new()),
        }
    }

    pub async fn get_custom_fields_by_provider_id(&self, provider_id: i64) -> Result<Vec<Arc<CacheCustomField>>, AppStoreError> {
        match self.get_provider_custom_field_ids(provider_id).await? {
            Some(ids) if !ids.is_empty() => self.get_custom_fields(&ids).await,
            _ => Ok(Vec::new()),
        }
    }

    pub async fn invalidate_model_custom_fields(&self, model_id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::CustomFieldsAssignment(model_id).to_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.custom_fields_assignment_cache.delete(&cache_key).await?)
    }

    pub async fn invalidate_provider_custom_fields(&self, provider_id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::CustomFieldsAssignment(provider_id).to_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.custom_fields_assignment_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 9. custom_field_definition_id(id) Set -> CacheCustomField[]
    // ============================================================================================
    pub async fn get_custom_fields(&self, ids: &HashSet<i64>) -> Result<Vec<Arc<CacheCustomField>>, AppStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        
        let keys: Vec<String> = ids.iter().map(|id| CacheKey::CustomField(*id).to_string()).collect();
        let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
        
        let results = self.custom_field_cache.mget(&key_refs).await?;
        
        results.iter().zip(key_refs.iter()).for_each(|(res, key)| {
            if res.is_none() {
                debug!("cache miss: {}", key);
            }
        });
        
        Ok(results.into_iter().flatten().collect())
    }

    pub async fn invalidate_custom_field(&self, id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::CustomField(id).to_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.custom_field_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 10. billing_plan_id(id) -> CacheBillingPlan
    // ============================================================================================
    pub async fn get_billing_plan_by_id(&self, id: i64) -> Result<Option<Arc<CacheBillingPlan>>, AppStoreError> {
        let cache_key = CacheKey::BillingPlan(id).to_string();

        if let Some(entry) = self.billing_plan_cache.get_entry(&cache_key).await? {
            return match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", &cache_key);
                    Ok(Some(value.clone()))
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", &cache_key);
                    Ok(None)
                }
            };
        }
        
        debug!("cache miss: {}", &cache_key);
        match BillingPlan::get_by_id(id) {
            Ok(plan) => {
                let rules = PriceRule::list_by_plan_id(id).unwrap_or_else(|_| {
                    cyder_tools::log::warn!("Failed to load price rules for plan_id: {}", id);
                    Vec::new()
                });
                let cache_item = Arc::new(CacheBillingPlan::from_db_with_rules(plan, rules));
                self.billing_plan_cache.set_positive(&cache_key, &cache_item).await?;
                Ok(Some(cache_item))
            },
            Err(_) => {
                self.billing_plan_cache.set_negative(&cache_key, self.negative_cache_ttl).await?;
                Ok(None)
            }
        }
    }

    pub async fn invalidate_billing_plan(&self, id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::BillingPlan(id).to_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.billing_plan_cache.delete(&cache_key).await?)
    }
}

#[derive(Debug, Error)]
pub enum AppStoreError {
    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Resource already exists: {0}")]
    AlreadyExists(String),

    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Lock error: {0}")]
    LockError(String),
}

impl From<CacheError> for AppStoreError {
    fn from(e: CacheError) -> Self {
        match e {
            CacheError::NotFound(msg) => AppStoreError::NotFound(msg),
            CacheError::AlreadyExists(msg) => AppStoreError::AlreadyExists(msg),
            _ => AppStoreError::CacheError(e.to_string()),
        }
    }
}

pub async fn create_app_state() -> Arc<AppState> {
    let app_state = Arc::new(AppState::new().await);
    app_state.clear_cache().await;
    app_state.reload().await;
    app_state
}

pub type StateRouter = Router<Arc<AppState>>;

pub fn create_state_router() -> StateRouter {
  Router::<Arc<AppState>>::new()
}
