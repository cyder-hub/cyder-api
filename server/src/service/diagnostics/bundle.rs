use std::io::Read;

use bytes::Bytes;
use cyder_tools::log::debug;
use flate2::read::GzDecoder;
use serde::Deserialize;

use crate::{
    controller::BaseError,
    database::request_log::{RequestLog, RequestLogRecord},
    schema::enum_def::StorageType,
    service::{
        diagnostics::policy,
        storage::{Storage, get_local_storage, get_s3_storage, types::GetObjectOptions},
    },
    utils::storage::{REQUEST_LOG_BUNDLE_V2_VERSION, RequestLogBundleV2},
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestLogBundleLocator {
    pub storage_type: StorageType,
    pub key: String,
}

#[derive(Deserialize)]
struct BundleHeader {
    version: u32,
}

pub(crate) async fn load_request_log_bundle(
    record: &RequestLogRecord,
) -> Result<Option<RequestLogBundleV2>, BaseError> {
    let Some(locator) = optional_request_log_bundle_locator(record) else {
        return Ok(None);
    };

    let storage = storage_for_type(&locator.storage_type).await?;
    load_request_log_bundle_with_storage(storage, &locator.key)
        .await
        .map(Some)
}

pub async fn load_request_log_bundle_content(request_log_id: i64) -> Result<Bytes, BaseError> {
    if !policy::raw_bundle_download_enabled() {
        return Err(BaseError::ParamInvalid(Some(
            "Raw request log bundle download is disabled by diagnostics.raw_bundle_download_enabled"
                .to_string(),
        )));
    }

    let record = RequestLog::get_by_id(request_log_id)?;
    let locator = resolve_request_log_bundle_content_location(
        record.bundle_storage_type,
        record.bundle_storage_key,
    )?;
    let storage = storage_for_type(&locator.storage_type).await?;
    load_request_log_bundle_content_with_storage(storage, &locator.key).await
}

fn optional_request_log_bundle_locator(
    record: &RequestLogRecord,
) -> Option<RequestLogBundleLocator> {
    Some(RequestLogBundleLocator {
        storage_type: record.bundle_storage_type.clone()?,
        key: record.bundle_storage_key.clone()?,
    })
}

pub(crate) fn resolve_request_log_bundle_content_location(
    storage_type: Option<StorageType>,
    storage_key: Option<String>,
) -> Result<RequestLogBundleLocator, BaseError> {
    match (storage_type, storage_key) {
        (Some(storage_type), Some(key)) => Ok(RequestLogBundleLocator { storage_type, key }),
        _ => Err(BaseError::NotFound(Some(
            "Storage type not found".to_string(),
        ))),
    }
}

async fn storage_for_type(storage_type: &StorageType) -> Result<&'static dyn Storage, BaseError> {
    match storage_type {
        StorageType::FileSystem => Ok(get_local_storage().await),
        StorageType::S3 => Ok(get_s3_storage()
            .await
            .ok_or_else(|| BaseError::NotFound(Some("S3 storage not available".to_string())))?),
    }
}

pub(crate) async fn load_request_log_bundle_with_storage(
    storage: &dyn Storage,
    key: &str,
) -> Result<RequestLogBundleV2, BaseError> {
    let bytes = storage
        .get_object(
            key,
            Some(GetObjectOptions {
                content_encoding: Some("gzip"),
            }),
        )
        .await
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to read request log bundle {}: {}",
                key, err
            )))
        })?;

    decode_request_log_bundle(&bytes).map_err(|err| BaseError::DatabaseFatal(Some(err)))
}

pub(crate) async fn load_request_log_bundle_content_with_storage(
    storage: &dyn Storage,
    key: &str,
) -> Result<Bytes, BaseError> {
    debug!("Getting request log bundle content for key: {}", key);
    storage
        .get_object(
            key,
            Some(GetObjectOptions {
                content_encoding: Some(""),
            }),
        )
        .await
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to read request log bundle content {}: {}",
                key, err
            )))
        })
}

pub(crate) fn decode_request_log_bundle(bytes: &[u8]) -> Result<RequestLogBundleV2, String> {
    decode_request_log_bundle_inner(bytes).or_else(|first_error| {
        if first_error.contains("only version 2") {
            return Err(first_error);
        }
        let mut decoder = GzDecoder::new(bytes);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|gzip_error| {
                format!(
                    "Failed to decode request log bundle: {}; gzip fallback failed: {}",
                    first_error, gzip_error
                )
            })?;
        decode_request_log_bundle_inner(&decompressed).map_err(|second_error| {
            format!(
                "Failed to decode request log bundle: {}; gzip decoded fallback failed: {}",
                first_error, second_error
            )
        })
    })
}

