use std::{
    collections::BTreeMap,
    fmt::Display,
    io::{Read, Write},
    sync::Arc,
};

use bytes::Bytes;
use chrono::Utc;
use cyder_tools::log::debug;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use futures::{Stream, StreamExt};
use reqwest::{
    Method,
    header::{CONTENT_ENCODING, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    config::CONFIG,
    controller::BaseError,
    cost::{
        COST_SNAPSHOT_SCHEMA_VERSION_V1, CostLedger, CostRatingContext, CostSnapshot,
        UsageNormalization, rate_cost,
    },
    database::{
        api_key::ApiKey,
        provider::ProviderApiKey,
        request_attempt::{RequestAttempt, RequestAttemptDetail},
        request_log::{RequestLog, RequestLogRecord},
        request_replay_run::{RequestReplayRun, RequestReplayRunRecord},
    },
    proxy::{
        GatewayReplayAttemptKind, GatewayReplayCandidateDecision, GatewayReplayExecutionFailure,
        GatewayReplayExecutionMetadata, GatewayReplayFinalAttempt, GatewayReplayInput,
        GatewayReplayPreparedRequest, ProxyCancellationContext, ProxyError, UtilityOperation,
        UtilityProtocol, apply_provider_request_auth_header, classify_reqwest_error,
        classify_upstream_status, execute_gateway_replay_request, preview_gateway_replay_request,
        process_success_response_body, send_with_first_byte_timeout,
    },
    schema::enum_def::{
        LlmApiType, ProviderType, RequestAttemptStatus, RequestReplayKind, RequestReplayMode,
        RequestReplaySemanticBasis, RequestReplayStatus, SchedulerAction, StorageType,
    },
    service::{
        app_state::AppState,
        cache::types::{CacheApiKey, CacheCostCatalogVersion, CacheProvider},
        request_log_artifact::{DecodedRequestLogBundle, load_request_log_bundle},
        storage::{
            Storage, get_local_storage, get_s3_storage, get_storage,
            types::{GetObjectOptions, PutObjectOptions},
        },
        transform::{StreamTransformer, unified::UnifiedTransformDiagnostic},
        vertex::get_vertex_token,
    },
    utils::{
        ID_GENERATOR,
        sse::SseParser,
        storage::{
            LogBodyCaptureState, RequestLogBundleRequestSnapshot,
            generate_replay_artifact_storage_path,
        },
    },
};

pub const REQUEST_REPLAY_ARTIFACT_VERSION: u32 = 1;
const REPLAY_PREVIEW_FINGERPRINT_VERSION: &str = "request-replay-preview-v1";
const REPLAY_PREVIEW_CONFIRMATION_TTL_MS: i64 = 15 * 60 * 1000;
const REPLAY_PREVIEW_CONFIRMATION_CLOCK_SKEW_MS: i64 = 60 * 1000;
const REPLAY_BODY_CAPTURE_COMPLETE: &str = "complete";
const REPLAY_BODY_CAPTURE_INCOMPLETE: &str = "incomplete";
const REPLAY_BODY_CAPTURE_NOT_CAPTURED: &str = "not_captured";
const REPLAY_BODY_CAPTURE_NOT_EXECUTED: &str = "not_executed";

fn log_replay_run_started(run: &RequestReplayRunRecord) {
    crate::info_event!(
        "replay.run_started",
        replay_run_id = run.id,
        request_log_id = run.source_request_log_id,
        attempt_id = run.source_attempt_id,
        replay_kind = request_replay_kind_label(&run.replay_kind),
        replay_mode = request_replay_mode_label(&run.replay_mode),
    );
}

fn log_replay_run_finished(run: &RequestReplayRunRecord) {
    let duration_ms = run
        .started_at
        .zip(run.completed_at)
        .map(|(started_at, completed_at)| completed_at.saturating_sub(started_at));
    crate::info_event!(
        "replay.run_finished",
        replay_run_id = run.id,
        request_log_id = run.source_request_log_id,
        attempt_id = run.source_attempt_id,
        replay_kind = request_replay_kind_label(&run.replay_kind),
        replay_mode = request_replay_mode_label(&run.replay_mode),
        status = request_replay_status_label(&run.status),
        http_status = run.http_status,
        error_code = run.error_code.as_deref(),
        route_id = run.executed_route_id,
        route_name = run.executed_route_name.as_deref(),
        provider_id = run.executed_provider_id,
        model_id = run.executed_model_id,
        duration_ms = duration_ms,
    );
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RequestReplayArtifact {
    pub version: u32,
    pub replay_run_id: i64,
    pub created_at: i64,
    pub source: RequestReplayArtifactSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_snapshot: Option<RequestReplayInputSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_preview: Option<RequestReplayExecutionPreview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<RequestReplayArtifactResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<RequestReplayArtifactDiff>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayArtifactSource {
    pub request_log_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<i64>,
    pub replay_kind: RequestReplayKind,
    pub replay_mode: RequestReplayMode,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequestReplayInputSnapshot {
    AttemptUpstream {
        request_uri: String,
        sanitized_request_headers: Vec<RequestReplayNameValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        llm_request_body: Option<RequestReplayBody>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<RequestReplayProviderSnapshot>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<RequestReplayModelSnapshot>,
    },
    GatewayRequest {
        request_path: String,
        query_params: Vec<RequestReplayQueryParam>,
        sanitized_original_headers: Vec<RequestReplayNameValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_request_body: Option<RequestReplayBody>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayNameValue {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayQueryParam {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default)]
    pub value_present: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RequestReplayBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_state: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayBodyCaptureMetadata {
    pub state: String,
    pub bytes_captured: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_size_bytes: Option<i64>,
    pub original_size_known: bool,
    pub truncated: bool,
    pub sha256: String,
    pub capture_limit_bytes: i64,
    pub body_encoding: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayProviderSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_api_key_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayModelSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_model_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_api_type: Option<LlmApiType>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RequestReplayExecutionPreview {
    pub semantic_basis: RequestReplaySemanticBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_route: Option<RequestReplayResolvedRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_candidate: Option<RequestReplayResolvedCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_decisions: Vec<RequestReplayCandidateDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_request_patch_summary: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_request_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub final_request_headers: Vec<RequestReplayNameValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_request_body: Option<RequestReplayBody>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayResolvedRoute {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayResolvedCandidate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_position: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_api_key_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_api_type: Option<LlmApiType>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayCandidateDecision {
    pub candidate_position: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_api_key_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_api_type: Option<LlmApiType>,
    pub attempt_status: RequestAttemptStatus,
    pub scheduler_action: SchedulerAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_uri: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RequestReplayArtifactResult {
    pub status: RequestReplayStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_headers: Vec<RequestReplayNameValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body: Option<RequestReplayBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body_capture_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body_capture: Option<RequestReplayBodyCaptureMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_normalization: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transform_diagnostics: Vec<UnifiedTransformDiagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempt_timeline: Vec<RequestReplayCandidateDecision>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequestReplayDiffBaselineKind {
    OriginalAttempt,
    OriginalRequestResult,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestReplayArtifactDiff {
    pub baseline_kind: RequestReplayDiffBaselineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_changed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers_changed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_changed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_delta: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_delta: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_lines: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RequestReplayArtifactStorage {
    pub artifact_version: i32,
    pub artifact_storage_type: StorageType,
    pub artifact_storage_key: String,
}

const REDACTED_HEADER_NAMES: &[&str] = &["authorization", "x-api-key", "x-goog-api-key", "cookie"];
const DISALLOWED_REPLAY_REQUEST_HEADER_NAMES: &[&str] = &[
    "authorization",
    "x-api-key",
    "x-goog-api-key",
    "cookie",
    "host",
    "content-length",
    "accept-encoding",
    "transfer-encoding",
];
const STRIPPED_PREVIEW_REQUEST_HEADER_NAMES: &[&str] = &[
    "host",
    "content-length",
    "accept-encoding",
    "transfer-encoding",
];
const STRIPPED_RESPONSE_HEADER_NAMES: &[&str] =
    &["set-cookie", "content-length", "transfer-encoding"];

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct AttemptReplayPreviewParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_api_key_id_override: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct AttemptReplayExecuteParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_api_key_id_override: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_mode: Option<RequestReplayMode>,
    #[serde(default)]
    pub confirm_live_request: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayReplayPreviewParams {}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayReplayExecuteParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_mode: Option<RequestReplayMode>,
    #[serde(default)]
    pub confirm_live_request: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AttemptReplayPreviewResponse {
    pub source_request_log_id: i64,
    pub source_attempt_id: i64,
    pub replay_kind: RequestReplayKind,
    pub semantic_basis: RequestReplaySemanticBasis,
    pub preview_fingerprint: String,
    pub preview_created_at: i64,
    pub selected_provider_api_key_id: i64,
    pub used_provider_api_key_override: bool,
    pub input_snapshot: RequestReplayInputSnapshot,
    pub execution_preview: RequestReplayExecutionPreview,
    pub baseline: AttemptReplayBaselineSummary,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GatewayReplayPreviewResponse {
    pub source_request_log_id: i64,
    pub replay_kind: RequestReplayKind,
    pub semantic_basis: RequestReplaySemanticBasis,
    pub preview_fingerprint: String,
    pub preview_created_at: i64,
    pub input_snapshot: RequestReplayInputSnapshot,
    pub execution_preview: RequestReplayExecutionPreview,
    pub baseline: GatewayReplayBaselineSummary,
}

#[derive(Serialize)]
struct RequestReplayPreviewFingerprintEnvelope<'a> {
    version: &'static str,
    preview_created_at: i64,
    input: &'a RequestReplayPreviewFingerprintInput,
}

#[derive(Serialize)]
struct RequestReplayPreviewFingerprintInput {
    replay_kind: RequestReplayKind,
    source_request_log_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_attempt_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_api_key_id_override: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_provider_api_key_id: Option<i64>,
    used_provider_api_key_override: bool,
    semantic_basis: RequestReplaySemanticBasis,
    input_snapshot: RequestReplayFingerprintInputSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_route: Option<RequestReplayResolvedRoute>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_candidate: Option<RequestReplayResolvedCandidate>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidate_decisions: Vec<RequestReplayCandidateDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied_request_patch_summary: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_request_uri: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    final_request_headers: Vec<RequestReplayNameValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_request_body: Option<RequestReplayFingerprintBodyDigest>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RequestReplayFingerprintInputSnapshot {
    AttemptUpstream {
        request_uri: String,
        sanitized_request_headers: Vec<RequestReplayNameValue>,
        #[serde(skip_serializing_if = "Option::is_none")]
        llm_request_body: Option<RequestReplayFingerprintBodyDigest>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider: Option<RequestReplayProviderSnapshot>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<RequestReplayModelSnapshot>,
    },
    GatewayRequest {
        request_path: String,
        query_params: Vec<RequestReplayQueryParam>,
        sanitized_original_headers: Vec<RequestReplayNameValue>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_request_body: Option<RequestReplayFingerprintBodyDigest>,
    },
}

#[derive(Serialize)]
struct RequestReplayFingerprintBodyDigest {
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture_state: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedReplayPreviewConfirmation {
    preview_created_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GatewayReplayBaselineSummary {
    pub overall_status: crate::schema::enum_def::RequestStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_nanos: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_response_body_capture_state: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AttemptReplayBaselineSummary {
    pub attempt_status: RequestAttemptStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_headers: Vec<RequestReplayNameValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body_capture_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_nanos: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_currency: Option<String>,
}

#[derive(Debug, Clone)]
struct AttemptReplaySource {
    request_log_id: i64,
    attempt: RequestAttemptDetail,
    resolved_route_id: Option<i64>,
    resolved_route_name: Option<String>,
    provider: Arc<CacheProvider>,
    llm_api_type: LlmApiType,
    request_uri: String,
    sanitized_request_headers: Vec<RequestReplayNameValue>,
    request_headers: HeaderMap,
    llm_request_body: DecodedBundleBody,
    baseline_response_headers: Vec<RequestReplayNameValue>,
    baseline_response_body: Option<DecodedBundleBody>,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
}

#[derive(Debug, Clone)]
struct GatewayReplaySource {
    request_log: RequestLogRecord,
    request_snapshot: RequestLogBundleRequestSnapshot,
    original_headers: HeaderMap,
    user_request_body: DecodedBundleBody,
    baseline_user_response_body: Option<DecodedBundleBody>,
    baseline_final_attempt: Option<RequestAttemptDetail>,
    system_api_key: Arc<CacheApiKey>,
    requested_model_name: String,
    kind: GatewayReplayAttemptKind,
}

#[derive(Debug, Clone)]
struct DecodedBundleBody {
    bytes: Bytes,
    media_type: Option<String>,
    capture_state: Option<String>,
}

#[derive(Debug, Clone)]
struct ReplayResolvedCredential {
    provider_api_key_id: i64,
    request_key: String,
    used_override: bool,
}

#[derive(Debug, Clone)]
struct AttemptReplayExecutionOutcome {
    status: RequestReplayStatus,
    http_status: Option<i32>,
    first_byte_at: Option<i64>,
    error_code: Option<String>,
    error_message: Option<String>,
    response_headers: Vec<RequestReplayNameValue>,
    response_body: Option<RequestReplayBody>,
    response_body_bytes: Option<Bytes>,
    response_body_capture_state: Option<String>,
    response_body_capture: Option<RequestReplayBodyCaptureMetadata>,
    usage_normalization: Option<UsageNormalization>,
    transform_diagnostics: Vec<UnifiedTransformDiagnostic>,
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    total_input_tokens: Option<i32>,
    total_output_tokens: Option<i32>,
    reasoning_tokens: Option<i32>,
    total_tokens: Option<i32>,
}

#[derive(Debug, Clone)]
struct ReplayResponseBodyCapture {
    body: Bytes,
    state: LogBodyCaptureState,
    original_size_bytes: Option<i64>,
    original_size_known: bool,
    truncated: bool,
    sha256: String,
    capture_limit_bytes: i64,
    body_encoding: String,
}

#[derive(Debug, Clone)]
struct GatewayReplayLiveOutcome {
    execution_preview: RequestReplayExecutionPreview,
    attempt_timeline: Vec<RequestReplayCandidateDecision>,
    outcome: AttemptReplayExecutionOutcome,
}

pub async fn preview_attempt_replay(
    app_state: &Arc<AppState>,
    request_log_id: i64,
    attempt_id: i64,
    params: AttemptReplayPreviewParams,
) -> Result<AttemptReplayPreviewResponse, BaseError> {
    let preview_created_at = Utc::now().timestamp_millis();
    let source = load_attempt_replay_source(app_state, request_log_id, attempt_id).await?;
    let credential = resolve_replay_provider_credentials(
        app_state,
        &source.provider,
        source.attempt.provider_api_key_id,
        params.provider_api_key_id_override,
    )
    .await?;
    build_attempt_replay_preview(&source, &credential, preview_created_at)
}

pub async fn execute_attempt_replay(
    app_state: &Arc<AppState>,
    request_log_id: i64,
    attempt_id: i64,
    params: AttemptReplayExecuteParams,
) -> Result<RequestReplayRunRecord, BaseError> {
    let confirmation = parse_replay_preview_confirmation(params.preview_fingerprint.as_deref())?;
    let source = load_attempt_replay_source(app_state, request_log_id, attempt_id).await?;
    let credential = resolve_replay_provider_credentials(
        app_state,
        &source.provider,
        source.attempt.provider_api_key_id,
        params.provider_api_key_id_override,
    )
    .await?;
    let preview =
        build_attempt_replay_preview(&source, &credential, confirmation.preview_created_at)?;
    ensure_replay_preview_confirmation_matches(
        params.preview_fingerprint.as_deref(),
        &preview.preview_fingerprint,
    )?;
    let replay_mode = replay_execute_mode(params.replay_mode);
    let storage = get_storage().await;
    execute_attempt_replay_with_storage(
        app_state,
        &**storage,
        &source,
        &credential,
        &preview,
        replay_mode,
        params.confirm_live_request,
    )
    .await
}

pub async fn preview_gateway_replay(
    app_state: &Arc<AppState>,
    request_log_id: i64,
    _params: GatewayReplayPreviewParams,
) -> Result<GatewayReplayPreviewResponse, BaseError> {
    let preview_created_at = Utc::now().timestamp_millis();
    let source = load_gateway_replay_source(request_log_id).await?;
    let prepared = preview_gateway_replay_request(
        Arc::clone(app_state),
        gateway_replay_input_from_source(&source),
    )
    .await
    .map_err(proxy_error_to_param_error)?;

    build_gateway_replay_preview(&source, &prepared, preview_created_at)
}

pub async fn execute_gateway_replay(
    app_state: &Arc<AppState>,
    request_log_id: i64,
    params: GatewayReplayExecuteParams,
) -> Result<RequestReplayRunRecord, BaseError> {
    let confirmation = parse_replay_preview_confirmation(params.preview_fingerprint.as_deref())?;
    let source = load_gateway_replay_source(request_log_id).await?;
    let prepared = preview_gateway_replay_request(
        Arc::clone(app_state),
        gateway_replay_input_from_source(&source),
    )
    .await
    .map_err(proxy_error_to_param_error)?;
    let preview =
        build_gateway_replay_preview(&source, &prepared, confirmation.preview_created_at)?;
    ensure_replay_preview_confirmation_matches(
        params.preview_fingerprint.as_deref(),
        &preview.preview_fingerprint,
    )?;
    let replay_mode = replay_execute_mode(params.replay_mode);
    let storage = get_storage().await;

    execute_gateway_replay_with_storage(
        app_state,
        &**storage,
        &source,
        &prepared,
        &preview,
        replay_mode,
        params.confirm_live_request,
    )
    .await
}

fn replay_execute_mode(mode: Option<RequestReplayMode>) -> RequestReplayMode {
    mode.unwrap_or(RequestReplayMode::Live)
}

async fn load_attempt_replay_source(
    app_state: &Arc<AppState>,
    request_log_id: i64,
    attempt_id: i64,
) -> Result<AttemptReplaySource, BaseError> {
    let request_log = RequestLog::get_by_id(request_log_id)?;
    let attempt = crate::database::request_attempt::RequestAttempt::get_by_id(attempt_id)?;

    if attempt.request_log_id != request_log_id {
        return Err(BaseError::NotFound(Some(format!(
            "Request attempt {} does not belong to request_log {}",
            attempt_id, request_log_id
        ))));
    }

    let Some(provider_id) = attempt.provider_id.or(request_log.final_provider_id) else {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Attempt {} does not have a provider snapshot",
            attempt_id
        ))));
    };
    let Some(model_id) = attempt.model_id.or(request_log.final_model_id) else {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Attempt {} does not have a model snapshot",
            attempt_id
        ))));
    };

    let provider = app_state
        .get_provider_by_id(provider_id)
        .await?
        .ok_or_else(|| BaseError::NotFound(Some(format!("Provider {} not found", provider_id))))?;
    let model = app_state
        .get_model_by_id(model_id)
        .await?
        .ok_or_else(|| BaseError::NotFound(Some(format!("Model {} not found", model_id))))?;

    let bundle = load_request_log_bundle(&request_log)
        .await?
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "Request log {} does not have a persisted bundle",
                request_log_id
            )))
        })?;

    let request_uri = require_non_empty(
        attempt.request_uri.as_deref(),
        format!("Attempt {} is missing downstream request URI", attempt_id),
    )?;
    let raw_request_headers = require_non_empty(
        attempt.request_headers_json.as_deref(),
        format!(
            "Attempt {} is missing downstream request headers",
            attempt_id
        ),
    )?;
    let sanitized_request_headers =
        parse_name_values_json_map(&raw_request_headers, "request headers")?;
    let request_headers = build_header_map_from_name_values(&sanitized_request_headers)?;
    let llm_request_body = extract_attempt_request_body(&bundle, &attempt)?;
    let baseline_response_headers = match attempt.response_headers_json.as_deref() {
        Some(raw) if !raw.trim().is_empty() => parse_name_values_json_map(raw, "response headers")?,
        _ => Vec::new(),
    };
    let baseline_response_body = extract_attempt_response_body(&bundle, &attempt)?;

    let cost_catalog_version = match attempt.cost_catalog_version_id {
        Some(cost_catalog_version_id) => app_state
            .get_cost_catalog_version_by_id(cost_catalog_version_id)
            .await?
            .map(|version| (*version).clone()),
        None => app_state
            .get_cost_catalog_version_by_model(model.id, Utc::now().timestamp_millis())
            .await?
            .map(|version| (*version).clone()),
    };
    let llm_api_type = attempt
        .llm_api_type
        .unwrap_or_else(|| infer_llm_api_type(request_log.user_api_type, &provider));

    Ok(AttemptReplaySource {
        request_log_id,
        attempt,
        resolved_route_id: request_log.resolved_route_id,
        resolved_route_name: request_log.resolved_route_name,
        provider,
        llm_api_type,
        request_uri,
        sanitized_request_headers,
        request_headers,
        llm_request_body,
        baseline_response_headers,
        baseline_response_body,
        cost_catalog_version,
    })
}

async fn load_gateway_replay_source(request_log_id: i64) -> Result<GatewayReplaySource, BaseError> {
    let request_log = RequestLog::get_by_id(request_log_id)?;
    let bundle = load_request_log_bundle(&request_log)
        .await?
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "Request log {} does not have a persisted bundle",
                request_log_id
            )))
        })?;
    let DecodedRequestLogBundle::V2(bundle) = bundle else {
        return Err(BaseError::ParamInvalid(Some(
            "Gateway replay requires a v2 request log bundle with request snapshot".to_string(),
        )));
    };
    let request_snapshot = bundle.request_snapshot.clone().ok_or_else(|| {
        BaseError::ParamInvalid(Some(format!(
            "Request log {} is missing request snapshot",
            request_log_id
        )))
    })?;
    if request_snapshot.request_path.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Request log {} has an empty request snapshot path",
            request_log_id
        ))));
    }

    let user_request_body = extract_gateway_user_request_body(&bundle)?;
    let baseline_user_response_body = extract_gateway_user_response_body(&bundle);
    let baseline_final_attempt = load_gateway_baseline_final_attempt(&request_log);
    let request_value =
        serde_json::from_slice::<Value>(&user_request_body.bytes).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Gateway replay user request body is not valid JSON: {}",
                err
            )))
        })?;
    let (requested_model_name, kind) =
        gateway_replay_kind_from_snapshot(&request_log, &request_snapshot, &request_value)?;
    let system_api_key = Arc::new(load_cache_api_key_by_id(request_log.api_key_id)?);
    let original_headers = header_map_from_snapshot(&request_snapshot)?;

    Ok(GatewayReplaySource {
        request_log,
        request_snapshot,
        original_headers,
        user_request_body,
        baseline_user_response_body,
        baseline_final_attempt,
        system_api_key,
        requested_model_name,
        kind,
    })
}

