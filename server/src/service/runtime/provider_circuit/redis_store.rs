use async_trait::async_trait;
use bb8_redis::redis::{Cmd, cmd};
use chrono::Utc;
use std::fmt::Display;
use std::time::Duration;
use uuid::Uuid;

use crate::config::ProviderGovernanceConfig;
use crate::service::redis::RedisPool;

use super::types::{
    ProviderCircuitDecision, ProviderCircuitError, ProviderCircuitProbePermit,
    ProviderCircuitRejection, ProviderCircuitStore, ProviderHealthSnapshot, ProviderHealthStatus,
};

const ALLOW_SCRIPT: &str = r#"
local state_key = KEYS[1]

local now_ms = tonumber(ARGV[1])
local governance_enabled = ARGV[2] == '1'
local open_cooldown_ms = tonumber(ARGV[3])
local probe_lease_ttl_ms = tonumber(ARGV[4])
local state_ttl_seconds = tonumber(ARGV[5])
local decision_id = ARGV[6]
local lease_id = ARGV[7]

local function raw(field)
    return redis.call('HGET', state_key, field) or ''
end

local function hnum(field)
    local value = redis.call('HGET', state_key, field)
    if not value then
        return 0
    end
    return tonumber(value) or 0
end

local function status_value()
    local status = redis.call('HGET', state_key, 'status')
    if not status then
        return 'healthy'
    end
    return status
end

local function prune_expired_probe()
    local probe_expires_at = tonumber(redis.call('HGET', state_key, 'probe_expires_at'))
    if probe_expires_at and probe_expires_at <= now_ms then
        redis.call('HDEL', state_key, 'probe_decision_id', 'probe_lease_id', 'probe_issued_at', 'probe_expires_at')
    end
end

local function result(allowed, rejection, retry_after_ms, permit_expires_at_ms)
    local probe_lease_id = redis.call('HGET', state_key, 'probe_lease_id')
    local probe_in_flight = 0
    if probe_lease_id then
        probe_in_flight = 1
    end
    return {
        allowed,
        status_value(),
        hnum('consecutive_failures'),
        probe_in_flight,
        raw('opened_at'),
        raw('last_failure_at'),
        raw('last_recovered_at'),
        raw('last_error'),
        rejection or '',
        retry_after_ms,
        permit_expires_at_ms or -1
    }
end

local function synthetic_result()
    return {
        1,
        'healthy',
        0,
        0,
        '',
        '',
        '',
        '',
        '',
        -1,
        -1
    }
end

if not governance_enabled then
    return synthetic_result()
end

prune_expired_probe()

local status = status_value()
if status ~= 'healthy' and status ~= 'open' and status ~= 'half_open' then
    return result(0, '', -1, -1)
end

if status == 'healthy' then
    return result(1, '', -1, -1)
end

if status == 'open' then
    local opened_at = tonumber(redis.call('HGET', state_key, 'opened_at')) or now_ms
    local elapsed_ms = now_ms - opened_at
    local retry_after_ms = open_cooldown_ms - elapsed_ms
    if retry_after_ms > 0 then
        return result(0, 'open_cooldown', retry_after_ms, -1)
    end

    local probe_expires_at = now_ms + probe_lease_ttl_ms
    redis.call(
        'HSET',
        state_key,
        'status',
        'half_open',
        'probe_decision_id',
        decision_id,
        'probe_lease_id',
        lease_id,
        'probe_issued_at',
        now_ms,
        'probe_expires_at',
        probe_expires_at,
        'updated_at_ms',
        now_ms
    )
    redis.call('EXPIRE', state_key, state_ttl_seconds)
    return result(1, '', -1, probe_expires_at)
end

if redis.call('HGET', state_key, 'probe_lease_id') then
    return result(0, 'half_open_probe_in_flight', -1, -1)
end

