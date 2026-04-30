use std::io::{Read, Write};

use bytes::Bytes;
use chrono::Utc;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};

use crate::{
    controller::BaseError,
    database::request_replay_run::{RequestReplayRun, RequestReplayRunRecord},
    schema::enum_def::{RequestReplayKind, RequestReplayMode, RequestReplayStatus, StorageType},
    service::{
        diagnostics::replay::types::{
            REQUEST_REPLAY_ARTIFACT_VERSION, RequestReplayArtifact, RequestReplayArtifactStorage,
        },
        storage::{
            Storage, get_local_storage, get_s3_storage, get_storage,
            types::{GetObjectOptions, PutObjectOptions},
        },
    },
    utils::storage::generate_replay_artifact_storage_path,
};

pub(crate) fn set_replay_artifact_locator(
    run: &mut RequestReplayRun,
    locator: &RequestReplayArtifactStorage,
) {
    run.artifact_version = Some(locator.artifact_version);
    run.artifact_storage_type = Some(locator.artifact_storage_type.clone());
    run.artifact_storage_key = Some(locator.artifact_storage_key.clone());
}

pub(crate) async fn store_replay_artifact_for_run(
    storage: &dyn Storage,
    run: &mut RequestReplayRun,
    artifact: &RequestReplayArtifact,
) -> Result<RequestReplayArtifactStorage, BaseError> {
    match store_replay_artifact_with_storage(storage, artifact).await {
        Ok(locator) => Ok(locator),
        Err(err) => Err(persist_replay_run_after_artifact_failure(run, err)),
    }
}

fn persist_replay_run_after_artifact_failure(
    run: &mut RequestReplayRun,
    artifact_error: BaseError,
) -> BaseError {
    persist_replay_run_after_artifact_failure_with(run, artifact_error, |updated_run| {
        RequestReplayRun::update(updated_run)
    })
}

fn persist_replay_run_after_artifact_failure_with<F>(
    run: &mut RequestReplayRun,
    artifact_error: BaseError,
    persist_run: F,
) -> BaseError
where
    F: FnOnce(&RequestReplayRun) -> Result<RequestReplayRunRecord, BaseError>,
{
    let prior_status = run.status;
    let prior_error_code = run.error_code.clone();
    let now = Utc::now().timestamp_millis();

    run.status = RequestReplayStatus::Error;
    run.error_code = Some("replay_artifact_storage_failed".to_string());
    run.error_message = Some(format!(
        "Failed to persist {} {} replay artifact after terminal status '{}'{}: {}",
        request_replay_kind_label(&run.replay_kind),
        request_replay_mode_label(&run.replay_mode),
        request_replay_status_label(&prior_status),
        prior_error_code
            .as_deref()
            .map(|code| format!(" with error code '{}'", code))
            .unwrap_or_default(),
        base_error_message(&artifact_error),
    ));
    run.artifact_version = None;
    run.artifact_storage_type = None;
    run.artifact_storage_key = None;
    if run.completed_at.is_none() {
        run.completed_at = Some(now);
    }
    run.updated_at = now;

    match persist_run(run) {
        Ok(updated_run) => {
            *run = updated_run;
            artifact_error
        }
        Err(update_error) => BaseError::DatabaseFatal(Some(format!(
            "{}; additionally failed to persist replay run {} failure state: {}",
            base_error_message(&artifact_error),
            run.id,
            base_error_message(&update_error),
        ))),
    }
}

pub async fn store_replay_artifact(
    artifact: &RequestReplayArtifact,
) -> Result<RequestReplayArtifactStorage, BaseError> {
    let storage = get_storage().await;
    store_replay_artifact_with_storage(&**storage, artifact).await
}

pub async fn store_replay_artifact_with_storage(
    storage: &dyn Storage,
    artifact: &RequestReplayArtifact,
) -> Result<RequestReplayArtifactStorage, BaseError> {
    let storage_type = storage.get_storage_type();
    let key = generate_replay_artifact_storage_path(
        artifact.created_at,
        artifact.replay_run_id,
        &storage_type,
    );
    let body = encode_replay_artifact(artifact)?;

    storage
        .put_object(
            &key,
            body,
            Some(PutObjectOptions {
                content_type: Some("application/msgpack"),
                content_encoding: Some("gzip"),
            }),
        )
        .await
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to write request replay artifact {}: {}",
                key, err
            )))
        })?;

    Ok(RequestReplayArtifactStorage {
        artifact_version: REQUEST_REPLAY_ARTIFACT_VERSION as i32,
        artifact_storage_type: storage_type,
        artifact_storage_key: key,
    })
}

