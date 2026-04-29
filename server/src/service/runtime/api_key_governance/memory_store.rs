use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::service::app_state::AppStoreError;
use crate::service::cache::types::CacheApiKey;
use chrono::Utc;

use super::types::{
    ApiKeyCompletionDelta, ApiKeyGovernanceAdmissionError, ApiKeyGovernanceSnapshot,
    ApiKeyRequestLease, ApiKeyRollupBaseline, ApiKeyRuntimeState, ApiKeyRuntimeStore,
};

pub const DEFAULT_API_KEY_REQUEST_LEASE_TTL: Duration = Duration::from_secs(900);

/// Single-instance default and dev/test backend; not a multi-instance correctness backend.
#[derive(Clone)]
pub struct MemoryApiKeyRuntimeStore {
    inner: Arc<Mutex<HashMap<i64, ApiKeyRuntimeState>>>,
    request_lease_ttl: Duration,
}

impl MemoryApiKeyRuntimeStore {
    pub fn with_request_lease_ttl(request_lease_ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            request_lease_ttl,
        }
    }

    fn prune_expired_leases_locked(
        guard: &mut HashMap<i64, ApiKeyRuntimeState>,
        api_key_id: i64,
        now_ms: i64,
    ) {
        let remove_entry = match guard.get_mut(&api_key_id) {
            Some(state) => {
                state.prune_expired_request_leases(now_ms);
                !state.is_active()
            }
            None => false,
        };

        if remove_entry {
            guard.remove(&api_key_id);
        }
    }
}

impl Default for MemoryApiKeyRuntimeStore {
    fn default() -> Self {
        Self::with_request_lease_ttl(DEFAULT_API_KEY_REQUEST_LEASE_TTL)
    }
}

#[async_trait]
impl ApiKeyRuntimeStore for MemoryApiKeyRuntimeStore {
    async fn snapshot(&self, api_key_id: i64) -> Result<ApiKeyGovernanceSnapshot, AppStoreError> {
        let mut guard = self.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        Self::prune_expired_leases_locked(&mut guard, api_key_id, Utc::now().timestamp_millis());
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

    async fn snapshots(&self) -> Result<Vec<ApiKeyGovernanceSnapshot>, AppStoreError> {
        let mut guard = self.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        let now_ms = Utc::now().timestamp_millis();
        let api_key_ids = guard.keys().copied().collect::<Vec<_>>();
        for api_key_id in api_key_ids {
            Self::prune_expired_leases_locked(&mut guard, api_key_id, now_ms);
        }
        let mut snapshots = guard
            .iter()
            .filter(|(_, state)| state.is_active())
            .map(|(api_key_id, state)| state.snapshot(*api_key_id))
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|snapshot| snapshot.api_key_id);
        Ok(snapshots)
    }

    async fn try_begin_request(
        &self,
        api_key: &CacheApiKey,
        now_ms: i64,
        baseline: &ApiKeyRollupBaseline,
    ) -> Result<Option<ApiKeyRequestLease>, ApiKeyGovernanceAdmissionError> {
        let mut guard = self.inner.lock().map_err(|e| {
            ApiKeyGovernanceAdmissionError::Internal(format!(
                "api key governance lock poisoned: {e}"
            ))
        })?;
        let state = guard.entry(api_key.id).or_default();
        state.try_begin_request(
            api_key,
            now_ms,
            baseline,
            Utc::now().timestamp_millis(),
            self.request_lease_ttl,
        )
    }

    async fn release_request_lease(&self, lease: &ApiKeyRequestLease) -> Result<(), AppStoreError> {
        let mut guard = self.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        let remove_entry = match guard.get_mut(&lease.api_key_id()) {
            Some(state) => {
                state.release_request_lease(lease.lease_id());
                !state.is_active()
            }
            None => false,
        };

        if remove_entry {
            guard.remove(&lease.api_key_id());
        }

        Ok(())
    }

    async fn apply_completion(
        &self,
        delta: &ApiKeyCompletionDelta,
        baseline: &ApiKeyRollupBaseline,
    ) -> Result<(), AppStoreError> {
        let mut guard = self.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        let state = guard.entry(delta.api_key_id).or_default();
        state.apply_rollup_baseline(baseline);
        state.apply_completion(delta);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        ApiKeyBilledAmountSnapshot, ApiKeyCompletionDelta, ApiKeyGovernanceAdmissionError,
        ApiKeyRollupBaseline, ApiKeyRuntimeState, ApiKeyRuntimeStore, day_bucket_start,
        minute_bucket_start, month_bucket_start,
    };
    use super::MemoryApiKeyRuntimeStore;
    use crate::schema::enum_def::Action;
    use crate::service::cache::types::CacheApiKey;
    use std::collections::HashMap;
    use std::time::Duration;

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

