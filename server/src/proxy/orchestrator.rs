use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    http::HeaderMap,
    response::Response,
};
use chrono::Utc;
use cyder_tools::log::{error, info, warn};
use reqwest::header::RETRY_AFTER;
use serde_json::Value;
use tokio::time::sleep;

use super::{
    ProxyError,
    auth::{admit_api_key_request, check_access_control},
    cancellation::ProxyCancellationContext,
    core::{ProxyLogMode, ProxyRequestFailure, ProxyResponseMode, proxy_request},
    load_runtime_request_patch_trace,
    logging::{LoggedBody, RequestLogContext, record_request_completion_and_log},
    prepare::{
        ExecutionCandidate, ExecutionPlan, prepare_generation_request, prepare_llm_request,
        prepare_simple_gemini_request, resolve_provider_credentials,
    },
    protocol_transform_error,
    provider_governance::ensure_provider_request_allowed,
    retry_policy::{
        ProviderGovernanceRejection, RetryDecision, RetryFailureKind, RetryPolicyContext,
        decide_retry,
    },
    util::{
        format_model_str, get_cost_catalog_version, serialize_downstream_request_headers_for_log,
        serialize_upstream_response_headers_for_log,
    },
    utility::{UtilityOperation, UtilityProtocol, validate_utility_target},
};
use crate::{
    config::CONFIG,
    database::request_attempt::RequestAttempt,
    schema::enum_def::{LlmApiType, RequestAttemptStatus, RequestStatus, SchedulerAction},
    service::{
        app_state::AppState,
        cache::types::{CacheApiKey, CacheCostCatalogVersion},
        transform::transform_request_data,
    },
    utils::storage::LogBodyCaptureState,
};

pub(super) const CAPABILITY_MISMATCH_SKIPPED_ERROR: &str = "capability_mismatch_skipped";
pub(super) const NO_CANDIDATE_AVAILABLE_ERROR: &str = "no_candidate_available_error";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ExecutionRequirement {
    pub requires_streaming: bool,
    pub requires_tools: bool,
    pub requires_reasoning: bool,
    pub requires_image_input: bool,
    pub requires_embeddings: bool,
    pub requires_rerank: bool,
}

impl ExecutionRequirement {
    fn required_capability_names(&self) -> Vec<&'static str> {
        [
            (self.requires_streaming, "streaming"),
            (self.requires_tools, "tools"),
            (self.requires_reasoning, "reasoning"),
            (self.requires_image_input, "image_input"),
            (self.requires_embeddings, "embeddings"),
            (self.requires_rerank, "rerank"),
        ]
        .into_iter()
        .filter_map(|(required, name)| required.then_some(name))
        .collect()
    }
}

