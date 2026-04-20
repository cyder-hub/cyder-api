use rand::{Rng, rng};
use reqwest::{Client, Proxy};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use chrono::{Datelike, TimeZone, Utc};
use cyder_tools::log::{debug, error, info};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::database::api_key::ApiKey;
use crate::database::api_key_rollup::{ApiKeyRollupDaily, ApiKeyRollupMonthly};
use crate::database::cost::{CostCatalogVersion, CostComponent};
use crate::database::model::Model;
use crate::database::model_route::{ApiKeyModelOverride, ModelRoute};
use crate::database::provider::{Provider, ProviderApiKey};
use crate::database::request_patch::RequestPatchRule;
use crate::schema::enum_def::ProviderApiKeyMode;

use super::cache::repository::{CacheRepository, DynCacheRepo};
use super::cache::types::{
    CacheApiKey, CacheApiKeyModelOverride, CacheCostCatalogVersion, CacheEntry, CacheModel,
    CacheModelRoute, CacheModelsCatalog, CacheProvider, CacheProviderKey, CacheRequestPatchRule,
    CacheResolvedModelRequestPatches, CacheSystemApiKey,
};
use super::cache::{CacheError, memory::MemoryCacheBackend};
use super::request_patch::resolve_effective_request_patches;

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
use crate::controller::BaseError;
use crate::service::redis::{self, RedisPool};

/// Type-erased cache repository, dispatching to the concrete backend
/// (Memory or Redis) via dynamic dispatch. The backend is selected once
/// at startup in `create_repo`, eliminating per-operation match arms.
type CacheRepo<T> = Arc<dyn DynCacheRepo<T>>;

enum CacheKey<'a> {
    ApiKeyHash(&'a str),
    ModelRouteById(i64),
    ModelRouteByName(&'a str),
    ApiKeyModelOverride(i64, &'a str),
    ModelsCatalog,
    ProviderById(i64),
    ProviderByKey(&'a str),
    ModelById(i64),
    ModelByName(&'a str, &'a str),
    ProviderApiKeys(i64),
    ProviderRequestPatchRules(i64),
    ModelRequestPatchRules(i64),
    ModelEffectiveRequestPatches(i64),
    CostCatalogVersion(i64),
}

use compact_str::{CompactString, format_compact};

impl<'a> std::fmt::Display for CacheKey<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheKey::ApiKeyHash(key) => write!(f, "api_key:hash:{}", key),
            CacheKey::ModelRouteById(id) => write!(f, "route:id:{}", id),
            CacheKey::ModelRouteByName(name) => write!(f, "route:name:{}", name),
            CacheKey::ApiKeyModelOverride(api_key_id, source_name) => {
                write!(f, "api_key_override:{}/{}", api_key_id, source_name)
            }
            CacheKey::ModelsCatalog => write!(f, "models:catalog"),
            CacheKey::ProviderById(id) => write!(f, "provider:id:{}", id),
            CacheKey::ProviderByKey(key) => write!(f, "provider:key:{}", key),
            CacheKey::ModelById(id) => write!(f, "model:id:{}", id),
            CacheKey::ModelByName(provider_key, model_name) => {
                write!(f, "model:name:{}/{}", provider_key, model_name)
            }
            CacheKey::ProviderApiKeys(provider_id) => write!(f, "provider_keys:{}", provider_id),
            CacheKey::ProviderRequestPatchRules(provider_id) => {
                write!(f, "request_patch:provider:{}", provider_id)
            }
            CacheKey::ModelRequestPatchRules(model_id) => {
                write!(f, "request_patch:model:{}", model_id)
            }
            CacheKey::ModelEffectiveRequestPatches(model_id) => {
                write!(f, "request_patch:model_effective:{}", model_id)
            }
            CacheKey::CostCatalogVersion(id) => write!(f, "cost_catalog_version:id:{}", id),
        }
    }
}