fn load_gateway_baseline_final_attempt(
    request_log: &RequestLogRecord,
) -> Option<RequestAttemptDetail> {
    let final_attempt_id = request_log.final_attempt_id?;
    match RequestAttempt::get_by_id(final_attempt_id) {
        Ok(attempt) if attempt.request_log_id == request_log.id => Some(attempt),
        Ok(attempt) => {
            debug!(
                "Gateway replay baseline final attempt {} belongs to request_log {}, expected {}",
                final_attempt_id, attempt.request_log_id, request_log.id
            );
            None
        }
        Err(err) => {
            debug!(
                "Gateway replay could not load baseline final attempt {} for request_log {}: {:?}",
                final_attempt_id, request_log.id, err
            );
            None
        }
    }
}

fn gateway_replay_input_from_source(source: &GatewayReplaySource) -> GatewayReplayInput {
    GatewayReplayInput {
        system_api_key: Arc::clone(&source.system_api_key),
        requested_model_name: source.requested_model_name.clone(),
        query_params: source.request_snapshot.query_params.clone(),
        original_headers: source.original_headers.clone(),
        request_snapshot: source.request_snapshot.clone(),
        client_ip_addr: source.request_log.client_ip.clone(),
        start_time: Utc::now().timestamp_millis(),
        original_request_body: source.user_request_body.bytes.clone(),
        kind: source.kind.clone(),
    }
}

fn build_gateway_replay_preview(
    source: &GatewayReplaySource,
    prepared: &GatewayReplayPreparedRequest,
    preview_created_at: i64,
) -> Result<GatewayReplayPreviewResponse, BaseError> {
    let mut response = GatewayReplayPreviewResponse {
        source_request_log_id: source.request_log.id,
        replay_kind: RequestReplayKind::GatewayRequest,
        semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
        preview_fingerprint: String::new(),
        preview_created_at,
        input_snapshot: gateway_input_snapshot(source),
        execution_preview: execution_preview_from_gateway_prepared(prepared),
        baseline: GatewayReplayBaselineSummary {
            overall_status: source.request_log.overall_status.clone(),
            final_error_code: source.request_log.final_error_code.clone(),
            final_error_message: source.request_log.final_error_message.clone(),
            total_tokens: source.request_log.total_tokens,
            estimated_cost_nanos: source.request_log.estimated_cost_nanos,
            estimated_cost_currency: source.request_log.estimated_cost_currency.clone(),
            user_response_body_capture_state: source
                .baseline_user_response_body
                .as_ref()
                .and_then(|body| body.capture_state.clone()),
        },
    };
    response.preview_fingerprint = gateway_replay_preview_fingerprint(&response, source, prepared)?;
    Ok(response)
}

fn gateway_input_snapshot(source: &GatewayReplaySource) -> RequestReplayInputSnapshot {
    RequestReplayInputSnapshot::GatewayRequest {
        request_path: source.request_snapshot.request_path.clone(),
        query_params: replay_query_params_from_snapshot(&source.request_snapshot.query_params),
        sanitized_original_headers: source
            .request_snapshot
            .sanitized_original_headers
            .iter()
            .map(|item| RequestReplayNameValue {
                name: item.name.clone(),
                value: Some(item.value.clone()),
            })
            .collect(),
        user_request_body: Some(body_to_replay_body(&source.user_request_body)),
    }
}

fn execution_preview_from_gateway_prepared(
    prepared: &GatewayReplayPreparedRequest,
) -> RequestReplayExecutionPreview {
    RequestReplayExecutionPreview {
        semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
        resolved_route: Some(RequestReplayResolvedRoute {
            route_id: prepared.resolved_route_id,
            route_name: prepared.resolved_route_name.clone(),
        }),
        resolved_candidate: Some(RequestReplayResolvedCandidate {
            candidate_position: Some(prepared.candidate_position),
            provider_id: Some(prepared.provider_id),
            provider_api_key_id: Some(prepared.provider_api_key_id),
            model_id: Some(prepared.model_id),
            llm_api_type: Some(prepared.llm_api_type),
        }),
        candidate_decisions: prepared
            .candidate_decisions
            .iter()
            .map(|decision| RequestReplayCandidateDecision {
                candidate_position: decision.candidate_position,
                provider_id: decision.provider_id,
                provider_api_key_id: decision.provider_api_key_id,
                model_id: decision.model_id,
                llm_api_type: decision.llm_api_type,
                attempt_status: decision.attempt_status,
                scheduler_action: decision.scheduler_action,
                error_code: decision.error_code.clone(),
                error_message: decision.error_message.clone(),
                request_uri: decision.request_uri.clone(),
            })
            .collect(),
        applied_request_patch_summary: prepared.applied_request_patch_summary.clone(),
        final_request_uri: Some(prepared.final_request_uri.clone()),
        final_request_headers: serialize_headers_for_output(
            &prepared.final_request_headers,
            STRIPPED_PREVIEW_REQUEST_HEADER_NAMES,
        ),
        final_request_body: Some(body_from_bytes(
            &prepared.final_request_body,
            Some("application/json".to_string()),
            Some("complete".to_string()),
        )),
    }
}

fn build_attempt_replay_preview(
    source: &AttemptReplaySource,
    credential: &ReplayResolvedCredential,
    preview_created_at: i64,
) -> Result<AttemptReplayPreviewResponse, BaseError> {
    let final_request_headers = build_replay_request_headers(
        &source.request_headers,
        &source.provider,
        source.llm_api_type,
        &credential.request_key,
    )?;

    let mut response = AttemptReplayPreviewResponse {
        source_request_log_id: source.request_log_id,
        source_attempt_id: source.attempt.id,
        replay_kind: RequestReplayKind::AttemptUpstream,
        semantic_basis: RequestReplaySemanticBasis::HistoricalAttemptSnapshot,
        preview_fingerprint: String::new(),
        preview_created_at,
        selected_provider_api_key_id: credential.provider_api_key_id,
        used_provider_api_key_override: credential.used_override,
        input_snapshot: RequestReplayInputSnapshot::AttemptUpstream {
            request_uri: source.request_uri.clone(),
            sanitized_request_headers: source.sanitized_request_headers.clone(),
            llm_request_body: Some(body_to_replay_body(&source.llm_request_body)),
            provider: Some(provider_snapshot_from_attempt(&source.attempt)),
            model: Some(model_snapshot_from_attempt(
                &source.attempt,
                source.llm_api_type,
            )),
        },
        execution_preview: RequestReplayExecutionPreview {
            semantic_basis: RequestReplaySemanticBasis::HistoricalAttemptSnapshot,
            resolved_route: Some(RequestReplayResolvedRoute {
                route_id: source.resolved_route_id,
                route_name: source.resolved_route_name.clone(),
            }),
            resolved_candidate: Some(RequestReplayResolvedCandidate {
                candidate_position: Some(source.attempt.candidate_position),
                provider_id: source.attempt.provider_id,
                provider_api_key_id: Some(credential.provider_api_key_id),
                model_id: source.attempt.model_id,
                llm_api_type: Some(source.llm_api_type),
            }),
            candidate_decisions: Vec::new(),
            applied_request_patch_summary: None,
            final_request_uri: Some(source.request_uri.clone()),
            final_request_headers: serialize_headers_for_output(
                &final_request_headers,
                STRIPPED_PREVIEW_REQUEST_HEADER_NAMES,
            ),
            final_request_body: Some(body_to_replay_body(&source.llm_request_body)),
        },
        baseline: AttemptReplayBaselineSummary {
            attempt_status: source.attempt.attempt_status,
            http_status: source.attempt.http_status,
            response_headers: source.baseline_response_headers.clone(),
            response_body_capture_state: source
                .baseline_response_body
                .as_ref()
                .and_then(|body| body.capture_state.clone())
                .or_else(|| source.attempt.llm_response_capture_state.clone()),
            total_tokens: source.attempt.total_tokens,
            estimated_cost_nanos: source.attempt.estimated_cost_nanos,
            estimated_cost_currency: source.attempt.estimated_cost_currency.clone(),
        },
    };
    response.preview_fingerprint =
        attempt_replay_preview_fingerprint(&response, source, credential)?;
    Ok(response)
}

fn attempt_replay_preview_fingerprint(
    response: &AttemptReplayPreviewResponse,
    source: &AttemptReplaySource,
    credential: &ReplayResolvedCredential,
) -> Result<String, BaseError> {
    let input = RequestReplayPreviewFingerprintInput {
        replay_kind: response.replay_kind,
        source_request_log_id: response.source_request_log_id,
        source_attempt_id: Some(response.source_attempt_id),
        provider_api_key_id_override: credential
            .used_override
            .then_some(credential.provider_api_key_id),
        selected_provider_api_key_id: Some(credential.provider_api_key_id),
        used_provider_api_key_override: credential.used_override,
        semantic_basis: response.semantic_basis,
        input_snapshot: RequestReplayFingerprintInputSnapshot::AttemptUpstream {
            request_uri: canonical_uri_for_fingerprint(&source.request_uri),
            sanitized_request_headers: canonical_name_values(
                &source.sanitized_request_headers,
                true,
            ),
            llm_request_body: Some(body_digest_from_decoded_body(&source.llm_request_body)),
            provider: Some(provider_snapshot_from_attempt(&source.attempt)),
            model: Some(model_snapshot_from_attempt(
                &source.attempt,
                source.llm_api_type,
            )),
        },
        resolved_route: response.execution_preview.resolved_route.clone(),
        resolved_candidate: response.execution_preview.resolved_candidate.clone(),
        candidate_decisions: response.execution_preview.candidate_decisions.clone(),
        applied_request_patch_summary: response
            .execution_preview
            .applied_request_patch_summary
            .clone(),
        final_request_uri: response
            .execution_preview
            .final_request_uri
            .as_deref()
            .map(canonical_uri_for_fingerprint),
        final_request_headers: canonical_name_values(
            &response.execution_preview.final_request_headers,
            true,
        ),
        final_request_body: Some(body_digest_from_decoded_body(&source.llm_request_body)),
    };

    build_replay_preview_fingerprint(response.preview_created_at, &input)
}

fn gateway_replay_preview_fingerprint(
    response: &GatewayReplayPreviewResponse,
    source: &GatewayReplaySource,
    prepared: &GatewayReplayPreparedRequest,
) -> Result<String, BaseError> {
    let input = RequestReplayPreviewFingerprintInput {
        replay_kind: response.replay_kind,
        source_request_log_id: response.source_request_log_id,
        source_attempt_id: None,
        provider_api_key_id_override: None,
        selected_provider_api_key_id: Some(prepared.provider_api_key_id),
        used_provider_api_key_override: false,
        semantic_basis: response.semantic_basis,
        input_snapshot: RequestReplayFingerprintInputSnapshot::GatewayRequest {
            request_path: source.request_snapshot.request_path.clone(),
            query_params: replay_query_params_from_snapshot(&source.request_snapshot.query_params),
            sanitized_original_headers: canonical_name_values(
                &source
                    .request_snapshot
                    .sanitized_original_headers
                    .iter()
                    .map(|item| RequestReplayNameValue {
                        name: item.name.clone(),
                        value: Some(item.value.clone()),
                    })
                    .collect::<Vec<_>>(),
                true,
            ),
            user_request_body: Some(body_digest_from_decoded_body(&source.user_request_body)),
        },
        resolved_route: response.execution_preview.resolved_route.clone(),
        resolved_candidate: response.execution_preview.resolved_candidate.clone(),
        candidate_decisions: response.execution_preview.candidate_decisions.clone(),
        applied_request_patch_summary: response
            .execution_preview
            .applied_request_patch_summary
            .clone(),
        final_request_uri: response
            .execution_preview
            .final_request_uri
            .as_deref()
            .map(canonical_uri_for_fingerprint),
        final_request_headers: canonical_name_values(
            &response.execution_preview.final_request_headers,
            true,
        ),
        final_request_body: Some(RequestReplayFingerprintBodyDigest {
            sha256: sha256_hex(&prepared.final_request_body),
            media_type: Some("application/json".to_string()),
            capture_state: Some("complete".to_string()),
        }),
    };

    build_replay_preview_fingerprint(response.preview_created_at, &input)
}

fn build_replay_preview_fingerprint(
    preview_created_at: i64,
    input: &RequestReplayPreviewFingerprintInput,
) -> Result<String, BaseError> {
    let envelope = RequestReplayPreviewFingerprintEnvelope {
        version: REPLAY_PREVIEW_FINGERPRINT_VERSION,
        preview_created_at,
        input,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|err| {
        BaseError::InternalServerError(Some(format!(
            "Failed to build replay preview fingerprint: {}",
            err
        )))
    })?;
    Ok(format!(
        "{}:{}:{}",
        REPLAY_PREVIEW_FINGERPRINT_VERSION,
        preview_created_at,
        sha256_hex(&bytes)
    ))
}

