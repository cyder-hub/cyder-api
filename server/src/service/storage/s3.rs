use crate::config::S3StorageConfig;
use crate::service::storage::types::{GetObjectOptions, PutObjectOptions, StorageError, StorageResult};
use crate::service::storage::Storage;
use async_trait::async_trait;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::{
    config::{Credentials, Region},
    Client, Config,
};
use bytes::Bytes;
use cyder_tools::log::info;
use std::time::Duration;

use crate::schema::enum_def::StorageType;

#[derive(Clone)]
pub struct S3Storage {
    client: Client,
    bucket: String,
}

impl S3Storage {
    pub async fn new(config: &S3StorageConfig) -> Self {
        let region = Region::new(config.region.clone().unwrap());
        let credentials = Credentials::new(
            config.access_key.clone().unwrap(),
            config.secret_key.clone().unwrap(),
            None,
            None,
            "default",
        );
        let mut s3_config_builder = Config::builder()
            .region(region)
            .credentials_provider(credentials)
            .behavior_version_latest()
            .response_checksum_validation(aws_sdk_s3::config::ResponseChecksumValidation::WhenRequired);

        if let Some(endpoint) = &config.endpoint {
            s3_config_builder = s3_config_builder
                .endpoint_url(endpoint.as_str())
                .force_path_style(true);
        }

        let s3_config = s3_config_builder.build();
        let client = Client::from_conf(s3_config);
        info!("S3 storage initialized for bucket: {}", &config.bucket);
        Self {
            client,
            bucket: config.bucket.clone(),
        }
    }
}

#[async_trait]
impl Storage for S3Storage {
    fn get_storage_type(&self) -> StorageType {
        StorageType::S3
    }

    async fn put_object(
        &self,
        key: &str,
        data: Bytes,
        options: Option<PutObjectOptions<'_>>,
    ) -> StorageResult<()> {
        let stream = ByteStream::from(data);
        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(stream);

        if let Some(opts) = options {
            if let Some(mt) = opts.content_type {
                request = request.content_type(mt);
            }

            if let Some(ce) = opts.content_encoding {
                request = request.content_encoding(ce);
            }
        }

        request
            .send()
            .await
            .map(|_| ())
            .map_err(|e| StorageError::Put(e.to_string()))
    }

    async fn get_object(&self, key: &str, options: Option<GetObjectOptions<'_>>) -> StorageResult<Bytes> {
        let mut request = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key);

        if let Some(opts) = options {
            if let Some(ce) = opts.content_encoding {
                request = request.response_content_encoding(ce);
            }
        }

        let resp = request
            .send()
            .await
            .map_err(|e| {
                StorageError::Get(e.to_string())
             })?;

