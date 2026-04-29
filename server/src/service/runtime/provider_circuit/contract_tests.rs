use std::env;
use std::sync::Arc;
use std::time::Duration;

use bb8::Pool;
use bb8_redis::{RedisConnectionManager, redis::cmd};
use uuid::Uuid;

use crate::config::ProviderGovernanceConfig;
use crate::service::redis::RedisPool;

use super::memory_store::MemoryProviderCircuitStore;
use super::redis_store::RedisProviderCircuitStore;
use super::types::{
    ProviderCircuitProbePermit, ProviderCircuitRejection, ProviderCircuitStore,
    ProviderHealthSnapshot, ProviderHealthStatus,
};

const TEST_REDIS_STATE_TTL: Duration = Duration::from_secs(60);

fn config(threshold: u32, cooldown_seconds: u64) -> ProviderGovernanceConfig {
    ProviderGovernanceConfig {
        enabled: true,
        consecutive_failure_threshold: threshold,
        open_cooldown_seconds: cooldown_seconds,
    }
}

fn disabled_config() -> ProviderGovernanceConfig {
    ProviderGovernanceConfig {
        enabled: false,
        consecutive_failure_threshold: 1,
        open_cooldown_seconds: 60,
    }
}

fn zero_threshold_config() -> ProviderGovernanceConfig {
    ProviderGovernanceConfig {
        enabled: true,
        consecutive_failure_threshold: 0,
        open_cooldown_seconds: 60,
    }
}

