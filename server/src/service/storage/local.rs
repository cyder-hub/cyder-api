use crate::service::storage::Storage;
use crate::service::storage::types::{
    GetObjectOptions, ListObjectOptions, PutObjectOptions, StorageError, StorageObjectList,
    StorageObjectMetadata, StorageResult,
};
use async_trait::async_trait;
use bytes::Bytes;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::schema::enum_def::StorageType;

#[derive(Clone, Debug)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: &str) -> Self {
        let storage = Self {
            root: Path::new(root).to_path_buf(),
        };
        // Preserve the legacy infallible constructor contract for shared
        // singleton callers; use try_new when initialization errors must be
        // surfaced immediately.
        let _ = storage.ensure_root();
        storage
    }

    pub fn try_new(root: &str) -> StorageResult<Self> {
        let storage = Self::new(root);
        storage.ensure_root()?;
        Ok(storage)
    }

    fn get_full_path(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }

    fn ensure_root(&self) -> StorageResult<()> {
        ensure_directory(
            &self.root,
            "create local storage root directory",
            StorageErrorKind::Config,
        )
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
            ensure_directory(
                parent,
                "create local storage object parent directory",
                StorageErrorKind::Put,
            )?;
        }
        fs::write(&full_path, data).map_err(|source| {
            StorageError::Put(format!(
                "failed to write local storage object '{}': {source}",
                full_path.display()
            ))
        })
    }

    async fn get_object(
        &self,
        key: &str,
        options: Option<GetObjectOptions<'_>>,
    ) -> StorageResult<Bytes> {
        let full_path = self.get_full_path(key);
        let data = fs::read(&full_path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                StorageError::NotFound
            } else {
                StorageError::Get(format!(
                    "failed to read local storage object '{}': {e}",
                    full_path.display()
                ))
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
        fs::remove_file(&full_path).map_err(|e| {
            StorageError::Delete(format!(
                "failed to delete local storage object '{}': {e}",
                full_path.display()
            ))
        })
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
            let entries = fs::read_dir(&dir).map_err(|e| {
                StorageError::Get(format!(
                    "failed to list local storage directory '{}': {e}",
                    dir.display()
                ))
            })?;
            for entry in entries {
                let entry = entry.map_err(|e| {
                    StorageError::Get(format!(
                        "failed to read local storage directory entry in '{}': {e}",
                        dir.display()
                    ))
                })?;
                let path = entry.path();
                let metadata = entry.metadata().map_err(|e| {
                    StorageError::Get(format!(
                        "failed to read local storage metadata '{}': {e}",
                        path.display()
                    ))
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
            if e.kind() == io::ErrorKind::NotFound {
                StorageError::NotFound
            } else {
                StorageError::Get(format!(
                    "failed to read local storage metadata '{}': {e}",
                    full_path.display()
                ))
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

#[derive(Clone, Copy)]
enum StorageErrorKind {
    Config,
    Put,
}

fn ensure_directory(
    path: &Path,
    operation: &'static str,
    error_kind: StorageErrorKind,
) -> StorageResult<()> {
    if path.exists() {
        if path.is_dir() {
            return Ok(());
        }
        return Err(storage_error(
            error_kind,
            operation,
            path,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "path exists but is not a directory",
            ),
        ));
    }

    fs::create_dir_all(path).map_err(|source| storage_error(error_kind, operation, path, source))
}

fn storage_error(
    kind: StorageErrorKind,
    operation: &'static str,
    path: &Path,
    source: io::Error,
) -> StorageError {
    let message = format!("failed to {operation} '{}': {source}", path.display());
    match kind {
        StorageErrorKind::Config => StorageError::Config(message),
        StorageErrorKind::Put => StorageError::Put(message),
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
        let storage = LocalStorage::try_new(dir.path().to_str().unwrap()).unwrap();
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

    #[test]
    fn local_storage_try_new_error_includes_operation_and_path() {
        let dir = tempdir().unwrap();
        let blocked = dir.path().join("blocked-storage-root");
        fs::write(&blocked, "not a directory").expect("blocking file should be written");

        let error = LocalStorage::try_new(blocked.to_str().expect("path should be utf8"))
            .expect_err("blocked local storage root should fail");
        let message = error.to_string();

        assert!(
            message.contains("create local storage root directory"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains(&blocked.display().to_string()),
            "unexpected error: {message}"
        );
    }

    #[tokio::test]
    async fn local_storage_new_materializes_root_for_empty_inventory() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("fresh-storage-root");

        let storage = LocalStorage::new(root.to_str().expect("path should be utf8"));
        let objects = storage
            .list_objects(None)
            .await
            .expect("fresh local storage root should be listable");

        assert!(root.is_dir());
        assert!(objects.objects.is_empty());
        assert!(!objects.limit_reached);
    }

    #[tokio::test]
    async fn local_storage_put_error_includes_operation_and_path() {
        let dir = tempdir().unwrap();
        let blocked = dir.path().join("blocked-storage-root");
        fs::write(&blocked, "not a directory").expect("blocking file should be written");
        let storage = LocalStorage::new(blocked.to_str().expect("path should be utf8"));

        let error = storage
            .put_object("bundle.json", Bytes::from_static(b"{}"), None)
            .await
            .expect_err("blocked local storage root should fail object write");
        let message = error.to_string();

        assert!(
            message.contains("create local storage object parent directory")
                || message.contains("write local storage object"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains(&blocked.display().to_string()),
            "unexpected error: {message}"
        );
    }
}