pub async fn load_replay_artifact_for_run(
    run: &RequestReplayRunRecord,
) -> Result<RequestReplayArtifact, BaseError> {
    if run.artifact_version.is_none()
        && run.artifact_storage_type.is_none()
        && run.artifact_storage_key.is_none()
    {
        return Err(BaseError::NotFound(Some(
            "Replay artifact not found".to_string(),
        )));
    }

    if run.artifact_version != Some(REQUEST_REPLAY_ARTIFACT_VERSION as i32) {
        return Err(BaseError::DatabaseFatal(Some(format!(
            "Unsupported request replay artifact version {:?}",
            run.artifact_version
        ))));
    }

    let Some(storage_type) = run.artifact_storage_type.clone() else {
        return Err(BaseError::NotFound(Some(
            "Replay artifact storage type not found".to_string(),
        )));
    };
    let Some(key) = run.artifact_storage_key.as_deref() else {
        return Err(BaseError::NotFound(Some(
            "Replay artifact storage key not found".to_string(),
        )));
    };

    let storage: &dyn Storage = match storage_type {
        StorageType::FileSystem => get_local_storage().await,
        StorageType::S3 => get_s3_storage()
            .await
            .ok_or_else(|| BaseError::NotFound(Some("S3 storage not available".to_string())))?,
    };

    load_replay_artifact_with_storage(storage, key).await
}

pub async fn load_replay_artifact_with_storage(
    storage: &dyn Storage,
    key: &str,
) -> Result<RequestReplayArtifact, BaseError> {
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
                "Failed to read request replay artifact {}: {}",
                key, err
            )))
        })?;

    decode_replay_artifact(&bytes).map_err(|err| BaseError::DatabaseFatal(Some(err)))
}

fn encode_replay_artifact(artifact: &RequestReplayArtifact) -> Result<Bytes, BaseError> {
    if artifact.version != REQUEST_REPLAY_ARTIFACT_VERSION {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Unsupported request replay artifact version {}",
            artifact.version
        ))));
    }

    let serialized = rmp_serde::to_vec_named(artifact).map_err(|err| {
        BaseError::DatabaseFatal(Some(format!(
            "Failed to serialize request replay artifact: {}",
            err
        )))
    })?;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&serialized).map_err(|err| {
        BaseError::DatabaseFatal(Some(format!(
            "Failed to gzip request replay artifact: {}",
            err
        )))
    })?;
    let compressed = encoder.finish().map_err(|err| {
        BaseError::DatabaseFatal(Some(format!(
            "Failed to finish request replay artifact gzip stream: {}",
            err
        )))
    })?;

    Ok(Bytes::from(compressed))
}

fn decode_replay_artifact(bytes: &[u8]) -> Result<RequestReplayArtifact, String> {
    let artifact =
        rmp_serde::from_slice::<RequestReplayArtifact>(bytes).or_else(|first_error| {
            let mut decoder = GzDecoder::new(bytes);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|gzip_error| {
                    format!(
                        "Failed to decode request replay artifact: {}; gzip fallback failed: {}",
                        first_error, gzip_error
                    )
                })?;
            rmp_serde::from_slice::<RequestReplayArtifact>(&decompressed).map_err(|second_error| {
                format!(
                "Failed to decode request replay artifact: {}; gzip decoded fallback failed: {}",
                first_error, second_error
            )
            })
        })?;

    if artifact.version != REQUEST_REPLAY_ARTIFACT_VERSION {
        return Err(format!(
            "Unsupported request replay artifact version {}",
            artifact.version
        ));
    }

    Ok(artifact)
}

fn request_replay_kind_label(kind: &RequestReplayKind) -> &'static str {
    match kind {
        RequestReplayKind::AttemptUpstream => "attempt_upstream",
        RequestReplayKind::GatewayRequest => "gateway_request",
    }
}

fn request_replay_mode_label(mode: &RequestReplayMode) -> &'static str {
    match mode {
        RequestReplayMode::DryRun => "dry_run",
        RequestReplayMode::Live => "live",
    }
}

fn request_replay_status_label(status: &RequestReplayStatus) -> &'static str {
    match status {
        RequestReplayStatus::Pending => "pending",
        RequestReplayStatus::Running => "running",
        RequestReplayStatus::Success => "success",
        RequestReplayStatus::Error => "error",
        RequestReplayStatus::Cancelled => "cancelled",
        RequestReplayStatus::Rejected => "rejected",
    }
}

