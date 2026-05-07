use crate::config::{CONFIG, StorageConfig};
use crate::schema::enum_def::StorageType;
use crate::service::storage::local::LocalStorage;
use crate::service::storage::s3::S3Storage;
use crate::service::storage::types::{
    GetObjectOptions, ListObjectOptions, PutObjectOptions, StorageError, StorageObjectList,
    StorageObjectMetadata, StorageResult,
};
use async_trait::async_trait;
use bytes::Bytes;
use cyder_tools::log::warn;
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
    async fn list_objects(
        &self,
        _options: Option<ListObjectOptions<'_>>,
    ) -> StorageResult<StorageObjectList> {
        Err(types::StorageError::Unsupported(
            "list_objects is not supported by this storage driver".to_string(),
        ))
    }
    async fn get_object_metadata(&self, _key: &str) -> StorageResult<StorageObjectMetadata> {
        Err(types::StorageError::Unsupported(
            "get_object_metadata is not supported by this storage driver".to_string(),
        ))
    }
    async fn get_presigned_url(&self, _key: &str) -> StorageResult<String> {
        Err(types::StorageError::Unsupported(
            "get_presigned_url is not supported by this storage driver".to_string(),
        ))
    }
}

static STORAGE: OnceCell<Box<dyn Storage>> = OnceCell::const_new();
static LOCAL_STORAGE: OnceCell<LocalStorage> = OnceCell::const_new();
static S3_STORAGE: OnceCell<StorageResult<Option<S3Storage>>> = OnceCell::const_new();

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

pub async fn get_s3_storage_result() -> StorageResult<Option<&'static S3Storage>> {
    match S3_STORAGE
        .get_or_init(|| async {
            if let Some(s3_config) = CONFIG.storage.s3.as_ref() {
                S3Storage::new(s3_config).await.map(Some)
            } else {
                Ok(None)
            }
        })
        .await
    {
        Ok(Some(storage)) => Ok(Some(storage)),
        Ok(None) => Ok(None),
        Err(error) => Err(error.clone()),
    }
}

pub async fn get_s3_storage() -> Option<&'static S3Storage> {
    match get_s3_storage_result().await {
        Ok(storage) => storage,
        Err(error) => {
            warn!("S3 storage is unavailable: {}", error);
            None
        }
    }
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
        crate::config::StorageDriver::Local => match LocalStorage::try_new(&config.local.root) {
            Ok(storage) => Box::new(storage),
            Err(error) => Box::new(UnavailableStorage::new(
                StorageType::FileSystem,
                error.to_string(),
            )),
        },
        crate::config::StorageDriver::S3 => match config.s3.as_ref() {
            Some(s3_config) => match S3Storage::new(s3_config).await {
                Ok(storage) => Box::new(storage),
                Err(error) => Box::new(UnavailableStorage::new(StorageType::S3, error.to_string())),
            },
            None => Box::new(UnavailableStorage::new(
                StorageType::S3,
                "S3 storage is not configured",
            )),
        },
    }
}

struct UnavailableStorage {
    storage_type: StorageType,
    reason: String,
}

impl UnavailableStorage {
    fn new(storage_type: StorageType, reason: impl Into<String>) -> Self {
        Self {
            storage_type,
            reason: reason.into(),
        }
    }

    fn error(&self) -> StorageError {
        StorageError::Config(self.reason.clone())
    }
}

#[async_trait]
impl Storage for UnavailableStorage {
    fn get_storage_type(&self) -> StorageType {
        self.storage_type.clone()
    }

    async fn put_object(
        &self,
        _key: &str,
        _data: Bytes,
        _options: Option<PutObjectOptions<'_>>,
    ) -> StorageResult<()> {
        Err(self.error())
    }

    async fn get_object(
        &self,
        _key: &str,
        _options: Option<GetObjectOptions<'_>>,
    ) -> StorageResult<Bytes> {
        Err(self.error())
    }

    async fn delete_object(&self, _key: &str) -> StorageResult<()> {
        Err(self.error())
    }

    async fn list_objects(
        &self,
        _options: Option<ListObjectOptions<'_>>,
    ) -> StorageResult<StorageObjectList> {
        Err(self.error())
    }

    async fn get_object_metadata(&self, _key: &str) -> StorageResult<StorageObjectMetadata> {
        Err(self.error())
    }

    async fn get_presigned_url(&self, _key: &str) -> StorageResult<String> {
        Err(self.error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_config_error<T>(result: StorageResult<T>) {
        match result {
            Err(StorageError::Config(message)) => {
                assert!(message.contains("S3 storage is not configured"));
                assert!(!message.contains("secret"));
                assert!(!message.contains("access"));
            }
            Ok(_) => panic!("expected config error, got success"),
            Err(other) => panic!("expected config error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unavailable_storage_returns_config_error_for_all_operations() {
        let storage = UnavailableStorage::new(StorageType::S3, "S3 storage is not configured");

        assert_eq!(storage.get_storage_type(), StorageType::S3);
        assert_config_error(
            storage
                .put_object("key", Bytes::from_static(b"body"), None)
                .await,
        );
        assert_config_error(storage.get_object("key", None).await);
        assert_config_error(storage.delete_object("key").await);
        assert_config_error(storage.list_objects(None).await);
        assert_config_error(storage.get_object_metadata("key").await);
        assert_config_error(storage.get_presigned_url("key").await);
    }

    #[tokio::test]
    async fn test_builds_keep_new_storage_isolated_to_local_storage() {
        let config = StorageConfig {
            driver: crate::config::StorageDriver::S3,
            local: crate::config::LocalStorageConfig {
                root: "unused".to_string(),
            },
            s3: None,
        };

        let storage = new_storage(&config).await;

        assert_eq!(storage.get_storage_type(), StorageType::FileSystem);
    }
}
