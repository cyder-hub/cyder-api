use async_trait::async_trait;
use bb8_redis::redis::{Cmd, cmd};
use chrono::Utc;
use std::collections::HashMap;
use std::fmt::Display;
use std::time::Duration;
use uuid::Uuid;

use crate::service::app_state::AppStoreError;
use crate::service::cache::types::CacheApiKey;
use crate::service::redis::RedisPool;

use super::types::{
    ApiKeyBilledAmountSnapshot, ApiKeyCompletionDelta, ApiKeyGovernanceAdmissionError,
    ApiKeyGovernanceSnapshot, ApiKeyRequestLease, ApiKeyRollupBaseline, ApiKeyRuntimeStore,
    minute_bucket_start, normalize_currency_code,
};

const ADMISSION_SCRIPT: &str = r#"
local state_key = KEYS[1]
local leases_key = KEYS[2]
local active_key = KEYS[3]

local api_key_id = ARGV[1]
local now_ms = tonumber(ARGV[2])
local lease_now_ms = tonumber(ARGV[3])
local lease_ttl_ms = tonumber(ARGV[4])
local state_ttl_seconds = tonumber(ARGV[5])
local minute_bucket = ARGV[6]
local day_bucket = ARGV[7]
local baseline_daily_request_count = tonumber(ARGV[8])
local baseline_daily_token_count = tonumber(ARGV[9])
local month_bucket = ARGV[10]
local baseline_monthly_token_count = tonumber(ARGV[11])
local rate_limit_rpm = ARGV[12]
local max_concurrent_requests = ARGV[13]
local quota_daily_requests = ARGV[14]
local quota_daily_tokens = ARGV[15]
local quota_monthly_tokens = ARGV[16]
local daily_budget_currency = ARGV[17]
local daily_budget_limit = ARGV[18]
local monthly_budget_currency = ARGV[19]
local monthly_budget_limit = ARGV[20]
local lease_id = ARGV[21]

local function hnum(field)
    local value = redis.call('HGET', state_key, field)
    if not value then
        return 0
    end
    return tonumber(value) or 0
end

local function clear_prefixed_fields(prefix)
    local fields = redis.call('HKEYS', state_key)
    for _, field in ipairs(fields) do
        if string.sub(field, 1, string.len(prefix)) == prefix then
            redis.call('HDEL', state_key, field)
        end
    end
end

local function apply_budget_baseline(prefix, count, idx)
    for _ = 1, count do
        local currency = ARGV[idx]
        local amount = ARGV[idx + 1]
        redis.call('HSET', state_key, prefix .. currency, amount)
        idx = idx + 2
    end
    return idx
end

redis.call('ZREMRANGEBYSCORE', leases_key, '-inf', lease_now_ms)
local current_concurrency = tonumber(redis.call('ZCARD', leases_key)) or 0

if redis.call('HGET', state_key, 'minute_bucket') ~= minute_bucket then
    redis.call('HSET', state_key, 'minute_bucket', minute_bucket, 'minute_request_count', 0)
end

local idx = 22
local daily_budget_baseline_count = tonumber(ARGV[idx])
idx = idx + 1
if redis.call('HGET', state_key, 'day_bucket') ~= day_bucket then
    clear_prefixed_fields('daily_budget:')
    redis.call(
        'HSET',
        state_key,
        'day_bucket',
        day_bucket,
        'daily_request_count',
        baseline_daily_request_count,
        'daily_token_count',
        baseline_daily_token_count
    )
    idx = apply_budget_baseline('daily_budget:', daily_budget_baseline_count, idx)
else
    idx = idx + (daily_budget_baseline_count * 2)
end

local monthly_budget_baseline_count = tonumber(ARGV[idx])
idx = idx + 1
if redis.call('HGET', state_key, 'month_bucket') ~= month_bucket then
    clear_prefixed_fields('monthly_budget:')
    redis.call(
        'HSET',
        state_key,
        'month_bucket',
        month_bucket,
        'monthly_token_count',
        baseline_monthly_token_count
    )
    idx = apply_budget_baseline('monthly_budget:', monthly_budget_baseline_count, idx)
