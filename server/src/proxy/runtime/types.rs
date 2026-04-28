use axum::body::Bytes;
use serde_json::Value;

use crate::schema::enum_def::{LlmApiType, RequestAttemptStatus, SchedulerAction};

#[derive(Debug, Clone)]
pub(crate) struct RuntimeCandidateDecision {
    pub candidate_position: i32,
    pub provider_id: Option<i64>,
    pub provider_api_key_id: Option<i64>,
    pub model_id: Option<i64>,
    pub llm_api_type: Option<LlmApiType>,
    pub attempt_status: RequestAttemptStatus,
    pub scheduler_action: SchedulerAction,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub request_uri: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeFinalAttempt {
    pub candidate_position: i32,
    pub provider_id: Option<i64>,
    pub provider_api_key_id: Option<i64>,
    pub model_id: Option<i64>,
    pub llm_api_type: Option<LlmApiType>,
    pub attempt_status: RequestAttemptStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub request_uri: Option<String>,
    pub request_headers_json: Option<String>,
    pub request_body: Option<Bytes>,
    pub request_body_capture_state: Option<String>,
    pub response_headers_json: Option<String>,
    pub response_body: Option<Bytes>,
    pub response_body_capture_state: Option<String>,
    pub http_status: Option<i32>,
    pub first_byte_at: Option<i64>,
    pub applied_request_patch_summary: Option<Value>,
    pub total_input_tokens: Option<i32>,
    pub total_output_tokens: Option<i32>,
    pub reasoning_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
}
