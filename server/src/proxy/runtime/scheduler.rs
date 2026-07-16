use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    http::HeaderMap,
    response::Response,
};
use tokio::time::sleep;

use crate::{
    proxy::{
        ProxyError,
        auth::check_access_control,
        cancellation::ProxyCancellationContext,
        logging::RequestLogContext,
        runtime::{
            attempt::RequestAttemptDraft,
            candidate_filter::{
                RequestedOperationKind, derive_generation_requirement, derive_utility_requirement,
                ensure_reasoning_preset_allows_operation, no_candidate_error_message,
                prefilter_execution_plan,
            },
            executor::{
                AttemptExecutionInput, AttemptExecutionKind, AttemptExecutionResult,
                execute_attempt,
            },
            log_writer::{
                NoCandidateFailureLogInput, build_candidate_manifest, log_fallback_next_candidate,
                log_request_failed, log_retry_scheduled, record_completion_if_allowed,
                record_no_candidate_failure,
            },
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
            route_resolver::{ExecutionCandidate, ExecutionPlan},
        },
    },
    schema::enum_def::{LlmApiType, SchedulerAction},
    service::{app_state::AppState, cache::types::CacheApiKey},
    utils::storage::{
        RequestLogBundleQueryParam, RequestLogBundleRequestSnapshot,
        RequestLogBundleTransformDiagnosticItem,
    },
};

pub(in crate::proxy) struct SchedulerExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub execution_plan: ExecutionPlan,
    pub query_params: HashMap<String, String>,
    pub replay_query_params: Option<Vec<RequestLogBundleQueryParam>>,
    pub original_headers: HeaderMap,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
    pub original_request_body: Bytes,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub log_mode: RuntimeLogMode,
    pub execution_policy: RuntimeExecutionPolicy,
    pub kind: AttemptExecutionKind,
}

pub(in crate::proxy) struct SchedulerExecutionSuccess {
    pub response: Response<Body>,
    pub terminal_attempt: RequestAttemptDraft,
    pub prior_attempts: Vec<RequestAttemptDraft>,
    pub log_context: RequestLogContext,
}

pub(in crate::proxy) struct SchedulerExecutionFailure {
    pub error: ProxyError,
    pub terminal_attempt: Option<RequestAttemptDraft>,
    pub prior_attempts: Vec<RequestAttemptDraft>,
    pub log_context: Option<RequestLogContext>,
}

#[derive(Debug, PartialEq, Eq)]
enum SchedulerStep {
    RetrySameCandidate { backoff_ms: u64 },
    FallbackNextCandidate,
    TerminalFailure,
}

fn user_api_type_for_kind(kind: &AttemptExecutionKind) -> LlmApiType {
    match kind {
        AttemptExecutionKind::Generation { user_api_type, .. } => *user_api_type,
        AttemptExecutionKind::Utility { operation, .. } => operation.api_type,
    }
}

fn next_candidate_available(
    candidate_index: usize,
    candidate_count: usize,
    candidate_budget: usize,
) -> bool {
    candidate_index + 1 < candidate_count && candidate_index + 1 < candidate_budget
}

pub(super) async fn next_accessible_candidate_available(
    candidate_index: usize,
    candidates: &[ExecutionCandidate],
    candidate_budget: usize,
    api_key: &CacheApiKey,
    app_state: &Arc<AppState>,
) -> bool {
    if !next_candidate_available(candidate_index, candidates.len(), candidate_budget) {
        return false;
    }

    let next_candidate = &candidates[candidate_index + 1];
    check_access_control(
        api_key,
        &next_candidate.provider,
        &next_candidate.model,
        app_state,
    )
    .await
    .is_ok()
}

