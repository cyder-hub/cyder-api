use async_trait::async_trait;
use bb8_redis::bb8;
use bb8_redis::redis::{AsyncCommands, FromRedisValue, RedisError, ToRedisArgs, cmd};
use serde::{de::DeserializeOwned, Serialize};
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use crate::service::redis::RedisPool;

use super::{metrics::CacheMetrics, types::CacheEntry, CacheBackend};

#[derive(Debug, Error)]
pub enum RedisCacheError {
    #[error("Redis error: {0}")]
    Redis(#[from] RedisError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("Client build error: {0}")]
    ClientBuild(String),
    #[error("Pool error: {0}")]
    Pool(#[from] bb8::RunError<RedisError>),
}

struct CacheValue<T: Clone + Serialize + DeserializeOwned>(Arc<CacheEntry<T>>);

impl<T: Clone + Serialize + DeserializeOwned> ToRedisArgs for CacheValue<T> {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let encoded = bincode::serialize(&*self.0).unwrap();
        out.write_arg(&encoded)
    }
}

impl<T: Clone + Serialize + DeserializeOwned> FromRedisValue for CacheValue<T> {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        if let redis::Value::Data(data) = v {
            let decoded: CacheEntry<T> = bincode::deserialize(data)
                .map_err(|e| RedisError::from((redis::ErrorKind::TypeError, "Deserialization error", e.to_string())))?;
            Ok(CacheValue(Arc::new(decoded)))
        } else {
            Err(RedisError::from((
                redis::ErrorKind::TypeError,
                "Expected binary data for CacheValue",
            )))
        }
    }
}

/// Redis cache backend
#[derive(Clone)]
pub struct RedisCacheBackend<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'static + Clone,
{
    pool: RedisPool,
    metrics: Arc<CacheMetrics>,
    key_prefix: String,
    _phantom: PhantomData<T>,
}

impl<T> RedisCacheBackend<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'static + Clone,
{
    pub fn new(
        pool: RedisPool,
        key_prefix: String,
    ) -> Self {
        Self {
            pool,
            metrics: Arc::new(CacheMetrics::new()),
            key_prefix,
            _phantom: PhantomData,
        }
    }

    fn get_full_key(&self, key: &str) -> String {
        format!("{}{}", self.key_prefix, key)
    }

    pub fn metrics(&self) -> &CacheMetrics {
        &self.metrics
    }
}

#[async_trait]
impl<T> CacheBackend<T> for RedisCacheBackend<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'static + Clone,
{
    type Error = RedisCacheError;

    async fn get(&self, key: &str) -> Result<Option<Arc<CacheEntry<T>>>, Self::Error> {
        let mut conn = self.pool.get().await?;
        let full_key = self.get_full_key(key);
        let result: Option<CacheValue<T>> = conn.get(full_key).await?;

        match result {
            Some(value) => {
                self.metrics.record_hit();
                Ok(Some(value.0))
            }
            None => {
                self.metrics.record_miss();
                Ok(None)
            }
        }
    }

    async fn set(&self, key: &str, value: Arc<CacheEntry<T>>, ttl: Option<Duration>) -> Result<(), Self::Error> {
        let mut conn = self.pool.get().await?;
        let full_key = self.get_full_key(key);
        let cache_value = CacheValue(value);

        if let Some(ttl) = ttl {
            conn.set_ex::<_, _, ()>(full_key, cache_value, ttl.as_secs()).await?;
        } else {
            conn.set::<_, _, ()>(full_key, cache_value).await?;
        }
        self.metrics.record_set();
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), Self::Error> {
        let mut conn = self.pool.get().await?;
        let full_key = self.get_full_key(key);
        conn.del::<_, ()>(full_key).await?;
        self.metrics.record_delete();
        Ok(())
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        let mut conn = self.pool.get().await?;
        let pattern = format!("{}*", self.key_prefix);
        
        // SCAN logic to find all keys with the prefix
        let mut keys_to_delete: Vec<String> = Vec::new();
        let mut cursor: i64 = 0;
        
        loop {
            let (next_cursor, keys): (i64, Vec<String>) = cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100) // Process 100 keys at a time
                .query_async(&mut *conn)
                .await?;

            keys_to_delete.extend(keys);
            
            if next_cursor == 0 {
                break;
            }
            cursor = next_cursor;
        }
        
        // Delete the keys in chunks to avoid blocking the server
        if !keys_to_delete.is_empty() {
            let mut pipe = redis::pipe();
            for key in &keys_to_delete {
                pipe.del(key);
            }
            pipe.query_async::<_, ()>(&mut *conn).await?;
            cyder_tools::log::info!("Cleared {} keys from Redis cache with prefix '{}'", keys_to_delete.len(), self.key_prefix);
        }

        Ok(())
    }

    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<Arc<CacheEntry<T>>>>, Self::Error> {
        let mut conn = self.pool.get().await?;
        let full_keys: Vec<String> = keys.iter().map(|k| self.get_full_key(k)).collect();
        let results: Vec<Option<CacheValue<T>>> = conn.get(full_keys).await?;
        let final_results: Vec<Option<Arc<CacheEntry<T>>>> = results.into_iter().map(|opt| opt.map(|cv| cv.0)).collect();

        for res in &final_results {
            if res.is_some() {
                self.metrics.record_hit();
            } else {
                self.metrics.record_miss();
            }
        }

        Ok(final_results)
    }

    async fn mset(&self, entries: &[(&str, Arc<CacheEntry<T>>)], ttl: Option<Duration>) -> Result<(), Self::Error> {
        let mut conn = self.pool.get().await?;
        let mut pipe = redis::pipe();
        let items: Vec<_> = entries
            .iter()
            .map(|(k, v)| (self.get_full_key(k), CacheValue(v.clone())))
            .collect();

        if let Some(ttl_duration) = ttl {
            for (key, value) in &items {
                pipe.set_ex(key, value, ttl_duration.as_secs());
            }
        } else {
            let items_for_mset: Vec<_> = items.iter().map(|(k, v)| (k.as_str(), v)).collect();
            pipe.mset(&items_for_mset);
        }

        pipe.query_async::<_, ()>(&mut *conn).await?;

        for _ in entries {
            self.metrics.record_set();
        }

        Ok(())
    }
}

