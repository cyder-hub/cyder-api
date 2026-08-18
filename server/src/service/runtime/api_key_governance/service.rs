use std::sync::Arc;

use chrono::Utc;

use crate::database::api_key_rollup::{ApiKeyRollupDaily, ApiKeyRollupMonthly};
use crate::database::runtime::DatabaseRuntime;
use crate::service::app_state::AppStoreError;
use crate::service::cache::types::CacheApiKey;

use super::memory_store::MemoryApiKeyRuntimeStore;
use super::types::{
    ApiKeyCompletionDelta, ApiKeyGovernanceAdmissionError, ApiKeyGovernanceSnapshot,
    ApiKeyRequestLease, ApiKeyRollupBaseline, ApiKeyRuntimeStore, day_bucket_start,
    month_bucket_start, normalize_currency_code,
};

pub(crate) trait ApiKeyGovernanceClock: Send + Sync {
    fn now_ms(&self) -> i64;
}

#[derive(Debug, Default)]
pub(crate) struct UtcApiKeyGovernanceClock;

impl ApiKeyGovernanceClock for UtcApiKeyGovernanceClock {
    fn now_ms(&self) -> i64 {
        Utc::now().timestamp_millis()
    }
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct FixedApiKeyGovernanceClock {
    now_ms: i64,
}

#[cfg(test)]
impl FixedApiKeyGovernanceClock {
    pub(crate) const fn new(now_ms: i64) -> Self {
        Self { now_ms }
    }
}

#[cfg(test)]
impl ApiKeyGovernanceClock for FixedApiKeyGovernanceClock {
    fn now_ms(&self) -> i64 {
        self.now_ms
    }
}

pub struct ApiKeyGovernanceService {
    database: Arc<DatabaseRuntime>,
    store: Arc<dyn ApiKeyRuntimeStore>,
    clock: Arc<dyn ApiKeyGovernanceClock>,
}

impl ApiKeyGovernanceService {
    pub(crate) fn new(database: Arc<DatabaseRuntime>, store: Arc<dyn ApiKeyRuntimeStore>) -> Self {
        Self::new_with_clock(database, store, Arc::new(UtcApiKeyGovernanceClock))
    }

    pub(crate) fn new_with_clock(
        database: Arc<DatabaseRuntime>,
        store: Arc<dyn ApiKeyRuntimeStore>,
        clock: Arc<dyn ApiKeyGovernanceClock>,
    ) -> Self {
        Self {
            database,
            store,
            clock,
        }
    }

    pub fn new_memory(database: Arc<DatabaseRuntime>) -> Self {
        Self::new(database, Arc::new(MemoryApiKeyRuntimeStore::default()))
    }

    #[cfg(test)]
    pub(crate) fn database(&self) -> &DatabaseRuntime {
        &self.database
    }

