use async_trait::async_trait;
use dashmap::DashMap;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

use super::{CacheBackend, metrics::CacheMetrics, types::CacheEntry};

#[derive(Debug, Error)]
#[error("Memory cache error: {0}")]
pub struct MemoryCacheError(String);

#[derive(Clone)]
pub struct MemoryCacheBackend<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    data: Arc<DashMap<String, (Arc<CacheEntry<T>>, Option<Instant>)>>, // value + expiration
    metrics: Arc<CacheMetrics>,
}

impl<T> MemoryCacheBackend<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    pub fn new() -> Self {
        let backend = Self {
            data: Arc::new(DashMap::new()),
            metrics: Arc::new(CacheMetrics::new()),
        };
        
        // Spawn cleanup task
        backend.clone().spawn_cleanup_task();
        
        backend
    }
    
    #[allow(dead_code)]
    pub fn with_capacity(capacity: usize) -> Self {
        let backend = Self {
            data: Arc::new(DashMap::with_capacity(capacity)),
            metrics: Arc::new(CacheMetrics::new()),
        };
        
        backend.clone().spawn_cleanup_task();
        
        backend
    }
    
    pub fn metrics(&self) -> &CacheMetrics {
        &self.metrics
    }
    
    fn spawn_cleanup_task(self) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                self.cleanup_expired();
            }
        });
    }
    
    fn cleanup_expired(&self) {
        let now = Instant::now();
        let mut removed_count = 0;
        
        self.data.retain(|_, (_, expiration)| {
            if let Some(exp) = expiration {
                if now >= *exp {
                    removed_count += 1;
                    return false;
                }
            }
            true
        });
        
        if removed_count > 0 {
            cyder_tools::log::debug!("Cleaned up {} expired cache entries", removed_count);
        }
    }
    
    fn is_expired(&self, expiration: Option<Instant>) -> bool {
        if let Some(exp) = expiration {
            Instant::now() >= exp
        } else {
            false
        }
    }
}

#[async_trait]
impl<T> CacheBackend<T> for MemoryCacheBackend<T> 
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    type Error = MemoryCacheError;
    
    async fn get(&self, key: &str) -> Result<Option<Arc<CacheEntry<T>>>, Self::Error> {
        if let Some(entry) = self.data.get(key) {
            let (value, expiration) = entry.value();
            
            if self.is_expired(*expiration) {
                drop(entry);
                self.data.remove(key);
                self.metrics.record_miss();
                return Ok(None);
            }
            
            self.metrics.record_hit();
            Ok(Some(value.clone()))
        } else {
            self.metrics.record_miss();
            Ok(None)
        }
    }
    
    async fn set(&self, key: &str, value: Arc<CacheEntry<T>>, ttl: Option<Duration>) -> Result<(), Self::Error> {
        let expiration = ttl.map(|d| Instant::now() + d);
        self.data.insert(key.to_string(), (value, expiration));
        self.metrics.record_set();
        Ok(())
    }
    
    async fn delete(&self, key: &str) -> Result<(), Self::Error> {
        self.data.remove(key);
        self.metrics.record_delete();
        Ok(())
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        self.data.clear();
        cyder_tools::log::info!("In-memory cache cleared.");
        Ok(())
    }
    
    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<Arc<CacheEntry<T>>>>, Self::Error> {
        let mut results = Vec::with_capacity(keys.len());
        
        for key in keys {
            results.push(self.get(key).await?);
        }
        
        Ok(results)
    }
    
    async fn mset(&self, entries: &[(&str, Arc<CacheEntry<T>>)], ttl: Option<Duration>) -> Result<(), Self::Error> {
        let expiration = ttl.map(|d| Instant::now() + d);
        
        for (key, value) in entries {
            self.data.insert(key.to_string(), (value.clone(), expiration));
            self.metrics.record_set();
        }
        
        Ok(())
    }
}

impl<T> Default for MemoryCacheBackend<T> 
where T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static 
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_basic_get_set() {
        let cache = MemoryCacheBackend::<String>::new();
        let value = Arc::new(CacheEntry::Positive(Arc::new("test_value".to_string())));
        cache.set("key1", value.clone(), None).await.unwrap();
        
        let result = cache.get("key1").await.unwrap();
        
        assert!(matches!(*result.unwrap(), CacheEntry::Positive(_)));
    }
    
    #[tokio::test]
    async fn test_negative_cache_set() {
        let cache = MemoryCacheBackend::<String>::new();
        let value = Arc::new(CacheEntry::Negative);
        cache.set("key1", value.clone(), None).await.unwrap();
        
        let result = cache.get("key1").await.unwrap();
        
        assert!(matches!(*result.unwrap(), CacheEntry::Negative));
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let cache = MemoryCacheBackend::<String>::new();
        
        let result = cache.get("nonexistent").await.unwrap();
        assert_eq!(result, None);
    }
    
    #[tokio::test]
    async fn test_delete() {
        let cache = MemoryCacheBackend::<String>::new();
        let value = Arc::new(CacheEntry::Positive(Arc::new("test_value".to_string())));
        cache.set("key1", value, None).await.unwrap();
        
        cache.delete("key1").await.unwrap();
        
        let result = cache.get("key1").await.unwrap();
        assert_eq!(result, None);
    }
    
    #[tokio::test]
    async fn test_ttl_expiration() {
        let cache = MemoryCacheBackend::<String>::new();
        
        let value = Arc::new(CacheEntry::Positive(Arc::new("test_value".to_string())));
        cache.set("key1", value, Some(Duration::from_millis(100))).await.unwrap();
        
        // Should exist immediately
        let result = cache.get("key1").await.unwrap();
        assert!(result.is_some());
        
        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Should be expired
        let result = cache.get("key1").await.unwrap();
        assert_eq!(result, None);
    }
    
    #[tokio::test]
    async fn test_mget_mset() {
        let cache = MemoryCacheBackend::<String>::new();
        
        let entries = vec![
            ("key1", Arc::new(CacheEntry::Positive(Arc::new("value1".to_string())))),
            ("key2", Arc::new(CacheEntry::Positive(Arc::new("value2".to_string())))),
            ("key3", Arc::new(CacheEntry::Negative)),
        ];
        
        cache.mset(&entries, None).await.unwrap();
        
        let keys = vec!["key1", "key2", "key3", "key4"];
        let results = cache.mget(&keys).await.unwrap();
        
        assert!(matches!(**results[0].as_ref().unwrap(), CacheEntry::Positive(_)));
        assert!(matches!(**results[1].as_ref().unwrap(), CacheEntry::Positive(_)));
        assert!(matches!(**results[2].as_ref().unwrap(), CacheEntry::Negative));
        assert!(results[3].is_none());
    }
    
    #[tokio::test]
    async fn test_metrics() {
        let cache = MemoryCacheBackend::<String>::new();
        
        let value = Arc::new(CacheEntry::Positive(Arc::new("test_value".to_string())));
        cache.set("key1", value, None).await.unwrap();
        
        // Hit
        cache.get("key1").await.unwrap();
        
        // Miss
        cache.get("key2").await.unwrap();
        
        let metrics = cache.metrics();
        assert_eq!(metrics.hits(), 1);
        assert_eq!(metrics.misses(), 1);
        assert_eq!(metrics.sets(), 1);
        assert_eq!(metrics.hit_rate(), 0.5);
    }
}
