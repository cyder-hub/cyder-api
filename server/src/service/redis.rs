use crate::config::CONFIG;
use bb8::Pool;
use bb8_redis::{redis, RedisConnectionManager};
use tokio::sync::OnceCell;
use cyder_tools::log::{info, error};

pub type RedisPool = Pool<RedisConnectionManager>;

static POOL: OnceCell<Option<RedisPool>> = OnceCell::const_new();

async fn initialize_pool() -> Option<RedisPool> {
    if let Some(redis_config) = CONFIG.redis.as_ref() {
        let manager = match RedisConnectionManager::new(redis_config.url.as_str()) {
            Ok(manager) => manager,
            Err(e) => {
                error!("Failed to create redis manager: {}", e);
                return None;
            }
        };
        let pool = match Pool::builder()
            .max_size(redis_config.pool_size as u32)
            .build(manager)
            .await
        {
            Ok(pool) => pool,
            Err(e) => {
                error!("Failed to create redis pool: {}", e);
                return None;
            }
        };

        // Test connection
        {
            let mut conn = match pool.get().await {
                Ok(conn) => conn,
                Err(e) => {
                    error!("Failed to get redis connection from pool for test: {}", e);
                    return None;
                }
            };
            if let Err(e) = redis::cmd("PING").query_async::<()>(&mut *conn).await {
                error!("Failed to ping redis: {}", e);
                return None;
            }
        }
        info!("Redis connection pool initialized and tested successfully");
        Some(pool)
    } else {
        None
    }
}

/// Returns a clone of the global Redis connection pool if Redis is configured.
pub async fn get_pool() -> Option<RedisPool> {
    POOL.get_or_init(initialize_pool).await.as_ref().cloned()
}
