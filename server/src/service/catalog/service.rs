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
use crate::database::provider::{Provider, ProviderApiKeyRepository};
use crate::database::request_patch::RequestPatchVariantRepository;
use crate::service::app_state::AppStoreError;
use crate::service::cache::memory::MemoryCacheBackend;
use crate::service::cache::redis::RedisCacheBackend;
use crate::service::cache::repository::{CacheRepository, DynCacheRepo};
use crate::service::cache::types::{
    CacheApiKey, CacheCostCatalogVersion, CacheEntry, CacheModel, CacheModelsCatalog,
    CacheProvider, CacheProviderKey, CacheRequestPatchVariant,
};
use crate::service::redis::{self, RedisPool};

use super::keys::CacheKey;
use super::reload::{
    cache_backend_name, increment_failure_counter, summarize_failures, summarize_repo_names,
};

type CacheRepo<T> = Arc<dyn DynCacheRepo<T>>;
const CATALOG_CACHE_SCHEMA_PREFIX: &str = "r312:";
type ProviderApiKeysInvalidationHook = Arc<
    dyn Fn(i64) -> Pin<Box<dyn Future<Output = Result<(), AppStoreError>> + Send + 'static>>
        + Send
        + Sync,
>;

#[derive(Clone)]
enum ProviderApiKeyRuntimeSnapshot {
    Trusted(Arc<Vec<CacheProviderKey>>),
    FailClosed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CatalogCacheBackendStatus {
    pub configured_backend: CacheBackendType,
    pub effective_backend: CacheBackendType,
    pub fallback_reason: Option<String>,
}

pub struct CatalogService {
    api_key_cache: CacheRepo<CacheApiKey>,
    models_catalog_cache: CacheRepo<CacheModelsCatalog>,
    provider_cache: CacheRepo<CacheProvider>,
    model_cache: CacheRepo<CacheModel>,
    provider_api_keys_cache: CacheRepo<Vec<CacheProviderKey>>,
    cost_catalog_version_cache: CacheRepo<CacheCostCatalogVersion>,
    backend_status: CatalogCacheBackendStatus,
    negative_cache_ttl: Duration,
    provider_api_keys_invalidation_hook:
        tokio::sync::RwLock<Option<ProviderApiKeysInvalidationHook>>,
    provider_api_key_runtime_snapshots:
        tokio::sync::RwLock<HashMap<i64, ProviderApiKeyRuntimeSnapshot>>,
    provider_api_key_refresh_lock: tokio::sync::Mutex<()>,
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
            models_catalog_cache: Self::create_repo(ttl, pool),
            provider_cache: Self::create_repo(ttl, pool),
            model_cache: Self::create_repo(ttl, pool),
            provider_api_keys_cache: Self::create_repo(ttl, pool),
            cost_catalog_version_cache: Self::create_repo(ttl, pool),
            backend_status,
            negative_cache_ttl,
            provider_api_keys_invalidation_hook: tokio::sync::RwLock::new(None),
            provider_api_key_runtime_snapshots: tokio::sync::RwLock::new(HashMap::new()),
            provider_api_key_refresh_lock: tokio::sync::Mutex::new(()),
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
                "{}{}{}",
                redis_config.key_prefix,
                CONFIG.cache.catalog_redis_key_prefix(),
                CATALOG_CACHE_SCHEMA_PREFIX,
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

    pub async fn reload(&self) {
        crate::info_event!("cache.reload_started");
        let mut failure_counts: HashMap<&'static str, usize> = HashMap::new();
        let mut catalog_providers = Vec::new();
        let mut catalog_models = Vec::new();
        let mut catalog_request_patch_variants = Vec::new();
        let mut api_key_count = 0usize;
        let mut provider_count = 0usize;
        let mut model_count = 0usize;
        let mut provider_api_key_count = 0usize;
        let mut provider_api_key_group_count = 0usize;
        let mut request_patch_variant_count = 0usize;
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
                    let Ok(cache_item) = CacheModel::from_db(model) else {
                        increment_failure_counter(
                            &mut failure_counts,
                            "model_source_binding_snapshot",
                        );
                        continue;
                    };
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

        let source_ids = catalog_providers
            .iter()
            .flat_map(|provider| provider.upstream_sources.iter().map(|source| source.id))
            .collect::<Vec<_>>();
        let model_ids = catalog_models
            .iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();
        match RequestPatchVariantRepository::list_by_source_ids(&source_ids).and_then(
            |source_variants| {
                RequestPatchVariantRepository::list_by_model_ids(&model_ids).map(|model_variants| {
                    source_variants
                        .into_iter()
                        .chain(model_variants)
                        .map(CacheRequestPatchVariant::from)
                        .collect::<Vec<_>>()
                })
            },
        ) {
            Ok(variants) => {
                request_patch_variant_count = variants.len();
                catalog_request_patch_variants = variants;
            }
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "request_patch_variant_list");
            }
        }

