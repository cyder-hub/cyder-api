use std::time::Duration;

use axum::{body::Body, response::Response};
use serde_json::Value;

use crate::{
    config::CONFIG,
    database::request_attempt::RequestAttempt,
    proxy::{
        ProxyError,
        logging::{LoggedBody, RequestLogContext},
        retry_policy::{
            ProviderGovernanceRejection, RetryDecision, RetryFailureKind, RetryPolicyContext,
            decide_retry,
        },
        runtime::{route_resolver::ExecutionCandidate, transport::ProxyRequestFailure},
        util::serialize_upstream_response_headers_for_log,
    },
    schema::enum_def::{LlmApiType, RequestAttemptStatus, SchedulerAction},
    service::cache::types::CacheCostCatalogVersion,
    utils::storage::LogBodyCaptureState,
};

use super::types::{RuntimeCandidateDecision, RuntimeFinalAttempt};

pub(in crate::proxy) const CAPABILITY_MISMATCH_SKIPPED_ERROR: &str = "capability_mismatch_skipped";
pub(in crate::proxy) const NO_CANDIDATE_AVAILABLE_ERROR: &str = "no_candidate_available_error";

#[derive(Debug, Clone)]
pub(in crate::proxy) struct RequestAttemptDraft {
    pub candidate_position: i32,
    pub provider_id: Option<i64>,
    pub provider_api_key_id: Option<i64>,
    pub model_id: Option<i64>,
    pub provider_key_snapshot: Option<String>,
    pub provider_name_snapshot: Option<String>,
    pub model_name_snapshot: Option<String>,
    pub real_model_name_snapshot: Option<String>,
    pub llm_api_type: Option<LlmApiType>,
    pub attempt_status: RequestAttemptStatus,
    pub scheduler_action: SchedulerAction,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub request_uri: Option<String>,
    pub request_headers_json: Option<String>,
    pub response_headers_json: Option<String>,
    pub http_status: Option<i32>,
    pub started_at: Option<i64>,
    pub first_byte_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub response_started_to_client: bool,
    pub backoff_ms: Option<i32>,
    pub applied_request_patch_ids_json: Option<String>,
    pub request_patch_summary_json: Option<String>,
    pub estimated_cost_nanos: Option<i64>,
    pub estimated_cost_currency: Option<String>,
    pub cost_catalog_version_id: Option<i64>,
    pub total_input_tokens: Option<i32>,
    pub total_output_tokens: Option<i32>,
    pub input_text_tokens: Option<i32>,
    pub output_text_tokens: Option<i32>,
    pub input_image_tokens: Option<i32>,
    pub output_image_tokens: Option<i32>,
    pub cache_read_tokens: Option<i32>,
    pub cache_write_tokens: Option<i32>,
    pub reasoning_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub llm_request_blob_id: Option<i32>,
    pub llm_request_patch_id: Option<i32>,
    pub llm_response_blob_id: Option<i32>,
    pub llm_response_capture_state: Option<String>,
    pub llm_request_body_for_log: Option<LoggedBody>,
    pub llm_response_body_for_log: Option<LoggedBody>,
}

impl RequestAttemptDraft {
    pub(in crate::proxy) fn pending_for_candidate(candidate: &ExecutionCandidate) -> Self {
        let real_model_name = candidate
            .model
            .real_model_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&candidate.model.model_name);