#[derive(Debug, Clone)]
pub(super) struct RequestAttemptDraft {
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
    pub(super) fn pending_for_candidate(candidate: &ExecutionCandidate) -> Self {
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

    pub(super) fn skipped_for_capability_mismatch(
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

    pub(super) fn to_request_attempt_with_id(
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

#[derive(Debug)]
pub(super) enum AttemptExecutionKind {
    Generation {
        user_api_type: LlmApiType,
        is_stream: bool,
        data: Value,
        original_request_value: Value,
    },
    Utility {
        operation: UtilityOperation,
        data: Value,
    },
}

pub(super) struct AttemptExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub system_api_key: Arc<CacheApiKey>,
    pub candidate: ExecutionCandidate,
    pub requested_model_name: String,
    pub resolved_name_scope: String,
    pub resolved_route_id: Option<i64>,
    pub resolved_route_name: Option<String>,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub original_request_body: Bytes,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub skipped_attempts: Vec<RequestAttemptDraft>,
    pub same_candidate_retry_count: u32,
    pub attempted_candidate_count: u32,
    pub next_candidate_available: bool,
    pub log_mode: AttemptLogMode,
    pub kind: AttemptExecutionKind,
}

pub(super) struct AttemptExecutionResult {
    pub attempt: RequestAttemptDraft,
    pub response: Result<Response<Body>, ProxyError>,
    pub log_context: RequestLogContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AttemptLogMode {
    #[allow(dead_code)]
    RecordAll,
    DeferNonStreaming,
}

impl AttemptLogMode {
    fn should_record_attempt_failure(self) -> bool {
        matches!(self, Self::RecordAll)
    }

    fn proxy_log_mode(self) -> ProxyLogMode {
        match self {
            Self::RecordAll => ProxyLogMode::RecordAll,
            Self::DeferNonStreaming => ProxyLogMode::DeferNonStreaming,
        }
    }
}

struct MaterializedAttemptRequest {
    final_url: String,
    final_headers: HeaderMap,
    final_body: Bytes,
    llm_request_body_for_log: Option<LoggedBody>,
    model_str: String,
    response_mode: ProxyResponseMode,
}

fn finalize_attempt_failure_context(
    mut log_context: RequestLogContext,
    skipped_attempts: &[RequestAttemptDraft],
    attempt: &RequestAttemptDraft,
    proxy_error: &ProxyError,
) -> RequestLogContext {
    log_context.completion_ts = Some(Utc::now().timestamp_millis());
    log_context.overall_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
        RequestStatus::Cancelled
    } else {
        RequestStatus::Error
    };
    log_context.final_error_code = Some(proxy_error.error_code().to_string());
    log_context.final_error_message = Some(truncate_error_message(proxy_error.message()));
    log_context.set_attempts_for_logging(skipped_attempts, Some(attempt.clone()));
    log_context
}

async fn record_attempt_failure(
    app_state: &Arc<AppState>,
    log_context: RequestLogContext,
    skipped_attempts: &[RequestAttemptDraft],
    attempt: &RequestAttemptDraft,
    proxy_error: &ProxyError,
) -> RequestLogContext {
    let log_context =
        finalize_attempt_failure_context(log_context, skipped_attempts, attempt, proxy_error);
    record_request_completion_and_log(app_state, log_context.clone()).await;
    log_context
}

async fn maybe_record_attempt_failure(
    app_state: &Arc<AppState>,
    log_context: RequestLogContext,
    skipped_attempts: &[RequestAttemptDraft],
    attempt: &RequestAttemptDraft,
    proxy_error: &ProxyError,
    log_mode: AttemptLogMode,
) -> RequestLogContext {
    if log_mode.should_record_attempt_failure() {
        record_attempt_failure(
            app_state,
            log_context,
            skipped_attempts,
            attempt,
            proxy_error,
        )
        .await
    } else {
        finalize_attempt_failure_context(log_context, skipped_attempts, attempt, proxy_error)
    }
}

fn classify_attempt_failure(
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
    if let super::retry_policy::RetryDecision::RetrySameCandidate { backoff_ms } = decision {
        attempt.backoff_ms = Some(i32::try_from(backoff_ms).unwrap_or(i32::MAX));
    }
}

fn classify_provider_governance_skip(
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

fn retry_after_from_headers(headers: Option<&HeaderMap>) -> Option<Duration> {
    headers
        .and_then(|headers| headers.get(RETRY_AFTER))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn complete_attempt_from_response(
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

fn sync_attempt_timing_and_usage(
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

fn sync_attempt_from_proxy_failure(
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

fn log_body_capture_state_as_str(capture_state: LogBodyCaptureState) -> &'static str {
    match capture_state {
        LogBodyCaptureState::Complete => "COMPLETE",
        LogBodyCaptureState::Incomplete => "INCOMPLETE",
        LogBodyCaptureState::NotCaptured => "NOT_CAPTURED",
    }
}

async fn materialize_generation_attempt(
    candidate: &ExecutionCandidate,
    mut data: Value,
    user_api_type: LlmApiType,
    is_stream: bool,
    _original_request_value: &Value,
    original_headers: &HeaderMap,
    query_params: &HashMap<String, String>,
    request_patches: &[crate::service::cache::types::CacheResolvedRequestPatch],
    provider_credentials: &super::prepare::ProviderCredentials,
) -> Result<MaterializedAttemptRequest, ProxyError> {
    let target_api_type = candidate.llm_api_type;
    data = transform_request_data(data, user_api_type, target_api_type, is_stream);
    let prepared_request = prepare_generation_request(
        &candidate.provider,
        &candidate.model,
        data,
        original_headers,
        request_patches,
        provider_credentials,
        target_api_type,
        is_stream,
        query_params,
    )
    .await?;
    let final_body = Bytes::from(
        serde_json::to_vec(&prepared_request.final_body_value).map_err(|err| {
            protocol_transform_error("Failed to serialize final request body", err)
        })?,
    );
    Ok(MaterializedAttemptRequest {
        final_url: prepared_request.final_url,
        final_headers: prepared_request.final_headers,
        llm_request_body_for_log: Some(LoggedBody::from_bytes(final_body.clone())),
        final_body,
        model_str: format_model_str(&candidate.provider, &candidate.model),
        response_mode: ProxyResponseMode::Generation {
            api_type: user_api_type,
            target_api_type,
        },
    })
}

async fn materialize_utility_attempt(
    candidate: &ExecutionCandidate,
    operation: &UtilityOperation,
    data: Value,
    original_headers: &HeaderMap,
    query_params: &HashMap<String, String>,
    request_patches: &[crate::service::cache::types::CacheResolvedRequestPatch],
    provider_credentials: &super::prepare::ProviderCredentials,
) -> Result<MaterializedAttemptRequest, ProxyError> {
    let (final_url, final_headers, final_body_value, provider_api_key_id) = match operation.protocol
    {
        UtilityProtocol::OpenaiCompatible => {
            prepare_llm_request(
                &candidate.provider,
                &candidate.model,
                data,
                original_headers,
                request_patches,
                provider_credentials,
                &operation.downstream_path,
            )
            .await?
        }
        UtilityProtocol::GeminiCompatible => {
            prepare_simple_gemini_request(
                &candidate.provider,
                &candidate.model,
                data,
                original_headers,
                request_patches,
                provider_credentials,
                &operation.downstream_path,
                query_params,
            )
            .await?
        }
    };
    debug_assert_eq!(provider_api_key_id, provider_credentials.key_id);
    let final_body =
        Bytes::from(serde_json::to_vec(&final_body_value).map_err(|err| {
            protocol_transform_error("Failed to serialize final request body", err)
        })?);

    Ok(MaterializedAttemptRequest {
        final_url,
        final_headers,
        llm_request_body_for_log: Some(LoggedBody::from_bytes(final_body.clone())),
        final_body,
        model_str: format_model_str(&candidate.provider, &candidate.model),
        response_mode: ProxyResponseMode::Utility {
            api_type: operation.api_type,
        },
    })
}

pub(super) async fn execute_attempt(
    app_state: Arc<AppState>,
    input: AttemptExecutionInput,
) -> AttemptExecutionResult {
    let AttemptExecutionInput {
        cancellation,
        system_api_key,
        candidate,
        requested_model_name,
        resolved_name_scope,
        resolved_route_id,
        resolved_route_name,
        query_params,
        original_headers,
        original_request_body,
        client_ip_addr,
        start_time,
        skipped_attempts,
        same_candidate_retry_count,
        attempted_candidate_count,
        next_candidate_available,
        log_mode,
        kind,
    } = input;

    let mut attempt = RequestAttemptDraft::pending_for_candidate(&candidate);
    let skipped_attempts_for_log = skipped_attempts.clone();
    info!(
        "Executing candidate {} for request model '{}'",
        candidate.candidate_position, requested_model_name
    );

    let user_api_type_for_log = match &kind {
        AttemptExecutionKind::Generation { user_api_type, .. } => *user_api_type,
        AttemptExecutionKind::Utility { operation, .. } => operation.api_type,
    };
    let mut log_context = RequestLogContext::new(
        &system_api_key,
        &candidate.provider,
        &candidate.model,
        None,
        &requested_model_name,
        &resolved_name_scope,
        resolved_route_id,
        resolved_route_name.as_deref(),
        start_time,
        &client_ip_addr,
        user_api_type_for_log,
        candidate.llm_api_type,
    );
    log_context.user_request_body = Some(LoggedBody::from_bytes(original_request_body));
    log_context.set_attempts_for_logging(&skipped_attempts_for_log, Some(attempt.clone()));

    let provider_credentials =
        match resolve_provider_credentials(&candidate.provider, &app_state).await {
            Ok(credentials) => credentials,
            Err(proxy_error) => {
                warn!(
                    "Failed to resolve provider credentials for provider {}: {:?}",
                    candidate.provider.id, proxy_error
                );
                attempt.completed_at = Some(Utc::now().timestamp_millis());
                classify_attempt_failure(
                    &mut attempt,
                    &proxy_error,
                    same_candidate_retry_count,
                    attempted_candidate_count,
                    next_candidate_available,
                    None,
                );
                let log_context = maybe_record_attempt_failure(
                    &app_state,
                    log_context,
                    &skipped_attempts_for_log,
                    &attempt,
                    &proxy_error,
                    log_mode,
                )
                .await;
                return AttemptExecutionResult {
                    attempt,
                    response: Err(proxy_error),
                    log_context,
                };
            }
        };
    attempt.provider_api_key_id = Some(provider_credentials.key_id);
    log_context.provider_api_key_id = Some(provider_credentials.key_id);
    log_context.set_attempts_for_logging(&skipped_attempts_for_log, Some(attempt.clone()));

    if let AttemptExecutionKind::Utility { operation, .. } = &kind {
        if let Err(proxy_error) = validate_utility_target(operation, candidate.llm_api_type) {
            attempt.completed_at = Some(Utc::now().timestamp_millis());
            classify_attempt_failure(
                &mut attempt,
                &proxy_error,
                same_candidate_retry_count,
                attempted_candidate_count,
                next_candidate_available,
                None,
            );
            let log_context = maybe_record_attempt_failure(
                &app_state,
                log_context,
                &skipped_attempts_for_log,
                &attempt,
                &proxy_error,
                log_mode,
            )
            .await;
            return AttemptExecutionResult {
                attempt,
                response: Err(proxy_error),
                log_context,
            };
        }
    }

    if let Err(proxy_error) = check_access_control(
        &system_api_key,
        &candidate.provider,
        &candidate.model,
        &app_state,
    )
    .await
    {
        warn!("Access control check failed: {:?}", proxy_error);
        attempt.completed_at = Some(Utc::now().timestamp_millis());
        classify_attempt_failure(
            &mut attempt,
            &proxy_error,
            same_candidate_retry_count,
            attempted_candidate_count,
            next_candidate_available,
            None,
        );
        let log_context = maybe_record_attempt_failure(
            &app_state,
            log_context,
            &skipped_attempts_for_log,
            &attempt,
            &proxy_error,
            log_mode,
        )
        .await;
        return AttemptExecutionResult {
            attempt,
            response: Err(proxy_error),
            log_context,
        };
    }

    let request_patch_trace = match load_runtime_request_patch_trace(
        &candidate.provider,
        Some(&candidate.model),
        &app_state,
    )
    .await
    {
        Ok(trace) => trace,
        Err(proxy_error) => {
            warn!(
                "Failed to load request patch trace for model {}: {:?}",
                candidate.model.id, proxy_error
            );
            attempt.completed_at = Some(Utc::now().timestamp_millis());
            classify_attempt_failure(
                &mut attempt,
                &proxy_error,
                same_candidate_retry_count,
                attempted_candidate_count,
                next_candidate_available,
                None,
            );
            let log_context = maybe_record_attempt_failure(
                &app_state,
                log_context,
                &skipped_attempts_for_log,
                &attempt,
                &proxy_error,
                log_mode,
            )
            .await;
            return AttemptExecutionResult {
                attempt,
                response: Err(proxy_error),
                log_context,
            };
        }
    };
    attempt.applied_request_patch_ids_json =
        request_patch_trace.applied_request_patch_ids_json.clone();
    attempt.request_patch_summary_json = request_patch_trace.request_patch_summary_json.clone();

    if let Some(proxy_error) = request_patch_trace.conflict_error(&candidate.model.model_name) {
        attempt.completed_at = Some(Utc::now().timestamp_millis());
        classify_attempt_failure(
            &mut attempt,
            &proxy_error,
            same_candidate_retry_count,
            attempted_candidate_count,
            next_candidate_available,
            None,
        );
        let log_context = maybe_record_attempt_failure(
            &app_state,
            log_context,
            &skipped_attempts_for_log,
            &attempt,
            &proxy_error,
            log_mode,
        )
        .await;
        return AttemptExecutionResult {
            attempt,
            response: Err(proxy_error),
            log_context,
        };
    }

    let cost_catalog_version = get_cost_catalog_version(&candidate.model, &app_state).await;
    let materialized = match kind {
        AttemptExecutionKind::Generation {
            user_api_type,
            is_stream,
            data,
            original_request_value,
        } => {
            match materialize_generation_attempt(
                &candidate,
                data,
                user_api_type,
                is_stream,
                &original_request_value,
                &original_headers,
                &query_params,
                &request_patch_trace.applied_rules,
                &provider_credentials,
            )
            .await
            {
                Ok(materialized) => materialized,
                Err(proxy_error) => {
                    error!(
                        "Failed to prepare generation request for target {:?}: {:?}",
                        candidate.llm_api_type, proxy_error
                    );
                    attempt.completed_at = Some(Utc::now().timestamp_millis());
                    classify_attempt_failure(
                        &mut attempt,
                        &proxy_error,
                        same_candidate_retry_count,
                        attempted_candidate_count,
                        next_candidate_available,
                        None,
                    );
                    let log_context = maybe_record_attempt_failure(
                        &app_state,
                        log_context,
                        &skipped_attempts_for_log,
                        &attempt,
                        &proxy_error,
                        log_mode,
                    )
                    .await;
                    return AttemptExecutionResult {
                        attempt,
                        response: Err(proxy_error),
                        log_context,
                    };
                }
            }
        }
        AttemptExecutionKind::Utility { operation, data } => match materialize_utility_attempt(
            &candidate,
            &operation,
            data,
            &original_headers,
            &query_params,
            &request_patch_trace.applied_rules,
            &provider_credentials,
        )
        .await
        {
            Ok(materialized) => materialized,
            Err(proxy_error) => {
                error!(
                    "Failed to prepare utility request '{}': {:?}",
                    operation.name, proxy_error
                );
                attempt.completed_at = Some(Utc::now().timestamp_millis());
                classify_attempt_failure(
                    &mut attempt,
                    &proxy_error,
                    same_candidate_retry_count,
                    attempted_candidate_count,
                    next_candidate_available,
                    None,
                );
                let log_context = maybe_record_attempt_failure(
                    &app_state,
                    log_context,
                    &skipped_attempts_for_log,
                    &attempt,
                    &proxy_error,
                    log_mode,
                )
                .await;
                return AttemptExecutionResult {
                    attempt,
                    response: Err(proxy_error),
                    log_context,
                };
            }
        },
    };

    attempt.llm_request_body_for_log = materialized.llm_request_body_for_log.clone();
    log_context.llm_request_body = materialized.llm_request_body_for_log;
    attempt.request_uri = Some(materialized.final_url.clone());
    attempt.request_headers_json =
        serialize_downstream_request_headers_for_log(&materialized.final_headers);
    attempt.started_at = Some(Utc::now().timestamp_millis());
    sync_attempt_timing_and_usage(&mut attempt, &log_context, cost_catalog_version.as_ref());
    log_context.set_attempts_for_logging(&skipped_attempts_for_log, Some(attempt.clone()));

    let api_key_concurrency_guard = match admit_api_key_request(&app_state, &system_api_key).await {
        Ok(guard) => guard,
        Err(proxy_error) => {
            warn!("API key request admission failed: {:?}", proxy_error);
            attempt.completed_at = Some(Utc::now().timestamp_millis());
            classify_attempt_failure(
                &mut attempt,
                &proxy_error,
                same_candidate_retry_count,
                attempted_candidate_count,
                next_candidate_available,
                None,
            );
            let log_context = maybe_record_attempt_failure(
                &app_state,
                log_context,
                &skipped_attempts_for_log,
                &attempt,
                &proxy_error,
                log_mode,
            )
            .await;
            return AttemptExecutionResult {
                attempt,
                response: Err(proxy_error),
                log_context,
            };
        }
    };

    if let Err(rejection) = ensure_provider_request_allowed(
        &app_state,
        candidate.provider.id,
        materialized.model_str.as_str(),
    )
    .await
    {
        let completed_at = Utc::now().timestamp_millis();
        warn!(
            "Provider governance skipped candidate {} for request model '{}': {}",
            candidate.candidate_position,
            requested_model_name,
            rejection.error_code()
        );
        attempt.completed_at = Some(completed_at);
        attempt.started_at = None;
        attempt.provider_api_key_id = None;
        attempt.request_uri = None;
        attempt.request_headers_json = None;
        attempt.llm_request_body_for_log = None;
        log_context.provider_api_key_id = None;
        log_context.request_url = None;
        log_context.llm_request_sent_at = None;
        log_context.llm_status = None;
        log_context.llm_request_body = None;
        log_context.llm_response_body = None;
        log_context.user_response_body = None;
        log_context.first_chunk_ts = None;
        classify_provider_governance_skip(
            &mut attempt,
            rejection,
            materialized.model_str.as_str(),
            attempted_candidate_count,
            next_candidate_available,
        );
        let proxy_error = rejection.to_proxy_error(materialized.model_str.as_str());
        let log_context = maybe_record_attempt_failure(
            &app_state,
            log_context,
            &skipped_attempts_for_log,
            &attempt,
            &proxy_error,
            log_mode,
        )
        .await;
        return AttemptExecutionResult {
            attempt,
            response: Err(proxy_error),
            log_context,
        };
    }

    let proxy_result = proxy_request(
        Arc::clone(&app_state),
        cancellation,
        log_context,
        materialized.final_url,
        materialized.final_body,
        materialized.final_headers,
        materialized.model_str,
        candidate.provider.use_proxy,
        cost_catalog_version.clone(),
        api_key_concurrency_guard,
        materialized.response_mode,
        log_mode.proxy_log_mode(),
    )
    .await;
    let completed_at = Utc::now().timestamp_millis();
    let (response, mut log_context) = match proxy_result {
        Ok(outcome) => {
            attempt.llm_response_body_for_log = outcome.log_context.llm_response_body.clone();
            sync_attempt_timing_and_usage(
                &mut attempt,
                &outcome.log_context,
                cost_catalog_version.as_ref(),
            );
            complete_attempt_from_response(&mut attempt, &outcome.response, completed_at);
            (Ok(outcome.response), outcome.log_context)
        }
        Err(failure) => {
            let retry_after = retry_after_from_headers(failure.response_headers.as_ref());
            attempt.llm_response_body_for_log = failure.log_context.llm_response_body.clone();
            sync_attempt_from_proxy_failure(&mut attempt, &failure, cost_catalog_version.as_ref());
            attempt.completed_at = Some(completed_at);
            classify_attempt_failure(
                &mut attempt,
                &failure.error,
                same_candidate_retry_count,
                attempted_candidate_count,
                next_candidate_available,
                retry_after,
            );
            let mut log_context = failure.log_context;
            if !log_mode.should_record_attempt_failure() {
                log_context = finalize_attempt_failure_context(
                    log_context,
                    &skipped_attempts_for_log,
                    &attempt,
                    &failure.error,
                );
            }
            (Err(failure.error), log_context)
        }
    };
    log_context.set_attempts_for_logging(&skipped_attempts_for_log, Some(attempt.clone()));

    AttemptExecutionResult {
        attempt,
        response,
        log_context,
    }
}

pub(super) struct GenerationOrchestrationInput {
    pub cancellation: ProxyCancellationContext,
    pub system_api_key: Arc<CacheApiKey>,
    pub api_type: LlmApiType,
    pub execution_plan: ExecutionPlan,
    pub is_stream: bool,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub data: Value,
    pub original_request_value: Value,
    pub original_request_body: Bytes,
}

pub(super) struct UtilityOrchestrationInput {
    pub cancellation: ProxyCancellationContext,
    pub system_api_key: Arc<CacheApiKey>,
    pub operation: UtilityOperation,
    pub execution_plan: ExecutionPlan,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub data: Value,
    pub original_request_body: Bytes,
}

async fn record_no_candidate_generation_failure(
    app_state: &Arc<AppState>,
    system_api_key: &CacheApiKey,
    execution_plan: &ExecutionPlan,
    skipped_attempts: Vec<RequestAttemptDraft>,
    api_type: LlmApiType,
    original_request_body: Bytes,
    start_time: i64,
    client_ip_addr: &Option<String>,
    message: &str,
) {
    let Some(first_skipped_attempt) = skipped_attempts.first() else {
        return;
    };

    let mut log_context = RequestLogContext::new_for_skipped_candidates(
        system_api_key,
        &execution_plan.requested_name,
        execution_plan.resolved_scope.as_str(),
        execution_plan.resolved_route_id,
        execution_plan.resolved_route_name.as_deref(),
        start_time,
        client_ip_addr,
        api_type,
        first_skipped_attempt,
    );
    log_context.user_request_body = Some(LoggedBody::from_bytes(original_request_body));
    log_context.completion_ts = Some(Utc::now().timestamp_millis());
    log_context.overall_status = RequestStatus::Error;
    log_context.final_error_code = Some(NO_CANDIDATE_AVAILABLE_ERROR.to_string());
    log_context.final_error_message = Some(message.to_string());
    log_context.set_attempts_for_logging(&skipped_attempts, None);
    record_request_completion_and_log(app_state, log_context).await;
}

async fn record_no_candidate_utility_failure(
    app_state: &Arc<AppState>,
    system_api_key: &CacheApiKey,
    execution_plan: &ExecutionPlan,
    skipped_attempts: Vec<RequestAttemptDraft>,
    operation: &UtilityOperation,
    original_request_body: Bytes,
    start_time: i64,
    client_ip_addr: &Option<String>,
    message: &str,
) {
    let Some(first_skipped_attempt) = skipped_attempts.first() else {
        return;
    };

    let mut log_context = RequestLogContext::new_for_skipped_candidates(
        system_api_key,
        &execution_plan.requested_name,
        execution_plan.resolved_scope.as_str(),
        execution_plan.resolved_route_id,
        execution_plan.resolved_route_name.as_deref(),
        start_time,
        client_ip_addr,
        operation.api_type,
        first_skipped_attempt,
    );
    log_context.user_request_body = Some(LoggedBody::from_bytes(original_request_body));
    log_context.completion_ts = Some(Utc::now().timestamp_millis());
    log_context.overall_status = RequestStatus::Error;
    log_context.final_error_code = Some(NO_CANDIDATE_AVAILABLE_ERROR.to_string());
    log_context.final_error_message = Some(message.to_string());
    log_context.set_attempts_for_logging(&skipped_attempts, None);
    record_request_completion_and_log(app_state, log_context).await;
}

pub(super) async fn orchestrate_generation(
    app_state: Arc<AppState>,
    input: GenerationOrchestrationInput,
) -> Result<Response<Body>, ProxyError> {
    let GenerationOrchestrationInput {
        cancellation,
        system_api_key,
        api_type,
        execution_plan,
        is_stream,
        query_params,
        original_headers,
        client_ip_addr,
        start_time,
        data,
        original_request_value,
        original_request_body,
    } = input;

    let requirement = derive_generation_requirement(&data, api_type, is_stream);
    let prefiltered_plan = prefilter_execution_plan(execution_plan, &requirement);
    let execution_plan = prefiltered_plan.execution_plan;
    let mut prior_attempts = prefiltered_plan.skipped_attempts;

    if execution_plan.candidates.is_empty() {
        let message = no_candidate_error_message(&requirement);
        record_no_candidate_generation_failure(
            &app_state,
            &system_api_key,
            &execution_plan,
            prior_attempts,
            api_type,
            original_request_body,
            start_time,
            &client_ip_addr,
            &message,
        )
        .await;
        return Err(ProxyError::BadRequest(message));
    }

    let requested_model_name = execution_plan.requested_name.clone();
    let resolved_name_scope = execution_plan.resolved_scope.as_str().to_string();
    let resolved_route_id = execution_plan.resolved_route_id;
    let resolved_route_name = execution_plan.resolved_route_name.clone();
    let candidate_budget = CONFIG.routing_resilience.max_candidates_per_request.max(1) as usize;
    let mut candidate_index = 0usize;

    while candidate_index < execution_plan.candidates.len() && candidate_index < candidate_budget {
        let candidate = execution_plan.candidates[candidate_index].clone();
        let attempted_candidate_count = (candidate_index + 1) as u32;
        let mut same_candidate_retry_count = 0u32;

        loop {
            let next_candidate_available = candidate_index + 1 < execution_plan.candidates.len()
                && candidate_index + 1 < candidate_budget;
            info!(
                "Orchestrating generation request model '{}' via {} candidate {} retry {}",
                requested_model_name,
                resolved_name_scope,
                candidate.candidate_position,
                same_candidate_retry_count
            );

            let result = execute_attempt(
                Arc::clone(&app_state),
                AttemptExecutionInput {
                    cancellation: cancellation.clone(),
                    system_api_key: Arc::clone(&system_api_key),
                    candidate: candidate.clone(),
                    requested_model_name: requested_model_name.clone(),
                    resolved_name_scope: resolved_name_scope.clone(),
                    resolved_route_id,
                    resolved_route_name: resolved_route_name.clone(),
                    query_params: query_params.clone(),
                    original_headers: original_headers.clone(),
                    original_request_body: original_request_body.clone(),
                    client_ip_addr: client_ip_addr.clone(),
                    start_time,
                    skipped_attempts: prior_attempts.clone(),
                    same_candidate_retry_count,
                    attempted_candidate_count,
                    next_candidate_available,
                    log_mode: AttemptLogMode::DeferNonStreaming,
                    kind: AttemptExecutionKind::Generation {
                        user_api_type: api_type,
                        is_stream,
                        data: data.clone(),
                        original_request_value: original_request_value.clone(),
                    },
                },
            )
            .await;

            let AttemptExecutionResult {
                attempt,
                response,
                log_context,
            } = result;

            match response {
                Ok(response) => {
                    if !log_context.is_stream {
                        record_request_completion_and_log(&app_state, log_context).await;
                    }
                    return Ok(response);
                }
                Err(error) => match attempt.scheduler_action {
                    SchedulerAction::RetrySameCandidate => {
                        let backoff_ms = attempt.backoff_ms.unwrap_or_default().max(0) as u64;
                        prior_attempts.push(attempt);
                        if backoff_ms > 0 {
                            sleep(Duration::from_millis(backoff_ms)).await;
                        }
                        same_candidate_retry_count = same_candidate_retry_count.saturating_add(1);
                    }
                    SchedulerAction::FallbackNextCandidate
                        if candidate_index + 1 < execution_plan.candidates.len()
                            && candidate_index + 1 < candidate_budget =>
                    {
                        prior_attempts.push(attempt);
                        candidate_index += 1;
                        break;
                    }
                    _ => {
                        record_request_completion_and_log(&app_state, log_context).await;
                        return Err(error);
                    }
                },
            }
        }
    }

    let message = no_candidate_error_message(&requirement);
    Err(ProxyError::BadRequest(message))
}

pub(super) async fn orchestrate_utility(
    app_state: Arc<AppState>,
    input: UtilityOrchestrationInput,
) -> Result<Response<Body>, ProxyError> {
    let UtilityOrchestrationInput {
        cancellation,
        system_api_key,
        operation,
        execution_plan,
        query_params,
        original_headers,
        client_ip_addr,
        start_time,
        data,
        original_request_body,
    } = input;

    let requirement = derive_utility_requirement(&operation.name, &data);
    let prefiltered_plan = prefilter_execution_plan(execution_plan, &requirement);
    let execution_plan = prefiltered_plan.execution_plan;
    let mut prior_attempts = prefiltered_plan.skipped_attempts;

    if execution_plan.candidates.is_empty() {
        let message = no_candidate_error_message(&requirement);
        record_no_candidate_utility_failure(
            &app_state,
            &system_api_key,
            &execution_plan,
            prior_attempts,
            &operation,
            original_request_body,
            start_time,
            &client_ip_addr,
            &message,
        )
        .await;
        return Err(ProxyError::BadRequest(message));
    }

    let requested_model_name = execution_plan.requested_name.clone();
    let resolved_name_scope = execution_plan.resolved_scope.as_str().to_string();
    let resolved_route_id = execution_plan.resolved_route_id;
    let resolved_route_name = execution_plan.resolved_route_name.clone();
    let candidate_budget = CONFIG.routing_resilience.max_candidates_per_request.max(1) as usize;
    let mut candidate_index = 0usize;

    while candidate_index < execution_plan.candidates.len() && candidate_index < candidate_budget {
        let candidate = execution_plan.candidates[candidate_index].clone();
        let attempted_candidate_count = (candidate_index + 1) as u32;
        let mut same_candidate_retry_count = 0u32;

        loop {
            let next_candidate_available = candidate_index + 1 < execution_plan.candidates.len()
                && candidate_index + 1 < candidate_budget;
            info!(
                "Orchestrating utility request '{}' model '{}' via {} candidate {} retry {}",
                operation.name,
                requested_model_name,
                resolved_name_scope,
                candidate.candidate_position,
                same_candidate_retry_count
            );

            let result = execute_attempt(
                Arc::clone(&app_state),
                AttemptExecutionInput {
                    cancellation: cancellation.clone(),
                    system_api_key: Arc::clone(&system_api_key),
                    candidate: candidate.clone(),
                    requested_model_name: requested_model_name.clone(),
                    resolved_name_scope: resolved_name_scope.clone(),
                    resolved_route_id,
                    resolved_route_name: resolved_route_name.clone(),
                    query_params: query_params.clone(),
                    original_headers: original_headers.clone(),
                    original_request_body: original_request_body.clone(),
                    client_ip_addr: client_ip_addr.clone(),
                    start_time,
                    skipped_attempts: prior_attempts.clone(),
                    same_candidate_retry_count,
                    attempted_candidate_count,
                    next_candidate_available,
                    log_mode: AttemptLogMode::DeferNonStreaming,
                    kind: AttemptExecutionKind::Utility {
                        operation: operation.clone(),
                        data: data.clone(),
                    },
                },
            )
            .await;

            let AttemptExecutionResult {
                attempt,
                response,
                log_context,
            } = result;

            match response {
                Ok(response) => {
                    if !log_context.is_stream {
                        record_request_completion_and_log(&app_state, log_context).await;
                    }
                    return Ok(response);
                }
                Err(error) => match attempt.scheduler_action {
                    SchedulerAction::RetrySameCandidate => {
                        let backoff_ms = attempt.backoff_ms.unwrap_or_default().max(0) as u64;
                        prior_attempts.push(attempt);
                        if backoff_ms > 0 {
                            sleep(Duration::from_millis(backoff_ms)).await;
                        }
                        same_candidate_retry_count = same_candidate_retry_count.saturating_add(1);
                    }
                    SchedulerAction::FallbackNextCandidate
                        if candidate_index + 1 < execution_plan.candidates.len()
                            && candidate_index + 1 < candidate_budget =>
                    {
                        prior_attempts.push(attempt);
                        candidate_index += 1;
                        break;
                    }
                    _ => {
                        record_request_completion_and_log(&app_state, log_context).await;
                        return Err(error);
                    }
                },
            }
        }
    }

    let message = no_candidate_error_message(&requirement);
    Err(ProxyError::BadRequest(message))
}

#[derive(Debug, Clone)]
pub(super) struct PrefilteredExecutionPlan {
    pub execution_plan: ExecutionPlan,
    pub skipped_attempts: Vec<RequestAttemptDraft>,
}

pub(super) fn derive_generation_requirement(
    data: &Value,
    _user_api_type: LlmApiType,
    is_stream: bool,
) -> ExecutionRequirement {
    ExecutionRequirement {
        requires_streaming: is_stream,
        requires_tools: request_uses_tools(data),
        requires_reasoning: request_uses_reasoning(data),
        requires_image_input: request_uses_image_input(data),
        requires_embeddings: false,
        requires_rerank: false,
    }
}

pub(super) fn derive_utility_requirement(
    operation_name: &str,
    data: &Value,
) -> ExecutionRequirement {
    let normalized_name = operation_name.to_ascii_lowercase();
    ExecutionRequirement {
        requires_streaming: false,
        requires_tools: false,
        requires_reasoning: false,
        requires_image_input: request_uses_image_input(data),
        requires_embeddings: normalized_name == "embeddings",
        requires_rerank: normalized_name == "rerank",
    }
}

pub(super) fn prefilter_execution_plan(
    execution_plan: ExecutionPlan,
    requirement: &ExecutionRequirement,
) -> PrefilteredExecutionPlan {
    let mut compatible_candidates = Vec::with_capacity(execution_plan.candidates.len());
    let mut skipped_attempts = Vec::new();

    for candidate in execution_plan.candidates {
        let missing_capabilities = missing_capabilities(&candidate, requirement);
        if missing_capabilities.is_empty() {
            compatible_candidates.push(candidate);
        } else {
            skipped_attempts.push(RequestAttemptDraft::skipped_for_capability_mismatch(
                &candidate,
                &missing_capabilities,
            ));
        }
    }

    PrefilteredExecutionPlan {
        execution_plan: ExecutionPlan {
            requested_name: execution_plan.requested_name,
            resolved_scope: execution_plan.resolved_scope,
            resolved_route_id: execution_plan.resolved_route_id,
            resolved_route_name: execution_plan.resolved_route_name,
            candidates: compatible_candidates,
        },
        skipped_attempts,
    }
}

pub(super) fn no_candidate_error_message(requirement: &ExecutionRequirement) -> String {
    let required = requirement.required_capability_names();
    if required.is_empty() {
        "No execution candidate is available for this request.".to_string()
    } else {
        format!(
            "No execution candidate supports the required capabilities: {}",
            required.join(", ")
        )
    }
}

fn missing_capabilities(
    candidate: &ExecutionCandidate,
    requirement: &ExecutionRequirement,
) -> Vec<&'static str> {
    let model = candidate.model.as_ref();
    [
        (
            requirement.requires_streaming && !model.supports_streaming,
            "streaming",
        ),
        (requirement.requires_tools && !model.supports_tools, "tools"),
        (
            requirement.requires_reasoning && !model.supports_reasoning,
            "reasoning",
        ),
        (
            requirement.requires_image_input && !model.supports_image_input,
            "image_input",
        ),
        (
            requirement.requires_embeddings && !model.supports_embeddings,
            "embeddings",
        ),
        (
            requirement.requires_rerank && !model.supports_rerank,
            "rerank",
        ),
    ]
    .into_iter()
    .filter_map(|(missing, name)| missing.then_some(name))
    .collect()
}

fn request_uses_tools(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            let key = key.as_str();
            if matches!(key, "tools" | "functions") {
                return match value {
                    Value::Array(items) => !items.is_empty(),
                    Value::Object(items) => !items.is_empty(),
                    Value::Null => false,
                    _ => true,
                };
            }
            if key == "tool_choice" || key == "function_call" {
                return !matches!(value, Value::Null)
                    && value.as_str().map_or(true, |choice| choice != "none");
            }
            request_uses_tools(value)
        }),
        Value::Array(items) => items.iter().any(request_uses_tools),
        _ => false,
    }
}

fn request_uses_reasoning(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            if matches!(
                key.as_str(),
                "reasoning"
                    | "reasoning_effort"
                    | "thinking"
                    | "thinking_config"
                    | "thinkingConfig"
                    | "include_reasoning"
                    | "includeReasoning"
            ) {
                return !matches!(value, Value::Null | Value::Bool(false));
            }
            request_uses_reasoning(value)
        }),
        Value::Array(items) => items.iter().any(request_uses_reasoning),
        _ => false,
    }
}