end

local current_minute_request_count = hnum('minute_request_count')
local current_daily_request_count = hnum('daily_request_count')
local current_daily_token_count = hnum('daily_token_count')
local current_monthly_token_count = hnum('monthly_token_count')

if rate_limit_rpm ~= '' and current_minute_request_count >= tonumber(rate_limit_rpm) then
    return {0, 'rate', tonumber(rate_limit_rpm), current_minute_request_count, ''}
end
if quota_daily_requests ~= '' and current_daily_request_count >= tonumber(quota_daily_requests) then
    return {0, 'daily_request', tonumber(quota_daily_requests), current_daily_request_count, ''}
end
if quota_daily_tokens ~= '' and current_daily_token_count >= tonumber(quota_daily_tokens) then
    return {0, 'daily_token', tonumber(quota_daily_tokens), current_daily_token_count, ''}
end
if quota_monthly_tokens ~= '' and current_monthly_token_count >= tonumber(quota_monthly_tokens) then
    return {0, 'monthly_token', tonumber(quota_monthly_tokens), current_monthly_token_count, ''}
end
if daily_budget_currency ~= '' and daily_budget_limit ~= '' then
    local current_daily_budget = hnum('daily_budget:' .. daily_budget_currency)
    if current_daily_budget >= tonumber(daily_budget_limit) then
        return {0, 'daily_budget', tonumber(daily_budget_limit), current_daily_budget, daily_budget_currency}
    end
end
if monthly_budget_currency ~= '' and monthly_budget_limit ~= '' then
    local current_monthly_budget = hnum('monthly_budget:' .. monthly_budget_currency)
    if current_monthly_budget >= tonumber(monthly_budget_limit) then
        return {0, 'monthly_budget', tonumber(monthly_budget_limit), current_monthly_budget, monthly_budget_currency}
    end
end
if max_concurrent_requests ~= '' and current_concurrency >= tonumber(max_concurrent_requests) then
    return {0, 'concurrency', tonumber(max_concurrent_requests), current_concurrency, ''}
end

local admitted_lease_id = ''
if max_concurrent_requests ~= '' then
    admitted_lease_id = lease_id
    redis.call('ZADD', leases_key, lease_now_ms + lease_ttl_ms, lease_id)
end

redis.call('HINCRBY', state_key, 'minute_request_count', 1)
redis.call('HINCRBY', state_key, 'daily_request_count', 1)
redis.call('HSET', state_key, 'updated_at_ms', lease_now_ms)
redis.call('SADD', active_key, api_key_id)
redis.call('EXPIRE', state_key, state_ttl_seconds)
redis.call('EXPIRE', leases_key, state_ttl_seconds)
redis.call('EXPIRE', active_key, state_ttl_seconds)

return {1, admitted_lease_id, 0, 0, ''}
"#;

const COMPLETION_SCRIPT: &str = r#"
local state_key = KEYS[1]
local active_key = KEYS[2]

local api_key_id = ARGV[1]
local occurred_at = tonumber(ARGV[2])
local state_ttl_seconds = tonumber(ARGV[3])
local day_bucket = ARGV[4]
local baseline_daily_request_count = tonumber(ARGV[5])
local baseline_daily_token_count = tonumber(ARGV[6])
local month_bucket = ARGV[7]
local baseline_monthly_token_count = tonumber(ARGV[8])
local total_tokens = tonumber(ARGV[9])
local billed_currency = ARGV[10]
local billed_amount_nanos = tonumber(ARGV[11])

local function clear_prefixed_fields(prefix)
    local fields = redis.call('HKEYS', state_key)
    for _, field in ipairs(fields) do
        if string.sub(field, 1, string.len(prefix)) == prefix then
            redis.call('HDEL', state_key, field)
        end
    end
end

local function apply_budget_baseline(prefix, count, idx)
    for _ = 1, count do
        local currency = ARGV[idx]
        local amount = ARGV[idx + 1]
        redis.call('HSET', state_key, prefix .. currency, amount)
        idx = idx + 2
    end
    return idx
end

