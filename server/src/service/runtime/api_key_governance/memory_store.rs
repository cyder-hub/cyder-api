use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::service::app_state::AppStoreError;
use crate::service::cache::types::CacheApiKey;

use super::types::{
    ApiKeyCompletionDelta, ApiKeyConcurrencyGuard, ApiKeyGovernanceAdmissionError,
    ApiKeyGovernanceSnapshot, ApiKeyRollupBaseline, ApiKeyRuntimeState, ApiKeyRuntimeStore,
};

#[derive(Clone, Default)]
pub struct MemoryApiKeyRuntimeStore {
    inner: Arc<Mutex<HashMap<i64, ApiKeyRuntimeState>>>,
}

impl ApiKeyRuntimeStore for MemoryApiKeyRuntimeStore {
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
        let concurrency_guard =
            state.try_begin_request(api_key, now_ms, Arc::clone(&self.inner))?;
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

        Ok(Some(ApiKeyConcurrencyGuard::new(
            api_key_id,
            Arc::clone(&self.inner),
        )))
    }

    fn apply_rollup_baseline(
        &self,
        api_key_id: i64,
        baseline: &ApiKeyRollupBaseline,
    ) -> Result<(), AppStoreError> {
        let mut guard = self.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        let state = guard.entry(api_key_id).or_default();
        state.apply_rollup_baseline(baseline);
        Ok(())
    }

    fn apply_completion(&self, delta: &ApiKeyCompletionDelta) -> Result<(), AppStoreError> {
        let mut guard = self.inner.lock().map_err(|e| {
            AppStoreError::LockError(format!("api key governance lock poisoned: {e}"))
        })?;
        let state = guard.entry(delta.api_key_id).or_default();
        state.apply_completion(delta);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        ApiKeyBilledAmountSnapshot, ApiKeyCompletionDelta, ApiKeyGovernanceAdmissionError,
        ApiKeyGovernanceSnapshot, ApiKeyRollupBaseline, ApiKeyRuntimeState, ApiKeyRuntimeStore,
        day_bucket_start, minute_bucket_start, month_bucket_start,
    };
    use super::MemoryApiKeyRuntimeStore;
    use crate::schema::enum_def::Action;
    use crate::service::cache::types::CacheApiKey;
    use std::collections::HashMap;

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

    #[test]
    fn api_key_concurrency_guard_releases_slots_on_drop() {
        let store = MemoryApiKeyRuntimeStore::default();

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
        let store = MemoryApiKeyRuntimeStore::default();
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
        let store = MemoryApiKeyRuntimeStore::default();
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

        let result = state.try_begin_request(&api_key, now_ms, Default::default());
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::RateLimited { .. })
        ));

        state.current_minute_request_count = 0;
        let result = state.try_begin_request(&api_key, now_ms, Default::default());
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::DailyRequestQuotaExceeded { .. })
        ));

        state.daily_request_count = 0;
        let result = state.try_begin_request(&api_key, now_ms, Default::default());
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::DailyTokenQuotaExceeded { .. })
        ));

        state.daily_token_count = 0;
        let result = state.try_begin_request(&api_key, now_ms, Default::default());
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::MonthlyTokenQuotaExceeded { .. })
        ));

        state.monthly_token_count = 0;
        let result = state.try_begin_request(&api_key, now_ms, Default::default());
        assert!(matches!(
            result,
            Err(ApiKeyGovernanceAdmissionError::DailyBudgetExceeded { .. })
        ));

        state.daily_billed_amounts.clear();
        let result = state.try_begin_request(&api_key, now_ms, Default::default());
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

        state
            .try_begin_request(&api_key, now_ms, Default::default())
            .expect("non-matching currencies should not exhaust USD budgets");

        assert_eq!(state.daily_request_count, 1);
        assert_eq!(state.current_minute_request_count, 1);
    }
}