        let data = resp
            .body
            .collect()
            .await
            .map(|d| d.into_bytes())
            .map_err(|e| {
                StorageError::Get(e.to_string())
            })?;
        Ok(data)
    }

    async fn delete_object(&self, key: &str) -> StorageResult<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map(|_| ())
            .map_err(|e| StorageError::Delete(e.to_string()))
    }

    async fn get_presigned_url(&self, key: &str) -> StorageResult<String> {
        let presigning_config = PresigningConfig::expires_in(Duration::from_secs(3600))
            .map_err(|e| StorageError::Unsupported(e.to_string()))?;
        let request = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning_config)
            .await
            .map_err(|e| StorageError::Unsupported(e.to_string()))?;

        Ok(request.uri().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CONFIG;
    use bytes::Bytes;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    // Helper to get S3 storage, skipping the test if not configured.
    async fn get_test_s3_storage() -> Option<S3Storage> {
        if CONFIG.storage.s3.is_none() {
            println!("Skipping S3 storage test: S3 not configured.");
            return None;
        }
        let s3_config = CONFIG.storage.s3.as_ref().unwrap();
        // Check for placeholder values, as the config might exist but be empty
        if s3_config.access_key.is_none() || s3_config.access_key.as_deref() == Some("") {
             println!("Skipping S3 storage test: S3 configuration is present but seems to be a placeholder.");
             return None;
        }
        Some(S3Storage::new(s3_config).await)
    }

    #[tokio::test]
    async fn test_s3_storage_basic_operations() {
        let storage = match get_test_s3_storage().await {
            Some(s) => s,
            None => return, // Skip test
        };

        let key = "test/s3_basic_ops.txt";
        let data = Bytes::from_static(b"Hello, S3!");

        // 1. Put object
        storage
            .put_object(key, data.clone(), None)
            .await
            .expect("Should be able to put object");

        // 2. Get object
        let fetched_data = storage
            .get_object(key, None)
            .await
            .expect("Should be able to get object");
        assert_eq!(fetched_data, data);

        // 3. Get presigned URL
        let presigned_url = storage
            .get_presigned_url(key)
            .await
            .expect("Should be able to get presigned URL");
        // A simple check to see if the URL looks plausible.
        // It should contain the bucket name, the key, and some auth params.
        assert!(presigned_url.contains(&storage.bucket));
        assert!(presigned_url.contains(key));
        assert!(presigned_url.contains("X-Amz-Algorithm"));

        // 4. Delete object
        storage
            .delete_object(key)
            .await
            .expect("Should be able to delete object");

        // 5. Verify deletion by trying to get it again
        let result = storage.get_object(key, None).await;
        assert!(result.is_err(), "Getting a deleted object should fail");
    }

    #[tokio::test]
    async fn test_put_object_with_options() {
        let storage = match get_test_s3_storage().await {
            Some(s) => s,
            None => return, // Skip test
        };

        let key = "test/s3_with_options.txt";
        let data = Bytes::from_static(b"Content with options");
        let options = PutObjectOptions {
            content_type: Some("text/plain; charset=utf-8"),
            content_encoding: Some("identity"),
        };

        // 1. Put object with options
        storage
            .put_object(key, data.clone(), Some(options))
            .await
            .expect("Should be able to put object with options");

        // We can't directly verify content-type/encoding with get_object.
        // A full verification would require making an HTTP HEAD request,
        // which is beyond the scope of this test. We'll just check if get works.
        // The main point is that the put_object call doesn't fail.

        // 2. Get object to make sure it's there
        let fetched_data = storage
            .get_object(key, None)
            .await
            .expect("Should be able to get object");
        assert_eq!(fetched_data, data);

        // 3. Clean up
        storage
            .delete_object(key)
            .await
            .expect("Should be able to delete the object");
    }

    #[tokio::test]
    async fn test_s3_storage_gzip_compression() {
        let storage = match get_test_s3_storage().await {
            Some(s) => s,
            None => return, // Skip test
        };

        let key = "test/log.json.gz";
        let original_data = Bytes::from(
            r#"{"level":"info","message":"This is a test log message which is long enough to benefit from compression. Repeating the message to make sure it is compressible. This is a test log message which is long enough to benefit from compression."}"#,
        );

        // 1. Compress data
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&original_data).unwrap();
        let compressed_vec = encoder.finish().unwrap();
        let compressed_data = Bytes::from(compressed_vec.clone());

        // Ensure compression actually happened
        assert!(
            compressed_data.len() < original_data.len(),
            "Compressed data should be smaller"
        );

        // 2. Put compressed object
        let options = PutObjectOptions {
            content_type: Some("application/json"),
            content_encoding: Some("gzip"),
        };
        storage
            .put_object(key, compressed_data.clone(), Some(options))
            .await
            .expect("Should be able to put gzipped object");

        // 3. Get object without decompression
        let fetched_compressed_data = storage
            .get_object(
                key,
                Some(GetObjectOptions {
                    content_encoding: Some(""),
                }),
            )
            .await
            .expect("Should be able to get gzipped object without decompression");
        assert_eq!(fetched_compressed_data, compressed_data);

        // 4. Get object with decompression
        let fetched_decompressed_data = storage
            .get_object(
                key,
                Some(GetObjectOptions {
                    content_encoding: Some("gzip"),
                }),
            )
            .await
            .expect("Should be able to get gzipped object with decompression");
        assert_eq!(fetched_decompressed_data, original_data);

        // 5. Clean up
        storage
            .delete_object(key)
            .await
            .expect("Should be able to delete the object");
    }
}
