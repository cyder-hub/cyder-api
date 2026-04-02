use rand::{Rng, rng};
use reqwest::{Client, Proxy};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use cyder_tools::log::{debug, error, info};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::database::access_control::AccessControlPolicy as DbAccessControlPolicy;
use crate::database::custom_field::CustomFieldDefinition;
use crate::database::model::Model;
use crate::database::model_alias::ModelAlias;
use crate::database::price::{BillingPlan, PriceRule};
use crate::database::provider::{Provider, ProviderApiKey};
use crate::database::system_api_key::SystemApiKey;
use crate::schema::enum_def::ProviderApiKeyMode;

use super::cache::repository::{CacheRepository, DynCacheRepo};
use super::cache::types::{
    CacheAccessControl, CacheBillingPlan, CacheCustomField, CacheEntry, CacheModel, CacheProvider,
    CacheProviderKey, CacheSystemApiKey,
};
use super::cache::{CacheError, memory::MemoryCacheBackend};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupItemSelectionStrategy {
    Random,
    Queue,
}

impl From<ProviderApiKeyMode> for GroupItemSelectionStrategy {
    fn from(value: ProviderApiKeyMode) -> Self {
        match value {
            ProviderApiKeyMode::Queue => GroupItemSelectionStrategy::Queue,
            ProviderApiKeyMode::Random => GroupItemSelectionStrategy::Random,
        }
    }
}

use super::cache::redis::RedisCacheBackend;
use crate::config::{CONFIG, CacheBackendType};
use crate::service::redis::{self, RedisPool};

/// Type-erased cache repository, dispatching to the concrete backend
/// (Memory or Redis) via dynamic dispatch. The backend is selected once
/// at startup in `create_repo`, eliminating per-operation match arms.
type CacheRepo<T> = Arc<dyn DynCacheRepo<T>>;

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

use compact_str::{CompactString, format_compact};

impl<'a> std::fmt::Display for CacheKey<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheKey::SystemApiKey(key) => write!(f, "sys_api_key:key:{}", key),
            CacheKey::ModelAlias(alias) => write!(f, "alias:{}", alias),
            CacheKey::ProviderById(id) => write!(f, "provider:id:{}", id),
            CacheKey::ProviderByKey(key) => write!(f, "provider:key:{}", key),
            CacheKey::ModelById(id) => write!(f, "model:id:{}", id),
            CacheKey::ModelByName(provider_key, model_name) => {
                write!(f, "model:name:{}/{}", provider_key, model_name)
            }
            CacheKey::AccessControlPolicy(id) => write!(f, "acp:id:{}", id),
            CacheKey::ProviderApiKeys(provider_id) => write!(f, "provider_keys:{}", provider_id),
            CacheKey::CustomFieldsAssignment(id) => write!(f, "cfa:{}", id),
            CacheKey::CustomField(id) => write!(f, "custom_field:id:{}", id),
            CacheKey::BillingPlan(id) => write!(f, "billing_plan:id:{}", id),
        }
    }
}

