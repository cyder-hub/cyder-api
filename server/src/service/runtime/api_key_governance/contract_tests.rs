use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;

use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use uuid::Uuid;

use crate::schema::enum_def::Action;
use crate::service::cache::types::CacheApiKey;
use crate::service::redis::RedisPool;

use super::memory_store::MemoryApiKeyRuntimeStore;
use super::redis_store::RedisApiKeyRuntimeStore;
use super::types::{
    ApiKeyCompletionDelta, ApiKeyGovernanceAdmissionError, ApiKeyRollupBaseline,
    ApiKeyRuntimeStore, day_bucket_start, month_bucket_start,
};

const TEST_REDIS_STATE_TTL: Duration = Duration::from_secs(60);

fn cache_api_key(id: i64) -> CacheApiKey {
    CacheApiKey {
        id,
        api_key_hash: format!("hash-{id}"),
        key_prefix: "cyder-prefix".to_string(),
        key_last4: "1234".to_string(),
        name: format!("runtime-{id}"),
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
        println!("skipping redis api key runtime contract tests: CYDER_TEST_REDIS_URL is not set");
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

fn redis_store(pool: RedisPool, lease_ttl: Duration) -> RedisApiKeyRuntimeStore {
    RedisApiKeyRuntimeStore::new(
        pool,
        format!("runtime:test:{}:", Uuid::new_v4()),
        lease_ttl,
        TEST_REDIS_STATE_TTL,
    )
}

fn empty_baseline(now_ms: i64) -> ApiKeyRollupBaseline {
    ApiKeyRollupBaseline {
        day_bucket: day_bucket_start(now_ms),
        month_bucket: month_bucket_start(now_ms),
        ..ApiKeyRollupBaseline::default()
    }
}

async fn assert_concurrency_release_contract(store: Arc<dyn ApiKeyRuntimeStore>) {
    let api_key = CacheApiKey {
        max_concurrent_requests: Some(1),
        ..cache_api_key(42)
    };
    let now_ms = 1_744_000_000_000;
    let baseline = empty_baseline(now_ms);

    let lease = store
        .try_begin_request(&api_key, now_ms, &baseline)
        .await
        .expect("request should be admitted")
        .expect("concurrency limit should create a lease");

    let err = store
        .try_begin_request(&api_key, now_ms, &baseline)
        .await
        .expect_err("second request should be concurrency limited");
    assert_eq!(
        err,
        ApiKeyGovernanceAdmissionError::ConcurrencyLimited {
            limit: 1,
            current: 1,
        }
    );

    store
        .release_request_lease(&lease)
        .await
        .expect("first release should succeed");
    store
        .release_request_lease(&lease)
        .await
        .expect("second release should be a no-op");

    let snapshot = store
        .snapshot(api_key.id)
        .await
        .expect("snapshot after repeated release");
    assert_eq!(snapshot.current_concurrency, 0);
    assert_eq!(snapshot.daily_request_count, 1);

    let admitted_after_release = store
        .try_begin_request(&api_key, now_ms, &baseline)
        .await
        .expect("release should free the slot");
    assert!(admitted_after_release.is_some());
}

async fn assert_lease_expiry_contract(store: Arc<dyn ApiKeyRuntimeStore>) {
    let api_key = CacheApiKey {
        max_concurrent_requests: Some(1),
        ..cache_api_key(7)
    };
    let now_ms = 1_744_000_000_000;
    let baseline = empty_baseline(now_ms);

    let _lease = store
        .try_begin_request(&api_key, now_ms, &baseline)
        .await
        .expect("request should be admitted")
        .expect("concurrency limit should create a lease");
    assert_eq!(
        store
            .snapshot(api_key.id)
            .await
            .expect("snapshot after admission")
            .current_concurrency,
        1
    );

    tokio::time::sleep(Duration::from_millis(80)).await;

    let snapshot = store
        .snapshot(api_key.id)
        .await
        .expect("snapshot after ttl expiry");
    assert_eq!(snapshot.current_concurrency, 0);
    assert_eq!(snapshot.daily_request_count, 1);

    store
        .try_begin_request(&api_key, now_ms, &baseline)
        .await
        .expect("expired lease should no longer block admission");
}

async fn assert_rpm_and_daily_request_contract(store: Arc<dyn ApiKeyRuntimeStore>) {
    let now_ms = 1_744_000_000_000;
    let baseline = empty_baseline(now_ms);
    let rpm_limited_key = CacheApiKey {
        rate_limit_rpm: Some(1),
        ..cache_api_key(100)
    };

    store
        .try_begin_request(&rpm_limited_key, now_ms, &baseline)
        .await
        .expect("first request should be admitted");
    let err = store
        .try_begin_request(&rpm_limited_key, now_ms + 1, &baseline)
        .await
        .expect_err("second same-minute request should be rate limited");
    assert_eq!(
        err,
        ApiKeyGovernanceAdmissionError::RateLimited {
            limit: 1,
            current: 1,
        }
    );

    let daily_request_limited_key = CacheApiKey {
        quota_daily_requests: Some(1),
        ..cache_api_key(101)
    };
    store
        .try_begin_request(&daily_request_limited_key, now_ms, &baseline)
        .await
        .expect("first daily request should be admitted");
    let err = store
        .try_begin_request(&daily_request_limited_key, now_ms + 60_000, &baseline)
        .await
        .expect_err("second daily request should exhaust quota");
    assert_eq!(
        err,
        ApiKeyGovernanceAdmissionError::DailyRequestQuotaExceeded {
            limit: 1,
            current: 1,
        }
    );
}

async fn assert_token_budget_and_completion_contract(store: Arc<dyn ApiKeyRuntimeStore>) {
    let now_ms = 1_744_000_000_000;
    let baseline = empty_baseline(now_ms);
    let completion_key = cache_api_key(200);

    store
        .try_begin_request(&completion_key, now_ms, &baseline)
        .await
        .expect("request should be admitted");
    store
        .apply_completion(
            &ApiKeyCompletionDelta {
                api_key_id: completion_key.id,
                occurred_at: now_ms,
                total_tokens: 7,
                billed_amount_nanos: 11,
                billed_currency: Some("usd".to_string()),
            },
            &baseline,
        )
        .await
        .expect("completion should apply");

    let snapshot = store
        .snapshot(completion_key.id)
        .await
        .expect("snapshot should include completion");
    assert_eq!(snapshot.daily_request_count, 1);
    assert_eq!(snapshot.daily_token_count, 7);
    assert_eq!(snapshot.monthly_token_count, 7);
    assert_eq!(
        snapshot
            .daily_billed_amounts
            .iter()
            .find(|amount| amount.currency == "USD")
            .map(|amount| amount.amount_nanos),
        Some(11)
    );
    assert_eq!(
        snapshot
            .monthly_billed_amounts
            .iter()
            .find(|amount| amount.currency == "USD")
            .map(|amount| amount.amount_nanos),
        Some(11)
    );

    let daily_token_key = CacheApiKey {
        quota_daily_tokens: Some(10),
        ..cache_api_key(201)
    };
    store
        .apply_completion(
            &ApiKeyCompletionDelta {
                api_key_id: daily_token_key.id,
                occurred_at: now_ms,
                total_tokens: 10,
                ..ApiKeyCompletionDelta::default()
            },
            &baseline,
        )
        .await
        .expect("daily token baseline should apply");
    let err = store
        .try_begin_request(&daily_token_key, now_ms, &baseline)
        .await
        .expect_err("daily token quota should block admission");
    assert_eq!(
        err,
        ApiKeyGovernanceAdmissionError::DailyTokenQuotaExceeded {
            limit: 10,
            current: 10,
        }
    );

    let monthly_token_key = CacheApiKey {
        quota_monthly_tokens: Some(10),
        ..cache_api_key(202)
    };
    store
        .apply_completion(
            &ApiKeyCompletionDelta {
                api_key_id: monthly_token_key.id,
                occurred_at: now_ms,
                total_tokens: 10,
                ..ApiKeyCompletionDelta::default()
            },
            &baseline,
        )
        .await
        .expect("monthly token baseline should apply");
    let err = store
        .try_begin_request(&monthly_token_key, now_ms, &baseline)
        .await
        .expect_err("monthly token quota should block admission");
    assert_eq!(
        err,
        ApiKeyGovernanceAdmissionError::MonthlyTokenQuotaExceeded {
            limit: 10,
            current: 10,
        }
    );

    let daily_budget_key = CacheApiKey {
        budget_daily_nanos: Some(25),
        budget_daily_currency: Some("usd".to_string()),
        ..cache_api_key(203)
    };
    store
        .apply_completion(
            &ApiKeyCompletionDelta {
                api_key_id: daily_budget_key.id,
                occurred_at: now_ms,
                billed_amount_nanos: 25,
                billed_currency: Some("usd".to_string()),
                ..ApiKeyCompletionDelta::default()
            },
            &baseline,
        )
        .await
        .expect("daily budget usage should apply");
    let err = store
        .try_begin_request(&daily_budget_key, now_ms, &baseline)
        .await
        .expect_err("daily budget should block admission");
    assert_eq!(
        err,
        ApiKeyGovernanceAdmissionError::DailyBudgetExceeded {
            currency: "USD".to_string(),
            limit_nanos: 25,
            current_nanos: 25,
        }
    );

    let monthly_budget_key = CacheApiKey {
        budget_monthly_nanos: Some(30),
        budget_monthly_currency: Some("usd".to_string()),
        ..cache_api_key(204)
    };
    store
        .apply_completion(
            &ApiKeyCompletionDelta {
                api_key_id: monthly_budget_key.id,
                occurred_at: now_ms,
                billed_amount_nanos: 30,
                billed_currency: Some("usd".to_string()),
                ..ApiKeyCompletionDelta::default()
            },
            &baseline,
        )
        .await
        .expect("monthly budget usage should apply");
    let err = store
        .try_begin_request(&monthly_budget_key, now_ms, &baseline)
        .await
        .expect_err("monthly budget should block admission");
    assert_eq!(
        err,
        ApiKeyGovernanceAdmissionError::MonthlyBudgetExceeded {
            currency: "USD".to_string(),
            limit_nanos: 30,
            current_nanos: 30,
        }
    );
}

async fn assert_same_bucket_baseline_contract(store: Arc<dyn ApiKeyRuntimeStore>) {
    let api_key = cache_api_key(99);
    let now_ms = 1_744_000_000_000;
    let baseline = ApiKeyRollupBaseline {
        day_bucket: day_bucket_start(now_ms),
        daily_request_count: 10,
        daily_token_count: 20,
        daily_billed_amounts: HashMap::from([(String::from("USD"), 30)]),
        month_bucket: month_bucket_start(now_ms),
        monthly_token_count: 40,
        monthly_billed_amounts: HashMap::from([(String::from("USD"), 50)]),
    };

    store
        .try_begin_request(&api_key, now_ms, &baseline)
        .await
        .expect("request should be admitted");
    store
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
    store
        .try_begin_request(&api_key, now_ms + 1_000, &baseline)
        .await
        .expect("second request should be admitted");

    let snapshot = store
        .snapshot(api_key.id)
        .await
        .expect("snapshot after live updates");
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

#[tokio::test]
async fn memory_store_enforces_concurrency_and_release_idempotency() {
    assert_concurrency_release_contract(Arc::new(MemoryApiKeyRuntimeStore::default())).await;
}

#[tokio::test]
async fn memory_store_expired_request_lease_stops_counting_as_current_concurrency() {
    assert_lease_expiry_contract(Arc::new(MemoryApiKeyRuntimeStore::with_request_lease_ttl(
        Duration::from_millis(20),
    )))
    .await;
}

#[tokio::test]
async fn memory_store_enforces_rpm_and_daily_request_quota() {
    assert_rpm_and_daily_request_contract(Arc::new(MemoryApiKeyRuntimeStore::default())).await;
}

#[tokio::test]
async fn memory_store_enforces_token_budget_and_completion_contract() {
    assert_token_budget_and_completion_contract(Arc::new(MemoryApiKeyRuntimeStore::default()))
        .await;
}

#[tokio::test]
async fn memory_store_same_bucket_baseline_does_not_overwrite_live_counters() {
    assert_same_bucket_baseline_contract(Arc::new(MemoryApiKeyRuntimeStore::default())).await;
}

#[tokio::test]
async fn redis_store_enforces_concurrency_and_release_idempotency() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_concurrency_release_contract(Arc::new(redis_store(pool, Duration::from_secs(60)))).await;
}

#[tokio::test]
async fn redis_store_expired_request_lease_stops_counting_as_current_concurrency() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_lease_expiry_contract(Arc::new(redis_store(pool, Duration::from_millis(20)))).await;
}

#[tokio::test]
async fn redis_store_enforces_rpm_and_daily_request_quota() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_rpm_and_daily_request_contract(Arc::new(redis_store(pool, Duration::from_secs(60))))
        .await;
}

#[tokio::test]
async fn redis_store_enforces_token_budget_and_completion_contract() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_token_budget_and_completion_contract(Arc::new(redis_store(
        pool,
        Duration::from_secs(60),
    )))
    .await;
}

#[tokio::test]
async fn redis_store_same_bucket_baseline_does_not_overwrite_live_counters() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    assert_same_bucket_baseline_contract(Arc::new(redis_store(pool, Duration::from_secs(60))))
        .await;
}