local probe_expires_at = now_ms + probe_lease_ttl_ms
redis.call(
    'HSET',
    state_key,
    'status',
    'half_open',
    'probe_decision_id',
    decision_id,
    'probe_lease_id',
    lease_id,
    'probe_issued_at',
    now_ms,
    'probe_expires_at',
    probe_expires_at,
    'updated_at_ms',
    now_ms
)
redis.call('EXPIRE', state_key, state_ttl_seconds)
return result(1, '', -1, probe_expires_at)
"#;

const RECORD_SUCCESS_SCRIPT: &str = r#"
local state_key = KEYS[1]

local provider_id = ARGV[1]
local now_ms = tonumber(ARGV[2])
local state_ttl_seconds = tonumber(ARGV[3])
local governance_enabled = ARGV[4] == '1'
local permit_provider_id = ARGV[5]
local permit_lease_id = ARGV[6]

local function raw(field)
    return redis.call('HGET', state_key, field) or ''
end

local function hnum(field)
    local value = redis.call('HGET', state_key, field)
    if not value then
        return 0
    end
    return tonumber(value) or 0
end

local function status_value()
    local status = redis.call('HGET', state_key, 'status')
    if not status then
        return 'healthy'
    end
    return status
end

local function prune_expired_probe()
    local probe_expires_at = tonumber(redis.call('HGET', state_key, 'probe_expires_at'))
    if probe_expires_at and probe_expires_at <= now_ms then
        redis.call('HDEL', state_key, 'probe_decision_id', 'probe_lease_id', 'probe_issued_at', 'probe_expires_at')
    end
end

local function result()
    local probe_lease_id = redis.call('HGET', state_key, 'probe_lease_id')
    local probe_in_flight = 0
    if probe_lease_id then
        probe_in_flight = 1
    end
    return {
        status_value(),
        hnum('consecutive_failures'),
        probe_in_flight,
        raw('opened_at'),
        raw('last_failure_at'),
        raw('last_recovered_at'),
        raw('last_error')
    }
end

local function synthetic_result()
    return {
        'healthy',
        0,
        0,
        '',
        '',
        '',
        ''
    }
end

if not governance_enabled then
    return synthetic_result()
end

prune_expired_probe()

local status = status_value()
if status ~= 'healthy' and status ~= 'open' and status ~= 'half_open' then
    return result()
end

if status == 'open' then
    return result()
end

if status == 'half_open' then
    local active_lease_id = redis.call('HGET', state_key, 'probe_lease_id')
    if permit_provider_id ~= provider_id or not active_lease_id or active_lease_id ~= permit_lease_id then
        return result()
    end
end

local was_unhealthy = status ~= 'healthy'
redis.call(
    'HSET',
    state_key,
    'status',
    'healthy',
    'consecutive_failures',
    0,
    'updated_at_ms',
    now_ms
)
redis.call('HDEL', state_key, 'opened_at', 'probe_lease_id', 'probe_expires_at', 'last_error')
redis.call('HDEL', state_key, 'probe_decision_id', 'probe_issued_at')
if was_unhealthy then
    redis.call('HSET', state_key, 'last_recovered_at', now_ms)
end
redis.call('EXPIRE', state_key, state_ttl_seconds)
return result()
"#;

const RECORD_FAILURE_SCRIPT: &str = r#"
local state_key = KEYS[1]

local provider_id = ARGV[1]
local now_ms = tonumber(ARGV[2])
local state_ttl_seconds = tonumber(ARGV[3])
local governance_enabled = ARGV[4] == '1'
local failure_threshold = tonumber(ARGV[5])
local error_message = ARGV[6]
local permit_provider_id = ARGV[7]
local permit_lease_id = ARGV[8]

local function raw(field)
    return redis.call('HGET', state_key, field) or ''
end

local function hnum(field)
    local value = redis.call('HGET', state_key, field)
    if not value then
        return 0
    end
    return tonumber(value) or 0
end

local function status_value()
    local status = redis.call('HGET', state_key, 'status')
    if not status then
        return 'healthy'
    end
    return status
end