impl<'a> CacheKey<'a> {
    fn to_compact_string(&self) -> CompactString {
        match self {
            CacheKey::SystemApiKey(key) => format_compact!("sys_api_key:key:{}", key),
            CacheKey::ModelAlias(alias) => format_compact!("alias:{}", alias),
            CacheKey::ProviderById(id) => format_compact!("provider:id:{}", id),
            CacheKey::ProviderByKey(key) => format_compact!("provider:key:{}", key),
            CacheKey::ModelById(id) => format_compact!("model:id:{}", id),
            CacheKey::ModelByName(provider_key, model_name) => {
                format_compact!("model:name:{}/{}", provider_key, model_name)
            }
            CacheKey::AccessControlPolicy(id) => format_compact!("acp:id:{}", id),
            CacheKey::ProviderApiKeys(provider_id) => {
                format_compact!("provider_keys:{}", provider_id)
            }
            CacheKey::CustomFieldsAssignment(id) => format_compact!("cfa:{}", id),
            CacheKey::CustomField(id) => format_compact!("custom_field:id:{}", id),
            CacheKey::BillingPlan(id) => format_compact!("billing_plan:id:{}", id),
        }
    }
}

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

    // HTTP clients
    pub client: Client,
    pub proxy_client: Client,

    // Config for negative caching TTL
    negative_cache_ttl: Duration,

    // In-process selection cursor for provider API key queue mode.
    provider_key_queue_state: Arc<tokio::sync::Mutex<HashMap<i64, usize>>>,
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
                info!(
                    "Redis is configured, but connection failed. Falling back to in-memory cache."
                );
            } else {
                info!("Using in-memory cache backend.");
            }
        }

        let pool = redis_pool.as_ref();

        let client = Self::build_http_client(false);

        let proxy_client = Self::build_http_client(true);

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
            client,
            proxy_client,
            negative_cache_ttl,
            provider_key_queue_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    fn build_http_client(use_proxy: bool) -> Client {
        let proxy_request_config = &CONFIG.proxy_request;
        let connect_timeout = proxy_request_config.connect_timeout();
        let request_timeout = proxy_request_config.request_timeout();

        let mut builder = Client::builder().connect_timeout(connect_timeout);

        if let Some(timeout) = request_timeout {
            builder = builder.timeout(timeout);
        }

        if use_proxy {
            if let Some(proxy_url) = &CONFIG.proxy {
                let proxy = Proxy::all(proxy_url).expect("Invalid proxy URL in configuration");
                builder = builder.proxy(proxy);
            }
        }

        info!(
            "Building reqwest client (use_proxy: {}, connect_timeout: {:?}, overall_timeout: {:?})",
            use_proxy, connect_timeout, request_timeout
        );

        builder.build().unwrap_or_else(|err| {
            panic!(
                "Failed to build {} reqwest client: {}",
                if use_proxy { "proxy" } else { "default" },
                err
            )
        })
    }

    fn create_repo<T>(ttl: Option<Duration>, pool: Option<&RedisPool>) -> CacheRepo<T>
    where
        T: Serialize
            + DeserializeOwned
            + Send
            + Sync
            + Clone
            + 'static
            + bincode::Encode
            + bincode::Decode<()>,
    {
        if let Some(pool) = pool {
            let redis_config = CONFIG
                .redis
                .as_ref()
                .expect("Redis config should exist if pool exists");
            let key_prefix = format!(
                "{}{}",
                redis_config.key_prefix, CONFIG.cache.redis.key_prefix
            );
            let backend = RedisCacheBackend::new(pool.clone(), key_prefix);
            Arc::new(CacheRepository::new(backend, ttl))
        } else {
            Arc::new(CacheRepository::new(MemoryCacheBackend::new(), ttl))
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
                let api_key_cache_key = CacheKey::SystemApiKey(&hashed_api_key).to_compact_string();
                let _ = self
                    .system_api_key_cache
                    .set_positive(&api_key_cache_key, &cache_item)
                    .await;
            }
        }

        // 3, 4. Providers
        let mut provider_id_to_key: HashMap<i64, String> = HashMap::new();
        if let Ok(providers) = Provider::list_all() {
            stats.insert("Providers", providers.len());
            for provider in providers {
                provider_id_to_key.insert(provider.id, provider.provider_key.clone());
                let cache_item = CacheProvider::from(provider);
                let _ = self
                    .provider_cache
                    .set_positive(
                        &CacheKey::ProviderById(cache_item.id).to_compact_string(),
                        &cache_item,
                    )
                    .await;
                let _ = self
                    .provider_cache
                    .set_positive(
                        &CacheKey::ProviderByKey(&cache_item.provider_key).to_compact_string(),
                        &cache_item,
                    )
                    .await;
            }
        }

        // 5. Models
        if let Ok(models) = Model::list_all() {
            stats.insert("Models", models.len());
            for model in models {
                let cache_item = CacheModel::from(model);
                let _ = self
                    .model_cache
                    .set_positive(
                        &CacheKey::ModelById(cache_item.id).to_compact_string(),
                        &cache_item,
                    )
                    .await;
                if let Some(provider_key) = provider_id_to_key.get(&cache_item.provider_id) {
                    let _ = self
                        .model_cache
                        .set_positive(
                            &CacheKey::ModelByName(provider_key, &cache_item.model_name)
                                .to_compact_string(),
                            &cache_item,
                        )
                        .await;
                }
            }
        }

        // 2. Model Aliases
        if let Ok(aliases) = ModelAlias::list_all() {
            stats.insert("Model Aliases", aliases.len());
            for alias in aliases {
                let _ = self
                    .alias_to_model_id_cache
                    .set_positive(
                        &CacheKey::ModelAlias(&alias.alias_name).to_compact_string(),
                        &alias.target_model_id,
                    )
                    .await;
            }
        }

        // 6. Access Control Policies
        if let Ok(policies) = DbAccessControlPolicy::list_all() {
            stats.insert("Access Control Policies", policies.len());
            for policy in policies {
                let cache_item = CacheAccessControl::from(policy);
                let _ = self
                    .access_control_policy_cache
                    .set_positive(
                        &CacheKey::AccessControlPolicy(cache_item.id).to_compact_string(),
                        &cache_item,
                    )
                    .await;
            }
        }

        // 7. Provider API Keys
        if let Ok(keys) = ProviderApiKey::list_all() {
            stats.insert("Provider API Keys", keys.len());
            let mut by_provider: HashMap<i64, Vec<CacheProviderKey>> = HashMap::new();
            for key in keys {
                by_provider
                    .entry(key.provider_id)
                    .or_default()
                    .push(CacheProviderKey::from(key));
            }
            stats.insert("Provider API Key Groups", by_provider.len());
            for (provider_id, provider_keys) in by_provider {
                let _ = self
                    .provider_api_keys_cache
                    .set_positive(
                        &CacheKey::ProviderApiKeys(provider_id).to_compact_string(),
                        &provider_keys,
                    )
                    .await;
            }
        }

        // 9. Custom Fields Definitions
        if let Ok(defs) = CustomFieldDefinition::list_all_active() {
            stats.insert("Custom Field Definitions", defs.len());
            for def in defs {
                let cache_item = CacheCustomField::from(def);
                let _ = self
                    .custom_field_cache
                    .set_positive(
                        &CacheKey::CustomField(cache_item.id).to_compact_string(),
                        &cache_item,
                    )
                    .await;
            }
        }

        // 8. Custom Field Assignments
        if let Ok(assignments) = CustomFieldDefinition::list_all_enabled_model_assignments() {
            let mut by_model: HashMap<i64, HashSet<i64>> = HashMap::new();
            for a in assignments {
                by_model
                    .entry(a.model_id)
                    .or_default()
                    .insert(a.custom_field_definition_id);
            }
            stats.insert("Model Custom Field Assignments", by_model.len());
            for (model_id, field_ids) in by_model {
                let _ = self
                    .custom_fields_assignment_cache
                    .set_positive(
                        &CacheKey::CustomFieldsAssignment(model_id).to_compact_string(),
                        &field_ids,
                    )
                    .await;
            }
        }
        if let Ok(assignments) = CustomFieldDefinition::list_all_enabled_provider_assignments() {
            let mut by_provider: HashMap<i64, HashSet<i64>> = HashMap::new();
            for a in assignments {
                by_provider
                    .entry(a.provider_id)
                    .or_default()
                    .insert(a.custom_field_definition_id);
            }
            stats.insert("Provider Custom Field Assignments", by_provider.len());
            for (provider_id, field_ids) in by_provider {
                let _ = self
                    .custom_fields_assignment_cache
                    .set_positive(
                        &CacheKey::CustomFieldsAssignment(provider_id).to_compact_string(),
                        &field_ids,
                    )
                    .await;
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
                let _ = self
                    .billing_plan_cache
                    .set_positive(
                        &CacheKey::BillingPlan(cache_item.id).to_compact_string(),
                        &cache_item,
                    )
                    .await;
            }
        }

        info!(
            "AppState reloaded successfully. Cache details:\n{:#?}",
            stats
        );
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

    async fn get_or_load<T, F, Fut>(
        &self,
        cache: &CacheRepo<T>,
        key: &str,
        loader: F,
    ) -> Result<Option<Arc<T>>, AppStoreError>
    where
        T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Option<T>, AppStoreError>>,
    {
        if let Some(entry) = cache.get_entry(key).await? {
            return match &*entry {
                CacheEntry::Positive(value) => {
                    debug!("cache hit (positive): {}", key);
                    Ok(Some(value.clone()))
                }
                CacheEntry::Negative => {
                    debug!("cache hit (negative): {}", key);
                    Ok(None)
                }
            };
        }

        debug!("cache miss: {}", key);
        match loader().await {
            Ok(Some(item)) => {
                let arc_item = Arc::new(item);
                cache.set_positive(key, &*arc_item).await?;
                Ok(Some(arc_item))
            }
            Ok(None) => {
                cache.set_negative(key, self.negative_cache_ttl).await?;
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    // ============================================================================================
    // 1. api_key(key) -> CacheSystemApiKey
    // ============================================================================================
    pub async fn get_system_api_key(
        &self,
        key: &str,
    ) -> Result<Option<Arc<CacheSystemApiKey>>, AppStoreError> {
        let hashed_key = Self::hash_api_key(key);
        let cache_key = CacheKey::SystemApiKey(&hashed_key).to_compact_string();

        self.get_or_load(&self.system_api_key_cache, &cache_key, || async {
            if let Ok(db_key) = SystemApiKey::get_by_key(key) {
                Ok(Some(CacheSystemApiKey::from(db_key)))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub async fn invalidate_system_api_key(&self, key: &str) -> Result<(), AppStoreError> {
        let hashed_key_to_find = Self::hash_api_key(key);
        let cache_key_to_find = CacheKey::SystemApiKey(&hashed_key_to_find).to_compact_string();
        debug!("invalidate: {}", &cache_key_to_find);
        self.system_api_key_cache.delete(&cache_key_to_find).await?;

        Ok(())
    }

    // ============================================================================================
    // 2. alias_name(key) -> CacheModel
    // ============================================================================================
    pub async fn get_model_by_alias(
        &self,
        alias: &str,
    ) -> Result<Option<Arc<CacheModel>>, AppStoreError> {
        let alias_key = CacheKey::ModelAlias(alias).to_compact_string();

        let model_id_arc = self
            .get_or_load(&self.alias_to_model_id_cache, &alias_key, || async {
                if let Ok(Some(db_alias)) = ModelAlias::get_by_alias_name(alias) {
                    Ok(Some(db_alias.target_model_id))
                } else {
                    Ok(None)
                }
            })
            .await?;

        match model_id_arc {
            Some(arc) => self.get_model_by_id(*arc).await,
            None => Ok(None),
        }
    }

    pub async fn invalidate_model_alias(&self, alias: &str) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ModelAlias(alias).to_compact_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.alias_to_model_id_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 3. provider_id(id) -> CacheProvider
    // ============================================================================================
    pub async fn get_provider_by_id(
        &self,
        id: i64,
    ) -> Result<Option<Arc<CacheProvider>>, AppStoreError> {
        let cache_key = CacheKey::ProviderById(id).to_compact_string();

        self.get_or_load(&self.provider_cache, &cache_key, || async {
            if let Ok(db_provider) = Provider::get_by_id(id) {
                let cache_item = CacheProvider::from(db_provider.clone());
                self.provider_cache
                    .set_positive(
                        &CacheKey::ProviderByKey(&db_provider.provider_key).to_compact_string(),
                        &cache_item,
                    )
                    .await?;
                Ok(Some(cache_item))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub async fn invalidate_provider_by_id(&self, id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ProviderById(id).to_compact_string();
        debug!("invalidate: {}", &cache_key);
        if let Some(provider) = self.get_provider_by_id(id).await? {
            let _ = self
                .invalidate_provider_by_key(&provider.provider_key)
                .await;
        }
        Ok(self.provider_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 4. provider_key(key) -> CacheProvider
    // ============================================================================================
    pub async fn get_provider_by_key(
        &self,
        key: &str,
    ) -> Result<Option<Arc<CacheProvider>>, AppStoreError> {
        let cache_key = CacheKey::ProviderByKey(key).to_compact_string();

        self.get_or_load(&self.provider_cache, &cache_key, || async {
            if let Ok(Some(db_provider)) = Provider::get_by_key(key) {
                let cache_item = CacheProvider::from(db_provider.clone());
                self.provider_cache
                    .set_positive(
                        &CacheKey::ProviderById(db_provider.id).to_compact_string(),
                        &cache_item,
                    )
                    .await?;
                Ok(Some(cache_item))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub async fn invalidate_provider_by_key(&self, key: &str) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ProviderByKey(key).to_compact_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.provider_cache.delete(&cache_key).await?)
    }

    pub async fn invalidate_provider(
        &self,
        id: i64,
        key: Option<&str>,
    ) -> Result<(), AppStoreError> {
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
    pub async fn get_model_by_name(
        &self,
        provider_key: &str,
        model_name: &str,
    ) -> Result<Option<Arc<CacheModel>>, AppStoreError> {
        let cache_key = CacheKey::ModelByName(provider_key, model_name).to_compact_string();

        self.get_or_load(&self.model_cache, &cache_key, || async {
            if let Some(provider) = self.get_provider_by_key(provider_key).await? {
                if let Ok(Some(db_model)) =
                    Model::get_by_name_and_provider_id(model_name, provider.id)
                {
                    let cache_item = CacheModel::from(db_model.clone());
                    self.model_cache
                        .set_positive(
                            &CacheKey::ModelById(db_model.id).to_compact_string(),
                            &cache_item,
                        )
                        .await?;
                    return Ok(Some(cache_item));
                }
            }
            Ok(None)
        })
        .await
    }

    // Internal helper for alias resolution + Lazy load by ID
    pub async fn get_model_by_id(&self, id: i64) -> Result<Option<Arc<CacheModel>>, AppStoreError> {
        let cache_key = CacheKey::ModelById(id).to_compact_string();

        self.get_or_load(&self.model_cache, &cache_key, || async {
            if let Ok(db_model) = Model::get_by_id(id) {
                let cache_item = CacheModel::from(db_model.clone());
                if let Ok(Some(provider)) = self.get_provider_by_id(db_model.provider_id).await {
                    self.model_cache
                        .set_positive(
                            &CacheKey::ModelByName(&provider.provider_key, &db_model.model_name)
                                .to_compact_string(),
                            &cache_item,
                        )
                        .await?;
                }
                Ok(Some(cache_item))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub async fn invalidate_model_by_name(
        &self,
        provider_key: &str,
        model_name: &str,
    ) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ModelByName(provider_key, model_name).to_compact_string();
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
                let _ = self
                    .invalidate_model_by_name(&p.provider_key, &m.model_name)
                    .await;
            }
        }
        Ok(self
            .model_cache
            .delete(&CacheKey::ModelById(id).to_compact_string())
            .await?)
    }

    // ============================================================================================
    // 6. access_control_policy_id(id) -> CacheAccessControl
    // ============================================================================================
    pub async fn get_access_control_policy(
        &self,
        id: i64,
    ) -> Result<Option<Arc<CacheAccessControl>>, AppStoreError> {
        let cache_key = CacheKey::AccessControlPolicy(id).to_compact_string();

        self.get_or_load(&self.access_control_policy_cache, &cache_key, || async {
            if let Ok(db_policy) = DbAccessControlPolicy::get_by_id(id) {
                Ok(Some(CacheAccessControl::from(db_policy)))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub async fn invalidate_access_control_policy(&self, id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::AccessControlPolicy(id).to_compact_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.access_control_policy_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 7. provider_id(id) -> CacheProviderKey[]
    // ============================================================================================
    pub async fn get_provider_api_keys(
        &self,
        provider_id: i64,
    ) -> Result<Arc<Vec<CacheProviderKey>>, AppStoreError> {
        let cache_key = CacheKey::ProviderApiKeys(provider_id).to_compact_string();

        let arc_list = self
            .get_or_load(&self.provider_api_keys_cache, &cache_key, || async {
                if let Ok(db_keys) = ProviderApiKey::list_by_provider_id(provider_id) {
                    Ok(Some(
                        db_keys.into_iter().map(CacheProviderKey::from).collect(),
                    ))
                } else {
                    Ok(None)
                }
            })
            .await?;

        Ok(arc_list.unwrap_or_else(|| Arc::new(Vec::new())))
    }

    pub async fn get_one_provider_api_key_by_provider(
        &self,
        provider_id: i64,
        strategy: GroupItemSelectionStrategy,
    ) -> Result<Option<Arc<CacheProviderKey>>, AppStoreError> {
        let keys = self.get_provider_api_keys(provider_id).await?;

        match keys.len() {
            0 => Ok(None),
            1 => Ok(keys.first().cloned().map(Arc::new)),
            _ => match strategy {
                GroupItemSelectionStrategy::Queue => {
                    let index = self.next_queue_index(provider_id, keys.len()).await;
                    Ok(keys.get(index).cloned().map(Arc::new))
                }
                GroupItemSelectionStrategy::Random => {
                    let index = Self::random_index(keys.len());
                    Ok(keys.get(index).cloned().map(Arc::new))
                }
            },
        }
    }

    pub async fn invalidate_provider_api_keys(
        &self,
        provider_id: i64,
    ) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ProviderApiKeys(provider_id).to_compact_string();
        debug!("invalidate: {}", &cache_key);
        self.provider_key_queue_state
            .lock()
            .await
            .remove(&provider_id);
        Ok(self.provider_api_keys_cache.delete(&cache_key).await?)
    }

    async fn next_queue_index(&self, provider_id: i64, key_count: usize) -> usize {
        let mut state = self.provider_key_queue_state.lock().await;
        let index = Self::advance_queue_cursor(&mut state, provider_id, key_count);
        debug!(
            "Selected provider API key by queue strategy: provider_id={}, key_count={}, index={}",
            provider_id, key_count, index
        );
        index
    }

    fn advance_queue_cursor(
        state: &mut HashMap<i64, usize>,
        provider_id: i64,
        key_count: usize,
    ) -> usize {
        let next_slot = state.entry(provider_id).or_insert(0);
        let selected_index = *next_slot % key_count;
        *next_slot = (selected_index + 1) % key_count;
        selected_index
    }

    fn random_index(key_count: usize) -> usize {
        rng().random_range(0..key_count)
    }

    // ============================================================================================
    // 8. entity_id(id) -> custom_field_definition_id Set
    // ============================================================================================
    pub async fn get_model_custom_field_ids(
        &self,
        model_id: i64,
    ) -> Result<Option<Arc<HashSet<i64>>>, AppStoreError> {
        let cache_key = CacheKey::CustomFieldsAssignment(model_id).to_compact_string();

        self.get_or_load(&self.custom_fields_assignment_cache, &cache_key, || async {
            match CustomFieldDefinition::list_enabled_model_assignments_by_model_id(model_id) {
                Ok(assignments) if !assignments.is_empty() => {
                    let field_ids: HashSet<i64> = assignments
                        .into_iter()
                        .map(|a| a.custom_field_definition_id)
                        .collect();
                    Ok(Some(field_ids))
                }
                _ => Ok(None),
            }
        })
        .await
    }

    pub async fn get_provider_custom_field_ids(
        &self,
        provider_id: i64,
    ) -> Result<Option<Arc<HashSet<i64>>>, AppStoreError> {
        let cache_key = CacheKey::CustomFieldsAssignment(provider_id).to_compact_string();

        self.get_or_load(&self.custom_fields_assignment_cache, &cache_key, || async {
            match CustomFieldDefinition::list_enabled_provider_assignments_by_provider_id(
                provider_id,
            ) {
                Ok(assignments) if !assignments.is_empty() => {
                    let field_ids: HashSet<i64> = assignments
                        .into_iter()
                        .map(|a| a.custom_field_definition_id)
                        .collect();
                    Ok(Some(field_ids))
                }
                _ => Ok(None),
            }
        })
        .await
    }

    pub async fn get_custom_fields_by_model_id(
        &self,
        model_id: i64,
    ) -> Result<Vec<Arc<CacheCustomField>>, AppStoreError> {
        match self.get_model_custom_field_ids(model_id).await? {
            Some(ids) if !ids.is_empty() => self.get_custom_fields(&ids).await,
            _ => Ok(Vec::new()),
        }
    }

    pub async fn get_custom_fields_by_provider_id(
        &self,
        provider_id: i64,
    ) -> Result<Vec<Arc<CacheCustomField>>, AppStoreError> {
        match self.get_provider_custom_field_ids(provider_id).await? {
            Some(ids) if !ids.is_empty() => self.get_custom_fields(&ids).await,
            _ => Ok(Vec::new()),
        }
    }

    pub async fn invalidate_model_custom_fields(&self, model_id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::CustomFieldsAssignment(model_id).to_compact_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self
            .custom_fields_assignment_cache
            .delete(&cache_key)
            .await?)
    }

    pub async fn invalidate_provider_custom_fields(
        &self,
        provider_id: i64,
    ) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::CustomFieldsAssignment(provider_id).to_compact_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self
            .custom_fields_assignment_cache
            .delete(&cache_key)
            .await?)
    }

    // ============================================================================================
    // 9. custom_field_definition_id(id) Set -> CacheCustomField[]
    // ============================================================================================
    pub async fn get_custom_fields(
        &self,
        ids: &HashSet<i64>,
    ) -> Result<Vec<Arc<CacheCustomField>>, AppStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let keys: Vec<compact_str::CompactString> = ids
            .iter()
            .map(|id| CacheKey::CustomField(*id).to_compact_string())
            .collect();
        let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();

        let results = self.custom_field_cache.mget(&key_refs).await?;

        let mut final_fields = Vec::new();
        for (id, res) in ids.iter().zip(results.into_iter()) {
            match res {
                Some(field) => final_fields.push(field),
                None => {
                    let id_val = *id;
                    debug!("cache miss for custom field {}, fetching from DB", id_val);
                    if let Ok(field_db) = CustomFieldDefinition::get_by_id(id_val) {
                        let cache_item = Arc::new(CacheCustomField::from(
                            CustomFieldDefinition::from(field_db),
                        ));
                        let _ = self
                            .custom_field_cache
                            .set_positive(
                                &CacheKey::CustomField(id_val).to_compact_string(),
                                &cache_item,
                            )
                            .await;
                        final_fields.push(cache_item);
                    } else {
                        error!(
                            "Failed to fetch custom field {} from DB after cache miss",
                            id_val
                        );
                    }
                }
            }
        }

        Ok(final_fields)
    }

    pub async fn invalidate_custom_field(&self, id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::CustomField(id).to_compact_string();
        debug!("invalidate: {}", &cache_key);
        Ok(self.custom_field_cache.delete(&cache_key).await?)
    }

    // ============================================================================================
    // 10. billing_plan_id(id) -> CacheBillingPlan
    // ============================================================================================
    pub async fn get_billing_plan_by_id(
        &self,
        id: i64,
    ) -> Result<Option<Arc<CacheBillingPlan>>, AppStoreError> {
        let cache_key = CacheKey::BillingPlan(id).to_compact_string();

        self.get_or_load(&self.billing_plan_cache, &cache_key, || async {
            match BillingPlan::get_by_id(id) {
                Ok(plan) => {
                    let rules = PriceRule::list_by_plan_id(id).unwrap_or_else(|_| {
                        cyder_tools::log::warn!("Failed to load price rules for plan_id: {}", id);
                        Vec::new()
                    });
                    Ok(Some(CacheBillingPlan::from_db_with_rules(plan, rules)))
                }
                Err(_) => Ok(None),
            }
        })
        .await
    }

    pub async fn invalidate_billing_plan(&self, id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::BillingPlan(id).to_compact_string();
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

#[cfg(test)]
mod tests {
    use super::{AppState, GroupItemSelectionStrategy};
    use crate::schema::enum_def::ProviderApiKeyMode;
    use std::collections::HashMap;

    #[test]
    fn queue_strategy_advances_and_wraps() {
        let mut state = HashMap::new();

        assert_eq!(AppState::advance_queue_cursor(&mut state, 42, 3), 0);
        assert_eq!(AppState::advance_queue_cursor(&mut state, 42, 3), 1);
        assert_eq!(AppState::advance_queue_cursor(&mut state, 42, 3), 2);
        assert_eq!(AppState::advance_queue_cursor(&mut state, 42, 3), 0);
    }

    #[test]
    fn queue_strategy_handles_key_count_changes() {
        let mut state = HashMap::new();

        assert_eq!(AppState::advance_queue_cursor(&mut state, 7, 4), 0);
        assert_eq!(AppState::advance_queue_cursor(&mut state, 7, 4), 1);
        assert_eq!(AppState::advance_queue_cursor(&mut state, 7, 2), 0);
        assert_eq!(AppState::advance_queue_cursor(&mut state, 7, 2), 1);
    }

    #[test]
    fn provider_api_key_mode_maps_to_runtime_strategy() {
        assert_eq!(
            GroupItemSelectionStrategy::from(ProviderApiKeyMode::Queue),
            GroupItemSelectionStrategy::Queue
        );
        assert_eq!(
            GroupItemSelectionStrategy::from(ProviderApiKeyMode::Random),
            GroupItemSelectionStrategy::Random
        );
    }
}
