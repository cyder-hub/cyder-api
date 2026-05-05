use crate::config::S3StorageConfig;
use crate::service::storage::Storage;
use crate::service::storage::types::{
    GetObjectOptions, ListObjectOptions, PutObjectOptions, StorageError, StorageObjectList,
    StorageObjectMetadata, StorageResult,
};
use async_trait::async_trait;
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::{
    Client, Config,
    config::{Credentials, Region},
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
    pub async fn new(config: &S3StorageConfig) -> StorageResult<Self> {
        let validated = ValidatedS3StorageConfig::new(config)?;
        let region = Region::new(validated.region.to_string());
        let credentials = Credentials::new(
            validated.access_key.to_string(),
            validated.secret_key.to_string(),
            None,
            None,
            "default",
        );
        let mut s3_config_builder = Config::builder()
            .region(region)
            .credentials_provider(credentials)
            .behavior_version_latest()
            .response_checksum_validation(
                aws_sdk_s3::config::ResponseChecksumValidation::WhenRequired,
            );

        if let Some(endpoint) = validated.endpoint {
            s3_config_builder = s3_config_builder.endpoint_url(endpoint);
        }
        if config.force_path_style || validated.endpoint.is_some() {
            s3_config_builder = s3_config_builder.force_path_style(true);
        }

        let s3_config = s3_config_builder.build();
        let client = Client::from_conf(s3_config);
        info!("S3 storage initialized for bucket: {}", validated.bucket);
        Ok(Self {
            client,
            bucket: validated.bucket.to_string(),
        })
    }
}

struct ValidatedS3StorageConfig<'a> {
    endpoint: Option<&'a str>,
    region: &'a str,
    bucket: &'a str,
    access_key: &'a str,
    secret_key: &'a str,
}

impl<'a> ValidatedS3StorageConfig<'a> {
    fn new(config: &'a S3StorageConfig) -> StorageResult<Self> {
        let mut missing_fields = Vec::new();
        let region =
            required_optional_field("region", config.region.as_deref(), &mut missing_fields);
        let bucket = required_field("bucket", &config.bucket, &mut missing_fields);
        let access_key = required_optional_field(
            "access_key",
            config.access_key.as_deref(),
            &mut missing_fields,
        );
        let secret_key = required_optional_field(
            "secret_key",
            config.secret_key.as_deref(),
            &mut missing_fields,
        );

        if !missing_fields.is_empty() {
            return Err(StorageError::Config(format!(
                "S3 storage configuration is incomplete: missing {}",
                missing_fields.join(", ")
            )));
        }

        Ok(Self {
            endpoint: optional_non_empty_field(config.endpoint.as_deref()),
            region: region.expect("region is checked above"),
            bucket: bucket.expect("bucket is checked above"),
            access_key: access_key.expect("access_key is checked above"),
            secret_key: secret_key.expect("secret_key is checked above"),
        })
    }
}

fn required_optional_field<'a>(
    name: &'static str,
    value: Option<&'a str>,
    missing_fields: &mut Vec<&'static str>,
) -> Option<&'a str> {
    let value = optional_non_empty_field(value);
    if value.is_none() {
        missing_fields.push(name);
    }
    value
}

fn required_field<'a>(
    name: &'static str,
    value: &'a str,
    missing_fields: &mut Vec<&'static str>,
) -> Option<&'a str> {
    required_optional_field(name, Some(value), missing_fields)
}