impl<'a> CacheKey<'a> {
    fn to_compact_string(&self) -> CompactString {
        match self {
            CacheKey::ApiKeyHash(key) => format_compact!("api_key:hash:{}", key),
            CacheKey::ModelRouteById(id) => format_compact!("route:id:{}", id),
            CacheKey::ModelRouteByName(name) => format_compact!("route:name:{}", name),
            CacheKey::ApiKeyModelOverride(api_key_id, source_name) => {
                format_compact!("api_key_override:{}/{}", api_key_id, source_name)
            }
            CacheKey::ModelsCatalog => format_compact!("models:catalog"),
            CacheKey::ProviderById(id) => format_compact!("provider:id:{}", id),
            CacheKey::ProviderByKey(key) => format_compact!("provider:key:{}", key),
            CacheKey::ModelById(id) => format_compact!("model:id:{}", id),
            CacheKey::ModelByName(provider_key, model_name) => {
                format_compact!("model:name:{}/{}", provider_key, model_name)
            }
            CacheKey::ProviderApiKeys(provider_id) => {
                format_compact!("provider_keys:{}", provider_id)
            }
            CacheKey::ProviderRequestPatchRules(provider_id) => {
                format_compact!("request_patch:provider:{}", provider_id)
            }
            CacheKey::ModelRequestPatchRules(model_id) => {
                format_compact!("request_patch:model:{}", model_id)
            }
            CacheKey::ModelEffectiveRequestPatches(model_id) => {
                format_compact!("request_patch:model_effective:{}", model_id)
            }
            CacheKey::CostCatalogVersion(id) => format_compact!("cost_catalog_version:id:{}", id),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderHealthStatus {
    Healthy,
    Open,
    HalfOpen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderHealthSnapshot {
    pub status: ProviderHealthStatus,
    pub consecutive_failures: u32,
    pub half_open_probe_in_flight: bool,
    pub opened_at: Option<i64>,
    pub last_failure_at: Option<i64>,
    pub last_recovered_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiKeyGovernanceSnapshot {
    pub api_key_id: i64,
    pub current_concurrency: u32,
    pub current_minute_bucket: Option<i64>,
    pub current_minute_request_count: u32,
    pub day_bucket: Option<i64>,
    pub daily_request_count: i64,
    pub daily_token_count: i64,
    pub month_bucket: Option<i64>,
    pub monthly_token_count: i64,
    pub daily_billed_amounts: Vec<ApiKeyBilledAmountSnapshot>,
    pub monthly_billed_amounts: Vec<ApiKeyBilledAmountSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiKeyBilledAmountSnapshot {
    pub currency: String,
    pub amount_nanos: i64,
}

#[derive(Clone, Debug, Default)]
pub struct ApiKeyCompletionDelta {
    pub api_key_id: i64,
    pub occurred_at: i64,
    pub total_tokens: i64,
    pub billed_amount_nanos: i64,
    pub billed_currency: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiKeyGovernanceAdmissionError {
    Internal(String),
    RateLimited {
        limit: i32,
        current: u32,
    },
    ConcurrencyLimited {
        limit: i32,
        current: u32,
    },
    DailyRequestQuotaExceeded {
        limit: i64,
        current: i64,
    },
    DailyTokenQuotaExceeded {
        limit: i64,
        current: i64,
    },
    MonthlyTokenQuotaExceeded {
        limit: i64,
        current: i64,
    },
    DailyBudgetExceeded {
        currency: String,
        limit_nanos: i64,
        current_nanos: i64,
    },
    MonthlyBudgetExceeded {
        currency: String,
        limit_nanos: i64,
        current_nanos: i64,
    },
}

#[derive(Clone, Debug, Default)]
struct ApiKeyRuntimeState {
    current_concurrency: u32,
    current_minute_bucket: Option<i64>,
    current_minute_request_count: u32,
    day_bucket: Option<i64>,
    daily_request_count: i64,
    daily_token_count: i64,
    daily_billed_amounts: HashMap<String, i64>,
    month_bucket: Option<i64>,
    monthly_token_count: i64,
    monthly_billed_amounts: HashMap<String, i64>,
}

#[derive(Clone, Default)]
struct ApiKeyGovernanceStore {
    inner: Arc<Mutex<HashMap<i64, ApiKeyRuntimeState>>>,
}

#[derive(Clone, Debug, Default)]
struct ApiKeyRollupBaseline {
    day_bucket: i64,
    daily_request_count: i64,
    daily_token_count: i64,
    daily_billed_amounts: HashMap<String, i64>,
    month_bucket: i64,
    monthly_token_count: i64,
    monthly_billed_amounts: HashMap<String, i64>,
}

impl ApiKeyGovernanceStore {
    fn try_begin_request(
        &self,
        api_key: &CacheApiKey,
        now_ms: i64,
    ) -> Result<Option<ApiKeyConcurrencyGuard>, ApiKeyGovernanceAdmissionError> {
        let mut guard = self.inner.lock().map_err(|e| {
            ApiKeyGovernanceAdmissionError::Internal(format!(
                "api key governance lock poisoned: {e}"
            ))
        })?;
        let state = guard.entry(api_key.id).or_default();
        state.check_admission_limits(api_key, now_ms)?;

        let concurrency_guard = match api_key.max_concurrent_requests {
            Some(limit) => {
                let limit = u32::try_from(limit).unwrap_or(0);
                if state.current_concurrency >= limit {
                    return Err(ApiKeyGovernanceAdmissionError::ConcurrencyLimited {
                        limit: api_key.max_concurrent_requests.unwrap_or_default(),
                        current: state.current_concurrency,
                    });
                }
                state.current_concurrency = state.current_concurrency.saturating_add(1);
                Some(ApiKeyConcurrencyGuard {
                    api_key_id: api_key.id,
                    store: Arc::clone(&self.inner),
                })
            }
            None => None,
        };

        state.record_request_admission();
        drop(guard);

        Ok(concurrency_guard)
    }

    fn try_acquire_concurrency(
        &self,
        api_key_id: i64,
        max_concurrent_requests: Option<i32>,
    ) -> Result<Option<ApiKeyConcurrencyGuard>, AppStoreError> {
        let Some(max_concurrent_requests) = max_concurrent_requests else {
            return Ok(None);
        };

        let max_concurrent_requests = u32::try_from(max_concurrent_requests).unwrap_or(0);
        let mut guard = self.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        let state = guard.entry(api_key_id).or_default();

        if state.current_concurrency >= max_concurrent_requests {
            return Ok(None);
        }

        state.current_concurrency = state.current_concurrency.saturating_add(1);
        drop(guard);

        Ok(Some(ApiKeyConcurrencyGuard {
            api_key_id,
            store: Arc::clone(&self.inner),
        }))
    }

    fn snapshot(&self, api_key_id: i64) -> Result<ApiKeyGovernanceSnapshot, AppStoreError> {
        let guard = self.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        Ok(match guard.get(&api_key_id) {
            Some(state) => state.snapshot(api_key_id),
            None => ApiKeyGovernanceSnapshot {
                api_key_id,
                current_concurrency: 0,
                current_minute_bucket: None,
                current_minute_request_count: 0,
                day_bucket: None,
                daily_request_count: 0,
                daily_token_count: 0,
                month_bucket: None,
                monthly_token_count: 0,
                daily_billed_amounts: vec![],
                monthly_billed_amounts: vec![],
            },
        })
    }

    fn snapshots(&self) -> Result<Vec<ApiKeyGovernanceSnapshot>, AppStoreError> {
        let guard = self.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        let mut snapshots = guard
            .iter()
            .filter(|(_, state)| state.is_active())
            .map(|(api_key_id, state)| state.snapshot(*api_key_id))
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|snapshot| snapshot.api_key_id);
        Ok(snapshots)
    }
}

impl ApiKeyRuntimeState {
    fn billed_amount_snapshots(amounts: &HashMap<String, i64>) -> Vec<ApiKeyBilledAmountSnapshot> {
        let mut snapshots = amounts
            .iter()
            .map(|(currency, amount_nanos)| ApiKeyBilledAmountSnapshot {
                currency: currency.clone(),
                amount_nanos: *amount_nanos,
            })
            .collect::<Vec<_>>();
        snapshots.sort_by(|a, b| a.currency.cmp(&b.currency));
        snapshots
    }

    fn snapshot(&self, api_key_id: i64) -> ApiKeyGovernanceSnapshot {
        ApiKeyGovernanceSnapshot {
            api_key_id,
            current_concurrency: self.current_concurrency,
            current_minute_bucket: self.current_minute_bucket,
            current_minute_request_count: self.current_minute_request_count,
            day_bucket: self.day_bucket,
            daily_request_count: self.daily_request_count,
            daily_token_count: self.daily_token_count,
            month_bucket: self.month_bucket,
            monthly_token_count: self.monthly_token_count,
            daily_billed_amounts: Self::billed_amount_snapshots(&self.daily_billed_amounts),
            monthly_billed_amounts: Self::billed_amount_snapshots(&self.monthly_billed_amounts),
        }
    }

    fn is_active(&self) -> bool {
        self.current_concurrency > 0
            || self.current_minute_request_count > 0
            || self.daily_request_count > 0
            || self.daily_token_count > 0
            || self.monthly_token_count > 0
            || self.daily_billed_amounts.values().any(|value| *value > 0)
            || self.monthly_billed_amounts.values().any(|value| *value > 0)
    }

    fn apply_rollup_baseline(&mut self, baseline: &ApiKeyRollupBaseline) {
        if self.day_bucket != Some(baseline.day_bucket) {
            self.day_bucket = Some(baseline.day_bucket);
            self.daily_request_count = baseline.daily_request_count;
            self.daily_token_count = baseline.daily_token_count;
            self.daily_billed_amounts = baseline.daily_billed_amounts.clone();
        }

        if self.month_bucket != Some(baseline.month_bucket) {
            self.month_bucket = Some(baseline.month_bucket);
            self.monthly_token_count = baseline.monthly_token_count;
            self.monthly_billed_amounts = baseline.monthly_billed_amounts.clone();
        }
    }

    fn refresh_minute_bucket(&mut self, minute_bucket: i64) {
        if self.current_minute_bucket != Some(minute_bucket) {
            self.current_minute_bucket = Some(minute_bucket);
            self.current_minute_request_count = 0;
        }
    }

    fn try_admit(
        &mut self,
        api_key: &CacheApiKey,
        now_ms: i64,
    ) -> Result<(), ApiKeyGovernanceAdmissionError> {
        self.check_admission_limits(api_key, now_ms)?;
        self.record_request_admission();
        Ok(())
    }

    fn check_admission_limits(
        &mut self,
        api_key: &CacheApiKey,
        now_ms: i64,
    ) -> Result<(), ApiKeyGovernanceAdmissionError> {
        let minute_bucket = AppState::minute_bucket_start(now_ms);
        self.refresh_minute_bucket(minute_bucket);

        if let Some(limit) = api_key.rate_limit_rpm {
            let limit = u32::try_from(limit).unwrap_or(0);
            if self.current_minute_request_count >= limit {
                return Err(ApiKeyGovernanceAdmissionError::RateLimited {
                    limit: api_key.rate_limit_rpm.unwrap_or_default(),
                    current: self.current_minute_request_count,
                });
            }
        }

        if let Some(limit) = api_key.quota_daily_requests {
            if self.daily_request_count >= limit {
                return Err(ApiKeyGovernanceAdmissionError::DailyRequestQuotaExceeded {
                    limit,
                    current: self.daily_request_count,
                });
            }
        }

        if let Some(limit) = api_key.quota_daily_tokens {
            if self.daily_token_count >= limit {
                return Err(ApiKeyGovernanceAdmissionError::DailyTokenQuotaExceeded {
                    limit,
                    current: self.daily_token_count,
                });
            }
        }

        if let Some(limit) = api_key.quota_monthly_tokens {
            if self.monthly_token_count >= limit {
                return Err(ApiKeyGovernanceAdmissionError::MonthlyTokenQuotaExceeded {
                    limit,
                    current: self.monthly_token_count,
                });
            }
        }

        if let (Some(limit_nanos), Some(currency)) = (
            api_key.budget_daily_nanos,
            api_key.budget_daily_currency.as_deref(),
        ) {
            let normalized_currency = AppState::normalize_currency_code(currency);
            let current_nanos = self
                .daily_billed_amounts
                .get(&normalized_currency)
                .copied()
                .unwrap_or_default();
            if current_nanos >= limit_nanos {
                return Err(ApiKeyGovernanceAdmissionError::DailyBudgetExceeded {
                    currency: normalized_currency,
                    limit_nanos,
                    current_nanos,
                });
            }
        }

        if let (Some(limit_nanos), Some(currency)) = (
            api_key.budget_monthly_nanos,
            api_key.budget_monthly_currency.as_deref(),
        ) {
            let normalized_currency = AppState::normalize_currency_code(currency);
            let current_nanos = self
                .monthly_billed_amounts
                .get(&normalized_currency)
                .copied()
                .unwrap_or_default();
            if current_nanos >= limit_nanos {
                return Err(ApiKeyGovernanceAdmissionError::MonthlyBudgetExceeded {
                    currency: normalized_currency,
                    limit_nanos,
                    current_nanos,
                });
            }
        }

        Ok(())
    }

    fn record_request_admission(&mut self) {
        self.current_minute_request_count = self.current_minute_request_count.saturating_add(1);
        self.daily_request_count = self.daily_request_count.saturating_add(1);
    }

    fn apply_completion(&mut self, delta: &ApiKeyCompletionDelta) {
        self.daily_token_count = self.daily_token_count.saturating_add(delta.total_tokens);
        self.monthly_token_count = self.monthly_token_count.saturating_add(delta.total_tokens);

        if let Some(currency) = delta.billed_currency.as_deref() {
            let normalized_currency = AppState::normalize_currency_code(currency);
            let daily_amount = self
                .daily_billed_amounts
                .entry(normalized_currency.clone())
                .or_default();
            *daily_amount = daily_amount.saturating_add(delta.billed_amount_nanos);

            let monthly_amount = self
                .monthly_billed_amounts
                .entry(normalized_currency)
                .or_default();
            *monthly_amount = monthly_amount.saturating_add(delta.billed_amount_nanos);
        }
    }
}

pub struct ApiKeyConcurrencyGuard {
    api_key_id: i64,
    store: Arc<Mutex<HashMap<i64, ApiKeyRuntimeState>>>,
}

impl Drop for ApiKeyConcurrencyGuard {
    fn drop(&mut self) {
        let Ok(mut guard) = self.store.lock() else {
            return;
        };

        let remove_entry = match guard.get_mut(&self.api_key_id) {
            Some(state) => {
                state.current_concurrency = state.current_concurrency.saturating_sub(1);
                !state.is_active()
            }
            None => false,
        };

        if remove_entry {
            guard.remove(&self.api_key_id);
        }
    }
}

#[derive(Clone, Debug)]
struct ProviderHealthState {
    status: ProviderHealthStatus,
    consecutive_failures: u32,
    opened_at_instant: Option<std::time::Instant>,
    opened_at: Option<i64>,
    half_open_probe_in_flight: bool,
    last_failure_at: Option<i64>,
    last_recovered_at: Option<i64>,
    last_error: Option<String>,
}

impl Default for ProviderHealthState {
    fn default() -> Self {
        Self {
            status: ProviderHealthStatus::Healthy,
            consecutive_failures: 0,
            opened_at_instant: None,
            opened_at: None,
            half_open_probe_in_flight: false,
            last_failure_at: None,
            last_recovered_at: None,
            last_error: None,
        }
    }
}

impl ProviderHealthState {
    fn snapshot(&self) -> ProviderHealthSnapshot {
        ProviderHealthSnapshot {
            status: self.status,
            consecutive_failures: self.consecutive_failures,
            half_open_probe_in_flight: self.half_open_probe_in_flight,
            opened_at: self.opened_at,
            last_failure_at: self.last_failure_at,
            last_recovered_at: self.last_recovered_at,
            last_error: self.last_error.clone(),
        }
    }

    fn allow_request(
        &mut self,
        config: &crate::config::ProviderGovernanceConfig,
        now: std::time::Instant,
    ) -> Result<(), Option<Duration>> {
        if !config.is_enabled() {
            return Ok(());
        }

        match self.status {
            ProviderHealthStatus::Healthy => Ok(()),
            ProviderHealthStatus::Open => {
                let Some(opened_at) = self.opened_at_instant else {
                    return Err(Some(config.open_cooldown()));
                };
                let elapsed = now.saturating_duration_since(opened_at);
                if elapsed < config.open_cooldown() {
                    return Err(Some(config.open_cooldown() - elapsed));
                }

                self.status = ProviderHealthStatus::HalfOpen;
                self.half_open_probe_in_flight = true;
                Ok(())
            }
            ProviderHealthStatus::HalfOpen => {
                if self.half_open_probe_in_flight {
                    Err(None)
                } else {
                    self.half_open_probe_in_flight = true;
                    Ok(())
                }
            }
        }
    }

    fn record_success(&mut self, now_ms: i64) {
        let was_unhealthy = self.status != ProviderHealthStatus::Healthy;
        self.status = ProviderHealthStatus::Healthy;
        self.consecutive_failures = 0;
        self.opened_at_instant = None;
        self.opened_at = None;
        self.half_open_probe_in_flight = false;
        if was_unhealthy {
            self.last_recovered_at = Some(now_ms);
        }
        self.last_error = None;
    }

    fn record_failure(
        &mut self,
        config: &crate::config::ProviderGovernanceConfig,
        now: std::time::Instant,
        now_ms: i64,
        error_message: String,
    ) {
        self.last_failure_at = Some(now_ms);
        self.last_error = Some(error_message);
        self.half_open_probe_in_flight = false;

        if !config.is_enabled() {
            self.status = ProviderHealthStatus::Healthy;
            self.consecutive_failures = 0;
            self.opened_at_instant = None;
            self.opened_at = None;
            return;
        }

        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.status == ProviderHealthStatus::HalfOpen
            || self.consecutive_failures >= config.consecutive_failure_threshold
        {
            self.status = ProviderHealthStatus::Open;
            self.opened_at_instant = Some(now);
            self.opened_at = Some(now_ms);
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    // 1. api_key(hash) -> CacheApiKey
    api_key_cache: CacheRepo<CacheApiKey>,

    // 2. route identity(key) -> route snapshot
    model_route_cache: CacheRepo<CacheModelRoute>,

    // 2a. api_key/source_name -> target route snapshot
    api_key_override_route_cache: CacheRepo<CacheModelRoute>,

    // 2b. aggregate listing snapshot for `/models`
    models_catalog_cache: CacheRepo<CacheModelsCatalog>,

    // 3, 4. provider_id(id)/provider_key(key) -> CacheProvider
    provider_cache: CacheRepo<CacheProvider>,

    // 5. model_name(key) -> CacheModel (Also keyed by ID for internal resolution)
    model_cache: CacheRepo<CacheModel>,

    // 6. provider_id(id) -> CacheProviderKey[]
    provider_api_keys_cache: CacheRepo<Vec<CacheProviderKey>>,

    // 7. provider_id(id) -> provider direct request patch rules
    provider_request_patch_rules_cache: CacheRepo<Vec<CacheRequestPatchRule>>,

    // 8. model_id(id) -> model direct request patch rules
    model_request_patch_rules_cache: CacheRepo<Vec<CacheRequestPatchRule>>,

    // 9. model_id(id) -> resolved effective request patches
    model_effective_request_patches_cache: CacheRepo<CacheResolvedModelRequestPatches>,

    // 10. cost_catalog_version_id(id) -> CacheCostCatalogVersion
    cost_catalog_version_cache: CacheRepo<CacheCostCatalogVersion>,

    // HTTP clients
    pub client: Client,
    pub proxy_client: Client,

    // Config for negative caching TTL
    negative_cache_ttl: Duration,

    // In-process selection cursor for provider API key queue mode.
    provider_key_queue_state: Arc<tokio::sync::Mutex<HashMap<i64, usize>>>,

    // In-process API key governance runtime state (admission counters, etc.).
    api_key_governance_store: ApiKeyGovernanceStore,

    // In-process provider governance state for circuit-open / half-open decisions.
    provider_health_state: Arc<tokio::sync::Mutex<HashMap<i64, ProviderHealthState>>>,
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
            client,
            proxy_client,
            negative_cache_ttl,
            provider_key_queue_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            api_key_governance_store: ApiKeyGovernanceStore::default(),
            provider_health_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    fn build_http_client(use_proxy: bool) -> Client {
        let proxy_request_config = &CONFIG.proxy_request;
        let connect_timeout = proxy_request_config.connect_timeout();
        let total_timeout = proxy_request_config.total_timeout();

        let mut builder = Client::builder().connect_timeout(connect_timeout);

        if let Some(timeout) = total_timeout {
            builder = builder.timeout(timeout);
        }

        if use_proxy {
            if let Some(proxy_url) = &CONFIG.proxy {
                let proxy = Proxy::all(proxy_url).expect("Invalid proxy URL in configuration");
                builder = builder.proxy(proxy);
            }
        }

        info!(
            "Building reqwest client (use_proxy: {}, connect_timeout: {:?}, first_byte_timeout: {:?}, total_timeout: {:?})",
            use_proxy,
            connect_timeout,
            proxy_request_config.first_byte_timeout(),
            total_timeout
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
        info!("Reloading AppState: Starting cache refresh...");
        let mut stats: HashMap<&'static str, usize> = HashMap::new();
        let mut catalog_providers = Vec::new();
        let mut catalog_models = Vec::new();
        let mut catalog_routes = Vec::new();
        let mut catalog_api_key_overrides = Vec::new();

        // 1. API keys with embedded ACL snapshots
        if let Ok(keys) = ApiKey::list_all_active() {
            stats.insert("API Keys", keys.len());
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
                    Err(err) => {
                        error!("Failed to preload api key cache snapshot: {}", err);
                    }
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

        // 5. Models
        if let Ok(models) = Model::list_all() {
            stats.insert("Models", models.len());
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

        // 2. Logical model routes
        if let Ok(routes) = ModelRoute::list_summary() {
            stats.insert("Model Routes", routes.len());
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
                    Err(err) => {
                        error!(
                            "Failed to preload model route {} into cache: {:?}",
                            route_item.route.id, err
                        );
                    }
                }
            }
        }

        // 2a. API key scoped overrides
        if let Ok(overrides) = ApiKeyModelOverride::list_all() {
            stats.insert("API Key Model Overrides", overrides.len());
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
                    Err(err) => {
                        error!(
                            "Failed to preload api key model override {} into cache: {}",
                            override_row.id, err
                        );
                    }
                }
            }
        }

        let models_catalog = CacheModelsCatalog {
            providers: catalog_providers.clone(),
            models: catalog_models.clone(),
            routes: catalog_routes.clone(),
            api_key_overrides: catalog_api_key_overrides.clone(),
        };
        let _ = self
            .models_catalog_cache
            .set_positive(
                &CacheKey::ModelsCatalog.to_compact_string(),
                &models_catalog,
            )
            .await;

        // 6. Provider API Keys
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

        // 7, 8, 9. Request patch direct/effective caches
        if let Ok(all_rules) = RequestPatchRule::list_all() {
            stats.insert("Request Patch Rules", all_rules.len());

            let mut provider_rules_by_id: HashMap<i64, Vec<CacheRequestPatchRule>> = HashMap::new();
            let mut model_rules_by_id: HashMap<i64, Vec<CacheRequestPatchRule>> = HashMap::new();

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
                Err(err) => {
                    error!("Failed to preload request patch rules into cache: {}", err);
                }
            }

            stats.insert("Provider Request Patch Groups", provider_rules_by_id.len());
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

            stats.insert("Model Request Patch Groups", model_rules_by_id.len());
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
            stats.insert("Model Effective Request Patch Groups", catalog_models.len());
        }

        // 10. Cost Catalog Versions
        if let Ok(versions) = CostCatalogVersion::list_all() {
            stats.insert("Cost Catalog Versions", versions.len());
            let mut components_by_version: HashMap<i64, Vec<CostComponent>> = HashMap::new();
            for component in CostComponent::list_all().unwrap_or_default() {
                components_by_version
                    .entry(component.catalog_version_id)
                    .or_default()
                    .push(component);
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

        info!(
            "AppState reloaded successfully. Cache details:\n{:#?}",
            stats
        );
    }

    pub async fn clear_cache(&self) {
        info!("Clearing app cache...");
        // Since all redis keys share the same prefix, we only need to clear one of the repos
        // to clear all of them. For memory cache, each repo is a separate instance.
        if let Err(e) = self.api_key_cache.clear().await {
            cyder_tools::log::error!("Failed to clear api_key_cache: {}", e);
        }
        if let Err(e) = self.models_catalog_cache.clear().await {
            cyder_tools::log::error!("Failed to clear models_catalog_cache: {}", e);
        }
        if let Err(e) = self.provider_cache.clear().await {
            cyder_tools::log::error!("Failed to clear provider_cache: {}", e);
        }
        if let Err(e) = self.model_cache.clear().await {
            cyder_tools::log::error!("Failed to clear model_cache: {}", e);
        }
        if let Err(e) = self.provider_api_keys_cache.clear().await {
            cyder_tools::log::error!("Failed to clear provider_api_keys_cache: {}", e);
        }
        if let Err(e) = self.provider_request_patch_rules_cache.clear().await {
            cyder_tools::log::error!("Failed to clear provider_request_patch_rules_cache: {}", e);
        }
        if let Err(e) = self.model_request_patch_rules_cache.clear().await {
            cyder_tools::log::error!("Failed to clear model_request_patch_rules_cache: {}", e);
        }
        if let Err(e) = self.model_effective_request_patches_cache.clear().await {
            cyder_tools::log::error!(
                "Failed to clear model_effective_request_patches_cache: {}",
                e
            );
        }
        if let Err(e) = self.cost_catalog_version_cache.clear().await {
            cyder_tools::log::error!("Failed to clear cost_catalog_version_cache: {}", e);
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
    // 1. api_key(hash) -> CacheApiKey
    // ============================================================================================
    pub async fn get_system_api_key(
        &self,
        key: &str,
    ) -> Result<Option<Arc<CacheSystemApiKey>>, AppStoreError> {
        let hashed_key = Self::hash_api_key(key);
        let cache_key = CacheKey::ApiKeyHash(&hashed_key).to_compact_string();
        let now = chrono::Utc::now().timestamp_millis();

        let result = self
            .get_or_load(&self.api_key_cache, &cache_key, || async {
                match ApiKey::get_active_by_hash(&hashed_key) {
                    Ok(db_key) => Ok(Some(Self::load_cache_api_key(db_key)?)),
                    Err(crate::controller::BaseError::NotFound(_)) => Ok(None),
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

            debug!("api key cache entry expired in memory: {}", cache_key);
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
                    Err(crate::controller::BaseError::NotFound(_)) => Ok(None),
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

            debug!("api key cache entry expired in memory: {}", cache_key);
            self.api_key_cache.delete(&cache_key).await?;
            return Ok(None);
        }

        Ok(None)
    }

    pub async fn invalidate_api_key_hash(&self, api_key_hash: &str) -> Result<(), AppStoreError> {
        let cache_key_to_find = CacheKey::ApiKeyHash(api_key_hash).to_compact_string();
        debug!("invalidate: {}", &cache_key_to_find);
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

    pub async fn invalidate_system_api_key(&self, key: &str) -> Result<(), AppStoreError> {
        self.invalidate_api_key_hash(&Self::hash_api_key(key)).await
    }

    // Legacy compatibility shim. Task 4 no longer keeps standalone ACL cache.
    pub async fn invalidate_access_control_policy(&self, _id: i64) -> Result<(), AppStoreError> {
        Ok(())
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
        debug!("invalidate: {}", &cache_key);
        Ok(self.model_route_cache.delete(&cache_key).await?)
    }

    pub async fn invalidate_api_key_model_override(
        &self,
        api_key_id: i64,
        source_name: &str,
    ) -> Result<(), AppStoreError> {
        let cache_key = CacheKey::ApiKeyModelOverride(api_key_id, source_name).to_compact_string();
        debug!("invalidate: {}", &cache_key);
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
        debug!(
            "invalidate model route: id={}, name={:?}",
            route_id, route_name
        );
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
        debug!("invalidate: {}", &cache_key);
        Ok(self.models_catalog_cache.delete(&cache_key).await?)
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

    // Internal helper for route/direct resolution + lazy load by ID.
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
        self.invalidate_models_catalog().await?;
        let _ = self.invalidate_model_routes_for_model(id).await;
        let _ = self.invalidate_model_request_patch_rules(id).await;
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
    // 6. provider_id(id) -> CacheProviderKey[]
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

    fn minute_bucket_start(timestamp_ms: i64) -> i64 {
        timestamp_ms.div_euclid(60_000) * 60_000
    }

    fn day_bucket_start(timestamp_ms: i64) -> i64 {
        timestamp_ms.div_euclid(86_400_000) * 86_400_000
    }

    fn month_bucket_start(timestamp_ms: i64) -> i64 {
        let timestamp = Utc
            .timestamp_millis_opt(timestamp_ms)
            .single()
            .unwrap_or_else(Utc::now);
        Utc.with_ymd_and_hms(timestamp.year(), timestamp.month(), 1, 0, 0, 0)
            .single()
            .expect("month bucket should be valid")
            .timestamp_millis()
    }

    fn normalize_currency_code(currency: &str) -> String {
        currency.trim().to_ascii_uppercase()
    }

    async fn load_api_key_rollup_baseline(
        &self,
        api_key_id: i64,
        timestamp_ms: i64,
    ) -> Result<ApiKeyRollupBaseline, AppStoreError> {
        let day_bucket = Self::day_bucket_start(timestamp_ms);
        let month_bucket = Self::month_bucket_start(timestamp_ms);
        let daily_rows =
            ApiKeyRollupDaily::list_by_bucket(api_key_id, day_bucket).map_err(|err| {
                AppStoreError::DatabaseError(format!(
                    "failed to load api key daily rollup baseline for {}: {:?}",
                    api_key_id, err
                ))
            })?;
        let monthly_rows =
            ApiKeyRollupMonthly::list_by_bucket(api_key_id, month_bucket).map_err(|err| {
                AppStoreError::DatabaseError(format!(
                    "failed to load api key monthly rollup baseline for {}: {:?}",
                    api_key_id, err
                ))
            })?;

        let mut baseline = ApiKeyRollupBaseline {
            day_bucket,
            month_bucket,
            ..ApiKeyRollupBaseline::default()
        };

        for row in daily_rows {
            baseline.daily_request_count = baseline
                .daily_request_count
                .saturating_add(row.request_count);
            baseline.daily_token_count =
                baseline.daily_token_count.saturating_add(row.total_tokens);
            let currency = Self::normalize_currency_code(&row.currency);
            let amount = baseline.daily_billed_amounts.entry(currency).or_default();
            *amount = amount.saturating_add(row.billed_amount_nanos);
        }

        for row in monthly_rows {
            baseline.monthly_token_count = baseline
                .monthly_token_count
                .saturating_add(row.total_tokens);
            let currency = Self::normalize_currency_code(&row.currency);
            let amount = baseline.monthly_billed_amounts.entry(currency).or_default();
            *amount = amount.saturating_add(row.billed_amount_nanos);
        }

        Ok(baseline)
    }

    async fn ensure_api_key_governance_usage_state(
        &self,
        api_key_id: i64,
        timestamp_ms: i64,
    ) -> Result<(), AppStoreError> {
        let day_bucket = Self::day_bucket_start(timestamp_ms);
        let month_bucket = Self::month_bucket_start(timestamp_ms);
        let needs_reload = {
            let guard = self.api_key_governance_store.inner.lock().map_err(|e| {
                AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
            })?;
            match guard.get(&api_key_id) {
                Some(state) => {
                    state.day_bucket != Some(day_bucket) || state.month_bucket != Some(month_bucket)
                }
                None => true,
            }
        };

        if !needs_reload {
            return Ok(());
        }

        let baseline = self
            .load_api_key_rollup_baseline(api_key_id, timestamp_ms)
            .await?;
        let mut guard = self.api_key_governance_store.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        let state = guard.entry(api_key_id).or_default();
        state.apply_rollup_baseline(&baseline);
        Ok(())
    }

    pub fn try_acquire_api_key_concurrency(
        &self,
        api_key_id: i64,
        max_concurrent_requests: Option<i32>,
    ) -> Result<Option<ApiKeyConcurrencyGuard>, AppStoreError> {
        self.api_key_governance_store
            .try_acquire_concurrency(api_key_id, max_concurrent_requests)
    }

    pub fn get_api_key_governance_snapshot(
        &self,
        api_key_id: i64,
    ) -> Result<ApiKeyGovernanceSnapshot, AppStoreError> {
        self.api_key_governance_store.snapshot(api_key_id)
    }

    pub fn list_api_key_governance_snapshots(
        &self,
    ) -> Result<Vec<ApiKeyGovernanceSnapshot>, AppStoreError> {
        self.api_key_governance_store.snapshots()
    }

    pub async fn try_admit_api_key_governance(
        &self,
        api_key: &CacheApiKey,
    ) -> Result<(), ApiKeyGovernanceAdmissionError> {
        let now_ms = Utc::now().timestamp_millis();
        self.ensure_api_key_governance_usage_state(api_key.id, now_ms)
            .await
            .map_err(|err| ApiKeyGovernanceAdmissionError::Internal(err.to_string()))?;

        let mut guard = self.api_key_governance_store.inner.lock().map_err(|e| {
            ApiKeyGovernanceAdmissionError::Internal(format!(
                "api key governance lock poisoned: {e}"
            ))
        })?;
        let state = guard.entry(api_key.id).or_default();
        state.try_admit(api_key, now_ms)
    }

    pub async fn try_begin_api_key_request(
        &self,
        api_key: &CacheApiKey,
    ) -> Result<Option<ApiKeyConcurrencyGuard>, ApiKeyGovernanceAdmissionError> {
        let now_ms = Utc::now().timestamp_millis();
        self.ensure_api_key_governance_usage_state(api_key.id, now_ms)
            .await
            .map_err(|err| ApiKeyGovernanceAdmissionError::Internal(err.to_string()))?;
        self.api_key_governance_store
            .try_begin_request(api_key, now_ms)
    }

    pub async fn record_api_key_completion(
        &self,
        delta: &ApiKeyCompletionDelta,
    ) -> Result<(), AppStoreError> {
        self.ensure_api_key_governance_usage_state(delta.api_key_id, delta.occurred_at)
            .await?;

        let mut guard = self.api_key_governance_store.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        let state = guard.entry(delta.api_key_id).or_default();
        state.apply_completion(delta);
        Ok(())
    }

    pub async fn allow_provider_request(
        &self,
        provider_id: i64,
    ) -> Result<ProviderHealthSnapshot, Option<Duration>> {
        let now = std::time::Instant::now();
        let mut state = self.provider_health_state.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        provider_state
            .allow_request(&CONFIG.provider_governance, now)
            .map(|_| provider_state.snapshot())
    }

    pub async fn record_provider_success(&self, provider_id: i64) -> ProviderHealthSnapshot {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut state = self.provider_health_state.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        provider_state.record_success(now_ms);
        provider_state.snapshot()
    }

    pub async fn record_provider_failure(
        &self,
        provider_id: i64,
        error_message: String,
    ) -> ProviderHealthSnapshot {
        let now = std::time::Instant::now();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut state = self.provider_health_state.lock().await;
        let provider_state = state.entry(provider_id).or_default();
        provider_state.record_failure(&CONFIG.provider_governance, now, now_ms, error_message);
        provider_state.snapshot()
    }

    pub async fn get_provider_health_snapshot(&self, provider_id: i64) -> ProviderHealthSnapshot {
        let state = self.provider_health_state.lock().await;
        state
            .get(&provider_id)
            .cloned()
            .unwrap_or_default()
            .snapshot()
    }

    // ============================================================================================
    // 7. provider_id(id) -> provider direct request patch rules
    // ============================================================================================
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
        debug!("invalidate: {}", &cache_key);
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

    // ============================================================================================
    // 8. model_id(id) -> model direct request patch rules
    // ============================================================================================
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
        debug!("invalidate: {}", &cache_key);
        self.model_request_patch_rules_cache
            .delete(&cache_key)
            .await?;
        self.invalidate_model_effective_request_patches(model_id)
            .await
    }

    // ============================================================================================
    // 9. model_id(id) -> resolved effective request patches
    // ============================================================================================
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
        debug!("invalidate: {}", &cache_key);
        Ok(self
            .model_effective_request_patches_cache
            .delete(&cache_key)
            .await?)
    }

    // ============================================================================================
    // 10. cost_catalog_version_id(id) -> CacheCostCatalogVersion
    // ============================================================================================
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
        debug!("invalidate: {}", &cache_key);
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

        Ok(CacheModelsCatalog {
            providers,
            models,
            routes,
            api_key_overrides,
        })
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
    use super::{
        ApiKeyBilledAmountSnapshot, ApiKeyCompletionDelta, ApiKeyGovernanceAdmissionError,
        ApiKeyGovernanceSnapshot, ApiKeyRollupBaseline, ApiKeyRuntimeState, AppState, CacheKey,
        GroupItemSelectionStrategy, ProviderHealthState, ProviderHealthStatus,
    };
    use crate::config::ProviderGovernanceConfig;
    use crate::schema::enum_def::{Action, ProviderApiKeyMode};
    use crate::service::cache::types::{
        CacheApiKey, CacheCostCatalogVersion, CacheEntry, CacheModelRoute, CacheModelRouteCandidate,
    };
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn governance_snapshot(api_key_id: i64, current_concurrency: u32) -> ApiKeyGovernanceSnapshot {
        ApiKeyGovernanceSnapshot {
            api_key_id,
            current_concurrency,
            current_minute_bucket: None,
            current_minute_request_count: 0,
            day_bucket: None,
            daily_request_count: 0,
            daily_token_count: 0,
            month_bucket: None,
            monthly_token_count: 0,
            daily_billed_amounts: vec![],
            monthly_billed_amounts: vec![],
        }
    }

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

    fn test_app_state() -> AppState {
        let ttl = Some(Duration::from_secs(60));

        AppState {
            api_key_cache: AppState::create_repo(ttl, None),
            model_route_cache: AppState::create_repo(ttl, None),
            api_key_override_route_cache: AppState::create_repo(ttl, None),
            models_catalog_cache: AppState::create_repo(ttl, None),
            provider_cache: AppState::create_repo(ttl, None),
            model_cache: AppState::create_repo(ttl, None),
            provider_api_keys_cache: AppState::create_repo(ttl, None),
            provider_request_patch_rules_cache: AppState::create_repo(ttl, None),
            model_request_patch_rules_cache: AppState::create_repo(ttl, None),
            model_effective_request_patches_cache: AppState::create_repo(ttl, None),
            cost_catalog_version_cache: AppState::create_repo(ttl, None),
            client: AppState::build_http_client(false),
            proxy_client: AppState::build_http_client(true),
            negative_cache_ttl: Duration::from_secs(1),
            provider_key_queue_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            api_key_governance_store: super::ApiKeyGovernanceStore::default(),
            provider_health_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

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

    #[test]
    fn provider_health_opens_after_threshold_failures() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 2,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let now_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();

        state.record_failure(&config, now, now_ms, "timeout".to_string());
        assert_eq!(state.status, ProviderHealthStatus::Healthy);
        assert_eq!(state.consecutive_failures, 1);
        assert_eq!(state.last_failure_at, Some(now_ms));
        assert!(state.opened_at.is_none());

        state.record_failure(&config, now, now_ms + 1_000, "another timeout".to_string());
        assert_eq!(state.status, ProviderHealthStatus::Open);
        assert_eq!(state.consecutive_failures, 2);
        assert_eq!(state.last_failure_at, Some(now_ms + 1_000));
        assert_eq!(state.opened_at, Some(now_ms + 1_000));
        assert!(state.opened_at.is_some());
        assert!(state.opened_at_instant.is_some());
    }

    #[test]
    fn provider_health_transitions_to_half_open_after_cooldown() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let now_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, now, now_ms, "timeout".to_string());

        assert_eq!(
            state.allow_request(&config, now + Duration::from_secs(10)),
            Err(Some(Duration::from_secs(20)))
        );
        assert_eq!(state.status, ProviderHealthStatus::Open);

        assert_eq!(
            state.allow_request(&config, now + Duration::from_secs(31)),
            Ok(())
        );
        assert_eq!(state.status, ProviderHealthStatus::HalfOpen);
        assert!(state.half_open_probe_in_flight);
    }

    #[test]
    fn provider_health_half_open_success_closes_circuit() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let open_at_ms = 1_700_000_000_000;
        let recover_at_ms = open_at_ms + 31_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, now, open_at_ms, "timeout".to_string());
        state
            .allow_request(&config, now + Duration::from_secs(31))
            .expect("half-open probe should be allowed");

        state.record_success(recover_at_ms);
        assert_eq!(state.status, ProviderHealthStatus::Healthy);
        assert_eq!(state.consecutive_failures, 0);
        assert!(!state.half_open_probe_in_flight);
        assert_eq!(state.last_recovered_at, Some(recover_at_ms));
        assert_eq!(state.last_failure_at, Some(open_at_ms));
        assert!(state.opened_at.is_none());
        assert!(state.opened_at_instant.is_none());
        assert!(state.last_error.is_none());
    }

    #[test]
    fn provider_health_half_open_failure_reopens_circuit() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 3,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let first_failure_at_ms = 1_700_000_000_000;
        let mut state = ProviderHealthState::default();
        state.record_failure(&config, now, first_failure_at_ms, "timeout".to_string());
        state.record_failure(
            &config,
            now,
            first_failure_at_ms + 1_000,
            "timeout".to_string(),
        );
        state.record_failure(
            &config,
            now,
            first_failure_at_ms + 2_000,
            "timeout".to_string(),
        );
        state
            .allow_request(&config, now + Duration::from_secs(31))
            .expect("half-open probe should be allowed");

        state.record_failure(
            &config,
            now + Duration::from_secs(31),
            first_failure_at_ms + 31_000,
            "half-open timeout".to_string(),
        );
        assert_eq!(state.status, ProviderHealthStatus::Open);
        assert!(!state.half_open_probe_in_flight);
        assert_eq!(state.last_failure_at, Some(first_failure_at_ms + 31_000));
        assert_eq!(state.opened_at, Some(first_failure_at_ms + 31_000));
    }

    #[test]
    fn provider_health_snapshot_exposes_runtime_timestamps() {
        let config = ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: 1,
            open_cooldown_seconds: 30,
        };
        let now = Instant::now();
        let open_at_ms = 1_700_000_000_000;
        let recover_at_ms = open_at_ms + 31_000;
        let mut state = ProviderHealthState::default();

        state.record_failure(&config, now, open_at_ms, "timeout".to_string());
        let open_snapshot = state.snapshot();
        assert_eq!(open_snapshot.opened_at, Some(open_at_ms));
        assert_eq!(open_snapshot.last_failure_at, Some(open_at_ms));
        assert_eq!(open_snapshot.last_recovered_at, None);
        assert_eq!(open_snapshot.last_error.as_deref(), Some("timeout"));

        state
            .allow_request(&config, now + Duration::from_secs(31))
            .expect("half-open probe should be allowed");
        state.record_success(recover_at_ms);

        let recovered_snapshot = state.snapshot();
        assert_eq!(recovered_snapshot.opened_at, None);
        assert_eq!(recovered_snapshot.last_failure_at, Some(open_at_ms));
        assert_eq!(recovered_snapshot.last_recovered_at, Some(recover_at_ms));
        assert!(recovered_snapshot.last_error.is_none());
    }

    #[tokio::test]
    async fn invalidate_cost_catalog_version_removes_cached_snapshot() {
        let app_state = test_app_state();
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

        app_state
            .cost_catalog_version_cache
            .set_positive(&cache_key, &cached_version)
            .await
            .expect("seed cache");

        let cached = app_state
            .cost_catalog_version_cache
            .get_entry(&cache_key)
            .await
            .expect("read cache before invalidate");
        assert!(matches!(cached.as_deref(), Some(CacheEntry::Positive(_))));

        app_state
            .invalidate_cost_catalog_version(88)
            .await
            .expect("invalidate version");

        let cached_after = app_state
            .cost_catalog_version_cache
            .get_entry(&cache_key)
            .await
            .expect("read cache after invalidate");
        assert!(cached_after.is_none());
    }

    #[tokio::test]
    async fn invalidate_model_route_by_name_removes_cached_snapshot() {
        let app_state = test_app_state();
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

        app_state
            .model_route_cache
            .set_positive(&cache_key, &route)
            .await
            .expect("seed route cache");

        app_state
            .invalidate_model_route_by_name("manual-smoke-route")
            .await
            .expect("invalidate route");

        let cached_after = app_state
            .model_route_cache
            .get_entry(&cache_key)
            .await
            .expect("read route cache after invalidate");
        assert!(cached_after.is_none());
    }

    #[tokio::test]
    async fn invalidate_api_key_model_override_removes_cached_snapshot() {
        let app_state = test_app_state();
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

        app_state
            .api_key_override_route_cache
            .set_positive(&cache_key, &route)
            .await
            .expect("seed override cache");

        app_state
            .invalidate_api_key_model_override(7, "manual-cli-model")
            .await
            .expect("invalidate override");

        let cached_after = app_state
            .api_key_override_route_cache
            .get_entry(&cache_key)
            .await
            .expect("read override cache after invalidate");
        assert!(cached_after.is_none());
    }

    #[test]
    fn api_key_concurrency_guard_releases_slots_on_drop() {
        let store = super::ApiKeyGovernanceStore::default();

        let first_guard = store
            .try_acquire_concurrency(42, Some(2))
            .expect("acquire first slot")
            .expect("limit should create guard");
        let second_guard = store
            .try_acquire_concurrency(42, Some(2))
            .expect("acquire second slot")
            .expect("limit should create guard");

        let snapshot = store.snapshot(42).expect("snapshot");
        assert_eq!(snapshot, governance_snapshot(42, 2));

        assert!(
            store
                .try_acquire_concurrency(42, Some(2))
                .expect("third acquire should not error")
                .is_none()
        );

        drop(first_guard);
        assert_eq!(
            store.snapshot(42).expect("snapshot after drop"),
            governance_snapshot(42, 1)
        );

        drop(second_guard);
        assert_eq!(
            store.snapshot(42).expect("final snapshot"),
            governance_snapshot(42, 0)
        );
    }

    #[test]
    fn api_key_governance_snapshots_are_sorted_and_only_include_active_entries() {
        let store = super::ApiKeyGovernanceStore::default();
        let _guard_b = store
            .try_acquire_concurrency(9, Some(1))
            .expect("acquire tracked slot")
            .expect("guard");
        let guard_a = store
            .try_acquire_concurrency(3, Some(1))
            .expect("acquire tracked slot")
            .expect("guard");

        let snapshots = store.snapshots().expect("snapshots");
        assert_eq!(
            snapshots,
            vec![governance_snapshot(3, 1), governance_snapshot(9, 1),]
        );

        drop(guard_a);
        assert_eq!(
            store.snapshots().expect("snapshots after release"),
            vec![governance_snapshot(9, 1)]
        );
    }

    #[test]
    fn begin_request_keeps_usage_counters_when_concurrency_guard_drops() {
        let store = super::ApiKeyGovernanceStore::default();
        let api_key = cache_api_key();
        let now_ms = 1_744_000_000_000;

        let guard = store
            .try_begin_request(&api_key, now_ms)
            .expect("begin request")
            .expect("concurrency limit should create guard");

        let snapshot = store.snapshot(api_key.id).expect("snapshot after begin");
        assert_eq!(snapshot.current_concurrency, 1);
        assert_eq!(snapshot.current_minute_request_count, 1);
        assert_eq!(snapshot.daily_request_count, 1);

        drop(guard);

        let snapshot = store
            .snapshot(api_key.id)
            .expect("snapshot after concurrency release");
        assert_eq!(snapshot.current_concurrency, 0);
        assert_eq!(snapshot.current_minute_request_count, 1);
        assert_eq!(snapshot.daily_request_count, 1);
    }

    #[test]
    fn api_key_runtime_state_blocks_rate_quota_and_budget_limits() {
        let api_key = cache_api_key();
        let now_ms = 1_744_000_000_000;
        let minute_bucket = AppState::minute_bucket_start(now_ms);
        let day_bucket = AppState::day_bucket_start(now_ms);
        let month_bucket = AppState::month_bucket_start(now_ms);

        let mut state = ApiKeyRuntimeState {
            current_minute_bucket: Some(minute_bucket),
            current_minute_request_count: 2,
            day_bucket: Some(day_bucket),
            daily_request_count: 3,
            daily_token_count: 100,
            month_bucket: Some(month_bucket),
            monthly_token_count: 200,
            daily_billed_amounts: HashMap::from([(String::from("USD"), 50)]),
            monthly_billed_amounts: HashMap::from([(String::from("USD"), 80)]),
            ..ApiKeyRuntimeState::default()
        };

        assert!(matches!(
            state.try_admit(&api_key, now_ms),
            Err(ApiKeyGovernanceAdmissionError::RateLimited { .. })
        ));

        state.current_minute_request_count = 0;
        assert!(matches!(
            state.try_admit(&api_key, now_ms),
            Err(ApiKeyGovernanceAdmissionError::DailyRequestQuotaExceeded { .. })
        ));

        state.daily_request_count = 0;
        assert!(matches!(
            state.try_admit(&api_key, now_ms),
            Err(ApiKeyGovernanceAdmissionError::DailyTokenQuotaExceeded { .. })
        ));

        state.daily_token_count = 0;
        assert!(matches!(
            state.try_admit(&api_key, now_ms),
            Err(ApiKeyGovernanceAdmissionError::MonthlyTokenQuotaExceeded { .. })
        ));

        state.monthly_token_count = 0;
        assert!(matches!(
            state.try_admit(&api_key, now_ms),
            Err(ApiKeyGovernanceAdmissionError::DailyBudgetExceeded { .. })
        ));

        state.daily_billed_amounts.clear();
        assert!(matches!(
            state.try_admit(&api_key, now_ms),
            Err(ApiKeyGovernanceAdmissionError::MonthlyBudgetExceeded { .. })
        ));
    }

    #[test]
    fn api_key_runtime_state_records_completion_usage_and_normalizes_currency() {
        let mut state = ApiKeyRuntimeState::default();
        state.apply_completion(&ApiKeyCompletionDelta {
            api_key_id: 42,
            occurred_at: 1_744_000_000_000,
            total_tokens: 33,
            billed_amount_nanos: 21,
            billed_currency: Some("usd".to_string()),
        });

        assert_eq!(state.daily_token_count, 33);
        assert_eq!(state.monthly_token_count, 33);
        assert_eq!(state.daily_billed_amounts.get("USD"), Some(&21));
        assert_eq!(state.monthly_billed_amounts.get("USD"), Some(&21));
    }

    #[test]
    fn api_key_runtime_state_restores_rollup_baseline_with_multi_currency_amounts() {
        let mut state = ApiKeyRuntimeState::default();
        let baseline = ApiKeyRollupBaseline {
            day_bucket: 1_744_000_000_000,
            daily_request_count: 5,
            daily_token_count: 144,
            daily_billed_amounts: HashMap::from([
                (String::from("USD"), 21),
                (String::from("EUR"), 34),
            ]),
            month_bucket: 1_743_984_000_000,
            monthly_token_count: 233,
            monthly_billed_amounts: HashMap::from([
                (String::from("USD"), 55),
                (String::from("JPY"), 89),
            ]),
        };

        state.apply_rollup_baseline(&baseline);

        let snapshot = state.snapshot(42);
        assert_eq!(snapshot.day_bucket, Some(baseline.day_bucket));
        assert_eq!(snapshot.daily_request_count, 5);
        assert_eq!(snapshot.daily_token_count, 144);
        assert_eq!(
            snapshot.daily_billed_amounts,
            vec![
                ApiKeyBilledAmountSnapshot {
                    currency: "EUR".to_string(),
                    amount_nanos: 34,
                },
                ApiKeyBilledAmountSnapshot {
                    currency: "USD".to_string(),
                    amount_nanos: 21,
                },
            ]
        );
        assert_eq!(snapshot.month_bucket, Some(baseline.month_bucket));
        assert_eq!(snapshot.monthly_token_count, 233);
        assert_eq!(
            snapshot.monthly_billed_amounts,
            vec![
                ApiKeyBilledAmountSnapshot {
                    currency: "JPY".to_string(),
                    amount_nanos: 89,
                },
                ApiKeyBilledAmountSnapshot {
                    currency: "USD".to_string(),
                    amount_nanos: 55,
                },
            ]
        );
    }

    #[test]
    fn api_key_runtime_state_budget_checks_are_currency_specific() {
        let api_key = CacheApiKey {
            budget_daily_nanos: Some(10),
            budget_daily_currency: Some("usd".to_string()),
            budget_monthly_nanos: Some(20),
            budget_monthly_currency: Some("usd".to_string()),
            rate_limit_rpm: None,
            max_concurrent_requests: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            ..cache_api_key()
        };
        let now_ms = 1_744_000_000_000;
        let mut state = ApiKeyRuntimeState {
            current_minute_bucket: Some(AppState::minute_bucket_start(now_ms)),
            day_bucket: Some(AppState::day_bucket_start(now_ms)),
            month_bucket: Some(AppState::month_bucket_start(now_ms)),
            daily_billed_amounts: HashMap::from([(String::from("EUR"), 999)]),
            monthly_billed_amounts: HashMap::from([(String::from("JPY"), 999)]),
            ..ApiKeyRuntimeState::default()
        };

        state
            .try_admit(&api_key, now_ms)
            .expect("non-matching currencies should not exhaust USD budgets");

        assert_eq!(state.daily_request_count, 1);
        assert_eq!(state.current_minute_request_count, 1);
    }

    #[tokio::test]
    async fn expired_api_key_cache_hit_is_evicted() {
        let app_state = AppState::new().await;
        let expired_at = Utc::now().timestamp_millis() - 1;
        let api_key_hash = "expired-hash".to_string();
        let cache_key = CacheKey::ApiKeyHash(&api_key_hash).to_compact_string();
        let cached_key = CacheApiKey {
            api_key_hash: api_key_hash.clone(),
            expires_at: Some(expired_at),
            ..cache_api_key()
        };

        app_state
            .api_key_cache
            .set_positive(&cache_key, &cached_key)
            .await
            .expect("seed expired cache entry");

        let result = app_state
            .get_api_key_by_hash(&api_key_hash)
            .await
            .expect("expired cache hit should not error");
        assert!(result.is_none());

        let cached_after = app_state
            .api_key_cache
            .get_entry(&cache_key)
            .await
            .expect("read cache after eviction");
        assert!(cached_after.is_none());
    }

    #[tokio::test]
    async fn get_or_load_rehydrates_after_cache_clear() {
        let app_state = AppState::new().await;
        let cache_key = CacheKey::ApiKeyHash("rehydrate").to_compact_string();
        let cached_key = cache_api_key();

        app_state
            .api_key_cache
            .set_positive(&cache_key, &cached_key)
            .await
            .expect("seed cache");
        app_state.clear_cache().await;

        let loaded = app_state
            .get_or_load(&app_state.api_key_cache, &cache_key, || async {
                Ok(Some(cached_key.clone()))
            })
            .await
            .expect("reload after clear should succeed")
            .expect("loader should repopulate cache");

        assert_eq!(loaded.id, cached_key.id);

        let cached_after = app_state
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