        Self {
            candidate_position: candidate.candidate_position as i32,
            provider_id: Some(candidate.provider.id),
            provider_api_key_id: None,
            model_id: Some(candidate.model.id),
            provider_key_snapshot: Some(candidate.provider.provider_key.clone()),
            provider_name_snapshot: Some(candidate.provider.name.clone()),
            model_name_snapshot: Some(candidate.model.model_name.clone()),
            real_model_name_snapshot: Some(real_model_name.to_string()),
            llm_api_type: Some(candidate.llm_api_type),
            attempt_status: RequestAttemptStatus::Skipped,
            scheduler_action: SchedulerAction::FailFast,
            error_code: None,
            error_message: None,
            ..Self::default()
        }
    }

    pub(in crate::proxy) fn skipped_for_capability_mismatch(
        candidate: &ExecutionCandidate,
        missing_capabilities: &[&'static str],
    ) -> Self {
        let message = format!(
            "Model '{}' does not support required capabilities: {}",
            candidate.model.model_name,
            missing_capabilities.join(", ")
        );

        Self {
            candidate_position: candidate.candidate_position as i32,
            attempt_status: RequestAttemptStatus::Skipped,
            scheduler_action: SchedulerAction::FallbackNextCandidate,
            error_code: Some(CAPABILITY_MISMATCH_SKIPPED_ERROR.to_string()),
            error_message: Some(truncate_error_message(&message)),
            ..Self::pending_for_candidate(candidate)
        }
    }

    pub(in crate::proxy) fn to_request_attempt_with_id(
        &self,
        id: i64,
        request_log_id: i64,
        attempt_index: i32,
        now: i64,
    ) -> RequestAttempt {
        let created_at = self
            .started_at
            .or(self.completed_at)
            .unwrap_or(now)
            .min(now);
        RequestAttempt {
            id,
            request_log_id,
            attempt_index,
            candidate_position: self.candidate_position,
            provider_id: self.provider_id,
            provider_api_key_id: self.provider_api_key_id,
            model_id: self.model_id,
            provider_key_snapshot: self.provider_key_snapshot.clone(),
            provider_name_snapshot: self.provider_name_snapshot.clone(),
            model_name_snapshot: self.model_name_snapshot.clone(),
            real_model_name_snapshot: self.real_model_name_snapshot.clone(),
            llm_api_type: self.llm_api_type,
            attempt_status: self.attempt_status,
            scheduler_action: self.scheduler_action,
            error_code: self.error_code.clone(),
            error_message: self.error_message.clone(),
            request_uri: self.request_uri.clone(),
            request_headers_json: self.request_headers_json.clone(),
            response_headers_json: self.response_headers_json.clone(),
            http_status: self.http_status,
            started_at: self.started_at,
            first_byte_at: self.first_byte_at,
            completed_at: self.completed_at.or(Some(now)),
            response_started_to_client: self.response_started_to_client,
            backoff_ms: self.backoff_ms,
            applied_request_patch_ids_json: self.applied_request_patch_ids_json.clone(),
            request_patch_summary_json: self.request_patch_summary_json.clone(),
            estimated_cost_nanos: self.estimated_cost_nanos,
            estimated_cost_currency: self.estimated_cost_currency.clone(),
            cost_catalog_version_id: self.cost_catalog_version_id,
            total_input_tokens: self.total_input_tokens,
            total_output_tokens: self.total_output_tokens,
            input_text_tokens: self.input_text_tokens,
            output_text_tokens: self.output_text_tokens,
            input_image_tokens: self.input_image_tokens,
            output_image_tokens: self.output_image_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
            reasoning_tokens: self.reasoning_tokens,
            total_tokens: self.total_tokens,
            llm_request_blob_id: self.llm_request_blob_id,
            llm_request_patch_id: self.llm_request_patch_id,
            llm_response_blob_id: self.llm_response_blob_id,
            llm_response_capture_state: self.llm_response_capture_state.clone(),
            created_at,
            updated_at: now,
        }
    }

    pub(in crate::proxy) fn to_runtime_candidate_decision(&self) -> RuntimeCandidateDecision {
        RuntimeCandidateDecision {
            candidate_position: self.candidate_position,
            provider_id: self.provider_id,
            provider_api_key_id: self.provider_api_key_id,
            model_id: self.model_id,
            llm_api_type: self.llm_api_type,
            attempt_status: self.attempt_status,
            scheduler_action: self.scheduler_action,
            error_code: self.error_code.clone(),
            error_message: self.error_message.clone(),
            request_uri: self.request_uri.clone(),
        }
    }

    pub(in crate::proxy) fn to_runtime_final_attempt(&self) -> RuntimeFinalAttempt {
        RuntimeFinalAttempt {
            candidate_position: self.candidate_position,
            provider_id: self.provider_id,
            provider_api_key_id: self.provider_api_key_id,
            model_id: self.model_id,
            llm_api_type: self.llm_api_type,
            attempt_status: self.attempt_status,
            error_code: self.error_code.clone(),
            error_message: self.error_message.clone(),
            request_uri: self.request_uri.clone(),
            request_headers_json: self.request_headers_json.clone(),
            request_body: logged_body_bytes(&self.llm_request_body_for_log),
            request_body_capture_state: logged_body_capture_state_string(
                &self.llm_request_body_for_log,
            ),
            response_headers_json: self.response_headers_json.clone(),
            response_body: logged_body_bytes(&self.llm_response_body_for_log),
            response_body_capture_state: self.llm_response_capture_state.clone(),
            http_status: self.http_status,
            first_byte_at: self.first_byte_at,
            applied_request_patch_summary: self
                .request_patch_summary_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok()),
            total_input_tokens: self.total_input_tokens,
            total_output_tokens: self.total_output_tokens,
            reasoning_tokens: self.reasoning_tokens,
            total_tokens: self.total_tokens,
        }
    }
}