fn base_error_message(error: &BaseError) -> String {
    match error {
        BaseError::ParamInvalid(Some(message))
        | BaseError::DatabaseFatal(Some(message))
        | BaseError::DatabaseDup(Some(message))
        | BaseError::NotFound(Some(message))
        | BaseError::Unauthorized(Some(message))
        | BaseError::StoreError(Some(message))
        | BaseError::InternalServerError(Some(message)) => message.clone(),
        BaseError::ParamInvalid(None) => "request params invalid".to_string(),
        BaseError::DatabaseFatal(None) => "database unknown error".to_string(),
        BaseError::DatabaseDup(None) => "some unique keys have conflicted".to_string(),
        BaseError::NotFound(None) => "data not found".to_string(),
        BaseError::Unauthorized(None) => "Unauthorized".to_string(),
        BaseError::StoreError(None) => "Application cache/store operation failed".to_string(),
        BaseError::InternalServerError(None) => "internal server error".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::{
        schema::enum_def::{
            LlmApiType, RequestReplayKind, RequestReplayMode, RequestReplaySemanticBasis,
            RequestReplayStatus, StorageType,
        },
        service::{
            diagnostics::replay::types::{
                RequestReplayArtifactDiff, RequestReplayArtifactResult,
                RequestReplayArtifactSource, RequestReplayBody, RequestReplayBodyCaptureMetadata,
                RequestReplayDiffBaselineKind, RequestReplayExecutionPreview,
                RequestReplayInputSnapshot, RequestReplayModelSnapshot, RequestReplayNameValue,
                RequestReplayProviderSnapshot, RequestReplayResolvedCandidate,
            },
            storage::local::LocalStorage,
        },
    };

    use super::*;

    fn artifact() -> RequestReplayArtifact {
        RequestReplayArtifact {
            version: REQUEST_REPLAY_ARTIFACT_VERSION,
            replay_run_id: 654321,
            created_at: 1_776_840_000_000,
            source: RequestReplayArtifactSource {
                request_log_id: 42,
                attempt_id: Some(101),
                replay_kind: RequestReplayKind::AttemptUpstream,
                replay_mode: RequestReplayMode::Live,
            },
            input_snapshot: Some(RequestReplayInputSnapshot::AttemptUpstream {
                request_uri: "https://upstream.example/v1/chat/completions".to_string(),
                sanitized_request_headers: vec![RequestReplayNameValue {
                    name: "content-type".to_string(),
                    value: Some("application/json".to_string()),
                }],
                llm_request_body: Some(RequestReplayBody {
                    media_type: Some("application/json".to_string()),
                    json: Some(serde_json::json!({"model": "gpt-test"})),
                    text: None,
                    capture_state: Some("complete".to_string()),
                }),
                provider: Some(RequestReplayProviderSnapshot {
                    provider_id: Some(2),
                    provider_api_key_id: Some(3),
                    provider_key: Some("openai".to_string()),
                    provider_name: Some("OpenAI".to_string()),
                }),
                model: Some(RequestReplayModelSnapshot {
                    model_id: Some(4),
                    model_name: Some("gpt-test".to_string()),
                    real_model_name: Some("gpt-real".to_string()),
                    llm_api_type: Some(LlmApiType::Openai),
                }),
            }),
            execution_preview: Some(RequestReplayExecutionPreview {
                semantic_basis: RequestReplaySemanticBasis::HistoricalAttemptSnapshot,
                requested_model_name: None,
                base_requested_model_name: None,
                resolved_reasoning_suffix: None,
                resolved_reasoning_preset: None,
                resolved_route: None,
                resolved_candidate: Some(RequestReplayResolvedCandidate {
                    candidate_position: Some(1),
                    provider_id: Some(2),
                    provider_api_key_id: Some(3),
                    model_id: Some(4),
                    llm_api_type: Some(LlmApiType::Openai),
                }),
                candidate_decisions: Vec::new(),
                applied_request_patch_summary: None,
                final_request_uri: Some("https://upstream.example/v1/chat/completions".to_string()),
                final_request_headers: Vec::new(),
                final_request_body: None,
            }),
            result: Some(RequestReplayArtifactResult {
                status: RequestReplayStatus::Success,
                http_status: Some(200),
                response_headers: Vec::new(),
                response_body: Some(RequestReplayBody {
                    media_type: Some("application/json".to_string()),
                    json: Some(serde_json::json!({"ok": true})),
                    text: None,
                    capture_state: Some("complete".to_string()),
                }),
                response_body_capture_state: Some("complete".to_string()),
                response_body_capture: Some(RequestReplayBodyCaptureMetadata {
                    state: "complete".to_string(),
                    bytes_captured: 11,
                    original_size_bytes: Some(11),
                    original_size_known: true,
                    truncated: false,
                    sha256: "a5e744d0164540d33b1d7ea616c28f2fa97e754a2d9cc56f8804a64bb764a55a"
                        .to_string(),
                    capture_limit_bytes: 4_194_304,
                    body_encoding: "identity".to_string(),
                }),
                usage_normalization: Some(serde_json::json!({"total_tokens": 12})),
                transform_diagnostics: Vec::new(),
                attempt_timeline: Vec::new(),
            }),
            diff: Some(RequestReplayArtifactDiff {
                baseline_kind: RequestReplayDiffBaselineKind::OriginalAttempt,
                status_changed: Some(false),
                headers_changed: Some(false),
                body_changed: Some(true),
                token_delta: Some(1),
                cost_delta: Some(100),
                summary_lines: vec!["response body changed".to_string()],
            }),
        }
    }

    #[tokio::test]
    async fn replay_artifact_round_trips_through_storage_trait() {
        let dir = tempdir().expect("temp dir should be created");
        let storage = LocalStorage::new(dir.path().to_str().expect("temp path should be utf8"));
        let artifact = artifact();

        let locator = store_replay_artifact_with_storage(&storage, &artifact)
            .await
            .expect("artifact should store");

        assert_eq!(locator.artifact_version, 1);
        assert_eq!(locator.artifact_storage_type, StorageType::FileSystem);
        assert_eq!(
            locator.artifact_storage_key,
            "replays/2026/04/22/65/654321.mp.gz"
        );

        let loaded = load_replay_artifact_with_storage(&storage, &locator.artifact_storage_key)
            .await
            .expect("artifact should load");
        assert_eq!(loaded, artifact);
    }

    #[test]
    fn replay_artifact_rejects_unknown_version_on_write() {
        let mut artifact = artifact();
        artifact.version = 999;

        let err = encode_replay_artifact(&artifact).expect_err("version should be rejected");
        assert!(matches!(err, BaseError::ParamInvalid(_)));
    }

    #[test]
    fn replay_artifact_rejects_unknown_version_on_read() {
        let mut artifact = artifact();
        artifact.version = 999;
        let bytes = rmp_serde::to_vec_named(&artifact).expect("artifact should encode");

        let err = decode_replay_artifact(&bytes).expect_err("version should be rejected");

        assert!(err.contains("Unsupported request replay artifact version 999"));
    }

    #[test]
    fn artifact_persist_failure_marks_run_as_terminal_error() {
        let mut run = RequestReplayRun {
            id: 654321,
            source_request_log_id: 42,
            source_attempt_id: Some(101),
            replay_kind: RequestReplayKind::AttemptUpstream,
            replay_mode: RequestReplayMode::Live,
            semantic_basis: RequestReplaySemanticBasis::HistoricalAttemptSnapshot,
            status: RequestReplayStatus::Success,
            error_code: Some("upstream_rate_limit_error".to_string()),
            completed_at: Some(1_776_840_000_100),
            updated_at: 1_776_840_000_100,
            ..Default::default()
        };
        let persisted = std::cell::RefCell::new(None);

        let err = persist_replay_run_after_artifact_failure_with(
            &mut run,
            BaseError::DatabaseFatal(Some("failed to put object: disk full".to_string())),
            |updated_run| {
                persisted.replace(Some(updated_run.clone()));
                Ok(updated_run.clone())
            },
        );

        assert!(matches!(
            err,
            BaseError::DatabaseFatal(Some(message))
                if message.contains("failed to put object: disk full")
        ));
        assert_eq!(run.status, RequestReplayStatus::Error);
        assert_eq!(
            run.error_code.as_deref(),
            Some("replay_artifact_storage_failed")
        );
        assert_eq!(run.artifact_version, None);
        assert_eq!(run.artifact_storage_type, None);
        assert_eq!(run.artifact_storage_key, None);
        assert_eq!(run.completed_at, Some(1_776_840_000_100));
        assert!(run.error_message.as_deref().is_some_and(|message| {
            message.contains("attempt_upstream live replay artifact")
                && message.contains("terminal status 'success'")
                && message.contains("upstream_rate_limit_error")
        }));

        let persisted = persisted.into_inner().expect("run should be persisted");
        assert_eq!(persisted.status, RequestReplayStatus::Error);
        assert_eq!(
            persisted.error_code.as_deref(),
            Some("replay_artifact_storage_failed")
        );
    }

    #[test]
    fn artifact_persist_failure_reports_run_update_failure() {
        let mut run = RequestReplayRun {
            id: 654322,
            source_request_log_id: 42,
            replay_kind: RequestReplayKind::GatewayRequest,
            replay_mode: RequestReplayMode::DryRun,
            semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
            status: RequestReplayStatus::Success,
            updated_at: 1,
            ..Default::default()
        };

        let err = persist_replay_run_after_artifact_failure_with(
            &mut run,
            BaseError::DatabaseFatal(Some("failed to put object: unavailable".to_string())),
            |_updated_run| {
                Err(BaseError::DatabaseFatal(Some(
                    "failed to update request replay run".to_string(),
                )))
            },
        );

        assert!(matches!(
            err,
            BaseError::DatabaseFatal(Some(message))
                if message.contains("failed to put object: unavailable")
                    && message.contains("failed to persist replay run 654322 failure state")
                    && message.contains("failed to update request replay run")
        ));
    }
}