local function prune_expired_probe()
    local probe_expires_at = tonumber(redis.call('HGET', state_key, 'probe_expires_at'))
    if probe_expires_at and probe_expires_at <= now_ms then
        redis.call('HDEL', state_key, 'probe_decision_id', 'probe_lease_id', 'probe_issued_at', 'probe_expires_at')
    end
end

local function result()
    local probe_lease_id = redis.call('HGET', state_key, 'probe_lease_id')
    local probe_in_flight = 0
    if probe_lease_id then
        probe_in_flight = 1
    end
    return {
        status_value(),
        hnum('consecutive_failures'),
        probe_in_flight,
        raw('opened_at'),
        raw('last_failure_at'),
        raw('last_recovered_at'),
        raw('last_error')
    }
end

local function synthetic_result()
    return {
        'healthy',
        0,
        0,
        '',
        '',
        '',
        ''
    }
end

if not governance_enabled then
    return synthetic_result()
end

prune_expired_probe()

local status = status_value()
if status ~= 'healthy' and status ~= 'open' and status ~= 'half_open' then
    return result()
end

local half_open_probe_failed = false
if status == 'half_open' then
    local active_lease_id = redis.call('HGET', state_key, 'probe_lease_id')
    half_open_probe_failed = permit_provider_id == provider_id and active_lease_id and active_lease_id == permit_lease_id
    if not half_open_probe_failed then
        return result()
    end
end

local consecutive_failures = hnum('consecutive_failures') + 1
redis.call(
    'HSET',
    state_key,
    'last_failure_at',
    now_ms,
    'last_error',
    error_message,
    'consecutive_failures',
    consecutive_failures,
    'updated_at_ms',
    now_ms
)

if half_open_probe_failed or consecutive_failures >= failure_threshold then
    redis.call('HSET', state_key, 'status', 'open', 'opened_at', now_ms)
    redis.call('HDEL', state_key, 'probe_decision_id', 'probe_lease_id', 'probe_issued_at', 'probe_expires_at')
else
    redis.call('HSET', state_key, 'status', status)
end

redis.call('EXPIRE', state_key, state_ttl_seconds)
return result()
"#;

const SNAPSHOT_SCRIPT: &str = r#"
local state_key = KEYS[1]

local now_ms = tonumber(ARGV[1])

local function raw(field)
    return redis.call('HGET', state_key, field) or ''
end

local function hnum(field)
    local value = redis.call('HGET', state_key, field)
    if not value then
        return 0
    end
    return tonumber(value) or 0
end

local function status_value()
    local status = redis.call('HGET', state_key, 'status')
    if not status then
        return 'healthy'
    end
    return status
end

local probe_expires_at = tonumber(redis.call('HGET', state_key, 'probe_expires_at'))
if probe_expires_at and probe_expires_at <= now_ms then
    redis.call('HDEL', state_key, 'probe_decision_id', 'probe_lease_id', 'probe_issued_at', 'probe_expires_at')
end

local probe_lease_id = redis.call('HGET', state_key, 'probe_lease_id')
local probe_in_flight = 0
if probe_lease_id then
    probe_in_flight = 1
end

return {
    status_value(),
    hnum('consecutive_failures'),
    probe_in_flight,
    raw('opened_at'),
    raw('last_failure_at'),
    raw('last_recovered_at'),
    raw('last_error')
}
"#;

type RedisAllowResult = (
    i64,
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
);

type RedisSnapshotResult = (String, i64, i64, String, String, String, String);

#[derive(Clone)]
pub struct RedisProviderCircuitStore {
    pool: RedisPool,
    key_prefix: String,
    probe_lease_ttl: Duration,
    state_ttl: Duration,
}

impl RedisProviderCircuitStore {
    pub fn new(
        pool: RedisPool,
        key_prefix: impl Into<String>,
        probe_lease_ttl: Duration,
        state_ttl: Duration,
    ) -> Self {
        Self {
            pool,
            key_prefix: key_prefix.into(),
            probe_lease_ttl,
            state_ttl,
        }
    }

