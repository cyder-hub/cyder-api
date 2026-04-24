use std::sync::Arc;

use chrono::Utc;

use crate::database::api_key_rollup::{ApiKeyRollupDaily, ApiKeyRollupMonthly};
use crate::service::app_state::AppStoreError;
use crate::service::cache::types::CacheApiKey;

use super::memory_store::MemoryApiKeyRuntimeStore;
use super::types::{
    ApiKeyCompletionDelta, ApiKeyConcurrencyGuard, ApiKeyGovernanceAdmissionError,
    ApiKeyGovernanceSnapshot, ApiKeyRollupBaseline, ApiKeyRuntimeStore, day_bucket_start,
    month_bucket_start, normalize_currency_code,
};

pub struct ApiKeyGovernanceService {
    store: Arc<dyn ApiKeyRuntimeStore>,
}

impl ApiKeyGovernanceService {
    pub(crate) fn new(store: Arc<dyn ApiKeyRuntimeStore>) -> Self {
        Self { store }
    }

    pub fn new_memory() -> Self {
        Self::new(Arc::new(MemoryApiKeyRuntimeStore::default()))
    }

    async fn load_api_key_rollup_baseline(
        &self,
        api_key_id: i64,
        timestamp_ms: i64,
    ) -> Result<ApiKeyRollupBaseline, AppStoreError> {
        let day_bucket = day_bucket_start(timestamp_ms);
        let month_bucket = month_bucket_start(timestamp_ms);
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

    async fn ensure_api_key_governance_usage_state(
        &self,
        api_key_id: i64,
        timestamp_ms: i64,
    ) -> Result<(), AppStoreError> {
        let day_bucket = day_bucket_start(timestamp_ms);
        let month_bucket = month_bucket_start(timestamp_ms);
        let snapshot = self.store.snapshot(api_key_id)?;
        let needs_reload =
            snapshot.day_bucket != Some(day_bucket) || snapshot.month_bucket != Some(month_bucket);

        if !needs_reload {
            return Ok(());
        }

        let baseline = self
            .load_api_key_rollup_baseline(api_key_id, timestamp_ms)
            .await?;
        self.store.apply_rollup_baseline(api_key_id, &baseline)
    }

    pub fn try_acquire_api_key_concurrency(
        &self,
        api_key_id: i64,
        max_concurrent_requests: Option<i32>,
    ) -> Result<Option<ApiKeyConcurrencyGuard>, AppStoreError> {
        self.store
            .try_acquire_concurrency(api_key_id, max_concurrent_requests)
    }

    pub fn get_api_key_governance_snapshot(
        &self,
        api_key_id: i64,
    ) -> Result<ApiKeyGovernanceSnapshot, AppStoreError> {
        self.store.snapshot(api_key_id)
    }

    pub fn list_api_key_governance_snapshots(
        &self,
    ) -> Result<Vec<ApiKeyGovernanceSnapshot>, AppStoreError> {
        self.store.snapshots()
    }

    pub async fn try_admit_api_key_governance(
        &self,
        api_key: &CacheApiKey,
    ) -> Result<(), ApiKeyGovernanceAdmissionError> {
        let now_ms = Utc::now().timestamp_millis();
        self.ensure_api_key_governance_usage_state(api_key.id, now_ms)
            .await
            .map_err(|err| ApiKeyGovernanceAdmissionError::Internal(err.to_string()))?;

        let mut api_key_without_concurrency = api_key.clone();
        api_key_without_concurrency.max_concurrent_requests = None;
        let _ = self
            .store
            .try_begin_request(&api_key_without_concurrency, now_ms)?;
        Ok(())
    }

    pub async fn try_begin_api_key_request(
        &self,
        api_key: &CacheApiKey,
    ) -> Result<Option<ApiKeyConcurrencyGuard>, ApiKeyGovernanceAdmissionError> {
        let now_ms = Utc::now().timestamp_millis();
        self.ensure_api_key_governance_usage_state(api_key.id, now_ms)
            .await
            .map_err(|err| ApiKeyGovernanceAdmissionError::Internal(err.to_string()))?;
        self.store.try_begin_request(api_key, now_ms)
    }

    pub async fn record_api_key_completion(
        &self,
        delta: &ApiKeyCompletionDelta,
    ) -> Result<(), AppStoreError> {
        self.ensure_api_key_governance_usage_state(delta.api_key_id, delta.occurred_at)
            .await?;
        self.store.apply_completion(delta)
    }
}

impl Default for ApiKeyGovernanceService {
    fn default() -> Self {
        Self::new_memory()
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{ApiKeyCompletionDelta, day_bucket_start, month_bucket_start};
    use super::ApiKeyGovernanceService;
    use crate::database::TestDbContext;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload};
    use crate::database::api_key_rollup::{NewApiKeyRollupDaily, NewApiKeyRollupMonthly};
    use crate::schema::enum_def::Action;
    use crate::service::cache::types::CacheApiKey;

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
        let test_db_context = TestDbContext::new_sqlite("api-key-governance-service-admit.sqlite");
        let service = ApiKeyGovernanceService::default();

        test_db_context
            .run_async(async {
                let api_key = cache_api_key(42);

                service
                    .try_admit_api_key_governance(&api_key)
                    .await
                    .expect("admission should succeed without consuming concurrency");

                let snapshot = service
                    .get_api_key_governance_snapshot(api_key.id)
                    .expect("snapshot should load");
                assert_eq!(snapshot.current_concurrency, 0);
                assert_eq!(snapshot.current_minute_request_count, 1);
                assert_eq!(snapshot.daily_request_count, 1);
            })
            .await;
    }

    #[tokio::test]
    async fn service_loads_rollup_baseline_without_app_state() {
        let test_db_context =
            TestDbContext::new_sqlite("api-key-governance-service-baseline.sqlite");
        let service = ApiKeyGovernanceService::default();

        test_db_context
            .run_async(async {
                let created = ApiKey::create(&CreateApiKeyPayload {
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
                })
                .expect("api key should be created for rollup baseline foreign key");
                let api_key = cache_api_key(created.detail.id);
                let now_ms = 1_744_000_000_000;
                let day_bucket = day_bucket_start(now_ms);
                let month_bucket = month_bucket_start(now_ms);

                crate::database::api_key_rollup::ApiKeyRollupDaily::upsert(&NewApiKeyRollupDaily {
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
                })
                .expect("daily rollup should insert");
                crate::database::api_key_rollup::ApiKeyRollupMonthly::upsert(
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
}