local idx = 12
local daily_budget_baseline_count = tonumber(ARGV[idx])
idx = idx + 1
if redis.call('HGET', state_key, 'day_bucket') ~= day_bucket then
    clear_prefixed_fields('daily_budget:')
    redis.call(
        'HSET',
        state_key,
        'day_bucket',
        day_bucket,
        'daily_request_count',
        baseline_daily_request_count,
        'daily_token_count',
        baseline_daily_token_count
    )
    idx = apply_budget_baseline('daily_budget:', daily_budget_baseline_count, idx)
else
    idx = idx + (daily_budget_baseline_count * 2)
end

local monthly_budget_baseline_count = tonumber(ARGV[idx])
idx = idx + 1
if redis.call('HGET', state_key, 'month_bucket') ~= month_bucket then
    clear_prefixed_fields('monthly_budget:')
    redis.call(
        'HSET',
        state_key,
        'month_bucket',
        month_bucket,
        'monthly_token_count',
        baseline_monthly_token_count
    )
    idx = apply_budget_baseline('monthly_budget:', monthly_budget_baseline_count, idx)
end

redis.call('HINCRBY', state_key, 'daily_token_count', total_tokens)
redis.call('HINCRBY', state_key, 'monthly_token_count', total_tokens)
if billed_currency ~= '' then
    redis.call('HINCRBY', state_key, 'daily_budget:' .. billed_currency, billed_amount_nanos)
    redis.call('HINCRBY', state_key, 'monthly_budget:' .. billed_currency, billed_amount_nanos)
end
redis.call('HSET', state_key, 'updated_at_ms', occurred_at)
redis.call('SADD', active_key, api_key_id)
redis.call('EXPIRE', state_key, state_ttl_seconds)
redis.call('EXPIRE', active_key, state_ttl_seconds)

return 1
"#;

#[derive(Clone)]
pub struct RedisApiKeyRuntimeStore {
    pool: RedisPool,
    key_prefix: String,
    request_lease_ttl: Duration,
    state_ttl: Duration,
}

impl RedisApiKeyRuntimeStore {
    pub fn new(
        pool: RedisPool,
        key_prefix: impl Into<String>,
        request_lease_ttl: Duration,
        state_ttl: Duration,
    ) -> Self {
        Self {
            pool,
            key_prefix: key_prefix.into(),
            request_lease_ttl,
            state_ttl,
        }
    }

    fn state_key(&self, api_key_id: i64) -> String {
        format!("{}api_key:{}:state", self.key_prefix, api_key_id)
    }

    fn leases_key(&self, api_key_id: i64) -> String {
        format!("{}api_key:{}:leases", self.key_prefix, api_key_id)
    }

    fn active_key(&self) -> String {
        format!("{}api_key:active", self.key_prefix)
    }

    fn state_ttl_seconds(&self) -> u64 {
        self.state_ttl.as_secs().max(1)
    }

    fn lease_ttl_ms(&self) -> i64 {
        i64::try_from(self.request_lease_ttl.as_millis()).unwrap_or(i64::MAX)
    }

    fn redis_cache_error(context: &str, err: impl Display) -> AppStoreError {
        AppStoreError::CacheError(format!("{context}: {err}"))
    }

    fn redis_admission_error(context: &str, err: impl Display) -> ApiKeyGovernanceAdmissionError {
        ApiKeyGovernanceAdmissionError::Internal(format!("{context}: {err}"))
    }

