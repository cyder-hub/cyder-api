use async_trait::async_trait;
use chrono::{Datelike, TimeZone, Utc};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use crate::service::app_state::AppStoreError;
use crate::service::cache::types::CacheApiKey;

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
pub(crate) struct ApiKeyRuntimeState {
    pub(crate) current_concurrency: u32,
    pub(crate) active_request_leases: HashMap<String, ApiKeyActiveRequestLease>,
    pub(crate) current_minute_bucket: Option<i64>,
    pub(crate) current_minute_request_count: u32,
    pub(crate) day_bucket: Option<i64>,
    pub(crate) daily_request_count: i64,
    pub(crate) daily_token_count: i64,
    pub(crate) daily_billed_amounts: HashMap<String, i64>,
    pub(crate) month_bucket: Option<i64>,
    pub(crate) monthly_token_count: i64,
    pub(crate) monthly_billed_amounts: HashMap<String, i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct ApiKeyActiveRequestLease {
    pub(crate) expires_at_ms: i64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ApiKeyRollupBaseline {
    pub(crate) day_bucket: i64,
    pub(crate) daily_request_count: i64,
    pub(crate) daily_token_count: i64,
    pub(crate) daily_billed_amounts: HashMap<String, i64>,
    pub(crate) month_bucket: i64,
    pub(crate) monthly_token_count: i64,
    pub(crate) monthly_billed_amounts: HashMap<String, i64>,
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

    pub(crate) fn snapshot(&self, api_key_id: i64) -> ApiKeyGovernanceSnapshot {
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

    pub(crate) fn is_active(&self) -> bool {
        self.current_concurrency > 0
            || self.current_minute_request_count > 0
            || self.daily_request_count > 0
            || self.daily_token_count > 0
            || self.monthly_token_count > 0
            || self.daily_billed_amounts.values().any(|value| *value > 0)
            || self.monthly_billed_amounts.values().any(|value| *value > 0)
    }

    pub(crate) fn apply_rollup_baseline(&mut self, baseline: &ApiKeyRollupBaseline) {
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

    fn check_admission_limits(
        &mut self,
        api_key: &CacheApiKey,
        now_ms: i64,
    ) -> Result<(), ApiKeyGovernanceAdmissionError> {
        let minute_bucket = minute_bucket_start(now_ms);
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
            let normalized_currency = normalize_currency_code(currency);
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
            let normalized_currency = normalize_currency_code(currency);
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

    fn sync_current_concurrency_from_leases(&mut self) {
        self.current_concurrency =
            u32::try_from(self.active_request_leases.len()).unwrap_or(u32::MAX);
    }

    pub(crate) fn prune_expired_request_leases(&mut self, now_ms: i64) {
        let previous_len = self.active_request_leases.len();
        self.active_request_leases
            .retain(|_, lease| lease.expires_at_ms > now_ms);
        if self.active_request_leases.len() != previous_len {
            self.sync_current_concurrency_from_leases();
        }
    }

    pub(crate) fn release_request_lease(&mut self, lease_id: &str) -> bool {
        let removed = self.active_request_leases.remove(lease_id).is_some();
        if removed {
            self.sync_current_concurrency_from_leases();
        }
        removed
    }

    pub(crate) fn try_begin_request(
        &mut self,
        api_key: &CacheApiKey,
        now_ms: i64,
        baseline: &ApiKeyRollupBaseline,
        lease_now_ms: i64,
        request_lease_ttl: Duration,
    ) -> Result<Option<ApiKeyRequestLease>, ApiKeyGovernanceAdmissionError> {
        self.apply_rollup_baseline(baseline);
        self.prune_expired_request_leases(lease_now_ms);
        self.check_admission_limits(api_key, now_ms)?;

        let request_lease = match api_key.max_concurrent_requests {
            Some(limit) => {
                let limit = u32::try_from(limit).unwrap_or(0);
                if self.current_concurrency >= limit {
                    return Err(ApiKeyGovernanceAdmissionError::ConcurrencyLimited {
                        limit: api_key.max_concurrent_requests.unwrap_or_default(),
                        current: self.current_concurrency,
                    });
                }
                let lease_id = Uuid::new_v4().to_string();
                let ttl_ms = i64::try_from(request_lease_ttl.as_millis()).unwrap_or(i64::MAX);
                self.active_request_leases.insert(
                    lease_id.clone(),
                    ApiKeyActiveRequestLease {
                        expires_at_ms: lease_now_ms.saturating_add(ttl_ms),
                    },
                );
                self.sync_current_concurrency_from_leases();
                Some(ApiKeyRequestLease::new(api_key.id, lease_id))
            }
            None => None,
        };

        self.record_request_admission();
        Ok(request_lease)
    }

    pub(crate) fn apply_completion(&mut self, delta: &ApiKeyCompletionDelta) {
        self.daily_token_count = self.daily_token_count.saturating_add(delta.total_tokens);
        self.monthly_token_count = self.monthly_token_count.saturating_add(delta.total_tokens);

        if let Some(currency) = delta.billed_currency.as_deref() {
            let normalized_currency = normalize_currency_code(currency);
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiKeyRequestLease {
    api_key_id: i64,
    lease_id: String,
}

impl ApiKeyRequestLease {
    pub(crate) fn new(api_key_id: i64, lease_id: String) -> Self {
        Self {
            api_key_id,
            lease_id,
        }
    }

    pub fn api_key_id(&self) -> i64 {
        self.api_key_id
    }

    pub fn lease_id(&self) -> &str {
        &self.lease_id
    }
}

#[async_trait]
pub(crate) trait ApiKeyRuntimeStore: Send + Sync {
    async fn snapshot(&self, api_key_id: i64) -> Result<ApiKeyGovernanceSnapshot, AppStoreError>;
    async fn snapshots(&self) -> Result<Vec<ApiKeyGovernanceSnapshot>, AppStoreError>;
    async fn try_begin_request(
        &self,
        api_key: &CacheApiKey,
        now_ms: i64,
        baseline: &ApiKeyRollupBaseline,
    ) -> Result<Option<ApiKeyRequestLease>, ApiKeyGovernanceAdmissionError>;
    async fn release_request_lease(&self, lease: &ApiKeyRequestLease) -> Result<(), AppStoreError>;
    async fn apply_completion(
        &self,
        delta: &ApiKeyCompletionDelta,
        baseline: &ApiKeyRollupBaseline,
    ) -> Result<(), AppStoreError>;
}

pub(crate) fn minute_bucket_start(timestamp_ms: i64) -> i64 {
    timestamp_ms.div_euclid(60_000) * 60_000
}

pub(crate) fn day_bucket_start(timestamp_ms: i64) -> i64 {
    timestamp_ms.div_euclid(86_400_000) * 86_400_000
}

pub(crate) fn month_bucket_start(timestamp_ms: i64) -> i64 {
    let timestamp = Utc
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .unwrap_or_else(Utc::now);
    Utc.with_ymd_and_hms(timestamp.year(), timestamp.month(), 1, 0, 0, 0)
        .single()
        .expect("month bucket should be valid")
        .timestamp_millis()
}

pub(crate) fn normalize_currency_code(currency: &str) -> String {
    currency.trim().to_ascii_uppercase()
}
