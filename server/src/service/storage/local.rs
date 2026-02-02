use crate::service::storage::types::{StorageError, StorageResult};
use crate::service::storage::Storage;
use async_trait::async_trait;
use bytes::Bytes;
use cyder_tools::log::error;
use std::fs;
use std::path::{Path, PathBuf};

use crate::schema::enum_def::StorageType;

#[derive(Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: &str) -> Self {
        let root_path = Path::new(root);
        if !root_path.exists() {
            fs::create_dir_all(root_path).expect("Failed to create local storage directory");
        }
        Self {
            root: root_path.to_path_buf(),
        }
    }

    fn get_full_path(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }
}

#[async_trait]
impl Storage for LocalStorage {
    fn get_storage_type(&self) -> StorageType {
        StorageType::FileSystem
    }

    async fn put_object(
        &self,
        key: &str,
        data: Bytes,
        _mimetype: Option<&str>,
    ) -> StorageResult<()> {
        let full_path = self.get_full_path(key);
        if let Some(parent) = full_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| {
                    error!("Failed to create directory for local storage: {}", e);
                    StorageError::Put("Failed to create directory".to_string())
                })?;
            }
        }
        fs::write(&full_path, data)
            .map_err(|e| StorageError::Put(format!("Failed to write to file: {}", e)))
    }

    async fn get_object(&self, key: &str) -> StorageResult<Bytes> {
        let full_path = self.get_full_path(key);
        fs::read(&full_path).map(Bytes::from).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound
            } else {
                StorageError::Get(format!("Failed to read file: {}", e))
            }
        })
    }

    async fn delete_object(&self, key: &str) -> StorageResult<()> {
        let full_path = self.get_full_path(key);
        fs::remove_file(full_path)
            .map_err(|e| StorageError::Delete(format!("Failed to delete file: {}", e)))
    }
}