    async fn load_api_key_rollup_baseline(
        &self,
        api_key_id: i64,
        timestamp_ms: i64,
    ) -> Result<ApiKeyRollupBaseline, AppStoreError> {
        let day_bucket = day_bucket_start(timestamp_ms);
        let month_bucket = month_bucket_start(timestamp_ms);
        let daily_rows = ApiKeyRollupDaily::list_by_bucket(&self.database, api_key_id, day_bucket)
            .await
            .map_err(|err| {
                AppStoreError::DatabaseError(format!(
                    "failed to load api key daily rollup baseline for {}: {:?}",
                    api_key_id, err
                ))
            })?;
        let monthly_rows =
            ApiKeyRollupMonthly::list_by_bucket(&self.database, api_key_id, month_bucket)
                .await
                .map_err(|err| {
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
            let currency = normalize_currency_code(&row.currency);
            let amount = baseline.daily_billed_amounts.entry(currency).or_default();
            *amount = amount.saturating_add(row.billed_amount_nanos);
        }

        for row in monthly_rows {
            baseline.monthly_token_count = baseline
                .monthly_token_count
                .saturating_add(row.total_tokens);
            let currency = normalize_currency_code(&row.currency);
            let amount = baseline.monthly_billed_amounts.entry(currency).or_default();
            *amount = amount.saturating_add(row.billed_amount_nanos);
        }

        Ok(baseline)
    }

    async fn rollup_baseline_for_store(
        &self,
        api_key_id: i64,
        timestamp_ms: i64,
    ) -> Result<ApiKeyRollupBaseline, AppStoreError> {
        let day_bucket = day_bucket_start(timestamp_ms);
        let month_bucket = month_bucket_start(timestamp_ms);
        let snapshot = self.store.snapshot(api_key_id).await?;

        if snapshot.day_bucket == Some(day_bucket) && snapshot.month_bucket == Some(month_bucket) {
            return Ok(ApiKeyRollupBaseline {
                day_bucket,
                month_bucket,
                ..ApiKeyRollupBaseline::default()
            });
        }

        self.load_api_key_rollup_baseline(api_key_id, timestamp_ms)
            .await
    }

    pub async fn get_api_key_governance_snapshot(
        &self,
        api_key_id: i64,
    ) -> Result<ApiKeyGovernanceSnapshot, AppStoreError> {
        self.store.snapshot(api_key_id).await
    }

    pub async fn list_api_key_governance_snapshots(
        &self,
    ) -> Result<Vec<ApiKeyGovernanceSnapshot>, AppStoreError> {
        self.store.snapshots().await
    }

    pub async fn try_admit_api_key_governance(
        &self,
        api_key: &CacheApiKey,
    ) -> Result<(), ApiKeyGovernanceAdmissionError> {
        let now_ms = self.clock.now_ms();
        let baseline = self
            .rollup_baseline_for_store(api_key.id, now_ms)
            .await
            .map_err(|err| ApiKeyGovernanceAdmissionError::Internal(err.to_string()))?;

        let mut api_key_without_concurrency = api_key.clone();
        api_key_without_concurrency.max_concurrent_requests = None;
        let _ = self
            .store
            .try_begin_request(&api_key_without_concurrency, now_ms, &baseline)
            .await?;
        Ok(())
    }

    pub async fn try_begin_api_key_request(
        &self,
        api_key: &CacheApiKey,
    ) -> Result<Option<ApiKeyRequestLease>, ApiKeyGovernanceAdmissionError> {
        let now_ms = self.clock.now_ms();
        let baseline = self
            .rollup_baseline_for_store(api_key.id, now_ms)
            .await
            .map_err(|err| ApiKeyGovernanceAdmissionError::Internal(err.to_string()))?;
        self.store
            .try_begin_request(api_key, now_ms, &baseline)
            .await
    }

    pub async fn release_api_key_request_lease(
        &self,
        lease: ApiKeyRequestLease,
    ) -> Result<(), AppStoreError> {
        self.store.release_request_lease(&lease).await
    }

    pub async fn record_api_key_completion(
        &self,
        delta: &ApiKeyCompletionDelta,
    ) -> Result<(), AppStoreError> {
        let baseline = self
            .rollup_baseline_for_store(delta.api_key_id, delta.occurred_at)
            .await?;
        self.store.apply_completion(delta, &baseline).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        memory_store::MemoryApiKeyRuntimeStore,
        types::{
            ApiKeyCompletionDelta, ApiKeyGovernanceAdmissionError, day_bucket_start,
            month_bucket_start,
        },
    };
    use super::{ApiKeyGovernanceService, FixedApiKeyGovernanceClock};
    use crate::database::TestDatabase;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload};
    use crate::database::api_key_rollup::{NewApiKeyRollupDaily, NewApiKeyRollupMonthly};
    use crate::schema::enum_def::Action;
    use crate::service::app_state::AppStoreError;
    use crate::service::cache::types::CacheApiKey;
    use std::{sync::Arc, time::Duration};