    fn state_key(&self, provider_id: i64) -> String {
        format!("{}provider_circuit:{}:state", self.key_prefix, provider_id)
    }

    fn state_ttl_seconds(&self) -> u64 {
        self.state_ttl.as_secs().max(1)
    }

    fn probe_lease_ttl_ms(&self) -> i64 {
        i64::try_from(self.probe_lease_ttl.as_millis()).unwrap_or(i64::MAX)
    }

    fn redis_error(context: &str, err: impl Display) -> ProviderCircuitError {
        ProviderCircuitError::Backend(format!("{context}: {err}"))
    }

    fn append_permit_args(command: &mut Cmd, permit: Option<&ProviderCircuitProbePermit>) {
        if let Some(permit) = permit {
            command.arg(permit.provider_id()).arg(permit.lease_id());
        } else {
            command.arg("").arg("");
        }
    }
}

#[async_trait]
impl ProviderCircuitStore for RedisProviderCircuitStore {
    async fn allow_request(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
    ) -> Result<ProviderCircuitDecision, ProviderCircuitError> {
        let now_ms = Utc::now().timestamp_millis();
        let decision_id = Uuid::new_v4().to_string();
        let lease_id = Uuid::new_v4().to_string();
        let cooldown_ms = i64::try_from(config.open_cooldown().as_millis()).unwrap_or(i64::MAX);
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_error("failed to get redis connection", err))?;
        let result: RedisAllowResult = cmd("EVAL")
            .arg(ALLOW_SCRIPT)
            .arg(1)
            .arg(self.state_key(provider_id))
            .arg(now_ms)
            .arg(if config.is_enabled() { "1" } else { "0" })
            .arg(cooldown_ms)
            .arg(self.probe_lease_ttl_ms())
            .arg(self.state_ttl_seconds())
            .arg(&decision_id)
            .arg(&lease_id)
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_error("provider circuit allow script failed", err))?;

        allow_result_to_domain(provider_id, decision_id, lease_id, now_ms, result)
    }

    async fn record_success(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
        permit: Option<&ProviderCircuitProbePermit>,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
        let now_ms = Utc::now().timestamp_millis();
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_error("failed to get redis connection", err))?;
        let mut command = cmd("EVAL");
        command
            .arg(RECORD_SUCCESS_SCRIPT)
            .arg(1)
            .arg(self.state_key(provider_id))
            .arg(provider_id)
            .arg(now_ms)
            .arg(self.state_ttl_seconds())
            .arg(if config.is_enabled() { "1" } else { "0" });
        Self::append_permit_args(&mut command, permit);
        let result: RedisSnapshotResult = command.query_async(&mut *conn).await.map_err(|err| {
            Self::redis_error("provider circuit record success script failed", err)
        })?;
        snapshot_result_to_domain(result)
    }

    async fn record_failure(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
        error_message: String,
        permit: Option<&ProviderCircuitProbePermit>,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
        let now_ms = Utc::now().timestamp_millis();
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_error("failed to get redis connection", err))?;
        let mut command = cmd("EVAL");
        command
            .arg(RECORD_FAILURE_SCRIPT)
            .arg(1)
            .arg(self.state_key(provider_id))
            .arg(provider_id)
            .arg(now_ms)
            .arg(self.state_ttl_seconds())
            .arg(if config.is_enabled() { "1" } else { "0" })
            .arg(config.consecutive_failure_threshold)
            .arg(error_message);
        Self::append_permit_args(&mut command, permit);
        let result: RedisSnapshotResult = command.query_async(&mut *conn).await.map_err(|err| {
            Self::redis_error("provider circuit record failure script failed", err)
        })?;
        snapshot_result_to_domain(result)
    }

    async fn snapshot(
        &self,
        provider_id: i64,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
        let now_ms = Utc::now().timestamp_millis();
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_error("failed to get redis connection", err))?;
        let result: RedisSnapshotResult = cmd("EVAL")
            .arg(SNAPSHOT_SCRIPT)
            .arg(1)
            .arg(self.state_key(provider_id))
            .arg(now_ms)
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_error("provider circuit snapshot script failed", err))?;
        snapshot_result_to_domain(result)
    }
}

