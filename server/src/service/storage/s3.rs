use crate::config::S3StorageConfig;
use crate::service::storage::types::{StorageError, StorageResult};
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
            .behavior_version_latest();

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
        mimetype: Option<&str>,
    ) -> StorageResult<()> {
        let stream = ByteStream::from(data);
        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(stream);

        if let Some(mt) = mimetype {
            request = request.content_type(mt);
        }

        request
            .send()
            .await
            .map(|_| ())
            .map_err(|e| StorageError::Put(e.to_string()))
    }

    async fn get_object(&self, key: &str) -> StorageResult<Bytes> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| StorageError::Get(e.to_string()))?;

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
