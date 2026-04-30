use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    schema::enum_def::{
        LlmApiType, RequestAttemptStatus, RequestReplayKind, RequestReplayMode,
        RequestReplaySemanticBasis, RequestReplayStatus, RequestStatus, SchedulerAction,
        StorageType,
    },
    service::transform::unified::UnifiedTransformDiagnostic,
};

pub const REQUEST_REPLAY_ARTIFACT_VERSION: u32 = 1;

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
    pub requested_model_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_requested_model_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_reasoning_suffix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_reasoning_preset: Option<String>,
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GatewayReplayBaselineSummary {
    pub overall_status: RequestStatus,
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

#[cfg(test)]
mod tests {
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
                candidate_decisions: vec![RequestReplayCandidateDecision {
                    candidate_position: 1,
                    provider_id: Some(2),
                    provider_api_key_id: Some(3),
                    model_id: Some(4),
                    llm_api_type: Some(LlmApiType::Openai),
                    attempt_status: RequestAttemptStatus::Success,
                    scheduler_action: SchedulerAction::ReturnSuccess,
                    error_code: None,
                    error_message: None,
                    request_uri: Some("https://upstream.example/v1/chat/completions".to_string()),
                }],
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

    #[test]
    fn replay_artifact_v1_round_trips_through_msgpack_contract() {
        let artifact = artifact();
        let encoded = rmp_serde::to_vec_named(&artifact).expect("artifact should encode");
        let decoded: RequestReplayArtifact =
            rmp_serde::from_slice(&encoded).expect("artifact should decode");

        assert_eq!(decoded.version, REQUEST_REPLAY_ARTIFACT_VERSION);
        assert_eq!(decoded, artifact);
    }

    #[test]
    fn replay_artifact_v1_preserves_frontend_wire_fields() {
        let json = serde_json::to_value(artifact()).expect("artifact should serialize");

        assert_eq!(json["version"], 1);
        assert_eq!(json["source"]["replay_kind"], "attempt_upstream");
        assert_eq!(json["source"]["replay_mode"], "live");
        assert_eq!(json["input_snapshot"]["kind"], "attempt_upstream");
        assert_eq!(
            json["execution_preview"]["semantic_basis"],
            "historical_attempt_snapshot"
        );
        assert_eq!(
            json["execution_preview"]["candidate_decisions"][0]["scheduler_action"],
            "RETURN_SUCCESS"
        );
        assert_eq!(json["result"]["status"], "success");
        assert_eq!(json["diff"]["baseline_kind"], "original_attempt");
    }

    #[test]
    fn replay_execute_params_default_to_safe_dry_run_inputs() {
        let attempt: AttemptReplayExecuteParams =
            serde_json::from_value(serde_json::json!({})).expect("attempt params should decode");
        let gateway: GatewayReplayExecuteParams =
            serde_json::from_value(serde_json::json!({})).expect("gateway params should decode");

        assert_eq!(attempt.replay_mode, None);
        assert!(!attempt.confirm_live_request);
        assert_eq!(gateway.replay_mode, None);
        assert!(!gateway.confirm_live_request);
    }
}
