use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    http::HeaderMap,
    response::Response,
};
use chrono::Utc;
use reqwest::header::RETRY_AFTER;
use serde_json::Value;

use crate::{
    proxy::{
        ProxyError,
        auth::{admit_api_key_request, check_access_control},
        cancellation::ProxyCancellationContext,
        logging::RequestLogContext,
        provider_governance::{ensure_provider_request_allowed, preview_provider_request_allowed},
        retry_policy::ProviderGovernanceRejection,
        runtime::{
            attempt::{
                RequestAttemptDraft, classify_attempt_failure, classify_provider_governance_skip,
                complete_attempt_from_response, sync_attempt_from_proxy_failure,
                sync_attempt_timing_and_usage,
            },
            credential::resolve_provider_credentials,
            log_writer::{
                AttemptLogContextInput, finalize_attempt_failure_context, log_provider_skipped,
                maybe_record_attempt_failure, new_attempt_log_context,
            },
            materializer::{materialize_generation_attempt, materialize_utility_attempt},
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
            request_patch::load_runtime_request_patch_trace,
            route_resolver::ExecutionCandidate,
            transport::send_materialized_request,
        },
        util::{get_cost_catalog_version, serialize_downstream_request_headers_for_log},
        utility::{UtilityOperation, validate_utility_target},
    },
    schema::enum_def::LlmApiType,
    service::{app_state::AppState, cache::types::CacheApiKey},
    utils::storage::{
        RequestLogBundleCandidateManifest, RequestLogBundleQueryParam,
        RequestLogBundleRequestSnapshot, RequestLogBundleTransformDiagnosticItem,
        RequestLogBundleTransformDiagnosticPhase,
    },
};

#[derive(Debug, Clone)]
pub(in crate::proxy) enum AttemptExecutionKind {
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

pub(in crate::proxy) struct AttemptExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub candidate: ExecutionCandidate,
    pub requested_model_name: String,
    pub base_requested_model_name: String,
    pub resolved_reasoning_suffix: Option<String>,
    pub resolved_reasoning_preset: Option<String>,
    pub resolved_name_scope: String,
    pub resolved_route_id: Option<i64>,
    pub resolved_route_name: Option<String>,
    pub query_params: HashMap<String, String>,
    pub replay_query_params: Option<Vec<RequestLogBundleQueryParam>>,
    pub original_headers: HeaderMap,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
    pub candidate_manifest: RequestLogBundleCandidateManifest,
    pub original_request_body: Bytes,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub skipped_attempts: Vec<RequestAttemptDraft>,
    pub prior_transform_diagnostics: Vec<RequestLogBundleTransformDiagnosticItem>,
    pub same_candidate_retry_count: u32,
    pub attempted_candidate_count: u32,
    pub next_candidate_available: bool,
    pub log_mode: RuntimeLogMode,
    pub execution_policy: RuntimeExecutionPolicy,
    pub kind: AttemptExecutionKind,
}

pub(in crate::proxy) struct AttemptExecutionResult {
    pub attempt: RequestAttemptDraft,
    pub response: Result<Response<Body>, ProxyError>,
    pub log_context: RequestLogContext,
}

#[derive(Clone, Copy)]
struct AttemptSchedulingContext {
    same_candidate_retry_count: u32,
    attempted_candidate_count: u32,
    next_candidate_available: bool,
}

