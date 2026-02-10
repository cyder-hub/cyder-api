use crate::service::storage::types::{GetObjectOptions, PutObjectOptions, StorageError, StorageResult};
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
        _options: Option<PutObjectOptions<'_>>,
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

    async fn get_object(&self, key: &str, options: Option<GetObjectOptions<'_>>) -> StorageResult<Bytes> {
        let full_path = self.get_full_path(key);
        let data = fs::read(&full_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound
            } else {
                StorageError::Get(format!("Failed to read file: {}", e))
            }
        })?;

        if let Some(opts) = options {
            if let Some("gzip") = opts.content_encoding {
                use flate2::read::GzDecoder;
                use std::io::Read;
                let mut decoder = GzDecoder::new(&data[..]);
                let mut decompressed_data = Vec::new();
                decoder
                    .read_to_end(&mut decompressed_data)
                    .map_err(|e| StorageError::Get(format!("Failed to decompress file: {}", e)))?;
                return Ok(Bytes::from(decompressed_data));
            }
        }

        Ok(Bytes::from(data))
    }

    async fn delete_object(&self, key: &str) -> StorageResult<()> {
        let full_path = self.get_full_path(key);
        fs::remove_file(full_path)
            .map_err(|e| StorageError::Delete(format!("Failed to delete file: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_local_storage_gzip_compression() {
        let dir = tempdir().unwrap();
        let storage = LocalStorage::new(dir.path().to_str().unwrap());
        let key = "test_gzip.txt";
        let original_data = Bytes::from("some data to be compressed");

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&original_data).unwrap();
        let compressed_data = Bytes::from(encoder.finish().unwrap());

        storage
            .put_object(key, compressed_data.clone(), None)
            .await
            .unwrap();

        // 1. Get with decompression
        let decompressed_result = storage
            .get_object(
                key,
                Some(GetObjectOptions {
                    content_encoding: Some("gzip"),
                }),
            )
            .await
            .unwrap();
        assert_eq!(decompressed_result, original_data);

        // 2. Get without decompression
        let raw_result = storage.get_object(key, None).await.unwrap();
        assert_eq!(raw_result, compressed_data);

        storage.delete_object(key).await.unwrap();
    }
}