fn parse_replay_preview_confirmation(
    fingerprint: Option<&str>,
) -> Result<ParsedReplayPreviewConfirmation, BaseError> {
    let raw = fingerprint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(replay_preview_confirmation_missing)?;
    let mut parts = raw.split(':');
    let version = parts.next();
    let created_at = parts.next();
    let digest = parts.next();

    if parts.next().is_some()
        || version != Some(REPLAY_PREVIEW_FINGERPRINT_VERSION)
        || digest.is_none_or(|value| {
            value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err(BaseError::ParamInvalid(Some(
            "Replay preview confirmation is invalid; regenerate preview before execute."
                .to_string(),
        )));
    }

    let preview_created_at = created_at
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(
                "Replay preview confirmation is invalid; regenerate preview before execute."
                    .to_string(),
            ))
        })?;
    let now = Utc::now().timestamp_millis();
    if preview_created_at <= 0
        || now.saturating_sub(preview_created_at) > REPLAY_PREVIEW_CONFIRMATION_TTL_MS
        || preview_created_at.saturating_sub(now) > REPLAY_PREVIEW_CONFIRMATION_CLOCK_SKEW_MS
    {
        return Err(BaseError::ParamInvalid(Some(
            "Replay preview confirmation expired; regenerate preview before execute.".to_string(),
        )));
    }

    Ok(ParsedReplayPreviewConfirmation { preview_created_at })
}

fn ensure_replay_preview_confirmation_matches(
    provided: Option<&str>,
    expected: &str,
) -> Result<(), BaseError> {
    let provided = provided
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(replay_preview_confirmation_missing)?;
    if provided != expected {
        return Err(BaseError::ParamInvalid(Some(
            "Replay preview confirmation mismatch; regenerate preview before execute.".to_string(),
        )));
    }
    Ok(())
}

fn replay_preview_confirmation_missing() -> BaseError {
    BaseError::ParamInvalid(Some(
        "Replay preview confirmation is missing; regenerate preview before execute.".to_string(),
    ))
}

async fn execute_attempt_replay_with_storage(
    app_state: &Arc<AppState>,
    storage: &dyn Storage,
    source: &AttemptReplaySource,
    credential: &ReplayResolvedCredential,
    preview: &AttemptReplayPreviewResponse,
    replay_mode: RequestReplayMode,
    confirm_live_request: bool,
) -> Result<RequestReplayRunRecord, BaseError> {
    let created_at = Utc::now().timestamp_millis();
    let mut run = RequestReplayRun::insert(&RequestReplayRun {
        id: ID_GENERATOR.generate_id(),
        source_request_log_id: source.request_log_id,
        source_attempt_id: Some(source.attempt.id),
        replay_kind: RequestReplayKind::AttemptUpstream,
        replay_mode,
        semantic_basis: RequestReplaySemanticBasis::HistoricalAttemptSnapshot,
        status: RequestReplayStatus::Pending,
        created_at,
        updated_at: created_at,
        ..Default::default()
    })?;
    log_replay_run_started(&run);

    let artifact_source = RequestReplayArtifactSource {
        request_log_id: source.request_log_id,
        attempt_id: Some(source.attempt.id),
        replay_kind: RequestReplayKind::AttemptUpstream,
        replay_mode,
    };

    if replay_mode == RequestReplayMode::DryRun {
        let started_at = Utc::now().timestamp_millis();
        let completed_at = Utc::now().timestamp_millis();
        let diff = dry_run_diff(
            "Attempt replay dry-run persisted the materialized upstream request; no upstream request was sent.",
            RequestReplayDiffBaselineKind::OriginalAttempt,
        );
        run.status = RequestReplayStatus::Success;
        run.started_at = Some(started_at);
        run.executed_route_id = source.resolved_route_id;
        run.executed_route_name = source.resolved_route_name.clone();
        run.executed_provider_id = source.attempt.provider_id;
        run.executed_provider_api_key_id = Some(preview.selected_provider_api_key_id);
        run.executed_model_id = source.attempt.model_id;
        run.executed_llm_api_type = Some(source.llm_api_type);
        run.downstream_request_uri = Some(source.request_uri.clone());
        run.diff_summary_json = serde_json::to_string(&diff).ok();
        run.completed_at = Some(completed_at);
        run.updated_at = completed_at;
        let artifact = RequestReplayArtifact {
            version: REQUEST_REPLAY_ARTIFACT_VERSION,
            replay_run_id: run.id,
            created_at,
            source: artifact_source,
            input_snapshot: Some(preview.input_snapshot.clone()),
            execution_preview: Some(preview.execution_preview.clone()),
            result: Some(dry_run_result(Vec::new(), Vec::new())),
            diff: Some(diff.clone()),
        };
        let locator = store_replay_artifact_for_run(storage, &mut run, &artifact).await?;
        set_replay_artifact_locator(&mut run, &locator);
        let persisted = RequestReplayRun::update(&run)?;
        log_replay_run_finished(&persisted);
        return Ok(persisted);
    }

    if !confirm_live_request {
        let diff = rejected_diff(
            "Replay rejected because confirm_live_request was false.",
            RequestReplayDiffBaselineKind::OriginalAttempt,
        );
        let completed_at = Utc::now().timestamp_millis();
        run.status = RequestReplayStatus::Rejected;
        run.error_code = Some("replay_rejected".to_string());
        run.error_message =
            Some("Replay rejected because confirm_live_request was false.".to_string());
        run.executed_route_id = source.resolved_route_id;
        run.executed_route_name = source.resolved_route_name.clone();
        run.executed_provider_id = source.attempt.provider_id;
        run.executed_provider_api_key_id = Some(preview.selected_provider_api_key_id);
        run.executed_model_id = source.attempt.model_id;
        run.executed_llm_api_type = Some(source.llm_api_type);
        run.downstream_request_uri = Some(source.request_uri.clone());
        run.diff_summary_json = serde_json::to_string(&diff).ok();
        run.completed_at = Some(completed_at);
        run.updated_at = completed_at;
        let artifact = RequestReplayArtifact {
            version: REQUEST_REPLAY_ARTIFACT_VERSION,
            replay_run_id: run.id,
            created_at,
            source: artifact_source,
            input_snapshot: Some(preview.input_snapshot.clone()),
            execution_preview: Some(preview.execution_preview.clone()),
            result: Some(RequestReplayArtifactResult {
                status: RequestReplayStatus::Rejected,
                http_status: None,
                response_headers: Vec::new(),
                response_body: None,
                response_body_capture_state: Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED.to_string()),
                response_body_capture: None,
                usage_normalization: None,
                transform_diagnostics: Vec::new(),
                attempt_timeline: Vec::new(),
            }),
            diff: Some(diff.clone()),
        };
        let locator = store_replay_artifact_for_run(storage, &mut run, &artifact).await?;
        set_replay_artifact_locator(&mut run, &locator);
        let persisted = RequestReplayRun::update(&run)?;
        log_replay_run_finished(&persisted);
        return Ok(persisted);
    }

    let started_at = Utc::now().timestamp_millis();
    run.status = RequestReplayStatus::Running;
    run.started_at = Some(started_at);
    run.executed_route_id = source.resolved_route_id;
    run.executed_route_name = source.resolved_route_name.clone();
    run.executed_provider_id = source.attempt.provider_id;
    run.executed_provider_api_key_id = Some(preview.selected_provider_api_key_id);
    run.executed_model_id = source.attempt.model_id;
    run.executed_llm_api_type = Some(source.llm_api_type);
    run.downstream_request_uri = Some(source.request_uri.clone());
    run.updated_at = started_at;
    run = RequestReplayRun::update(&run)?;

    let outcome = perform_attempt_replay_execution(app_state, source, credential).await;
    let diff = build_attempt_replay_diff(source, &outcome);
    let completed_at = Utc::now().timestamp_millis();
    let artifact = RequestReplayArtifact {
        version: REQUEST_REPLAY_ARTIFACT_VERSION,
        replay_run_id: run.id,
        created_at,
        source: artifact_source,
        input_snapshot: Some(preview.input_snapshot.clone()),
        execution_preview: Some(preview.execution_preview.clone()),
        result: Some(RequestReplayArtifactResult {
            status: outcome.status,
            http_status: outcome.http_status,
            response_headers: outcome.response_headers.clone(),
            response_body: outcome.response_body.clone(),
            response_body_capture_state: outcome.response_body_capture_state.clone(),
            response_body_capture: outcome.response_body_capture.clone(),
            usage_normalization: outcome
                .usage_normalization
                .as_ref()
                .and_then(|value| serde_json::to_value(value).ok()),
            transform_diagnostics: outcome.transform_diagnostics.clone(),
            attempt_timeline: Vec::new(),
        }),
        diff: Some(diff.clone()),
    };
    run.status = outcome.status;
    run.http_status = outcome.http_status;
    run.first_byte_at = outcome.first_byte_at;
    run.error_code = outcome.error_code;
    run.error_message = outcome.error_message;
    run.total_input_tokens = outcome.total_input_tokens;
    run.total_output_tokens = outcome.total_output_tokens;
    run.reasoning_tokens = outcome.reasoning_tokens;
    run.total_tokens = outcome.total_tokens;
    run.estimated_cost_nanos = outcome.estimated_cost_nanos;
    run.estimated_cost_currency = outcome.estimated_cost_currency;
    run.diff_summary_json = serde_json::to_string(&diff).ok();
    run.completed_at = Some(completed_at);
    run.updated_at = completed_at;
    let locator = store_replay_artifact_for_run(storage, &mut run, &artifact).await?;
    set_replay_artifact_locator(&mut run, &locator);
    let persisted = RequestReplayRun::update(&run)?;
    log_replay_run_finished(&persisted);
    Ok(persisted)
}

async fn execute_gateway_replay_with_storage(
    app_state: &Arc<AppState>,
    storage: &dyn Storage,
    source: &GatewayReplaySource,
    prepared: &GatewayReplayPreparedRequest,
    preview: &GatewayReplayPreviewResponse,
    replay_mode: RequestReplayMode,
    confirm_live_request: bool,
) -> Result<RequestReplayRunRecord, BaseError> {
    let created_at = Utc::now().timestamp_millis();
    let mut run = RequestReplayRun::insert(&RequestReplayRun {
        id: ID_GENERATOR.generate_id(),
        source_request_log_id: source.request_log.id,
        source_attempt_id: None,
        replay_kind: RequestReplayKind::GatewayRequest,
        replay_mode,
        semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
        status: RequestReplayStatus::Pending,
        created_at,
        updated_at: created_at,
        ..Default::default()
    })?;
    log_replay_run_started(&run);

    let artifact_source = RequestReplayArtifactSource {
        request_log_id: source.request_log.id,
        attempt_id: None,
        replay_kind: RequestReplayKind::GatewayRequest,
        replay_mode,
    };

    if replay_mode == RequestReplayMode::DryRun {
        let started_at = Utc::now().timestamp_millis();
        let completed_at = Utc::now().timestamp_millis();
        let diff = dry_run_diff(
            "Gateway replay dry-run persisted the materialized request; no upstream request was sent.",
            RequestReplayDiffBaselineKind::OriginalRequestResult,
        );
        run.status = RequestReplayStatus::Success;
        run.started_at = Some(started_at);
        fill_gateway_run_target(&mut run, prepared);
        run.diff_summary_json = serde_json::to_string(&diff).ok();
        run.completed_at = Some(completed_at);
        run.updated_at = completed_at;
        let artifact = RequestReplayArtifact {
            version: REQUEST_REPLAY_ARTIFACT_VERSION,
            replay_run_id: run.id,
            created_at,
            source: artifact_source,
            input_snapshot: Some(preview.input_snapshot.clone()),
            execution_preview: Some(preview.execution_preview.clone()),
            result: Some(dry_run_result(
                prepared.transform_diagnostics.clone(),
                preview.execution_preview.candidate_decisions.clone(),
            )),
            diff: Some(diff.clone()),
        };
        let locator = store_replay_artifact_for_run(storage, &mut run, &artifact).await?;
        set_replay_artifact_locator(&mut run, &locator);
        let persisted = RequestReplayRun::update(&run)?;
        log_replay_run_finished(&persisted);
        return Ok(persisted);
    }

    if !confirm_live_request {
        let diff = rejected_diff(
            "Gateway replay rejected because confirm_live_request was false.",
            RequestReplayDiffBaselineKind::OriginalRequestResult,
        );
        let completed_at = Utc::now().timestamp_millis();
        run.status = RequestReplayStatus::Rejected;
        run.error_code = Some("replay_rejected".to_string());
        run.error_message =
            Some("Gateway replay rejected because confirm_live_request was false.".to_string());
        fill_gateway_run_target(&mut run, prepared);
        run.diff_summary_json = serde_json::to_string(&diff).ok();
        run.completed_at = Some(completed_at);
        run.updated_at = completed_at;
        let artifact = RequestReplayArtifact {
            version: REQUEST_REPLAY_ARTIFACT_VERSION,
            replay_run_id: run.id,
            created_at,
            source: artifact_source,
            input_snapshot: Some(preview.input_snapshot.clone()),
            execution_preview: Some(preview.execution_preview.clone()),
            result: Some(RequestReplayArtifactResult {
                status: RequestReplayStatus::Rejected,
                http_status: None,
                response_headers: Vec::new(),
                response_body: None,
                response_body_capture_state: Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED.to_string()),
                response_body_capture: None,
                usage_normalization: None,
                transform_diagnostics: prepared.transform_diagnostics.clone(),
                attempt_timeline: Vec::new(),
            }),
            diff: Some(diff.clone()),
        };
        let locator = store_replay_artifact_for_run(storage, &mut run, &artifact).await?;
        set_replay_artifact_locator(&mut run, &locator);
        let persisted = RequestReplayRun::update(&run)?;
        log_replay_run_finished(&persisted);
        return Ok(persisted);
    }

    let started_at = Utc::now().timestamp_millis();
    run.status = RequestReplayStatus::Running;
    run.started_at = Some(started_at);
    run.updated_at = started_at;
    run = RequestReplayRun::update(&run)?;

    let live = perform_gateway_replay_execution(app_state, source).await;
    let outcome = &live.outcome;
    let diff = build_gateway_replay_diff(source, &live.execution_preview, outcome);
    let completed_at = Utc::now().timestamp_millis();
    let artifact = RequestReplayArtifact {
        version: REQUEST_REPLAY_ARTIFACT_VERSION,
        replay_run_id: run.id,
        created_at,
        source: artifact_source,
        input_snapshot: Some(preview.input_snapshot.clone()),
        execution_preview: Some(live.execution_preview.clone()),
        result: Some(RequestReplayArtifactResult {
            status: outcome.status,
            http_status: outcome.http_status,
            response_headers: outcome.response_headers.clone(),
            response_body: outcome.response_body.clone(),
            response_body_capture_state: outcome.response_body_capture_state.clone(),
            response_body_capture: outcome.response_body_capture.clone(),
            usage_normalization: outcome
                .usage_normalization
                .as_ref()
                .and_then(|value| serde_json::to_value(value).ok()),
            transform_diagnostics: outcome.transform_diagnostics.clone(),
            attempt_timeline: live.attempt_timeline.clone(),
        }),
        diff: Some(diff.clone()),
    };
    run.status = outcome.status;
    fill_gateway_run_target_from_live(&mut run, &live.execution_preview);
    run.http_status = outcome.http_status;
    run.first_byte_at = outcome.first_byte_at;
    run.error_code = outcome.error_code.clone();
    run.error_message = outcome.error_message.clone();
    run.total_input_tokens = outcome.total_input_tokens;
    run.total_output_tokens = outcome.total_output_tokens;
    run.reasoning_tokens = outcome.reasoning_tokens;
    run.total_tokens = outcome.total_tokens;
    run.estimated_cost_nanos = outcome.estimated_cost_nanos;
    run.estimated_cost_currency = outcome.estimated_cost_currency.clone();
    run.diff_summary_json = serde_json::to_string(&diff).ok();
    run.completed_at = Some(completed_at);
    run.updated_at = completed_at;
    let locator = store_replay_artifact_for_run(storage, &mut run, &artifact).await?;
    set_replay_artifact_locator(&mut run, &locator);
    let persisted = RequestReplayRun::update(&run)?;
    log_replay_run_finished(&persisted);
    Ok(persisted)
}

fn fill_gateway_run_target(run: &mut RequestReplayRun, prepared: &GatewayReplayPreparedRequest) {
    run.executed_route_id = prepared.resolved_route_id;
    run.executed_route_name = prepared.resolved_route_name.clone();
    run.executed_provider_id = Some(prepared.provider_id);
    run.executed_provider_api_key_id = Some(prepared.provider_api_key_id);
    run.executed_model_id = Some(prepared.model_id);
    run.executed_llm_api_type = Some(prepared.llm_api_type);
    run.downstream_request_uri = Some(prepared.final_request_uri.clone());
}

fn fill_gateway_run_target_from_live(
    run: &mut RequestReplayRun,
    execution_preview: &RequestReplayExecutionPreview,
) {
    run.executed_route_id = execution_preview
        .resolved_route
        .as_ref()
        .and_then(|route| route.route_id);
    run.executed_route_name = execution_preview
        .resolved_route
        .as_ref()
        .and_then(|route| route.route_name.clone());
    run.executed_provider_id = execution_preview
        .resolved_candidate
        .as_ref()
        .and_then(|candidate| candidate.provider_id);
    run.executed_provider_api_key_id = execution_preview
        .resolved_candidate
        .as_ref()
        .and_then(|candidate| candidate.provider_api_key_id);
    run.executed_model_id = execution_preview
        .resolved_candidate
        .as_ref()
        .and_then(|candidate| candidate.model_id);
    run.executed_llm_api_type = execution_preview
        .resolved_candidate
        .as_ref()
        .and_then(|candidate| candidate.llm_api_type);
    run.downstream_request_uri = execution_preview.final_request_uri.clone();
}

fn replay_response_capture_limit() -> usize {
    CONFIG.replay_response_capture_max_bytes.max(1)
}

