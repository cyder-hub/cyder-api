use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use super::{CacheBackend, CacheError, types::CacheEntry};

/// Type-erased cache repository trait for dynamic dispatch.
///
/// This trait allows `AppState` to hold cache repositories without knowing
/// the concrete backend type (Memory vs Redis), avoiding the need for an
/// enum wrapper with match arms on every operation.
#[async_trait]
pub trait DynCacheRepo<T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static>:
    Send + Sync
{
    async fn get(&self, key: &str) -> Result<Option<Arc<T>>, CacheError>;
    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<Arc<T>>>, CacheError>;
    async fn get_entry(&self, key: &str) -> Result<Option<Arc<CacheEntry<T>>>, CacheError>;
    async fn set_positive(&self, key: &str, value: &T) -> Result<(), CacheError>;
    async fn set_negative(&self, key: &str, ttl: Duration) -> Result<(), CacheError>;
    async fn delete(&self, key: &str) -> Result<(), CacheError>;
    async fn clear(&self) -> Result<(), CacheError>;
}

/// Simplified cache repository - just basic KV operations
/// No more CacheStorable trait with id(), key(), group_id()
/// Cache keys are managed by the caller (AppState)
pub struct CacheRepository<T, B>
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    B: CacheBackend<T>,
{
    backend: B,
    default_ttl: Option<Duration>,
    _phantom: PhantomData<T>,
}

impl<T, B> CacheRepository<T, B>
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    B: CacheBackend<T>,
{
    pub fn new(backend: B, default_ttl: Option<Duration>) -> Self {
        Self {
            backend,
            default_ttl,
            _phantom: PhantomData,
        }
    }

    /// Get the raw cache entry (Positive or Negative)
    pub async fn get_entry(&self, cache_key: &str) -> Result<Option<Arc<CacheEntry<T>>>, CacheError> {
        self.backend.get(cache_key).await
            .map_err(|e| CacheError::BackendError(e.to_string()))
    }
    
    /// Get item by complete cache key, unwrapping Positive entries.
    /// Returns Ok(None) for Negative entries or cache misses.
    pub async fn get(&self, cache_key: &str) -> Result<Option<Arc<T>>, CacheError> {
        match self.get_entry(cache_key).await? {
            Some(entry) => match &*entry {
                CacheEntry::Positive(value) => Ok(Some(value.clone())),
                CacheEntry::Negative => Ok(None),
            },
            None => Ok(None),
        }
    }
    
    /// Set a positive cache entry
    pub async fn set_positive(&self, cache_key: &str, value: &T) -> Result<(), CacheError> {
        let entry = Arc::new(CacheEntry::Positive(Arc::new(value.clone())));
        self.backend.set(cache_key, entry, self.default_ttl).await
            .map_err(|e| CacheError::BackendError(e.to_string()))
    }
    
    /// Set a negative cache entry with a specific TTL
    pub async fn set_negative(&self, cache_key: &str, ttl: Duration) -> Result<(), CacheError> {
        let entry = Arc::new(CacheEntry::Negative);
        self.backend.set(cache_key, entry, Some(ttl)).await
            .map_err(|e| CacheError::BackendError(e.to_string()))
    }
    
    /// Delete item by complete cache key
    pub async fn delete(&self, cache_key: &str) -> Result<(), CacheError> {
        self.backend.delete(cache_key).await
            .map_err(|e| CacheError::BackendError(e.to_string()))
    }

    /// Clear all items from the cache (respecting backend's prefix if any)
    pub async fn clear(&self) -> Result<(), CacheError> {
        self.backend.clear().await
            .map_err(|e| CacheError::BackendError(e.to_string()))
    }
    
    // Note: For list caches, use CacheRepository<Vec<ItemType>, Backend>
    // Then get() returns Arc<Vec<ItemType>> and set() takes &Vec<ItemType>
    
    /// Batch get by multiple complete cache keys. Unwraps Positive entries.
    pub async fn mget(&self, cache_keys: &[&str]) -> Result<Vec<Option<Arc<T>>>, CacheError> {
        let entries = self.backend.mget(cache_keys).await
            .map_err(|e| CacheError::BackendError(e.to_string()))?;
        
        let results = entries.into_iter().map(|entry_opt| {
            entry_opt.and_then(|entry| match &*entry {
                CacheEntry::Positive(value) => Some(value.clone()),
                CacheEntry::Negative => None,
            })
        }).collect();
        
        Ok(results)
    }
    
    /// Batch set positive entries with multiple complete cache keys
    pub async fn mset_positive(&self, entries: &[(&str, &T)]) -> Result<(), CacheError> {
        let arc_entries: Vec<(&str, Arc<CacheEntry<T>>)> = entries.iter()
            .map(|(key, value)| {
                let entry = Arc::new(CacheEntry::Positive(Arc::new((*value).clone())));
                (*key, entry)
            })
            .collect();
        
        self.backend.mset(&arc_entries, self.default_ttl).await
            .map_err(|e| CacheError::BackendError(e.to_string()))
    }
}