    fn normalize_amounts(amounts: &HashMap<String, i64>) -> Vec<(String, i64)> {
        let mut normalized: HashMap<String, i64> = HashMap::new();
        for (currency, amount) in amounts {
            let currency = normalize_currency_code(currency);
            let total = normalized.entry(currency).or_default();
            *total = total.saturating_add(*amount);
        }
        let mut entries = normalized.into_iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    fn append_budget_baseline(command: &mut Cmd, amounts: &HashMap<String, i64>) {
        let entries = Self::normalize_amounts(amounts);
        command.arg(entries.len());
        for (currency, amount) in entries {
            command.arg(currency).arg(amount);
        }
    }

    fn append_admission_args(
        &self,
        command: &mut Cmd,
        api_key: &CacheApiKey,
        now_ms: i64,
        baseline: &ApiKeyRollupBaseline,
        lease_now_ms: i64,
        lease_id: &str,
    ) {
        let minute_bucket = minute_bucket_start(now_ms);
        let daily_budget_currency = api_key
            .budget_daily_currency
            .as_deref()
            .map(normalize_currency_code)
            .unwrap_or_default();
        let monthly_budget_currency = api_key
            .budget_monthly_currency
            .as_deref()
            .map(normalize_currency_code)
            .unwrap_or_default();

        command
            .arg(api_key.id)
            .arg(now_ms)
            .arg(lease_now_ms)
            .arg(self.lease_ttl_ms())
            .arg(self.state_ttl_seconds())
            .arg(minute_bucket)
            .arg(baseline.day_bucket)
            .arg(baseline.daily_request_count)
            .arg(baseline.daily_token_count)
            .arg(baseline.month_bucket)
            .arg(baseline.monthly_token_count)
            .arg(optional_i32(api_key.rate_limit_rpm))
            .arg(optional_i32(api_key.max_concurrent_requests))
            .arg(optional_i64(api_key.quota_daily_requests))
            .arg(optional_i64(api_key.quota_daily_tokens))
            .arg(optional_i64(api_key.quota_monthly_tokens))
            .arg(daily_budget_currency)
            .arg(optional_i64(api_key.budget_daily_nanos))
            .arg(monthly_budget_currency)
            .arg(optional_i64(api_key.budget_monthly_nanos))
            .arg(lease_id);
        Self::append_budget_baseline(command, &baseline.daily_billed_amounts);
        Self::append_budget_baseline(command, &baseline.monthly_billed_amounts);
    }

    fn append_completion_args(
        &self,
        command: &mut Cmd,
        delta: &ApiKeyCompletionDelta,
        baseline: &ApiKeyRollupBaseline,
    ) {
        command
            .arg(delta.api_key_id)
            .arg(delta.occurred_at)
            .arg(self.state_ttl_seconds())
            .arg(baseline.day_bucket)
            .arg(baseline.daily_request_count)
            .arg(baseline.daily_token_count)
            .arg(baseline.month_bucket)
            .arg(baseline.monthly_token_count)
            .arg(delta.total_tokens)
            .arg(
                delta
                    .billed_currency
                    .as_deref()
                    .map(normalize_currency_code)
                    .unwrap_or_default(),
            )
            .arg(delta.billed_amount_nanos);
        Self::append_budget_baseline(command, &baseline.daily_billed_amounts);
        Self::append_budget_baseline(command, &baseline.monthly_billed_amounts);
    }

    async fn prune_expired_leases(&self, api_key_id: i64) -> Result<u32, AppStoreError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_cache_error("failed to get redis connection", err))?;
        let leases_key = self.leases_key(api_key_id);
        let now_ms = Utc::now().timestamp_millis();
        let _: i64 = cmd("ZREMRANGEBYSCORE")
            .arg(&leases_key)
            .arg("-inf")
            .arg(now_ms)
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_cache_error("failed to prune api key leases", err))?;
        let current_concurrency: i64 = cmd("ZCARD")
            .arg(&leases_key)
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_cache_error("failed to count api key leases", err))?;
        Ok(u32::try_from(current_concurrency).unwrap_or(u32::MAX))
    }

    async fn load_state_hash(
        &self,
        api_key_id: i64,
    ) -> Result<HashMap<String, String>, AppStoreError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_cache_error("failed to get redis connection", err))?;
        cmd("HGETALL")
            .arg(self.state_key(api_key_id))
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_cache_error("failed to load api key runtime state", err))
    }

    async fn remove_active_ids(&self, ids: &[String]) -> Result<(), AppStoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_cache_error("failed to get redis connection", err))?;
        let mut command = cmd("SREM");
        command.arg(self.active_key());
        for id in ids {
            command.arg(id);
        }
        let _: i64 = command
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_cache_error("failed to remove inactive api keys", err))?;
        Ok(())
    }
}

