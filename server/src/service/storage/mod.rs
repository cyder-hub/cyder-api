use crate::config::{CONFIG, StorageConfig};
use crate::schema::enum_def::StorageType;
use crate::service::storage::local::LocalStorage;
use crate::service::storage::s3::S3Storage;
use crate::service::storage::types::{GetObjectOptions, PutObjectOptions, StorageResult};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::OnceCell;

#[cfg(test)]
use std::sync::LazyLock;
#[cfg(test)]
use tempfile::TempDir;

pub mod local;
pub mod s3;
pub mod types;

#[async_trait]
pub trait Storage: Send + Sync {
    fn get_storage_type(&self) -> StorageType;
    async fn put_object(
        &self,
        key: &str,
        data: Bytes,
        options: Option<PutObjectOptions<'_>>,
    ) -> StorageResult<()>;
    async fn get_object(
        &self,
        key: &str,
        options: Option<GetObjectOptions<'_>>,
    ) -> StorageResult<Bytes>;
    async fn delete_object(&self, key: &str) -> StorageResult<()>;
    async fn get_presigned_url(&self, _key: &str) -> StorageResult<String> {
        Err(types::StorageError::Unsupported(
            "get_presigned_url is not supported by this storage driver".to_string(),
        ))
    }
}

static STORAGE: OnceCell<Box<dyn Storage>> = OnceCell::const_new();
static LOCAL_STORAGE: OnceCell<LocalStorage> = OnceCell::const_new();
static S3_STORAGE: OnceCell<Option<S3Storage>> = OnceCell::const_new();

#[cfg(test)]
static TEST_LOCAL_STORAGE_DIR: LazyLock<TempDir> =
    LazyLock::new(|| tempfile::tempdir().expect("test local storage dir should be created"));

#[cfg(test)]
fn local_storage_root() -> String {
    TEST_LOCAL_STORAGE_DIR.path().to_string_lossy().into_owned()
}

#[cfg(not(test))]
fn local_storage_root() -> String {
    CONFIG.storage.local.root.clone()
}

pub async fn get_local_storage() -> &'static LocalStorage {
    LOCAL_STORAGE
        .get_or_init(|| async { LocalStorage::new(&local_storage_root()) })
        .await
}

pub async fn get_s3_storage() -> Option<&'static S3Storage> {
    S3_STORAGE
        .get_or_init(|| async {
            if let Some(s3_config) = CONFIG.storage.s3.as_ref() {
                Some(S3Storage::new(s3_config).await)
            } else {
                None
            }
        })
        .await
        .as_ref()
}

async fn initialize_storage() -> Box<dyn Storage> {
    let storage_config = &CONFIG.storage;
    new_storage(storage_config).await
}

pub async fn get_storage() -> &'static Box<dyn Storage> {
    STORAGE.get_or_init(initialize_storage).await
}

pub async fn new_storage(config: &StorageConfig) -> Box<dyn Storage> {
    #[cfg(test)]
    {
        let _ = config;
        return Box::new(LocalStorage::new(&local_storage_root()));
    }

    #[cfg(not(test))]
    match config.driver {
        crate::config::StorageDriver::Local => Box::new(LocalStorage::new(&config.local.root)),
        crate::config::StorageDriver::S3 => {
            if let Some(s3_config) = config.s3.as_ref() {
                Box::new(S3Storage::new(s3_config).await)
            } else {
                Box::new(LocalStorage::new(&config.local.root))
            }
        }
    }
}