    fn cache_api_key(id: i64) -> CacheApiKey {
        CacheApiKey {
            id,
            api_key_hash: "hash".to_string(),
            key_prefix: "cyder-prefix".to_string(),
            key_last4: "1234".to_string(),
            name: "runtime".to_string(),
            description: None,
            default_action: Action::Allow,
            is_enabled: true,
            expires_at: None,
            rate_limit_rpm: Some(5),
            max_concurrent_requests: Some(1),
            quota_daily_requests: Some(20),
            quota_daily_tokens: Some(200),
            quota_monthly_tokens: Some(500),
            budget_daily_nanos: Some(100),
            budget_daily_currency: Some("usd".to_string()),
            budget_monthly_nanos: Some(200),
            budget_monthly_currency: Some("usd".to_string()),
            acl_rules: vec![],
        }
    }

    #[tokio::test]
    async fn try_admit_api_key_governance_does_not_hold_concurrency_slots() {
        let test_db_context =
            TestDatabase::new_sqlite_default("api-key-governance-service-admit.sqlite").await;
        let service = ApiKeyGovernanceService::new_memory(test_db_context.runtime());

        (async {
            let api_key = cache_api_key(42);

            service
                .try_admit_api_key_governance(&api_key)
                .await
                .expect("admission should succeed without consuming concurrency");

            let snapshot = service
                .get_api_key_governance_snapshot(api_key.id)
                .await
                .expect("snapshot should load");
            assert_eq!(snapshot.current_concurrency, 0);
            assert_eq!(snapshot.current_minute_request_count, 1);
            assert_eq!(snapshot.daily_request_count, 1);
        })
        .await;
    }

    #[tokio::test]
    async fn fixed_clock_produces_exact_admission_reset_fact() {
        const NOW_MS: i64 = 1_785_760_499_999;
        let test_db_context =
            TestDatabase::new_sqlite_default("api-key-governance-fixed-clock.sqlite").await;
        let service = ApiKeyGovernanceService::new_with_clock(
            test_db_context.runtime(),
            Arc::new(MemoryApiKeyRuntimeStore::default()),
            Arc::new(FixedApiKeyGovernanceClock::new(NOW_MS)),
        );

        (async {
            let api_key = CacheApiKey {
                rate_limit_rpm: Some(1),
                max_concurrent_requests: None,
                quota_daily_requests: None,
                quota_daily_tokens: None,
                quota_monthly_tokens: None,
                budget_daily_nanos: None,
                budget_daily_currency: None,
                budget_monthly_nanos: None,
                budget_monthly_currency: None,
                ..cache_api_key(43)
            };

            service
                .try_begin_api_key_request(&api_key)
                .await
                .expect("first request should be admitted");
            let error = service
                .try_begin_api_key_request(&api_key)
                .await
                .expect_err("second request should be rate limited");
            assert_eq!(
                error,
                ApiKeyGovernanceAdmissionError::RateLimited {
                    limit: 1,
                    current: 1,
                    retry_after: Duration::from_millis(1),
                }
            );
        })
        .await;
    }