async fn read_replay_response_body_bounded<S, E, F>(
    stream: S,
    is_gzip: bool,
    capture_limit_bytes: usize,
    mut map_error: F,
) -> Result<ReplayResponseBodyCapture, ProxyError>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: Display,
    F: FnMut(E) -> ProxyError,
{
    let limit = capture_limit_bytes.max(1);
    let mut encoded = Vec::new();
    let mut encoded_truncated = false;
    futures::pin_mut!(stream);

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(&mut map_error)?;
        if chunk.is_empty() {
            continue;
        }
        let remaining = limit.saturating_sub(encoded.len());
        if chunk.len() > remaining {
            encoded.extend_from_slice(&chunk[..remaining]);
            encoded_truncated = true;
            break;
        }
        encoded.extend_from_slice(&chunk);
        if encoded.len() >= limit {
            while let Some(next_result) = stream.next().await {
                let next = next_result.map_err(&mut map_error)?;
                if !next.is_empty() {
                    encoded_truncated = true;
                    break;
                }
            }
            break;
        }
    }

    let decoded = if is_gzip {
        decode_gzip_replay_capture_bounded(&encoded, encoded_truncated, limit)
    } else {
        ReplayDecodedBody {
            body: Bytes::from(encoded),
            truncated: encoded_truncated,
            decode_failed: false,
        }
    };
    let state = if decoded.truncated || decoded.decode_failed {
        LogBodyCaptureState::Incomplete
    } else {
        LogBodyCaptureState::Complete
    };
    let original_size_known = state == LogBodyCaptureState::Complete;
    let original_size_bytes = original_size_known.then_some(decoded.body.len() as i64);
    let sha256 = sha256_hex(&decoded.body);

    Ok(ReplayResponseBodyCapture {
        body: decoded.body,
        state,
        original_size_bytes,
        original_size_known,
        truncated: state == LogBodyCaptureState::Incomplete,
        sha256,
        capture_limit_bytes: limit as i64,
        body_encoding: if is_gzip && !decoded.decode_failed {
            "decoded:gzip".to_string()
        } else if is_gzip {
            "encoded:gzip-decode-failed".to_string()
        } else {
            "identity".to_string()
        },
    })
}

struct ReplayDecodedBody {
    body: Bytes,
    truncated: bool,
    decode_failed: bool,
}

fn decode_gzip_replay_capture_bounded(
    encoded: &[u8],
    encoded_truncated: bool,
    limit: usize,
) -> ReplayDecodedBody {
    if encoded.is_empty() {
        return ReplayDecodedBody {
            body: Bytes::new(),
            truncated: encoded_truncated,
            decode_failed: false,
        };
    }

    let mut decoder = GzDecoder::new(encoded);
    let mut output = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match decoder.read(&mut buf) {
            Ok(0) => {
                return ReplayDecodedBody {
                    body: Bytes::from(output),
                    truncated: encoded_truncated,
                    decode_failed: false,
                };
            }
            Ok(read) => {
                let remaining = limit.saturating_sub(output.len());
                if read > remaining {
                    output.extend_from_slice(&buf[..remaining]);
                    return ReplayDecodedBody {
                        body: Bytes::from(output),
                        truncated: true,
                        decode_failed: false,
                    };
                }
                output.extend_from_slice(&buf[..read]);
            }
            Err(_) => {
                let fallback_len = encoded.len().min(limit);
                return ReplayDecodedBody {
                    body: Bytes::copy_from_slice(&encoded[..fallback_len]),
                    truncated: encoded_truncated || encoded.len() > limit,
                    decode_failed: true,
                };
            }
        }
    }
}

fn replay_body_capture_metadata(
    capture: &ReplayResponseBodyCapture,
) -> RequestReplayBodyCaptureMetadata {
    RequestReplayBodyCaptureMetadata {
        state: log_capture_state_to_string(&capture.state),
        bytes_captured: capture.body.len() as i64,
        original_size_bytes: capture.original_size_bytes,
        original_size_known: capture.original_size_known,
        truncated: capture.truncated,
        sha256: capture.sha256.clone(),
        capture_limit_bytes: capture.capture_limit_bytes,
        body_encoding: capture.body_encoding.clone(),
    }
}

fn replay_body_capture_metadata_from_bytes(
    body: &Bytes,
    capture_state: Option<&str>,
) -> RequestReplayBodyCaptureMetadata {
    let state = capture_state
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(REPLAY_BODY_CAPTURE_COMPLETE)
        .to_string();
    let truncated = state == REPLAY_BODY_CAPTURE_INCOMPLETE;
    RequestReplayBodyCaptureMetadata {
        state,
        bytes_captured: body.len() as i64,
        original_size_bytes: (!truncated).then_some(body.len() as i64),
        original_size_known: !truncated,
        truncated,
        sha256: sha256_hex(body),
        capture_limit_bytes: replay_response_capture_limit() as i64,
        body_encoding: "unknown".to_string(),
    }
}

async fn perform_attempt_replay_execution(
    app_state: &Arc<AppState>,
    source: &AttemptReplaySource,
    credential: &ReplayResolvedCredential,
) -> AttemptReplayExecutionOutcome {
    let headers = match build_replay_request_headers(
        &source.request_headers,
        &source.provider,
        source.llm_api_type,
        &credential.request_key,
    ) {
        Ok(headers) => headers,
        Err(err) => {
            return AttemptReplayExecutionOutcome {
                status: RequestReplayStatus::Error,
                http_status: None,
                first_byte_at: None,
                error_code: Some("invalid_replay_preview_headers".to_string()),
                error_message: Some(match err {
                    BaseError::ParamInvalid(message) => {
                        message.unwrap_or("invalid replay preview headers".to_string())
                    }
                    other => format!("{other:?}"),
                }),
                response_body_capture_state: Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED.to_string()),
                response_body_capture: None,
                response_headers: Vec::new(),
                response_body: None,
                response_body_bytes: None,
                usage_normalization: None,
                transform_diagnostics: Vec::new(),
                estimated_cost_nanos: None,
                estimated_cost_currency: None,
                total_input_tokens: None,
                total_output_tokens: None,
                reasoning_tokens: None,
                total_tokens: None,
            };
        }
    };
    let client = if source.provider.use_proxy {
        &app_state.proxy_client
    } else {
        &app_state.client
    };

    let cancellation = ProxyCancellationContext::new();
    let response = match send_with_first_byte_timeout(
        &cancellation,
        client
            .request(Method::POST, &source.request_uri)
            .headers(headers)
            .body(source.llm_request_body.bytes.clone()),
        "Attempt replay upstream request",
    )
    .await
    {
        Ok(response) => response,
        Err(proxy_error) => {
            return execution_outcome_from_proxy_error(proxy_error);
        }
    };

    let status_code = response.status();
    let response_headers =
        serialize_headers_for_output(response.headers(), STRIPPED_RESPONSE_HEADER_NAMES);
    let is_gzip = response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("gzip"));
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let is_sse = content_type
        .as_deref()
        .is_some_and(|value| value.contains("text/event-stream"));
    let first_byte_at = Some(Utc::now().timestamp_millis());

    let capture = match read_replay_response_body_bounded(
        response.bytes_stream(),
        is_gzip,
        replay_response_capture_limit(),
        |err| classify_reqwest_error("Reading attempt replay response body", &err),
    )
    .await
    {
        Ok(capture) => capture,
        Err(proxy_error) => return execution_outcome_from_proxy_error(proxy_error),
    };
    let decompressed_body = capture.body.clone();
    let response_body_capture_state = Some(log_capture_state_to_string(&capture.state));
    let response_body_capture = Some(replay_body_capture_metadata(&capture));
    let response_body = Some(body_from_bytes(
        &decompressed_body,
        content_type.clone(),
        response_body_capture_state.clone(),
    ));

    let (usage_normalization, transform_diagnostics) = if is_sse {
        parse_stream_usage_and_diagnostics(&decompressed_body, source.llm_api_type)
    } else if status_code.is_success() {
        let (_, _, usage_normalization, diagnostics) = process_success_response_body(
            &decompressed_body,
            source.llm_api_type,
            source.llm_api_type,
        );
        (usage_normalization, diagnostics)
    } else {
        (None, Vec::new())
    };

    let (estimated_cost_nanos, estimated_cost_currency) = usage_normalization
        .as_ref()
        .and_then(|normalization| {
            source
                .cost_catalog_version
                .as_ref()
                .map(|version| rate_replay_cost(normalization, version))
        })
        .unwrap_or((None, None));
    let (total_input_tokens, total_output_tokens, reasoning_tokens, total_tokens) =
        usage_normalization
            .as_ref()
            .map(usage_totals_for_run)
            .unwrap_or((None, None, None, None));

    if status_code.is_success() {
        AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Success,
            http_status: Some(i32::from(status_code.as_u16())),
            first_byte_at,
            error_code: None,
            error_message: None,
            response_headers,
            response_body,
            response_body_bytes: Some(decompressed_body),
            response_body_capture_state,
            response_body_capture,
            usage_normalization,
            transform_diagnostics,
            estimated_cost_nanos,
            estimated_cost_currency,
            total_input_tokens,
            total_output_tokens,
            reasoning_tokens,
            total_tokens,
        }
    } else {
        let proxy_error = classify_upstream_status(status_code, &decompressed_body);
        AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Error,
            http_status: Some(i32::from(status_code.as_u16())),
            first_byte_at,
            error_code: Some(proxy_error.error_code().to_string()),
            error_message: Some(proxy_error.message().to_string()),
            response_headers,
            response_body,
            response_body_bytes: Some(decompressed_body),
            response_body_capture_state,
            response_body_capture,
            usage_normalization,
            transform_diagnostics,
            estimated_cost_nanos,
            estimated_cost_currency,
            total_input_tokens,
            total_output_tokens,
            reasoning_tokens,
            total_tokens,
        }
    }
}

fn gateway_replay_status_from_attempt(attempt: &GatewayReplayFinalAttempt) -> RequestReplayStatus {
    match attempt.attempt_status {
        RequestAttemptStatus::Success => RequestReplayStatus::Success,
        RequestAttemptStatus::Cancelled => RequestReplayStatus::Cancelled,
        RequestAttemptStatus::Error | RequestAttemptStatus::Skipped => RequestReplayStatus::Error,
    }
}

fn gateway_candidate_decision_from_execution(
    decision: &GatewayReplayCandidateDecision,
) -> RequestReplayCandidateDecision {
    RequestReplayCandidateDecision {
        candidate_position: decision.candidate_position,
        provider_id: decision.provider_id,
        provider_api_key_id: decision.provider_api_key_id,
        model_id: decision.model_id,
        llm_api_type: decision.llm_api_type,
        attempt_status: decision.attempt_status,
        scheduler_action: decision.scheduler_action,
        error_code: decision.error_code.clone(),
        error_message: decision.error_message.clone(),
        request_uri: decision.request_uri.clone(),
    }
}

fn gateway_candidate_decisions_from_execution(
    decisions: &[GatewayReplayCandidateDecision],
) -> Vec<RequestReplayCandidateDecision> {
    decisions
        .iter()
        .map(gateway_candidate_decision_from_execution)
        .collect()
}

fn name_values_from_json_map(raw: Option<&str>, label: &str) -> Vec<RequestReplayNameValue> {
    raw.and_then(|value| parse_name_values_json_map(value, label).ok())
        .unwrap_or_default()
}

fn request_headers_from_final_attempt(
    attempt: &GatewayReplayFinalAttempt,
) -> Vec<RequestReplayNameValue> {
    name_values_from_json_map(
        attempt.request_headers_json.as_deref(),
        "gateway request headers",
    )
    .into_iter()
    .filter(|item| {
        let normalized_name = item.name.to_ascii_lowercase();
        !STRIPPED_PREVIEW_REQUEST_HEADER_NAMES.contains(&normalized_name.as_str())
    })
    .collect()
}

fn response_headers_from_final_attempt(
    attempt: &GatewayReplayFinalAttempt,
) -> Vec<RequestReplayNameValue> {
    name_values_from_json_map(
        attempt.response_headers_json.as_deref(),
        "gateway response headers",
    )
    .into_iter()
    .filter(|item| {
        let normalized_name = item.name.to_ascii_lowercase();
        !STRIPPED_RESPONSE_HEADER_NAMES.contains(&normalized_name.as_str())
    })
    .collect()
}

fn execution_preview_from_gateway_metadata(
    metadata: &GatewayReplayExecutionMetadata,
) -> RequestReplayExecutionPreview {
    let final_attempt = &metadata.final_attempt;
    RequestReplayExecutionPreview {
        semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
        resolved_route: Some(RequestReplayResolvedRoute {
            route_id: metadata.resolved_route_id,
            route_name: metadata.resolved_route_name.clone(),
        }),
        resolved_candidate: Some(RequestReplayResolvedCandidate {
            candidate_position: Some(final_attempt.candidate_position),
            provider_id: final_attempt.provider_id,
            provider_api_key_id: final_attempt.provider_api_key_id,
            model_id: final_attempt.model_id,
            llm_api_type: final_attempt.llm_api_type,
        }),
        candidate_decisions: gateway_candidate_decisions_from_execution(
            &metadata.candidate_decisions,
        ),
        applied_request_patch_summary: final_attempt.applied_request_patch_summary.clone(),
        final_request_uri: final_attempt.request_uri.clone(),
        final_request_headers: request_headers_from_final_attempt(final_attempt),
        final_request_body: final_attempt.request_body.as_ref().map(|body| {
            body_from_bytes(
                body,
                Some("application/json".to_string()),
                final_attempt
                    .request_body_capture_state
                    .clone()
                    .or_else(|| Some("complete".to_string())),
            )
        }),
    }
}

fn execution_preview_from_gateway_failure(
    failure: &GatewayReplayExecutionFailure,
) -> RequestReplayExecutionPreview {
    failure
        .metadata
        .as_ref()
        .map(execution_preview_from_gateway_metadata)
        .unwrap_or_else(|| RequestReplayExecutionPreview {
            semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
            resolved_route: None,
            resolved_candidate: None,
            candidate_decisions: gateway_candidate_decisions_from_execution(
                &failure.candidate_decisions,
            ),
            applied_request_patch_summary: None,
            final_request_uri: None,
            final_request_headers: Vec::new(),
            final_request_body: None,
        })
}

fn gateway_live_outcome_from_failure(
    failure: GatewayReplayExecutionFailure,
) -> GatewayReplayLiveOutcome {
    let execution_preview = execution_preview_from_gateway_failure(&failure);
    let attempt_timeline = execution_preview.candidate_decisions.clone();
    let mut outcome = execution_outcome_from_proxy_error(failure.error);

    if let Some(metadata) = failure.metadata {
        let final_attempt = metadata.final_attempt;
        let final_response_body_capture_state = final_attempt
            .response_body_capture_state
            .clone()
            .or(outcome.response_body_capture_state.clone());
        outcome.status = gateway_replay_status_from_attempt(&final_attempt);
        outcome.http_status = final_attempt.http_status;
        outcome.first_byte_at = final_attempt.first_byte_at;
        outcome.error_code = final_attempt.error_code.clone().or(outcome.error_code);
        outcome.error_message = final_attempt
            .error_message
            .clone()
            .or(outcome.error_message);
        outcome.response_headers = response_headers_from_final_attempt(&final_attempt);
        outcome.response_body = final_attempt.response_body.as_ref().map(|body| {
            body_from_bytes(
                body,
                None,
                final_response_body_capture_state
                    .clone()
                    .or_else(|| Some(REPLAY_BODY_CAPTURE_COMPLETE.to_string())),
            )
        });
        outcome.response_body_bytes = final_attempt.response_body.clone();
        outcome.response_body_capture_state = final_response_body_capture_state;
        outcome.response_body_capture = final_attempt.response_body.as_ref().map(|body| {
            replay_body_capture_metadata_from_bytes(
                body,
                outcome.response_body_capture_state.as_deref(),
            )
        });
        outcome.usage_normalization = metadata.usage_normalization;
        outcome.transform_diagnostics = metadata.transform_diagnostics;
        outcome.total_input_tokens = final_attempt.total_input_tokens;
        outcome.total_output_tokens = final_attempt.total_output_tokens;
        outcome.reasoning_tokens = final_attempt.reasoning_tokens;
        outcome.total_tokens = final_attempt.total_tokens;
    }

    GatewayReplayLiveOutcome {
        execution_preview,
        attempt_timeline,
        outcome,
    }
}

