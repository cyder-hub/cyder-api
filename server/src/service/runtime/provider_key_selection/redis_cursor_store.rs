use async_trait::async_trait;
use bb8_redis::redis::cmd;
use std::fmt::Display;
use std::time::Duration;

use crate::service::app_state::AppStoreError;
use crate::service::redis::RedisPool;

use super::ProviderKeyCursorStore;

const NEXT_QUEUE_INDEX_SCRIPT: &str = r#"
local cursor_key = KEYS[1]
local key_count = tonumber(ARGV[1])
local state_ttl_seconds = tonumber(ARGV[2])

if not key_count or key_count <= 0 then
    return redis.error_reply('provider key cursor requires at least one key')
end

local seq = redis.call('INCR', cursor_key)
redis.call('EXPIRE', cursor_key, state_ttl_seconds)
return (seq - 1) % key_count
"#;

#[derive(Clone)]
pub struct RedisProviderKeyCursorStore {
    pool: RedisPool,
    key_prefix: String,
    state_ttl: Duration,
}

impl RedisProviderKeyCursorStore {
    pub fn new(pool: RedisPool, key_prefix: impl Into<String>, state_ttl: Duration) -> Self {
        Self {
            pool,
            key_prefix: key_prefix.into(),
            state_ttl,
        }
    }

    fn cursor_key(&self, provider_id: i64) -> String {
        format!("{}provider_key_cursor:{}", self.key_prefix, provider_id)
    }

    fn state_ttl_seconds(&self) -> u64 {
        self.state_ttl.as_secs().max(1)
    }

    fn redis_error(context: &str, err: impl Display) -> AppStoreError {
        AppStoreError::CacheError(format!("{context}: {err}"))
    }
}

#[async_trait]
impl ProviderKeyCursorStore for RedisProviderKeyCursorStore {
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
        let key_count = i64::try_from(key_count).map_err(|_| {
            AppStoreError::CacheError("provider key cursor key count is too large".to_string())
        })?;
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_error("failed to get redis connection", err))?;
        let index: i64 = cmd("EVAL")
            .arg(NEXT_QUEUE_INDEX_SCRIPT)
            .arg(1)
            .arg(self.cursor_key(provider_id))
            .arg(key_count)
            .arg(self.state_ttl_seconds())
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_error("provider key cursor script failed", err))?;
        usize::try_from(index)
            .map_err(|_| AppStoreError::CacheError("provider key cursor index is negative".into()))
    }

    async fn reset_provider_cursor(&self, provider_id: i64) -> Result<(), AppStoreError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_error("failed to get redis connection", err))?;
        let _: i64 = cmd("DEL")
            .arg(self.cursor_key(provider_id))
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_error("failed to reset provider key cursor", err))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb8::Pool;
    use bb8_redis::RedisConnectionManager;

    fn redis_unavailable_pool() -> RedisPool {
        let manager = RedisConnectionManager::new("redis://127.0.0.1:1")
            .expect("redis test URL should be valid");
        Pool::builder()
            .connection_timeout(Duration::from_millis(20))
            .build_unchecked(manager)
    }

    #[tokio::test]
    async fn cursor_returns_cache_error_when_redis_connection_fails() {
        let store = RedisProviderKeyCursorStore::new(
            redis_unavailable_pool(),
            "runtime:test:unavailable:",
            Duration::from_secs(60),
        );

        let err = store
            .next_queue_index(1, 3)
            .await
            .expect_err("redis connection failure should be reported");
        assert!(matches!(err, AppStoreError::CacheError(_)));
    }
}