async fn redis_pool_or_skip() -> Option<RedisPool> {
    let Ok(url) = env::var("CYDER_TEST_REDIS_URL") else {
        println!("skipping redis provider circuit contract tests: CYDER_TEST_REDIS_URL is not set");
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

fn redis_store(pool: RedisPool, probe_ttl: Duration) -> RedisProviderCircuitStore {
    RedisProviderCircuitStore::new(
        pool,
        format!("runtime:test:{}:", Uuid::new_v4()),
        probe_ttl,
        TEST_REDIS_STATE_TTL,
    )
}

fn redis_store_with_prefix(
    pool: RedisPool,
    key_prefix: String,
    probe_ttl: Duration,
    state_ttl: Duration,
) -> RedisProviderCircuitStore {
    RedisProviderCircuitStore::new(pool, key_prefix, probe_ttl, state_ttl)
}

fn redis_state_key(key_prefix: &str, provider_id: i64) -> String {
    format!("{key_prefix}provider_circuit:{provider_id}:state")
}

async fn redis_key_exists(pool: RedisPool, key: &str) -> bool {
    let mut conn = pool
        .get()
        .await
        .expect("test Redis connection should be available");
    let exists: i64 = cmd("EXISTS")
        .arg(key)
        .query_async(&mut *conn)
        .await
        .expect("EXISTS should succeed");
    exists > 0
}

async fn redis_key_pttl_ms(pool: RedisPool, key: &str) -> i64 {
    let mut conn = pool
        .get()
        .await
        .expect("test Redis connection should be available");
    cmd("PTTL")
        .arg(key)
        .query_async(&mut *conn)
        .await
        .expect("PTTL should succeed")
}

fn synthetic_healthy() -> ProviderHealthSnapshot {
    ProviderHealthSnapshot::synthetic_healthy()
}

fn assert_probe_permit_contract(permit: &ProviderCircuitProbePermit, provider_id: i64) {
    assert_eq!(permit.provider_id(), provider_id);
    assert!(!permit.decision_id().is_empty());
    assert!(!permit.lease_id().is_empty());
    assert!(permit.issued_at_ms() > 0);
    assert!(permit.probe_expires_at_ms() > permit.issued_at_ms());
    assert_eq!(permit.expires_at_ms(), permit.probe_expires_at_ms());
}

async fn assert_threshold_open_contract(store: Arc<dyn ProviderCircuitStore>) {
    let config = config(2, 60);
    store
        .record_failure(700, &config, "timeout".to_string(), None)
        .await
        .expect("first failure should record");
    let snapshot = store.snapshot(700).await.expect("snapshot should load");
    assert_eq!(snapshot.status, ProviderHealthStatus::Healthy);
    assert_eq!(snapshot.consecutive_failures, 1);

    let snapshot = store
        .record_failure(700, &config, "timeout again".to_string(), None)
        .await
        .expect("second failure should open circuit");
    assert_eq!(snapshot.status, ProviderHealthStatus::Open);
    assert_eq!(snapshot.consecutive_failures, 2);
    assert!(snapshot.opened_at.is_some());
}

async fn assert_disabled_governance_noop_contract(
    store: Arc<dyn ProviderCircuitStore>,
    disabled_config: ProviderGovernanceConfig,
    provider_id: i64,
) {
    let enabled_config = config(1, 60);
    let stale_snapshot = store
        .record_failure(provider_id, &enabled_config, "timeout".to_string(), None)
        .await
        .expect("enabled failure should open circuit");
    assert_eq!(stale_snapshot.status, ProviderHealthStatus::Open);
    assert!(stale_snapshot.last_error.is_some());

    let allow = store
        .allow_request(provider_id, &disabled_config)
        .await
        .expect("disabled allow should be synthetic");
    assert!(allow.allowed);
    assert_eq!(allow.snapshot, synthetic_healthy());
    assert!(allow.probe_permit.is_none());
    assert_eq!(
        store
            .snapshot(provider_id)
            .await
            .expect("snapshot should load"),
        stale_snapshot
    );

    let failure = store
        .record_failure(
            provider_id,
            &disabled_config,
            "disabled timeout".to_string(),
            None,
        )
        .await
        .expect("disabled failure should be a no-op");
    assert_eq!(failure, synthetic_healthy());
    assert_eq!(
        store
            .snapshot(provider_id)
            .await
            .expect("snapshot should load"),
        stale_snapshot
    );

    let success = store
        .record_success(provider_id, &disabled_config, None)
        .await
        .expect("disabled success should be a no-op");
    assert_eq!(success, synthetic_healthy());
    assert_eq!(
        store
            .snapshot(provider_id)
            .await
            .expect("snapshot should load"),
        stale_snapshot
    );
}

async fn assert_cooldown_then_half_open_contract(store: Arc<dyn ProviderCircuitStore>) {
    let config = config(1, 1);
    store
        .record_failure(701, &config, "timeout".to_string(), None)
        .await
        .expect("failure should open circuit");

    let blocked = store
        .allow_request(701, &config)
        .await
        .expect("cooldown allow should evaluate");
    assert!(!blocked.allowed);
    assert_eq!(blocked.snapshot.status, ProviderHealthStatus::Open);
    assert_eq!(
        blocked.rejection,
        Some(ProviderCircuitRejection::OpenCooldown)
    );
    assert!(blocked.retry_after.is_some());

    tokio::time::sleep(Duration::from_millis(1100)).await;

    let allowed = store
        .allow_request(701, &config)
        .await
        .expect("post-cooldown allow should evaluate");
    assert!(allowed.allowed);
    assert_eq!(allowed.snapshot.status, ProviderHealthStatus::HalfOpen);
    assert_probe_permit_contract(
        allowed
            .probe_permit
            .as_ref()
            .expect("half-open probe should include a permit"),
        701,
    );
}

async fn assert_single_probe_contract(store: Arc<dyn ProviderCircuitStore>) {
    let config = config(1, 0);
    store
        .record_failure(702, &config, "timeout".to_string(), None)
        .await
        .expect("failure should open circuit");

    let first = store
        .allow_request(702, &config)
        .await
        .expect("allow should evaluate");
    assert!(first.allowed);
    assert_eq!(first.snapshot.status, ProviderHealthStatus::HalfOpen);
    assert_probe_permit_contract(
        first
            .probe_permit
            .as_ref()
            .expect("half-open probe should include a permit"),
        702,
    );

    let second = store
        .allow_request(702, &config)
        .await
        .expect("second allow should evaluate");
    assert!(!second.allowed);
    assert_eq!(
        second.rejection,
        Some(ProviderCircuitRejection::HalfOpenProbeInFlight)
    );
    assert_eq!(second.snapshot.status, ProviderHealthStatus::HalfOpen);
    assert!(second.snapshot.half_open_probe_in_flight);
}

async fn assert_probe_ttl_contract(store: Arc<dyn ProviderCircuitStore>) {
    let config = config(1, 0);
    store
        .record_failure(703, &config, "timeout".to_string(), None)
        .await
        .expect("failure should open circuit");
    let first = store
        .allow_request(703, &config)
        .await
        .expect("first allow should evaluate");
    assert!(first.allowed);

    tokio::time::sleep(Duration::from_millis(80)).await;

    let second = store
        .allow_request(703, &config)
        .await
        .expect("second allow should evaluate");
    assert!(second.allowed);
    assert_probe_permit_contract(
        second
            .probe_permit
            .as_ref()
            .expect("second probe should include a permit"),
        703,
    );
    assert_ne!(
        first
            .probe_permit
            .as_ref()
            .expect("first permit")
            .lease_id(),
        second
            .probe_permit
            .as_ref()
            .expect("second permit")
            .lease_id()
    );
}

async fn assert_success_close_contract(store: Arc<dyn ProviderCircuitStore>) {
    let config = config(1, 0);
    store
        .record_failure(704, &config, "timeout".to_string(), None)
        .await
        .expect("failure should open circuit");
    let decision = store
        .allow_request(704, &config)
        .await
        .expect("allow should evaluate");
    assert!(decision.allowed);

    store
        .record_success(704, &config, None)
        .await
        .expect("success without permit should not fail");
    let snapshot = store.snapshot(704).await.expect("snapshot should load");
    assert_eq!(snapshot.status, ProviderHealthStatus::HalfOpen);
    assert!(snapshot.half_open_probe_in_flight);

    store
        .record_success(704, &config, decision.probe_permit.as_ref())
        .await
        .expect("success with matching permit should close");
    let snapshot = store.snapshot(704).await.expect("snapshot should load");
    assert_eq!(snapshot.status, ProviderHealthStatus::Healthy);
    assert!(!snapshot.half_open_probe_in_flight);
}

async fn assert_failure_reopen_contract(store: Arc<dyn ProviderCircuitStore>) {
    let config = config(1, 0);
    store
        .record_failure(705, &config, "timeout".to_string(), None)
        .await
        .expect("failure should open circuit");
    let decision = store
        .allow_request(705, &config)
        .await
        .expect("allow should evaluate");
    let permit = decision
        .probe_permit
        .as_ref()
        .expect("half-open probe should include a permit");

    let snapshot = store
        .record_failure(705, &config, "half-open timeout".to_string(), Some(permit))
        .await
        .expect("matching probe failure should reopen");
    assert_eq!(snapshot.status, ProviderHealthStatus::Open);
    assert!(!snapshot.half_open_probe_in_flight);
    assert_eq!(snapshot.last_error.as_deref(), Some("half-open timeout"));
}

#[tokio::test]
async fn memory_store_opens_after_threshold_failures() {
    assert_threshold_open_contract(Arc::new(MemoryProviderCircuitStore::default())).await;
}

#[tokio::test]
async fn memory_store_rejects_during_cooldown_then_allows_half_open_probe() {
    assert_cooldown_then_half_open_contract(Arc::new(MemoryProviderCircuitStore::default())).await;
}

#[tokio::test]
async fn memory_store_rejects_second_half_open_probe_while_lease_is_active() {
    assert_single_probe_contract(Arc::new(MemoryProviderCircuitStore::with_probe_lease_ttl(
        Duration::from_secs(30),
    )))
    .await;
}

#[tokio::test]
async fn memory_store_allows_new_half_open_probe_after_lease_ttl() {
    assert_probe_ttl_contract(Arc::new(MemoryProviderCircuitStore::with_probe_lease_ttl(
        Duration::from_millis(20),
    )))
    .await;
}

#[tokio::test]
async fn memory_store_matching_probe_success_closes_circuit() {
    assert_success_close_contract(Arc::new(MemoryProviderCircuitStore::default())).await;
}

#[tokio::test]
async fn memory_store_matching_probe_failure_reopens_circuit() {
    assert_failure_reopen_contract(Arc::new(MemoryProviderCircuitStore::default())).await;
}

#[tokio::test]
async fn memory_store_disabled_governance_returns_synthetic_and_leaves_state_untouched() {
    assert_disabled_governance_noop_contract(
        Arc::new(MemoryProviderCircuitStore::default()),
        disabled_config(),
        706,
    )
    .await;
}

#[tokio::test]
async fn memory_store_zero_threshold_returns_synthetic_and_leaves_state_untouched() {
    assert_disabled_governance_noop_contract(
        Arc::new(MemoryProviderCircuitStore::default()),
        zero_threshold_config(),
        707,
    )
    .await;
}

#[tokio::test]
async fn redis_store_opens_after_threshold_failures() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_threshold_open_contract(Arc::new(redis_store(pool, Duration::from_secs(30)))).await;
}

#[tokio::test]
async fn redis_store_rejects_during_cooldown_then_allows_half_open_probe() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_cooldown_then_half_open_contract(Arc::new(redis_store(pool, Duration::from_secs(30))))
        .await;
}