fn request_uses_image_input(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map
                .get("type")
                .and_then(Value::as_str)
                .map_or(false, |kind| {
                    matches!(kind, "image" | "image_url" | "input_image")
                })
            {
                return true;
            }

            if map.contains_key("image_url") {
                return true;
            }

            if map.iter().any(|(key, value)| {
                matches!(key.as_str(), "mime_type" | "mimeType")
                    && value
                        .as_str()
                        .map_or(false, |mime_type| mime_type.starts_with("image/"))
            }) {
                return true;
            }

            map.values().any(request_uses_image_input)
        }
        Value::Array(items) => items.iter().any(request_uses_image_input),
        _ => false,
    }
}

fn truncate_error_message(message: &str) -> String {
    const MAX_ERROR_MESSAGE_CHARS: usize = 512;
    message.chars().take(MAX_ERROR_MESSAGE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use axum::http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    };
    use serde_json::json;

    use super::*;
    use crate::{
        proxy::prepare::{
            ExecutionCandidate, ExecutionPlan, ProviderCredentials, ResolvedNameScope,
        },
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

    fn model(id: i64, supports_tools: bool, supports_image_input: bool) -> Arc<CacheModel> {
        Arc::new(CacheModel {
            id,
            provider_id: id,
            model_name: format!("model-{id}"),
            real_model_name: None,
            cost_catalog_id: None,
            supports_streaming: true,
            supports_tools,
            supports_reasoning: true,
            supports_image_input,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled: true,
        })
    }

    fn candidate(
        position: usize,
        supports_tools: bool,
        supports_image_input: bool,
    ) -> ExecutionCandidate {
        ExecutionCandidate {
            candidate_position: position,
            route_id: Some(1),
            route_name: Some("route".to_string()),
            route_candidate_priority: Some(position as i32),
            provider: provider(position as i64),
            model: model(position as i64, supports_tools, supports_image_input),
            llm_api_type: LlmApiType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        }
    }

    fn plan() -> ExecutionPlan {
        ExecutionPlan {
            requested_name: "route".to_string(),
            resolved_scope: ResolvedNameScope::GlobalRoute,
            resolved_route_id: Some(1),
            resolved_route_name: Some("route".to_string()),
            candidates: vec![candidate(1, false, true), candidate(2, true, true)],
        }
    }

    #[test]
    fn derive_generation_requirement_detects_tools_reasoning_images_and_streaming() {
        let requirement = derive_generation_requirement(
            &json!({
                "stream": true,
                "tools": [{"type": "function"}],
                "reasoning_effort": "medium",
                "messages": [{
                    "role": "user",
                    "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,abc"}}]
                }]
            }),
            LlmApiType::Openai,
            true,
        );

        assert!(requirement.requires_streaming);
        assert!(requirement.requires_tools);
        assert!(requirement.requires_reasoning);
        assert!(requirement.requires_image_input);
        assert!(!requirement.requires_embeddings);
        assert!(!requirement.requires_rerank);
    }

    #[test]
    fn derive_utility_requirement_detects_embeddings_and_rerank() {
        let embeddings = derive_utility_requirement("embeddings", &json!({ "input": "hello" }));
        let rerank = derive_utility_requirement("rerank", &json!({ "query": "hello" }));

        assert!(embeddings.requires_embeddings);
        assert!(!embeddings.requires_rerank);
        assert!(rerank.requires_rerank);
    }

    #[test]
    fn prefilter_execution_plan_skips_incompatible_candidates_without_reordering() {
        let requirement = ExecutionRequirement {
            requires_tools: true,
            ..ExecutionRequirement::default()
        };

        let prefiltered = prefilter_execution_plan(plan(), &requirement);

        assert_eq!(prefiltered.execution_plan.candidate_model_ids(), vec![2]);
        assert_eq!(prefiltered.skipped_attempts.len(), 1);
        assert_eq!(prefiltered.skipped_attempts[0].candidate_position, 1);
        assert_eq!(
            prefiltered.skipped_attempts[0].error_code.as_deref(),
            Some(CAPABILITY_MISMATCH_SKIPPED_ERROR)
        );
        assert_eq!(
            prefiltered.skipped_attempts[0].scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
    }

    #[test]
    fn classify_attempt_failure_retries_then_fallbacks_when_output_is_not_visible() {
        let candidate = candidate(1, true, true);
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
        let candidate = candidate(1, true, true);
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
        let candidate = candidate(1, true, true);
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
        let candidate = candidate(1, true, true);
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
        let mut streaming_attempt =
            RequestAttemptDraft::pending_for_candidate(&candidate(1, true, true));
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

        let mut error_attempt =
            RequestAttemptDraft::pending_for_candidate(&candidate(1, true, true));
        let error_response = Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .header(CONTENT_TYPE, "text/event-stream")
            .body(Body::empty())
            .unwrap();

        complete_attempt_from_response(&mut error_attempt, &error_response, 2_100);

        assert_eq!(error_attempt.scheduler_action, SchedulerAction::FailFast);
        assert!(!error_attempt.response_started_to_client);
    }

    #[tokio::test]
    async fn materialize_openai_utility_attempt_prepares_headers_uri_and_body_snapshot() {
        let candidate = candidate(1, true, true);
        let operation = UtilityOperation {
            name: "embeddings".to_string(),
            api_type: LlmApiType::Openai,
            protocol: UtilityProtocol::OpenaiCompatible,
            downstream_path: "embeddings".to_string(),
        };
        let mut original_headers = HeaderMap::new();
        original_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        original_headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer user-key"));
        let credentials = ProviderCredentials {
            key_id: 42,
            request_key: "provider-secret".to_string(),
        };

        let materialized = materialize_utility_attempt(
            &candidate,
            &operation,
            json!({ "input": "embed me" }),
            &original_headers,
            &HashMap::new(),
            &[],
            &credentials,
        )
        .await
        .unwrap();

        assert_eq!(materialized.final_url, "https://example.com/embeddings");
        assert_eq!(
            materialized
                .final_headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer provider-secret")
        );
        assert_eq!(
            materialized
                .final_headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert_eq!(body["model"], "model-1");
        assert_eq!(body["input"], "embed me");
        match materialized.llm_request_body_for_log.unwrap() {
            LoggedBody::InMemory { bytes, .. } => {
                assert_eq!(bytes, materialized.final_body);
            }
            LoggedBody::Spooled { .. } => panic!("small request body should stay in memory"),
        }
    }

    #[tokio::test]
    async fn materialize_gemini_utility_attempt_prepares_headers_uri_and_body_snapshot() {
        let mut candidate = candidate(1, true, true);
        candidate.provider = Arc::new(CacheProvider {
            endpoint: "https://example.com/v1beta/models".to_string(),
            provider_type: ProviderType::Gemini,
            ..(*candidate.provider).clone()
        });
        candidate.llm_api_type = LlmApiType::Gemini;
        let operation = UtilityOperation {
            name: "countTokens".to_string(),
            api_type: LlmApiType::Gemini,
            protocol: UtilityProtocol::GeminiCompatible,
            downstream_path: "countTokens".to_string(),
        };
        let mut original_headers = HeaderMap::new();
        original_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        original_headers.insert("x-goog-api-key", HeaderValue::from_static("user-key"));
        let query_params = HashMap::from([
            ("foo".to_string(), "bar".to_string()),
            ("key".to_string(), "user-key".to_string()),
        ]);
        let credentials = ProviderCredentials {
            key_id: 42,
            request_key: "provider-secret".to_string(),
        };

        let materialized = materialize_utility_attempt(
            &candidate,
            &operation,
            json!({ "contents": [{ "parts": [{ "text": "count this" }] }] }),
            &original_headers,
            &query_params,
            &[],
            &credentials,
        )
        .await
        .unwrap();

        assert_eq!(
            materialized.final_url,
            "https://example.com/v1beta/models/model-1:countTokens?foo=bar"
        );
        assert_eq!(
            materialized
                .final_headers
                .get("x-goog-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("provider-secret")
        );
        assert_eq!(
            materialized
                .final_headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let body: serde_json::Value = serde_json::from_slice(&materialized.final_body).unwrap();
        assert_eq!(body["contents"][0]["parts"][0]["text"], "count this");
        match materialized.llm_request_body_for_log.unwrap() {
            LoggedBody::InMemory { bytes, .. } => {
                assert_eq!(bytes, materialized.final_body);
            }
            LoggedBody::Spooled { .. } => panic!("small request body should stay in memory"),
        }
    }
}