fn scheduler_step_for_attempt(
    attempt: &RequestAttemptDraft,
    candidate_index: usize,
    candidate_count: usize,
    candidate_budget: usize,
) -> SchedulerStep {
    match attempt.scheduler_action {
        SchedulerAction::RetrySameCandidate => SchedulerStep::RetrySameCandidate {
            backoff_ms: attempt.backoff_ms.unwrap_or_default().max(0) as u64,
        },
        SchedulerAction::FallbackNextCandidate
            if next_candidate_available(candidate_index, candidate_count, candidate_budget) =>
        {
            SchedulerStep::FallbackNextCandidate
        }
        _ => SchedulerStep::TerminalFailure,
    }
}

fn should_log_fallback_next_candidate(proxy_error: &ProxyError) -> bool {
    !matches!(
        proxy_error,
        ProxyError::ProviderOpenSkipped(_) | ProxyError::ProviderHalfOpenProbeInFlight(_)
    )
}

fn build_no_candidate_failure(
    error: ProxyError,
    prior_attempts: Vec<RequestAttemptDraft>,
    log_context: Option<RequestLogContext>,
) -> SchedulerExecutionFailure {
    SchedulerExecutionFailure {
        error,
        terminal_attempt: None,
        prior_attempts,
        log_context,
    }
}