#[tokio::test]
async fn redis_store_rejects_second_half_open_probe_while_lease_is_active() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_single_probe_contract(Arc::new(redis_store(pool, Duration::from_secs(30)))).await;
}

#[tokio::test]
async fn redis_store_allows_new_half_open_probe_after_lease_ttl() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_probe_ttl_contract(Arc::new(redis_store(pool, Duration::from_millis(20)))).await;
}

#[tokio::test]
async fn redis_store_matching_probe_success_closes_circuit() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_success_close_contract(Arc::new(redis_store(pool, Duration::from_secs(30)))).await;
}

#[tokio::test]
async fn redis_store_matching_probe_failure_reopens_circuit() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_failure_reopen_contract(Arc::new(redis_store(pool, Duration::from_secs(30)))).await;
}

#[tokio::test]
async fn redis_store_disabled_governance_returns_synthetic_and_leaves_state_untouched() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_disabled_governance_noop_contract(
        Arc::new(redis_store(pool, Duration::from_secs(30))),
        disabled_config(),
        708,
    )
    .await;
}

#[tokio::test]
async fn redis_store_zero_threshold_returns_synthetic_and_leaves_state_untouched() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_disabled_governance_noop_contract(
        Arc::new(redis_store(pool, Duration::from_secs(30))),
        zero_threshold_config(),
        709,
    )
    .await;
}