fn decode_request_log_bundle_inner(bytes: &[u8]) -> Result<RequestLogBundleV2, String> {
    let header: BundleHeader = rmp_serde::from_slice(bytes)
        .map_err(|err| format!("bundle header decode failed: {}", err))?;
    match header.version {
        REQUEST_LOG_BUNDLE_V2_VERSION => rmp_serde::from_slice::<RequestLogBundleV2>(bytes)
            .map_err(|err| format!("bundle v2 decode failed: {}", err)),
        1 => Err(
            "request log bundle version 1 is no longer supported; only version 2 can be read"
                .to_string(),
        ),
        other => Err(format!(
            "unsupported request log bundle version {}; only version 2 is supported",
            other
        )),
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use flate2::{Compression, write::GzEncoder};
    use serde::Serialize;
    use std::io::Write;
    use tempfile::tempdir;

    use crate::{
        controller::BaseError,
        schema::enum_def::StorageType,
        service::storage::{Storage, local::LocalStorage},
        utils::storage::{
            RequestLogBundleRequestSection, RequestLogBundleV2, RequestLogBundleV2Builder,
        },
    };

    use super::{
        decode_request_log_bundle, load_request_log_bundle_content_with_storage,
        load_request_log_bundle_with_storage, resolve_request_log_bundle_content_location,
    };

    fn bundle() -> RequestLogBundleV2 {
        RequestLogBundleV2Builder::new().finish(
            42,
            1_776_840_000_000,
            RequestLogBundleRequestSection {
                user_request_blob_id: None,
                user_response_blob_id: None,
                user_response_capture_state: None,
            },
            Vec::new(),
            Default::default(),
        )
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn request_log_bundle_decode_accepts_gzipped_msgpack() {
        let body = rmp_serde::to_vec_named(&bundle()).unwrap();
        let compressed = gzip(&body);

        let decoded = decode_request_log_bundle(&compressed).unwrap();
        assert_eq!(decoded.version, 2);
        assert_eq!(decoded.log_id, 42);
    }

    #[test]
    fn request_log_bundle_decode_rejects_v1_bundle() {
        #[derive(Serialize)]
        struct LegacyBundleHeaderOnly {
            version: u32,
        }

        let encoded = rmp_serde::to_vec_named(&LegacyBundleHeaderOnly { version: 1 }).unwrap();
        let err = decode_request_log_bundle(&encoded).unwrap_err();

        assert_eq!(
            err,
            "request log bundle version 1 is no longer supported; only version 2 can be read"
        );
    }

    #[test]
    fn request_log_content_location_uses_persisted_bundle_key_only() {
        let locator = resolve_request_log_bundle_content_location(
            Some(StorageType::FileSystem),
            Some("explicit/bundle-key.mp.gz".to_string()),
        )
        .expect("persisted bundle key should resolve");

        assert_eq!(locator.storage_type, StorageType::FileSystem);
        assert_eq!(locator.key, "explicit/bundle-key.mp.gz");

        let missing_key =
            resolve_request_log_bundle_content_location(Some(StorageType::FileSystem), None);
        assert!(matches!(missing_key, Err(BaseError::NotFound(_))));

        let missing_storage = resolve_request_log_bundle_content_location(
            None,
            Some("legacy-derived-key".to_string()),
        );
        assert!(matches!(missing_storage, Err(BaseError::NotFound(_))));
    }

    #[tokio::test]
    async fn request_log_content_reads_raw_gzip_msgpack_without_decompression() {
        let dir = tempdir().unwrap();
        let storage = LocalStorage::new(dir.path().to_str().unwrap());
        let key = "request-log/42/bundle.msgpack.gz";
        let encoded = rmp_serde::to_vec_named(&bundle()).unwrap();
        let compressed = Bytes::from(gzip(&encoded));

        storage
            .put_object(key, compressed.clone(), None)
            .await
            .unwrap();

        let raw = load_request_log_bundle_content_with_storage(&storage, key)
            .await
            .unwrap();

        assert_eq!(raw, compressed);
        assert_ne!(raw, Bytes::from(encoded.clone()));
        let decoded = decode_request_log_bundle(&raw).unwrap();
        assert_eq!(decoded.log_id, 42);
    }

    #[tokio::test]
    async fn request_log_bundle_load_decodes_from_storage() {
        let dir = tempdir().unwrap();
        let storage = LocalStorage::new(dir.path().to_str().unwrap());
        let key = "request-log/42/bundle.msgpack.gz";
        let encoded = rmp_serde::to_vec_named(&bundle()).unwrap();
        let compressed = Bytes::from(gzip(&encoded));

        storage.put_object(key, compressed, None).await.unwrap();

        let decoded = load_request_log_bundle_with_storage(&storage, key)
            .await
            .unwrap();

        assert_eq!(decoded.version, 2);
        assert_eq!(decoded.log_id, 42);
    }
}
