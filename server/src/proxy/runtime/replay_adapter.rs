use std::{collections::HashMap, sync::Arc};

use axum::{
    body::{Body, Bytes},
    http::HeaderMap,
    response::Response,
};
use chrono::Utc;
use serde_json::Value;

use crate::{
    config::RoutingResilienceConfig,
    cost::UsageNormalization,
    proxy::{
        ProxyError,
        auth::check_access_control,
        cancellation::ProxyCancellationContext,
        logging::RequestLogContext,
        provider_governance::{ProviderGovernanceCheckError, preview_provider_request_allowed},
        runtime::{
            attempt::{
                RequestAttemptDraft, classify_attempt_failure, classify_provider_governance_skip,
            },
            candidate_filter::{
                RequestedOperationKind, derive_generation_requirement, derive_utility_requirement,
                ensure_reasoning_preset_allows_operation, no_candidate_error_message,
                prefilter_execution_plan,
            },
            credential::resolve_provider_credentials,
            executor::AttemptExecutionKind,
            log_writer::build_candidate_manifest,
            materializer::{materialize_generation_attempt, materialize_utility_attempt},
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
            request_patch::load_runtime_request_patch_trace,
            route_resolver::{ExecutionCandidate, ExecutionPlan, build_execution_plan},
            scheduler::{SchedulerExecutionFailure, SchedulerExecutionInput, schedule_execution},
        },
        util::serialize_downstream_request_headers_for_log,
        utility::{UtilityOperation, validate_utility_target},
    },
    schema::enum_def::{LlmApiType, RequestAttemptStatus, SchedulerAction},
    service::{
        app_state::AppState, cache::types::CacheApiKey,
        transform::unified::UnifiedTransformDiagnostic,
    },
    utils::storage::{
        RequestLogBundleCandidateManifest, RequestLogBundleQueryParam,
        RequestLogBundleRequestSnapshot,
    },
};

