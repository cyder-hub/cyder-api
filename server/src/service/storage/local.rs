use crate::service::storage::Storage;
use crate::service::storage::types::{
    GetObjectOptions, ListObjectOptions, PutObjectOptions, StorageError, StorageObjectList,
    StorageObjectMetadata, StorageResult,
};
use async_trait::async_trait;
use bytes::Bytes;
use cyder_tools::log::error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

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

    fn metadata_for_path(&self, key: String, metadata: fs::Metadata) -> StorageObjectMetadata {
        StorageObjectMetadata {
            key,
            size_bytes: i64::try_from(metadata.len()).ok(),
            last_modified_ms: metadata.modified().ok().and_then(system_time_millis),
        }
    }

    fn key_for_path(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        Some(
            relative
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/"),
        )
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

    async fn get_object(
        &self,
        key: &str,
        options: Option<GetObjectOptions<'_>>,
    ) -> StorageResult<Bytes> {
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

    async fn list_objects(
        &self,
        options: Option<ListObjectOptions<'_>>,
    ) -> StorageResult<StorageObjectList> {
        let prefix = options.as_ref().and_then(|options| options.prefix);
        let limit = options.as_ref().and_then(|options| options.limit);
        let probe_limit = limit.map(|limit| limit.saturating_add(1));
        let mut objects = Vec::new();
        let mut dirs = vec![self.root.clone()];

        'walk: while let Some(dir) = dirs.pop() {
            let entries = fs::read_dir(&dir)
                .map_err(|e| StorageError::Get(format!("Failed to list directory: {}", e)))?;
            for entry in entries {
                let entry = entry
                    .map_err(|e| StorageError::Get(format!("Failed to read directory: {}", e)))?;
                let path = entry.path();
                let metadata = entry.metadata().map_err(|e| {
                    StorageError::Get(format!("Failed to read file metadata: {}", e))
                })?;

                if metadata.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if !metadata.is_file() {
                    continue;
                }

                let Some(key) = self.key_for_path(&path) else {
                    continue;
                };
                if prefix.is_some_and(|prefix| !key.starts_with(prefix)) {
                    continue;
                }
                objects.push(self.metadata_for_path(key, metadata));
                if probe_limit.is_some_and(|probe_limit| objects.len() >= probe_limit) {
                    break 'walk;
                }
            }
        }

        objects.sort_by(|left, right| left.key.cmp(&right.key));
        let limit_reached = limit.is_some_and(|limit| objects.len() > limit);
        if let Some(limit) = limit {
            objects.truncate(limit);
        }
        Ok(StorageObjectList {
            objects,
            limit_reached,
        })
    }

    async fn get_object_metadata(&self, key: &str) -> StorageResult<StorageObjectMetadata> {
        let full_path = self.get_full_path(key);
        let metadata = fs::metadata(&full_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound
            } else {
                StorageError::Get(format!("Failed to read file metadata: {}", e))
            }
        })?;
        if !metadata.is_file() {
            return Err(StorageError::Get(format!(
                "Storage key {} does not reference a file",
                key
            )));
        }
        Ok(self.metadata_for_path(key.to_string(), metadata))
    }
}

fn system_time_millis(time: std::time::SystemTime) -> Option<i64> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    Some(i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
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