pub(in crate::proxy) async fn schedule_execution(
    app_state: Arc<AppState>,
    input: SchedulerExecutionInput,
) -> Result<SchedulerExecutionSuccess, SchedulerExecutionFailure> {
    let SchedulerExecutionInput {
        cancellation,
        api_key,
        execution_plan,
        query_params,
        replay_query_params,
        original_headers,
        request_snapshot,
        original_request_body,
        client_ip_addr,
        start_time,
        log_mode,
        execution_policy,
        kind,
    } = input;

    let candidate_manifest = build_candidate_manifest(&execution_plan);
    let requirement = match &kind {
        AttemptExecutionKind::Generation {
            user_api_type,
            is_stream,
            data,
            ..
        } => {
            if let Err(error) = ensure_reasoning_preset_allows_operation(
                &execution_plan,
                RequestedOperationKind::Generation,
                "generation",
            ) {
                return Err(SchedulerExecutionFailure {
                    error,
                    terminal_attempt: None,
                    prior_attempts: Vec::new(),
                    log_context: None,
                });
            }
            derive_generation_requirement(
                data,
                *user_api_type,
                *is_stream,
                execution_plan.resolved_reasoning_preset,
            )
        }
        AttemptExecutionKind::Utility { operation, data } => {
            if let Err(error) = ensure_reasoning_preset_allows_operation(
                &execution_plan,
                RequestedOperationKind::Utility,
                &operation.name,
            ) {
                return Err(SchedulerExecutionFailure {
                    error,
                    terminal_attempt: None,
                    prior_attempts: Vec::new(),
                    log_context: None,
                });
            }
            derive_utility_requirement(&operation.name, data)
        }
    };

    let prefiltered_plan = prefilter_execution_plan(execution_plan, &requirement);
    let execution_plan = prefiltered_plan.execution_plan;
    let mut prior_attempts = prefiltered_plan.skipped_attempts;
    let mut prior_transform_diagnostics: Vec<RequestLogBundleTransformDiagnosticItem> = Vec::new();
    let user_api_type = user_api_type_for_kind(&kind);

    if execution_plan.candidates.is_empty() {
        let message = no_candidate_error_message(&requirement);
        let log_context = record_no_candidate_failure(NoCandidateFailureLogInput {
            app_state: &app_state,
            api_key: &api_key,
            execution_plan: &execution_plan,
            skipped_attempts: prior_attempts.clone(),
            user_api_type,
            request_snapshot,
            candidate_manifest,
            original_request_body,
            start_time,
            client_ip_addr: &client_ip_addr,
            message: &message,
            execution_policy,
        })
        .await;
        return Err(build_no_candidate_failure(
            ProxyError::BadRequest(message),
            prior_attempts,
            log_context,
        ));
    }

    let requested_model_name = execution_plan.requested_name.clone();
    let base_requested_model_name = execution_plan.base_requested_name.clone();
    let resolved_reasoning_suffix = execution_plan.resolved_reasoning_suffix.clone();
    let resolved_reasoning_preset = execution_plan
        .resolved_reasoning_preset
        .map(|preset| preset.as_key().to_string());
    let resolved_name_scope = execution_plan.resolved_scope.as_str().to_string();
    let resolved_route_id = execution_plan.resolved_route_id;
    let resolved_route_name = execution_plan.resolved_route_name.clone();
    let runtime_snapshot = app_state.system_config.runtime_snapshot().await;
    let routing_resilience = runtime_snapshot.routing_resilience;
    let candidate_budget = routing_resilience.max_candidates_per_request.max(1) as usize;
    let mut candidate_index = 0usize;

    while candidate_index < execution_plan.candidates.len() && candidate_index < candidate_budget {
        let candidate = execution_plan.candidates[candidate_index].clone();
        let attempted_candidate_count = (candidate_index + 1) as u32;
        let mut same_candidate_retry_count = 0u32;

        loop {
            let next_candidate_available = next_accessible_candidate_available(
                candidate_index,
                &execution_plan.candidates,
                candidate_budget,
                &api_key,
                &app_state,
            )
            .await;
            let result = Box::pin(execute_attempt(
                Arc::clone(&app_state),
                AttemptExecutionInput {
                    cancellation: cancellation.clone(),
                    api_key: Arc::clone(&api_key),
                    candidate: candidate.clone(),
                    requested_model_name: requested_model_name.clone(),
                    base_requested_model_name: base_requested_model_name.clone(),
                    resolved_reasoning_suffix: resolved_reasoning_suffix.clone(),
                    resolved_reasoning_preset: resolved_reasoning_preset.clone(),
                    resolved_name_scope: resolved_name_scope.clone(),
                    resolved_route_id,
                    resolved_route_name: resolved_route_name.clone(),
                    query_params: query_params.clone(),
                    replay_query_params: replay_query_params.clone(),
                    original_headers: original_headers.clone(),
                    request_snapshot: request_snapshot.clone(),
                    candidate_manifest: candidate_manifest.clone(),
                    original_request_body: original_request_body.clone(),
                    client_ip_addr: client_ip_addr.clone(),
                    start_time,
                    skipped_attempts: prior_attempts.clone(),
                    prior_transform_diagnostics: prior_transform_diagnostics.clone(),
                    same_candidate_retry_count,
                    attempted_candidate_count,
                    next_candidate_available,
                    routing_resilience: routing_resilience.clone(),
                    log_mode,
                    execution_policy,
                    kind: kind.clone(),
                },
            ))
            .await;

            let AttemptExecutionResult {
                attempt,
                response,
                log_context,
            } = result;

            match response {
                Ok(response) => {
                    if !log_context.is_stream && !log_mode.should_record_immediate() {
                        record_completion_if_allowed(
                            &app_state,
                            log_context.clone(),
                            execution_policy,
                        )
                        .await;
                    }
                    return Ok(SchedulerExecutionSuccess {
                        response,
                        terminal_attempt: attempt,
                        prior_attempts,
                        log_context,
                    });
                }
                Err(error) => match scheduler_step_for_attempt(
                    &attempt,
                    candidate_index,
                    execution_plan.candidates.len(),
                    candidate_budget,
                ) {
                    SchedulerStep::RetrySameCandidate { backoff_ms } => {
                        log_retry_scheduled(&log_context, &attempt, &error);
                        prior_transform_diagnostics = log_context.transform_diagnostics.clone();
                        prior_attempts.push(attempt);
                        if backoff_ms > 0 {
                            sleep(Duration::from_millis(backoff_ms)).await;
                        }
                        same_candidate_retry_count = same_candidate_retry_count.saturating_add(1);
                    }
                    SchedulerStep::FallbackNextCandidate => {
                        if should_log_fallback_next_candidate(&error) {
                            log_fallback_next_candidate(&log_context, &attempt, &error);
                        }
                        prior_transform_diagnostics = log_context.transform_diagnostics.clone();
                        prior_attempts.push(attempt);
                        candidate_index += 1;
                        break;
                    }
                    SchedulerStep::TerminalFailure => {
                        log_request_failed(&log_context, Some(&attempt), &error);
                        if !log_mode.should_record_attempt_failure() {
                            record_completion_if_allowed(
                                &app_state,
                                log_context.clone(),
                                execution_policy,
                            )
                            .await;
                        }
                        return Err(SchedulerExecutionFailure {
                            error,
                            terminal_attempt: Some(attempt),
                            prior_attempts,
                            log_context: Some(log_context),
                        });
                    }
                },
            }
        }
    }

    let message = no_candidate_error_message(&requirement);
    let log_context = record_no_candidate_failure(NoCandidateFailureLogInput {
        app_state: &app_state,
        api_key: &api_key,
        execution_plan: &execution_plan,
        skipped_attempts: prior_attempts.clone(),
        user_api_type,
        request_snapshot,
        candidate_manifest,
        original_request_body,
        start_time,
        client_ip_addr: &client_ip_addr,
        message: &message,
        execution_policy,
    })
    .await;
    Err(build_no_candidate_failure(
        ProxyError::BadRequest(message),
        prior_attempts,
        log_context,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        SchedulerStep, next_candidate_available, scheduler_step_for_attempt,
        should_log_fallback_next_candidate,
    };
    use crate::{
        proxy::{ProxyError, runtime::attempt::RequestAttemptDraft},
        schema::enum_def::SchedulerAction,
    };

    #[test]
    fn next_candidate_availability_respects_candidate_budget() {
        assert!(next_candidate_available(0, 3, 2));
        assert!(!next_candidate_available(1, 3, 2));
        assert!(!next_candidate_available(0, 3, 1));
        assert!(!next_candidate_available(0, 1, 3));
    }

    #[test]
    fn scheduler_step_retries_same_candidate_with_non_negative_backoff() {
        let mut attempt = RequestAttemptDraft {
            scheduler_action: SchedulerAction::RetrySameCandidate,
            backoff_ms: Some(-25),
            ..RequestAttemptDraft::default()
        };
        assert_eq!(
            scheduler_step_for_attempt(&attempt, 0, 2, 2),
            SchedulerStep::RetrySameCandidate { backoff_ms: 0 }
        );

        attempt.backoff_ms = Some(250);
        assert_eq!(
            scheduler_step_for_attempt(&attempt, 0, 2, 2),
            SchedulerStep::RetrySameCandidate { backoff_ms: 250 }
        );
    }

    #[test]
    fn scheduler_step_fallback_requires_available_candidate_within_budget() {
        let attempt = RequestAttemptDraft {
            scheduler_action: SchedulerAction::FallbackNextCandidate,
            ..RequestAttemptDraft::default()
        };

        assert_eq!(
            scheduler_step_for_attempt(&attempt, 0, 3, 2),
            SchedulerStep::FallbackNextCandidate
        );
        assert_eq!(
            scheduler_step_for_attempt(&attempt, 1, 3, 2),
            SchedulerStep::TerminalFailure
        );
        assert_eq!(
            scheduler_step_for_attempt(&attempt, 0, 3, 1),
            SchedulerStep::TerminalFailure
        );
    }

    #[test]
    fn provider_governance_skip_fallbacks_are_not_double_logged() {
        assert!(!should_log_fallback_next_candidate(
            &ProxyError::ProviderOpenSkipped("open".to_string())
        ));
        assert!(!should_log_fallback_next_candidate(
            &ProxyError::ProviderHalfOpenProbeInFlight("probe".to_string())
        ));
        assert!(should_log_fallback_next_candidate(&ProxyError::BadRequest(
            "bad".to_string()
        )));
    }
}
