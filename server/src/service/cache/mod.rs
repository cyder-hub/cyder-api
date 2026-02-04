use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;
use serde::{de::DeserializeOwned, Serialize};

use self::types::CacheEntry;

pub mod memory;
pub mod repository;
pub mod metrics;
pub mod redis;
pub mod types;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    
    #[error("Backend error: {0}")]
    BackendError(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Already exists: {0}")]
    AlreadyExists(String),
    
    #[error("Database error: {0}")]
    DatabaseError(String),
}

impl From<bincode::error::EncodeError> for CacheError {
    fn from(e: bincode::error::EncodeError) -> Self {
        CacheError::SerializationError(e.to_string())
    }
}

impl From<bincode::error::DecodeError> for CacheError {
    fn from(e: bincode::error::DecodeError) -> Self {
        CacheError::DeserializationError(e.to_string())
    }
}

/// Simplified cache backend trait - basic KV operations only
#[async_trait]
pub trait CacheBackend<T>: Send + Sync + Clone + 'static 
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    type Error: std::error::Error + Send + Sync + 'static;
    
    // Basic operations
    async fn get(&self, key: &str) -> Result<Option<std::sync::Arc<CacheEntry<T>>>, Self::Error>;
    async fn set(&self, key: &str, value: std::sync::Arc<CacheEntry<T>>, ttl: Option<Duration>) -> Result<(), Self::Error>;
    async fn delete(&self, key: &str) -> Result<(), Self::Error>;
    async fn clear(&self) -> Result<(), Self::Error>;
    
    // Batch operations (for performance)
    async fn mget(&self, keys: &[&str]) -> Result<Vec<Option<std::sync::Arc<CacheEntry<T>>>>, Self::Error>;
    async fn mset(&self, entries: &[(&str, std::sync::Arc<CacheEntry<T>>)], ttl: Option<Duration>) -> Result<(), Self::Error>;
}