async fn perform_gateway_replay_execution(
    app_state: &Arc<AppState>,
    source: &GatewayReplaySource,
) -> GatewayReplayLiveOutcome {
    let execution = match execute_gateway_replay_request(
        Arc::clone(app_state),
        gateway_replay_input_from_source(source),
    )
    .await
    {
        Ok(execution) => execution,
        Err(failure) => return gateway_live_outcome_from_failure(failure),
    };

    let first_byte_at = Some(Utc::now().timestamp_millis());
    let execution_preview = execution_preview_from_gateway_metadata(&execution.metadata);
    let attempt_timeline = execution_preview.candidate_decisions.clone();
    let status_code = execution.response.status();
    let response_headers =
        serialize_headers_for_output(execution.response.headers(), STRIPPED_RESPONSE_HEADER_NAMES);
    let content_type = execution
        .response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let is_sse = content_type
        .as_deref()
        .is_some_and(|value| value.contains("text/event-stream"));
    let is_gzip = execution
        .response
        .headers()
        .get(CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("gzip"));
    let capture = match read_replay_response_body_bounded(
        execution.response.into_body().into_data_stream(),
        is_gzip,
        replay_response_capture_limit(),
        |err| {
            ProxyError::BadGateway(format!(
                "Reading gateway replay response body failed: {}",
                err
            ))
        },
    )
    .await
    {
        Ok(capture) => capture,
        Err(proxy_error) => {
            let mut outcome = execution_outcome_from_proxy_error(proxy_error);
            outcome.http_status = execution.metadata.final_attempt.http_status;
            outcome.first_byte_at = execution
                .metadata
                .final_attempt
                .first_byte_at
                .or(first_byte_at);
            outcome.response_headers = response_headers;
            outcome.transform_diagnostics = execution.metadata.transform_diagnostics.clone();
            return GatewayReplayLiveOutcome {
                execution_preview,
                attempt_timeline,
                outcome,
            };
        }
    };
    let body_bytes = capture.body.clone();
    let response_body_capture_state = Some(log_capture_state_to_string(&capture.state));
    let response_body_capture = Some(replay_body_capture_metadata(&capture));

    let response_body = Some(body_from_bytes(
        &body_bytes,
        content_type.clone(),
        response_body_capture_state.clone(),
    ));
    let (parsed_usage_normalization, parsed_transform_diagnostics) = if is_sse {
        parse_stream_usage_and_diagnostics(&body_bytes, source.request_log.user_api_type)
    } else if status_code.is_success() {
        let (_, _, usage_normalization, diagnostics) = process_success_response_body(
            &body_bytes,
            source.request_log.user_api_type,
            source.request_log.user_api_type,
        );
        (usage_normalization, diagnostics)
    } else {
        (None, Vec::new())
    };
    let usage_normalization = execution
        .metadata
        .usage_normalization
        .clone()
        .or(parsed_usage_normalization);
    let mut transform_diagnostics = execution.metadata.transform_diagnostics.clone();
    if is_sse || transform_diagnostics.is_empty() {
        transform_diagnostics.extend(parsed_transform_diagnostics);
    }

    let cost_catalog_version = match execution.metadata.final_attempt.model_id {
        Some(model_id) => app_state
            .get_cost_catalog_version_by_model(model_id, Utc::now().timestamp_millis())
            .await
            .ok()
            .flatten()
            .map(|version| (*version).clone()),
        None => None,
    };
    let (estimated_cost_nanos, estimated_cost_currency) = usage_normalization
        .as_ref()
        .and_then(|normalization| {
            cost_catalog_version
                .as_ref()
                .map(|version| rate_replay_cost(normalization, version))
        })
        .unwrap_or((None, None));
    let (total_input_tokens, total_output_tokens, reasoning_tokens, total_tokens) =
        usage_normalization
            .as_ref()
            .map(usage_totals_for_run)
            .unwrap_or((None, None, None, None));

    if status_code.is_success() {
        let outcome = AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Success,
            http_status: Some(i32::from(status_code.as_u16())),
            first_byte_at,
            error_code: None,
            error_message: None,
            response_headers,
            response_body,
            response_body_bytes: Some(body_bytes),
            response_body_capture_state,
            response_body_capture,
            usage_normalization,
            transform_diagnostics,
            estimated_cost_nanos,
            estimated_cost_currency,
            total_input_tokens,
            total_output_tokens,
            reasoning_tokens,
            total_tokens,
        };
        GatewayReplayLiveOutcome {
            execution_preview,
            attempt_timeline,
            outcome,
        }
    } else {
        let proxy_error = classify_upstream_status(status_code, &body_bytes);
        let outcome = AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Error,
            http_status: Some(i32::from(status_code.as_u16())),
            first_byte_at,
            error_code: Some(proxy_error.error_code().to_string()),
            error_message: Some(proxy_error.message().to_string()),
            response_headers,
            response_body,
            response_body_bytes: Some(body_bytes),
            response_body_capture_state,
            response_body_capture,
            usage_normalization,
            transform_diagnostics,
            estimated_cost_nanos,
            estimated_cost_currency,
            total_input_tokens,
            total_output_tokens,
            reasoning_tokens,
            total_tokens,
        };
        GatewayReplayLiveOutcome {
            execution_preview,
            attempt_timeline,
            outcome,
        }
    }
}

async fn resolve_replay_provider_credentials(
    app_state: &Arc<AppState>,
    provider: &CacheProvider,
    historical_provider_api_key_id: Option<i64>,
    provider_api_key_id_override: Option<i64>,
) -> Result<ReplayResolvedCredential, BaseError> {
    let (selected_key, used_override) = if let Some(key_id) = provider_api_key_id_override {
        (
            load_provider_api_key_for_replay(provider.id, key_id, true)?,
            true,
        )
    } else if let Some(key_id) = historical_provider_api_key_id {
        match load_provider_api_key_for_replay(provider.id, key_id, false) {
            Ok(provider_api_key) => (provider_api_key, false),
            Err(_) => (load_default_provider_api_key(provider.id)?, false),
        }
    } else {
        (load_default_provider_api_key(provider.id)?, false)
    };

    let request_key = match provider.provider_type {
        ProviderType::Vertex | ProviderType::VertexOpenai => get_vertex_token(
            &app_state.proxy_client,
            selected_key.id,
            &selected_key.api_key,
        )
        .await
        .map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Failed to resolve Vertex credential for provider '{}' and key {}: {}",
                provider.name, selected_key.id, err
            )))
        })?,
        _ => selected_key.api_key.clone(),
    };

    Ok(ReplayResolvedCredential {
        provider_api_key_id: selected_key.id,
        request_key,
        used_override,
    })
}

fn load_provider_api_key_for_replay(
    provider_id: i64,
    key_id: i64,
    is_override: bool,
) -> Result<ProviderApiKey, BaseError> {
    let provider_api_key = ProviderApiKey::get_by_id(key_id)?;
    if provider_api_key.provider_id != provider_id {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Provider API key {} does not belong to provider {}",
            key_id, provider_id
        ))));
    }
    if !provider_api_key.is_enabled {
        let label = if is_override {
            "override"
        } else {
            "historical"
        };
        return Err(BaseError::ParamInvalid(Some(format!(
            "Replay {} provider API key {} is disabled",
            label, key_id
        ))));
    }
    Ok(provider_api_key)
}

fn load_default_provider_api_key(provider_id: i64) -> Result<ProviderApiKey, BaseError> {
    ProviderApiKey::list_by_provider_id(provider_id)?
        .into_iter()
        .find(|key| key.is_enabled)
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "No enabled provider API key is available for provider {}",
                provider_id
            )))
        })
}

fn build_replay_request_headers(
    historical_headers: &HeaderMap,
    provider: &CacheProvider,
    target_api_type: LlmApiType,
    request_key: &str,
) -> Result<HeaderMap, BaseError> {
    let mut headers = HeaderMap::new();
    for (name, value) in historical_headers.iter() {
        let normalized_name = name.as_str().to_ascii_lowercase();
        if DISALLOWED_REPLAY_REQUEST_HEADER_NAMES.contains(&normalized_name.as_str()) {
            continue;
        }
        headers.insert(name.clone(), value.clone());
    }

    apply_provider_request_auth_header(&mut headers, provider, target_api_type, request_key)
        .map_err(proxy_error_to_param_error)?;

    Ok(headers)
}

fn build_header_map_from_name_values(
    headers: &[RequestReplayNameValue],
) -> Result<HeaderMap, BaseError> {
    let mut header_map = HeaderMap::new();
    for item in headers {
        let Some(value) = item.value.as_deref() else {
            continue;
        };
        let name = HeaderName::try_from(item.name.as_str()).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Invalid replay header name '{}': {}",
                item.name, err
            )))
        })?;
        let value = HeaderValue::try_from(value).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Invalid replay header value for '{}': {}",
                item.name, err
            )))
        })?;
        header_map.insert(name, value);
    }
    Ok(header_map)
}

fn parse_name_values_json_map(
    raw: &str,
    label: &str,
) -> Result<Vec<RequestReplayNameValue>, BaseError> {
    let map = serde_json::from_str::<BTreeMap<String, String>>(raw).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Failed to parse replay {} JSON map: {}",
            label, err
        )))
    })?;
    Ok(map
        .into_iter()
        .map(|(name, value)| RequestReplayNameValue {
            name,
            value: Some(value),
        })
        .collect())
}

fn serialize_headers_for_output(
    headers: &HeaderMap,
    stripped_names: &[&str],
) -> Vec<RequestReplayNameValue> {
    let mut items = headers
        .iter()
        .filter_map(|(name, value)| {
            let normalized_name = name.as_str().to_ascii_lowercase();
            if stripped_names.contains(&normalized_name.as_str()) {
                return None;
            }

            Some(RequestReplayNameValue {
                name: normalized_name.clone(),
                value: if REDACTED_HEADER_NAMES.contains(&normalized_name.as_str()) {
                    None
                } else {
                    Some(value.to_str().unwrap_or("").to_string())
                },
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.value.cmp(&right.value))
    });
    items
}

fn canonical_name_values(
    items: &[RequestReplayNameValue],
    lowercase_names: bool,
) -> Vec<RequestReplayNameValue> {
    let mut values = items
        .iter()
        .map(|item| RequestReplayNameValue {
            name: if lowercase_names {
                item.name.to_ascii_lowercase()
            } else {
                item.name.clone()
            },
            value: item.value.clone(),
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.value.cmp(&right.value))
    });
    values
}

fn replay_query_params_from_snapshot(
    params: &[crate::utils::storage::RequestLogBundleQueryParam],
) -> Vec<RequestReplayQueryParam> {
    params
        .iter()
        .map(|item| RequestReplayQueryParam {
            name: item.name.clone(),
            value: item.value_for_replay(),
            value_present: item.has_value(),
        })
        .collect()
}

fn body_digest_from_decoded_body(body: &DecodedBundleBody) -> RequestReplayFingerprintBodyDigest {
    RequestReplayFingerprintBodyDigest {
        sha256: sha256_hex(&body.bytes),
        media_type: body.media_type.clone(),
        capture_state: body.capture_state.clone(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn canonical_uri_for_fingerprint(uri: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(uri) else {
        return uri.to_string();
    };
    let mut pairs = parsed
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return parsed.to_string();
    }

    pairs.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    parsed.set_query(None);
    {
        let mut query = parsed.query_pairs_mut();
        for (name, value) in pairs {
            query.append_pair(&name, &value);
        }
    }
    parsed.to_string()
}

fn provider_snapshot_from_attempt(attempt: &RequestAttemptDetail) -> RequestReplayProviderSnapshot {
    RequestReplayProviderSnapshot {
        provider_id: attempt.provider_id,
        provider_api_key_id: attempt.provider_api_key_id,
        provider_key: attempt.provider_key_snapshot.clone(),
        provider_name: attempt.provider_name_snapshot.clone(),
    }
}

fn model_snapshot_from_attempt(
    attempt: &RequestAttemptDetail,
    llm_api_type: LlmApiType,
) -> RequestReplayModelSnapshot {
    RequestReplayModelSnapshot {
        model_id: attempt.model_id,
        model_name: attempt.model_name_snapshot.clone(),
        real_model_name: attempt.real_model_name_snapshot.clone(),
        llm_api_type: Some(attempt.llm_api_type.unwrap_or(llm_api_type)),
    }
}

fn load_cache_api_key_by_id(api_key_id: i64) -> Result<CacheApiKey, BaseError> {
    let row = ApiKey::get_by_id(api_key_id)?;
    let acl_rules = ApiKey::load_acl_rules(row.id)?;
    let cache_key = CacheApiKey::from_db(row, acl_rules);
    if !cache_key.is_active_at(Utc::now().timestamp_millis()) {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Source api key {} is not active under current configuration",
            api_key_id
        ))));
    }
    Ok(cache_key)
}

fn header_map_from_snapshot(
    snapshot: &RequestLogBundleRequestSnapshot,
) -> Result<HeaderMap, BaseError> {
    let mut headers = HeaderMap::new();
    for item in &snapshot.sanitized_original_headers {
        let name = HeaderName::try_from(item.name.as_str()).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Invalid gateway replay snapshot header name '{}': {}",
                item.name, err
            )))
        })?;
        let value = HeaderValue::try_from(item.value.as_str()).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Invalid gateway replay snapshot header value for '{}': {}",
                item.name, err
            )))
        })?;
        headers.insert(name, value);
    }
    Ok(headers)
}

fn extract_gateway_user_request_body(
    bundle: &crate::utils::storage::RequestLogBundleV2,
) -> Result<DecodedBundleBody, BaseError> {
    let blob_id = bundle.request_section.user_request_blob_id.ok_or_else(|| {
        BaseError::ParamInvalid(Some(
            "Gateway replay requires a captured user request body".to_string(),
        ))
    })?;
    let blob = bundle
        .blob_pool
        .iter()
        .find(|blob| blob.blob_id == blob_id)
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "Gateway replay user request blob {} is missing",
                blob_id
            )))
        })?;
    Ok(DecodedBundleBody {
        bytes: blob.body.clone(),
        media_type: Some(blob.media_type.clone()),
        capture_state: Some("complete".to_string()),
    })
}

fn extract_gateway_user_response_body(
    bundle: &crate::utils::storage::RequestLogBundleV2,
) -> Option<DecodedBundleBody> {
    let blob_id = bundle.request_section.user_response_blob_id?;
    bundle
        .blob_pool
        .iter()
        .find(|blob| blob.blob_id == blob_id)
        .map(|blob| DecodedBundleBody {
            bytes: blob.body.clone(),
            media_type: Some(blob.media_type.clone()),
            capture_state: bundle
                .request_section
                .user_response_capture_state
                .as_ref()
                .map(log_capture_state_to_string),
        })
}

fn gateway_replay_kind_from_snapshot(
    request_log: &RequestLogRecord,
    snapshot: &RequestLogBundleRequestSnapshot,
    request_value: &Value,
) -> Result<(String, GatewayReplayAttemptKind), BaseError> {
    let operation_kind = snapshot.operation_kind.as_str();
    if request_log.user_api_type == LlmApiType::Openai
        && matches!(operation_kind, "embeddings" | "rerank")
    {
        let requested_model = require_model_from_request_value(request_value)?;
        let operation = UtilityOperation {
            name: operation_kind.to_string(),
            api_type: LlmApiType::Openai,
            protocol: UtilityProtocol::OpenaiCompatible,
            downstream_path: operation_kind.to_string(),
        };
        return Ok((
            requested_model,
            GatewayReplayAttemptKind::Utility {
                operation,
                data: request_value.clone(),
            },
        ));
    }

    if request_log.user_api_type == LlmApiType::Gemini {
        let (model_name, action) = parse_gemini_model_action_from_path(&snapshot.request_path)?;
        if matches!(
            action.as_str(),
            "countMessageTokens" | "countTextTokens" | "countTokens"
        ) {
            let operation = UtilityOperation {
                name: action.clone(),
                api_type: LlmApiType::Gemini,
                protocol: UtilityProtocol::GeminiCompatible,
                downstream_path: action,
            };
            return Ok((
                model_name,
                GatewayReplayAttemptKind::Utility {
                    operation,
                    data: request_value.clone(),
                },
            ));
        }

        let is_stream = action == "streamGenerateContent";
        return Ok((
            model_name,
            GatewayReplayAttemptKind::Generation {
                api_type: LlmApiType::Gemini,
                is_stream,
                data: request_value.clone(),
                original_request_value: request_value.clone(),
            },
        ));
    }

    let requested_model = require_model_from_request_value(request_value)?;
    let is_stream = request_value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok((
        requested_model,
        GatewayReplayAttemptKind::Generation {
            api_type: request_log.user_api_type,
            is_stream,
            data: request_value.clone(),
            original_request_value: request_value.clone(),
        },
    ))
}

fn require_model_from_request_value(value: &Value) -> Result<String, BaseError> {
    value
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(
                "Gateway replay request body is missing string field 'model'".to_string(),
            ))
        })
}

fn parse_gemini_model_action_from_path(path: &str) -> Result<(String, String), BaseError> {
    let action_segment = path
        .find("/models/")
        .map(|index| &path[index + "/models/".len()..])
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path));
    let (model_name, action) = action_segment.rsplit_once(':').ok_or_else(|| {
        BaseError::ParamInvalid(Some(format!(
            "Gateway replay Gemini request path '{}' does not contain a model action",
            path
        )))
    })?;
    Ok((model_name.to_string(), action.to_string()))
}

fn extract_attempt_request_body(
    bundle: &DecodedRequestLogBundle,
    attempt: &RequestAttemptDetail,
) -> Result<DecodedBundleBody, BaseError> {
    match bundle {
        DecodedRequestLogBundle::Legacy(bundle) => {
            if attempt.attempt_index != 1 {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "Legacy request bundle cannot replay attempt {} because only the first attempt body was captured",
                    attempt.id
                ))));
            }
            let bytes = bundle.llm_request_body.clone().ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "Attempt {} is missing historical downstream request body",
                    attempt.id
                )))
            })?;
            Ok(DecodedBundleBody {
                bytes,
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            })
        }
        DecodedRequestLogBundle::V2(bundle) => {
            let section = bundle_attempt_section(bundle, attempt).ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "Attempt {} is missing bundle section",
                    attempt.id
                )))
            })?;
            reconstruct_request_body_from_v2_bundle(
                bundle,
                section.llm_request_blob_id,
                section.llm_request_patch_id,
            )
            .ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "Attempt {} is missing historical downstream request body",
                    attempt.id
                )))
            })
        }
    }
}

fn extract_attempt_response_body(
    bundle: &DecodedRequestLogBundle,
    attempt: &RequestAttemptDetail,
) -> Result<Option<DecodedBundleBody>, BaseError> {
    match bundle {
        DecodedRequestLogBundle::Legacy(bundle) => {
            Ok(bundle
                .llm_response_body
                .clone()
                .map(|bytes| DecodedBundleBody {
                    bytes,
                    media_type: Some("application/json".to_string()),
                    capture_state: bundle
                        .llm_response_capture_state
                        .as_ref()
                        .map(log_capture_state_to_string),
                }))
        }
        DecodedRequestLogBundle::V2(bundle) => {
            let Some(section) = bundle_attempt_section(bundle, attempt) else {
                return Ok(None);
            };
            Ok(section
                .llm_response_blob_id
                .and_then(|blob_id| bundle.blob_pool.iter().find(|blob| blob.blob_id == blob_id))
                .map(|blob| DecodedBundleBody {
                    bytes: blob.body.clone(),
                    media_type: Some(blob.media_type.clone()),
                    capture_state: section
                        .llm_response_capture_state
                        .as_ref()
                        .map(log_capture_state_to_string),
                }))
        }
    }
}

fn bundle_attempt_section<'a>(
    bundle: &'a crate::utils::storage::RequestLogBundleV2,
    attempt: &RequestAttemptDetail,
) -> Option<&'a crate::utils::storage::RequestLogBundleAttemptSection> {
    bundle.attempt_sections.iter().find(|section| {
        section
            .attempt_id
            .is_some_and(|attempt_id| attempt_id == attempt.id)
            || section.attempt_index == attempt.attempt_index
    })
}