fn allow_result_to_domain(
    provider_id: i64,
    decision_id: String,
    lease_id: String,
    issued_at_ms: i64,
    result: RedisAllowResult,
) -> Result<ProviderCircuitDecision, ProviderCircuitError> {
    let (
        allowed,
        status,
        consecutive_failures,
        half_open_probe_in_flight,
        opened_at,
        last_failure_at,
        last_recovered_at,
        last_error,
        rejection,
        retry_after_ms,
        permit_expires_at_ms,
    ) = result;
    let snapshot = snapshot_from_parts(
        status,
        consecutive_failures,
        half_open_probe_in_flight,
        opened_at,
        last_failure_at,
        last_recovered_at,
        last_error,
    )?;
    let permit = if allowed == 1 && permit_expires_at_ms >= 0 {
        Some(ProviderCircuitProbePermit::new(
            provider_id,
            decision_id,
            lease_id,
            issued_at_ms,
            permit_expires_at_ms,
        ))
    } else {
        None
    };
    if allowed == 1 {
        return Ok(ProviderCircuitDecision::allowed(snapshot, permit));
    }
    let retry_after = if retry_after_ms >= 0 {
        Some(Duration::from_millis(
            u64::try_from(retry_after_ms).unwrap_or(u64::MAX),
        ))
    } else {
        None
    };
    Ok(ProviderCircuitDecision::rejected(
        snapshot,
        parse_rejection(&rejection)?,
        retry_after,
    ))
}

fn snapshot_result_to_domain(
    result: RedisSnapshotResult,
) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
    let (
        status,
        consecutive_failures,
        half_open_probe_in_flight,
        opened_at,
        last_failure_at,
        last_recovered_at,
        last_error,
    ) = result;
    snapshot_from_parts(
        status,
        consecutive_failures,
        half_open_probe_in_flight,
        opened_at,
        last_failure_at,
        last_recovered_at,
        last_error,
    )
}

fn snapshot_from_parts(
    status: String,
    consecutive_failures: i64,
    half_open_probe_in_flight: i64,
    opened_at: String,
    last_failure_at: String,
    last_recovered_at: String,
    last_error: String,
) -> Result<ProviderHealthSnapshot, ProviderCircuitError> {
    Ok(ProviderHealthSnapshot {
        status: parse_status(&status)?,
        consecutive_failures: u32::try_from(consecutive_failures).unwrap_or(u32::MAX),
        half_open_probe_in_flight: half_open_probe_in_flight > 0,
        opened_at: parse_optional_i64(&opened_at),
        last_failure_at: parse_optional_i64(&last_failure_at),
        last_recovered_at: parse_optional_i64(&last_recovered_at),
        last_error: if last_error.is_empty() {
            None
        } else {
            Some(last_error)
        },
    })
}

fn parse_status(status: &str) -> Result<ProviderHealthStatus, ProviderCircuitError> {
    match status {
        "healthy" => Ok(ProviderHealthStatus::Healthy),
        "open" => Ok(ProviderHealthStatus::Open),
        "half_open" => Ok(ProviderHealthStatus::HalfOpen),
        other => Err(ProviderCircuitError::Backend(format!(
            "provider circuit returned unknown status: {other}"
        ))),
    }
}

fn parse_rejection(rejection: &str) -> Result<ProviderCircuitRejection, ProviderCircuitError> {
    match rejection {
        "open_cooldown" => Ok(ProviderCircuitRejection::OpenCooldown),
        "half_open_probe_in_flight" => Ok(ProviderCircuitRejection::HalfOpenProbeInFlight),
        other => Err(ProviderCircuitError::Backend(format!(
            "provider circuit returned unknown rejection: {other}"
        ))),
    }
}