fn retry_after_from_headers(headers: Option<&HeaderMap>) -> Option<Duration> {
    headers
        .and_then(|headers| headers.get(RETRY_AFTER))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

async fn finalize_early_attempt_failure(
    app_state: &Arc<AppState>,
    log_context: RequestLogContext,
    skipped_attempts_for_log: &[RequestAttemptDraft],
    attempt: &mut RequestAttemptDraft,
    proxy_error: &ProxyError,
    scheduling: AttemptSchedulingContext,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
) -> RequestLogContext {
    attempt.completed_at = Some(Utc::now().timestamp_millis());
    classify_attempt_failure(
        attempt,
        proxy_error,
        scheduling.same_candidate_retry_count,
        scheduling.attempted_candidate_count,
        scheduling.next_candidate_available,
        None,
    );
    maybe_record_attempt_failure(
        app_state,
        log_context,
        skipped_attempts_for_log,
        attempt,
        proxy_error,
        log_mode,
        execution_policy,
    )
    .await
}

async fn ensure_provider_governance_for_policy(
    app_state: &AppState,
    execution_policy: RuntimeExecutionPolicy,
    provider_id: i64,
    provider_label: &str,
) -> Result<(), ProviderGovernanceRejection> {
    if execution_policy.uses_mutating_provider_governance() {
        ensure_provider_request_allowed(app_state, provider_id, provider_label).await
    } else {
        debug_assert!(execution_policy.uses_read_only_provider_governance());
        preview_provider_request_allowed(app_state, provider_id).await
    }
}

fn clear_provider_governance_skip_runtime_fields(
    attempt: &mut RequestAttemptDraft,
    log_context: &mut RequestLogContext,
) {
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
}

pub(in crate::proxy) async fn execute_attempt(
    app_state: Arc<AppState>,
    input: AttemptExecutionInput,
) -> AttemptExecutionResult {
    let AttemptExecutionInput {
        cancellation,
        api_key,
        candidate,
        requested_model_name,
        base_requested_model_name,
        resolved_reasoning_suffix,
        resolved_reasoning_preset,
        resolved_name_scope,
        resolved_route_id,
        resolved_route_name,
        query_params,
        replay_query_params,
        original_headers,
        request_snapshot,
        candidate_manifest,
        original_request_body,
        client_ip_addr,
        start_time,
        skipped_attempts,
        prior_transform_diagnostics,
        same_candidate_retry_count,
        attempted_candidate_count,
        next_candidate_available,
        log_mode,
        execution_policy,
        kind,
    } = input;

    let scheduling = AttemptSchedulingContext {
        same_candidate_retry_count,
        attempted_candidate_count,
        next_candidate_available,
    };
    let mut attempt = RequestAttemptDraft::pending_for_candidate(&candidate);
    let skipped_attempts_for_log = skipped_attempts.clone();

    let user_api_type_for_log = match &kind {
        AttemptExecutionKind::Generation { user_api_type, .. } => *user_api_type,
        AttemptExecutionKind::Utility { operation, .. } => operation.api_type,
    };
    let mut log_context = new_attempt_log_context(AttemptLogContextInput {
        api_key: &api_key,
        candidate: &candidate,
        requested_model_name: &requested_model_name,
        base_requested_model_name: &base_requested_model_name,
        resolved_reasoning_suffix: resolved_reasoning_suffix.as_deref(),
        resolved_reasoning_preset: resolved_reasoning_preset.as_deref(),
        resolved_name_scope: &resolved_name_scope,
        resolved_route_id,
        resolved_route_name: resolved_route_name.as_deref(),
        request_snapshot: request_snapshot.clone(),
        candidate_manifest,
        prior_transform_diagnostics: &prior_transform_diagnostics,
        original_request_body,
        client_ip_addr: &client_ip_addr,
        start_time,
        user_api_type: user_api_type_for_log,
        current_attempt: attempt.clone(),
        skipped_attempts: &skipped_attempts_for_log,
    });

    let provider_credentials =
        match resolve_provider_credentials(&candidate.provider, &app_state).await {
            Ok(credentials) => credentials,
            Err(proxy_error) => {
                let log_context = finalize_early_attempt_failure(
                    &app_state,
                    log_context,
                    &skipped_attempts_for_log,
                    &mut attempt,
                    &proxy_error,
                    scheduling,
                    log_mode,
                    execution_policy,
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
            let log_context = finalize_early_attempt_failure(
                &app_state,
                log_context,
                &skipped_attempts_for_log,
                &mut attempt,
                &proxy_error,
                scheduling,
                log_mode,
                execution_policy,
            )
            .await;
            return AttemptExecutionResult {
                attempt,
                response: Err(proxy_error),
                log_context,
            };
        }
    }

    if let Err(proxy_error) =
        check_access_control(&api_key, &candidate.provider, &candidate.model, &app_state).await
    {
        let log_context = finalize_early_attempt_failure(
            &app_state,
            log_context,
            &skipped_attempts_for_log,
            &mut attempt,
            &proxy_error,
            scheduling,
            log_mode,
            execution_policy,
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
        Some(&candidate),
        &app_state,
    )
    .await
    {
        Ok(trace) => trace,
        Err(proxy_error) => {
            let log_context = finalize_early_attempt_failure(
                &app_state,
                log_context,
                &skipped_attempts_for_log,
                &mut attempt,
                &proxy_error,
                scheduling,
                log_mode,
                execution_policy,
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
        let log_context = finalize_early_attempt_failure(
            &app_state,
            log_context,
            &skipped_attempts_for_log,
            &mut attempt,
            &proxy_error,
            scheduling,
            log_mode,
            execution_policy,
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
                replay_query_params.as_deref(),
                &request_patch_trace.applied_rules,
                &provider_credentials,
            )
            .await
            {
                Ok(materialized) => materialized,
                Err(proxy_error) => {
                    let log_context = finalize_early_attempt_failure(
                        &app_state,
                        log_context,
                        &skipped_attempts_for_log,
                        &mut attempt,
                        &proxy_error,
                        scheduling,
                        log_mode,
                        execution_policy,
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
            replay_query_params.as_deref(),
            &request_patch_trace.applied_rules,
            &provider_credentials,
        )
        .await
        {
            Ok(materialized) => materialized,
            Err(proxy_error) => {
                let log_context = finalize_early_attempt_failure(
                    &app_state,
                    log_context,
                    &skipped_attempts_for_log,
                    &mut attempt,
                    &proxy_error,
                    scheduling,
                    log_mode,
                    execution_policy,
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
    debug_assert_eq!(
        attempt.provider_api_key_id,
        Some(materialized.provider_api_key_id)
    );
    log_context.llm_request_body = materialized.llm_request_body_for_log;
    log_context.append_transform_diagnostics(
        RequestLogBundleTransformDiagnosticPhase::Request,
        &materialized.transform_diagnostics,
    );
    attempt.request_uri = Some(materialized.final_url.clone());
    attempt.request_headers_json =
        serialize_downstream_request_headers_for_log(&materialized.final_headers);
    attempt.started_at = Some(Utc::now().timestamp_millis());
    sync_attempt_timing_and_usage(&mut attempt, &log_context, cost_catalog_version.as_ref());
    log_context.set_attempts_for_logging(&skipped_attempts_for_log, Some(attempt.clone()));

    let api_key_concurrency_guard = if execution_policy.admits_api_key_requests() {
        match admit_api_key_request(&app_state, &api_key).await {
            Ok(guard) => guard,
            Err(proxy_error) => {
                let log_context = finalize_early_attempt_failure(
                    &app_state,
                    log_context,
                    &skipped_attempts_for_log,
                    &mut attempt,
                    &proxy_error,
                    scheduling,
                    log_mode,
                    execution_policy,
                )
                .await;
                return AttemptExecutionResult {
                    attempt,
                    response: Err(proxy_error),
                    log_context,
                };
            }
        }
    } else {
        None
    };

    if let Err(rejection) = ensure_provider_governance_for_policy(
        &app_state,
        execution_policy,
        candidate.provider.id,
        materialized.model_str.as_str(),
    )
    .await
    {
        let completed_at = Utc::now().timestamp_millis();
        attempt.completed_at = Some(completed_at);
        clear_provider_governance_skip_runtime_fields(&mut attempt, &mut log_context);
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
            execution_policy,
        )
        .await;
        log_provider_skipped(&log_context, &attempt, &proxy_error);
        return AttemptExecutionResult {
            attempt,
            response: Err(proxy_error),
            log_context,
        };
    }

    let proxy_result = send_materialized_request(
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
        execution_policy,
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

#[cfg(test)]
mod tests {
    use super::{ensure_provider_governance_for_policy, retry_after_from_headers};
    use async_trait::async_trait;
    use axum::http::HeaderMap;
    use reqwest::header::{HeaderValue, RETRY_AFTER};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use crate::{
        config::ProviderGovernanceConfig,
        proxy::runtime::policy::RuntimeExecutionPolicy,
        service::{
            app_state::AppState,
            runtime::{
                ProviderCircuitService, ProviderCircuitStore, ProviderHealthSnapshot,
                ProviderHealthStatus,
            },
        },
    };

    struct RecordingProviderCircuitStore {
        allow_calls: AtomicUsize,
        snapshot: tokio::sync::Mutex<ProviderHealthSnapshot>,
    }

    impl RecordingProviderCircuitStore {
        fn new(snapshot: ProviderHealthSnapshot) -> Self {
            Self {
                allow_calls: AtomicUsize::new(0),
                snapshot: tokio::sync::Mutex::new(snapshot),
            }
        }

        fn open_snapshot() -> ProviderHealthSnapshot {
            ProviderHealthSnapshot {
                status: ProviderHealthStatus::Open,
                consecutive_failures: 1,
                half_open_probe_in_flight: false,
                opened_at: None,
                last_failure_at: Some(1),
                last_recovered_at: None,
                last_error: Some("forced open".to_string()),
            }
        }
    }

    #[async_trait]
    impl ProviderCircuitStore for RecordingProviderCircuitStore {
        async fn allow_request(
            &self,
            _provider_id: i64,
            _config: &ProviderGovernanceConfig,
        ) -> Result<ProviderHealthSnapshot, Option<Duration>> {
            self.allow_calls.fetch_add(1, Ordering::SeqCst);
            let mut snapshot = self.snapshot.lock().await;
            snapshot.status = ProviderHealthStatus::HalfOpen;
            snapshot.half_open_probe_in_flight = true;
            Ok(snapshot.clone())
        }

        async fn record_success(&self, _provider_id: i64) -> ProviderHealthSnapshot {
            self.snapshot.lock().await.clone()
        }

        async fn record_failure(
            &self,
            _provider_id: i64,
            _config: &ProviderGovernanceConfig,
            _error_message: String,
        ) -> ProviderHealthSnapshot {
            self.snapshot.lock().await.clone()
        }

        async fn snapshot(&self, _provider_id: i64) -> ProviderHealthSnapshot {
            self.snapshot.lock().await.clone()
        }
    }

    #[test]
    fn retry_after_from_headers_parses_delta_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("7"));

        assert_eq!(
            retry_after_from_headers(Some(&headers)),
            Some(Duration::from_secs(7))
        );
    }

    #[test]
    fn retry_after_from_headers_ignores_invalid_values() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("not-seconds"));

        assert_eq!(retry_after_from_headers(Some(&headers)), None);
        assert_eq!(retry_after_from_headers(None), None);
    }

    #[tokio::test]
    async fn replay_live_provider_governance_uses_read_only_preview() {
        let store = Arc::new(RecordingProviderCircuitStore::new(
            RecordingProviderCircuitStore::open_snapshot(),
        ));
        let mut app_state = AppState::new().await;
        app_state.provider_circuit = Arc::new(ProviderCircuitService::new(store.clone()));

        let result = ensure_provider_governance_for_policy(
            &app_state,
            RuntimeExecutionPolicy::ReplayLive,
            7,
            "model",
        )
        .await;

        assert!(result.is_err());
        assert_eq!(store.allow_calls.load(Ordering::SeqCst), 0);
        let snapshot = app_state
            .provider_circuit
            .get_provider_health_snapshot(7)
            .await;
        assert_eq!(snapshot.status, ProviderHealthStatus::Open);
        assert!(!snapshot.half_open_probe_in_flight);
    }

    #[tokio::test]
    async fn normal_provider_governance_uses_mutating_allow() {
        let store = Arc::new(RecordingProviderCircuitStore::new(
            RecordingProviderCircuitStore::open_snapshot(),
        ));
        let mut app_state = AppState::new().await;
        app_state.provider_circuit = Arc::new(ProviderCircuitService::new(store.clone()));

        let result = ensure_provider_governance_for_policy(
            &app_state,
            RuntimeExecutionPolicy::Normal,
            7,
            "model",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(store.allow_calls.load(Ordering::SeqCst), 1);
        let snapshot = app_state
            .provider_circuit
            .get_provider_health_snapshot(7)
            .await;
        assert_eq!(snapshot.status, ProviderHealthStatus::HalfOpen);
        assert!(snapshot.half_open_probe_in_flight);
    }
}