impl Default for RequestAttemptDraft {
    fn default() -> Self {
        Self {
            candidate_position: 0,
            provider_id: None,
            provider_api_key_id: None,
            model_id: None,
            provider_key_snapshot: None,
            provider_name_snapshot: None,
            model_name_snapshot: None,
            real_model_name_snapshot: None,
            llm_api_type: None,
            attempt_status: RequestAttemptStatus::Skipped,
            scheduler_action: SchedulerAction::FailFast,
            error_code: None,
            error_message: None,
            request_uri: None,
            request_headers_json: None,
            response_headers_json: None,
            http_status: None,
            started_at: None,
            first_byte_at: None,
            completed_at: None,
            response_started_to_client: false,
            backoff_ms: None,
            applied_request_patch_ids_json: None,
            request_patch_summary_json: None,
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            cost_catalog_version_id: None,
            total_input_tokens: None,
            total_output_tokens: None,
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            llm_request_blob_id: None,
            llm_request_patch_id: None,
            llm_response_blob_id: None,
            llm_response_capture_state: None,
            llm_request_body_for_log: None,
            llm_response_body_for_log: None,
        }
    }
}

pub(in crate::proxy) fn classify_attempt_failure(
    attempt: &mut RequestAttemptDraft,
    proxy_error: &ProxyError,
    same_candidate_retry_count: u32,
    attempted_candidate_count: u32,
    next_candidate_available: bool,
    retry_after: Option<Duration>,
) {
    attempt.attempt_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
        RequestAttemptStatus::Cancelled
    } else {
        RequestAttemptStatus::Error
    };
    attempt.error_code = Some(proxy_error.error_code().to_string());
    attempt.error_message = Some(truncate_error_message(proxy_error.message()));

    let decision = decide_retry(
        &CONFIG.routing_resilience,
        RetryPolicyContext {
            failure: RetryFailureKind::ProxyError(proxy_error),
            same_candidate_retry_count,
            attempted_candidate_count,
            next_candidate_available,
            response_started_to_client: attempt.response_started_to_client,
            retry_after,
        },
    );
    attempt.scheduler_action = decision.scheduler_action();
    if let RetryDecision::RetrySameCandidate { backoff_ms } = decision {
        attempt.backoff_ms = Some(i32::try_from(backoff_ms).unwrap_or(i32::MAX));
    }
}