fn reconstruct_request_body_from_v2_bundle(
    bundle: &crate::utils::storage::RequestLogBundleV2,
    blob_id: Option<i32>,
    patch_id: Option<i32>,
) -> Option<DecodedBundleBody> {
    let blob_id = blob_id?;
    let blob = bundle
        .blob_pool
        .iter()
        .find(|blob| blob.blob_id == blob_id)?;
    let bytes = if let Some(patch_id) = patch_id {
        let patch = bundle
            .patch_pool
            .iter()
            .find(|patch| patch.patch_id == patch_id)?;
        apply_json_patch_bytes(&blob.body, &patch.patch_body).ok()?
    } else {
        blob.body.clone()
    };

    Some(DecodedBundleBody {
        bytes,
        media_type: Some(blob.media_type.clone()),
        capture_state: Some("complete".to_string()),
    })
}

fn apply_json_patch_bytes(base: &[u8], patch: &[u8]) -> Result<Bytes, BaseError> {
    let mut value = serde_json::from_slice::<Value>(base).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Replay request patch base JSON decode failed: {}",
            err
        )))
    })?;
    let patch: json_patch::Patch = serde_json::from_slice(patch).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Replay request patch JSON decode failed: {}",
            err
        )))
    })?;
    json_patch::patch(&mut value, &patch).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Replay request patch application failed: {}",
            err
        )))
    })?;
    serde_json::to_vec(&value).map(Bytes::from).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Replay request patch serialization failed: {}",
            err
        )))
    })
}

fn infer_llm_api_type(user_api_type: LlmApiType, provider: &CacheProvider) -> LlmApiType {
    match provider.provider_type {
        ProviderType::Vertex | ProviderType::Gemini => LlmApiType::Gemini,
        ProviderType::Ollama => LlmApiType::Ollama,
        ProviderType::Anthropic => LlmApiType::Anthropic,
        ProviderType::Responses => LlmApiType::Responses,
        ProviderType::GeminiOpenai => LlmApiType::GeminiOpenai,
        ProviderType::Openai | ProviderType::VertexOpenai => user_api_type,
    }
}

fn body_to_replay_body(body: &DecodedBundleBody) -> RequestReplayBody {
    body_from_bytes(
        &body.bytes,
        body.media_type.clone(),
        body.capture_state.clone(),
    )
}

fn body_from_bytes(
    bytes: &Bytes,
    media_type: Option<String>,
    capture_state: Option<String>,
) -> RequestReplayBody {
    let json = serde_json::from_slice::<Value>(bytes).ok();
    let text = if json.is_none() {
        Some(String::from_utf8_lossy(bytes).to_string())
    } else {
        None
    };

    RequestReplayBody {
        media_type,
        json,
        text,
        capture_state,
    }
}

fn parse_stream_usage_and_diagnostics(
    bytes: &Bytes,
    api_type: LlmApiType,
) -> (Option<UsageNormalization>, Vec<UnifiedTransformDiagnostic>) {
    let mut parser = SseParser::new();
    let events = parser.process(bytes);
    if events.is_empty() {
        return (None, Vec::new());
    }

    let mut transformer = StreamTransformer::new(api_type, api_type);
    let _ = transformer.transform_events(events);
    (
        transformer.parse_usage_normalization(),
        transformer.diagnostics_snapshot(),
    )
}

fn execution_outcome_from_proxy_error(proxy_error: ProxyError) -> AttemptReplayExecutionOutcome {
    AttemptReplayExecutionOutcome {
        status: RequestReplayStatus::Error,
        http_status: None,
        first_byte_at: None,
        error_code: Some(proxy_error.error_code().to_string()),
        error_message: Some(proxy_error.message().to_string()),
        response_headers: Vec::new(),
        response_body: None,
        response_body_bytes: None,
        response_body_capture_state: Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED.to_string()),
        response_body_capture: None,
        usage_normalization: None,
        transform_diagnostics: Vec::new(),
        estimated_cost_nanos: None,
        estimated_cost_currency: None,
        total_input_tokens: None,
        total_output_tokens: None,
        reasoning_tokens: None,
        total_tokens: None,
    }
}

fn usage_totals_for_run(
    normalization: &UsageNormalization,
) -> (Option<i32>, Option<i32>, Option<i32>, Option<i32>) {
    let total_tokens = normalization.total_input_tokens + normalization.total_output_tokens;
    (
        i64_to_i32(normalization.total_input_tokens),
        i64_to_i32(normalization.total_output_tokens),
        i64_to_i32(normalization.reasoning_tokens),
        i64_to_i32(total_tokens),
    )
}

fn rate_replay_cost(
    normalization: &UsageNormalization,
    version: &CacheCostCatalogVersion,
) -> (Option<i64>, Option<String>) {
    let ledger = CostLedger::from(normalization);
    let rating = match rate_cost(
        &ledger,
        &CostRatingContext {
            total_input_tokens: normalization.total_input_tokens,
        },
        version,
    ) {
        Ok(rating) => rating,
        Err(err) => {
            debug!(
                "Attempt replay cost rating failed for version {}: {:?}",
                version.id, err
            );
            return (None, None);
        }
    };

    let snapshot = CostSnapshot {
        schema_version: COST_SNAPSHOT_SCHEMA_VERSION_V1,
        cost_catalog_id: version.catalog_id,
        cost_catalog_version_id: version.id,
        total_cost_nanos: rating.total_cost_nanos,
        currency: rating.currency.clone(),
        detail_lines: rating.detail_lines,
        unmatched_items: rating.unmatched_items,
        warnings: rating.warnings,
    };

    if serde_json::to_string(&snapshot).is_ok() {
        (Some(snapshot.total_cost_nanos), Some(snapshot.currency))
    } else {
        (None, None)
    }
}

fn build_attempt_replay_diff(
    source: &AttemptReplaySource,
    outcome: &AttemptReplayExecutionOutcome,
) -> RequestReplayArtifactDiff {
    let status_changed = if source.attempt.http_status.is_some() || outcome.http_status.is_some() {
        Some(source.attempt.http_status != outcome.http_status)
    } else {
        Some(!attempt_status_matches_replay_status(
            source.attempt.attempt_status,
            outcome.status,
        ))
    };

    let headers_changed =
        if !source.baseline_response_headers.is_empty() || !outcome.response_headers.is_empty() {
            Some(
                normalized_name_values(&source.baseline_response_headers)
                    != normalized_name_values(&outcome.response_headers),
            )
        } else {
            None
        };

    let body_comparison = compare_replay_body_capture(
        source
            .baseline_response_body
            .as_ref()
            .map(|body| (body.bytes.as_ref(), body.capture_state.as_deref())),
        outcome.response_body_bytes.as_ref().map(|body| {
            (
                body.as_ref(),
                outcome.response_body_capture_state.as_deref(),
            )
        }),
        "response body comparison was partial because one side lacked a captured body",
        "response body comparison was partial because one side had an incomplete capture",
    );
    let body_changed = body_comparison.changed;

    let token_delta = match (source.attempt.total_tokens, outcome.total_tokens) {
        (Some(left), Some(right)) => Some(right - left),
        _ => None,
    };
    let cost_delta = match (
        source.attempt.estimated_cost_nanos,
        outcome.estimated_cost_nanos,
    ) {
        (Some(left), Some(right)) => Some(right - left),
        _ => None,
    };

    let mut summary_lines = Vec::new();
    match (
        source.attempt.http_status,
        outcome.http_status,
        status_changed,
    ) {
        (Some(left), Some(right), Some(true)) => {
            summary_lines.push(format!("status changed: {} -> {}", left, right));
        }
        (Some(code), Some(_), Some(false)) => {
            summary_lines.push(format!("status unchanged: {}", code));
        }
        _ => summary_lines.push(
            "status comparison was partial because one side lacked an upstream HTTP status"
                .to_string(),
        ),
    }

    summary_lines.push(match headers_changed {
        Some(true) => "response headers changed".to_string(),
        Some(false) => "response headers unchanged".to_string(),
        None => "response header comparison was partial because one side lacked captured headers"
            .to_string(),
    });
    summary_lines.push(match body_changed {
        Some(true) if body_comparison.partial => {
            format!("response body changed; {}", body_comparison.reason)
        }
        Some(true) => "response body changed".to_string(),
        Some(false) => "response body unchanged".to_string(),
        None => body_comparison.reason,
    });
    summary_lines.push(match token_delta {
        Some(delta) => format!("total_tokens delta: {}", delta),
        None => "token comparison unavailable".to_string(),
    });
    summary_lines.push(match cost_delta {
        Some(delta) => format!("estimated_cost_nanos delta: {}", delta),
        None => "cost comparison unavailable".to_string(),
    });

    RequestReplayArtifactDiff {
        baseline_kind: RequestReplayDiffBaselineKind::OriginalAttempt,
        status_changed,
        headers_changed,
        body_changed,
        token_delta,
        cost_delta,
        summary_lines,
    }
}

fn build_gateway_replay_diff(
    source: &GatewayReplaySource,
    execution_preview: &RequestReplayExecutionPreview,
    outcome: &AttemptReplayExecutionOutcome,
) -> RequestReplayArtifactDiff {
    let status_comparison = build_gateway_status_comparison(source, outcome);

    let body_comparison = compare_replay_body_capture(
        source
            .baseline_user_response_body
            .as_ref()
            .map(|body| (body.bytes.as_ref(), body.capture_state.as_deref())),
        outcome.response_body_bytes.as_ref().map(|body| {
            (
                body.as_ref(),
                outcome.response_body_capture_state.as_deref(),
            )
        }),
        "user response body comparison was partial because one side lacked a captured body",
        "user response body comparison was partial because one side had an incomplete capture",
    );
    let body_changed = body_comparison.changed;
    let baseline_response_headers = gateway_baseline_response_headers(source);
    let headers_changed =
        if !baseline_response_headers.is_empty() || !outcome.response_headers.is_empty() {
            Some(
                normalized_name_values(&baseline_response_headers)
                    != normalized_name_values(&outcome.response_headers),
            )
        } else {
            None
        };
    let token_delta = match (source.request_log.total_tokens, outcome.total_tokens) {
        (Some(left), Some(right)) => Some(right - left),
        _ => None,
    };
    let cost_delta = match (
        source.request_log.estimated_cost_nanos,
        outcome.estimated_cost_nanos,
    ) {
        (Some(left), Some(right)) => Some(right - left),
        _ => None,
    };

    let mut summary_lines = Vec::new();
    let route_label = execution_preview
        .resolved_route
        .as_ref()
        .and_then(|route| route.route_name.clone())
        .unwrap_or_else(|| "unresolved route".to_string());
    let candidate_count = execution_preview.candidate_decisions.len();
    summary_lines.push(format!(
        "gateway replay executed via '{}' with {} observed candidate decision(s)",
        route_label, candidate_count
    ));
    summary_lines.push(match status_comparison.changed {
        true => format!(
            "gateway result changed: {} -> {}",
            status_comparison.baseline_label, status_comparison.replay_label
        ),
        false => format!(
            "gateway result unchanged: {}",
            status_comparison.replay_label
        ),
    });
    summary_lines.push(match headers_changed {
        Some(true) => "gateway response headers changed".to_string(),
        Some(false) => "gateway response headers unchanged".to_string(),
        None => {
            "gateway response header comparison unavailable because neither side had captured headers"
                .to_string()
        }
    });
    summary_lines.push(match body_changed {
        Some(true) if body_comparison.partial => {
            format!("user response body changed; {}", body_comparison.reason)
        }
        Some(true) => "user response body changed".to_string(),
        Some(false) => "user response body unchanged".to_string(),
        None => body_comparison.reason,
    });
    summary_lines.push(match token_delta {
        Some(delta) => format!("total_tokens delta: {}", delta),
        None => "token comparison unavailable".to_string(),
    });
    summary_lines.push(match cost_delta {
        Some(delta) => format!("estimated_cost_nanos delta: {}", delta),
        None => "cost comparison unavailable".to_string(),
    });

    RequestReplayArtifactDiff {
        baseline_kind: RequestReplayDiffBaselineKind::OriginalRequestResult,
        status_changed: Some(status_comparison.changed),
        headers_changed,
        body_changed,
        token_delta,
        cost_delta,
        summary_lines,
    }
}

struct GatewayReplayStatusComparison {
    changed: bool,
    baseline_label: String,
    replay_label: String,
}

fn build_gateway_status_comparison(
    source: &GatewayReplaySource,
    outcome: &AttemptReplayExecutionOutcome,
) -> GatewayReplayStatusComparison {
    let baseline_http_status = source
        .baseline_final_attempt
        .as_ref()
        .and_then(|attempt| attempt.http_status);
    let baseline_error_code = source
        .baseline_final_attempt
        .as_ref()
        .and_then(|attempt| attempt.error_code.as_deref())
        .or(source.request_log.final_error_code.as_deref());
    let replay_error_code = outcome.error_code.as_deref();
    let request_status_changed =
        !request_status_matches_replay_status(&source.request_log.overall_status, &outcome.status);
    let http_status_changed = (source.baseline_final_attempt.is_some()
        || baseline_http_status.is_some()
        || outcome.http_status.is_some())
        && baseline_http_status != outcome.http_status;
    let error_code_changed = (baseline_error_code.is_some() || replay_error_code.is_some())
        && baseline_error_code != replay_error_code;

    GatewayReplayStatusComparison {
        changed: request_status_changed || http_status_changed || error_code_changed,
        baseline_label: format_gateway_baseline_result_label(
            &source.request_log.overall_status,
            baseline_http_status,
            baseline_error_code,
        ),
        replay_label: format_gateway_replay_result_label(
            &outcome.status,
            outcome.http_status,
            replay_error_code,
        ),
    }
}

fn gateway_baseline_response_headers(source: &GatewayReplaySource) -> Vec<RequestReplayNameValue> {
    source
        .baseline_final_attempt
        .as_ref()
        .and_then(|attempt| attempt.response_headers_json.as_deref())
        .and_then(|raw| parse_name_values_json_map(raw, "gateway baseline response headers").ok())
        .unwrap_or_default()
}

fn format_gateway_baseline_result_label(
    status: &crate::schema::enum_def::RequestStatus,
    http_status: Option<i32>,
    error_code: Option<&str>,
) -> String {
    format_gateway_result_label(request_status_label(status), http_status, error_code)
}

fn format_gateway_replay_result_label(
    status: &RequestReplayStatus,
    http_status: Option<i32>,
    error_code: Option<&str>,
) -> String {
    format_gateway_result_label(request_replay_status_label(status), http_status, error_code)
}

fn format_gateway_result_label(
    status_label: &str,
    http_status: Option<i32>,
    error_code: Option<&str>,
) -> String {
    let mut parts = vec![status_label.to_string()];
    if let Some(http_status) = http_status {
        parts.push(format!("http={http_status}"));
    }
    if let Some(error_code) = error_code {
        parts.push(format!("error_code={error_code}"));
    }
    parts.join(" / ")
}

fn rejected_diff(
    message: &str,
    baseline_kind: RequestReplayDiffBaselineKind,
) -> RequestReplayArtifactDiff {
    RequestReplayArtifactDiff {
        baseline_kind,
        status_changed: None,
        headers_changed: None,
        body_changed: None,
        token_delta: None,
        cost_delta: None,
        summary_lines: vec![message.to_string()],
    }
}

fn dry_run_result(
    transform_diagnostics: Vec<UnifiedTransformDiagnostic>,
    attempt_timeline: Vec<RequestReplayCandidateDecision>,
) -> RequestReplayArtifactResult {
    RequestReplayArtifactResult {
        status: RequestReplayStatus::Success,
        http_status: None,
        response_headers: Vec::new(),
        response_body: None,
        response_body_capture_state: Some(REPLAY_BODY_CAPTURE_NOT_EXECUTED.to_string()),
        response_body_capture: None,
        usage_normalization: None,
        transform_diagnostics,
        attempt_timeline,
    }
}

fn dry_run_diff(
    message: &str,
    baseline_kind: RequestReplayDiffBaselineKind,
) -> RequestReplayArtifactDiff {
    RequestReplayArtifactDiff {
        baseline_kind,
        status_changed: None,
        headers_changed: None,
        body_changed: None,
        token_delta: None,
        cost_delta: None,
        summary_lines: vec![message.to_string()],
    }
}

fn set_replay_artifact_locator(run: &mut RequestReplayRun, locator: &RequestReplayArtifactStorage) {
    run.artifact_version = Some(locator.artifact_version);
    run.artifact_storage_type = Some(locator.artifact_storage_type.clone());
    run.artifact_storage_key = Some(locator.artifact_storage_key.clone());
}

