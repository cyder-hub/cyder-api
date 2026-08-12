use super::{
    GroupItemSelectionStrategy, MemoryProviderKeyCursorStore, ProviderKeyCursorStore,
    ProviderKeySelector, RedisProviderKeyCursorStore,
};
use crate::config::SecretEncryptionConfig;
use crate::database::provider::{
    NewProvider, NewProviderApiKey, Provider, ProviderApiKeyRepository, ProviderApiKeySummary,
};
use crate::database::upstream_source::NewUpstreamSource;
use crate::database::{DbConnection, TestDbContext, get_connection};
use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};
use crate::service::catalog::CatalogService;
use crate::service::redis::RedisPool;
use crate::service::secret_encryption::{SecretDomain, SecretEncryptionService, SensitiveSecret};
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use diesel::connection::SimpleConnection;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const TEST_REDIS_STATE_TTL: Duration = Duration::from_secs(60);

fn seed_provider(id: i64) -> Provider {
    Provider::create(
        &NewProvider {
            id,
            provider_key: format!("provider-key-cursor-{id}"),
            name: format!("Provider Key Cursor {id}"),
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        },
        &NewUpstreamSource {
            id,
            provider_id: id,
            profile_type: UpstreamProfileType::Openai,
            base_url: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            is_default: true,
            created_at: 1,
            updated_at: 1,
            ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
        },
    )
    .expect("provider seed should succeed")
    .provider
}

fn seed_provider_api_key(
    id: i64,
    provider_id: i64,
    api_key: &str,
    created_at: i64,
) -> ProviderApiKeySummary {
    let config: SecretEncryptionConfig = serde_yaml::from_str(
        "downstream_mode: one_time\nencryption_key: '000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'\n",
    )
    .expect("test secret config should parse");
    let service = SecretEncryptionService::from_config(&config);
    let secret = SensitiveSecret::new(api_key.to_string());
    let encrypted_secret = service
        .encrypt_current(SecretDomain::ProviderApiKey(id), &secret)
        .expect("provider test secret should encrypt");
    let secret_hmac = service
        .provider_secret_fingerprint(provider_id, &secret)
        .expect("provider test secret should fingerprint");
    ProviderApiKeyRepository::insert(&NewProviderApiKey {
        id,
        provider_id,
        description: Some("provider key cursor contract".to_string()),
        key_prefix: api_key.chars().take(4).collect(),
        key_last4: api_key
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
        encrypted_secret,
        secret_hmac,
        is_enabled: true,
        created_at,
        updated_at: created_at,
    })
    .expect("provider api key seed should succeed")
}

async fn selected_api_key(selector: &ProviderKeySelector, provider_id: i64) -> Option<i64> {
    selector
        .get_one_provider_api_key_by_provider(provider_id, GroupItemSelectionStrategy::Queue)
        .await
        .expect("provider key selection should succeed")
        .map(|key| key.id)
}