pub(in crate::proxy) fn classify_provider_governance_skip(
    attempt: &mut RequestAttemptDraft,
    rejection: ProviderGovernanceRejection,
    provider_label: &str,
    attempted_candidate_count: u32,
    next_candidate_available: bool,
) {
    attempt.attempt_status = RequestAttemptStatus::Skipped;
    attempt.error_code = Some(rejection.error_code().to_string());
    attempt.error_message = Some(truncate_error_message(&rejection.message(provider_label)));
    attempt.http_status = None;
    attempt.response_headers_json = None;
    attempt.first_byte_at = None;
    attempt.response_started_to_client = false;
    attempt.backoff_ms = None;
    attempt.llm_response_body_for_log = None;
    attempt.llm_response_blob_id = None;
    attempt.llm_response_capture_state = None;

    let decision = decide_retry(
        &CONFIG.routing_resilience,
        RetryPolicyContext {
            failure: RetryFailureKind::ProviderGovernance(rejection),
            same_candidate_retry_count: 0,
            attempted_candidate_count,
            next_candidate_available,
            response_started_to_client: false,
            retry_after: None,
        },
    );
    attempt.scheduler_action = decision.scheduler_action();
    if let RetryDecision::RetrySameCandidate { backoff_ms } = decision {
        attempt.backoff_ms = Some(i32::try_from(backoff_ms).unwrap_or(i32::MAX));
    }
}

pub(in crate::proxy) fn complete_attempt_from_response(
    attempt: &mut RequestAttemptDraft,
    response: &Response<Body>,
    completed_at: i64,
) {
    let status = response.status();
    attempt.http_status = Some(i32::from(status.as_u16()));
    attempt.response_headers_json = serialize_upstream_response_headers_for_log(response.headers());
    attempt.completed_at = Some(completed_at);
    if status.is_success() {
        attempt.attempt_status = RequestAttemptStatus::Success;
        attempt.scheduler_action = SchedulerAction::ReturnSuccess;
    } else {
        attempt.attempt_status = RequestAttemptStatus::Error;
        attempt.scheduler_action = SchedulerAction::FailFast;
    }
}

pub(in crate::proxy) fn sync_attempt_timing_and_usage(
    attempt: &mut RequestAttemptDraft,
    context: &RequestLogContext,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
) {
    attempt.started_at = context.llm_request_sent_at;
    attempt.first_byte_at = context.first_chunk_ts;
    attempt.cost_catalog_version_id = cost_catalog_version.map(|version| version.id);
    if let Some(usage) = context.usage_normalization.as_ref() {
        attempt.total_input_tokens = Some(usage.total_input_tokens as i32);
        attempt.total_output_tokens = Some(usage.total_output_tokens as i32);
        attempt.input_text_tokens = Some(usage.input_text_tokens as i32);
        attempt.output_text_tokens = Some(usage.output_text_tokens as i32);
        attempt.input_image_tokens = Some(usage.input_image_tokens as i32);
        attempt.output_image_tokens = Some(usage.output_image_tokens as i32);
        attempt.cache_read_tokens = Some(usage.cache_read_tokens as i32);
        attempt.cache_write_tokens = Some(usage.cache_write_tokens as i32);
        attempt.reasoning_tokens = Some(usage.reasoning_tokens as i32);
        attempt.total_tokens = Some((usage.total_input_tokens + usage.total_output_tokens) as i32);
    } else if let Some(usage) = context.usage.as_ref() {
        attempt.total_input_tokens = Some(usage.input_tokens);
        attempt.total_output_tokens = Some(usage.output_tokens);
        attempt.input_image_tokens = Some(usage.input_image_tokens);
        attempt.output_image_tokens = Some(usage.output_image_tokens);
        attempt.cache_read_tokens = Some(usage.cached_tokens);
        attempt.reasoning_tokens = Some(usage.reasoning_tokens);
        attempt.total_tokens = Some(usage.total_tokens);
    }
    attempt.llm_response_capture_state = context
        .llm_response_body
        .as_ref()
        .map(|body| body.capture_state())
        .or_else(|| {
            (context.request_url.is_some() || context.llm_status.is_some())
                .then_some(LogBodyCaptureState::NotCaptured)
        })
        .map(log_body_capture_state_as_str)
        .map(str::to_string);
}