async fn store_replay_artifact_for_run(
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
    let prior_status = run.status.clone();
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

fn request_status_label(status: &crate::schema::enum_def::RequestStatus) -> &'static str {
    match status {
        crate::schema::enum_def::RequestStatus::Pending => "pending",
        crate::schema::enum_def::RequestStatus::Success => "success",
        crate::schema::enum_def::RequestStatus::Error => "error",
        crate::schema::enum_def::RequestStatus::Cancelled => "cancelled",
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

fn normalized_name_values(items: &[RequestReplayNameValue]) -> BTreeMap<String, Option<String>> {
    items
        .iter()
        .map(|item| (item.name.to_ascii_lowercase(), item.value.clone()))
        .collect()
}

struct ReplayBodyComparison {
    changed: Option<bool>,
    reason: String,
    partial: bool,
}

fn compare_replay_body_capture(
    baseline: Option<(&[u8], Option<&str>)>,
    replay: Option<(&[u8], Option<&str>)>,
    missing_reason: &'static str,
    incomplete_reason: &'static str,
) -> ReplayBodyComparison {
    let (Some((baseline_body, baseline_state)), Some((replay_body, replay_state))) =
        (baseline, replay)
    else {
        return ReplayBodyComparison {
            changed: None,
            reason: missing_reason.to_string(),
            partial: true,
        };
    };

    let baseline_complete = replay_capture_state_is_complete(baseline_state);
    let replay_complete = replay_capture_state_is_complete(replay_state);
    if baseline_complete && replay_complete {
        return ReplayBodyComparison {
            changed: Some(!body_bytes_equal(baseline_body, replay_body)),
            reason: String::new(),
            partial: false,
        };
    }

    let comparable_len = baseline_body.len().min(replay_body.len());
    let changed = if baseline_body[..comparable_len] != replay_body[..comparable_len] {
        Some(true)
    } else {
        None
    };

    ReplayBodyComparison {
        changed,
        reason: incomplete_reason.to_string(),
        partial: true,
    }
}

fn replay_capture_state_is_complete(state: Option<&str>) -> bool {
    !matches!(
        state,
        Some(REPLAY_BODY_CAPTURE_INCOMPLETE)
            | Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED)
            | Some(REPLAY_BODY_CAPTURE_NOT_EXECUTED)
    )
}

fn body_bytes_equal(left: &[u8], right: &[u8]) -> bool {
    match (
        serde_json::from_slice::<Value>(left),
        serde_json::from_slice::<Value>(right),
    ) {
        (Ok(left_json), Ok(right_json)) => left_json == right_json,
        _ => left == right,
    }
}

fn request_status_matches_replay_status(
    request_status: &crate::schema::enum_def::RequestStatus,
    replay_status: &RequestReplayStatus,
) -> bool {
    matches!(
        (request_status, replay_status),
        (
            crate::schema::enum_def::RequestStatus::Pending,
            RequestReplayStatus::Pending,
        ) | (
            crate::schema::enum_def::RequestStatus::Pending,
            RequestReplayStatus::Running,
        ) | (
            crate::schema::enum_def::RequestStatus::Success,
            RequestReplayStatus::Success,
        ) | (
            crate::schema::enum_def::RequestStatus::Error,
            RequestReplayStatus::Error,
        ) | (
            crate::schema::enum_def::RequestStatus::Cancelled,
            RequestReplayStatus::Cancelled,
        )
    )
}

fn attempt_status_matches_replay_status(
    attempt_status: RequestAttemptStatus,
    replay_status: RequestReplayStatus,
) -> bool {
    matches!(
        (attempt_status, replay_status),
        (RequestAttemptStatus::Success, RequestReplayStatus::Success)
            | (RequestAttemptStatus::Error, RequestReplayStatus::Error)
            | (
                RequestAttemptStatus::Cancelled,
                RequestReplayStatus::Cancelled
            )
    )
}

fn require_non_empty(value: Option<&str>, message: String) -> Result<String, BaseError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| BaseError::ParamInvalid(Some(message)))
}

fn proxy_error_to_param_error(error: ProxyError) -> BaseError {
    BaseError::ParamInvalid(Some(error.to_string()))
}

fn log_capture_state_to_string(state: &LogBodyCaptureState) -> String {
    match state {
        LogBodyCaptureState::Complete => "complete",
        LogBodyCaptureState::Incomplete => "incomplete",
        LogBodyCaptureState::NotCaptured => "not_captured",
    }
    .to_string()
}