async fn redis_pool_or_skip() -> Option<RedisPool> {
    let Ok(url) = env::var("CYDER_TEST_REDIS_URL") else {
        println!("skipping redis provider key cursor tests: CYDER_TEST_REDIS_URL is not set");
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

fn redis_cursor_store(pool: RedisPool, key_prefix: &str) -> RedisProviderKeyCursorStore {
    RedisProviderKeyCursorStore::new(pool, key_prefix.to_string(), TEST_REDIS_STATE_TTL)
}

#[tokio::test]
async fn memory_cursor_advances_wraps_and_handles_key_count_changes() {
    let store = MemoryProviderKeyCursorStore::default();

    assert_eq!(store.next_queue_index(42, 3).await.unwrap(), 0);
    assert_eq!(store.next_queue_index(42, 3).await.unwrap(), 1);
    assert_eq!(store.next_queue_index(42, 3).await.unwrap(), 2);
    assert_eq!(store.next_queue_index(42, 3).await.unwrap(), 0);

    assert_eq!(store.next_queue_index(7, 4).await.unwrap(), 0);
    assert_eq!(store.next_queue_index(7, 4).await.unwrap(), 1);
    assert_eq!(store.next_queue_index(7, 2).await.unwrap(), 0);
    assert_eq!(store.next_queue_index(7, 2).await.unwrap(), 1);
}

#[tokio::test]
async fn memory_cursor_reset_restarts_provider_from_zero() {
    let store = MemoryProviderKeyCursorStore::default();

    assert_eq!(store.next_queue_index(9, 3).await.unwrap(), 0);
    assert_eq!(store.next_queue_index(9, 3).await.unwrap(), 1);

    store.reset_provider_cursor(9).await.unwrap();

    assert_eq!(store.next_queue_index(9, 3).await.unwrap(), 0);
}

#[tokio::test]
async fn selectors_sharing_memory_cursor_rotate_provider_keys_globally() {
    let test_db_context = TestDbContext::new_sqlite("provider-key-cursor-shared-selector.sqlite");

    test_db_context
        .run_async(async {
            let provider = seed_provider(91_001);
            seed_provider_api_key(91_101, provider.id, "sk-one", 1);
            seed_provider_api_key(91_102, provider.id, "sk-two", 2);
            seed_provider_api_key(91_103, provider.id, "sk-three", 3);

            let catalog = Arc::new(CatalogService::new(true).await);
            let cursor_store: Arc<dyn ProviderKeyCursorStore> =
                Arc::new(MemoryProviderKeyCursorStore::default());
            let selector_a =
                ProviderKeySelector::new(Arc::clone(&catalog), Arc::clone(&cursor_store)).await;
            let selector_b =
                ProviderKeySelector::new(Arc::clone(&catalog), Arc::clone(&cursor_store)).await;

            let provider_keys = catalog
                .get_provider_api_keys(provider.id)
                .await
                .expect("provider keys should load");
            let expected = provider_keys.iter().map(|key| key.id).collect::<Vec<_>>();

            assert_eq!(
                vec![
                    selected_api_key(&selector_a, provider.id).await.unwrap(),
                    selected_api_key(&selector_b, provider.id).await.unwrap(),
                    selected_api_key(&selector_a, provider.id).await.unwrap(),
                    selected_api_key(&selector_b, provider.id).await.unwrap(),
                ],
                vec![
                    expected[0].clone(),
                    expected[1].clone(),
                    expected[2].clone(),
                    expected[0].clone(),
                ]
            );
        })
        .await;
}

#[tokio::test]
async fn provider_api_key_invalidation_resets_memory_cursor() {
    let test_db_context =
        TestDbContext::new_sqlite("provider-key-cursor-invalidation-reset.sqlite");

    test_db_context
        .run_async(async {
            let provider = seed_provider(92_001);
            seed_provider_api_key(92_101, provider.id, "sk-one", 1);
            seed_provider_api_key(92_102, provider.id, "sk-two", 2);
            seed_provider_api_key(92_103, provider.id, "sk-three", 3);

            let catalog = Arc::new(CatalogService::new(true).await);
            let selector = ProviderKeySelector::new_memory(Arc::clone(&catalog)).await;
            let provider_keys = catalog
                .get_provider_api_keys(provider.id)
                .await
                .expect("provider keys should load");
            let first_key = provider_keys
                .first()
                .expect("provider key list should not be empty")
                .id;

            assert_eq!(
                selected_api_key(&selector, provider.id).await,
                Some(first_key)
            );
            assert_ne!(
                selected_api_key(&selector, provider.id).await,
                Some(first_key)
            );

            catalog
                .invalidate_provider_api_keys(provider.id)
                .await
                .expect("provider key invalidation should succeed");

            assert_eq!(
                selected_api_key(&selector, provider.id).await,
                Some(first_key)
            );
        })
        .await;
}

#[tokio::test]
async fn redis_cursor_advances_wraps_and_handles_key_count_changes() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    let prefix = format!("runtime:test:{}:", Uuid::new_v4());
    let store = redis_cursor_store(pool, &prefix);

    assert_eq!(store.next_queue_index(93_001, 3).await.unwrap(), 0);
    assert_eq!(store.next_queue_index(93_001, 3).await.unwrap(), 1);
    assert_eq!(store.next_queue_index(93_001, 3).await.unwrap(), 2);
    assert_eq!(store.next_queue_index(93_001, 3).await.unwrap(), 0);

    assert_eq!(store.next_queue_index(93_002, 4).await.unwrap(), 0);
    assert_eq!(store.next_queue_index(93_002, 4).await.unwrap(), 1);
    assert_eq!(store.next_queue_index(93_002, 2).await.unwrap(), 0);
    assert_eq!(store.next_queue_index(93_002, 2).await.unwrap(), 1);
}

#[tokio::test]
async fn redis_cursor_reset_restarts_provider_from_zero() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    let prefix = format!("runtime:test:{}:", Uuid::new_v4());
    let store = redis_cursor_store(pool, &prefix);

    assert_eq!(store.next_queue_index(94_001, 3).await.unwrap(), 0);
    assert_eq!(store.next_queue_index(94_001, 3).await.unwrap(), 1);

    store.reset_provider_cursor(94_001).await.unwrap();

    assert_eq!(store.next_queue_index(94_001, 3).await.unwrap(), 0);
}