#[async_trait]
impl ApiKeyRuntimeStore for RedisApiKeyRuntimeStore {
    async fn snapshot(&self, api_key_id: i64) -> Result<ApiKeyGovernanceSnapshot, AppStoreError> {
        let current_concurrency = self.prune_expired_leases(api_key_id).await?;
        let state = self.load_state_hash(api_key_id).await?;
        Ok(snapshot_from_hash(api_key_id, current_concurrency, &state))
    }

    async fn snapshots(&self) -> Result<Vec<ApiKeyGovernanceSnapshot>, AppStoreError> {
        let ids: Vec<String> = {
            let mut conn =
                self.pool.get().await.map_err(|err| {
                    Self::redis_cache_error("failed to get redis connection", err)
                })?;
            cmd("SMEMBERS")
                .arg(self.active_key())
                .query_async(&mut *conn)
                .await
                .map_err(|err| Self::redis_cache_error("failed to list active api keys", err))?
        };

        let mut snapshots = Vec::new();
        let mut stale_ids = Vec::new();
        for raw_id in ids {
            let Ok(api_key_id) = raw_id.parse::<i64>() else {
                stale_ids.push(raw_id);
                continue;
            };
            let snapshot = self.snapshot(api_key_id).await?;
            if snapshot_is_active(&snapshot) {
                snapshots.push(snapshot);
            } else {
                stale_ids.push(raw_id);
            }
        }

        self.remove_active_ids(&stale_ids).await?;
        snapshots.sort_by_key(|snapshot| snapshot.api_key_id);
        Ok(snapshots)
    }

    async fn try_begin_request(
        &self,
        api_key: &CacheApiKey,
        now_ms: i64,
        baseline: &ApiKeyRollupBaseline,
    ) -> Result<Option<ApiKeyRequestLease>, ApiKeyGovernanceAdmissionError> {
        let lease_id = Uuid::new_v4().to_string();
        let lease_now_ms = Utc::now().timestamp_millis();
        let mut conn =
            self.pool.get().await.map_err(|err| {
                Self::redis_admission_error("failed to get redis connection", err)
            })?;

        let mut command = cmd("EVAL");
        command
            .arg(ADMISSION_SCRIPT)
            .arg(3)
            .arg(self.state_key(api_key.id))
            .arg(self.leases_key(api_key.id))
            .arg(self.active_key());
        self.append_admission_args(
            &mut command,
            api_key,
            now_ms,
            baseline,
            lease_now_ms,
            &lease_id,
        );

        let result: (i64, String, i64, i64, String) = command
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_admission_error("api key admission script failed", err))?;

        admission_result_to_domain(api_key.id, result)
    }

    async fn release_request_lease(&self, lease: &ApiKeyRequestLease) -> Result<(), AppStoreError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_cache_error("failed to get redis connection", err))?;
        let _: i64 = cmd("ZREM")
            .arg(self.leases_key(lease.api_key_id()))
            .arg(lease.lease_id())
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_cache_error("failed to release api key lease", err))?;
        Ok(())
    }

    async fn apply_completion(
        &self,
        delta: &ApiKeyCompletionDelta,
        baseline: &ApiKeyRollupBaseline,
    ) -> Result<(), AppStoreError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_cache_error("failed to get redis connection", err))?;
        let mut command = cmd("EVAL");
        command
            .arg(COMPLETION_SCRIPT)
            .arg(2)
            .arg(self.state_key(delta.api_key_id))
            .arg(self.active_key());
        self.append_completion_args(&mut command, delta, baseline);
        let _: i64 = command
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_cache_error("api key completion script failed", err))?;
        Ok(())
    }
}

