use std::fmt::Debug;
use std::io::Read;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use crate::{
    controller::BaseError,
    database::{
        request_attempt::{RequestAttempt, RequestAttemptDetail},
        request_log::{RequestLog, RequestLogRecord},
    },
    schema::enum_def::{LlmApiType, ProviderApiKeyMode, StorageType},
    service::{
        storage::{Storage, get_local_storage, get_s3_storage, types::GetObjectOptions},
        transform::unified::UnifiedTransformDiagnostic,
    },
    utils::storage::{
        LogBodyCaptureState, LogBundle, REQUEST_LOG_BUNDLE_V1_VERSION,
        REQUEST_LOG_BUNDLE_V2_VERSION, RequestLogBundleCandidateManifest,
        RequestLogBundleRequestSnapshot, RequestLogBundleTransformDiagnostics, RequestLogBundleV2,
    },
};

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestLogArtifactResponse {
    pub payload_manifest: PayloadManifestResponse,
    pub request_snapshot: Option<RequestSnapshotResponse>,
    pub candidate_manifest: CandidateManifestResponse,
    pub transform_diagnostics: TransformDiagnosticsSummaryResponse,
    pub replay_capability: ReplayCapabilitySummary,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct PayloadManifestResponse {
    pub bundle_version: Option<u32>,
    pub log_id: i64,
    pub created_at: Option<i64>,
    pub request: PayloadRequestManifestResponse,
    pub attempts: Vec<PayloadAttemptManifestResponse>,
    pub blob_count: usize,
    pub patch_count: usize,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct PayloadRequestManifestResponse {
    pub has_user_request_body: bool,
    pub user_request_blob_id: Option<i32>,
    pub has_user_response_body: bool,
    pub user_response_blob_id: Option<i32>,
    pub user_response_capture_state: Option<String>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct PayloadAttemptManifestResponse {
    pub attempt_id: Option<i64>,
    pub attempt_index: i32,
    pub has_llm_request_body: bool,
    pub llm_request_blob_id: Option<i32>,
    pub llm_request_patch_id: Option<i32>,
    pub has_llm_response_body: bool,
    pub llm_response_blob_id: Option<i32>,
    pub llm_response_capture_state: Option<String>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestSnapshotResponse {
    pub request_path: String,
    pub operation_kind: String,
    pub query_params: Vec<NameValueResponse>,
    pub sanitized_original_headers: Vec<NameValueResponse>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct NameValueResponse {
    pub name: String,
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_present: Option<bool>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateManifestResponse {
    pub has_asset: bool,
    pub items: Vec<CandidateManifestItemResponse>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateManifestItemResponse {
    pub candidate_position: i32,
    pub route_id: Option<i64>,
    pub route_name: Option<String>,
    pub provider_id: i64,
    pub provider_key: String,
    pub model_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub llm_api_type: String,
    pub provider_api_key_mode: String,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct TransformDiagnosticsSummaryResponse {
    pub has_asset: bool,
    pub summary: TransformDiagnosticsSummaryBodyResponse,
    pub items: Vec<TransformDiagnosticItemResponse>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct TransformDiagnosticsSummaryBodyResponse {
    pub count: u32,
    pub max_loss_level: Option<String>,
    pub kinds: Vec<String>,
    pub phases: Vec<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct TransformDiagnosticItemResponse {
    pub phase: String,
    pub diagnostic: UnifiedTransformDiagnostic,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayCapabilitySummary {
    pub attempt_upstream: ReplayKindCapabilitySummary,
    pub gateway_request: ReplayKindCapabilitySummary,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayKindCapabilitySummary {
    pub available: bool,
    pub reasons: Vec<String>,
    pub attempt_ids: Vec<i64>,
}

pub(crate) enum DecodedRequestLogBundle {
    Legacy(LogBundle),
    V2(RequestLogBundleV2),
}

impl DecodedRequestLogBundle {
    pub(crate) fn as_v2(&self) -> Option<&RequestLogBundleV2> {
        match self {
            Self::V2(bundle) => Some(bundle),
            Self::Legacy(_) => None,
        }
    }

    pub(crate) fn as_legacy(&self) -> Option<&LogBundle> {
        match self {
            Self::Legacy(bundle) => Some(bundle),
            Self::V2(_) => None,
        }
    }
}

enum RequestLogBundleCapabilityView<'a> {
    Legacy(&'a LogBundle),
    V2(&'a RequestLogBundleV2),
}

#[derive(Deserialize)]
struct BundleHeader {
    version: u32,
}

pub async fn get_request_log_artifacts(
    request_log_id: i64,
) -> Result<RequestLogArtifactResponse, BaseError> {
    let record = RequestLog::get_by_id(request_log_id)?;
    let attempts = RequestAttempt::list_by_request_log_id(request_log_id)?;
    let bundle = load_request_log_bundle(&record).await?;

    Ok(build_request_log_artifact_response(
        &record,
        &attempts,
        bundle.as_ref(),
    ))
}

pub(crate) async fn load_request_log_bundle(
    record: &RequestLogRecord,
) -> Result<Option<DecodedRequestLogBundle>, BaseError> {
    let Some(storage_type) = record.bundle_storage_type.clone() else {
        return Ok(None);
    };
    let Some(key) = record.bundle_storage_key.as_deref() else {
        return Ok(None);
    };

    let storage: &dyn Storage = match storage_type {
        StorageType::FileSystem => get_local_storage().await,
        StorageType::S3 => get_s3_storage()
            .await
            .ok_or_else(|| BaseError::NotFound(Some("S3 storage not available".to_string())))?,
    };
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

    decode_request_log_bundle(&bytes)
        .map(Some)
        .map_err(|err| BaseError::DatabaseFatal(Some(err)))
}

fn build_request_log_artifact_response(
    record: &RequestLogRecord,
    attempts: &[RequestAttemptDetail],
    bundle: Option<&DecodedRequestLogBundle>,
) -> RequestLogArtifactResponse {
    match bundle {
        Some(DecodedRequestLogBundle::V2(bundle)) => {
            build_v2_artifact_response(record, attempts, bundle)
        }
        Some(DecodedRequestLogBundle::Legacy(bundle)) => {
            build_legacy_artifact_response(record, attempts, bundle)
        }
        None => build_empty_artifact_response(record, attempts),
    }
}

fn build_empty_artifact_response(
    record: &RequestLogRecord,
    attempts: &[RequestAttemptDetail],
) -> RequestLogArtifactResponse {
    RequestLogArtifactResponse {
        payload_manifest: PayloadManifestResponse {
            bundle_version: record.bundle_version.map(|version| version as u32),
            log_id: record.id,
            ..Default::default()
        },
        replay_capability: build_replay_capability(None, attempts),
        ..Default::default()
    }
}

fn build_legacy_artifact_response(
    record: &RequestLogRecord,
    attempts: &[RequestAttemptDetail],
    bundle: &LogBundle,
) -> RequestLogArtifactResponse {
    RequestLogArtifactResponse {
        payload_manifest: PayloadManifestResponse {
            bundle_version: Some(bundle.version),
            log_id: bundle.log_id,
            created_at: Some(bundle.created_at),
            request: PayloadRequestManifestResponse {
                has_user_request_body: bundle.user_request_body.is_some(),
                has_user_response_body: bundle.user_response_body.is_some(),
                user_response_capture_state: bundle
                    .user_response_capture_state
                    .as_ref()
                    .map(capture_state_response),
                ..Default::default()
            },
            attempts: Vec::new(),
            blob_count: [
                bundle.user_request_body.as_ref(),
                bundle.llm_request_body.as_ref(),
                bundle.llm_response_body.as_ref(),
                bundle.user_response_body.as_ref(),
            ]
            .into_iter()
            .filter(|body| body.is_some())
            .count(),
            patch_count: 0,
        },
        replay_capability: build_replay_capability(
            Some(RequestLogBundleCapabilityView::Legacy(bundle)),
            attempts,
        ),
        ..build_empty_artifact_response(record, attempts)
    }
}

fn build_v2_artifact_response(
    _record: &RequestLogRecord,
    attempts: &[RequestAttemptDetail],
    bundle: &RequestLogBundleV2,
) -> RequestLogArtifactResponse {
    RequestLogArtifactResponse {
        payload_manifest: build_v2_payload_manifest(bundle),
        request_snapshot: bundle
            .request_snapshot
            .as_ref()
            .map(request_snapshot_response),
        candidate_manifest: candidate_manifest_response(bundle.candidate_manifest.as_ref()),
        transform_diagnostics: transform_diagnostics_response(
            bundle.transform_diagnostics.as_ref(),
        ),
        replay_capability: build_replay_capability(
            Some(RequestLogBundleCapabilityView::V2(bundle)),
            attempts,
        ),
    }
}

fn build_v2_payload_manifest(bundle: &RequestLogBundleV2) -> PayloadManifestResponse {
    PayloadManifestResponse {
        bundle_version: Some(bundle.version),
        log_id: bundle.log_id,
        created_at: Some(bundle.created_at),
        request: PayloadRequestManifestResponse {
            has_user_request_body: bundle.request_section.user_request_blob_id.is_some(),
            user_request_blob_id: bundle.request_section.user_request_blob_id,
            has_user_response_body: bundle.request_section.user_response_blob_id.is_some(),
            user_response_blob_id: bundle.request_section.user_response_blob_id,
            user_response_capture_state: bundle
                .request_section
                .user_response_capture_state
                .as_ref()
                .map(capture_state_response),
        },
        attempts: bundle
            .attempt_sections
            .iter()
            .map(|attempt| PayloadAttemptManifestResponse {
                attempt_id: attempt.attempt_id,
                attempt_index: attempt.attempt_index,
                has_llm_request_body: attempt.llm_request_blob_id.is_some(),
                llm_request_blob_id: attempt.llm_request_blob_id,
                llm_request_patch_id: attempt.llm_request_patch_id,
                has_llm_response_body: attempt.llm_response_blob_id.is_some(),
                llm_response_blob_id: attempt.llm_response_blob_id,
                llm_response_capture_state: attempt
                    .llm_response_capture_state
                    .as_ref()
                    .map(capture_state_response),
            })
            .collect(),
        blob_count: bundle.blob_pool.len(),
        patch_count: bundle.patch_pool.len(),
    }
}

fn request_snapshot_response(
    snapshot: &RequestLogBundleRequestSnapshot,
) -> RequestSnapshotResponse {
    RequestSnapshotResponse {
        request_path: snapshot.request_path.clone(),
        operation_kind: snapshot.operation_kind.clone(),
        query_params: snapshot
            .query_params
            .iter()
            .map(|param| NameValueResponse {
                name: param.name.clone(),
                value: param.value_for_replay(),
                value_present: Some(param.has_value()),
            })
            .collect(),
        sanitized_original_headers: snapshot
            .sanitized_original_headers
            .iter()
            .map(|header| NameValueResponse {
                name: header.name.clone(),
                value: Some(header.value.clone()),
                value_present: None,
            })
            .collect(),
    }
}

fn candidate_manifest_response(
    manifest: Option<&RequestLogBundleCandidateManifest>,
) -> CandidateManifestResponse {
    let Some(manifest) = manifest else {
        return CandidateManifestResponse::default();
    };

    CandidateManifestResponse {
        has_asset: true,
        items: manifest
            .items
            .iter()
            .map(|item| CandidateManifestItemResponse {
                candidate_position: item.candidate_position,
                route_id: item.route_id,
                route_name: item.route_name.clone(),
                provider_id: item.provider_id,
                provider_key: item.provider_key.clone(),
                model_id: item.model_id,
                model_name: item.model_name.clone(),
                real_model_name: item.real_model_name.clone(),
                llm_api_type: llm_api_type_response(item.llm_api_type),
                provider_api_key_mode: provider_api_key_mode_response(&item.provider_api_key_mode),
            })
            .collect(),
    }
}

fn transform_diagnostics_response(
    diagnostics: Option<&RequestLogBundleTransformDiagnostics>,
) -> TransformDiagnosticsSummaryResponse {
    let Some(diagnostics) = diagnostics else {
        return TransformDiagnosticsSummaryResponse::default();
    };

    TransformDiagnosticsSummaryResponse {
        has_asset: true,
        summary: TransformDiagnosticsSummaryBodyResponse {
            count: diagnostics.summary.count,
            max_loss_level: diagnostics
                .summary
                .max_loss_level
                .as_ref()
                .map(serialized_enum_response),
            kinds: diagnostics
                .summary
                .kinds
                .iter()
                .map(serialized_enum_response)
                .collect(),
            phases: diagnostics
                .summary
                .phases
                .iter()
                .map(serialized_enum_response)
                .collect(),
        },
        items: diagnostics
            .items
            .iter()
            .map(|item| TransformDiagnosticItemResponse {
                phase: serialized_enum_response(&item.phase),
                diagnostic: item.diagnostic.clone(),
            })
            .collect(),
    }
}

fn build_replay_capability(
    bundle: Option<RequestLogBundleCapabilityView<'_>>,
    attempts: &[RequestAttemptDetail],
) -> ReplayCapabilitySummary {
    let attempt_upstream = build_attempt_upstream_capability(bundle.as_ref(), attempts);
    let gateway_request = build_gateway_request_capability(bundle.as_ref());

    ReplayCapabilitySummary {
        attempt_upstream,
        gateway_request,
    }
}

fn build_attempt_upstream_capability(
    bundle: Option<&RequestLogBundleCapabilityView<'_>>,
    attempts: &[RequestAttemptDetail],
) -> ReplayKindCapabilitySummary {
    let Some(bundle) = bundle else {
        return unavailable("bundle_missing");
    };

    let attempt_ids = attempts
        .iter()
        .filter(|attempt| {
            has_non_empty(attempt.request_uri.as_deref())
                && has_non_empty(attempt.request_headers_json.as_deref())
                && attempt_request_body_available(bundle, attempt)
        })
        .map(|attempt| attempt.id)
        .collect::<Vec<_>>();

    if attempt_ids.is_empty() {
        unavailable("no_attempt_with_complete_downstream_request_snapshot")
    } else {
        ReplayKindCapabilitySummary {
            available: true,
            reasons: Vec::new(),
            attempt_ids,
        }
    }
}

fn build_gateway_request_capability(
    bundle: Option<&RequestLogBundleCapabilityView<'_>>,
) -> ReplayKindCapabilitySummary {
    let Some(RequestLogBundleCapabilityView::V2(bundle)) = bundle else {
        return unavailable("request_snapshot_missing");
    };

    if bundle
        .request_snapshot
        .as_ref()
        .is_none_or(|snapshot| snapshot.request_path.is_empty())
    {
        return unavailable("request_snapshot_missing");
    }

    if !bundle
        .request_section
        .user_request_blob_id
        .is_some_and(|blob_id| blob_exists(bundle, blob_id))
    {
        return unavailable("user_request_body_missing");
    }

    ReplayKindCapabilitySummary {
        available: true,
        reasons: Vec::new(),
        attempt_ids: Vec::new(),
    }
}

fn attempt_request_body_available(
    bundle: &RequestLogBundleCapabilityView<'_>,
    attempt: &RequestAttemptDetail,
) -> bool {
    match bundle {
        RequestLogBundleCapabilityView::Legacy(bundle) => {
            bundle.llm_request_body.is_some() && attempt.attempt_index == 1
        }
        RequestLogBundleCapabilityView::V2(bundle) => bundle
            .attempt_sections
            .iter()
            .find(|section| {
                section
                    .attempt_id
                    .is_some_and(|attempt_id| attempt_id == attempt.id)
                    || section.attempt_index == attempt.attempt_index
            })
            .is_some_and(|section| {
                section
                    .llm_request_blob_id
                    .is_some_and(|blob_id| blob_exists(bundle, blob_id))
                    && section
                        .llm_request_patch_id
                        .is_none_or(|patch_id| patch_exists(bundle, patch_id))
            }),
    }
}

fn blob_exists(bundle: &RequestLogBundleV2, blob_id: i32) -> bool {
    bundle.blob_pool.iter().any(|blob| blob.blob_id == blob_id)
}

fn patch_exists(bundle: &RequestLogBundleV2, patch_id: i32) -> bool {
    bundle
        .patch_pool
        .iter()
        .any(|patch| patch.patch_id == patch_id)
}

fn unavailable(reason: &str) -> ReplayKindCapabilitySummary {
    ReplayKindCapabilitySummary {
        available: false,
        reasons: vec![reason.to_string()],
        attempt_ids: Vec::new(),
    }
}

fn has_non_empty(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn decode_request_log_bundle(bytes: &[u8]) -> Result<DecodedRequestLogBundle, String> {
    decode_request_log_bundle_inner(bytes).or_else(|first_error| {
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

fn decode_request_log_bundle_inner(bytes: &[u8]) -> Result<DecodedRequestLogBundle, String> {
    let header: BundleHeader = rmp_serde::from_slice(bytes)
        .map_err(|err| format!("bundle header decode failed: {}", err))?;
    match header.version {
        REQUEST_LOG_BUNDLE_V1_VERSION => rmp_serde::from_slice::<LogBundle>(bytes)
            .map(DecodedRequestLogBundle::Legacy)
            .map_err(|err| format!("bundle v1 decode failed: {}", err)),
        REQUEST_LOG_BUNDLE_V2_VERSION => rmp_serde::from_slice::<RequestLogBundleV2>(bytes)
            .map(DecodedRequestLogBundle::V2)
            .map_err(|err| format!("bundle v2 decode failed: {}", err)),
        other => Err(format!("unsupported request log bundle version {}", other)),
    }
}

fn capture_state_response(state: &LogBodyCaptureState) -> String {
    match state {
        LogBodyCaptureState::Complete => "complete",
        LogBodyCaptureState::Incomplete => "incomplete",
        LogBodyCaptureState::NotCaptured => "not_captured",
    }
    .to_string()
}

fn llm_api_type_response(value: LlmApiType) -> String {
    serialized_enum_response(&value)
}

fn provider_api_key_mode_response(value: &ProviderApiKeyMode) -> String {
    serialized_enum_response(value)
}

fn serialized_enum_response<T>(value: &T) -> String
where
    T: Serialize + Debug,
{
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{value:?}"))
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use crate::{
        database::{request_attempt::RequestAttemptDetail, request_log::RequestLogRecord},
        schema::enum_def::{LlmApiType, ProviderApiKeyMode},
        service::transform::unified::{
            UnifiedTransformDiagnostic, UnifiedTransformDiagnosticAction,
            UnifiedTransformDiagnosticKind, UnifiedTransformDiagnosticLossLevel,
        },
        utils::storage::{
            LogBodyCaptureState, RequestLogBundleAttemptSection, RequestLogBundleCandidateManifest,
            RequestLogBundleCandidateManifestItem, RequestLogBundleHttpHeader,
            RequestLogBundleRequestSection, RequestLogBundleRequestSnapshot,
            RequestLogBundleTransformDiagnosticItem, RequestLogBundleTransformDiagnosticPhase,
            RequestLogBundleTransformDiagnostics, RequestLogBundleTransformDiagnosticsSummary,
            RequestLogBundleV2, RequestLogBundleV2Builder, RequestLogBundleV2DiagnosticAssets,
        },
    };

    use super::{
        DecodedRequestLogBundle, build_request_log_artifact_response, decode_request_log_bundle,
    };

    fn record() -> RequestLogRecord {
        RequestLogRecord {
            id: 42,
            api_key_id: 7,
            bundle_version: Some(2),
            ..Default::default()
        }
    }

    fn attempt() -> RequestAttemptDetail {
        RequestAttemptDetail {
            id: 101,
            request_log_id: 42,
            attempt_index: 1,
            request_uri: Some("https://upstream.example/v1/chat/completions".to_string()),
            request_headers_json: Some("{\"content-type\":\"application/json\"}".to_string()),
            ..Default::default()
        }
    }

    fn diagnostic() -> UnifiedTransformDiagnostic {
        UnifiedTransformDiagnostic {
            type_: "transform_diagnostic".to_string(),
            diagnostic_kind: UnifiedTransformDiagnosticKind::CapabilityDowngrade,
            provider: "Responses".to_string(),
            target_provider: "OpenAI".to_string(),
            source: "responses".to_string(),
            target: "openai".to_string(),
            stream_id: None,
            stage: Some("request".to_string()),
            loss_level: UnifiedTransformDiagnosticLossLevel::LossyMajor,
            action: UnifiedTransformDiagnosticAction::Drop,
            semantic_unit: "reasoning".to_string(),
            reason: "reasoning summary was dropped".to_string(),
            context: None,
            raw_data_summary: None,
            recovery_hint: None,
        }
    }

    fn bundle() -> RequestLogBundleV2 {
        let mut builder = RequestLogBundleV2Builder::new();
        let user_request_blob_id =
            builder.add_user_request_body(Bytes::from_static(br#"{"model":"route"}"#));
        let llm_request = builder.add_llm_request_body(
            LlmApiType::Openai,
            LlmApiType::Openai,
            1,
            Bytes::from_static(br#"{"model":"upstream"}"#),
        );
        let llm_response_blob_id = builder.add_response_body(Bytes::from_static(br#"{"ok":true}"#));

        builder.finish(
            42,
            1_776_840_000_000,
            RequestLogBundleRequestSection {
                user_request_blob_id: Some(user_request_blob_id),
                user_response_blob_id: None,
                user_response_capture_state: None,
            },
            vec![RequestLogBundleAttemptSection {
                attempt_id: Some(101),
                attempt_index: 1,
                llm_request_blob_id: Some(llm_request.blob_id),
                llm_request_patch_id: llm_request.patch_id,
                llm_response_blob_id: Some(llm_response_blob_id),
                llm_response_capture_state: Some(LogBodyCaptureState::Complete),
            }],
            RequestLogBundleV2DiagnosticAssets {
                request_snapshot: Some(RequestLogBundleRequestSnapshot {
                    request_path: "/ai/openai/v1/chat/completions".to_string(),
                    operation_kind: "chat_completions".to_string(),
                    query_params: Vec::new(),
                    sanitized_original_headers: vec![RequestLogBundleHttpHeader {
                        name: "x-trace-id".to_string(),
                        value: "trace-1".to_string(),
                    }],
                }),
                candidate_manifest: Some(RequestLogBundleCandidateManifest {
                    items: vec![RequestLogBundleCandidateManifestItem {
                        candidate_position: 1,
                        route_id: Some(8),
                        route_name: Some("primary".to_string()),
                        provider_id: 2,
                        provider_key: "openai".to_string(),
                        model_id: 3,
                        model_name: "gpt-test".to_string(),
                        real_model_name: Some("gpt-real".to_string()),
                        llm_api_type: LlmApiType::Openai,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    }],
                }),
                transform_diagnostics: Some(RequestLogBundleTransformDiagnostics {
                    summary: RequestLogBundleTransformDiagnosticsSummary {
                        count: 1,
                        max_loss_level: Some(UnifiedTransformDiagnosticLossLevel::LossyMajor),
                        kinds: vec![UnifiedTransformDiagnosticKind::CapabilityDowngrade],
                        phases: vec![RequestLogBundleTransformDiagnosticPhase::Request],
                    },
                    items: vec![RequestLogBundleTransformDiagnosticItem {
                        phase: RequestLogBundleTransformDiagnosticPhase::Request,
                        diagnostic: diagnostic(),
                    }],
                }),
            },
        )
    }

    #[test]
    fn artifact_response_from_v2_bundle_reports_manifest_and_replay_capability() {
        let record = record();
        let attempt = attempt();
        let bundle = bundle();
        let decoded = DecodedRequestLogBundle::V2(bundle);

        let response = build_request_log_artifact_response(&record, &[attempt], Some(&decoded));

        assert_eq!(response.payload_manifest.bundle_version, Some(2));
        assert!(response.payload_manifest.blob_count >= 2);
        assert_eq!(
            response.request_snapshot.unwrap().request_path,
            "/ai/openai/v1/chat/completions"
        );
        assert!(response.candidate_manifest.has_asset);
        assert_eq!(response.candidate_manifest.items[0].llm_api_type, "OPENAI");
        assert!(response.transform_diagnostics.has_asset);
        assert_eq!(
            response
                .transform_diagnostics
                .summary
                .max_loss_level
                .as_deref(),
            Some("lossy_major")
        );
        assert!(response.replay_capability.attempt_upstream.available);
        assert_eq!(
            response.replay_capability.attempt_upstream.attempt_ids,
            vec![101]
        );
        assert!(response.replay_capability.gateway_request.available);
    }

    #[test]
    fn artifact_response_reports_gateway_replay_unavailable_without_request_snapshot() {
        let mut bundle = bundle();
        bundle.request_snapshot = None;
        let decoded = DecodedRequestLogBundle::V2(bundle);

        let response = build_request_log_artifact_response(&record(), &[attempt()], Some(&decoded));

        assert!(response.replay_capability.attempt_upstream.available);
        assert!(!response.replay_capability.gateway_request.available);
        assert_eq!(
            response.replay_capability.gateway_request.reasons,
            vec!["request_snapshot_missing".to_string()]
        );
    }

    #[test]
    fn artifact_response_reports_gateway_replay_unavailable_without_user_body() {
        let mut bundle = bundle();
        bundle.request_section.user_request_blob_id = None;
        let decoded = DecodedRequestLogBundle::V2(bundle);

        let response = build_request_log_artifact_response(&record(), &[attempt()], Some(&decoded));

        assert!(response.replay_capability.attempt_upstream.available);
        assert!(!response.replay_capability.gateway_request.available);
        assert_eq!(
            response.replay_capability.gateway_request.reasons,
            vec!["user_request_body_missing".to_string()]
        );
    }

    #[test]
    fn artifact_response_reports_attempt_replay_unavailable_without_attempt_body() {
        let mut bundle = bundle();
        bundle.attempt_sections[0].llm_request_blob_id = None;
        let decoded = DecodedRequestLogBundle::V2(bundle);

        let response = build_request_log_artifact_response(&record(), &[attempt()], Some(&decoded));

        assert!(!response.replay_capability.attempt_upstream.available);
        assert_eq!(
            response.replay_capability.attempt_upstream.reasons,
            vec!["no_attempt_with_complete_downstream_request_snapshot".to_string()]
        );
        assert!(response.replay_capability.gateway_request.available);
    }

    #[test]
    fn artifact_response_without_bundle_is_stable_and_replay_disabled() {
        let response = build_request_log_artifact_response(&record(), &[attempt()], None);

        assert_eq!(response.payload_manifest.log_id, 42);
        assert!(!response.candidate_manifest.has_asset);
        assert!(!response.transform_diagnostics.has_asset);
        assert!(!response.replay_capability.attempt_upstream.available);
        assert_eq!(
            response.replay_capability.attempt_upstream.reasons,
            vec!["bundle_missing".to_string()]
        );
        assert!(!response.replay_capability.gateway_request.available);
    }

    #[test]
    fn request_log_bundle_decode_accepts_gzipped_msgpack() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;

        let body = rmp_serde::to_vec_named(&bundle()).unwrap();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&body).unwrap();
        let compressed = encoder.finish().unwrap();

        let decoded = decode_request_log_bundle(&compressed).unwrap();
        match decoded {
            DecodedRequestLogBundle::V2(bundle) => {
                assert_eq!(bundle.version, 2);
                assert!(bundle.request_snapshot.is_some());
            }
            DecodedRequestLogBundle::Legacy(_) => panic!("expected v2 bundle"),
        }
    }
}