#[tokio::test]
async fn selectors_sharing_redis_cursor_rotate_provider_keys_globally() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    let prefix = format!("runtime:test:{}:", Uuid::new_v4());
    let test_db_context = TestDbContext::new_sqlite("provider-key-redis-cursor-shared.sqlite");

    test_db_context
        .run_async(async {
            let provider = seed_provider(95_001);
            seed_provider_api_key(95_101, provider.id, "sk-one", 1);
            seed_provider_api_key(95_102, provider.id, "sk-two", 2);
            seed_provider_api_key(95_103, provider.id, "sk-three", 3);

            let catalog = Arc::new(CatalogService::new(true).await);
            let cursor_store: Arc<dyn ProviderKeyCursorStore> =
                Arc::new(redis_cursor_store(pool.clone(), &prefix));
            let selector_a =
                ProviderKeySelector::new(Arc::clone(&catalog), Arc::clone(&cursor_store)).await;
            let selector_b =
                ProviderKeySelector::new(Arc::clone(&catalog), Arc::clone(&cursor_store)).await;

            let provider_keys = catalog
                .get_provider_api_keys(provider.id)
                .await
                .expect("provider keys should load");
            let expected = provider_keys.iter().map(|key| key.id).collect::<Vec<_>>();

            assert_eq!(
                vec![
                    selected_api_key(&selector_a, provider.id).await.unwrap(),
                    selected_api_key(&selector_b, provider.id).await.unwrap(),
                    selected_api_key(&selector_a, provider.id).await.unwrap(),
                    selected_api_key(&selector_b, provider.id).await.unwrap(),
                ],
                vec![
                    expected[0].clone(),
                    expected[1].clone(),
                    expected[2].clone(),
                    expected[0].clone(),
                ]
            );
        })
        .await;
}

#[tokio::test]
async fn provider_api_key_invalidation_resets_redis_cursor_for_all_selectors() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    let prefix = format!("runtime:test:{}:", Uuid::new_v4());
    let test_db_context =
        TestDbContext::new_sqlite("provider-key-redis-cursor-invalidation.sqlite");

    test_db_context
        .run_async(async {
            let provider = seed_provider(96_001);
            seed_provider_api_key(96_101, provider.id, "sk-one", 1);
            seed_provider_api_key(96_102, provider.id, "sk-two", 2);
            seed_provider_api_key(96_103, provider.id, "sk-three", 3);

            let catalog = Arc::new(CatalogService::new(true).await);
            let cursor_store: Arc<dyn ProviderKeyCursorStore> =
                Arc::new(redis_cursor_store(pool.clone(), &prefix));
            let selector_a =
                ProviderKeySelector::new(Arc::clone(&catalog), Arc::clone(&cursor_store)).await;
            let selector_b =
                ProviderKeySelector::new(Arc::clone(&catalog), Arc::clone(&cursor_store)).await;
            let provider_keys = catalog
                .get_provider_api_keys(provider.id)
                .await
                .expect("provider keys should load");
            let first_key = provider_keys
                .first()
                .expect("provider key list should not be empty")
                .id;

            assert_eq!(
                selected_api_key(&selector_a, provider.id).await,
                Some(first_key)
            );
            assert_ne!(
                selected_api_key(&selector_b, provider.id).await,
                Some(first_key)
            );

            catalog
                .invalidate_provider_api_keys(provider.id)
                .await
                .expect("provider key invalidation should succeed");

            assert_eq!(
                selected_api_key(&selector_a, provider.id).await,
                Some(first_key)
            );
        })
        .await;
}

