use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::config::{CONFIG, CacheBackendType};
use crate::controller::BaseError;
use crate::database::api_key::ApiKey;
use crate::database::cost::{CostCatalogVersion, CostComponent};
use crate::database::model::Model;
use crate::database::model_route::{ApiKeyModelOverride, ModelRoute};
use crate::database::provider::{Provider, ProviderApiKey};
use crate::database::reasoning_config::ReasoningConfig;
use crate::database::request_patch::RequestPatchRule;
use crate::database::runtime_feature_config::RuntimeFeatureConfig;
use crate::service::app_state::AppStoreError;
use crate::service::cache::memory::MemoryCacheBackend;
use crate::service::cache::redis::RedisCacheBackend;
use crate::service::cache::repository::{CacheRepository, DynCacheRepo};
use crate::service::cache::types::{
    CacheApiKey, CacheApiKeyModelOverride, CacheCostCatalogVersion, CacheEntry, CacheModel,
    CacheModelRoute, CacheModelsCatalog, CacheProvider, CacheProviderKey, CacheReasoningConfig,
    CacheRequestPatchRule, CacheResolvedModelRequestPatches, CacheRuntimeFeatureConfig,
};
use crate::service::redis::{self, RedisPool};
use crate::service::request_patch::resolve_effective_request_patches;

use super::keys::CacheKey;
use super::reload::{
    cache_backend_name, increment_failure_counter, summarize_failures, summarize_repo_names,
};

type CacheRepo<T> = Arc<dyn DynCacheRepo<T>>;
type ProviderApiKeysInvalidationHook =
    Arc<dyn Fn(i64) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CatalogCacheBackendStatus {
    pub configured_backend: CacheBackendType,
    pub effective_backend: CacheBackendType,
    pub fallback_reason: Option<String>,
}

pub struct CatalogService {
    api_key_cache: CacheRepo<CacheApiKey>,
    model_route_cache: CacheRepo<CacheModelRoute>,
    api_key_override_route_cache: CacheRepo<CacheModelRoute>,
    models_catalog_cache: CacheRepo<CacheModelsCatalog>,
    provider_cache: CacheRepo<CacheProvider>,
    model_cache: CacheRepo<CacheModel>,
    provider_api_keys_cache: CacheRepo<Vec<CacheProviderKey>>,
    provider_request_patch_rules_cache: CacheRepo<Vec<CacheRequestPatchRule>>,
    model_request_patch_rules_cache: CacheRepo<Vec<CacheRequestPatchRule>>,
    model_effective_request_patches_cache: CacheRepo<CacheResolvedModelRequestPatches>,
    cost_catalog_version_cache: CacheRepo<CacheCostCatalogVersion>,
    backend_status: CatalogCacheBackendStatus,
    negative_cache_ttl: Duration,
    provider_api_keys_invalidation_hook:
        tokio::sync::RwLock<Option<ProviderApiKeysInvalidationHook>>,
}

impl CatalogService {
    pub async fn new(force_memory_cache: bool) -> Self {
        let negative_cache_ttl = CONFIG.cache.catalog_negative_ttl();
        let ttl = Some(CONFIG.cache.catalog_ttl());
        let redis_pool = if force_memory_cache {
            None
        } else {
            redis::get_pool().await
        };
        let configured_backend = CONFIG.cache.catalog_backend();
        let backend_status = select_catalog_cache_backend_status(
            configured_backend.clone(),
            force_memory_cache,
            CONFIG.redis.is_some(),
            redis_pool.is_some(),
        );
        let use_redis = backend_status.effective_backend == CacheBackendType::Redis;

        crate::info_event!(
            "cache.catalog.backend_selected",
            configured_backend = cache_backend_name(backend_status.configured_backend.clone()),
            effective_backend = cache_backend_name(backend_status.effective_backend.clone()),
            fallback_reason = &backend_status.fallback_reason,
        );

        let pool = if use_redis { redis_pool.as_ref() } else { None };

        Self {
            api_key_cache: Self::create_repo(ttl, pool),
            model_route_cache: Self::create_repo(ttl, pool),
            api_key_override_route_cache: Self::create_repo(ttl, pool),
            models_catalog_cache: Self::create_repo(ttl, pool),
            provider_cache: Self::create_repo(ttl, pool),
            model_cache: Self::create_repo(ttl, pool),
            provider_api_keys_cache: Self::create_repo(ttl, pool),
            provider_request_patch_rules_cache: Self::create_repo(ttl, pool),
            model_request_patch_rules_cache: Self::create_repo(ttl, pool),
            model_effective_request_patches_cache: Self::create_repo(ttl, pool),
            cost_catalog_version_cache: Self::create_repo(ttl, pool),
            backend_status,
            negative_cache_ttl,
            provider_api_keys_invalidation_hook: tokio::sync::RwLock::new(None),
        }
    }

    pub fn backend_status(&self) -> CatalogCacheBackendStatus {
        self.backend_status.clone()
    }