    #[tokio::test]
    async fn api_key_request_lease_release_frees_slots() {
        let store = MemoryApiKeyRuntimeStore::default();
        let api_key = CacheApiKey {
            rate_limit_rpm: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            ..cache_api_key()
        };
        let now_ms = 1_744_000_000_000;
        let baseline = ApiKeyRollupBaseline {
            day_bucket: day_bucket_start(now_ms),
            month_bucket: month_bucket_start(now_ms),
            ..ApiKeyRollupBaseline::default()
        };

        let first_lease = store
            .try_begin_request(&api_key, now_ms, &baseline)
            .await
            .expect("begin first request")
            .expect("limit should create lease");
        let second_lease = store
            .try_begin_request(&api_key, now_ms, &baseline)
            .await
            .expect("begin second request")
            .expect("limit should create lease");

        let snapshot = store.snapshot(42).await.expect("snapshot");
        assert_eq!(snapshot.current_concurrency, 2);

        let err = store
            .try_begin_request(&api_key, now_ms, &baseline)
            .await
            .expect_err("third request should be concurrency limited");
        assert_eq!(
            err,
            ApiKeyGovernanceAdmissionError::ConcurrencyLimited {
                limit: 2,
                current: 2,
            }
        );

        store
            .release_request_lease(&first_lease)
            .await
            .expect("release first lease");
        assert_eq!(
            store
                .snapshot(42)
                .await
                .expect("snapshot after release")
                .current_concurrency,
            1
        );

        store
            .release_request_lease(&second_lease)
            .await
            .expect("release second lease");
        assert_eq!(
            store
                .snapshot(42)
                .await
                .expect("final snapshot")
                .current_concurrency,
            0
        );
    }