pub(in crate::proxy) fn sync_attempt_from_proxy_failure(
    attempt: &mut RequestAttemptDraft,
    failure: &ProxyRequestFailure,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
) {
    sync_attempt_timing_and_usage(attempt, &failure.log_context, cost_catalog_version);
    attempt.http_status = failure
        .log_context
        .llm_status
        .map(|status| i32::from(status.as_u16()));
    attempt.response_headers_json = failure
        .response_headers
        .as_ref()
        .and_then(serialize_upstream_response_headers_for_log)
        .or_else(|| failure.log_context.response_headers_json.clone());
    attempt.response_started_to_client = failure.log_context.first_chunk_ts.is_some();
}

pub(in crate::proxy) fn truncate_error_message(message: &str) -> String {
    const MAX_ERROR_MESSAGE_CHARS: usize = 512;
    message.chars().take(MAX_ERROR_MESSAGE_CHARS).collect()
}

pub(in crate::proxy) fn log_body_capture_state_as_str(
    capture_state: LogBodyCaptureState,
) -> &'static str {
    match capture_state {
        LogBodyCaptureState::Complete => "COMPLETE",
        LogBodyCaptureState::Incomplete => "INCOMPLETE",
        LogBodyCaptureState::NotCaptured => "NOT_CAPTURED",
    }
}

fn logged_body_bytes(body: &Option<LoggedBody>) -> Option<bytes::Bytes> {
    match body {
        Some(LoggedBody::InMemory { bytes, .. }) => Some(bytes.clone()),
        Some(LoggedBody::Spooled { .. }) | None => None,
    }
}