    pub(crate) async fn set_provider_api_keys_invalidation_hook(
        &self,
        hook: ProviderApiKeysInvalidationHook,
    ) {
        *self.provider_api_keys_invalidation_hook.write().await = Some(hook);
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
                redis_config.key_prefix,
                CONFIG.cache.catalog_redis_key_prefix()
            );
            let backend = RedisCacheBackend::new(pool.clone(), key_prefix);
            Arc::new(CacheRepository::new(backend, ttl))
        } else {
            Arc::new(CacheRepository::new(MemoryCacheBackend::new(), ttl))
        }
    }

    fn hash_api_key(key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn load_cache_api_key(row: ApiKey) -> Result<CacheApiKey, AppStoreError> {
        let acl_rules = ApiKey::load_acl_rules(row.id).map_err(|err| {
            AppStoreError::DatabaseError(format!(
                "failed to load api key ACL rules for {}: {:?}",
                row.id, err
            ))
        })?;

        Ok(CacheApiKey::from_db(row, acl_rules))
    }

    fn cache_request_patch_rules(
        rows: Vec<crate::database::request_patch::RequestPatchRuleResponse>,
    ) -> Result<Vec<CacheRequestPatchRule>, AppStoreError> {
        rows.into_iter()
            .map(|row| {
                CacheRequestPatchRule::try_from(row).map_err(|err| {
                    AppStoreError::DatabaseError(format!(
                        "failed to serialize request patch rule for cache: {}",
                        err
                    ))
                })
            })
            .collect()
    }

    async fn load_model_effective_request_patches(
        &self,
        model_id: i64,
    ) -> Result<Option<CacheResolvedModelRequestPatches>, AppStoreError> {
        let Some(model) = self.get_model_by_id(model_id).await? else {
            return Ok(None);
        };

        let provider_rules = self
            .get_provider_request_patch_rules(model.provider_id)
            .await?;
        let model_rules = self.get_model_request_patch_rules(model_id).await?;

        Ok(Some(resolve_effective_request_patches(
            model.provider_id,
            model_id,
            provider_rules.as_ref(),
            model_rules.as_ref(),
        )))
    }

    pub async fn reload(&self) {
        crate::info_event!("cache.reload_started");
        let mut failure_counts: HashMap<&'static str, usize> = HashMap::new();
        let mut catalog_providers = Vec::new();
        let mut catalog_models = Vec::new();
        let mut catalog_routes = Vec::new();
        let mut catalog_api_key_overrides = Vec::new();
        let mut catalog_reasoning_configs = Vec::new();
        let mut catalog_runtime_feature_configs = Vec::new();
        let mut api_key_count = 0usize;
        let mut provider_count = 0usize;
        let mut model_count = 0usize;
        let mut model_route_count = 0usize;
        let mut api_key_override_count = 0usize;
        let mut reasoning_config_count = 0usize;
        let mut runtime_feature_config_count = 0usize;
        let mut provider_api_key_count = 0usize;
        let mut provider_api_key_group_count = 0usize;
        let mut request_patch_rule_count = 0usize;
        let mut provider_request_patch_group_count = 0usize;
        let mut model_request_patch_group_count = 0usize;
        let mut model_effective_request_patch_group_count = 0usize;
        let mut cost_catalog_version_count = 0usize;

        match ApiKey::list_all_active() {
            Ok(keys) => {
                api_key_count = keys.len();
                let now = chrono::Utc::now().timestamp_millis();
                for key in keys {
                    match Self::load_cache_api_key(key) {
                        Ok(cache_item) if cache_item.is_active_at(now) => {
                            let api_key_cache_key =
                                CacheKey::ApiKeyHash(&cache_item.api_key_hash).to_compact_string();
                            let _ = self
                                .api_key_cache
                                .set_positive(&api_key_cache_key, &cache_item)
                                .await;
                        }
                        Ok(_) => {}
                        Err(_) => {
                            increment_failure_counter(&mut failure_counts, "api_key_snapshot");
                        }
                    }
                }
            }
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "api_key_list");
            }
        }

        let mut provider_id_to_key: HashMap<i64, String> = HashMap::new();
        match Provider::list_all() {
            Ok(providers) => {
                provider_count = providers.len();
                for provider in providers {
                    provider_id_to_key.insert(provider.id, provider.provider_key.clone());
                    let cache_item = CacheProvider::from(provider);
                    catalog_providers.push(cache_item.clone());
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
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "provider_list");
            }
        }

        match Model::list_all() {
            Ok(models) => {
                model_count = models.len();
                for model in models {
                    let cache_item = CacheModel::from(model);
                    catalog_models.push(cache_item.clone());
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
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "model_list");
            }
        }

        match ModelRoute::list_summary() {
            Ok(routes) => {
                model_route_count = routes.len();
                for route_item in routes {
                    match ModelRoute::get_detail(route_item.route.id) {
                        Ok(route_detail) => {
                            let cache_item = CacheModelRoute::from_detail(&route_detail);
                            catalog_routes.push(cache_item.clone());
                            let _ = self
                                .model_route_cache
                                .set_positive(
                                    &CacheKey::ModelRouteById(cache_item.id).to_compact_string(),
                                    &cache_item,
                                )
                                .await;
                            let _ = self
                                .model_route_cache
                                .set_positive(
                                    &CacheKey::ModelRouteByName(&cache_item.route_name)
                                        .to_compact_string(),
                                    &cache_item,
                                )
                                .await;
                        }
                        Err(_) => {
                            increment_failure_counter(&mut failure_counts, "model_route_detail");
                        }
                    }
                }
            }
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "model_route_list");
            }
        }

        match ApiKeyModelOverride::list_all() {
            Ok(overrides) => {
                api_key_override_count = overrides.len();
                for override_row in overrides {
                    catalog_api_key_overrides
                        .push(CacheApiKeyModelOverride::from(override_row.clone()));

                    if !override_row.is_enabled {
                        continue;
                    }

                    match self
                        .get_model_route_by_id(override_row.target_route_id)
                        .await
                    {
                        Ok(Some(route)) => {
                            let _ = self
                                .api_key_override_route_cache
                                .set_positive(
                                    &CacheKey::ApiKeyModelOverride(
                                        override_row.api_key_id,
                                        &override_row.source_name,
                                    )
                                    .to_compact_string(),
                                    route.as_ref(),
                                )
                                .await;
                        }
                        Ok(None) => {}
                        Err(_) => {
                            increment_failure_counter(
                                &mut failure_counts,
                                "api_key_model_override_route",
                            );
                        }
                    }
                }
            }
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "api_key_model_override_list");
            }
        }

        match ReasoningConfig::list_active_with_presets() {
            Ok(configs) => {
                reasoning_config_count = configs.len();
                catalog_reasoning_configs
                    .extend(configs.into_iter().map(CacheReasoningConfig::from));
            }
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "reasoning_config_list");
            }
        }

        match RuntimeFeatureConfig::list_active() {
            Ok(configs) => {
                runtime_feature_config_count = configs.len();
                catalog_runtime_feature_configs
                    .extend(configs.into_iter().map(CacheRuntimeFeatureConfig::from));
            }
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "runtime_feature_config_list");
            }
        }

        let models_catalog = CacheModelsCatalog {
            providers: catalog_providers.clone(),
            models: catalog_models.clone(),
            routes: catalog_routes.clone(),
            api_key_overrides: catalog_api_key_overrides.clone(),
            reasoning_configs: catalog_reasoning_configs.clone(),
            runtime_feature_configs: catalog_runtime_feature_configs.clone(),
        };
        let _ = self
            .models_catalog_cache
            .set_positive(
                &CacheKey::ModelsCatalog.to_compact_string(),
                &models_catalog,
            )
            .await;

        match ProviderApiKey::list_all() {
            Ok(keys) => {
                provider_api_key_count = keys.len();
                let mut by_provider: HashMap<i64, Vec<CacheProviderKey>> = HashMap::new();
                for key in keys {
                    by_provider
                        .entry(key.provider_id)
                        .or_default()
                        .push(CacheProviderKey::from(key));
                }
                provider_api_key_group_count = by_provider.len();
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
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "provider_api_key_list");
            }
        }

        match RequestPatchRule::list_all() {
            Ok(all_rules) => {
                request_patch_rule_count = all_rules.len();

                let mut provider_rules_by_id: HashMap<i64, Vec<CacheRequestPatchRule>> =
                    HashMap::new();
                let mut model_rules_by_id: HashMap<i64, Vec<CacheRequestPatchRule>> =
                    HashMap::new();

                match Self::cache_request_patch_rules(all_rules) {
                    Ok(cache_rules) => {
                        for rule in cache_rules {
                            if let Some(provider_id) = rule.provider_id {
                                provider_rules_by_id
                                    .entry(provider_id)
                                    .or_default()
                                    .push(rule.clone());
                            }
                            if let Some(model_id) = rule.model_id {
                                model_rules_by_id.entry(model_id).or_default().push(rule);
                            }
                        }
                    }
                    Err(_) => {
                        increment_failure_counter(&mut failure_counts, "request_patch_materialize");
                    }
                }

                provider_request_patch_group_count = provider_rules_by_id.len();
                for provider in &catalog_providers {
                    let rules = provider_rules_by_id
                        .get(&provider.id)
                        .cloned()
                        .unwrap_or_default();
                    let _ = self
                        .provider_request_patch_rules_cache
                        .set_positive(
                            &CacheKey::ProviderRequestPatchRules(provider.id).to_compact_string(),
                            &rules,
                        )
                        .await;
                }

                model_request_patch_group_count = model_rules_by_id.len();
                for model in &catalog_models {
                    let rules = model_rules_by_id.remove(&model.id).unwrap_or_default();
                    let _ = self
                        .model_request_patch_rules_cache
                        .set_positive(
                            &CacheKey::ModelRequestPatchRules(model.id).to_compact_string(),
                            &rules,
                        )
                        .await;

                    let resolved = resolve_effective_request_patches(
                        model.provider_id,
                        model.id,
                        provider_rules_by_id
                            .get(&model.provider_id)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]),
                        &rules,
                    );
                    let _ = self
                        .model_effective_request_patches_cache
                        .set_positive(
                            &CacheKey::ModelEffectiveRequestPatches(model.id).to_compact_string(),
                            &resolved,
                        )
                        .await;
                }
                model_effective_request_patch_group_count = catalog_models.len();
            }
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "request_patch_list");
            }
        }

        match CostCatalogVersion::list_all() {
            Ok(versions) => {
                cost_catalog_version_count = versions.len();
                let mut components_by_version: HashMap<i64, Vec<CostComponent>> = HashMap::new();
                match CostComponent::list_all() {
                    Ok(components) => {
                        for component in components {
                            components_by_version
                                .entry(component.catalog_version_id)
                                .or_default()
                                .push(component);
                        }
                    }
                    Err(_) => {
                        increment_failure_counter(&mut failure_counts, "cost_component_list");
                    }
                }

                for version in versions {
                    let components = components_by_version
                        .remove(&version.id)
                        .unwrap_or_default();
                    let cache_item =
                        CacheCostCatalogVersion::from_db_with_components(version, components);
                    let _ = self
                        .cost_catalog_version_cache
                        .set_positive(
                            &CacheKey::CostCatalogVersion(cache_item.id).to_compact_string(),
                            &cache_item,
                        )
                        .await;
                }
            }
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "cost_catalog_version_list");
            }
        }

        let failure_summary = summarize_failures(&failure_counts);
        if failure_counts.is_empty() {
            crate::info_event!(
                "cache.reload_finished",
                status = "success",
                api_key_count = api_key_count,
                provider_count = provider_count,
                model_count = model_count,
                model_route_count = model_route_count,
                api_key_override_count = api_key_override_count,
                reasoning_config_count = reasoning_config_count,
                runtime_feature_config_count = runtime_feature_config_count,
                provider_api_key_count = provider_api_key_count,
                provider_api_key_group_count = provider_api_key_group_count,
                request_patch_rule_count = request_patch_rule_count,
                provider_request_patch_group_count = provider_request_patch_group_count,
                model_request_patch_group_count = model_request_patch_group_count,
                model_effective_request_patch_group_count =
                    model_effective_request_patch_group_count,
                cost_catalog_version_count = cost_catalog_version_count,
                failed_group_count = 0usize,
            );
        } else {
            crate::warn_event!(
                "cache.reload_finished",
                status = "partial_failure",
                api_key_count = api_key_count,
                provider_count = provider_count,
                model_count = model_count,
                model_route_count = model_route_count,
                api_key_override_count = api_key_override_count,
                reasoning_config_count = reasoning_config_count,
                runtime_feature_config_count = runtime_feature_config_count,
                provider_api_key_count = provider_api_key_count,
                provider_api_key_group_count = provider_api_key_group_count,
                request_patch_rule_count = request_patch_rule_count,
                provider_request_patch_group_count = provider_request_patch_group_count,
                model_request_patch_group_count = model_request_patch_group_count,
                model_effective_request_patch_group_count =
                    model_effective_request_patch_group_count,
                cost_catalog_version_count = cost_catalog_version_count,
                failed_group_count = failure_counts.len(),
                failed_groups = failure_summary.as_deref(),
            );
        }
    }

    pub async fn clear_cache(&self) {
        crate::info_event!("cache.clear_started");

        let mut failed_repos = Vec::new();

        if self.api_key_cache.clear().await.is_err() {
            failed_repos.push("api_key_cache");
        }
        if self.model_route_cache.clear().await.is_err() {
            failed_repos.push("model_route_cache");
        }
        if self.api_key_override_route_cache.clear().await.is_err() {
            failed_repos.push("api_key_override_route_cache");
        }
        if self.models_catalog_cache.clear().await.is_err() {
            failed_repos.push("models_catalog_cache");
        }
        if self.provider_cache.clear().await.is_err() {
            failed_repos.push("provider_cache");
        }
        if self.model_cache.clear().await.is_err() {
            failed_repos.push("model_cache");
        }
        if self.provider_api_keys_cache.clear().await.is_err() {
            failed_repos.push("provider_api_keys_cache");
        }
        if self
            .provider_request_patch_rules_cache
            .clear()
            .await
            .is_err()
        {
            failed_repos.push("provider_request_patch_rules_cache");
        }
        if self.model_request_patch_rules_cache.clear().await.is_err() {
            failed_repos.push("model_request_patch_rules_cache");
        }
        if self
            .model_effective_request_patches_cache
            .clear()
            .await
            .is_err()
        {
            failed_repos.push("model_effective_request_patches_cache");
        }
        if self.cost_catalog_version_cache.clear().await.is_err() {
            failed_repos.push("cost_catalog_version_cache");
        }

        let total_repo_count = 11usize;
        let failed_repo_count = failed_repos.len();
        let failed_repo_summary = summarize_repo_names(&failed_repos);

        if failed_repo_count == 0 {
            crate::info_event!(
                "cache.clear_finished",
                status = "success",
                repo_count = total_repo_count,
                failed_repo_count = failed_repo_count,
            );
        } else if failed_repo_count < total_repo_count {
            crate::warn_event!(
                "cache.clear_finished",
                status = "partial_failure",
                repo_count = total_repo_count,
                failed_repo_count = failed_repo_count,
                failed_repos = failed_repo_summary.as_deref(),
            );
        } else {
            crate::error_event!(
                "cache.clear_finished",
                status = "failed",
                repo_count = total_repo_count,
                failed_repo_count = failed_repo_count,
                failed_repos = failed_repo_summary.as_deref(),
            );
        }
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
                CacheEntry::Positive(value) => Ok(Some(value.clone())),
                CacheEntry::Negative => Ok(None),
            };
        }

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

    pub async fn get_api_key(&self, key: &str) -> Result<Option<Arc<CacheApiKey>>, AppStoreError> {
        let hashed_key = Self::hash_api_key(key);
        let cache_key = CacheKey::ApiKeyHash(&hashed_key).to_compact_string();
        let now = chrono::Utc::now().timestamp_millis();

        let result = self
            .get_or_load(&self.api_key_cache, &cache_key, || async {
                match ApiKey::get_active_by_hash(&hashed_key) {
                    Ok(db_key) => Ok(Some(Self::load_cache_api_key(db_key)?)),
                    Err(BaseError::NotFound(_)) => Ok(None),
                    Err(err) => Err(AppStoreError::DatabaseError(format!(
                        "failed to load api key by hash: {:?}",
                        err
                    ))),
                }
            })
            .await?;

        if let Some(api_key) = result {
            if api_key.is_active_at(now) {
                return Ok(Some(api_key));
            }

            self.api_key_cache.delete(&cache_key).await?;
            return Ok(None);
        }

        Ok(None)
    }

    pub async fn get_api_key_by_hash(
        &self,
        api_key_hash: &str,
    ) -> Result<Option<Arc<CacheApiKey>>, AppStoreError> {
        let cache_key = CacheKey::ApiKeyHash(api_key_hash).to_compact_string();
        let now = chrono::Utc::now().timestamp_millis();

        let result = self
            .get_or_load(&self.api_key_cache, &cache_key, || async {
                match ApiKey::get_active_by_hash(api_key_hash) {
                    Ok(db_key) => Ok(Some(Self::load_cache_api_key(db_key)?)),
                    Err(BaseError::NotFound(_)) => Ok(None),
                    Err(err) => Err(AppStoreError::DatabaseError(format!(
                        "failed to load api key by hash: {:?}",
                        err
                    ))),
                }
            })
            .await?;

        if let Some(api_key) = result {
            if api_key.is_active_at(now) {
                return Ok(Some(api_key));
            }

            self.api_key_cache.delete(&cache_key).await?;
            return Ok(None);
        }

        Ok(None)
    }

    pub async fn invalidate_api_key_hash(&self, api_key_hash: &str) -> Result<(), AppStoreError> {
        let cache_key_to_find = CacheKey::ApiKeyHash(api_key_hash).to_compact_string();
        self.api_key_cache.delete(&cache_key_to_find).await?;
        Ok(())
    }

    pub async fn invalidate_api_key_id(&self, id: i64) -> Result<(), AppStoreError> {
        if let Ok(row) = ApiKey::get_by_id(id) {
            let api_key_hash = row
                .api_key_hash
                .unwrap_or_else(|| crate::database::api_key::hash_api_key(&row.api_key));
            self.invalidate_api_key_hash(&api_key_hash).await?;
        }

        Ok(())
    }

    pub async fn invalidate_api_key(&self, key: &str) -> Result<(), AppStoreError> {
        self.invalidate_api_key_hash(&Self::hash_api_key(key)).await
    }

    pub async fn get_model_route_by_id(
        &self,
        id: i64,
    ) -> Result<Option<Arc<CacheModelRoute>>, AppStoreError> {
        let cache_key = CacheKey::ModelRouteById(id).to_compact_string();

        self.get_or_load(&self.model_route_cache, &cache_key, || async {
            match ModelRoute::get_detail(id) {
                Ok(detail) => {
                    let cache_item = CacheModelRoute::from_detail(&detail);
                    self.model_route_cache
                        .set_positive(
                            &CacheKey::ModelRouteByName(&cache_item.route_name).to_compact_string(),
                            &cache_item,
                        )
                        .await?;
                    Ok(Some(cache_item))
                }
                Err(BaseError::NotFound(_)) => Ok(None),
                Err(err) => Err(AppStoreError::DatabaseError(format!(
                    "failed to load model route by id {}: {:?}",
                    id, err
                ))),
            }
        })
        .await
    }

    pub async fn get_model_route_by_name(
        &self,
        name: &str,
    ) -> Result<Option<Arc<CacheModelRoute>>, AppStoreError> {
        let cache_key = CacheKey::ModelRouteByName(name).to_compact_string();

        self.get_or_load(&self.model_route_cache, &cache_key, || async {
            match ModelRoute::get_active_by_name(name) {
                Ok(Some(route)) => {
                    let detail = ModelRoute::get_detail(route.id).map_err(|err| {
                        AppStoreError::DatabaseError(format!(
                            "failed to load model route detail {}: {:?}",
                            route.id, err
                        ))
                    })?;
                    let cache_item = CacheModelRoute::from_detail(&detail);
                    self.model_route_cache
                        .set_positive(
                            &CacheKey::ModelRouteById(cache_item.id).to_compact_string(),
                            &cache_item,
                        )
                        .await?;
                    Ok(Some(cache_item))
                }
                Ok(None) => Ok(None),
                Err(err) => Err(AppStoreError::DatabaseError(format!(
                    "failed to load model route by name '{}': {:?}",
                    name, err
                ))),
            }
        })
        .await
    }

    pub async fn get_api_key_override_route(
        &self,
        api_key_id: i64,
        source_name: &str,
    ) -> Result<Option<Arc<CacheModelRoute>>, AppStoreError> {
        let cache_key = CacheKey::ApiKeyModelOverride(api_key_id, source_name).to_compact_string();

        self.get_or_load(&self.api_key_override_route_cache, &cache_key, || async {
            match ApiKeyModelOverride::get_active_by_source_name(api_key_id, source_name) {
                Ok(Some(override_row)) => {
                    if !override_row.is_enabled {
                        return Ok(None);
                    }
                    let route = self
                        .get_model_route_by_id(override_row.target_route_id)
                        .await?
                        .map(|route| route.as_ref().clone());
                    Ok(route)
                }
                Ok(None) => Ok(None),
                Err(err) => Err(AppStoreError::DatabaseError(format!(
                    "failed to load api key override {}:{}: {:?}",
                    api_key_id, source_name, err
                ))),
            }
        })
        .await
    }

    pub async fn invalidate_model_route_by_name(&self, name: &str) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ModelRouteByName(name).to_compact_string();
        Ok(self.model_route_cache.delete(&cache_key).await?)
    }

    pub async fn invalidate_api_key_model_override(
        &self,
        api_key_id: i64,
        source_name: &str,
    ) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ApiKeyModelOverride(api_key_id, source_name).to_compact_string();
        self.invalidate_models_catalog().await?;
        Ok(self.api_key_override_route_cache.delete(&cache_key).await?)
    }

    pub async fn invalidate_api_key_model_overrides_by_route(
        &self,
        route_id: i64,
    ) -> Result<(), AppStoreError> {
        for override_row in
            ApiKeyModelOverride::list_by_target_route_id(route_id).map_err(|err| {
                AppStoreError::DatabaseError(format!(
                    "failed to list api key overrides for route {}: {:?}",
                    route_id, err
                ))
            })?
        {
            let _ = self
                .invalidate_api_key_model_override(
                    override_row.api_key_id,
                    &override_row.source_name,
                )
                .await;
        }

        Ok(())
    }

    pub async fn invalidate_model_route(
        &self,
        route_id: i64,
        route_name: Option<&str>,
    ) -> Result<(), AppStoreError> {
        self.invalidate_models_catalog().await?;
        if let Some(name) = route_name {
            let _ = self.invalidate_model_route_by_name(name).await;
        } else if let Some(route) = self.get_model_route_by_id(route_id).await? {
            let _ = self.invalidate_model_route_by_name(&route.route_name).await;
        }
        self.invalidate_api_key_model_overrides_by_route(route_id)
            .await?;
        Ok(self
            .model_route_cache
            .delete(&CacheKey::ModelRouteById(route_id).to_compact_string())
            .await?)
    }

    pub async fn invalidate_model_routes_for_model(
        &self,
        model_id: i64,
    ) -> Result<(), AppStoreError> {
        for route in ModelRoute::list_by_model_id(model_id).map_err(|err| {
            AppStoreError::DatabaseError(format!(
                "failed to list model routes for model {}: {:?}",
                model_id, err
            ))
        })? {
            let _ = self
                .invalidate_model_route(route.id, Some(&route.route_name))
                .await;
        }

        Ok(())
    }

    pub async fn invalidate_model_routes_for_provider(
        &self,
        provider_id: i64,
    ) -> Result<(), AppStoreError> {
        for route in ModelRoute::list_by_provider_id(provider_id).map_err(|err| {
            AppStoreError::DatabaseError(format!(
                "failed to list model routes for provider {}: {:?}",
                provider_id, err
            ))
        })? {
            let _ = self
                .invalidate_model_route(route.id, Some(&route.route_name))
                .await;
        }

        Ok(())
    }

    pub async fn get_models_catalog(&self) -> Result<Arc<CacheModelsCatalog>, AppStoreError> {
        let cache_key = CacheKey::ModelsCatalog.to_compact_string();

        let catalog = self
            .get_or_load(&self.models_catalog_cache, &cache_key, || async {
                Ok(Some(Self::load_models_catalog()?))
            })
            .await?;

        Ok(catalog.expect("models catalog loader always returns a value"))
    }

    pub async fn invalidate_models_catalog(&self) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ModelsCatalog.to_compact_string();
        Ok(self.models_catalog_cache.delete(&cache_key).await?)
    }

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
        if let Some(provider) = self.get_provider_by_id(id).await? {
            let _ = self
                .invalidate_provider_by_key(&provider.provider_key)
                .await;
        }
        Ok(self.provider_cache.delete(&cache_key).await?)
    }

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
        Ok(self.provider_cache.delete(&cache_key).await?)
    }

    pub async fn invalidate_provider(
        &self,
        id: i64,
        key: Option<&str>,
    ) -> Result<(), AppStoreError> {
        self.invalidate_models_catalog().await?;
        let _ = self.invalidate_model_routes_for_provider(id).await;
        let _ = self.invalidate_provider_request_patch_rules(id).await;
        if let Some(k) = key {
            let _ = self.invalidate_provider_by_key(k).await;
        } else if let Some(p) = self.get_provider_by_id(id).await? {
            let _ = self.invalidate_provider_by_key(&p.provider_key).await;
        }
        self.invalidate_provider_by_id(id).await
    }

    pub async fn invalidate_reasoning_provider_config(
        &self,
        provider_id: i64,
    ) -> Result<(), AppStoreError> {
        self.invalidate_models_catalog().await?;

        let provider_key = Provider::get_by_id(provider_id)
            .ok()
            .map(|provider| provider.provider_key);
        if let Some(key) = provider_key.as_deref() {
            let _ = self.invalidate_provider_by_key(key).await;
        }
        let _ = self
            .provider_cache
            .delete(&CacheKey::ProviderById(provider_id).to_compact_string())
            .await;

        for model in Model::list_by_provider_id(provider_id).map_err(|err| {
            AppStoreError::DatabaseError(format!(
                "failed to list models for provider reasoning config invalidation {}: {:?}",
                provider_id, err
            ))
        })? {
            let _ = self
                .model_cache
                .delete(&CacheKey::ModelById(model.id).to_compact_string())
                .await;
            if let Some(key) = provider_key.as_deref() {
                let _ = self
                    .model_cache
                    .delete(&CacheKey::ModelByName(key, &model.model_name).to_compact_string())
                    .await;
            }
        }

        self.invalidate_model_routes_for_provider(provider_id).await
    }

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

    pub async fn get_model_by_id(&self, id: i64) -> Result<Option<Arc<CacheModel>>, AppStoreError> {
        let cache_key = CacheKey::ModelById(id).to_compact_string();

        self.get_or_load(&self.model_cache, &cache_key, || async {
            if let Ok(db_model) = Model::get_by_id(id) {
                let cache_item = CacheModel::from(db_model.clone());
                if let Some(provider) = self.get_provider_by_id(db_model.provider_id).await? {
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
        Ok(self.model_cache.delete(&cache_key).await?)
    }

    pub async fn invalidate_model(&self, id: i64, name: Option<&str>) -> Result<(), AppStoreError> {
        self.invalidate_models_catalog().await?;
        let _ = self.invalidate_model_routes_for_model(id).await;
        let _ = self.invalidate_model_request_patch_rules(id).await;
        if let Some(n) = name {
            let parts: Vec<&str> = n.splitn(2, '/').collect();
            if parts.len() == 2 {
                let _ = self.invalidate_model_by_name(parts[0], parts[1]).await;
            }
        } else if let Some(m) = self.get_model_by_id(id).await? {
            if let Some(p) = self.get_provider_by_id(m.provider_id).await? {
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

    pub async fn invalidate_reasoning_model_config(
        &self,
        model_id: i64,
    ) -> Result<(), AppStoreError> {
        self.invalidate_models_catalog().await?;

        if let Ok(model) = Model::get_by_id(model_id) {
            if let Ok(provider) = Provider::get_by_id(model.provider_id) {
                let _ = self
                    .invalidate_model_by_name(&provider.provider_key, &model.model_name)
                    .await;
            }
        }

        let _ = self.invalidate_model_routes_for_model(model_id).await;
        Ok(self
            .model_cache
            .delete(&CacheKey::ModelById(model_id).to_compact_string())
            .await?)
    }

    pub async fn invalidate_runtime_feature_provider_config(
        &self,
        _provider_id: i64,
    ) -> Result<(), AppStoreError> {
        self.invalidate_models_catalog().await
    }

    pub async fn invalidate_runtime_feature_model_config(
        &self,
        _model_id: i64,
    ) -> Result<(), AppStoreError> {
        self.invalidate_models_catalog().await
    }

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

    async fn run_provider_api_keys_invalidation_hook(&self, provider_id: i64) {
        let hook = self
            .provider_api_keys_invalidation_hook
            .read()
            .await
            .clone();
        if let Some(hook) = hook {
            (hook)(provider_id).await;
        }
    }

    pub async fn invalidate_provider_api_keys(
        &self,
        provider_id: i64,
    ) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ProviderApiKeys(provider_id).to_compact_string();
        self.provider_api_keys_cache.delete(&cache_key).await?;
        self.run_provider_api_keys_invalidation_hook(provider_id)
            .await;
        Ok(())
    }

    pub async fn get_provider_request_patch_rules(
        &self,
        provider_id: i64,
    ) -> Result<Arc<Vec<CacheRequestPatchRule>>, AppStoreError> {
        let cache_key = CacheKey::ProviderRequestPatchRules(provider_id).to_compact_string();

        let rules = self
            .get_or_load(
                &self.provider_request_patch_rules_cache,
                &cache_key,
                || async {
                    match RequestPatchRule::list_by_provider_id(provider_id) {
                        Ok(rows) => Ok(Some(Self::cache_request_patch_rules(rows)?)),
                        Err(BaseError::NotFound(_)) => Ok(None),
                        Err(err) => Err(AppStoreError::DatabaseError(format!(
                            "failed to load provider request patch rules for {}: {:?}",
                            provider_id, err
                        ))),
                    }
                },
            )
            .await?;

        Ok(rules.unwrap_or_else(|| Arc::new(Vec::new())))
    }

    pub async fn invalidate_provider_request_patch_rules(
        &self,
        provider_id: i64,
    ) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ProviderRequestPatchRules(provider_id).to_compact_string();
        self.provider_request_patch_rules_cache
            .delete(&cache_key)
            .await?;

        for model in Model::list_by_provider_id(provider_id).map_err(|err| {
            AppStoreError::DatabaseError(format!(
                "failed to list models for provider request patch invalidation {}: {:?}",
                provider_id, err
            ))
        })? {
            let _ = self
                .invalidate_model_effective_request_patches(model.id)
                .await;
        }

        Ok(())
    }

    pub async fn get_model_request_patch_rules(
        &self,
        model_id: i64,
    ) -> Result<Arc<Vec<CacheRequestPatchRule>>, AppStoreError> {
        let cache_key = CacheKey::ModelRequestPatchRules(model_id).to_compact_string();

        let rules = self
            .get_or_load(
                &self.model_request_patch_rules_cache,
                &cache_key,
                || async {
                    match RequestPatchRule::list_by_model_id(model_id) {
                        Ok(rows) => Ok(Some(Self::cache_request_patch_rules(rows)?)),
                        Err(BaseError::NotFound(_)) => Ok(None),
                        Err(err) => Err(AppStoreError::DatabaseError(format!(
                            "failed to load model request patch rules for {}: {:?}",
                            model_id, err
                        ))),
                    }
                },
            )
            .await?;

        Ok(rules.unwrap_or_else(|| Arc::new(Vec::new())))
    }

    pub async fn invalidate_model_request_patch_rules(
        &self,
        model_id: i64,
    ) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ModelRequestPatchRules(model_id).to_compact_string();
        self.model_request_patch_rules_cache
            .delete(&cache_key)
            .await?;
        self.invalidate_model_effective_request_patches(model_id)
            .await
    }

    pub async fn get_model_effective_request_patches(
        &self,
        model_id: i64,
    ) -> Result<Option<Arc<CacheResolvedModelRequestPatches>>, AppStoreError> {
        let cache_key = CacheKey::ModelEffectiveRequestPatches(model_id).to_compact_string();

        self.get_or_load(
            &self.model_effective_request_patches_cache,
            &cache_key,
            || async { self.load_model_effective_request_patches(model_id).await },
        )
        .await
    }

    pub async fn invalidate_model_effective_request_patches(
        &self,
        model_id: i64,
    ) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ModelEffectiveRequestPatches(model_id).to_compact_string();
        Ok(self
            .model_effective_request_patches_cache
            .delete(&cache_key)
            .await?)
    }

    pub async fn get_cost_catalog_version_by_id(
        &self,
        id: i64,
    ) -> Result<Option<Arc<CacheCostCatalogVersion>>, AppStoreError> {
        let cache_key = CacheKey::CostCatalogVersion(id).to_compact_string();

        self.get_or_load(&self.cost_catalog_version_cache, &cache_key, || async {
            match CostCatalogVersion::get_by_id(id) {
                Ok(version) => {
                    let components =
                        CostComponent::list_by_catalog_version_id(id).map_err(|e| {
                            AppStoreError::DatabaseError(format!(
                                "failed to list cost components for version {}: {:?}",
                                id, e
                            ))
                        })?;
                    Ok(Some(CacheCostCatalogVersion::from_db_with_components(
                        version, components,
                    )))
                }
                Err(BaseError::ParamInvalid(_)) => Ok(None),
                Err(err) => Err(AppStoreError::DatabaseError(format!(
                    "failed to get cost catalog version {}: {:?}",
                    id, err
                ))),
            }
        })
        .await
    }

    pub async fn get_cost_catalog_version_by_model(
        &self,
        model_id: i64,
        at_time_ms: i64,
    ) -> Result<Option<Arc<CacheCostCatalogVersion>>, AppStoreError> {
        let Some(model) = self.get_model_by_id(model_id).await? else {
            return Ok(None);
        };
        let Some(cost_catalog_id) = model.cost_catalog_id else {
            return Ok(None);
        };

        let active_version =
            CostCatalogVersion::get_active_by_catalog_id(cost_catalog_id, at_time_ms).map_err(
                |e| {
                    AppStoreError::DatabaseError(format!(
                        "failed to resolve active cost catalog version for catalog {} at {}: {:?}",
                        cost_catalog_id, at_time_ms, e
                    ))
                },
            )?;

        match active_version {
            Some(version) => self.get_cost_catalog_version_by_id(version.id).await,
            None => Ok(None),
        }
    }

    pub async fn invalidate_cost_catalog_version(&self, id: i64) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::CostCatalogVersion(id).to_compact_string();
        Ok(self.cost_catalog_version_cache.delete(&cache_key).await?)
    }

    fn load_models_catalog() -> Result<CacheModelsCatalog, AppStoreError> {
        let providers = Provider::list_all()
            .map_err(|e| AppStoreError::DatabaseError(format!("failed to list providers: {e:?}")))?
            .into_iter()
            .map(CacheProvider::from)
            .collect();
        let models = Model::list_all()
            .map_err(|e| AppStoreError::DatabaseError(format!("failed to list models: {e:?}")))?
            .into_iter()
            .map(CacheModel::from)
            .collect();
        let mut routes = Vec::new();
        for route_item in ModelRoute::list_summary().map_err(|e| {
            AppStoreError::DatabaseError(format!("failed to list model routes: {e:?}"))
        })? {
            let detail = ModelRoute::get_detail(route_item.route.id).map_err(|e| {
                AppStoreError::DatabaseError(format!(
                    "failed to load model route detail {}: {e:?}",
                    route_item.route.id
                ))
            })?;
            routes.push(CacheModelRoute::from_detail(&detail));
        }
        let api_key_overrides = ApiKeyModelOverride::list_all()
            .map_err(|e| {
                AppStoreError::DatabaseError(format!(
                    "failed to list api key model overrides: {e:?}"
                ))
            })?
            .into_iter()
            .map(CacheApiKeyModelOverride::from)
            .collect();
        let reasoning_configs = ReasoningConfig::list_active_with_presets()
            .map_err(|e| {
                AppStoreError::DatabaseError(format!("failed to list reasoning configs: {e:?}"))
            })?
            .into_iter()
            .map(CacheReasoningConfig::from)
            .collect();
        let runtime_feature_configs = RuntimeFeatureConfig::list_active()
            .map_err(|e| {
                AppStoreError::DatabaseError(format!(
                    "failed to list runtime feature configs: {e:?}"
                ))
            })?
            .into_iter()
            .map(CacheRuntimeFeatureConfig::from)
            .collect();

        Ok(CacheModelsCatalog {
            providers,
            models,
            routes,
            api_key_overrides,
            reasoning_configs,
            runtime_feature_configs,
        })
    }
}

fn select_catalog_cache_backend_status(
    configured_backend: CacheBackendType,
    force_memory_cache: bool,
    redis_configured: bool,
    redis_available: bool,
) -> CatalogCacheBackendStatus {
    if force_memory_cache {
        return CatalogCacheBackendStatus {
            configured_backend,
            effective_backend: CacheBackendType::Memory,
            fallback_reason: Some("test_isolation".to_string()),
        };
    }

    match configured_backend {
        CacheBackendType::Memory => CatalogCacheBackendStatus {
            configured_backend,
            effective_backend: CacheBackendType::Memory,
            fallback_reason: None,
        },
        CacheBackendType::Redis if redis_available => CatalogCacheBackendStatus {
            configured_backend,
            effective_backend: CacheBackendType::Redis,
            fallback_reason: None,
        },
        CacheBackendType::Redis if !redis_configured => CatalogCacheBackendStatus {
            configured_backend,
            effective_backend: CacheBackendType::Memory,
            fallback_reason: Some("redis_config_missing".to_string()),
        },
        CacheBackendType::Redis => CatalogCacheBackendStatus {
            configured_backend,
            effective_backend: CacheBackendType::Memory,
            fallback_reason: Some("redis_unavailable".to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogService, select_catalog_cache_backend_status};
    use crate::config::CacheBackendType;
    use crate::database::TestDbContext;
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::reasoning_config::{
        ReasoningConfig, ReasoningConfigMode, ReasoningConfigPresetInput, ReasoningConfigScope,
        ReasoningPatchFamily, ReasoningPreset,
    };
    use crate::database::runtime_feature_config::{RuntimeFeatureConfig, RuntimeFeatureKey};
    use crate::schema::enum_def::{Action, ProviderApiKeyMode, ProviderType};
    use crate::service::cache::types::{
        CacheApiKey, CacheCostCatalogVersion, CacheEntry, CacheModel, CacheModelRoute,
        CacheModelRouteCandidate, CacheModelsCatalog, CacheProvider,
    };
    use crate::service::catalog::keys::CacheKey;
    use chrono::Utc;

    fn cache_api_key() -> CacheApiKey {
        CacheApiKey {
            id: 42,
            api_key_hash: "hash".to_string(),
            key_prefix: "cyder-prefix".to_string(),
            key_last4: "1234".to_string(),
            name: "runtime".to_string(),
            description: None,
            default_action: Action::Allow,
            is_enabled: true,
            expires_at: None,
            rate_limit_rpm: Some(2),
            max_concurrent_requests: Some(2),
            quota_daily_requests: Some(3),
            quota_daily_tokens: Some(100),
            quota_monthly_tokens: Some(200),
            budget_daily_nanos: Some(50),
            budget_daily_currency: Some("usd".to_string()),
            budget_monthly_nanos: Some(80),
            budget_monthly_currency: Some("usd".to_string()),
            acl_rules: vec![],
        }
    }

    fn seed_provider(id: i64, provider_key: &str) -> Provider {
        Provider::create(&NewProvider {
            id,
            provider_key: provider_key.to_string(),
            name: provider_key.to_string(),
            endpoint: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        })
        .expect("provider seed should succeed")
    }

    fn seed_model(provider_id: i64, model_name: &str) -> CacheModel {
        let model = Model::create(
            provider_id,
            model_name,
            None,
            true,
            ModelCapabilityFlags::default(),
        )
        .expect("model seed should succeed");
        CacheModel::from(model)
    }

    #[test]
    fn catalog_cache_backend_status_uses_memory_when_memory_is_configured() {
        let status =
            select_catalog_cache_backend_status(CacheBackendType::Memory, false, true, true);

        assert_eq!(status.configured_backend, CacheBackendType::Memory);
        assert_eq!(status.effective_backend, CacheBackendType::Memory);
        assert!(status.fallback_reason.is_none());
    }

    #[test]
    fn catalog_cache_backend_status_exposes_missing_redis_config_fallback() {
        let status =
            select_catalog_cache_backend_status(CacheBackendType::Redis, false, false, false);

        assert_eq!(status.configured_backend, CacheBackendType::Redis);
        assert_eq!(status.effective_backend, CacheBackendType::Memory);
        assert_eq!(
            status.fallback_reason.as_deref(),
            Some("redis_config_missing")
        );
    }

    #[test]
    fn catalog_cache_backend_status_exposes_unavailable_redis_fallback() {
        let status =
            select_catalog_cache_backend_status(CacheBackendType::Redis, false, true, false);

        assert_eq!(status.configured_backend, CacheBackendType::Redis);
        assert_eq!(status.effective_backend, CacheBackendType::Memory);
        assert_eq!(status.fallback_reason.as_deref(), Some("redis_unavailable"));
    }

    #[test]
    fn catalog_cache_backend_status_uses_redis_when_configured_and_available() {
        let status =
            select_catalog_cache_backend_status(CacheBackendType::Redis, false, true, true);

        assert_eq!(status.configured_backend, CacheBackendType::Redis);
        assert_eq!(status.effective_backend, CacheBackendType::Redis);
        assert!(status.fallback_reason.is_none());
    }

    #[test]
    fn catalog_cache_backend_status_exposes_test_isolation() {
        let status = select_catalog_cache_backend_status(CacheBackendType::Redis, true, true, true);

        assert_eq!(status.configured_backend, CacheBackendType::Redis);
        assert_eq!(status.effective_backend, CacheBackendType::Memory);
        assert_eq!(status.fallback_reason.as_deref(), Some("test_isolation"));
    }

    fn preset(
        preset_key: &str,
        expose_in_models: bool,
        is_enabled: bool,
    ) -> ReasoningConfigPresetInput {
        ReasoningConfigPresetInput {
            preset_key: preset_key.to_string(),
            expose_in_models,
            is_enabled,
        }
    }

    #[tokio::test]
    async fn invalidate_cost_catalog_version_removes_cached_snapshot() {
        let catalog = CatalogService::new(true).await;
        let cache_key = CacheKey::CostCatalogVersion(88).to_compact_string();
        let cached_version = CacheCostCatalogVersion {
            id: 88,
            catalog_id: 7,
            version: "v1".to_string(),
            currency: "USD".to_string(),
            source: Some("test".to_string()),
            effective_from: 100,
            effective_until: None,
            is_enabled: true,
            components: Vec::new(),
        };

        catalog
            .cost_catalog_version_cache
            .set_positive(&cache_key, &cached_version)
            .await
            .expect("seed cache");

        let cached = catalog
            .cost_catalog_version_cache
            .get_entry(&cache_key)
            .await
            .expect("read cache before invalidate");
        assert!(matches!(cached.as_deref(), Some(CacheEntry::Positive(_))));

        catalog
            .invalidate_cost_catalog_version(88)
            .await
            .expect("invalidate version");

        let cached_after = catalog
            .cost_catalog_version_cache
            .get_entry(&cache_key)
            .await
            .expect("read cache after invalidate");
        assert!(cached_after.is_none());
    }

    #[tokio::test]
    async fn invalidate_model_route_by_name_removes_cached_snapshot() {
        let catalog = CatalogService::new(true).await;
        let cache_key = CacheKey::ModelRouteByName("manual-smoke-route").to_compact_string();
        let route = CacheModelRoute {
            id: 88,
            route_name: "manual-smoke-route".to_string(),
            description: None,
            is_enabled: true,
            expose_in_models: true,
            candidates: vec![CacheModelRouteCandidate {
                route_id: 88,
                model_id: 2,
                provider_id: 1,
                priority: 0,
                is_enabled: true,
            }],
        };

        catalog
            .model_route_cache
            .set_positive(&cache_key, &route)
            .await
            .expect("seed route cache");

        catalog
            .invalidate_model_route_by_name("manual-smoke-route")
            .await
            .expect("invalidate route");

        let cached_after = catalog
            .model_route_cache
            .get_entry(&cache_key)
            .await
            .expect("read route cache after invalidate");
        assert!(cached_after.is_none());
    }

    #[tokio::test]
    async fn invalidate_api_key_model_override_removes_cached_snapshot() {
        let catalog = CatalogService::new(true).await;
        let cache_key = CacheKey::ApiKeyModelOverride(7, "manual-cli-model").to_compact_string();
        let route = CacheModelRoute {
            id: 88,
            route_name: "manual-smoke-route".to_string(),
            description: None,
            is_enabled: true,
            expose_in_models: true,
            candidates: vec![CacheModelRouteCandidate {
                route_id: 88,
                model_id: 2,
                provider_id: 1,
                priority: 0,
                is_enabled: true,
            }],
        };

        catalog
            .api_key_override_route_cache
            .set_positive(&cache_key, &route)
            .await
            .expect("seed override cache");

        catalog
            .invalidate_api_key_model_override(7, "manual-cli-model")
            .await
            .expect("invalidate override");

        let cached_after = catalog
            .api_key_override_route_cache
            .get_entry(&cache_key)
            .await
            .expect("read override cache after invalidate");
        assert!(cached_after.is_none());
    }

    #[tokio::test]
    async fn reload_preheats_reasoning_config_snapshots() {
        let db = TestDbContext::new_sqlite("catalog-reasoning-config-reload.sqlite");
        db.run_async(async {
            let provider = seed_provider(101, "openai");
            let model = Model::create(
                provider.id,
                "gpt-4o-mini",
                None,
                true,
                ModelCapabilityFlags::default(),
            )
            .expect("model");

            let provider_config = ReasoningConfig::upsert_provider_config(
                provider.id,
                "openai_chat_reasoning_effort",
                &[preset("high", true, true), preset("low", false, false)],
            )
            .expect("provider config");
            let model_config = ReasoningConfig::upsert_model_config(
                model.id,
                ReasoningConfigMode::Custom,
                Some("openai_responses_reasoning"),
                &[preset("medium", false, true)],
            )
            .expect("model config");
            let provider_feature = RuntimeFeatureConfig::upsert_provider_config(
                provider.id,
                RuntimeFeatureKey::OpenAiReasoningContentRepair,
                true,
            )
            .expect("provider runtime feature config");
            let model_feature = RuntimeFeatureConfig::upsert_model_config(
                model.id,
                RuntimeFeatureKey::OpenAiReasoningContentRepair,
                false,
            )
            .expect("model runtime feature config");

            let catalog = CatalogService::new(true).await;
            catalog.reload().await;

            let snapshot = catalog
                .get_models_catalog()
                .await
                .expect("catalog load should succeed");
            assert_eq!(snapshot.reasoning_configs.len(), 2);
            assert!(snapshot.providers.iter().any(|item| item.id == provider.id));
            assert!(snapshot.models.iter().any(|item| item.id == model.id));

            let chat_config = snapshot
                .reasoning_configs
                .iter()
                .find(|config| config.id == provider_config.config.id)
                .expect("provider config snapshot");
            assert_eq!(chat_config.scope_kind, ReasoningConfigScope::Provider);
            assert_eq!(chat_config.provider_id, Some(provider.id));
            assert_eq!(
                chat_config.family,
                Some(ReasoningPatchFamily::OpenAiChatReasoningEffort)
            );
            assert_eq!(chat_config.presets.len(), 2);
            let high = chat_config
                .presets
                .iter()
                .find(|preset| preset.preset == ReasoningPreset::High)
                .expect("high preset snapshot");
            assert_eq!(high.suffix, "high");
            assert!(high.requires_reasoning);
            assert_eq!(high.allowed_operation_kinds, vec!["generation".to_string()]);
            assert!(high.is_enabled);

            let model_snapshot = snapshot
                .reasoning_configs
                .iter()
                .find(|config| config.id == model_config.config.id)
                .expect("model config snapshot");
            assert_eq!(model_snapshot.scope_kind, ReasoningConfigScope::Model);
            assert_eq!(model_snapshot.model_id, Some(model.id));
            assert_eq!(snapshot.runtime_feature_configs.len(), 2);
            let provider_feature_snapshot = snapshot
                .runtime_feature_configs
                .iter()
                .find(|config| config.id == provider_feature.config.id)
                .expect("provider runtime feature snapshot");
            assert_eq!(provider_feature_snapshot.provider_id, Some(provider.id));
            assert!(provider_feature_snapshot.enabled);
            assert_eq!(
                provider_feature_snapshot.feature_key,
                RuntimeFeatureKey::OpenAiReasoningContentRepair
            );
            let model_feature_snapshot = snapshot
                .runtime_feature_configs
                .iter()
                .find(|config| config.id == model_feature.config.id)
                .expect("model runtime feature snapshot");
            assert_eq!(model_feature_snapshot.model_id, Some(model.id));
            assert!(!model_feature_snapshot.enabled);

            let encoded = bincode::encode_to_vec(&*snapshot, bincode::config::standard())
                .expect("catalog snapshot should bincode encode");
            let (decoded, _): (CacheModelsCatalog, usize) =
                bincode::decode_from_slice(&encoded, bincode::config::standard())
                    .expect("catalog snapshot should bincode decode");
            assert_eq!(decoded.reasoning_configs.len(), 2);
            assert_eq!(decoded.runtime_feature_configs.len(), 2);
        })
        .await;
    }

    #[tokio::test]
    async fn invalidate_reasoning_config_removes_owner_and_related_snapshots() {
        let db = TestDbContext::new_sqlite("catalog-reasoning-config-invalidate.sqlite");
        db.run_async(async {
            let provider = seed_provider(201, "openai");
            let model = seed_model(provider.id, "gpt-4o-mini");
            let catalog = CatalogService::new(true).await;
            let cached_provider = CacheProvider::from(provider.clone());
            let empty_catalog = CacheModelsCatalog {
                providers: vec![cached_provider.clone()],
                models: vec![model.clone()],
                routes: vec![],
                api_key_overrides: vec![],
                reasoning_configs: vec![],
                runtime_feature_configs: vec![],
            };

            catalog
                .models_catalog_cache
                .set_positive(&CacheKey::ModelsCatalog.to_compact_string(), &empty_catalog)
                .await
                .expect("seed models catalog");
            catalog
                .provider_cache
                .set_positive(
                    &CacheKey::ProviderById(provider.id).to_compact_string(),
                    &cached_provider,
                )
                .await
                .expect("seed provider id cache");
            catalog
                .provider_cache
                .set_positive(
                    &CacheKey::ProviderByKey(&provider.provider_key).to_compact_string(),
                    &cached_provider,
                )
                .await
                .expect("seed provider key cache");
            catalog
                .model_cache
                .set_positive(&CacheKey::ModelById(model.id).to_compact_string(), &model)
                .await
                .expect("seed model id cache");
            catalog
                .model_cache
                .set_positive(
                    &CacheKey::ModelByName(&provider.provider_key, &model.model_name)
                        .to_compact_string(),
                    &model,
                )
                .await
                .expect("seed model name cache");

            catalog
                .invalidate_reasoning_provider_config(provider.id)
                .await
                .expect("invalidate provider config");

            assert!(
                catalog
                    .models_catalog_cache
                    .get_entry(&CacheKey::ModelsCatalog.to_compact_string())
                    .await
                    .expect("models catalog cache read")
                    .is_none()
            );
            assert!(
                catalog
                    .provider_cache
                    .get_entry(&CacheKey::ProviderById(provider.id).to_compact_string())
                    .await
                    .expect("provider id cache read")
                    .is_none()
            );
            assert!(
                catalog
                    .provider_cache
                    .get_entry(&CacheKey::ProviderByKey(&provider.provider_key).to_compact_string())
                    .await
                    .expect("provider key cache read")
                    .is_none()
            );
            assert!(
                catalog
                    .model_cache
                    .get_entry(&CacheKey::ModelById(model.id).to_compact_string())
                    .await
                    .expect("model id cache read")
                    .is_none()
            );
            assert!(
                catalog
                    .model_cache
                    .get_entry(
                        &CacheKey::ModelByName(&provider.provider_key, &model.model_name)
                            .to_compact_string(),
                    )
                    .await
                    .expect("model name cache read")
                    .is_none()
            );

            catalog
                .model_cache
                .set_positive(&CacheKey::ModelById(model.id).to_compact_string(), &model)
                .await
                .expect("reseed model id cache");
            catalog
                .model_cache
                .set_positive(
                    &CacheKey::ModelByName(&provider.provider_key, &model.model_name)
                        .to_compact_string(),
                    &model,
                )
                .await
                .expect("reseed model name cache");

            catalog
                .invalidate_reasoning_model_config(model.id)
                .await
                .expect("invalidate model config");

            assert!(
                catalog
                    .model_cache
                    .get_entry(&CacheKey::ModelById(model.id).to_compact_string())
                    .await
                    .expect("model id cache read after model config invalidate")
                    .is_none()
            );
            assert!(
                catalog
                    .model_cache
                    .get_entry(
                        &CacheKey::ModelByName(&provider.provider_key, &model.model_name)
                            .to_compact_string(),
                    )
                    .await
                    .expect("model name cache read after model config invalidate")
                    .is_none()
            );
        })
        .await;
    }

    #[tokio::test]
    async fn expired_api_key_cache_hit_is_evicted() {
        let catalog = CatalogService::new(true).await;
        let expired_at = Utc::now().timestamp_millis() - 1;
        let api_key_hash = "expired-hash".to_string();
        let cache_key = CacheKey::ApiKeyHash(&api_key_hash).to_compact_string();
        let cached_key = CacheApiKey {
            api_key_hash: api_key_hash.clone(),
            expires_at: Some(expired_at),
            ..cache_api_key()
        };

        catalog
            .api_key_cache
            .set_positive(&cache_key, &cached_key)
            .await
            .expect("seed expired cache entry");

        let result = catalog
            .get_api_key_by_hash(&api_key_hash)
            .await
            .expect("expired cache hit should not error");
        assert!(result.is_none());

        let cached_after = catalog
            .api_key_cache
            .get_entry(&cache_key)
            .await
            .expect("read cache after eviction");
        assert!(cached_after.is_none());
    }

    #[tokio::test]
    async fn get_or_load_rehydrates_after_cache_clear() {
        let catalog = CatalogService::new(true).await;
        let cache_key = CacheKey::ApiKeyHash("rehydrate").to_compact_string();
        let cached_key = cache_api_key();

        catalog
            .api_key_cache
            .set_positive(&cache_key, &cached_key)
            .await
            .expect("seed cache");
        catalog.clear_cache().await;

        let loaded = catalog
            .get_or_load(&catalog.api_key_cache, &cache_key, || async {
                Ok(Some(cached_key.clone()))
            })
            .await
            .expect("reload after clear should succeed")
            .expect("loader should repopulate cache");

        assert_eq!(loaded.id, cached_key.id);

        let cached_after = catalog
            .api_key_cache
            .get_entry(&cache_key)
            .await
            .expect("read cache after reload");
        assert!(matches!(
            cached_after.as_deref(),
            Some(CacheEntry::Positive(_))
        ));
    }
}