    #[tokio::test]
    async fn api_key_governance_snapshots_are_sorted_and_release_updates_concurrency() {
        let store = MemoryApiKeyRuntimeStore::default();
        let now_ms = 1_744_000_000_000;
        let baseline = ApiKeyRollupBaseline {
            day_bucket: day_bucket_start(now_ms),
            month_bucket: month_bucket_start(now_ms),
            ..ApiKeyRollupBaseline::default()
        };
        let api_key_b = CacheApiKey {
            id: 9,
            rate_limit_rpm: None,
            max_concurrent_requests: Some(1),
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            ..cache_api_key()
        };
        let api_key_a = CacheApiKey {
            id: 3,
            rate_limit_rpm: None,
            max_concurrent_requests: Some(1),
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            ..cache_api_key()
        };
        let _lease_b = store
            .try_begin_request(&api_key_b, now_ms, &baseline)
            .await
            .expect("begin tracked request")
            .expect("lease");
        let lease_a = store
            .try_begin_request(&api_key_a, now_ms, &baseline)
            .await
            .expect("begin tracked request")
            .expect("lease");

        let snapshots = store.snapshots().await.expect("snapshots");
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].api_key_id, 3);
        assert_eq!(snapshots[0].current_concurrency, 1);
        assert_eq!(snapshots[1].api_key_id, 9);
        assert_eq!(snapshots[1].current_concurrency, 1);

        store
            .release_request_lease(&lease_a)
            .await
            .expect("release tracked request");
        let snapshots = store.snapshots().await.expect("snapshots after release");
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].api_key_id, 3);
        assert_eq!(snapshots[0].current_concurrency, 0);
        assert_eq!(snapshots[0].daily_request_count, 1);
        assert_eq!(snapshots[1].api_key_id, 9);
        assert_eq!(snapshots[1].current_concurrency, 1);
    }

    #[tokio::test]
    async fn begin_request_keeps_usage_counters_when_concurrency_guard_drops() {
        let store = MemoryApiKeyRuntimeStore::default();
        let api_key = cache_api_key();
        let now_ms = 1_744_000_000_000;

        let baseline = ApiKeyRollupBaseline {
            day_bucket: day_bucket_start(now_ms),
            month_bucket: month_bucket_start(now_ms),
            ..ApiKeyRollupBaseline::default()
        };

        let lease = store
            .try_begin_request(&api_key, now_ms, &baseline)
            .await
            .expect("begin request")
            .expect("concurrency limit should create lease");

        let snapshot = store
            .snapshot(api_key.id)
            .await
            .expect("snapshot after begin");
        assert_eq!(snapshot.current_concurrency, 1);
        assert_eq!(snapshot.current_minute_request_count, 1);
        assert_eq!(snapshot.daily_request_count, 1);

        store
            .release_request_lease(&lease)
            .await
            .expect("release request lease");

        let snapshot = store
            .snapshot(api_key.id)
            .await
            .expect("snapshot after concurrency release");
        assert_eq!(snapshot.current_concurrency, 0);
        assert_eq!(snapshot.current_minute_request_count, 1);
        assert_eq!(snapshot.daily_request_count, 1);
    }

    #[tokio::test]
    async fn expired_request_leases_are_pruned_from_snapshot() {
        let store = MemoryApiKeyRuntimeStore::with_request_lease_ttl(Duration::from_millis(200));
        let api_key = CacheApiKey {
            rate_limit_rpm: None,
            max_concurrent_requests: Some(1),
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            ..cache_api_key()
        };
        let now_ms = 1_744_000_000_000;
        let baseline = ApiKeyRollupBaseline {
            day_bucket: day_bucket_start(now_ms),
            month_bucket: month_bucket_start(now_ms),
            ..ApiKeyRollupBaseline::default()
        };

        let _lease = store
            .try_begin_request(&api_key, now_ms, &baseline)
            .await
            .expect("begin request")
            .expect("concurrency limit should create lease");

        let snapshot = store
            .snapshot(api_key.id)
            .await
            .expect("snapshot after begin");
        assert_eq!(snapshot.current_concurrency, 1);

        std::thread::sleep(Duration::from_millis(250));

        let snapshot = store
            .snapshot(api_key.id)
            .await
            .expect("snapshot after lease ttl");
        assert_eq!(snapshot.current_concurrency, 0);
    }

    #[test]
    fn api_key_runtime_state_blocks_rate_quota_and_budget_limits() {
        let api_key = cache_api_key();
        let now_ms = 1_744_000_000_000;
        let minute_bucket = minute_bucket_start(now_ms);
        let day_bucket = day_bucket_start(now_ms);
        let month_bucket = month_bucket_start(now_ms);

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
        let baseline = ApiKeyRollupBaseline {
            day_bucket,
            month_bucket,
            ..ApiKeyRollupBaseline::default()
        };

        let result = state.try_begin_request(
            &api_key,
            now_ms,
            &baseline,
            now_ms,
            Duration::from_secs(900),
        );
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::RateLimited { .. })
        ));

        state.current_minute_request_count = 0;
        let result = state.try_begin_request(
            &api_key,
            now_ms,
            &baseline,
            now_ms,
            Duration::from_secs(900),
        );
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::DailyRequestQuotaExceeded { .. })
        ));

        state.daily_request_count = 0;
        let result = state.try_begin_request(
            &api_key,
            now_ms,
            &baseline,
            now_ms,
            Duration::from_secs(900),
        );
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::DailyTokenQuotaExceeded { .. })
        ));

        state.daily_token_count = 0;
        let result = state.try_begin_request(
            &api_key,
            now_ms,
            &baseline,
            now_ms,
            Duration::from_secs(900),
        );
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::MonthlyTokenQuotaExceeded { .. })
        ));

        state.monthly_token_count = 0;
        let result = state.try_begin_request(
            &api_key,
            now_ms,
            &baseline,
            now_ms,
            Duration::from_secs(900),
        );
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::DailyBudgetExceeded { .. })
        ));

        state.daily_billed_amounts.clear();
        let result = state.try_begin_request(
            &api_key,
            now_ms,
            &baseline,
            now_ms,
            Duration::from_secs(900),
        );
        assert!(matches!(
            result,
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
            current_minute_bucket: Some(minute_bucket_start(now_ms)),
            day_bucket: Some(day_bucket_start(now_ms)),
            month_bucket: Some(month_bucket_start(now_ms)),
            daily_billed_amounts: HashMap::from([(String::from("EUR"), 999)]),
            monthly_billed_amounts: HashMap::from([(String::from("JPY"), 999)]),
            ..ApiKeyRuntimeState::default()
        };
        let baseline = ApiKeyRollupBaseline {
            day_bucket: day_bucket_start(now_ms),
            month_bucket: month_bucket_start(now_ms),
            ..ApiKeyRollupBaseline::default()
        };

        state
            .try_begin_request(
                &api_key,
                now_ms,
                &baseline,
                now_ms,
                Duration::from_secs(900),
            )
            .expect("non-matching currencies should not exhaust USD budgets");

        assert_eq!(state.daily_request_count, 1);
        assert_eq!(state.current_minute_request_count, 1);
    }
}