fn optional_i32(value: Option<i32>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn optional_i64(value: Option<i64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn parse_i64_field(state: &HashMap<String, String>, field: &str) -> i64 {
    state
        .get(field)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default()
}

fn parse_u32_field(state: &HashMap<String, String>, field: &str) -> u32 {
    u32::try_from(parse_i64_field(state, field)).unwrap_or(u32::MAX)
}

fn parse_optional_i64_field(state: &HashMap<String, String>, field: &str) -> Option<i64> {
    state.get(field).and_then(|value| value.parse::<i64>().ok())
}

fn parse_budget_snapshots(
    state: &HashMap<String, String>,
    prefix: &str,
) -> Vec<ApiKeyBilledAmountSnapshot> {
    let mut snapshots = state
        .iter()
        .filter_map(|(field, value)| {
            field
                .strip_prefix(prefix)
                .map(|currency| ApiKeyBilledAmountSnapshot {
                    currency: currency.to_string(),
                    amount_nanos: value.parse::<i64>().unwrap_or_default(),
                })
        })
        .collect::<Vec<_>>();
    snapshots.sort_by(|a, b| a.currency.cmp(&b.currency));
    snapshots
}

fn snapshot_from_hash(
    api_key_id: i64,
    current_concurrency: u32,
    state: &HashMap<String, String>,
) -> ApiKeyGovernanceSnapshot {
    ApiKeyGovernanceSnapshot {
        api_key_id,
        current_concurrency,
        current_minute_bucket: parse_optional_i64_field(state, "minute_bucket"),
        current_minute_request_count: parse_u32_field(state, "minute_request_count"),
        day_bucket: parse_optional_i64_field(state, "day_bucket"),
        daily_request_count: parse_i64_field(state, "daily_request_count"),
        daily_token_count: parse_i64_field(state, "daily_token_count"),
        month_bucket: parse_optional_i64_field(state, "month_bucket"),
        monthly_token_count: parse_i64_field(state, "monthly_token_count"),
        daily_billed_amounts: parse_budget_snapshots(state, "daily_budget:"),
        monthly_billed_amounts: parse_budget_snapshots(state, "monthly_budget:"),
    }
}

fn snapshot_is_active(snapshot: &ApiKeyGovernanceSnapshot) -> bool {
    snapshot.current_concurrency > 0
        || snapshot.current_minute_request_count > 0
        || snapshot.daily_request_count > 0
        || snapshot.daily_token_count > 0
        || snapshot.monthly_token_count > 0
        || snapshot
            .daily_billed_amounts
            .iter()
            .any(|amount| amount.amount_nanos > 0)
        || snapshot
            .monthly_billed_amounts
            .iter()
            .any(|amount| amount.amount_nanos > 0)
}

fn admission_result_to_domain(
    api_key_id: i64,
    result: (i64, String, i64, i64, String),
) -> Result<Option<ApiKeyRequestLease>, ApiKeyGovernanceAdmissionError> {
    let (allowed, marker, limit, current, currency) = result;
    if allowed == 1 {
        return if marker.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ApiKeyRequestLease::new(api_key_id, marker)))
        };
    }

    let current_u32 = u32::try_from(current).unwrap_or(u32::MAX);
    match marker.as_str() {
        "rate" => Err(ApiKeyGovernanceAdmissionError::RateLimited {
            limit: i32::try_from(limit).unwrap_or(i32::MAX),
            current: current_u32,
        }),
        "concurrency" => Err(ApiKeyGovernanceAdmissionError::ConcurrencyLimited {
            limit: i32::try_from(limit).unwrap_or(i32::MAX),
            current: current_u32,
        }),
        "daily_request" => {
            Err(ApiKeyGovernanceAdmissionError::DailyRequestQuotaExceeded { limit, current })
        }
        "daily_token" => {
            Err(ApiKeyGovernanceAdmissionError::DailyTokenQuotaExceeded { limit, current })
        }
        "monthly_token" => {
            Err(ApiKeyGovernanceAdmissionError::MonthlyTokenQuotaExceeded { limit, current })
        }
        "daily_budget" => Err(ApiKeyGovernanceAdmissionError::DailyBudgetExceeded {
            currency,
            limit_nanos: limit,
            current_nanos: current,
        }),
        "monthly_budget" => Err(ApiKeyGovernanceAdmissionError::MonthlyBudgetExceeded {
            currency,
            limit_nanos: limit,
            current_nanos: current,
        }),
        _ => Err(ApiKeyGovernanceAdmissionError::Internal(format!(
            "api key admission script returned unknown result marker: {marker}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb8::Pool;
    use bb8_redis::RedisConnectionManager;
    use std::env;
    use std::sync::Arc;

    use super::super::types::{day_bucket_start, month_bucket_start};
    use crate::database::TestDbContext;
    use crate::schema::enum_def::Action;
    use crate::service::runtime::ApiKeyGovernanceService;

    fn cache_api_key(id: i64) -> CacheApiKey {
        CacheApiKey {
            id,
            api_key_hash: format!("hash-{id}"),
            key_prefix: "cyder-prefix".to_string(),
            key_last4: "1234".to_string(),
            name: format!("redis-runtime-{id}"),
            description: None,
            default_action: Action::Allow,
            is_enabled: true,
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
            acl_rules: vec![],
        }
    }

    async fn redis_pool_or_skip() -> Option<RedisPool> {
        let Ok(url) = env::var("CYDER_TEST_REDIS_URL") else {
            println!("skipping redis api key runtime tests: CYDER_TEST_REDIS_URL is not set");
            return None;
        };
        let manager = RedisConnectionManager::new(url.as_str())
            .expect("CYDER_TEST_REDIS_URL should be a valid Redis URL");
        Some(
            Pool::builder()
                .max_size(4)
                .build(manager)
                .await
                .expect("test Redis pool should connect"),
        )
    }

    fn redis_store(pool: RedisPool, key_prefix: &str) -> RedisApiKeyRuntimeStore {
        RedisApiKeyRuntimeStore::new(
            pool,
            key_prefix.to_string(),
            Duration::from_secs(60),
            Duration::from_secs(3600),
        )
    }

    #[tokio::test]
    async fn redis_store_shared_services_enforce_concurrency_limit() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let service_a = ApiKeyGovernanceService::new(Arc::new(redis_store(pool.clone(), &prefix)));
        let service_b = ApiKeyGovernanceService::new(Arc::new(redis_store(pool.clone(), &prefix)));
        let api_key = CacheApiKey {
            max_concurrent_requests: Some(1),
            ..cache_api_key(710_001)
        };
        let test_db_context =
            TestDbContext::new_sqlite("redis-api-key-concurrency-contract.sqlite");

        test_db_context
            .run_async(async {
                let lease = service_a
                    .try_begin_api_key_request(&api_key)
                    .await
                    .expect("first request should be admitted")
                    .expect("concurrency limit should create lease");

                let err = service_b
                    .try_begin_api_key_request(&api_key)
                    .await
                    .expect_err("second service should see shared concurrency");
                assert_eq!(
                    err,
                    ApiKeyGovernanceAdmissionError::ConcurrencyLimited {
                        limit: 1,
                        current: 1,
                    }
                );

                service_a
                    .release_api_key_request_lease(lease)
                    .await
                    .expect("release should succeed");

                service_b
                    .try_begin_api_key_request(&api_key)
                    .await
                    .expect("second service should admit after release");
            })
            .await;
    }

    #[tokio::test]
    async fn redis_store_shared_services_enforce_rate_limit() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let service_a = ApiKeyGovernanceService::new(Arc::new(redis_store(pool.clone(), &prefix)));
        let service_b = ApiKeyGovernanceService::new(Arc::new(redis_store(pool.clone(), &prefix)));
        let api_key = CacheApiKey {
            rate_limit_rpm: Some(1),
            ..cache_api_key(710_002)
        };
        let test_db_context = TestDbContext::new_sqlite("redis-api-key-rpm-contract.sqlite");

        test_db_context
            .run_async(async {
                service_a
                    .try_begin_api_key_request(&api_key)
                    .await
                    .expect("first request should be admitted");

                let err = service_b
                    .try_begin_api_key_request(&api_key)
                    .await
                    .expect_err("second service should share rpm bucket");
                assert_eq!(
                    err,
                    ApiKeyGovernanceAdmissionError::RateLimited {
                        limit: 1,
                        current: 1,
                    }
                );
            })
            .await;
    }

    #[tokio::test]
    async fn redis_store_completion_written_by_one_service_is_visible_to_another() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let service_a = ApiKeyGovernanceService::new(Arc::new(redis_store(pool.clone(), &prefix)));
        let service_b = ApiKeyGovernanceService::new(Arc::new(redis_store(pool.clone(), &prefix)));
        let api_key = cache_api_key(710_003);
        let test_db_context = TestDbContext::new_sqlite("redis-api-key-completion-contract.sqlite");

        test_db_context
            .run_async(async {
                service_a
                    .try_begin_api_key_request(&api_key)
                    .await
                    .expect("request should be admitted");
                service_a
                    .record_api_key_completion(&ApiKeyCompletionDelta {
                        api_key_id: api_key.id,
                        occurred_at: Utc::now().timestamp_millis(),
                        total_tokens: 17,
                        billed_amount_nanos: 23,
                        billed_currency: Some("usd".to_string()),
                    })
                    .await
                    .expect("completion should be recorded");

                let snapshot = service_b
                    .get_api_key_governance_snapshot(api_key.id)
                    .await
                    .expect("snapshot should load from shared Redis state");
                assert_eq!(snapshot.daily_request_count, 1);
                assert_eq!(snapshot.daily_token_count, 17);
                assert_eq!(snapshot.monthly_token_count, 17);
                assert_eq!(
                    snapshot
                        .daily_billed_amounts
                        .iter()
                        .find(|amount| amount.currency == "USD")
                        .map(|amount| amount.amount_nanos),
                    Some(23)
                );
                assert_eq!(
                    service_b
                        .list_api_key_governance_snapshots()
                        .await
                        .expect("active snapshots should load")
                        .len(),
                    1
                );
            })
            .await;
    }

    #[tokio::test]
    async fn redis_store_same_bucket_baseline_does_not_overwrite_live_counters() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let store_a = redis_store(pool.clone(), &prefix);
        let store_b = redis_store(pool.clone(), &prefix);
        let api_key = cache_api_key(710_004);
        let now_ms = Utc::now().timestamp_millis();
        let baseline = ApiKeyRollupBaseline {
            day_bucket: day_bucket_start(now_ms),
            daily_request_count: 10,
            daily_token_count: 20,
            daily_billed_amounts: HashMap::from([(String::from("usd"), 30)]),
            month_bucket: month_bucket_start(now_ms),
            monthly_token_count: 40,
            monthly_billed_amounts: HashMap::from([(String::from("usd"), 50)]),
        };

        store_a
            .try_begin_request(&api_key, now_ms, &baseline)
            .await
            .expect("request should be admitted");
        store_a
            .apply_completion(
                &ApiKeyCompletionDelta {
                    api_key_id: api_key.id,
                    occurred_at: now_ms,
                    total_tokens: 5,
                    billed_amount_nanos: 7,
                    billed_currency: Some("usd".to_string()),
                },
                &baseline,
            )
            .await
            .expect("completion should apply");

        let conflicting_baseline = ApiKeyRollupBaseline {
            daily_request_count: 999,
            daily_token_count: 999,
            monthly_token_count: 999,
            daily_billed_amounts: HashMap::from([(String::from("USD"), 999)]),
            monthly_billed_amounts: HashMap::from([(String::from("USD"), 999)]),
            ..baseline.clone()
        };
        store_b
            .try_begin_request(&api_key, now_ms + 1_000, &conflicting_baseline)
            .await
            .expect("same bucket request should be admitted");

        let snapshot = store_b
            .snapshot(api_key.id)
            .await
            .expect("snapshot should load");
        assert_eq!(snapshot.daily_request_count, 12);
        assert_eq!(snapshot.daily_token_count, 25);
        assert_eq!(snapshot.monthly_token_count, 45);
        assert_eq!(
            snapshot
                .daily_billed_amounts
                .iter()
                .find(|amount| amount.currency == "USD")
                .map(|amount| amount.amount_nanos),
            Some(37)
        );
        assert_eq!(
            snapshot
                .monthly_billed_amounts
                .iter()
                .find(|amount| amount.currency == "USD")
                .map(|amount| amount.amount_nanos),
            Some(57)
        );
    }
}
