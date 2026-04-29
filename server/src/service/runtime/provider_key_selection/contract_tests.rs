use super::{
    GroupItemSelectionStrategy, MemoryProviderKeyCursorStore, ProviderKeyCursorStore,
    ProviderKeySelector, RedisProviderKeyCursorStore,
};
use crate::database::TestDbContext;
use crate::database::provider::{NewProvider, NewProviderApiKey, Provider, ProviderApiKey};
use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
use crate::service::catalog::CatalogService;
use crate::service::redis::RedisPool;
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const TEST_REDIS_STATE_TTL: Duration = Duration::from_secs(60);

fn seed_provider(id: i64) -> Provider {
    Provider::create(&NewProvider {
        id,
        provider_key: format!("provider-key-cursor-{id}"),
        name: format!("Provider Key Cursor {id}"),
        endpoint: "https://api.example.com/v1".to_string(),
        use_proxy: false,
        is_enabled: true,
        created_at: 1,
        updated_at: 1,
        provider_type: ProviderType::Openai,
        provider_api_key_mode: ProviderApiKeyMode::Queue,
    })
    .expect("provider seed should succeed")
}

fn seed_provider_api_key(
    id: i64,
    provider_id: i64,
    api_key: &str,
    created_at: i64,
) -> ProviderApiKey {
    ProviderApiKey::insert(&NewProviderApiKey {
        id,
        provider_id,
        api_key: api_key.to_string(),
        description: Some("provider key cursor contract".to_string()),
        is_enabled: true,
        created_at,
        updated_at: created_at,
    })
    .expect("provider api key seed should succeed")
}

async fn selected_api_key(selector: &ProviderKeySelector, provider_id: i64) -> Option<String> {
    selector
        .get_one_provider_api_key_by_provider(provider_id, GroupItemSelectionStrategy::Queue)
        .await
        .expect("provider key selection should succeed")
        .map(|key| key.api_key.clone())
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
            let expected = provider_keys
                .iter()
                .map(|key| key.api_key.clone())
                .collect::<Vec<_>>();

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
                .api_key
                .clone();

            assert_eq!(
                selected_api_key(&selector, provider.id).await.as_deref(),
                Some(first_key.as_str())
            );
            assert_ne!(
                selected_api_key(&selector, provider.id).await.as_deref(),
                Some(first_key.as_str())
            );

            catalog
                .invalidate_provider_api_keys(provider.id)
                .await
                .expect("provider key invalidation should succeed");

            assert_eq!(
                selected_api_key(&selector, provider.id).await.as_deref(),
                Some(first_key.as_str())
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
            let expected = provider_keys
                .iter()
                .map(|key| key.api_key.clone())
                .collect::<Vec<_>>();

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
                .api_key
                .clone();

            assert_eq!(
                selected_api_key(&selector_a, provider.id).await.as_deref(),
                Some(first_key.as_str())
            );
            assert_ne!(
                selected_api_key(&selector_b, provider.id).await.as_deref(),
                Some(first_key.as_str())
            );

            catalog
                .invalidate_provider_api_keys(provider.id)
                .await
                .expect("provider key invalidation should succeed");

            assert_eq!(
                selected_api_key(&selector_a, provider.id).await.as_deref(),
                Some(first_key.as_str())
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
            assert_eq!(third, provider_keys[2].api_key);
        })
        .await;
}