        let models_catalog = CacheModelsCatalog {
            providers: catalog_providers.clone(),
            models: catalog_models.clone(),
            request_patch_variants: catalog_request_patch_variants.clone(),
        };
        let _ = self
            .models_catalog_cache
            .set_positive(
                &CacheKey::ModelsCatalog.to_compact_string(),
                &models_catalog,
            )
            .await;

        match ProviderApiKeyRepository::list_all_selections() {
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
                let mut snapshots = provider_id_to_key
                    .keys()
                    .map(|provider_id| {
                        (
                            *provider_id,
                            ProviderApiKeyRuntimeSnapshot::Trusted(Arc::new(Vec::new())),
                        )
                    })
                    .collect::<HashMap<_, _>>();
                for (provider_id, provider_keys) in by_provider {
                    snapshots.insert(
                        provider_id,
                        ProviderApiKeyRuntimeSnapshot::Trusted(Arc::new(provider_keys.clone())),
                    );
                    let _ = self
                        .provider_api_keys_cache
                        .set_positive(
                            &CacheKey::ProviderApiKeys(provider_id).to_compact_string(),
                            &provider_keys,
                        )
                        .await;
                }
                *self.provider_api_key_runtime_snapshots.write().await = snapshots;
            }
            Err(_) => {
                increment_failure_counter(&mut failure_counts, "provider_api_key_list");
                *self.provider_api_key_runtime_snapshots.write().await = provider_id_to_key
                    .keys()
                    .map(|provider_id| (*provider_id, ProviderApiKeyRuntimeSnapshot::FailClosed))
                    .collect();
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
                provider_api_key_count = provider_api_key_count,
                provider_api_key_group_count = provider_api_key_group_count,
                request_patch_variant_count = request_patch_variant_count,
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
                provider_api_key_count = provider_api_key_count,
                provider_api_key_group_count = provider_api_key_group_count,
                request_patch_variant_count = request_patch_variant_count,
                cost_catalog_version_count = cost_catalog_version_count,
                failed_group_count = failure_counts.len(),
                failed_groups = failure_summary.as_deref(),
            );
        }
    }

    pub async fn clear_cache(&self) {
        crate::info_event!("cache.clear_started");

        self.provider_api_key_runtime_snapshots
            .write()
            .await
            .clear();

        let mut failed_repos = Vec::new();

        if self.api_key_cache.clear().await.is_err() {
            failed_repos.push("api_key_cache");
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
        if self.cost_catalog_version_cache.clear().await.is_err() {
            failed_repos.push("cost_catalog_version_cache");
        }

        let total_repo_count = 6usize;
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
            self.invalidate_api_key_hash(&row.api_key_hash).await?;
        }

        Ok(())
    }

    pub async fn invalidate_api_key(&self, key: &str) -> Result<(), AppStoreError> {
        self.invalidate_api_key_hash(&Self::hash_api_key(key)).await
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
            match Provider::get_by_id(id) {
                Ok(db_provider) => {
                    let cache_item = CacheProvider::from(db_provider.clone());
                    self.provider_cache
                        .set_positive(
                            &CacheKey::ProviderByKey(&db_provider.provider_key).to_compact_string(),
                            &cache_item,
                        )
                        .await?;
                    Ok(Some(cache_item))
                }
                Err(BaseError::ParamInvalid(_)) => Ok(None),
                Err(error) => Err(AppStoreError::DatabaseError(format!(
                    "failed to load provider aggregate {id}: {error:?}"
                ))),
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
            match Provider::get_by_key(key) {
                Ok(Some(db_provider)) => {
                    let cache_item = CacheProvider::from(db_provider.clone());
                    self.provider_cache
                        .set_positive(
                            &CacheKey::ProviderById(db_provider.id).to_compact_string(),
                            &cache_item,
                        )
                        .await?;
                    Ok(Some(cache_item))
                }
                Ok(None) => Ok(None),
                Err(error) => Err(AppStoreError::DatabaseError(format!(
                    "failed to load provider aggregate by key {key}: {error:?}"
                ))),
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
        if let Some(k) = key {
            let _ = self.invalidate_provider_by_key(k).await;
        } else if let Some(p) = self.get_provider_by_id(id).await? {
            let _ = self.invalidate_provider_by_key(&p.provider_key).await;
        }
        self.invalidate_provider_by_id(id).await
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
                    let cache_item = CacheModel::from_db(db_model.clone())
                        .map_err(|error| AppStoreError::DatabaseError(error))?;
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
                let cache_item = CacheModel::from_db(db_model.clone())
                    .map_err(|error| AppStoreError::DatabaseError(error))?;
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

    pub async fn get_provider_api_keys(
        &self,
        provider_id: i64,
    ) -> Result<Arc<Vec<CacheProviderKey>>, AppStoreError> {
        if let Some(snapshot) = self
            .provider_api_key_runtime_snapshots
            .read()
            .await
            .get(&provider_id)
            .cloned()
        {
            return Self::resolve_provider_key_snapshot(provider_id, snapshot);
        }

        let _refresh_guard = self.provider_api_key_refresh_lock.lock().await;
        if let Some(snapshot) = self
            .provider_api_key_runtime_snapshots
            .read()
            .await
            .get(&provider_id)
            .cloned()
        {
            return Self::resolve_provider_key_snapshot(provider_id, snapshot);
        }
        self.provider_api_key_runtime_snapshots
            .write()
            .await
            .insert(provider_id, ProviderApiKeyRuntimeSnapshot::FailClosed);
        let snapshot = Arc::new(Self::load_provider_api_key_snapshot(provider_id)?);
        self.provider_api_key_runtime_snapshots
            .write()
            .await
            .insert(
                provider_id,
                ProviderApiKeyRuntimeSnapshot::Trusted(Arc::clone(&snapshot)),
            );
        self.publish_provider_api_key_snapshot_best_effort(provider_id, snapshot.as_ref())
            .await;
        Ok(snapshot)
    }

    fn resolve_provider_key_snapshot(
        provider_id: i64,
        snapshot: ProviderApiKeyRuntimeSnapshot,
    ) -> Result<Arc<Vec<CacheProviderKey>>, AppStoreError> {
        match snapshot {
            ProviderApiKeyRuntimeSnapshot::Trusted(keys) => Ok(keys),
            ProviderApiKeyRuntimeSnapshot::FailClosed => Err(AppStoreError::CacheError(format!(
                "provider credential snapshot is fail-closed for provider {provider_id}"
            ))),
        }
    }

    fn load_provider_api_key_snapshot(
        provider_id: i64,
    ) -> Result<Vec<CacheProviderKey>, AppStoreError> {
        ProviderApiKeyRepository::list_selections_by_provider_id(provider_id)
            .map(|rows| rows.into_iter().map(CacheProviderKey::from).collect())
            .map_err(|_| {
                AppStoreError::DatabaseError(format!(
                    "failed to load provider credential snapshot for provider {provider_id}"
                ))
            })
    }

    async fn publish_provider_api_key_snapshot_best_effort(
        &self,
        provider_id: i64,
        snapshot: &Vec<CacheProviderKey>,
    ) {
        let cache_key = CacheKey::ProviderApiKeys(provider_id).to_compact_string();
        if let Err(error) = self.provider_api_keys_cache.delete(&cache_key).await {
            crate::warn_event!(
                "provider_credential.remote_cache_delete_failed",
                provider_id = provider_id,
                error = &error.to_string(),
            );
        }
        if let Err(error) = self
            .provider_api_keys_cache
            .set_positive(&cache_key, snapshot)
            .await
        {
            crate::warn_event!(
                "provider_credential.remote_cache_write_failed",
                provider_id = provider_id,
                error = &error.to_string(),
            );
        }
    }

    async fn run_provider_api_keys_invalidation_hook(
        &self,
        provider_id: i64,
    ) -> Result<(), AppStoreError> {
        let hook = self
            .provider_api_keys_invalidation_hook
            .read()
            .await
            .clone();
        if let Some(hook) = hook {
            (hook)(provider_id).await?;
        }
        Ok(())
    }

    pub async fn invalidate_provider_api_keys(
        &self,
        provider_id: i64,
    ) -> Result<(), AppStoreError> {
        let _refresh_guard = self.provider_api_key_refresh_lock.lock().await;
        self.provider_api_key_runtime_snapshots
            .write()
            .await
            .insert(provider_id, ProviderApiKeyRuntimeSnapshot::FailClosed);
        let snapshot = Arc::new(Self::load_provider_api_key_snapshot(provider_id)?);
        if let Err(error) = self
            .run_provider_api_keys_invalidation_hook(provider_id)
            .await
        {
            return Err(error);
        }
        self.provider_api_key_runtime_snapshots
            .write()
            .await
            .insert(
                provider_id,
                ProviderApiKeyRuntimeSnapshot::Trusted(Arc::clone(&snapshot)),
            );
        self.publish_provider_api_key_snapshot_best_effort(provider_id, snapshot.as_ref())
            .await;
        Ok(())
    }

    pub async fn get_request_patch_variants(
        &self,
    ) -> Result<Arc<Vec<CacheRequestPatchVariant>>, AppStoreError> {
        Ok(Arc::new(
            self.get_models_catalog()
                .await?
                .request_patch_variants
                .clone(),
        ))
    }

    pub async fn invalidate_request_patch_source(
        &self,
        _source_id: i64,
    ) -> Result<(), AppStoreError> {
        self.invalidate_models_catalog().await
    }

    pub async fn invalidate_request_patch_model(
        &self,
        _model_id: i64,
    ) -> Result<(), AppStoreError> {
        self.invalidate_models_catalog().await
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
        let providers: Vec<CacheProvider> = Provider::list_all()
            .map_err(|e| AppStoreError::DatabaseError(format!("failed to list providers: {e:?}")))?
            .into_iter()
            .map(CacheProvider::from)
            .collect();
        let models = Model::list_all()
            .map_err(|e| AppStoreError::DatabaseError(format!("failed to list models: {e:?}")))?
            .into_iter()
            .map(|model| CacheModel::from_db(model).map_err(AppStoreError::DatabaseError))
            .collect::<Result<Vec<_>, _>>()?;
        let source_ids = providers
            .iter()
            .flat_map(|provider| provider.upstream_sources.iter().map(|source| source.id))
            .collect::<Vec<_>>();
        let model_ids = models.iter().map(|model| model.id).collect::<Vec<_>>();
        let request_patch_variants = RequestPatchVariantRepository::list_by_source_ids(&source_ids)
            .and_then(|source_variants| {
                RequestPatchVariantRepository::list_by_model_ids(&model_ids).map(|model_variants| {
                    source_variants
                        .into_iter()
                        .chain(model_variants)
                        .map(CacheRequestPatchVariant::from)
                        .collect()
                })
            })
            .map_err(|e| {
                AppStoreError::DatabaseError(format!(
                    "failed to list request patch Variants: {e:?}"
                ))
            })?;

        Ok(CacheModelsCatalog {
            providers,
            models,
            request_patch_variants,
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
    use super::{CATALOG_CACHE_SCHEMA_PREFIX, CatalogService, select_catalog_cache_backend_status};
    use crate::config::CacheBackendType;
    use crate::database::TestDbContext;
    use crate::database::model::Model;
    use crate::database::provider::{NewProvider, Provider, ProviderAggregate};
    use crate::database::request_patch::{
        RequestPatchRuleInput, RequestPatchVariantInput, RequestPatchVariantRepository,
    };
    use crate::database::upstream_source::NewUpstreamSource;
    use crate::schema::enum_def::{
        Action, ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement,
        UpstreamProfileType,
    };
    use crate::service::cache::types::{
        CacheApiKey, CacheCostCatalogVersion, CacheEntry, CacheModel, CacheModelsCatalog,
        CacheProvider,
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

    fn seed_provider(id: i64, provider_key: &str) -> ProviderAggregate {
        Provider::create(
            &NewProvider {
                id,
                provider_key: provider_key.to_string(),
                name: provider_key.to_string(),
                is_enabled: true,
                created_at: 1,
                updated_at: 1,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
            },
            &NewUpstreamSource {
                id: id + 10_000,
                provider_id: id,
                profile_type: UpstreamProfileType::Openai,
                base_url: "https://api.example.com/v1".to_string(),
                use_proxy: false,
                is_enabled: true,
                is_default: true,
                created_at: 1,
                updated_at: 1,
                ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
            },
        )
        .expect("provider seed should succeed")
    }

    fn seed_model(provider_id: i64, model_name: &str) -> CacheModel {
        let model = Model::create(
            provider_id,
            model_name,
            None,
            crate::schema::enum_def::ModelKind::Chat,
            true,
        )
        .expect("model seed should succeed");
        CacheModel::from_db(model).expect("model source snapshot should load")
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

    #[test]
    fn catalog_cache_schema_uses_the_r312_namespace() {
        assert_eq!(CATALOG_CACHE_SCHEMA_PREFIX, "r312:");
        assert_ne!(CATALOG_CACHE_SCHEMA_PREFIX, "r311:");
    }

    #[tokio::test]
    async fn reload_preheats_source_bound_request_patch_variants_and_roundtrips_catalog() {
        let database = TestDbContext::new_sqlite("catalog-source-bound-request-patch.sqlite");
        database
            .run_async(async {
                let provider = seed_provider(301, "catalog-provider");
                let model = seed_model(provider.id, "catalog-model");
                let source_id = provider.upstream_sources[0].id;
                let variant = RequestPatchVariantRepository::create(&RequestPatchVariantInput {
                    source_id,
                    model_id: None,
                    suffix: Some("fast".to_string()),
                    enabled: true,
                    expose_in_models: true,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/options/temperature".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(serde_json::json!(0.2))),
                        description: Some("catalog test".to_string()),
                    }],
                })
                .expect("request patch Variant should be persisted");
                let model_variant =
                    RequestPatchVariantRepository::create(&RequestPatchVariantInput {
                        source_id,
                        model_id: Some(model.id),
                        suffix: Some("fast".to_string()),
                        enabled: true,
                        expose_in_models: false,
                        rules: vec![RequestPatchRuleInput {
                            placement: RequestPatchPlacement::Body,
                            target: "/options/top_p".to_string(),
                            operation: RequestPatchOperation::Set,
                            value_json: Some(Some(serde_json::json!(0.8))),
                            description: Some("model catalog test".to_string()),
                        }],
                    })
                    .expect("model request patch Variant should be persisted");

                let catalog = CatalogService::new(true).await;
                catalog.reload().await;
                let snapshot = catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should load");
                let cached = snapshot
                    .request_patch_variants
                    .iter()
                    .find(|item| item.id == variant.variant.id)
                    .expect("request patch Variant should be in catalog");
                assert_eq!(cached.source_id, source_id);
                assert_eq!(cached.model_id, None);
                assert_eq!(cached.suffix.as_deref(), Some("fast"));
                assert_eq!(cached.rules.len(), 1);
                assert!(snapshot.models.iter().any(|item| item.id == model.id));
                assert_eq!(snapshot.request_patch_variants.len(), 2);
                assert!(
                    snapshot
                        .request_patch_variants
                        .iter()
                        .any(|item| item.id == model_variant.variant.id)
                );

                let encoded = bincode::encode_to_vec(&*snapshot, bincode::config::standard())
                    .expect("catalog should encode");
                let (decoded, _): (CacheModelsCatalog, usize) =
                    bincode::decode_from_slice(&encoded, bincode::config::standard())
                        .expect("catalog should decode");
                assert_eq!(
                    decoded.request_patch_variants,
                    snapshot.request_patch_variants
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
