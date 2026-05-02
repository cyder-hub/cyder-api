use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    controller::BaseError,
    database::{
        request_log::{RequestLog, RequestLogBundleRetentionRecord},
        request_replay_run::{RequestReplayArtifactRetentionRecord, RequestReplayRun},
    },
    schema::enum_def::StorageType,
    service::{
        diagnostics::policy::DiagnosticsPolicy,
        storage::{Storage, get_local_storage, get_s3_storage_result},
    },
};

const MILLIS_PER_DAY: u64 = 24 * 60 * 60 * 1000;
const RETENTION_STORAGE_KEY_SAMPLE_LIMIT: usize = 20;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DiagnosticsRetentionParams {
    pub request_log_bundle_retention_days: Option<u64>,
    pub replay_artifact_retention_days: Option<u64>,
    pub delete_batch_size: Option<usize>,
    pub include_request_log_bundles: Option<bool>,
    pub include_replay_artifacts: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsRetentionItemStatus {
    Candidate,
    Deleted,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsRetentionItem {
    pub id: i64,
    pub request_log_id: Option<i64>,
    pub replay_run_id: Option<i64>,
    pub storage_type: StorageType,
    pub storage_key: String,
    pub artifact_version: Option<i32>,
    pub bundle_version: Option<i32>,
    pub created_at: i64,
    pub status: DiagnosticsRetentionItemStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsRetentionStorageTypeCount {
    pub storage_type: StorageType,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsRetentionBucket {
    pub retention_days: u64,
    pub cutoff_created_before: i64,
    pub candidate_count: usize,
    pub storage_type_counts: Vec<DiagnosticsRetentionStorageTypeCount>,
    pub oldest_created_at: Option<i64>,
    pub newest_created_at: Option<i64>,
    pub sample_storage_keys: Vec<String>,
    pub succeeded_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub items: Vec<DiagnosticsRetentionItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsRetentionResponse {
    pub enabled: bool,
    pub executed: bool,
    pub now_ms: i64,
    pub delete_batch_size: usize,
    pub request_log_bundles: DiagnosticsRetentionBucket,
    pub replay_artifacts: DiagnosticsRetentionBucket,
}

pub fn preview_retention(
    params: DiagnosticsRetentionParams,
    policy: &DiagnosticsPolicy,
) -> Result<DiagnosticsRetentionResponse, BaseError> {
    preview_retention_at(params, Utc::now().timestamp_millis(), policy)
}

pub async fn execute_retention(
    params: DiagnosticsRetentionParams,
    policy: &DiagnosticsPolicy,
) -> Result<DiagnosticsRetentionResponse, BaseError> {
    execute_retention_at(params, Utc::now().timestamp_millis(), policy).await
}

fn preview_retention_at(
    params: DiagnosticsRetentionParams,
    now_ms: i64,
    policy: &DiagnosticsPolicy,
) -> Result<DiagnosticsRetentionResponse, BaseError> {
    let resolved = ResolvedRetentionParams::new(params, now_ms, policy);
    let request_log_bundles = if resolved.include_request_log_bundles {
        preview_request_log_bundles(&resolved)?
    } else {
        empty_bucket(
            resolved.request_log_bundle_retention_days,
            resolved.request_log_bundle_cutoff,
        )
    };
    let replay_artifacts = if resolved.include_replay_artifacts {
        preview_replay_artifacts(&resolved)?
    } else {
        empty_bucket(
            resolved.replay_artifact_retention_days,
            resolved.replay_artifact_cutoff,
        )
    };

    Ok(DiagnosticsRetentionResponse {
        enabled: policy.retention_enabled(),
        executed: false,
        now_ms,
        delete_batch_size: resolved.delete_batch_size,
        request_log_bundles,
        replay_artifacts,
    })
}

async fn execute_retention_at(
    params: DiagnosticsRetentionParams,
    now_ms: i64,
    policy: &DiagnosticsPolicy,
) -> Result<DiagnosticsRetentionResponse, BaseError> {
    let storage_resolver = DefaultRetentionStorageResolver;
    execute_retention_at_with_storage_resolver(params, now_ms, policy, &storage_resolver).await
}

async fn execute_retention_at_with_storage_resolver<R>(
    params: DiagnosticsRetentionParams,
    now_ms: i64,
    policy: &DiagnosticsPolicy,
    storage_resolver: &R,
) -> Result<DiagnosticsRetentionResponse, BaseError>
where
    R: RetentionStorageResolver + Sync,
{
    if !policy.retention_enabled() {
        return Err(BaseError::ParamInvalid(Some(
            "Diagnostics retention execute is disabled by diagnostics.retention.enabled"
                .to_string(),
        )));
    }

    let resolved = ResolvedRetentionParams::new(params, now_ms, policy);
    let request_log_bundles = if resolved.include_request_log_bundles {
        execute_request_log_bundle_retention(&resolved, storage_resolver).await?
    } else {
        empty_bucket(
            resolved.request_log_bundle_retention_days,
            resolved.request_log_bundle_cutoff,
        )
    };
    let replay_artifacts = if resolved.include_replay_artifacts {
        execute_replay_artifact_retention(&resolved, storage_resolver).await?
    } else {
        empty_bucket(
            resolved.replay_artifact_retention_days,
            resolved.replay_artifact_cutoff,
        )
    };

    Ok(DiagnosticsRetentionResponse {
        enabled: policy.retention_enabled(),
        executed: true,
        now_ms,
        delete_batch_size: resolved.delete_batch_size,
        request_log_bundles,
        replay_artifacts,
    })
}

#[derive(Debug, Clone)]
struct ResolvedRetentionParams {
    now_ms: i64,
    request_log_bundle_retention_days: u64,
    replay_artifact_retention_days: u64,
    request_log_bundle_cutoff: i64,
    replay_artifact_cutoff: i64,
    delete_batch_size: usize,
    include_request_log_bundles: bool,
    include_replay_artifacts: bool,
}

impl ResolvedRetentionParams {
    fn new(params: DiagnosticsRetentionParams, now_ms: i64, policy: &DiagnosticsPolicy) -> Self {
        let request_log_bundle_retention_days = params
            .request_log_bundle_retention_days
            .unwrap_or_else(|| policy.request_log_bundle_retention_days());
        let replay_artifact_retention_days = params
            .replay_artifact_retention_days
            .unwrap_or_else(|| policy.replay_artifact_retention_days());
        let delete_batch_size = params
            .delete_batch_size
            .unwrap_or_else(|| policy.retention_delete_batch_size())
            .max(1);

        Self {
            now_ms,
            request_log_bundle_retention_days,
            replay_artifact_retention_days,
            request_log_bundle_cutoff: cutoff_for_days(now_ms, request_log_bundle_retention_days),
            replay_artifact_cutoff: cutoff_for_days(now_ms, replay_artifact_retention_days),
            delete_batch_size,
            include_request_log_bundles: params.include_request_log_bundles.unwrap_or(true),
            include_replay_artifacts: params.include_replay_artifacts.unwrap_or(true),
        }
    }

    fn limit(&self) -> i64 {
        i64::try_from(self.delete_batch_size).unwrap_or(i64::MAX)
    }
}

fn cutoff_for_days(now_ms: i64, days: u64) -> i64 {
    let retention_ms = days.saturating_mul(MILLIS_PER_DAY);
    now_ms.saturating_sub(i64::try_from(retention_ms).unwrap_or(i64::MAX))
}

fn preview_request_log_bundles(
    params: &ResolvedRetentionParams,
) -> Result<DiagnosticsRetentionBucket, BaseError> {
    let candidates = RequestLog::list_bundle_retention_candidates(
        params.request_log_bundle_cutoff,
        params.limit(),
    )?;
    Ok(bucket_from_items(
        params.request_log_bundle_retention_days,
        params.request_log_bundle_cutoff,
        candidates
            .into_iter()
            .map(request_log_bundle_candidate_item)
            .collect(),
    ))
}

fn preview_replay_artifacts(
    params: &ResolvedRetentionParams,
) -> Result<DiagnosticsRetentionBucket, BaseError> {
    let candidates = RequestReplayRun::list_artifact_retention_candidates(
        params.replay_artifact_cutoff,
        params.limit(),
    )?;
    Ok(bucket_from_items(
        params.replay_artifact_retention_days,
        params.replay_artifact_cutoff,
        candidates
            .into_iter()
            .map(replay_artifact_candidate_item)
            .collect(),
    ))
}

async fn execute_request_log_bundle_retention(
    params: &ResolvedRetentionParams,
    storage_resolver: &(impl RetentionStorageResolver + Sync),
) -> Result<DiagnosticsRetentionBucket, BaseError> {
    let candidates = RequestLog::list_bundle_retention_candidates(
        params.request_log_bundle_cutoff,
        params.limit(),
    )?;
    let mut items = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        items.push(delete_request_log_bundle(candidate, params.now_ms, storage_resolver).await);
    }

    Ok(bucket_from_items(
        params.request_log_bundle_retention_days,
        params.request_log_bundle_cutoff,
        items,
    ))
}

async fn execute_replay_artifact_retention(
    params: &ResolvedRetentionParams,
    storage_resolver: &(impl RetentionStorageResolver + Sync),
) -> Result<DiagnosticsRetentionBucket, BaseError> {
    let candidates = RequestReplayRun::list_artifact_retention_candidates(
        params.replay_artifact_cutoff,
        params.limit(),
    )?;
    let mut items = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        items.push(delete_replay_artifact(candidate, params.now_ms, storage_resolver).await);
    }

    Ok(bucket_from_items(
        params.replay_artifact_retention_days,
        params.replay_artifact_cutoff,
        items,
    ))
}

async fn delete_request_log_bundle(
    candidate: RequestLogBundleRetentionRecord,
    now_ms: i64,
    storage_resolver: &(impl RetentionStorageResolver + Sync),
) -> DiagnosticsRetentionItem {
    let mut item = request_log_bundle_candidate_item(candidate);
    if let Err(message) =
        delete_storage_object(&item.storage_type, &item.storage_key, storage_resolver).await
    {
        item.status = DiagnosticsRetentionItemStatus::Failed;
        item.message = Some(message);
        return item;
    }

    let log_id = item.id;
    match RequestLog::clear_bundle_locator_if_matches(
        log_id,
        item.storage_type.clone(),
        &item.storage_key,
        now_ms,
    ) {
        Ok(true) => {
            item.status = DiagnosticsRetentionItemStatus::Deleted;
            item.message = Some("request log bundle deleted and locator cleared".to_string());
        }
        Ok(false) => {
            item.status = DiagnosticsRetentionItemStatus::Skipped;
            item.message = Some("request log bundle locator changed before cleanup".to_string());
        }
        Err(err) => {
            item.status = DiagnosticsRetentionItemStatus::Failed;
            item.message = Some(base_error_message(err));
        }
    }
    item
}

async fn delete_replay_artifact(
    candidate: RequestReplayArtifactRetentionRecord,
    now_ms: i64,
    storage_resolver: &(impl RetentionStorageResolver + Sync),
) -> DiagnosticsRetentionItem {
    let mut item = replay_artifact_candidate_item(candidate);
    if let Err(message) =
        delete_storage_object(&item.storage_type, &item.storage_key, storage_resolver).await
    {
        item.status = DiagnosticsRetentionItemStatus::Failed;
        item.message = Some(message);
        return item;
    }

    let Some(replay_run_id) = item.replay_run_id else {
        item.status = DiagnosticsRetentionItemStatus::Failed;
        item.message = Some("replay artifact candidate missing replay_run_id".to_string());
        return item;
    };

    match RequestReplayRun::clear_artifact_locator_if_matches(
        replay_run_id,
        item.storage_type.clone(),
        &item.storage_key,
        now_ms,
    ) {
        Ok(true) => {
            item.status = DiagnosticsRetentionItemStatus::Deleted;
            item.message = Some("replay artifact deleted and locator cleared".to_string());
        }
        Ok(false) => {
            item.status = DiagnosticsRetentionItemStatus::Skipped;
            item.message = Some("replay artifact locator changed before cleanup".to_string());
        }
        Err(err) => {
            item.status = DiagnosticsRetentionItemStatus::Failed;
            item.message = Some(base_error_message(err));
        }
    }
    item
}

fn request_log_bundle_candidate_item(
    candidate: RequestLogBundleRetentionRecord,
) -> DiagnosticsRetentionItem {
    DiagnosticsRetentionItem {
        id: candidate.id,
        request_log_id: Some(candidate.id),
        replay_run_id: None,
        storage_type: candidate.bundle_storage_type,
        storage_key: candidate.bundle_storage_key,
        artifact_version: None,
        bundle_version: candidate.bundle_version,
        created_at: candidate.created_at,
        status: DiagnosticsRetentionItemStatus::Candidate,
        message: None,
    }
}

fn replay_artifact_candidate_item(
    candidate: RequestReplayArtifactRetentionRecord,
) -> DiagnosticsRetentionItem {
    DiagnosticsRetentionItem {
        id: candidate.id,
        request_log_id: Some(candidate.source_request_log_id),
        replay_run_id: Some(candidate.id),
        storage_type: candidate.artifact_storage_type,
        storage_key: candidate.artifact_storage_key,
        artifact_version: Some(candidate.artifact_version),
        bundle_version: None,
        created_at: candidate.created_at,
        status: DiagnosticsRetentionItemStatus::Candidate,
        message: None,
    }
}

async fn delete_storage_object(
    storage_type: &StorageType,
    key: &str,
    storage_resolver: &(impl RetentionStorageResolver + Sync),
) -> Result<(), String> {
    let storage = storage_resolver.storage_for_type(storage_type).await?;
    storage
        .delete_object(key)
        .await
        .map_err(|err| format!("Failed to delete storage object {}: {}", key, err))
}

#[async_trait]
trait RetentionStorageResolver {
    async fn storage_for_type(
        &self,
        storage_type: &StorageType,
    ) -> Result<&'static dyn Storage, String>;
}

struct DefaultRetentionStorageResolver;

#[async_trait]
impl RetentionStorageResolver for DefaultRetentionStorageResolver {
    async fn storage_for_type(
        &self,
        storage_type: &StorageType,
    ) -> Result<&'static dyn Storage, String> {
        match storage_type {
            StorageType::FileSystem => Ok(get_local_storage().await),
            StorageType::S3 => match get_s3_storage_result().await {
                Ok(Some(storage)) => Ok(storage),
                Ok(None) => Err("S3 storage is not configured".to_string()),
                Err(error) => Err(format!("S3 storage is not available: {}", error)),
            },
        }
    }
}

fn bucket_from_items(
    retention_days: u64,
    cutoff_created_before: i64,
    items: Vec<DiagnosticsRetentionItem>,
) -> DiagnosticsRetentionBucket {
    let storage_type_counts = storage_type_counts(&items);
    let oldest_created_at = items.iter().map(|item| item.created_at).min();
    let newest_created_at = items.iter().map(|item| item.created_at).max();
    let sample_storage_keys = items
        .iter()
        .take(RETENTION_STORAGE_KEY_SAMPLE_LIMIT)
        .map(|item| item.storage_key.clone())
        .collect();
    let succeeded_count = items
        .iter()
        .filter(|item| item.status == DiagnosticsRetentionItemStatus::Deleted)
        .count();
    let skipped_count = items
        .iter()
        .filter(|item| item.status == DiagnosticsRetentionItemStatus::Skipped)
        .count();
    let failed_count = items
        .iter()
        .filter(|item| item.status == DiagnosticsRetentionItemStatus::Failed)
        .count();

    DiagnosticsRetentionBucket {
        retention_days,
        cutoff_created_before,
        candidate_count: items.len(),
        storage_type_counts,
        oldest_created_at,
        newest_created_at,
        sample_storage_keys,
        succeeded_count,
        skipped_count,
        failed_count,
        items,
    }
}

fn empty_bucket(retention_days: u64, cutoff_created_before: i64) -> DiagnosticsRetentionBucket {
    bucket_from_items(retention_days, cutoff_created_before, Vec::new())
}

fn storage_type_counts(
    items: &[DiagnosticsRetentionItem],
) -> Vec<DiagnosticsRetentionStorageTypeCount> {
    let mut filesystem_count = 0;
    let mut s3_count = 0;

    for item in items {
        match &item.storage_type {
            StorageType::FileSystem => filesystem_count += 1,
            StorageType::S3 => s3_count += 1,
        }
    }

    let mut counts = Vec::with_capacity(2);
    if filesystem_count > 0 {
        counts.push(DiagnosticsRetentionStorageTypeCount {
            storage_type: StorageType::FileSystem,
            count: filesystem_count,
        });
    }
    if s3_count > 0 {
        counts.push(DiagnosticsRetentionStorageTypeCount {
            storage_type: StorageType::S3,
            count: s3_count,
        });
    }
    counts
}

fn base_error_message(error: BaseError) -> String {
    let fallback = format!("{error:?}");
    match error {
        BaseError::ParamInvalid(message)
        | BaseError::DatabaseFatal(message)
        | BaseError::DatabaseDup(message)
        | BaseError::NotFound(message)
        | BaseError::Unauthorized(message)
        | BaseError::StoreError(message)
        | BaseError::InternalServerError(message) => message.unwrap_or(fallback),
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use diesel::connection::SimpleConnection;

    use super::*;
    use crate::{
        database::{DbConnection, TestDbContext, get_connection},
        service::{
            diagnostics::replay::artifact_store::load_replay_artifact_for_run,
            storage::{Storage, get_local_storage},
        },
    };

    fn seed_retention_rows(bundle_key: &str, replay_key: &str) {
        seed_retention_rows_for_storage(bundle_key, replay_key, "FILE_SYSTEM");
    }

    fn seed_retention_rows_for_storage(bundle_key: &str, replay_key: &str, storage_type: &str) {
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
            ) VALUES (
                10, 1, 'OPENAI', 'SUCCESS', 1,
                0, 0, 100, 0,
                0, 0,
                2, '{storage_type}', '{bundle_key}',
                100, 100
            );

            INSERT INTO request_replay_run (
                id, source_request_log_id, source_attempt_id, replay_kind, replay_mode,
                semantic_basis, status, artifact_version, artifact_storage_type,
                artifact_storage_key, created_at, updated_at
            ) VALUES (
                201, 10, NULL, 'GATEWAY_REQUEST', 'DRY_RUN',
                'HISTORICAL_REQUEST_SNAPSHOT_WITH_CURRENT_CONFIG', 'SUCCESS',
                1, '{storage_type}', '{replay_key}', 100, 100
            );"
        );

        match &mut conn {
            DbConnection::Sqlite(conn) => conn.batch_execute(&sql).expect("seed sqlite rows"),
            DbConnection::Postgres(conn) => conn.batch_execute(&sql).expect("seed postgres rows"),
        }
    }

    struct FailingS3RetentionStorageResolver;

    #[async_trait]
    impl RetentionStorageResolver for FailingS3RetentionStorageResolver {
        async fn storage_for_type(
            &self,
            storage_type: &StorageType,
        ) -> Result<&'static dyn Storage, String> {
            match storage_type {
                StorageType::FileSystem => Ok(get_local_storage().await),
                StorageType::S3 => Err(
                    "S3 storage is not available: configuration error: S3 storage configuration is incomplete: missing bucket, access_key, secret_key"
                        .to_string(),
                ),
            }
        }
    }

    fn params() -> DiagnosticsRetentionParams {
        DiagnosticsRetentionParams {
            request_log_bundle_retention_days: Some(1),
            replay_artifact_retention_days: Some(1),
            delete_batch_size: Some(100),
            include_request_log_bundles: Some(true),
            include_replay_artifacts: Some(true),
        }
    }

    fn policy(enabled: bool) -> DiagnosticsPolicy {
        let mut config = crate::config::DiagnosticsConfig::default();
        config.retention.enabled = enabled;
        DiagnosticsPolicy::from_config(&config)
    }

    #[tokio::test]
    async fn retention_preview_lists_candidates_without_deleting_objects() {
        let db = TestDbContext::new_sqlite("diagnostics-retention-preview.sqlite");
        db.run_async(async {
            let bundle_key = "retention/preview-bundle.msgpack.gz";
            let replay_key = "retention/preview-replay.msgpack.gz";
            seed_retention_rows(bundle_key, replay_key);
            let storage = get_local_storage().await;
            storage
                .put_object(bundle_key, Bytes::from_static(b"bundle"), None)
                .await
                .expect("bundle object should write");
            storage
                .put_object(replay_key, Bytes::from_static(b"replay"), None)
                .await
                .expect("replay object should write");

            let response =
                preview_retention(params(), &policy(false)).expect("preview should succeed");

            assert!(!response.executed);
            assert_eq!(response.request_log_bundles.candidate_count, 1);
            assert_eq!(response.replay_artifacts.candidate_count, 1);
            assert_eq!(
                response.request_log_bundles.storage_type_counts[0].storage_type,
                StorageType::FileSystem
            );
            assert_eq!(response.request_log_bundles.storage_type_counts[0].count, 1);
            assert_eq!(response.request_log_bundles.oldest_created_at, Some(100));
            assert_eq!(response.request_log_bundles.newest_created_at, Some(100));
            assert_eq!(
                response.request_log_bundles.sample_storage_keys,
                vec![bundle_key.to_string()]
            );
            assert_eq!(
                response.replay_artifacts.storage_type_counts[0].storage_type,
                StorageType::FileSystem
            );
            assert_eq!(response.replay_artifacts.storage_type_counts[0].count, 1);
            assert_eq!(response.replay_artifacts.oldest_created_at, Some(100));
            assert_eq!(response.replay_artifacts.newest_created_at, Some(100));
            assert_eq!(
                response.replay_artifacts.sample_storage_keys,
                vec![replay_key.to_string()]
            );
            assert_eq!(
                response.request_log_bundles.items[0].status,
                DiagnosticsRetentionItemStatus::Candidate
            );
            storage
                .get_object(bundle_key, None)
                .await
                .expect("preview must not delete bundle");
            storage
                .get_object(replay_key, None)
                .await
                .expect("preview must not delete replay artifact");
        })
        .await;
    }

    #[test]
    fn retention_preview_empty_buckets_have_null_time_summary() {
        let db = TestDbContext::new_sqlite("diagnostics-retention-empty-summary.sqlite");
        db.run_sync(|| {
            let response = preview_retention_at(
                DiagnosticsRetentionParams {
                    include_request_log_bundles: Some(false),
                    include_replay_artifacts: Some(false),
                    ..params()
                },
                1_000,
                &policy(false),
            )
            .expect("preview should succeed");

            assert_eq!(response.request_log_bundles.candidate_count, 0);
            assert!(response.request_log_bundles.storage_type_counts.is_empty());
            assert_eq!(response.request_log_bundles.oldest_created_at, None);
            assert_eq!(response.request_log_bundles.newest_created_at, None);
            assert!(response.request_log_bundles.sample_storage_keys.is_empty());
            assert_eq!(response.replay_artifacts.candidate_count, 0);
            assert!(response.replay_artifacts.storage_type_counts.is_empty());
            assert_eq!(response.replay_artifacts.oldest_created_at, None);
            assert_eq!(response.replay_artifacts.newest_created_at, None);
            assert!(response.replay_artifacts.sample_storage_keys.is_empty());
        });
    }

    #[tokio::test]
    async fn retention_execute_requires_enabled_policy() {
        let err = execute_retention(params(), &policy(false))
            .await
            .expect_err("default retention execute should be disabled");

        match err {
            BaseError::ParamInvalid(message) => assert!(
                message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("diagnostics.retention.enabled")
            ),
            other => panic!("expected ParamInvalid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn retention_execute_deletes_objects_and_clears_locators_without_deleting_metadata() {
        let db = TestDbContext::new_sqlite("diagnostics-retention-execute.sqlite");
        db.run_async(async {
            let bundle_key = "retention/execute-bundle.msgpack.gz";
            let replay_key = "retention/execute-replay.msgpack.gz";
            seed_retention_rows(bundle_key, replay_key);
            let storage = get_local_storage().await;
            storage
                .put_object(bundle_key, Bytes::from_static(b"bundle"), None)
                .await
                .expect("bundle object should write");
            storage
                .put_object(replay_key, Bytes::from_static(b"replay"), None)
                .await
                .expect("replay object should write");

            let response =
                execute_retention_at(params(), Utc::now().timestamp_millis(), &policy(true))
                    .await
                    .expect("enabled execute should succeed");

            assert!(response.executed);
            assert_eq!(response.request_log_bundles.succeeded_count, 1);
            assert_eq!(response.replay_artifacts.succeeded_count, 1);
            assert_eq!(response.request_log_bundles.storage_type_counts[0].count, 1);
            assert_eq!(
                response.request_log_bundles.sample_storage_keys,
                vec![bundle_key.to_string()]
            );
            assert_eq!(response.replay_artifacts.storage_type_counts[0].count, 1);
            assert_eq!(
                response.replay_artifacts.sample_storage_keys,
                vec![replay_key.to_string()]
            );
            let log = RequestLog::get_by_id(10).expect("request log metadata should remain");
            assert_eq!(log.bundle_version, Some(2));
            assert_eq!(log.bundle_storage_type, None);
            assert_eq!(log.bundle_storage_key, None);
            let run = RequestReplayRun::get_by_id(201).expect("replay run summary should remain");
            assert_eq!(run.artifact_version, None);
            assert_eq!(run.artifact_storage_type, None);
            assert_eq!(run.artifact_storage_key, None);
            assert!(storage.get_object(bundle_key, None).await.is_err());
            assert!(storage.get_object(replay_key, None).await.is_err());

            let artifact_err = load_replay_artifact_for_run(&run)
                .await
                .expect_err("cleared replay artifact locator should be not found");
            assert!(matches!(artifact_err, BaseError::NotFound(_)));
        })
        .await;
    }

    #[tokio::test]
    async fn retention_execute_reports_s3_unavailable_and_keeps_locators() {
        let db = TestDbContext::new_sqlite("diagnostics-retention-s3-unavailable.sqlite");
        db.run_async(async {
            let bundle_key = "retention/s3-unavailable-bundle.msgpack.gz";
            let replay_key = "retention/s3-unavailable-replay.msgpack.gz";
            seed_retention_rows_for_storage(bundle_key, replay_key, "S3");

            let response = execute_retention_at_with_storage_resolver(
                params(),
                Utc::now().timestamp_millis(),
                &policy(true),
                &FailingS3RetentionStorageResolver,
            )
            .await
            .expect("enabled execute should return item-level failures");

            assert!(response.executed);
            assert_eq!(response.request_log_bundles.failed_count, 1);
            assert_eq!(response.replay_artifacts.failed_count, 1);
            assert_eq!(
                response.request_log_bundles.items[0].status,
                DiagnosticsRetentionItemStatus::Failed
            );
            assert_eq!(
                response.replay_artifacts.items[0].status,
                DiagnosticsRetentionItemStatus::Failed
            );
            for item in response
                .request_log_bundles
                .items
                .iter()
                .chain(response.replay_artifacts.items.iter())
            {
                let message = item.message.as_deref().unwrap_or_default();
                assert!(message.contains("S3 storage is not available"));
                assert!(message.contains("configuration error"));
                assert!(message.contains("bucket"));
                assert!(message.contains("access_key"));
                assert!(message.contains("secret_key"));
                assert!(!message.contains("test-access-key"));
                assert!(!message.contains("test-secret-key"));
            }

            let log = RequestLog::get_by_id(10).expect("request log metadata should remain");
            assert_eq!(log.bundle_version, Some(2));
            assert_eq!(log.bundle_storage_type, Some(StorageType::S3));
            assert_eq!(log.bundle_storage_key.as_deref(), Some(bundle_key));
            let run = RequestReplayRun::get_by_id(201).expect("replay run summary should remain");
            assert_eq!(run.artifact_version, Some(1));
            assert_eq!(run.artifact_storage_type, Some(StorageType::S3));
            assert_eq!(run.artifact_storage_key.as_deref(), Some(replay_key));
        })
        .await;
    }
}
