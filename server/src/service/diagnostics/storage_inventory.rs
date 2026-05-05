use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{
    controller::BaseError,
    database::{
        request_log::{RequestLog, RequestLogBundleRetentionRecord},
        request_replay_run::{RequestReplayArtifactRetentionRecord, RequestReplayRun},
    },
    schema::enum_def::StorageType,
    service::storage::{
        Storage, get_local_storage, get_s3_storage_result,
        types::{ListObjectOptions, StorageError, StorageObjectMetadata},
    },
};

const DEFAULT_OBJECT_SAMPLE_LIMIT: usize = 20;
const DEFAULT_MISSING_LOCATOR_SAMPLE_LIMIT: usize = 20;
const DEFAULT_OBJECT_SCAN_LIMIT: usize = 10_000;
const DEFAULT_DB_LOCATOR_LIMIT: usize = 10_000;
const MAX_SAMPLE_LIMIT: usize = 200;
const MAX_OBJECT_SCAN_LIMIT: usize = 100_000;
const MAX_DB_LOCATOR_LIMIT: usize = 100_000;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DiagnosticsStorageInventoryParams {
    pub storage_types: Option<Vec<StorageType>>,
    pub prefix: Option<String>,
    pub object_sample_limit: Option<usize>,
    pub missing_locator_sample_limit: Option<usize>,
    pub object_scan_limit: Option<usize>,
    pub db_locator_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsStorageInventoryStatus {
    Available,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsStorageLocatorKind {
    RequestLogBundle,
    ReplayArtifact,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsStorageObjectSample {
    pub key: String,
    pub size_bytes: Option<i64>,
    pub last_modified_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsStorageMissingLocatorSample {
    pub locator_kind: DiagnosticsStorageLocatorKind,
    pub request_log_id: Option<i64>,
    pub replay_run_id: Option<i64>,
    pub storage_type: StorageType,
    pub storage_key: String,
    pub artifact_version: Option<i32>,
    pub bundle_version: Option<i32>,
    pub created_at: i64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsStorageInventoryBucket {
    pub storage_type: StorageType,
    pub status: DiagnosticsStorageInventoryStatus,
    pub message: Option<String>,
    pub prefix: Option<String>,
    pub object_scan_limit: usize,
    pub object_limit_reached: bool,
    pub object_count: usize,
    pub total_size_bytes: i64,
    pub unknown_size_object_count: usize,
    pub referenced_object_count: usize,
    pub unreferenced_object_count: usize,
    pub missing_locator_count: usize,
    pub locator_check_failed_count: usize,
    pub object_samples: Vec<DiagnosticsStorageObjectSample>,
    pub unreferenced_samples: Vec<DiagnosticsStorageObjectSample>,
    pub missing_locator_samples: Vec<DiagnosticsStorageMissingLocatorSample>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsStorageInventoryResponse {
    pub prefix: Option<String>,
    pub object_sample_limit: usize,
    pub missing_locator_sample_limit: usize,
    pub object_scan_limit: usize,
    pub db_locator_limit: usize,
    pub db_locator_scanned_count: usize,
    pub db_locator_limit_reached: bool,
    pub storage: Vec<DiagnosticsStorageInventoryBucket>,
}

#[derive(Debug, Clone)]
struct ResolvedStorageInventoryParams {
    storage_types: Vec<StorageType>,
    prefix: Option<String>,
    object_sample_limit: usize,
    missing_locator_sample_limit: usize,
    object_scan_limit: usize,
    db_locator_limit: usize,
}

#[derive(Debug, Clone)]
struct DbStorageLocator {
    locator_kind: DiagnosticsStorageLocatorKind,
    request_log_id: Option<i64>,
    replay_run_id: Option<i64>,
    storage_type: StorageType,
    storage_key: String,
    artifact_version: Option<i32>,
    bundle_version: Option<i32>,
    created_at: i64,
}

pub async fn preview_storage_inventory(
    params: DiagnosticsStorageInventoryParams,
) -> Result<DiagnosticsStorageInventoryResponse, BaseError> {
    let resolved = ResolvedStorageInventoryParams::new(params);
    let (locators, db_locator_limit_reached) = load_db_locators(resolved.db_locator_limit)?;
    let db_locator_scanned_count = locators.len();
    let mut storage = Vec::with_capacity(resolved.storage_types.len());

    for storage_type in &resolved.storage_types {
        storage.push(inspect_storage(storage_type.clone(), &resolved, &locators).await);
    }

    Ok(DiagnosticsStorageInventoryResponse {
        prefix: resolved.prefix,
        object_sample_limit: resolved.object_sample_limit,
        missing_locator_sample_limit: resolved.missing_locator_sample_limit,
        object_scan_limit: resolved.object_scan_limit,
        db_locator_limit: resolved.db_locator_limit,
        db_locator_scanned_count,
        db_locator_limit_reached,
        storage,
    })
}

impl ResolvedStorageInventoryParams {
    fn new(params: DiagnosticsStorageInventoryParams) -> Self {
        Self {
            storage_types: params
                .storage_types
                .filter(|storage_types| !storage_types.is_empty())
                .unwrap_or_else(|| vec![StorageType::FileSystem, StorageType::S3]),
            prefix: params.prefix.filter(|prefix| !prefix.is_empty()),
            object_sample_limit: bounded_limit(
                params.object_sample_limit,
                DEFAULT_OBJECT_SAMPLE_LIMIT,
                MAX_SAMPLE_LIMIT,
            ),
            missing_locator_sample_limit: bounded_limit(
                params.missing_locator_sample_limit,
                DEFAULT_MISSING_LOCATOR_SAMPLE_LIMIT,
                MAX_SAMPLE_LIMIT,
            ),
            object_scan_limit: bounded_limit(
                params.object_scan_limit,
                DEFAULT_OBJECT_SCAN_LIMIT,
                MAX_OBJECT_SCAN_LIMIT,
            ),
            db_locator_limit: bounded_limit(
                params.db_locator_limit,
                DEFAULT_DB_LOCATOR_LIMIT,
                MAX_DB_LOCATOR_LIMIT,
            ),
        }
    }
}

fn bounded_limit(value: Option<usize>, default_value: usize, max_value: usize) -> usize {
    value.unwrap_or(default_value).clamp(1, max_value)
}

fn load_db_locators(limit: usize) -> Result<(Vec<DbStorageLocator>, bool), BaseError> {
    let db_limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let request_log_bundles = RequestLog::list_bundle_storage_locators(db_limit)?;
    let replay_artifacts = RequestReplayRun::list_artifact_storage_locators(db_limit)?;
    let limit_reached = request_log_bundles.len() >= limit || replay_artifacts.len() >= limit;
    let mut locators = Vec::with_capacity(request_log_bundles.len() + replay_artifacts.len());
    locators.extend(
        request_log_bundles
            .into_iter()
            .map(request_log_bundle_locator),
    );
    locators.extend(replay_artifacts.into_iter().map(replay_artifact_locator));
    Ok((locators, limit_reached))
}

async fn inspect_storage(
    storage_type: StorageType,
    params: &ResolvedStorageInventoryParams,
    db_locators: &[DbStorageLocator],
) -> DiagnosticsStorageInventoryBucket {
    let prefix = params.prefix.as_deref();
    let matching_locators: Vec<&DbStorageLocator> = db_locators
        .iter()
        .filter(|locator| locator.storage_type == storage_type)
        .filter(|locator| key_matches_prefix(&locator.storage_key, prefix))
        .collect();
    let db_locator_keys: HashSet<&str> = matching_locators
        .iter()
        .map(|locator| locator.storage_key.as_str())
        .collect();

    let storage = match storage_for_type(&storage_type).await {
        StorageInventoryTarget::Available(storage) => storage,
        StorageInventoryTarget::Unavailable { status, message } => {
            return empty_bucket(
                storage_type,
                params.prefix.clone(),
                params.object_scan_limit,
                status,
                Some(message),
            );
        }
    };

    let object_list = match storage
        .list_objects(Some(ListObjectOptions {
            prefix,
            limit: Some(params.object_scan_limit),
        }))
        .await
    {
        Ok(objects) => objects,
        Err(StorageError::Unsupported(message)) => {
            return empty_bucket(
                storage_type,
                params.prefix.clone(),
                params.object_scan_limit,
                DiagnosticsStorageInventoryStatus::Skipped,
                Some(message),
            );
        }
        Err(error) => {
            return empty_bucket(
                storage_type,
                params.prefix.clone(),
                params.object_scan_limit,
                DiagnosticsStorageInventoryStatus::Failed,
                Some(format!("Failed to list storage objects: {}", error)),
            );
        }
    };
    let object_limit_reached = object_list.limit_reached;
    let objects = object_list.objects;

    let object_key_set: HashSet<&str> = objects.iter().map(|object| object.key.as_str()).collect();
    let object_count = objects.len();
    let total_size_bytes = objects.iter().filter_map(|object| object.size_bytes).sum();
    let unknown_size_object_count = objects
        .iter()
        .filter(|object| object.size_bytes.is_none())
        .count();
    let referenced_object_count = objects
        .iter()
        .filter(|object| db_locator_keys.contains(object.key.as_str()))
        .count();
    let unreferenced_objects: Vec<&StorageObjectMetadata> = objects
        .iter()
        .filter(|object| !db_locator_keys.contains(object.key.as_str()))
        .collect();

    let mut missing_locator_count = 0;
    let mut locator_check_failed_count = 0;
    let mut missing_locator_samples = Vec::new();
    let mut locator_check_failure_messages = Vec::new();
    for locator in matching_locators {
        if object_key_set.contains(locator.storage_key.as_str()) {
            continue;
        }

        match storage.get_object_metadata(&locator.storage_key).await {
            Ok(_) => {}
            Err(StorageError::NotFound) => {
                missing_locator_count += 1;
                if missing_locator_samples.len() < params.missing_locator_sample_limit {
                    missing_locator_samples.push(missing_locator_sample(
                        locator,
                        Some("object not found".to_string()),
                    ));
                }
            }
            Err(StorageError::Unsupported(message)) => {
                return empty_bucket(
                    storage_type,
                    params.prefix.clone(),
                    params.object_scan_limit,
                    DiagnosticsStorageInventoryStatus::Skipped,
                    Some(message),
                );
            }
            Err(error) => {
                locator_check_failed_count += 1;
                if locator_check_failure_messages.len() < 3 {
                    locator_check_failure_messages
                        .push(format!("{}: {}", locator.storage_key, error));
                }
            }
        }
    }

    let mut message_parts = Vec::new();
    if object_limit_reached {
        message_parts.push(format!(
            "Object scan limit reached at {}; object counts and samples only include scanned objects",
            params.object_scan_limit
        ));
    }
    if locator_check_failed_count > 0 {
        if locator_check_failure_messages.is_empty() {
            message_parts.push(format!(
                "{} DB locator metadata checks failed",
                locator_check_failed_count
            ));
        } else {
            message_parts.push(format!(
                "{} DB locator metadata checks failed; sample errors: {}",
                locator_check_failed_count,
                locator_check_failure_messages.join("; ")
            ));
        }
    }
    let message = (!message_parts.is_empty()).then(|| message_parts.join("; "));

    DiagnosticsStorageInventoryBucket {
        storage_type,
        status: DiagnosticsStorageInventoryStatus::Available,
        message,
        prefix: params.prefix.clone(),
        object_scan_limit: params.object_scan_limit,
        object_limit_reached,
        object_count,
        total_size_bytes,
        unknown_size_object_count,
        referenced_object_count,
        unreferenced_object_count: unreferenced_objects.len(),
        missing_locator_count,
        locator_check_failed_count,
        object_samples: object_samples(objects.iter(), params.object_sample_limit),
        unreferenced_samples: object_samples(
            unreferenced_objects.into_iter(),
            params.object_sample_limit,
        ),
        missing_locator_samples,
    }
}

enum StorageInventoryTarget {
    Available(&'static dyn Storage),
    Unavailable {
        status: DiagnosticsStorageInventoryStatus,
        message: String,
    },
}

async fn storage_for_type(storage_type: &StorageType) -> StorageInventoryTarget {
    match storage_type {
        StorageType::FileSystem => StorageInventoryTarget::Available(get_local_storage().await),
        StorageType::S3 => match get_s3_storage_result().await {
            Ok(Some(storage)) => StorageInventoryTarget::Available(storage),
            Ok(None) => StorageInventoryTarget::Unavailable {
                status: DiagnosticsStorageInventoryStatus::Skipped,
                message: "S3 storage is not configured".to_string(),
            },
            Err(error) => StorageInventoryTarget::Unavailable {
                status: DiagnosticsStorageInventoryStatus::Failed,
                message: format!("S3 storage is not available: {}", error),
            },
        },
    }
}

fn object_samples<'a>(
    objects: impl Iterator<Item = &'a StorageObjectMetadata>,
    limit: usize,
) -> Vec<DiagnosticsStorageObjectSample> {
    objects
        .take(limit)
        .map(|object| DiagnosticsStorageObjectSample {
            key: object.key.clone(),
            size_bytes: object.size_bytes,
            last_modified_ms: object.last_modified_ms,
        })
        .collect()
}

fn key_matches_prefix(key: &str, prefix: Option<&str>) -> bool {
    prefix.is_none_or(|prefix| key.starts_with(prefix))
}

fn empty_bucket(
    storage_type: StorageType,
    prefix: Option<String>,
    object_scan_limit: usize,
    status: DiagnosticsStorageInventoryStatus,
    message: Option<String>,
) -> DiagnosticsStorageInventoryBucket {
    DiagnosticsStorageInventoryBucket {
        storage_type,
        status,
        message,
        prefix,
        object_scan_limit,
        object_limit_reached: false,
        object_count: 0,
        total_size_bytes: 0,
        unknown_size_object_count: 0,
        referenced_object_count: 0,
        unreferenced_object_count: 0,
        missing_locator_count: 0,
        locator_check_failed_count: 0,
        object_samples: Vec::new(),
        unreferenced_samples: Vec::new(),
        missing_locator_samples: Vec::new(),
    }
}

fn request_log_bundle_locator(record: RequestLogBundleRetentionRecord) -> DbStorageLocator {
    DbStorageLocator {
        locator_kind: DiagnosticsStorageLocatorKind::RequestLogBundle,
        request_log_id: Some(record.id),
        replay_run_id: None,
        storage_type: record.bundle_storage_type,
        storage_key: record.bundle_storage_key,
        artifact_version: None,
        bundle_version: record.bundle_version,
        created_at: record.created_at,
    }
}

fn replay_artifact_locator(record: RequestReplayArtifactRetentionRecord) -> DbStorageLocator {
    DbStorageLocator {
        locator_kind: DiagnosticsStorageLocatorKind::ReplayArtifact,
        request_log_id: Some(record.source_request_log_id),
        replay_run_id: Some(record.id),
        storage_type: record.artifact_storage_type,
        storage_key: record.artifact_storage_key,
        artifact_version: Some(record.artifact_version),
        bundle_version: None,
        created_at: record.created_at,
    }
}

fn missing_locator_sample(
    locator: &DbStorageLocator,
    message: Option<String>,
) -> DiagnosticsStorageMissingLocatorSample {
    DiagnosticsStorageMissingLocatorSample {
        locator_kind: locator.locator_kind.clone(),
        request_log_id: locator.request_log_id,
        replay_run_id: locator.replay_run_id,
        storage_type: locator.storage_type.clone(),
        storage_key: locator.storage_key.clone(),
        artifact_version: locator.artifact_version,
        bundle_version: locator.bundle_version,
        created_at: locator.created_at,
        message,
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use diesel::connection::SimpleConnection;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::{
        database::{DbConnection, TestDbContext, get_connection},
        service::storage::{
            Storage, get_local_storage, get_s3_storage_result, s3::S3Storage,
            types::ListObjectOptions,
        },
    };

    fn seed_inventory_rows(prefix: &str) {
        seed_inventory_rows_for_storage(prefix, "FILE_SYSTEM");
    }

    fn seed_inventory_rows_for_storage(prefix: &str, storage_type: &str) {
        let missing_bundle_key = format!("{prefix}missing-bundle.msgpack.gz");
        let existing_bundle_key = format!("{prefix}existing-bundle.msgpack.gz");
        let missing_replay_key = format!("{prefix}missing-replay.msgpack.gz");
        let mut conn = get_connection().expect("test db connection");
        let sql = format!(
            "INSERT INTO api_key (
                id, api_key, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm, max_concurrent_requests,
                quota_daily_requests, quota_daily_tokens, quota_monthly_tokens,
                budget_daily_nanos, budget_daily_currency, budget_monthly_nanos,
                budget_monthly_currency, deleted_at, created_at, updated_at
            ) VALUES (
                1, 'ck-test', 'hash', 'ck-test', 'test', 'Test key', NULL,
                'ALLOW', 1, NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, 1, 1
            );

            INSERT INTO request_log (
                id, api_key_id, user_api_type, overall_status, attempt_count,
                retry_count, fallback_count, request_received_at, is_stream,
                has_transform_diagnostics, transform_diagnostic_count,
                bundle_version, bundle_storage_type, bundle_storage_key,
                created_at, updated_at
            ) VALUES
            (
                10, 1, 'OPENAI', 'SUCCESS', 1,
                0, 0, 100, 0,
                0, 0,
                2, '{storage_type}', '{missing_bundle_key}',
                100, 100
            ),
            (
                11, 1, 'OPENAI', 'SUCCESS', 1,
                0, 0, 100, 0,
                0, 0,
                2, '{storage_type}', '{existing_bundle_key}',
                100, 100
            );

            INSERT INTO request_replay_run (
                id, source_request_log_id, source_attempt_id, replay_kind, replay_mode,
                semantic_basis, status, artifact_version, artifact_storage_type,
                artifact_storage_key, created_at, updated_at
            ) VALUES (
                201, 11, NULL, 'GATEWAY_REQUEST', 'DRY_RUN',
                'HISTORICAL_REQUEST_SNAPSHOT_WITH_CURRENT_CONFIG', 'SUCCESS',
                1, '{storage_type}', '{missing_replay_key}', 100, 100
            );"
        );

        match &mut conn {
            DbConnection::Sqlite(conn) => conn.batch_execute(&sql).expect("seed sqlite rows"),
            DbConnection::Postgres(conn) => conn.batch_execute(&sql).expect("seed postgres rows"),
        }
    }

    fn local_params(prefix: &str) -> DiagnosticsStorageInventoryParams {
        DiagnosticsStorageInventoryParams {
            storage_types: Some(vec![StorageType::FileSystem]),
            prefix: Some(prefix.to_string()),
            object_sample_limit: Some(10),
            missing_locator_sample_limit: Some(10),
            object_scan_limit: Some(100),
            db_locator_limit: Some(100),
        }
    }

    fn s3_params(prefix: &str) -> DiagnosticsStorageInventoryParams {
        DiagnosticsStorageInventoryParams {
            storage_types: Some(vec![StorageType::S3]),
            prefix: Some(prefix.to_string()),
            object_sample_limit: Some(10),
            missing_locator_sample_limit: Some(10),
            object_scan_limit: Some(100),
            db_locator_limit: Some(100),
        }
    }

    fn unique_inventory_prefix(name: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        format!("inventory/{name}-{nanos}/")
    }

    async fn get_reachable_s3_storage() -> Option<&'static S3Storage> {
        let storage = match get_s3_storage_result().await {
            Ok(Some(storage)) => storage,
            Ok(None) => {
                println!("Skipping real S3 inventory test: S3 is not configured.");
                return None;
            }
            Err(error) => {
                println!("Skipping real S3 inventory test: {error}");
                return None;
            }
        };

        let probe_key = format!(
            "{}probe.msgpack.gz",
            unique_inventory_prefix("real-s3-probe")
        );
        match storage
            .put_object(&probe_key, Bytes::from_static(b"s3-inventory-probe"), None)
            .await
        {
            Ok(()) => {
                let _ = storage.delete_object(&probe_key).await;
                Some(storage)
            }
            Err(error) => {
                println!("Skipping real S3 inventory test: S3 endpoint is not reachable: {error}");
                None
            }
        }
    }

    #[tokio::test]
    async fn storage_inventory_detects_local_orphans_and_missing_db_locators() {
        let db = TestDbContext::new_sqlite("diagnostics-storage-inventory.sqlite");
        db.run_async(async {
            let prefix = "inventory/local-orphans-and-missing/";
            seed_inventory_rows(prefix);
            let storage = get_local_storage().await;
            let existing_bundle_key = format!("{prefix}existing-bundle.msgpack.gz");
            let orphan_key = format!("{prefix}orphan-object.msgpack.gz");
            storage
                .put_object(&existing_bundle_key, Bytes::from_static(b"bundle"), None)
                .await
                .expect("existing bundle object should write");
            storage
                .put_object(&orphan_key, Bytes::from_static(b"orphan"), None)
                .await
                .expect("orphan object should write");

            let listed = storage
                .list_objects(Some(ListObjectOptions {
                    prefix: Some(prefix),
                    limit: None,
                }))
                .await
                .expect("local storage inventory should list objects");
            assert_eq!(listed.objects.len(), 2);
            assert!(!listed.limit_reached);
            let existing_metadata = storage
                .get_object_metadata(&existing_bundle_key)
                .await
                .expect("local storage metadata should read");
            assert_eq!(existing_metadata.size_bytes, Some(6));

            let response = preview_storage_inventory(local_params(prefix))
                .await
                .expect("inventory preview should succeed");
            let bucket = response
                .storage
                .first()
                .expect("filesystem bucket should be returned");

            assert_eq!(bucket.status, DiagnosticsStorageInventoryStatus::Available);
            assert_eq!(bucket.object_count, 2);
            assert_eq!(bucket.referenced_object_count, 1);
            assert_eq!(bucket.unreferenced_object_count, 1);
            assert_eq!(bucket.missing_locator_count, 2);
            assert_eq!(bucket.locator_check_failed_count, 0);
            assert_eq!(bucket.unreferenced_samples[0].key, orphan_key);
            assert!(
                bucket
                    .missing_locator_samples
                    .iter()
                    .any(|sample| sample.locator_kind
                        == DiagnosticsStorageLocatorKind::RequestLogBundle
                        && sample.request_log_id == Some(10))
            );
            assert!(
                bucket
                    .missing_locator_samples
                    .iter()
                    .any(|sample| sample.locator_kind
                        == DiagnosticsStorageLocatorKind::ReplayArtifact
                        && sample.replay_run_id == Some(201))
            );
        })
        .await;
    }

    #[tokio::test]
    async fn storage_inventory_limits_local_object_scan_without_hiding_missing_locators() {
        let db = TestDbContext::new_sqlite("diagnostics-storage-inventory-limit.sqlite");
        db.run_async(async {
            let prefix = unique_inventory_prefix("local-scan-limit");
            seed_inventory_rows(&prefix);
            let storage = get_local_storage().await;
            let existing_bundle_key = format!("{prefix}existing-bundle.msgpack.gz");
            let orphan_a_key = format!("{prefix}orphan-a.msgpack.gz");
            let orphan_b_key = format!("{prefix}orphan-b.msgpack.gz");
            storage
                .put_object(&existing_bundle_key, Bytes::from_static(b"bundle"), None)
                .await
                .expect("existing bundle object should write");
            storage
                .put_object(&orphan_a_key, Bytes::from_static(b"orphan-a"), None)
                .await
                .expect("first orphan object should write");
            storage
                .put_object(&orphan_b_key, Bytes::from_static(b"orphan-b"), None)
                .await
                .expect("second orphan object should write");

            let direct_list = storage
                .list_objects(Some(ListObjectOptions {
                    prefix: Some(&prefix),
                    limit: Some(2),
                }))
                .await
                .expect("local storage inventory should honor list limit");
            assert_eq!(direct_list.objects.len(), 2);
            assert!(direct_list.limit_reached);

            let mut params = local_params(&prefix);
            params.object_sample_limit = Some(1);
            params.object_scan_limit = Some(2);
            let response = preview_storage_inventory(params)
                .await
                .expect("inventory preview should succeed");
            let bucket = response
                .storage
                .first()
                .expect("filesystem bucket should be returned");

            assert_eq!(response.object_scan_limit, 2);
            assert_eq!(bucket.object_scan_limit, 2);
            assert!(bucket.object_limit_reached);
            assert_eq!(bucket.object_count, 2);
            assert_eq!(bucket.object_samples.len(), 1);
            assert_eq!(bucket.unreferenced_samples.len(), 1);
            assert_eq!(bucket.missing_locator_count, 2);
            assert_eq!(bucket.locator_check_failed_count, 0);
            assert!(
                bucket
                    .message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("Object scan limit reached")
            );
            assert!(
                bucket
                    .missing_locator_samples
                    .iter()
                    .any(|sample| sample.locator_kind
                        == DiagnosticsStorageLocatorKind::RequestLogBundle
                        && sample.request_log_id == Some(10))
            );
            assert!(
                bucket
                    .missing_locator_samples
                    .iter()
                    .any(|sample| sample.locator_kind
                        == DiagnosticsStorageLocatorKind::ReplayArtifact
                        && sample.replay_run_id == Some(201))
            );
        })
        .await;
    }

    #[tokio::test]
    async fn storage_inventory_skips_unconfigured_s3_without_panicking() {
        if crate::config::CONFIG.storage.s3.is_some() {
            println!("Skipping unconfigured S3 inventory test: S3 is configured.");
            return;
        }

        let db = TestDbContext::new_sqlite("diagnostics-storage-inventory-s3.sqlite");
        db.run_async(async {
            let response = preview_storage_inventory(DiagnosticsStorageInventoryParams {
                storage_types: Some(vec![StorageType::S3]),
                prefix: Some("inventory/s3-skipped/".to_string()),
                object_sample_limit: Some(5),
                missing_locator_sample_limit: Some(5),
                object_scan_limit: Some(5),
                db_locator_limit: Some(5),
            })
            .await
            .expect("inventory preview should not require S3");
            let bucket = response
                .storage
                .first()
                .expect("S3 bucket should be returned");

            assert_eq!(bucket.status, DiagnosticsStorageInventoryStatus::Skipped);
            assert!(
                bucket
                    .message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("S3 storage")
            );
            assert_eq!(bucket.object_count, 0);
        })
        .await;
    }

    #[tokio::test]
    async fn storage_inventory_checks_real_s3_when_configured() {
        let storage = match get_reachable_s3_storage().await {
            Some(storage) => storage,
            None => return,
        };

        let db = TestDbContext::new_sqlite("diagnostics-storage-inventory-real-s3.sqlite");
        db.run_async(async {
            let prefix = unique_inventory_prefix("real-s3");
            seed_inventory_rows_for_storage(&prefix, "S3");
            let existing_bundle_key = format!("{prefix}existing-bundle.msgpack.gz");
            let orphan_key = format!("{prefix}orphan-object.msgpack.gz");
            storage
                .put_object(&existing_bundle_key, Bytes::from_static(b"bundle"), None)
                .await
                .expect("existing S3 bundle object should write");
            storage
                .put_object(&orphan_key, Bytes::from_static(b"orphan"), None)
                .await
                .expect("orphan S3 object should write");

            let listed = storage
                .list_objects(Some(ListObjectOptions {
                    prefix: Some(&prefix),
                    limit: None,
                }))
                .await
                .expect("S3 inventory should list objects");
            assert!(
                listed
                    .objects
                    .iter()
                    .any(|object| object.key == existing_bundle_key)
            );
            assert!(listed.objects.iter().any(|object| object.key == orphan_key));
            assert!(!listed.limit_reached);
            let existing_metadata = storage
                .get_object_metadata(&existing_bundle_key)
                .await
                .expect("S3 metadata should read");
            assert_eq!(existing_metadata.size_bytes, Some(6));

            let response = preview_storage_inventory(s3_params(&prefix))
                .await
                .expect("S3 inventory preview should succeed");
            let bucket = response
                .storage
                .first()
                .expect("S3 bucket should be returned");

            assert_eq!(bucket.status, DiagnosticsStorageInventoryStatus::Available);
            assert_eq!(bucket.object_count, 2);
            assert_eq!(bucket.referenced_object_count, 1);
            assert_eq!(bucket.unreferenced_object_count, 1);
            assert_eq!(bucket.missing_locator_count, 2);
            assert_eq!(bucket.locator_check_failed_count, 0);
            assert_eq!(bucket.unreferenced_samples[0].key, orphan_key);
            assert!(
                bucket
                    .missing_locator_samples
                    .iter()
                    .any(|sample| sample.locator_kind
                        == DiagnosticsStorageLocatorKind::RequestLogBundle
                        && sample.request_log_id == Some(10))
            );
            assert!(
                bucket
                    .missing_locator_samples
                    .iter()
                    .any(|sample| sample.locator_kind
                        == DiagnosticsStorageLocatorKind::ReplayArtifact
                        && sample.replay_run_id == Some(201))
            );

            let limited_list = storage
                .list_objects(Some(ListObjectOptions {
                    prefix: Some(&prefix),
                    limit: Some(1),
                }))
                .await
                .expect("S3 inventory should honor list limit");
            assert_eq!(limited_list.objects.len(), 1);
            assert!(limited_list.limit_reached);

            let mut limited_params = s3_params(&prefix);
            limited_params.object_scan_limit = Some(1);
            let limited_response = preview_storage_inventory(limited_params)
                .await
                .expect("limited S3 inventory preview should succeed");
            let limited_bucket = limited_response
                .storage
                .first()
                .expect("limited S3 bucket should be returned");
            assert_eq!(limited_bucket.object_scan_limit, 1);
            assert!(limited_bucket.object_limit_reached);
            assert_eq!(limited_bucket.object_count, 1);
            assert_eq!(limited_bucket.missing_locator_count, 2);

            storage
                .delete_object(&existing_bundle_key)
                .await
                .expect("existing S3 bundle object should delete");
            storage
                .delete_object(&orphan_key)
                .await
                .expect("orphan S3 object should delete");
        })
        .await;
    }
}