    #[tokio::test]
    async fn service_loads_rollup_baseline_without_app_state() {
        let test_db_context =
            TestDatabase::new_sqlite_default("api-key-governance-service-baseline.sqlite").await;
        let service = ApiKeyGovernanceService::new_memory(test_db_context.runtime());

        (async {
            let created = ApiKey::create(
                &test_db_context,
                &CreateApiKeyPayload {
                    name: "runtime-baseline".to_string(),
                    description: None,
                    default_action: Some(Action::Allow),
                    is_enabled: Some(true),
                    expires_at: None,
                    rate_limit_rpm: None,
                    max_concurrent_requests: None,
                    quota_daily_requests: None,
                    quota_daily_tokens: None,
                    quota_monthly_tokens: None,
                    budget_daily_nanos: None,
                    budget_daily_currency: None,
                    budget_monthly_nanos: None,
                    budget_monthly_currency: None,
                    acl_rules: None,
                },
            )
            .await
            .expect("api key should be created for rollup baseline foreign key");
            let api_key = cache_api_key(created.detail.id);
            let now_ms = 1_744_000_000_000;
            let day_bucket = day_bucket_start(now_ms);
            let month_bucket = month_bucket_start(now_ms);

            crate::database::api_key_rollup::ApiKeyRollupDaily::upsert(
                &test_db_context,
                &NewApiKeyRollupDaily {
                    api_key_id: api_key.id,
                    day_bucket,
                    currency: "usd".to_string(),
                    request_count: 5,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                    total_reasoning_tokens: 0,
                    total_tokens: 50,
                    billed_amount_nanos: 7,
                    last_request_at: Some(now_ms),
                    created_at: now_ms,
                    updated_at: now_ms,
                },
            )
            .await
            .expect("daily rollup should insert");
            crate::database::api_key_rollup::ApiKeyRollupMonthly::upsert(
                &test_db_context,
                &NewApiKeyRollupMonthly {
                    api_key_id: api_key.id,
                    month_bucket,
                    currency: "usd".to_string(),
                    request_count: 5,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                    total_reasoning_tokens: 0,
                    total_tokens: 80,
                    billed_amount_nanos: 11,
                    last_request_at: Some(now_ms),
                    created_at: now_ms,
                    updated_at: now_ms,
                },
            )
            .await
            .expect("monthly rollup should insert");

            service
                .record_api_key_completion(&ApiKeyCompletionDelta {
                    api_key_id: api_key.id,
                    occurred_at: now_ms,
                    total_tokens: 3,
                    billed_amount_nanos: 2,
                    billed_currency: Some("usd".to_string()),
                })
                .await
                .expect("completion should load baseline and record usage");

            let snapshot = service
                .get_api_key_governance_snapshot(api_key.id)
                .await
                .expect("snapshot should load");
            assert_eq!(snapshot.day_bucket, Some(day_bucket));
            assert_eq!(snapshot.daily_request_count, 5);
            assert_eq!(snapshot.daily_token_count, 53);
            assert_eq!(snapshot.month_bucket, Some(month_bucket));
            assert_eq!(snapshot.monthly_token_count, 83);
            assert_eq!(
                snapshot
                    .daily_billed_amounts
                    .first()
                    .map(|item| item.amount_nanos),
                Some(9)
            );
            assert_eq!(
                snapshot
                    .monthly_billed_amounts
                    .first()
                    .map(|item| item.amount_nanos),
                Some(13)
            );
        })
        .await;
    }

    #[tokio::test]
    async fn warm_bucket_continues_but_cross_bucket_baseline_failure_is_closed() {
        let test_db_context =
            TestDatabase::new_sqlite_default("api-key-governance-cross-bucket-failure.sqlite")
                .await;
        let service = ApiKeyGovernanceService::new_memory(test_db_context.runtime());
        const FIRST_BUCKET_MS: i64 = 1_744_000_000_000;

        (async {
            let first = ApiKeyCompletionDelta {
                api_key_id: 51,
                occurred_at: FIRST_BUCKET_MS,
                total_tokens: 1,
                billed_amount_nanos: 0,
                billed_currency: None,
            };
            service
                .record_api_key_completion(&first)
                .await
                .expect("first completion should initialize the bucket baseline");

            test_db_context
                .execute_sqlite_batch(
                    "ALTER TABLE api_key_rollup_daily RENAME TO api_key_rollup_daily_unavailable",
                )
                .await
                .expect("daily rollup table should become unavailable");

            service
                .record_api_key_completion(&ApiKeyCompletionDelta {
                    total_tokens: 2,
                    ..first.clone()
                })
                .await
                .expect("warm current bucket should not require database baseline reload");

            let error = service
                .record_api_key_completion(&ApiKeyCompletionDelta {
                    occurred_at: FIRST_BUCKET_MS + 86_400_000,
                    ..first
                })
                .await
                .expect_err("new bucket must fail closed when baseline cannot load");
            assert!(matches!(error, AppStoreError::DatabaseError(_)));

            test_db_context
                .execute_sqlite_batch(
                    "ALTER TABLE api_key_rollup_daily_unavailable RENAME TO api_key_rollup_daily",
                )
                .await
                .expect("daily rollup table should recover");
        })
        .await;
    }
}