fn logged_body_capture_state_string(body: &Option<LoggedBody>) -> Option<String> {
    body.as_ref()
        .map(|body| log_body_capture_state_as_str(body.capture_state()).to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{StatusCode, header::CONTENT_TYPE},
    };
    use bytes::Bytes;

    use super::*;
    use crate::{
        proxy::runtime::route_resolver::ExecutionCandidate,
        schema::enum_def::{ProviderApiKeyMode, ProviderType},
        service::cache::types::{CacheModel, CacheProvider},
    };

    fn provider(id: i64) -> Arc<CacheProvider> {
        Arc::new(CacheProvider {
            id,
            provider_key: format!("provider-{id}"),
            name: format!("Provider {id}"),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
        })
    }

    fn model(id: i64) -> Arc<CacheModel> {
        Arc::new(CacheModel {
            id,
            provider_id: id,
            model_name: format!("model-{id}"),
            real_model_name: None,
            cost_catalog_id: None,
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: true,
            supports_image_input: true,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled: true,
        })
    }

    fn candidate(position: usize) -> ExecutionCandidate {
        ExecutionCandidate {
            candidate_position: position,
            route_id: Some(1),
            route_name: Some("route".to_string()),
            route_candidate_priority: Some(position as i32),
            provider: provider(position as i64),
            model: model(position as i64),
            llm_api_type: LlmApiType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            reasoning_config_id: None,
            reasoning_config_scope: None,
            reasoning_config_source: None,
            reasoning_config_preset_id: None,
            reasoning_family: None,
            reasoning_preset: None,
            reasoning_suffix: None,
        }
    }

    #[test]
    fn skipped_for_capability_mismatch_preserves_candidate_identity_and_fallback_action() {
        let attempt =
            RequestAttemptDraft::skipped_for_capability_mismatch(&candidate(7), &["tools"]);

        assert_eq!(attempt.candidate_position, 7);
        assert_eq!(attempt.provider_id, Some(7));
        assert_eq!(attempt.model_id, Some(7));
        assert_eq!(
            attempt.error_code.as_deref(),
            Some(CAPABILITY_MISMATCH_SKIPPED_ERROR)
        );
        assert_eq!(
            attempt.scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
        assert_eq!(attempt.attempt_status, RequestAttemptStatus::Skipped);
    }

    #[test]
    fn to_request_attempt_with_id_preserves_persisted_field_mapping() {
        let mut draft = RequestAttemptDraft::pending_for_candidate(&candidate(2));
        draft.provider_api_key_id = Some(42);
        draft.attempt_status = RequestAttemptStatus::Success;
        draft.scheduler_action = SchedulerAction::ReturnSuccess;
        draft.request_uri = Some("https://example.com/v1/chat/completions".to_string());
        draft.http_status = Some(200);
        draft.started_at = Some(1_000);
        draft.completed_at = Some(1_500);
        draft.total_tokens = Some(99);

        let persisted = draft.to_request_attempt_with_id(11, 22, 3, 2_000);

        assert_eq!(persisted.id, 11);
        assert_eq!(persisted.request_log_id, 22);
        assert_eq!(persisted.attempt_index, 3);
        assert_eq!(persisted.candidate_position, 2);
        assert_eq!(persisted.provider_api_key_id, Some(42));
        assert_eq!(persisted.scheduler_action, SchedulerAction::ReturnSuccess);
        assert_eq!(persisted.http_status, Some(200));
        assert_eq!(persisted.created_at, 1_000);
        assert_eq!(persisted.updated_at, 2_000);
        assert_eq!(persisted.total_tokens, Some(99));
    }

    #[test]
    fn classify_attempt_failure_retries_then_fallbacks_when_output_is_not_visible() {
        let candidate = candidate(1);
        let error = ProxyError::UpstreamTimeout("timeout".to_string());
        let mut first_attempt = RequestAttemptDraft::pending_for_candidate(&candidate);

        classify_attempt_failure(&mut first_attempt, &error, 0, 1, true, None);

        assert_eq!(first_attempt.attempt_status, RequestAttemptStatus::Error);
        assert_eq!(
            first_attempt.scheduler_action,
            SchedulerAction::RetrySameCandidate
        );
        assert_eq!(first_attempt.backoff_ms, Some(250));
        assert_eq!(
            first_attempt.error_code.as_deref(),
            Some("upstream_timeout_error")
        );

        let mut second_attempt = RequestAttemptDraft::pending_for_candidate(&candidate);
        classify_attempt_failure(&mut second_attempt, &error, 1, 1, true, None);

        assert_eq!(
            second_attempt.scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
        assert_eq!(second_attempt.backoff_ms, None);
    }

    #[test]
    fn classify_attempt_failure_fails_fast_after_visible_streaming_output() {
        let candidate = candidate(1);
        let error = ProxyError::UpstreamTimeout("timeout after chunks".to_string());
        let mut attempt = RequestAttemptDraft::pending_for_candidate(&candidate);
        attempt.response_started_to_client = true;

        classify_attempt_failure(&mut attempt, &error, 0, 1, true, None);

        assert_eq!(attempt.attempt_status, RequestAttemptStatus::Error);
        assert_eq!(attempt.scheduler_action, SchedulerAction::FailFast);
        assert_eq!(attempt.backoff_ms, None);
    }

    #[test]
    fn classify_provider_governance_skip_fallbacks_without_retrying_same_candidate() {
        let candidate = candidate(1);
        let mut attempt = RequestAttemptDraft::pending_for_candidate(&candidate);

        classify_provider_governance_skip(
            &mut attempt,
            ProviderGovernanceRejection::Open,
            "provider/model",
            1,
            true,
        );

        assert_eq!(attempt.attempt_status, RequestAttemptStatus::Skipped);
        assert_eq!(attempt.error_code.as_deref(), Some("provider_open_skipped"));
        assert_eq!(
            attempt.scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
        assert_eq!(attempt.backoff_ms, None);
        assert_eq!(attempt.http_status, None);
        assert!(!attempt.response_started_to_client);
    }

    #[test]
    fn classify_provider_governance_half_open_probe_in_flight_fails_fast_without_next_candidate() {
        let candidate = candidate(1);
        let mut attempt = RequestAttemptDraft::pending_for_candidate(&candidate);

        classify_provider_governance_skip(
            &mut attempt,
            ProviderGovernanceRejection::HalfOpenProbeInFlight,
            "provider/model",
            1,
            false,
        );

        assert_eq!(attempt.attempt_status, RequestAttemptStatus::Skipped);
        assert_eq!(
            attempt.error_code.as_deref(),
            Some("provider_half_open_skipped")
        );
        assert_eq!(attempt.scheduler_action, SchedulerAction::FailFast);
        assert_eq!(attempt.backoff_ms, None);
        assert_eq!(attempt.http_status, None);
    }

    #[test]
    fn complete_attempt_from_response_does_not_mark_streaming_headers_as_client_visible() {
        let mut streaming_attempt = RequestAttemptDraft::pending_for_candidate(&candidate(1));
        let streaming_response = Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .body(Body::empty())
            .unwrap();

        complete_attempt_from_response(&mut streaming_attempt, &streaming_response, 2_000);

        assert_eq!(
            streaming_attempt.scheduler_action,
            SchedulerAction::ReturnSuccess
        );
        assert!(!streaming_attempt.response_started_to_client);

        let mut error_attempt = RequestAttemptDraft::pending_for_candidate(&candidate(1));
        let error_response = Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .header(CONTENT_TYPE, "text/event-stream")
            .body(Body::empty())
            .unwrap();

        complete_attempt_from_response(&mut error_attempt, &error_response, 2_100);

        assert_eq!(error_attempt.scheduler_action, SchedulerAction::FailFast);
        assert!(!error_attempt.response_started_to_client);
    }

    #[test]
    fn runtime_replay_conversion_keeps_operator_visible_attempt_fields() {
        let mut attempt = RequestAttemptDraft::pending_for_candidate(&candidate(3));
        attempt.provider_api_key_id = Some(17);
        attempt.attempt_status = RequestAttemptStatus::Error;
        attempt.scheduler_action = SchedulerAction::FallbackNextCandidate;
        attempt.error_code = Some("upstream_timeout_error".to_string());
        attempt.error_message = Some("timeout".to_string());
        attempt.request_uri = Some("https://example.com/v1/chat/completions".to_string());
        attempt.request_headers_json = Some("{\"content-type\":\"application/json\"}".to_string());
        attempt.llm_request_body_for_log = Some(LoggedBody::InMemory {
            bytes: Bytes::from_static(br#"{"model":"model-3"}"#),
            capture_state: LogBodyCaptureState::Complete,
        });
        attempt.response_headers_json = Some("{\"x-request-id\":\"req-1\"}".to_string());
        attempt.llm_response_body_for_log = Some(LoggedBody::InMemory {
            bytes: Bytes::from_static(br#"{"error":"timeout"}"#),
            capture_state: LogBodyCaptureState::Incomplete,
        });
        attempt.llm_response_capture_state = Some("INCOMPLETE".to_string());
        attempt.http_status = Some(504);
        attempt.first_byte_at = Some(1_234);
        attempt.request_patch_summary_json = Some(r#"{"applied_count":1}"#.to_string());
        attempt.total_input_tokens = Some(10);
        attempt.total_output_tokens = Some(2);
        attempt.reasoning_tokens = Some(1);
        attempt.total_tokens = Some(12);

        let decision = attempt.to_runtime_candidate_decision();
        let final_attempt = attempt.to_runtime_final_attempt();

        assert_eq!(decision.candidate_position, 3);
        assert_eq!(
            decision.scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
        assert_eq!(
            final_attempt.request_body,
            Some(Bytes::from_static(br#"{"model":"model-3"}"#))
        );
        assert_eq!(
            final_attempt.response_body,
            Some(Bytes::from_static(br#"{"error":"timeout"}"#))
        );
        assert_eq!(
            final_attempt.applied_request_patch_summary,
            Some(serde_json::json!({ "applied_count": 1 }))
        );
        assert_eq!(final_attempt.total_tokens, Some(12));
    }
}