#[tokio::test]
async fn redis_store_disabled_record_failure_does_not_create_state_key() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    let key_prefix = format!("runtime:test:{}:", Uuid::new_v4());
    let provider_id = 710;
    let state_key = redis_state_key(&key_prefix, provider_id);
    let store = redis_store_with_prefix(
        pool.clone(),
        key_prefix,
        Duration::from_secs(30),
        TEST_REDIS_STATE_TTL,
    );

    let snapshot = store
        .record_failure(
            provider_id,
            &disabled_config(),
            "disabled timeout".to_string(),
            None,
        )
        .await
        .expect("disabled failure should return synthetic healthy");
    assert_eq!(snapshot, synthetic_healthy());
    assert!(!redis_key_exists(pool.clone(), &state_key).await);

    let success = store
        .record_success(provider_id, &disabled_config(), None)
        .await
        .expect("disabled success should return synthetic healthy");
    assert_eq!(success, synthetic_healthy());
    assert!(!redis_key_exists(pool.clone(), &state_key).await);

    let allow = store
        .allow_request(provider_id, &disabled_config())
        .await
        .expect("disabled allow should return synthetic healthy");
    assert!(allow.allowed);
    assert_eq!(allow.snapshot, synthetic_healthy());
    assert!(!redis_key_exists(pool, &state_key).await);
}

#[tokio::test]
async fn redis_store_disabled_record_failure_does_not_refresh_stale_state_ttl() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    let key_prefix = format!("runtime:test:{}:", Uuid::new_v4());
    let provider_id = 711;
    let state_key = redis_state_key(&key_prefix, provider_id);
    let store = redis_store_with_prefix(
        pool.clone(),
        key_prefix,
        Duration::from_secs(30),
        TEST_REDIS_STATE_TTL,
    );

    store
        .record_failure(provider_id, &config(1, 60), "timeout".to_string(), None)
        .await
        .expect("enabled failure should create open circuit state");
    let ttl_before = redis_key_pttl_ms(pool.clone(), &state_key).await;
    assert!(ttl_before > 0);

    tokio::time::sleep(Duration::from_millis(30)).await;

    let snapshot = store
        .record_failure(
            provider_id,
            &disabled_config(),
            "disabled timeout".to_string(),
            None,
        )
        .await
        .expect("disabled failure should return synthetic healthy");
    assert_eq!(snapshot, synthetic_healthy());

    let ttl_after = redis_key_pttl_ms(pool, &state_key).await;
    assert!(ttl_after > 0);
    assert!(
        ttl_after <= ttl_before,
        "disabled record_failure must not refresh stale state TTL: before={ttl_before}, after={ttl_after}"
    );
}