#[tokio::test]
async fn redis_cursor_new_selector_extends_previous_queue_position() {
    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    let prefix = format!("runtime:test:{}:", Uuid::new_v4());
    let test_db_context = TestDbContext::new_sqlite("provider-key-redis-cursor-restart.sqlite");

    test_db_context
        .run_async(async {
            let provider = seed_provider(97_001);
            seed_provider_api_key(97_101, provider.id, "sk-one", 1);
            seed_provider_api_key(97_102, provider.id, "sk-two", 2);
            seed_provider_api_key(97_103, provider.id, "sk-three", 3);

            let catalog = Arc::new(CatalogService::new(true).await);
            let selector_a = ProviderKeySelector::new(
                Arc::clone(&catalog),
                Arc::new(redis_cursor_store(pool.clone(), &prefix))
                    as Arc<dyn ProviderKeyCursorStore>,
            )
            .await;
            let first = selected_api_key(&selector_a, provider.id)
                .await
                .expect("first selection should return a key");
            let second = selected_api_key(&selector_a, provider.id)
                .await
                .expect("second selection should return a key");
            assert_ne!(first, second);

            let restarted_selector = ProviderKeySelector::new(
                Arc::clone(&catalog),
                Arc::new(redis_cursor_store(pool, &prefix)) as Arc<dyn ProviderKeyCursorStore>,
            )
            .await;
            let third = selected_api_key(&restarted_selector, provider.id)
                .await
                .expect("third selection should return a key");

            let provider_keys = catalog
                .get_provider_api_keys(provider.id)
                .await
                .expect("provider keys should load");
            assert_eq!(third, provider_keys[2].id);
        })
        .await;
}

#[tokio::test]
async fn database_reload_failure_leaves_provider_selection_fail_closed() {
    let test_db_context = TestDbContext::new_sqlite("provider-key-fail-closed.sqlite");
    test_db_context
        .run_async(async {
            let provider = seed_provider(98_001);
            seed_provider_api_key(98_101, provider.id, "sk-before-failure", 1);
            let catalog = Arc::new(CatalogService::new(true).await);
            let selector = ProviderKeySelector::new_memory(Arc::clone(&catalog)).await;
            assert_eq!(selected_api_key(&selector, provider.id).await, Some(98_101));

            {
                let mut connection = get_connection().expect("test connection should load");
                match &mut connection {
                    DbConnection::Sqlite(connection) => connection
                        .batch_execute("DROP TABLE provider_api_key;")
                        .expect("provider key table should drop for failure injection"),
                    DbConnection::Postgres(_) => unreachable!("test uses sqlite"),
                }
            }

            assert!(
                catalog
                    .invalidate_provider_api_keys(provider.id)
                    .await
                    .is_err()
            );
            let error = selector
                .get_one_provider_api_key_by_provider(
                    provider.id,
                    GroupItemSelectionStrategy::Queue,
                )
                .await
                .expect_err("provider must remain fail-closed after DB reload failure");
            assert!(error.to_string().contains("fail-closed"));
        })
        .await;
}

#[tokio::test]
async fn redis_provider_key_snapshot_round_trips_only_encrypted_material() {
    use crate::service::cache::redis::RedisCacheBackend;
    use crate::service::cache::repository::CacheRepository;
    use crate::service::cache::types::CacheProviderKey;

    let Some(pool) = redis_pool_or_skip().await else {
        return;
    };
    let prefix = format!("catalog:test:{}:", Uuid::new_v4());
    let repository = CacheRepository::new(
        RedisCacheBackend::new(pool, prefix),
        Some(TEST_REDIS_STATE_TTL),
    );
    let snapshot = vec![CacheProviderKey {
        id: 99_101,
        provider_id: 99_001,
        secret_ciphertext: vec![1, 2, 3, 4],
        secret_nonce: vec![0; 24],
        secret_format_version: 1,
        secret_key_fingerprint: "a".repeat(64),
    }];
    repository
        .set_positive("provider-keys", &snapshot)
        .await
        .expect("encrypted provider snapshot should write to Redis");
    let loaded = repository
        .get("provider-keys")
        .await
        .expect("encrypted provider snapshot should read from Redis")
        .expect("encrypted provider snapshot should exist");
    assert_eq!(loaded[0].id, snapshot[0].id);
    assert_eq!(loaded[0].secret_ciphertext, snapshot[0].secret_ciphertext);
    let serialized = serde_json::to_value(loaded.as_ref()).expect("snapshot should serialize");
    let text = serialized.to_string();
    assert!(!text.contains("api_key"));
    assert!(!text.contains("secret_hmac"));
    assert!(!format!("{:?}", loaded[0]).contains(&snapshot[0].secret_key_fingerprint));
    repository
        .delete("provider-keys")
        .await
        .expect("encrypted provider snapshot should delete from Redis");
}