fn parse_optional_i64(value: &str) -> Option<i64> {
    if value.is_empty() {
        None
    } else {
        value.parse::<i64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb8::Pool;
    use bb8_redis::RedisConnectionManager;
    use std::env;

    fn redis_unavailable_pool() -> RedisPool {
        let manager = RedisConnectionManager::new("redis://127.0.0.1:1")
            .expect("redis test URL should be valid");
        Pool::builder()
            .connection_timeout(Duration::from_millis(20))
            .build_unchecked(manager)
    }

    #[tokio::test]
    async fn snapshot_returns_backend_error_when_redis_connection_fails() {
        let store = RedisProviderCircuitStore::new(
            redis_unavailable_pool(),
            "runtime:test:unavailable:",
            Duration::from_secs(30),
            Duration::from_secs(60),
        );

        let err = store
            .snapshot(1)
            .await
            .expect_err("redis connection failure should be reported");
        assert!(matches!(err, ProviderCircuitError::Backend(_)));
    }

    #[test]
    fn unknown_redis_status_is_backend_error_not_healthy() {
        let err = snapshot_result_to_domain((
            "corrupted".to_string(),
            0,
            0,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ))
        .expect_err("unknown status should be a backend error");
        assert!(matches!(err, ProviderCircuitError::Backend(_)));
    }

    async fn redis_pool_or_skip() -> Option<RedisPool> {
        let Ok(url) = env::var("CYDER_TEST_REDIS_URL") else {
            println!("skipping redis provider circuit tests: CYDER_TEST_REDIS_URL is not set");
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

    fn redis_store(pool: RedisPool, key_prefix: &str) -> RedisProviderCircuitStore {
        RedisProviderCircuitStore::new(
            pool,
            key_prefix.to_string(),
            Duration::from_secs(30),
            Duration::from_secs(3600),
        )
    }

    fn config(threshold: u32, cooldown_seconds: u64) -> ProviderGovernanceConfig {
        ProviderGovernanceConfig {
            enabled: true,
            consecutive_failure_threshold: threshold,
            open_cooldown_seconds: cooldown_seconds,
        }
    }

    #[tokio::test]
    async fn redis_store_unknown_provider_snapshot_defaults_to_healthy() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let store = redis_store(pool, &prefix);

        let snapshot = store.snapshot(720_000).await.expect("snapshot should load");
        assert_eq!(snapshot.status, ProviderHealthStatus::Healthy);
        assert_eq!(snapshot.consecutive_failures, 0);
        assert!(!snapshot.half_open_probe_in_flight);
    }

    #[tokio::test]
    async fn redis_store_shared_instances_reject_second_half_open_probe() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let store_a = redis_store(pool.clone(), &prefix);
        let store_b = redis_store(pool, &prefix);
        let config = config(1, 0);
        let provider_id = 720_001;

        store_a
            .record_failure(provider_id, &config, "timeout".to_string(), None)
            .await
            .expect("failure should open circuit");

        let first = store_a
            .allow_request(provider_id, &config)
            .await
            .expect("first probe should evaluate");
        assert!(first.allowed);
        assert_eq!(first.snapshot.status, ProviderHealthStatus::HalfOpen);
        let permit = first
            .probe_permit
            .as_ref()
            .expect("half-open probe should include a permit");
        assert_eq!(permit.provider_id(), provider_id);
        assert!(!permit.decision_id().is_empty());
        assert!(!permit.lease_id().is_empty());
        assert!(permit.probe_expires_at_ms() > permit.issued_at_ms());

        let second = store_b
            .allow_request(provider_id, &config)
            .await
            .expect("second probe should evaluate");
        assert!(!second.allowed);
        assert_eq!(
            second.rejection,
            Some(ProviderCircuitRejection::HalfOpenProbeInFlight)
        );
        assert_eq!(second.snapshot.status, ProviderHealthStatus::HalfOpen);
        assert!(second.snapshot.half_open_probe_in_flight);
    }

    #[tokio::test]
    async fn redis_store_half_open_success_closes_shared_circuit() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let store_a = redis_store(pool.clone(), &prefix);
        let store_b = redis_store(pool, &prefix);
        let config = config(1, 0);
        let provider_id = 720_002;

        store_a
            .record_failure(provider_id, &config, "timeout".to_string(), None)
            .await
            .expect("failure should open circuit");
        let decision = store_b
            .allow_request(provider_id, &config)
            .await
            .expect("probe should be allowed");
        let permit = decision
            .probe_permit
            .as_ref()
            .expect("half-open probe should include a permit");

        store_a
            .record_success(provider_id, &config, Some(permit))
            .await
            .expect("matching probe success should close circuit");
        let snapshot = store_b
            .snapshot(provider_id)
            .await
            .expect("snapshot should load");
        assert_eq!(snapshot.status, ProviderHealthStatus::Healthy);
        assert_eq!(snapshot.consecutive_failures, 0);
        assert!(!snapshot.half_open_probe_in_flight);
        assert!(snapshot.last_recovered_at.is_some());
        assert!(snapshot.last_error.is_none());
    }

    #[tokio::test]
    async fn redis_store_half_open_failure_reopens_shared_circuit() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let store_a = redis_store(pool.clone(), &prefix);
        let store_b = redis_store(pool, &prefix);
        let config = config(1, 0);
        let provider_id = 720_003;

        store_a
            .record_failure(provider_id, &config, "timeout".to_string(), None)
            .await
            .expect("failure should open circuit");
        let decision = store_a
            .allow_request(provider_id, &config)
            .await
            .expect("probe should be allowed");
        let permit = decision
            .probe_permit
            .as_ref()
            .expect("half-open probe should include a permit");

        let snapshot = store_b
            .record_failure(
                provider_id,
                &config,
                "half-open timeout".to_string(),
                Some(permit),
            )
            .await
            .expect("matching probe failure should reopen circuit");
        assert_eq!(snapshot.status, ProviderHealthStatus::Open);
        assert!(!snapshot.half_open_probe_in_flight);
        assert_eq!(snapshot.last_error.as_deref(), Some("half-open timeout"));
    }

    #[tokio::test]
    async fn redis_store_open_cooldown_rejects_shared_instances_until_elapsed() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let store_a = redis_store(pool.clone(), &prefix);
        let store_b = redis_store(pool, &prefix);
        let config = config(1, 60);
        let provider_id = 720_004;

        store_a
            .record_failure(provider_id, &config, "timeout".to_string(), None)
            .await
            .expect("failure should open circuit");

        let decision = store_b
            .allow_request(provider_id, &config)
            .await
            .expect("allow should evaluate");
        assert!(!decision.allowed);
        assert_eq!(decision.snapshot.status, ProviderHealthStatus::Open);
        assert_eq!(
            decision.rejection,
            Some(ProviderCircuitRejection::OpenCooldown)
        );
        assert!(decision.retry_after.is_some());
        assert!(decision.probe_permit.is_none());
    }

    #[tokio::test]
    async fn redis_store_new_instance_sees_previous_open_state() {
        let Some(pool) = redis_pool_or_skip().await else {
            return;
        };
        let prefix = format!("runtime:test:{}:", Uuid::new_v4());
        let store_a = redis_store(pool.clone(), &prefix);
        let config = config(1, 60);
        let provider_id = 720_005;

        store_a
            .record_failure(provider_id, &config, "timeout".to_string(), None)
            .await
            .expect("failure should open circuit");

        let restarted_store = redis_store(pool, &prefix);
        let snapshot = restarted_store
            .snapshot(provider_id)
            .await
            .expect("snapshot should load from shared Redis state");
        assert_eq!(snapshot.status, ProviderHealthStatus::Open);
        assert_eq!(snapshot.consecutive_failures, 1);
        assert_eq!(snapshot.last_error.as_deref(), Some("timeout"));
    }
}