#[derive(Debug, Clone)]
pub(crate) enum GatewayReplayAttemptKind {
    Generation {
        api_type: LlmApiType,
        is_stream: bool,
        data: Value,
        original_request_value: Value,
    },
    Utility {
        operation: UtilityOperation,
        data: Value,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct GatewayReplayInput {
    pub api_key: Arc<CacheApiKey>,
    pub requested_model_name: String,
    pub query_params: Vec<RequestLogBundleQueryParam>,
    pub original_headers: HeaderMap,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub original_request_body: Bytes,
    pub kind: GatewayReplayAttemptKind,
}

#[derive(Debug, Clone)]
pub(crate) struct GatewayReplayPreparedRequest {
    pub requested_model_name: String,
    pub base_requested_model_name: String,
    pub resolved_reasoning_suffix: Option<String>,
    pub resolved_reasoning_preset: Option<String>,
    pub resolved_name_scope: String,
    pub resolved_route_id: Option<i64>,
    pub resolved_route_name: Option<String>,
    pub candidate_position: i32,
    pub provider_id: i64,
    pub provider_api_key_id: i64,
    pub model_id: i64,
    pub llm_api_type: LlmApiType,
    pub applied_request_patch_summary: Option<Value>,
    pub final_request_uri: String,
    pub final_request_headers: HeaderMap,
    pub final_request_body: Bytes,
    pub transform_diagnostics: Vec<UnifiedTransformDiagnostic>,
    pub candidate_manifest: RequestLogBundleCandidateManifest,
    pub candidate_decisions: Vec<GatewayReplayCandidateDecision>,
}

fn replay_query_params_to_map(params: &[RequestLogBundleQueryParam]) -> HashMap<String, String> {
    params
        .iter()
        .filter_map(|param| {
            param
                .value_for_replay()
                .map(|value| (param.name.clone(), value))
        })
        .collect()
}

#[derive(Debug)]
pub(crate) struct GatewayReplayExecutionSuccess {
    pub response: Response<Body>,
    pub metadata: GatewayReplayExecutionMetadata,
}

#[derive(Debug)]
pub(crate) struct GatewayReplayExecutionFailure {
    pub error: ProxyError,
    pub metadata: Option<GatewayReplayExecutionMetadata>,
    pub candidate_decisions: Vec<GatewayReplayCandidateDecision>,
}

impl GatewayReplayExecutionFailure {
    fn without_attempt(error: ProxyError) -> Self {
        Self {
            error,
            metadata: None,
            candidate_decisions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GatewayReplayExecutionMetadata {
    pub requested_model_name: String,
    pub base_requested_model_name: String,
    pub resolved_reasoning_suffix: Option<String>,
    pub resolved_reasoning_preset: Option<String>,
    pub resolved_route_id: Option<i64>,
    pub resolved_route_name: Option<String>,
    pub final_attempt: GatewayReplayFinalAttempt,
    pub candidate_decisions: Vec<GatewayReplayCandidateDecision>,
    pub transform_diagnostics: Vec<UnifiedTransformDiagnostic>,
    pub usage_normalization: Option<UsageNormalization>,
}

#[derive(Debug, Clone)]
pub(crate) struct GatewayReplayFinalAttempt {
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

#[derive(Debug, Clone)]
pub(crate) struct GatewayReplayCandidateDecision {
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

impl From<&RequestAttemptDraft> for GatewayReplayCandidateDecision {
    fn from(attempt: &RequestAttemptDraft) -> Self {
        let decision = attempt.to_runtime_candidate_decision();
        Self {
            candidate_position: decision.candidate_position,
            provider_id: decision.provider_id,
            provider_api_key_id: decision.provider_api_key_id,
            model_id: decision.model_id,
            llm_api_type: decision.llm_api_type,
            attempt_status: decision.attempt_status,
            scheduler_action: decision.scheduler_action,
            error_code: decision.error_code,
            error_message: decision.error_message,
            request_uri: decision.request_uri,
        }
    }
}

fn gateway_replay_candidate_decisions(
    prior_attempts: &[RequestAttemptDraft],
    terminal_attempt: Option<&RequestAttemptDraft>,
) -> Vec<GatewayReplayCandidateDecision> {
    prior_attempts
        .iter()
        .chain(terminal_attempt)
        .map(GatewayReplayCandidateDecision::from)
        .collect()
}

fn gateway_replay_execution_metadata(
    resolved_route_id: Option<i64>,
    resolved_route_name: Option<String>,
    prior_attempts: &[RequestAttemptDraft],
    terminal_attempt: &RequestAttemptDraft,
    log_context: &RequestLogContext,
) -> GatewayReplayExecutionMetadata {
    let candidate_decisions =
        gateway_replay_candidate_decisions(prior_attempts, Some(terminal_attempt));
    let final_attempt = terminal_attempt.to_runtime_final_attempt();
    GatewayReplayExecutionMetadata {
        requested_model_name: log_context.requested_model_name.clone(),
        base_requested_model_name: log_context.base_requested_model_name.clone(),
        resolved_reasoning_suffix: log_context.resolved_reasoning_suffix.clone(),
        resolved_reasoning_preset: log_context.resolved_reasoning_preset.clone(),
        resolved_route_id,
        resolved_route_name,
        final_attempt: GatewayReplayFinalAttempt {
            candidate_position: final_attempt.candidate_position,
            provider_id: final_attempt.provider_id,
            provider_api_key_id: final_attempt.provider_api_key_id,
            model_id: final_attempt.model_id,
            llm_api_type: final_attempt.llm_api_type,
            attempt_status: final_attempt.attempt_status,
            error_code: final_attempt.error_code,
            error_message: final_attempt.error_message,
            request_uri: final_attempt.request_uri,
            request_headers_json: final_attempt.request_headers_json,
            request_body: final_attempt.request_body,
            request_body_capture_state: final_attempt.request_body_capture_state,
            response_headers_json: final_attempt.response_headers_json,
            response_body: final_attempt.response_body,
            response_body_capture_state: final_attempt.response_body_capture_state,
            http_status: final_attempt.http_status,
            first_byte_at: final_attempt.first_byte_at,
            applied_request_patch_summary: final_attempt.applied_request_patch_summary,
            total_input_tokens: final_attempt.total_input_tokens,
            total_output_tokens: final_attempt.total_output_tokens,
            reasoning_tokens: final_attempt.reasoning_tokens,
            total_tokens: final_attempt.total_tokens,
        },
        candidate_decisions,
        transform_diagnostics: log_context
            .transform_diagnostics
            .iter()
            .map(|item| item.diagnostic.clone())
            .collect(),
        usage_normalization: log_context.usage_normalization.clone(),
    }
}

fn gateway_replay_failure_from_scheduler(
    failure: SchedulerExecutionFailure,
    resolved_route_id: Option<i64>,
    resolved_route_name: Option<String>,
) -> GatewayReplayExecutionFailure {
    let candidate_decisions = gateway_replay_candidate_decisions(
        &failure.prior_attempts,
        failure.terminal_attempt.as_ref(),
    );

    if let (Some(terminal_attempt), Some(log_context)) = (
        failure.terminal_attempt.as_ref(),
        failure.log_context.as_ref(),
    ) {
        let metadata = gateway_replay_execution_metadata(
            resolved_route_id,
            resolved_route_name,
            &failure.prior_attempts,
            terminal_attempt,
            log_context,
        );
        return GatewayReplayExecutionFailure {
            error: failure.error,
            metadata: Some(metadata),
            candidate_decisions,
        };
    }

    GatewayReplayExecutionFailure {
        error: failure.error,
        metadata: None,
        candidate_decisions,
    }
}

pub(crate) async fn preview_gateway_replay_request(
    app_state: Arc<AppState>,
    input: GatewayReplayInput,
) -> Result<GatewayReplayPreparedRequest, ProxyError> {
    debug_assert!(!RuntimeExecutionPolicy::ReplayDryRun.sends_upstream_request());
    debug_assert!(!RuntimeExecutionPolicy::ReplayDryRun.admits_api_key_requests());
    debug_assert!(RuntimeExecutionPolicy::ReplayDryRun.uses_read_only_provider_governance());
    let execution_plan =
        build_execution_plan(&app_state, input.api_key.id, &input.requested_model_name)
            .await
            .map_err(ProxyError::BadRequest)?;
    materialize_gateway_replay_request(app_state, input, execution_plan).await
}

pub(crate) async fn execute_gateway_replay_request(
    app_state: Arc<AppState>,
    input: GatewayReplayInput,
) -> Result<GatewayReplayExecutionSuccess, GatewayReplayExecutionFailure> {
    debug_assert!(RuntimeExecutionPolicy::ReplayLive.sends_upstream_request());
    debug_assert!(!RuntimeExecutionPolicy::ReplayLive.records_request_log());
    debug_assert!(!RuntimeExecutionPolicy::ReplayLive.records_provider_runtime());
    debug_assert!(!RuntimeExecutionPolicy::ReplayLive.admits_api_key_requests());
    debug_assert!(RuntimeExecutionPolicy::ReplayLive.uses_read_only_provider_governance());

    let execution_plan =
        build_execution_plan(&app_state, input.api_key.id, &input.requested_model_name)
            .await
            .map_err(|err| {
                GatewayReplayExecutionFailure::without_attempt(ProxyError::BadRequest(err))
            })?;

    let resolved_route_id = execution_plan.resolved_route_id;
    let resolved_route_name = execution_plan.resolved_route_name.clone();
    let GatewayReplayInput {
        api_key,
        requested_model_name: _,
        query_params,
        original_headers,
        request_snapshot,
        client_ip_addr,
        start_time,
        original_request_body,
        kind,
    } = input;
    let query_param_map = replay_query_params_to_map(&query_params);
    let attempt_kind = match kind {
        GatewayReplayAttemptKind::Generation {
            api_type,
            is_stream,
            data,
            original_request_value,
        } => AttemptExecutionKind::Generation {
            user_api_type: api_type,
            is_stream,
            data,
            original_request_value,
        },
        GatewayReplayAttemptKind::Utility { operation, data } => {
            AttemptExecutionKind::Utility { operation, data }
        }
    };

    match Box::pin(schedule_execution(
        Arc::clone(&app_state),
        SchedulerExecutionInput {
            cancellation: ProxyCancellationContext::new(),
            api_key,
            execution_plan,
            query_params: query_param_map,
            replay_query_params: Some(query_params),
            original_headers,
            request_snapshot,
            original_request_body,
            client_ip_addr,
            start_time,
            log_mode: RuntimeLogMode::DeferNonStreaming,
            execution_policy: RuntimeExecutionPolicy::ReplayLive,
            kind: attempt_kind,
        },
    ))
    .await
    {
        Ok(success) => {
            let metadata = gateway_replay_execution_metadata(
                resolved_route_id,
                resolved_route_name,
                &success.prior_attempts,
                &success.terminal_attempt,
                &success.log_context,
            );
            Ok(GatewayReplayExecutionSuccess {
                response: success.response,
                metadata,
            })
        }
        Err(failure) => Err(gateway_replay_failure_from_scheduler(
            failure,
            resolved_route_id,
            resolved_route_name,
        )),
    }
}

async fn materialize_gateway_replay_request(
    app_state: Arc<AppState>,
    input: GatewayReplayInput,
    execution_plan: ExecutionPlan,
) -> Result<GatewayReplayPreparedRequest, ProxyError> {
    let candidate_manifest = build_candidate_manifest(&execution_plan);
    let requirement = match &input.kind {
        GatewayReplayAttemptKind::Generation {
            api_type,
            is_stream,
            data,
            ..
        } => {
            ensure_reasoning_preset_allows_operation(
                &execution_plan,
                RequestedOperationKind::Generation,
                "generation",
            )?;
            derive_generation_requirement(
                data,
                *api_type,
                *is_stream,
                execution_plan.resolved_reasoning_preset,
            )
        }
        GatewayReplayAttemptKind::Utility { operation, data } => {
            ensure_reasoning_preset_allows_operation(
                &execution_plan,
                RequestedOperationKind::Utility,
                &operation.name,
            )?;
            derive_utility_requirement(&operation.name, data)
        }
    };
    let prefiltered_plan = prefilter_execution_plan(execution_plan, &requirement);
    let execution_plan = prefiltered_plan.execution_plan;
    let mut candidate_decisions: Vec<GatewayReplayCandidateDecision> = prefiltered_plan
        .skipped_attempts
        .iter()
        .map(GatewayReplayCandidateDecision::from)
        .collect();
    if execution_plan.candidates.is_empty() {
        return Err(ProxyError::BadRequest(no_candidate_error_message(
            &requirement,
        )));
    }

    let runtime_snapshot = app_state.system_config.runtime_snapshot().await;
    let routing_resilience = runtime_snapshot.routing_resilience;
    let candidate_budget = routing_resilience.max_candidates_per_request.max(1) as usize;
    let mut candidate_index = 0usize;
    while candidate_index < execution_plan.candidates.len() && candidate_index < candidate_budget {
        let candidate = execution_plan.candidates[candidate_index].clone();
        let attempted_candidate_count = (candidate_index + 1) as u32;
        let mut same_candidate_retry_count = 0u32;

        loop {
            let next_candidate_available = candidate_index + 1 < execution_plan.candidates.len()
                && candidate_index + 1 < candidate_budget;
            match materialize_gateway_replay_candidate(
                &app_state,
                &input,
                &execution_plan,
                &candidate_manifest,
                &candidate,
                &routing_resilience,
                same_candidate_retry_count,
                attempted_candidate_count,
                next_candidate_available,
            )
            .await?
            {
                GatewayReplayCandidateMaterialization::Ready {
                    mut prepared,
                    attempt,
                } => {
                    candidate_decisions.push(GatewayReplayCandidateDecision::from(&attempt));
                    prepared.candidate_decisions = candidate_decisions;
                    return Ok(prepared);
                }
                GatewayReplayCandidateMaterialization::Rejected { attempt, error } => {
                    let scheduler_action = attempt.scheduler_action;
                    candidate_decisions.push(GatewayReplayCandidateDecision::from(&attempt));
                    match scheduler_action {
                        SchedulerAction::RetrySameCandidate => {
                            same_candidate_retry_count =
                                same_candidate_retry_count.saturating_add(1);
                        }
                        SchedulerAction::FallbackNextCandidate
                            if candidate_index + 1 < execution_plan.candidates.len()
                                && candidate_index + 1 < candidate_budget =>
                        {
                            candidate_index += 1;
                            break;
                        }
                        _ => return Err(error),
                    }
                }
            }
        }
    }

    Err(ProxyError::BadRequest(no_candidate_error_message(
        &requirement,
    )))
}

enum GatewayReplayCandidateMaterialization {
    Ready {
        prepared: GatewayReplayPreparedRequest,
        attempt: RequestAttemptDraft,
    },
    Rejected {
        attempt: RequestAttemptDraft,
        error: ProxyError,
    },
}

fn rejected_gateway_replay_candidate(
    routing_resilience: &RoutingResilienceConfig,
    mut attempt: RequestAttemptDraft,
    proxy_error: ProxyError,
    same_candidate_retry_count: u32,
    attempted_candidate_count: u32,
    next_candidate_available: bool,
) -> GatewayReplayCandidateMaterialization {
    attempt.completed_at = Some(Utc::now().timestamp_millis());
    classify_attempt_failure(
        routing_resilience,
        &mut attempt,
        &proxy_error,
        same_candidate_retry_count,
        attempted_candidate_count,
        next_candidate_available,
        None,
    );
    GatewayReplayCandidateMaterialization::Rejected {
        attempt,
        error: proxy_error,
    }
}

async fn materialize_gateway_replay_candidate(
    app_state: &Arc<AppState>,
    input: &GatewayReplayInput,
    execution_plan: &ExecutionPlan,
    candidate_manifest: &RequestLogBundleCandidateManifest,
    candidate: &ExecutionCandidate,
    routing_resilience: &RoutingResilienceConfig,
    same_candidate_retry_count: u32,
    attempted_candidate_count: u32,
    next_candidate_available: bool,
) -> Result<GatewayReplayCandidateMaterialization, ProxyError> {
    let mut attempt = RequestAttemptDraft::pending_for_candidate(candidate);
    let provider_credentials =
        match resolve_provider_credentials(&candidate.provider, app_state).await {
            Ok(credentials) => credentials,
            Err(proxy_error) => {
                return Ok(rejected_gateway_replay_candidate(
                    routing_resilience,
                    attempt,
                    proxy_error,
                    same_candidate_retry_count,
                    attempted_candidate_count,
                    next_candidate_available,
                ));
            }
        };
    attempt.provider_api_key_id = Some(provider_credentials.key_id);

    if let GatewayReplayAttemptKind::Utility { operation, .. } = &input.kind {
        if let Err(proxy_error) = validate_utility_target(operation, candidate.llm_api_type) {
            return Ok(rejected_gateway_replay_candidate(
                routing_resilience,
                attempt,
                proxy_error,
                same_candidate_retry_count,
                attempted_candidate_count,
                next_candidate_available,
            ));
        }
    }

    if let Err(proxy_error) = check_access_control(
        &input.api_key,
        &candidate.provider,
        &candidate.model,
        app_state,
    )
    .await
    {
        return Ok(rejected_gateway_replay_candidate(
            routing_resilience,
            attempt,
            proxy_error,
            same_candidate_retry_count,
            attempted_candidate_count,
            next_candidate_available,
        ));
    }

    let request_patch_trace = match load_runtime_request_patch_trace(
        &candidate.provider,
        Some(&candidate.model),
        Some(candidate),
        app_state,
    )
    .await
    {
        Ok(trace) => trace,
        Err(proxy_error) => {
            return Ok(rejected_gateway_replay_candidate(
                routing_resilience,
                attempt,
                proxy_error,
                same_candidate_retry_count,
                attempted_candidate_count,
                next_candidate_available,
            ));
        }
    };
    attempt.applied_request_patch_ids_json =
        request_patch_trace.applied_request_patch_ids_json.clone();
    attempt.request_patch_summary_json = request_patch_trace.request_patch_summary_json.clone();
    if let Some(proxy_error) = request_patch_trace.conflict_error(&candidate.model.model_name) {
        return Ok(rejected_gateway_replay_candidate(
            routing_resilience,
            attempt,
            proxy_error,
            same_candidate_retry_count,
            attempted_candidate_count,
            next_candidate_available,
        ));
    }

    let replay_query_param_map = replay_query_params_to_map(&input.query_params);
    let materialized = match &input.kind {
        GatewayReplayAttemptKind::Generation {
            api_type,
            is_stream,
            data,
            original_request_value,
        } => match materialize_generation_attempt(
            candidate,
            data.clone(),
            *api_type,
            *is_stream,
            original_request_value,
            &input.original_headers,
            &replay_query_param_map,
            Some(&input.query_params),
            &request_patch_trace.applied_rules,
            &provider_credentials,
        )
        .await
        {
            Ok(materialized) => materialized,
            Err(proxy_error) => {
                return Ok(rejected_gateway_replay_candidate(
                    routing_resilience,
                    attempt,
                    proxy_error,
                    same_candidate_retry_count,
                    attempted_candidate_count,
                    next_candidate_available,
                ));
            }
        },
        GatewayReplayAttemptKind::Utility { operation, data } => match materialize_utility_attempt(
            candidate,
            operation,
            data.clone(),
            &input.original_headers,
            &replay_query_param_map,
            Some(&input.query_params),
            &request_patch_trace.applied_rules,
            &provider_credentials,
        )
        .await
        {
            Ok(materialized) => materialized,
            Err(proxy_error) => {
                return Ok(rejected_gateway_replay_candidate(
                    routing_resilience,
                    attempt,
                    proxy_error,
                    same_candidate_retry_count,
                    attempted_candidate_count,
                    next_candidate_available,
                ));
            }
        },
    };
    attempt.llm_request_body_for_log = materialized.llm_request_body_for_log.clone();
    attempt.request_uri = Some(materialized.final_url.clone());
    attempt.request_headers_json =
        serialize_downstream_request_headers_for_log(&materialized.final_headers);
    let now = Utc::now().timestamp_millis();
    attempt.started_at = Some(now);

    match preview_provider_request_allowed(app_state, candidate.provider.id).await {
        Ok(()) => {}
        Err(ProviderGovernanceCheckError::Rejected(rejection)) => {
            attempt.completed_at = Some(Utc::now().timestamp_millis());
            attempt.started_at = None;
            attempt.provider_api_key_id = None;
            attempt.request_uri = None;
            attempt.request_headers_json = None;
            attempt.llm_request_body_for_log = None;
            classify_provider_governance_skip(
                routing_resilience,
                &mut attempt,
                rejection,
                materialized.model_str.as_str(),
                attempted_candidate_count,
                next_candidate_available,
            );
            return Ok(GatewayReplayCandidateMaterialization::Rejected {
                attempt,
                error: rejection.to_proxy_error(materialized.model_str.as_str()),
            });
        }
        Err(ProviderGovernanceCheckError::Backend(proxy_error)) => return Err(proxy_error),
    }

    attempt.completed_at = Some(now);
    attempt.attempt_status = RequestAttemptStatus::Success;
    attempt.scheduler_action = SchedulerAction::ReturnSuccess;

    Ok(GatewayReplayCandidateMaterialization::Ready {
        prepared: GatewayReplayPreparedRequest {
            requested_model_name: execution_plan.requested_name.clone(),
            base_requested_model_name: execution_plan.base_requested_name.clone(),
            resolved_reasoning_suffix: execution_plan.resolved_reasoning_suffix.clone(),
            resolved_reasoning_preset: execution_plan
                .resolved_reasoning_preset
                .map(|preset| preset.as_key().to_string()),
            resolved_name_scope: execution_plan.resolved_scope.as_str().to_string(),
            resolved_route_id: execution_plan.resolved_route_id,
            resolved_route_name: execution_plan.resolved_route_name.clone(),
            candidate_position: candidate.candidate_position as i32,
            provider_id: candidate.provider.id,
            provider_api_key_id: materialized.provider_api_key_id,
            model_id: candidate.model.id,
            llm_api_type: candidate.llm_api_type,
            applied_request_patch_summary: request_patch_trace
                .request_patch_summary_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok()),
            final_request_uri: materialized.final_url,
            final_request_headers: materialized.final_headers,
            final_request_body: materialized.final_body,
            transform_diagnostics: materialized.transform_diagnostics,
            candidate_manifest: candidate_manifest.clone(),
            candidate_decisions: Vec::new(),
        },
        attempt,
    })
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use serde_json::json;

    use crate::{
        proxy::runtime::{
            attempt::RequestAttemptDraft,
            replay_adapter::{GatewayReplayCandidateDecision, replay_query_params_to_map},
        },
        schema::enum_def::{LlmApiType, RequestAttemptStatus, SchedulerAction},
        utils::storage::RequestLogBundleQueryParam,
    };

    #[test]
    fn replay_query_params_to_map_preserves_blank_values_and_omits_absent_flags() {
        let params = vec![
            RequestLogBundleQueryParam {
                name: "present".to_string(),
                value: Some("value".to_string()),
                value_present: true,
                encoded_name: None,
                encoded_value: None,
            },
            RequestLogBundleQueryParam {
                name: "blank".to_string(),
                value: None,
                value_present: true,
                encoded_name: None,
                encoded_value: None,
            },
            RequestLogBundleQueryParam {
                name: "absent".to_string(),
                value: None,
                value_present: false,
                encoded_name: None,
                encoded_value: None,
            },
        ];

        let mapped = replay_query_params_to_map(&params);

        assert_eq!(mapped.get("present").map(String::as_str), Some("value"));
        assert_eq!(mapped.get("blank").map(String::as_str), Some(""));
        assert!(!mapped.contains_key("absent"));
    }

    #[test]
    fn gateway_replay_candidate_decision_uses_attempt_runtime_projection() {
        let attempt = RequestAttemptDraft {
            candidate_position: 2,
            provider_id: Some(3),
            provider_api_key_id: Some(4),
            model_id: Some(5),
            llm_api_type: Some(LlmApiType::Openai),
            attempt_status: RequestAttemptStatus::Error,
            scheduler_action: SchedulerAction::FallbackNextCandidate,
            error_code: Some("upstream_timeout".to_string()),
            error_message: Some("timed out".to_string()),
            request_uri: Some("https://upstream.example/v1/chat/completions".to_string()),
            llm_request_body_for_log: Some(crate::proxy::logging::LoggedBody::from_bytes(
                Bytes::from_static(br#"{"model":"gpt"}"#),
            )),
            request_patch_summary_json: Some(json!({ "applied": [1] }).to_string()),
            ..RequestAttemptDraft::default()
        };

        let decision = GatewayReplayCandidateDecision::from(&attempt);

        assert_eq!(decision.candidate_position, 2);
        assert_eq!(decision.provider_id, Some(3));
        assert_eq!(decision.provider_api_key_id, Some(4));
        assert_eq!(decision.model_id, Some(5));
        assert_eq!(decision.llm_api_type, Some(LlmApiType::Openai));
        assert_eq!(decision.attempt_status, RequestAttemptStatus::Error);
        assert_eq!(
            decision.scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
        assert_eq!(decision.error_code.as_deref(), Some("upstream_timeout"));
        assert_eq!(
            decision.request_uri.as_deref(),
            Some("https://upstream.example/v1/chat/completions")
        );
    }
}