fn i64_to_i32(value: i64) -> Option<i32> {
    i32::try_from(value).ok()
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Router,
        body::Bytes as AxumBytes,
        http::{HeaderMap as AxumHeaderMap, StatusCode, header::CONTENT_TYPE},
        routing::post,
    };
    use reqwest::header::AUTHORIZATION;
    use tempfile::tempdir;

    use crate::{
        schema::enum_def::{Action, ProviderApiKeyMode, ProviderType, RequestStatus},
        service::{app_state::AppState, cache::types::CacheProvider, storage::local::LocalStorage},
    };

    use super::*;

    #[derive(Debug, Clone, Default)]
    struct CapturedReplayRequest {
        authorization: Option<String>,
        x_api_key: Option<String>,
        x_goog_api_key: Option<String>,
        body: String,
    }

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
                response_body_capture_state: Some(REPLAY_BODY_CAPTURE_COMPLETE.to_string()),
                response_body_capture: Some(RequestReplayBodyCaptureMetadata {
                    state: REPLAY_BODY_CAPTURE_COMPLETE.to_string(),
                    bytes_captured: 11,
                    original_size_bytes: Some(11),
                    original_size_known: true,
                    truncated: false,
                    sha256: sha256_hex(br#"{"ok":true}"#),
                    capture_limit_bytes: replay_response_capture_limit() as i64,
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

    fn credential(request_key: &str) -> ReplayResolvedCredential {
        ReplayResolvedCredential {
            provider_api_key_id: 3,
            request_key: request_key.to_string(),
            used_override: false,
        }
    }

    fn llm_api_type_for_provider(provider_type: &ProviderType) -> LlmApiType {
        match provider_type {
            ProviderType::Gemini | ProviderType::Vertex => LlmApiType::Gemini,
            ProviderType::Anthropic => LlmApiType::Anthropic,
            ProviderType::Responses => LlmApiType::Responses,
            ProviderType::GeminiOpenai => LlmApiType::GeminiOpenai,
            ProviderType::Ollama => LlmApiType::Ollama,
            ProviderType::Openai | ProviderType::VertexOpenai => LlmApiType::Openai,
        }
    }

    fn provider_key_and_name(provider_type: &ProviderType) -> (&'static str, &'static str) {
        match provider_type {
            ProviderType::Openai => ("openai", "OpenAI"),
            ProviderType::Gemini => ("gemini", "Gemini"),
            ProviderType::Vertex => ("vertex", "Vertex"),
            ProviderType::VertexOpenai => ("vertex-openai", "Vertex OpenAI"),
            ProviderType::Ollama => ("ollama", "Ollama"),
            ProviderType::Anthropic => ("anthropic", "Anthropic"),
            ProviderType::Responses => ("responses", "Responses"),
            ProviderType::GeminiOpenai => ("gemini-openai", "Gemini OpenAI"),
        }
    }

    fn source(request_uri: String, provider_type: ProviderType) -> AttemptReplaySource {
        let llm_api_type = llm_api_type_for_provider(&provider_type);
        let (provider_key, provider_name) = provider_key_and_name(&provider_type);
        let sanitized_request_headers = vec![
            RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            },
            RequestReplayNameValue {
                name: "x-trace-id".to_string(),
                value: Some("trace-1".to_string()),
            },
        ];
        let request_headers =
            build_header_map_from_name_values(&sanitized_request_headers).expect("headers");
        let baseline_response_body = DecodedBundleBody {
            bytes: Bytes::from_static(
                br#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"message":{"role":"assistant","content":"pong"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":3,"total_tokens":7}}"#,
            ),
            media_type: Some("application/json".to_string()),
            capture_state: Some("complete".to_string()),
        };

        AttemptReplaySource {
            request_log_id: 42,
            attempt: RequestAttemptDetail {
                id: 101,
                request_log_id: 42,
                attempt_index: 1,
                candidate_position: 1,
                provider_id: Some(2),
                provider_api_key_id: Some(3),
                model_id: Some(4),
                provider_key_snapshot: Some(provider_key.to_string()),
                provider_name_snapshot: Some(provider_name.to_string()),
                model_name_snapshot: Some("gpt-test".to_string()),
                real_model_name_snapshot: Some("gpt-4o-mini".to_string()),
                llm_api_type: Some(llm_api_type),
                attempt_status: RequestAttemptStatus::Success,
                http_status: Some(200),
                total_tokens: Some(7),
                estimated_cost_nanos: Some(100),
                estimated_cost_currency: Some("USD".to_string()),
                ..Default::default()
            },
            resolved_route_id: Some(8),
            resolved_route_name: Some("primary".to_string()),
            provider: Arc::new(CacheProvider {
                id: 2,
                provider_key: provider_key.to_string(),
                name: provider_name.to_string(),
                endpoint: "https://upstream.example/v1".to_string(),
                use_proxy: false,
                provider_type,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
                is_enabled: true,
            }),
            llm_api_type,
            request_uri,
            sanitized_request_headers,
            request_headers,
            llm_request_body: DecodedBundleBody {
                bytes: Bytes::from_static(
                    br#"{"model":"gpt-4o-mini","messages":[{"role":"user","content":"ping"}]}"#,
                ),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            },
            baseline_response_headers: vec![RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            }],
            baseline_response_body: Some(baseline_response_body),
            cost_catalog_version: None,
        }
    }

    fn replay_path(provider_type: &ProviderType) -> &'static str {
        match provider_type {
            ProviderType::Gemini | ProviderType::Vertex => {
                "/v1beta/models/gemini-2.5-pro:generateContent"
            }
            ProviderType::Anthropic => "/v1/messages",
            ProviderType::Ollama => "/api/chat",
            ProviderType::Openai
            | ProviderType::Responses
            | ProviderType::GeminiOpenai
            | ProviderType::VertexOpenai => "/v1/chat/completions",
        }
    }

    fn replay_request_uri(base_url: &str, provider_type: &ProviderType) -> String {
        format!("{}{}", base_url, replay_path(provider_type))
    }

    fn replay_auth_header_name(provider_type: &ProviderType) -> &'static str {
        match provider_type {
            ProviderType::Gemini => "x-goog-api-key",
            ProviderType::Anthropic => "x-api-key",
            ProviderType::Openai
            | ProviderType::Responses
            | ProviderType::Vertex
            | ProviderType::VertexOpenai
            | ProviderType::Ollama
            | ProviderType::GeminiOpenai => "authorization",
        }
    }

    async fn spawn_upstream(
        path: &'static str,
        status: StatusCode,
        response_content_type: &'static str,
        response_body: impl Into<String>,
    ) -> (
        String,
        Arc<tokio::sync::Mutex<Option<CapturedReplayRequest>>>,
    ) {
        let captured = Arc::new(tokio::sync::Mutex::new(None));
        let captured_for_handler = Arc::clone(&captured);
        let response_body = Arc::new(response_body.into());
        let app = Router::new().route(
            path,
            post(move |headers: AxumHeaderMap, body: AxumBytes| {
                let captured_for_handler = Arc::clone(&captured_for_handler);
                let response_body = Arc::clone(&response_body);
                async move {
                    *captured_for_handler.lock().await = Some(CapturedReplayRequest {
                        authorization: headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                        x_api_key: headers
                            .get("x-api-key")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                        x_goog_api_key: headers
                            .get("x-goog-api-key")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                        body: String::from_utf8_lossy(&body).to_string(),
                    });
                    (
                        status,
                        [(CONTENT_TYPE, response_content_type)],
                        response_body.as_str().to_string(),
                    )
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener should have address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock upstream should serve");
        });

        (format!("http://{}", addr), captured)
    }

    #[test]
    fn attempt_replay_preview_redacts_provider_specific_auth_headers() {
        let cases = [
            ProviderType::Openai,
            ProviderType::Responses,
            ProviderType::Gemini,
            ProviderType::Vertex,
            ProviderType::VertexOpenai,
            ProviderType::GeminiOpenai,
            ProviderType::Anthropic,
            ProviderType::Ollama,
        ];

        for provider_type in cases {
            let source = source(
                format!("https://upstream.example{}", replay_path(&provider_type)),
                provider_type.clone(),
            );

            let preview =
                build_attempt_replay_preview(&source, &credential("sk-live"), 1_776_840_000_000)
                    .expect("preview should build");

            let auth_header = preview
                .execution_preview
                .final_request_headers
                .iter()
                .find(|header| header.name == replay_auth_header_name(&provider_type))
                .expect("provider auth header should be present");
            assert_eq!(auth_header.value, None);
            assert!(
                preview
                    .execution_preview
                    .final_request_headers
                    .iter()
                    .any(|header| {
                        header.name == "content-type"
                            && header.value.as_deref() == Some("application/json")
                    })
            );
            assert_eq!(preview.selected_provider_api_key_id, 3);
            assert_eq!(preview.baseline.total_tokens, Some(7));
            assert_eq!(preview.preview_created_at, 1_776_840_000_000);
            assert!(
                preview
                    .preview_fingerprint
                    .starts_with("request-replay-preview-v1:1776840000000:")
            );
        }
    }

    #[test]
    fn attempt_replay_preview_rejects_provider_protocol_mismatch() {
        let mut source = source(
            "https://upstream.example/v1/messages".to_string(),
            ProviderType::Anthropic,
        );
        source.llm_api_type = LlmApiType::Openai;
        source.attempt.llm_api_type = Some(LlmApiType::Openai);

        let err = build_attempt_replay_preview(&source, &credential("sk-live"), 1_776_840_000_000)
            .expect_err("preview should reject mismatched provider protocol");

        assert!(matches!(
            err,
            BaseError::ParamInvalid(Some(message))
                if message.contains("does not support downstream protocol")
        ));
    }

    #[test]
    fn replay_preview_fingerprint_is_deterministic_for_equivalent_attempt_preview() {
        let left = source(
            "https://upstream.example/v1/chat/completions".to_string(),
            ProviderType::Openai,
        );
        let mut right = left.clone();
        right.sanitized_request_headers.reverse();
        right.request_headers =
            build_header_map_from_name_values(&right.sanitized_request_headers).expect("headers");

        let left_preview =
            build_attempt_replay_preview(&left, &credential("sk-live"), 1_776_840_000_000)
                .expect("left preview should build");
        let right_preview =
            build_attempt_replay_preview(&right, &credential("sk-live"), 1_776_840_000_000)
                .expect("right preview should build");

        assert_eq!(
            left_preview.preview_fingerprint,
            right_preview.preview_fingerprint
        );
    }

    #[test]
    fn replay_preview_fingerprint_canonicalizes_uri_query_order() {
        assert_eq!(
            canonical_uri_for_fingerprint(
                "https://upstream.example/v1/models/gemini:generate?b=2&a=1"
            ),
            canonical_uri_for_fingerprint(
                "https://upstream.example/v1/models/gemini:generate?a=1&b=2"
            )
        );
    }

    #[test]
    fn replay_preview_confirmation_distinguishes_missing_expired_and_mismatch() {
        let missing = parse_replay_preview_confirmation(None)
            .expect_err("missing confirmation should be rejected");
        assert!(
            matches!(missing, BaseError::ParamInvalid(Some(message)) if message.contains("missing"))
        );

        let expired = format!(
            "{}:{}:{}",
            REPLAY_PREVIEW_FINGERPRINT_VERSION,
            1,
            "0".repeat(64)
        );
        let expired = parse_replay_preview_confirmation(Some(&expired))
            .expect_err("expired confirmation should be rejected");
        assert!(
            matches!(expired, BaseError::ParamInvalid(Some(message)) if message.contains("expired"))
        );

        let source = source(
            "https://upstream.example/v1/chat/completions".to_string(),
            ProviderType::Openai,
        );
        let preview = build_attempt_replay_preview(
            &source,
            &credential("sk-live"),
            Utc::now().timestamp_millis(),
        )
        .expect("preview should build");
        let mismatch =
            ensure_replay_preview_confirmation_matches(Some(&preview.preview_fingerprint), "other")
                .expect_err("mismatched confirmation should be rejected");
        assert!(
            matches!(mismatch, BaseError::ParamInvalid(Some(message)) if message.contains("mismatch"))
        );
    }

    #[test]
    fn replay_preview_fingerprint_changes_when_override_or_body_changes() {
        let source = source(
            "https://upstream.example/v1/chat/completions".to_string(),
            ProviderType::Openai,
        );
        let created_at = 1_776_840_000_000;
        let baseline = build_attempt_replay_preview(&source, &credential("sk-live"), created_at)
            .expect("baseline preview should build");

        let mut override_credential = credential("sk-other");
        override_credential.provider_api_key_id = 9;
        override_credential.used_override = true;
        let override_preview =
            build_attempt_replay_preview(&source, &override_credential, created_at)
                .expect("override preview should build");
        assert_ne!(
            baseline.preview_fingerprint,
            override_preview.preview_fingerprint
        );

        let mut changed_source = source.clone();
        changed_source.llm_request_body.bytes = Bytes::from_static(
            br#"{"model":"gpt-4o-mini","messages":[{"role":"user","content":"changed"}]}"#,
        );
        let changed_preview =
            build_attempt_replay_preview(&changed_source, &credential("sk-live"), created_at)
                .expect("changed preview should build");
        assert_ne!(
            baseline.preview_fingerprint,
            changed_preview.preview_fingerprint
        );
    }

    #[tokio::test]
    async fn attempt_replay_execute_reuses_request_snapshot_and_parses_usage() {
        let response_body = r#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"message":{"role":"assistant","content":"pong"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":3,"total_tokens":7}}"#;
        let provider_type = ProviderType::Openai;
        let (base_url, captured) = spawn_upstream(
            replay_path(&provider_type),
            StatusCode::OK,
            "application/json",
            response_body,
        )
        .await;
        let source = source(replay_request_uri(&base_url, &provider_type), provider_type);
        let app_state = Arc::new(AppState::new().await);

        let outcome =
            perform_attempt_replay_execution(&app_state, &source, &credential("sk-live")).await;

        assert_eq!(outcome.status, RequestReplayStatus::Success);
        assert_eq!(outcome.http_status, Some(200));
        assert_eq!(outcome.total_tokens, Some(7));
        assert_eq!(
            outcome.response_body_capture_state.as_deref(),
            Some("complete")
        );
        assert!(
            outcome
                .response_body
                .as_ref()
                .and_then(|body| body.json.as_ref())
                .is_some()
        );

        let captured = captured
            .lock()
            .await
            .clone()
            .expect("request should be captured");
        assert_eq!(captured.authorization.as_deref(), Some("Bearer sk-live"));
        assert!(captured.x_api_key.is_none());
        assert!(captured.x_goog_api_key.is_none());
        assert!(captured.body.contains("\"ping\""));

        let diff = build_attempt_replay_diff(&source, &outcome);
        assert_eq!(diff.status_changed, Some(false));
        assert_eq!(diff.body_changed, Some(false));
    }

    #[tokio::test]
    async fn attempt_replay_execute_rebuilds_provider_specific_auth_headers() {
        let cases = [
            (
                ProviderType::Gemini,
                "gk-live",
                Some(("x_goog_api_key", "gk-live")),
            ),
            (
                ProviderType::Anthropic,
                "ak-live",
                Some(("x_api_key", "ak-live")),
            ),
            (
                ProviderType::Vertex,
                "ya29.vertex-live",
                Some(("authorization", "Bearer ya29.vertex-live")),
            ),
            (
                ProviderType::VertexOpenai,
                "ya29.vertex-openai-live",
                Some(("authorization", "Bearer ya29.vertex-openai-live")),
            ),
        ];

        for (provider_type, request_key, expected_auth) in cases {
            let (base_url, captured) = spawn_upstream(
                replay_path(&provider_type),
                StatusCode::OK,
                "text/plain",
                "pong",
            )
            .await;
            let source = source(
                replay_request_uri(&base_url, &provider_type),
                provider_type.clone(),
            );
            let app_state = Arc::new(AppState::new().await);

            let outcome =
                perform_attempt_replay_execution(&app_state, &source, &credential(request_key))
                    .await;

            assert_eq!(outcome.status, RequestReplayStatus::Success);
            assert_eq!(outcome.http_status, Some(200));

            let captured = captured
                .lock()
                .await
                .clone()
                .expect("request should be captured");
            match expected_auth {
                Some(("authorization", value)) => {
                    assert_eq!(captured.authorization.as_deref(), Some(value));
                    assert!(captured.x_api_key.is_none());
                    assert!(captured.x_goog_api_key.is_none());
                }
                Some(("x_api_key", value)) => {
                    assert_eq!(captured.x_api_key.as_deref(), Some(value));
                    assert!(captured.authorization.is_none());
                    assert!(captured.x_goog_api_key.is_none());
                }
                Some(("x_goog_api_key", value)) => {
                    assert_eq!(captured.x_goog_api_key.as_deref(), Some(value));
                    assert!(captured.authorization.is_none());
                    assert!(captured.x_api_key.is_none());
                }
                Some((other, _)) => panic!("unexpected auth capture field: {other}"),
                None => panic!("auth expectation must be present"),
            }
        }
    }

    #[tokio::test]
    async fn attempt_replay_execute_maps_upstream_errors_and_preserves_error_body() {
        let response_body = r#"{"error":{"message":"slow down"}}"#;
        let provider_type = ProviderType::Openai;
        let (base_url, _captured) = spawn_upstream(
            replay_path(&provider_type),
            StatusCode::TOO_MANY_REQUESTS,
            "application/json",
            response_body,
        )
        .await;
        let source = source(replay_request_uri(&base_url, &provider_type), provider_type);
        let app_state = Arc::new(AppState::new().await);

        let outcome =
            perform_attempt_replay_execution(&app_state, &source, &credential("sk-live")).await;

        assert_eq!(outcome.status, RequestReplayStatus::Error);
        assert_eq!(outcome.http_status, Some(429));
        assert_eq!(
            outcome.error_code.as_deref(),
            Some("upstream_rate_limit_error")
        );
        assert_eq!(
            outcome.response_body_capture_state.as_deref(),
            Some("complete")
        );

        let diff = build_attempt_replay_diff(&source, &outcome);
        assert_eq!(diff.status_changed, Some(true));
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line.contains("status changed"))
        );
    }

    #[tokio::test]
    async fn replay_response_capture_marks_large_plain_body_incomplete() {
        let limit = 32usize;
        let stream = futures::stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::from(vec![b'a'; 20])),
            Ok::<Bytes, std::io::Error>(Bytes::from(vec![b'b'; 20])),
        ]);

        let capture = read_replay_response_body_bounded(stream, false, limit, |err| {
            ProxyError::BadGateway(err.to_string())
        })
        .await
        .expect("capture should succeed");

        assert_eq!(capture.state, LogBodyCaptureState::Incomplete);
        assert_eq!(capture.body.len(), limit);
        assert!(capture.truncated);
        let metadata = replay_body_capture_metadata(&capture);
        assert_eq!(metadata.bytes_captured, limit as i64);
        assert!(!metadata.original_size_known);
        assert_eq!(metadata.body_encoding, "identity");
    }

    #[tokio::test]
    async fn replay_response_capture_limits_gzip_after_decode() {
        let limit = 64usize;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&vec![b'x'; limit * 4])
            .expect("gzip write should succeed");
        let compressed = encoder.finish().expect("gzip finish should succeed");
        let stream =
            futures::stream::iter(vec![Ok::<Bytes, std::io::Error>(Bytes::from(compressed))]);

        let capture = read_replay_response_body_bounded(stream, true, limit, |err| {
            ProxyError::BadGateway(err.to_string())
        })
        .await
        .expect("capture should succeed");

        assert_eq!(capture.state, LogBodyCaptureState::Incomplete);
        assert_eq!(capture.body.len(), limit);
        assert_eq!(capture.body, Bytes::from(vec![b'x'; limit]));
        assert_eq!(capture.body_encoding, "decoded:gzip");
    }

    #[tokio::test]
    async fn replay_response_capture_marks_large_sse_body_incomplete_and_degrades_parse() {
        let limit = 80usize;
        let sse = format!(
            "data: {}\n\ndata: {}\n\n",
            serde_json::json!({"choices":[{"delta":{"content":"a".repeat(limit)}}]}),
            serde_json::json!({"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
        );
        let stream = futures::stream::iter(vec![Ok::<Bytes, std::io::Error>(Bytes::from(sse))]);

        let capture = read_replay_response_body_bounded(stream, false, limit, |err| {
            ProxyError::BadGateway(err.to_string())
        })
        .await
        .expect("capture should succeed");
        let (usage, diagnostics) =
            parse_stream_usage_and_diagnostics(&capture.body, LlmApiType::Openai);

        assert_eq!(capture.state, LogBodyCaptureState::Incomplete);
        assert_eq!(capture.body.len(), limit);
        assert!(usage.is_none());
        assert!(diagnostics.is_empty());
    }

    #[tokio::test]
    async fn attempt_replay_large_response_persists_incomplete_capture() {
        let limit = replay_response_capture_limit();
        let response_body = "x".repeat(limit + 1024);
        let provider_type = ProviderType::Openai;
        let (base_url, _captured) = spawn_upstream(
            replay_path(&provider_type),
            StatusCode::OK,
            "text/plain",
            response_body,
        )
        .await;
        let source = source(replay_request_uri(&base_url, &provider_type), provider_type);
        let app_state = Arc::new(AppState::new().await);

        let outcome =
            perform_attempt_replay_execution(&app_state, &source, &credential("sk-live")).await;

        assert_eq!(outcome.status, RequestReplayStatus::Success);
        assert_eq!(outcome.http_status, Some(200));
        assert_eq!(
            outcome.response_body_capture_state.as_deref(),
            Some(REPLAY_BODY_CAPTURE_INCOMPLETE)
        );
        assert_eq!(
            outcome
                .response_body_capture
                .as_ref()
                .map(|capture| capture.bytes_captured),
            Some(limit as i64)
        );
        assert_eq!(
            outcome.response_body_bytes.as_ref().map(Bytes::len),
            Some(limit)
        );

        let diff = build_attempt_replay_diff(&source, &outcome);
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line.contains("partial") && line.contains("incomplete"))
        );
    }

    #[test]
    fn attempt_replay_diff_marks_partial_body_comparison_when_replay_body_missing() {
        let source = source(
            "https://upstream.example/v1/chat/completions".to_string(),
            ProviderType::Openai,
        );
        let outcome = execution_outcome_from_proxy_error(ProxyError::BadGateway(
            "upstream body missing".to_string(),
        ));

        let diff = build_attempt_replay_diff(&source, &outcome);

        assert_eq!(diff.body_changed, None);
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line.contains("partial"))
        );
    }

    fn gateway_request_log(
        overall_status: RequestStatus,
        final_error_code: Option<&str>,
        final_attempt_id: Option<i64>,
    ) -> RequestLogRecord {
        RequestLogRecord {
            id: 42,
            api_key_id: 7,
            requested_model_name: Some("gpt-test".to_string()),
            resolved_name_scope: Some("direct".to_string()),
            resolved_route_id: Some(8),
            resolved_route_name: Some("primary".to_string()),
            user_api_type: LlmApiType::Openai,
            overall_status,
            final_error_code: final_error_code.map(str::to_string),
            final_error_message: None,
            attempt_count: 1,
            retry_count: 0,
            fallback_count: 0,
            request_received_at: 1,
            first_attempt_started_at: Some(2),
            response_started_to_client_at: Some(3),
            completed_at: Some(4),
            client_ip: None,
            final_attempt_id,
            final_provider_id: Some(2),
            final_provider_api_key_id: Some(3),
            final_model_id: Some(4),
            final_provider_key_snapshot: Some("openai".to_string()),
            final_provider_name_snapshot: Some("OpenAI".to_string()),
            final_model_name_snapshot: Some("gpt-test".to_string()),
            final_real_model_name_snapshot: Some("gpt-4o-mini".to_string()),
            final_llm_api_type: Some(LlmApiType::Openai),
            estimated_cost_nanos: Some(100),
            estimated_cost_currency: Some("USD".to_string()),
            cost_catalog_id: None,
            cost_catalog_version_id: None,
            cost_snapshot_json: None,
            total_input_tokens: Some(4),
            total_output_tokens: Some(3),
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            total_tokens: Some(7),
            has_transform_diagnostics: false,
            transform_diagnostic_count: 0,
            transform_diagnostic_max_loss_level: None,
            bundle_version: Some(2),
            bundle_storage_type: Some(StorageType::FileSystem),
            bundle_storage_key: Some("logs/2026/04/23/42.mp.gz".to_string()),
            created_at: 1,
            updated_at: 4,
        }
    }

    fn gateway_final_attempt(
        http_status: Option<i32>,
        error_code: Option<&str>,
        response_headers_json: Option<&str>,
    ) -> RequestAttemptDetail {
        RequestAttemptDetail {
            id: 101,
            request_log_id: 42,
            attempt_index: 1,
            candidate_position: 1,
            provider_id: Some(2),
            provider_api_key_id: Some(3),
            model_id: Some(4),
            provider_key_snapshot: Some("openai".to_string()),
            provider_name_snapshot: Some("OpenAI".to_string()),
            model_name_snapshot: Some("gpt-test".to_string()),
            real_model_name_snapshot: Some("gpt-4o-mini".to_string()),
            llm_api_type: Some(LlmApiType::Openai),
            attempt_status: if http_status == Some(200) {
                RequestAttemptStatus::Success
            } else {
                RequestAttemptStatus::Error
            },
            scheduler_action: SchedulerAction::ReturnSuccess,
            error_code: error_code.map(str::to_string),
            response_headers_json: response_headers_json.map(str::to_string),
            http_status,
            ..Default::default()
        }
    }

    fn gateway_source_for_diff(
        request_log: RequestLogRecord,
        baseline_final_attempt: Option<RequestAttemptDetail>,
    ) -> GatewayReplaySource {
        GatewayReplaySource {
            request_log,
            request_snapshot: RequestLogBundleRequestSnapshot {
                request_path: "/ai/openai/v1/chat/completions".to_string(),
                operation_kind: "chat_completions_create".to_string(),
                ..Default::default()
            },
            original_headers: HeaderMap::new(),
            user_request_body: DecodedBundleBody {
                bytes: Bytes::from_static(
                    br#"{"model":"gpt-test","messages":[{"role":"user","content":"ping"}]}"#,
                ),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            },
            baseline_user_response_body: Some(DecodedBundleBody {
                bytes: Bytes::from_static(br#"{"ok":false}"#),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            }),
            baseline_final_attempt,
            system_api_key: Arc::new(CacheApiKey {
                id: 7,
                api_key_hash: "hash".to_string(),
                key_prefix: "ck-test".to_string(),
                key_last4: "1234".to_string(),
                name: "Test".to_string(),
                description: None,
                default_action: Action::Allow,
                is_enabled: true,
                expires_at: None,
                rate_limit_rpm: None,
                max_concurrent_requests: None,
                quota_daily_requests: None,
                quota_daily_tokens: None,
                quota_monthly_tokens: None,
                budget_daily_nanos: None,
                budget_daily_currency: None,
                budget_monthly_nanos: None,
                budget_monthly_currency: None,
                acl_rules: Vec::new(),
            }),
            requested_model_name: "gpt-test".to_string(),
            kind: GatewayReplayAttemptKind::Generation {
                api_type: LlmApiType::Openai,
                is_stream: false,
                data: serde_json::json!({"model": "gpt-test"}),
                original_request_value: serde_json::json!({"model": "gpt-test"}),
            },
        }
    }

    fn gateway_execution_preview() -> RequestReplayExecutionPreview {
        RequestReplayExecutionPreview {
            semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
            resolved_route: Some(RequestReplayResolvedRoute {
                route_id: Some(8),
                route_name: Some("primary".to_string()),
            }),
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
        }
    }

    #[test]
    fn gateway_replay_diff_marks_status_and_headers_changed_when_failure_shape_changes() {
        let source = gateway_source_for_diff(
            gateway_request_log(
                RequestStatus::Error,
                Some("upstream_rate_limit_error"),
                Some(101),
            ),
            Some(gateway_final_attempt(
                Some(429),
                Some("upstream_rate_limit_error"),
                Some(r#"{"content-type":"application/json","retry-after":"1"}"#),
            )),
        );
        let outcome = AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Error,
            http_status: Some(503),
            first_byte_at: None,
            error_code: Some("upstream_service_unavailable".to_string()),
            error_message: Some("provider unavailable".to_string()),
            response_headers: vec![RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            }],
            response_body: None,
            response_body_bytes: None,
            response_body_capture_state: Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED.to_string()),
            response_body_capture: None,
            usage_normalization: None,
            transform_diagnostics: Vec::new(),
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        };

        let diff = build_gateway_replay_diff(&source, &gateway_execution_preview(), &outcome);

        assert_eq!(diff.status_changed, Some(true));
        assert_eq!(diff.headers_changed, Some(true));
        assert!(diff.summary_lines.iter().any(|line| {
            line.contains("gateway result changed")
                && line.contains("http=429")
                && line.contains("http=503")
                && line.contains("upstream_rate_limit_error")
                && line.contains("upstream_service_unavailable")
        }));
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line == "gateway response headers changed")
        );
    }

    #[test]
    fn gateway_replay_diff_keeps_status_unchanged_when_rich_baseline_matches() {
        let source = gateway_source_for_diff(
            gateway_request_log(
                RequestStatus::Error,
                Some("upstream_rate_limit_error"),
                Some(101),
            ),
            Some(gateway_final_attempt(
                Some(429),
                Some("upstream_rate_limit_error"),
                Some(r#"{"content-type":"application/json"}"#),
            )),
        );
        let outcome = AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Error,
            http_status: Some(429),
            first_byte_at: None,
            error_code: Some("upstream_rate_limit_error".to_string()),
            error_message: Some("slow down".to_string()),
            response_headers: vec![RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            }],
            response_body: None,
            response_body_bytes: None,
            response_body_capture_state: Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED.to_string()),
            response_body_capture: None,
            usage_normalization: None,
            transform_diagnostics: Vec::new(),
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        };

        let diff = build_gateway_replay_diff(&source, &gateway_execution_preview(), &outcome);

        assert_eq!(diff.status_changed, Some(false));
        assert_eq!(diff.headers_changed, Some(false));
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line == "gateway result unchanged: error / http=429 / error_code=upstream_rate_limit_error")
        );
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line == "gateway response headers unchanged")
        );
    }

    #[test]
    fn gateway_replay_parses_gemini_full_snapshot_path() {
        let mut request_log = gateway_request_log(RequestStatus::Success, None, None);
        request_log.requested_model_name = None;
        request_log.resolved_name_scope = None;
        request_log.resolved_route_id = None;
        request_log.resolved_route_name = None;
        request_log.user_api_type = LlmApiType::Gemini;
        request_log.attempt_count = 0;
        request_log.total_input_tokens = None;
        request_log.total_output_tokens = None;
        request_log.total_tokens = None;
        request_log.bundle_version = None;
        request_log.bundle_storage_type = None;
        request_log.bundle_storage_key = None;
        let snapshot = RequestLogBundleRequestSnapshot {
            request_path: "/ai/gemini/v1beta/models/gemini-2.5-pro:streamGenerateContent"
                .to_string(),
            operation_kind: "stream_generate_content".to_string(),
            ..Default::default()
        };

        let (model, kind) = gateway_replay_kind_from_snapshot(
            &request_log,
            &snapshot,
            &serde_json::json!({"contents": []}),
        )
        .expect("gemini gateway replay kind should parse");

        assert_eq!(model, "gemini-2.5-pro");
        match kind {
            GatewayReplayAttemptKind::Generation {
                api_type,
                is_stream,
                ..
            } => {
                assert_eq!(api_type, LlmApiType::Gemini);
                assert!(is_stream);
            }
            other => panic!("expected generation kind, got {other:?}"),
        }
    }

    #[test]
    fn gateway_replay_preview_redacts_materialized_auth_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer sk-live"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let prepared = GatewayReplayPreparedRequest {
            requested_model_name: "gpt-test".to_string(),
            resolved_name_scope: "direct".to_string(),
            resolved_route_id: Some(8),
            resolved_route_name: Some("primary".to_string()),
            candidate_position: 1,
            provider_id: 2,
            provider_api_key_id: 3,
            model_id: 4,
            llm_api_type: LlmApiType::Openai,
            applied_request_patch_summary: None,
            final_request_uri: "https://upstream.example/v1/chat/completions".to_string(),
            final_request_headers: headers,
            final_request_body: Bytes::from_static(br#"{"model":"gpt-test"}"#),
            transform_diagnostics: Vec::new(),
            candidate_manifest: Default::default(),
            candidate_decisions: Vec::new(),
        };

        let preview = execution_preview_from_gateway_prepared(&prepared);

        let authorization = preview
            .final_request_headers
            .iter()
            .find(|header| header.name == "authorization")
            .expect("authorization header should be represented");
        assert_eq!(authorization.value, None);
        assert!(
            preview
                .final_request_body
                .and_then(|body| body.json)
                .is_some()
        );
    }
}