/// Implement DynCacheRepo for CacheRepository, enabling type-erased dispatch.
#[async_trait]
impl<T, B> DynCacheRepo<T> for CacheRepository<T, B>
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    B: CacheBackend<T>,
{
    async fn get(&self, key: &str) -> Result<Option<Arc<T>>, CacheError> {
        CacheRepository::get(self, key).await
    }

    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<Arc<T>>>, CacheError> {
        CacheRepository::mget(self, keys).await
    }

    async fn get_entry(&self, key: &str) -> Result<Option<Arc<CacheEntry<T>>>, CacheError> {
        CacheRepository::get_entry(self, key).await
    }

    async fn set_positive(&self, key: &str, value: &T) -> Result<(), CacheError> {
        CacheRepository::set_positive(self, key, value).await
    }

    async fn set_negative(&self, key: &str, ttl: Duration) -> Result<(), CacheError> {
        CacheRepository::set_negative(self, key, ttl).await
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        CacheRepository::delete(self, key).await
    }

    async fn clear(&self) -> Result<(), CacheError> {
        CacheRepository::clear(self).await
    }
}

impl<T, B> Clone for CacheRepository<T, B>
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    B: CacheBackend<T>,
{
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            default_ttl: self.default_ttl,
            _phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::cache::memory::MemoryCacheBackend;
    use serde::{Deserialize, Serialize};
    
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestItem {
        id: i64,
        name: String,
    }
    
    #[tokio::test]
    async fn test_get_set_positive() {
        let backend = MemoryCacheBackend::new();
        let repo = CacheRepository::new(backend, None);
        
        let item = TestItem {
            id: 1,
            name: "test_item".to_string(),
        };
        
        repo.set_positive("item:id:1", &item).await.unwrap();
        
        let result = repo.get("item:id:1").await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "test_item");
    }

    #[tokio::test]
    async fn test_get_set_negative() {
        let backend = MemoryCacheBackend::new();
        let repo: CacheRepository<TestItem, _> = CacheRepository::new(backend, None);

        repo.set_negative("item:id:2", Duration::from_secs(60)).await.unwrap();

        // get() should return None for a negative entry
        let result = repo.get("item:id:2").await.unwrap();
        assert!(result.is_none());

        // get_entry() should reveal the Negative entry
        let entry_result = repo.get_entry("item:id:2").await.unwrap();
        assert!(matches!(&*entry_result.unwrap(), &CacheEntry::Negative));
    }
    
    #[tokio::test]
    async fn test_multiple_dimensions() {
        let backend = MemoryCacheBackend::new();
        let repo = CacheRepository::new(backend, None);
        
        let item = TestItem {
            id: 1,
            name: "test_item".to_string(),
        };
        
        repo.set_positive("item:id:1", &item).await.unwrap();
        repo.set_positive("item:name:test_item", &item).await.unwrap();
        
        let by_id = repo.get("item:id:1").await.unwrap();
        let by_name = repo.get("item:name:test_item").await.unwrap();
        
        assert!(by_id.is_some());
        assert!(by_name.is_some());
        assert_eq!(by_id.unwrap().id, 1);
        assert_eq!(by_name.unwrap().id, 1);
    }
    
    #[tokio::test]
    async fn test_list_cache() {
        let backend = MemoryCacheBackend::new();
        let repo: CacheRepository<Vec<TestItem>, _> = CacheRepository::new(backend, None);
        
        let items = vec![
            TestItem { id: 1, name: "item1".to_string() },
            TestItem { id: 2, name: "item2".to_string() },
        ];
        
        repo.set_positive("items:group:10", &items).await.unwrap();
        
        let result = repo.get("items:group:10").await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 2);
    }
    
    #[tokio::test]
    async fn test_delete() {
        let backend = MemoryCacheBackend::new();
        let repo = CacheRepository::new(backend, None);
        
        let item = TestItem {
            id: 1,
            name: "test_item".to_string(),
        };
        
        repo.set_positive("item:id:1", &item).await.unwrap();
        assert!(repo.get("item:id:1").await.unwrap().is_some());
        
        repo.delete("item:id:1").await.unwrap();
        assert!(repo.get("item:id:1").await.unwrap().is_none());
    }
    
    #[tokio::test]
    async fn test_mget_mset() {
        let backend = MemoryCacheBackend::new();
        let repo: CacheRepository<TestItem, _> = CacheRepository::new(backend, None);
        
        let items = vec![
            TestItem { id: 1, name: "item1".to_string() },
            TestItem { id: 2, name: "item2".to_string() },
        ];
        
        let entries = vec![
            ("item:id:1", &items[0]),
            ("item:id:2", &items[1]),
        ];
        repo.mset_positive(&entries).await.unwrap();
        repo.set_negative("item:id:3", Duration::from_secs(60)).await.unwrap();
        
        let keys = vec!["item:id:1", "item:id:2", "item:id:3", "item:id:4"];
        let results = repo.mget(&keys).await.unwrap();
        
        assert_eq!(results.len(), 4);
        assert!(results[0].is_some());
        assert!(results[1].is_some());
        assert!(results[2].is_none()); // Negative entry
        assert!(results[3].is_none()); // Cache miss
    }
}
