use async_trait::async_trait;
use cyder_tools::log::warn;
use rand::{Rng, rng};
use std::collections::HashMap;
use std::sync::{Arc, Weak};

use crate::schema::enum_def::ProviderApiKeyMode;
use crate::service::app_state::AppStoreError;
use crate::service::cache::types::CacheProviderKey;
use crate::service::catalog::CatalogService;

mod redis_cursor_store;

pub use redis_cursor_store::RedisProviderKeyCursorStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupItemSelectionStrategy {
    Random,
    Queue,
}

impl From<ProviderApiKeyMode> for GroupItemSelectionStrategy {
    fn from(value: ProviderApiKeyMode) -> Self {
        match value {
            ProviderApiKeyMode::Queue => GroupItemSelectionStrategy::Queue,
            ProviderApiKeyMode::Random => GroupItemSelectionStrategy::Random,
        }
    }
}

#[async_trait]
pub trait ProviderKeyCursorStore: Send + Sync {
    async fn next_queue_index(
        &self,
        provider_id: i64,
        key_count: usize,
    ) -> Result<usize, AppStoreError>;

    async fn reset_provider_cursor(&self, provider_id: i64) -> Result<(), AppStoreError>;
}

/// Single-instance default and dev/test backend; not a multi-instance correctness backend.
#[derive(Default)]
pub struct MemoryProviderKeyCursorStore {
    inner: tokio::sync::Mutex<HashMap<i64, usize>>,
}

impl MemoryProviderKeyCursorStore {
    fn advance_queue_cursor(
        state: &mut HashMap<i64, usize>,
        provider_id: i64,
        key_count: usize,
    ) -> usize {
        let next_slot = state.entry(provider_id).or_insert(0);
        let selected_index = *next_slot % key_count;
        *next_slot = (selected_index + 1) % key_count;
        selected_index
    }
}

#[async_trait]
impl ProviderKeyCursorStore for MemoryProviderKeyCursorStore {
    async fn next_queue_index(
        &self,
        provider_id: i64,
        key_count: usize,
    ) -> Result<usize, AppStoreError> {
        if key_count == 0 {
            return Err(AppStoreError::CacheError(
                "provider key cursor requires at least one key".to_string(),
            ));
        }

        let mut state = self.inner.lock().await;
        Ok(Self::advance_queue_cursor(
            &mut state,
            provider_id,
            key_count,
        ))
    }

    async fn reset_provider_cursor(&self, provider_id: i64) -> Result<(), AppStoreError> {
        self.inner.lock().await.remove(&provider_id);
        Ok(())
    }
}

pub struct ProviderKeySelector {
    catalog: Arc<CatalogService>,
    queue_cursor_store: Arc<dyn ProviderKeyCursorStore>,
}

impl ProviderKeySelector {
    pub async fn new(
        catalog: Arc<CatalogService>,
        queue_cursor_store: Arc<dyn ProviderKeyCursorStore>,
    ) -> Arc<Self> {
        let selector = Arc::new(Self {
            catalog,
            queue_cursor_store,
        });
        selector.install_invalidation_hook().await;
        selector
    }

    pub async fn new_memory(catalog: Arc<CatalogService>) -> Arc<Self> {
        Self::new(
            catalog,
            Arc::new(MemoryProviderKeyCursorStore::default()) as Arc<dyn ProviderKeyCursorStore>,
        )
        .await
    }

    async fn install_invalidation_hook(self: &Arc<Self>) {
        let selector: Weak<Self> = Arc::downgrade(self);
        self.catalog
            .set_provider_api_keys_invalidation_hook(Arc::new(move |provider_id| {
                let selector = selector.upgrade();
                Box::pin(async move {
                    if let Some(selector) = selector {
                        if let Err(err) = selector.reset_provider_cursor(provider_id).await {
                            warn!(
                                "failed to reset provider key cursor after provider api key invalidation: provider_id={}, error={}",
                                provider_id, err
                            );
                        }
                    }
                })
            }))
            .await;
    }

    pub async fn get_one_provider_api_key_by_provider(
        &self,
        provider_id: i64,
        strategy: GroupItemSelectionStrategy,
    ) -> Result<Option<Arc<CacheProviderKey>>, AppStoreError> {
        let keys = self.catalog.get_provider_api_keys(provider_id).await?;

        match keys.len() {
            0 => Ok(None),
            1 => Ok(keys.first().cloned().map(Arc::new)),
            _ => match strategy {
                GroupItemSelectionStrategy::Queue => {
                    let index = self
                        .queue_cursor_store
                        .next_queue_index(provider_id, keys.len())
                        .await?;
                    Ok(keys.get(index).cloned().map(Arc::new))
                }
                GroupItemSelectionStrategy::Random => {
                    let index = Self::random_index(keys.len());
                    Ok(keys.get(index).cloned().map(Arc::new))
                }
            },
        }
    }

    pub async fn reset_provider_cursor(&self, provider_id: i64) -> Result<(), AppStoreError> {
        self.queue_cursor_store
            .reset_provider_cursor(provider_id)
            .await
    }

    fn random_index(key_count: usize) -> usize {
        rng().random_range(0..key_count)
    }
}

#[cfg(test)]
mod contract_tests;

#[cfg(test)]
mod tests {
    use super::GroupItemSelectionStrategy;
    use crate::schema::enum_def::ProviderApiKeyMode;
    use crate::service::catalog::CatalogService;
    use std::sync::Arc;

    #[test]
    fn provider_api_key_mode_maps_to_runtime_strategy() {
        assert_eq!(
            GroupItemSelectionStrategy::from(ProviderApiKeyMode::Queue),
            GroupItemSelectionStrategy::Queue
        );
        assert_eq!(
            GroupItemSelectionStrategy::from(ProviderApiKeyMode::Random),
            GroupItemSelectionStrategy::Random
        );
    }

    #[tokio::test]
    async fn invalidation_hook_does_not_retain_selector() {
        let catalog = Arc::new(CatalogService::new(true).await);
        let selector = super::ProviderKeySelector::new_memory(Arc::clone(&catalog)).await;
        let weak_selector = Arc::downgrade(&selector);

        assert_eq!(Arc::strong_count(&selector), 1);
        assert_eq!(Arc::strong_count(&catalog), 2);

        drop(selector);

        assert!(weak_selector.upgrade().is_none());
        assert_eq!(Arc::strong_count(&catalog), 1);
    }
}
