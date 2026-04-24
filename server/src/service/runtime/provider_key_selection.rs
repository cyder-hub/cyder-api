use rand::{Rng, rng};
use std::collections::HashMap;
use std::sync::{Arc, Weak};

use crate::schema::enum_def::ProviderApiKeyMode;
use crate::service::app_state::AppStoreError;
use crate::service::cache::types::CacheProviderKey;
use crate::service::catalog::CatalogService;

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

#[derive(Default)]
struct QueueCursorStore {
    inner: tokio::sync::Mutex<HashMap<i64, usize>>,
}

impl QueueCursorStore {
    async fn next_queue_index(&self, provider_id: i64, key_count: usize) -> usize {
        let mut state = self.inner.lock().await;
        Self::advance_queue_cursor(&mut state, provider_id, key_count)
    }

    async fn reset_provider_cursor(&self, provider_id: i64) {
        self.inner.lock().await.remove(&provider_id);
    }

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

pub struct ProviderKeySelector {
    catalog: Arc<CatalogService>,
    queue_cursor_store: QueueCursorStore,
}

impl ProviderKeySelector {
    pub async fn new(catalog: Arc<CatalogService>) -> Arc<Self> {
        let selector = Arc::new(Self {
            catalog,
            queue_cursor_store: QueueCursorStore::default(),
        });
        selector.install_invalidation_hook().await;
        selector
    }

    async fn install_invalidation_hook(self: &Arc<Self>) {
        let selector: Weak<Self> = Arc::downgrade(self);
        self.catalog
            .set_provider_api_keys_invalidation_hook(Arc::new(move |provider_id| {
                let selector = selector.upgrade();
                Box::pin(async move {
                    if let Some(selector) = selector {
                        selector.reset_provider_cursor(provider_id).await;
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
                        .await;
                    Ok(keys.get(index).cloned().map(Arc::new))
                }
                GroupItemSelectionStrategy::Random => {
                    let index = Self::random_index(keys.len());
                    Ok(keys.get(index).cloned().map(Arc::new))
                }
            },
        }
    }

    pub async fn reset_provider_cursor(&self, provider_id: i64) {
        self.queue_cursor_store
            .reset_provider_cursor(provider_id)
            .await;
    }

    fn random_index(key_count: usize) -> usize {
        rng().random_range(0..key_count)
    }
}

#[cfg(test)]
mod tests {
    use super::{GroupItemSelectionStrategy, QueueCursorStore};
    use crate::schema::enum_def::ProviderApiKeyMode;
    use crate::service::catalog::CatalogService;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn queue_strategy_advances_and_wraps() {
        let mut state = HashMap::new();

        assert_eq!(QueueCursorStore::advance_queue_cursor(&mut state, 42, 3), 0);
        assert_eq!(QueueCursorStore::advance_queue_cursor(&mut state, 42, 3), 1);
        assert_eq!(QueueCursorStore::advance_queue_cursor(&mut state, 42, 3), 2);
        assert_eq!(QueueCursorStore::advance_queue_cursor(&mut state, 42, 3), 0);
    }

    #[test]
    fn queue_strategy_handles_key_count_changes() {
        let mut state = HashMap::new();

        assert_eq!(QueueCursorStore::advance_queue_cursor(&mut state, 7, 4), 0);
        assert_eq!(QueueCursorStore::advance_queue_cursor(&mut state, 7, 4), 1);
        assert_eq!(QueueCursorStore::advance_queue_cursor(&mut state, 7, 2), 0);
        assert_eq!(QueueCursorStore::advance_queue_cursor(&mut state, 7, 2), 1);
    }

    #[tokio::test]
    async fn resetting_provider_cursor_restarts_queue_from_zero() {
        let store = QueueCursorStore::default();

        assert_eq!(store.next_queue_index(9, 3).await, 0);
        assert_eq!(store.next_queue_index(9, 3).await, 1);

        store.reset_provider_cursor(9).await;

        assert_eq!(store.next_queue_index(9, 3).await, 0);
    }

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
        let selector = super::ProviderKeySelector::new(Arc::clone(&catalog)).await;
        let weak_selector = Arc::downgrade(&selector);

        assert_eq!(Arc::strong_count(&selector), 1);
        assert_eq!(Arc::strong_count(&catalog), 2);

        drop(selector);

        assert!(weak_selector.upgrade().is_none());
        assert_eq!(Arc::strong_count(&catalog), 1);
    }
}