fn optional_non_empty_field(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
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

    async fn get_object(
        &self,
        key: &str,
        options: Option<GetObjectOptions<'_>>,
    ) -> StorageResult<Bytes> {
        let mut request = self.client.get_object().bucket(&self.bucket).key(key);

        if let Some(opts) = options {
            if let Some(ce) = opts.content_encoding {
                request = request.response_content_encoding(ce);
            }
        }

        let resp = request.send().await.map_err(s3_get_error)?;

        let data = resp
            .body
            .collect()
            .await
            .map(|d| d.into_bytes())
            .map_err(|e| StorageError::Get(e.to_string()))?;
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

    async fn list_objects(
        &self,
        options: Option<ListObjectOptions<'_>>,
    ) -> StorageResult<StorageObjectList> {
        let prefix = options
            .as_ref()
            .and_then(|options| options.prefix)
            .map(str::to_string);
        let limit = options.as_ref().and_then(|options| options.limit);
        let mut continuation_token: Option<String> = None;
        let mut objects = Vec::new();

        loop {
            let mut request = self.client.list_objects_v2().bucket(&self.bucket);
            if let Some(prefix) = prefix.as_ref() {
                request = request.prefix(prefix);
            }
            if let Some(token) = continuation_token.as_ref() {
                request = request.continuation_token(token);
            }
            if let Some(limit) = limit {
                let remaining_probe = limit.saturating_add(1).saturating_sub(objects.len());
                request = request.max_keys(remaining_probe.clamp(1, 1000) as i32);
            }

            let response = request.send().await.map_err(s3_operation_error)?;

            for object in response.contents() {
                let Some(key) = object.key() else {
                    continue;
                };
                objects.push(StorageObjectMetadata {
                    key: key.to_string(),
                    size_bytes: object.size(),
                    last_modified_ms: None,
                });
                if let Some(limit) = limit {
                    if objects.len() > limit {
                        objects.truncate(limit);
                        return Ok(StorageObjectList {
                            objects,
                            limit_reached: true,
                        });
                    }
                }
            }

            if let Some(limit) = limit {
                if objects.len() == limit && response.is_truncated().unwrap_or(false) {
                    return Ok(StorageObjectList {
                        objects,
                        limit_reached: true,
                    });
                }
            }
            if !response.is_truncated().unwrap_or(false) {
                break;
            }
            continuation_token = response.next_continuation_token().map(str::to_string);
            if continuation_token.is_none() {
                break;
            }
        }

        Ok(StorageObjectList {
            objects,
            limit_reached: false,
        })
    }

    async fn get_object_metadata(&self, key: &str) -> StorageResult<StorageObjectMetadata> {
        let response = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(s3_get_error)?;

        Ok(StorageObjectMetadata {
            key: key.to_string(),
            size_bytes: response.content_length(),
            last_modified_ms: None,
        })
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

fn s3_get_error(error: impl ProvideErrorMetadata + ToString) -> StorageError {
    let display_message = error.to_string();
    if s3_error_text_is_not_found(error.code())
        || s3_error_text_is_not_found(error.message())
        || s3_error_text_is_not_found(Some(display_message.as_str()))
    {
        StorageError::NotFound
    } else {
        StorageError::Get(s3_error_message(&error, &display_message))
    }
}

fn s3_operation_error(error: impl ProvideErrorMetadata + ToString) -> StorageError {
    let display_message = error.to_string();
    StorageError::Get(s3_error_message(&error, &display_message))
}

fn s3_error_text_is_not_found(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let lower_value = value.to_ascii_lowercase();
    lower_value.contains("nosuchkey")
        || lower_value.contains("no such key")
        || lower_value.contains("notfound")
        || lower_value.contains("not found")
        || lower_value.contains("status code: 404")
        || lower_value.contains("status: 404")
        || lower_value.contains("404")
}

fn s3_error_message(error: &impl ProvideErrorMetadata, display_message: &str) -> String {
    match (error.code(), error.message()) {
        (Some(code), Some(message)) => format!("{display_message}: {code}: {message}"),
        (Some(code), None) => format!("{display_message}: {code}"),
        (None, Some(message)) => format!("{display_message}: {message}"),
        (None, None) => display_message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CONFIG, S3AccessMode};
    use bytes::Bytes;
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn complete_s3_config() -> S3StorageConfig {
        S3StorageConfig {
            endpoint: Some("http://127.0.0.1:9000".to_string()),
            region: Some("us-east-1".to_string()),
            bucket: "dev".to_string(),
            access_mode: S3AccessMode::Proxy,
            access_key: Some("test-access-key".to_string()),
            secret_key: Some("test-secret-key".to_string()),
            force_path_style: true,
            public_url: None,
        }
    }

    async fn get_test_s3_storage() -> Option<S3Storage> {
        if CONFIG.storage.s3.is_none() {
            println!("Skipping S3 storage test: S3 not configured.");
            return None;
        }
        let s3_config = CONFIG.storage.s3.as_ref().unwrap();
        let storage = match S3Storage::new(s3_config).await {
            Ok(storage) => storage,
            Err(error) => {
                println!("Skipping S3 storage test: {}", error);
                return None;
            }
        };

        let probe_key = format!("{}probe.txt", unique_s3_test_prefix("probe"));
        match storage
            .put_object(&probe_key, Bytes::from_static(b"s3-test-probe"), None)
            .await
        {
            Ok(()) => {
                let _ = storage.delete_object(&probe_key).await;
                Some(storage)
            }
            Err(error) => {
                println!("Skipping S3 storage test: S3 endpoint is not reachable: {error}");
                None
            }
        }
    }

    fn unique_s3_test_prefix(name: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        format!("test/{name}-{nanos}/")
    }

    #[tokio::test]
    async fn s3_storage_new_rejects_missing_required_fields_without_panicking() {
        let mut config = complete_s3_config();
        config.region = None;
        config.bucket.clear();

        let result = S3Storage::new(&config).await;

        assert!(matches!(
            result,
            Err(StorageError::Config(message))
                if message.contains("region") && message.contains("bucket")
        ));
    }

    #[tokio::test]
    async fn s3_storage_new_rejects_empty_credentials_without_panicking() {
        let mut config = complete_s3_config();
        config.access_key = Some("   ".to_string());
        config.secret_key = Some(String::new());

        let result = S3Storage::new(&config).await;

        assert!(matches!(
            result,
            Err(StorageError::Config(message))
                if message.contains("access_key") && message.contains("secret_key")
        ));
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
    async fn test_s3_storage_list_and_head_operations() {
        let storage = match get_test_s3_storage().await {
            Some(s) => s,
            None => return, // Skip test
        };

        let prefix = unique_s3_test_prefix("list-head");
        let key_a = format!("{prefix}a.txt");
        let key_b = format!("{prefix}b.txt");
        let data_a = Bytes::from_static(b"list and head object a");
        let data_b = Bytes::from_static(b"list and head object b");

        storage
            .put_object(&key_a, data_a.clone(), None)
            .await
            .expect("Should be able to put first object");
        storage
            .put_object(&key_b, data_b.clone(), None)
            .await
            .expect("Should be able to put second object");

        let listed = storage
            .list_objects(Some(ListObjectOptions {
                prefix: Some(&prefix),
                limit: None,
            }))
            .await
            .expect("Should be able to list objects");
        let listed_keys: Vec<&str> = listed
            .objects
            .iter()
            .map(|object| object.key.as_str())
            .collect();
        assert!(listed_keys.contains(&key_a.as_str()));
        assert!(listed_keys.contains(&key_b.as_str()));
        assert!(!listed.limit_reached);

        let limited = storage
            .list_objects(Some(ListObjectOptions {
                prefix: Some(&prefix),
                limit: Some(1),
            }))
            .await
            .expect("Should be able to list objects with a limit");
        assert_eq!(limited.objects.len(), 1);
        assert!(limited.limit_reached);

        let metadata = storage
            .get_object_metadata(&key_a)
            .await
            .expect("Should be able to read object metadata");
        assert_eq!(metadata.key, key_a);
        assert_eq!(metadata.size_bytes, Some(data_a.len() as i64));

        storage
            .delete_object(&key_a)
            .await
            .expect("Should be able to delete first object");
        storage
            .delete_object(&key_b)
            .await
            .expect("Should be able to delete second object");
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
