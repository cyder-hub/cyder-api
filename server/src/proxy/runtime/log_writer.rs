use std::sync::Arc;

use axum::{body::Bytes, http::StatusCode};
use chrono::Utc;
use tokio::sync::Mutex as TokioMutex;

use crate::{
    cost::UsageNormalization,
    proxy::{
        ProxyError,
        error::ProxyLogLevel,
        logging::{LoggedBody, RequestLogContext, record_request_completion_and_log},
        runtime::{
            attempt::{NO_CANDIDATE_AVAILABLE_ERROR, RequestAttemptDraft, truncate_error_message},
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
            route_resolver::{ExecutionCandidate, ExecutionPlan},
        },
    },
    schema::enum_def::{LlmApiType, RequestStatus, SchedulerAction},
    service::{
        app_state::AppState,
        cache::types::{CacheApiKey, CacheCostCatalogVersion},
        transform::unified::UnifiedTransformDiagnostic,
    },
    utils::{
        storage::{
            RequestLogBundleCandidateManifest, RequestLogBundleCandidateManifestItem,
            RequestLogBundleRequestSnapshot, RequestLogBundleTransformDiagnosticItem,
            RequestLogBundleTransformDiagnosticPhase,
        },
        usage::UsageInfo,
    },
};

pub(in crate::proxy) struct AttemptLogContextInput<'a> {
    pub api_key: &'a CacheApiKey,
    pub candidate: &'a ExecutionCandidate,
    pub requested_model_name: &'a str,
    pub base_requested_model_name: &'a str,
    pub resolved_reasoning_suffix: Option<&'a str>,
    pub resolved_reasoning_preset: Option<&'a str>,
    pub resolved_name_scope: &'a str,
    pub resolved_route_id: Option<i64>,
    pub resolved_route_name: Option<&'a str>,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
    pub candidate_manifest: RequestLogBundleCandidateManifest,
    pub prior_transform_diagnostics: &'a [RequestLogBundleTransformDiagnosticItem],
    pub original_request_body: Bytes,
    pub client_ip_addr: &'a Option<String>,
    pub start_time: i64,
    pub user_api_type: LlmApiType,
    pub current_attempt: RequestAttemptDraft,
    pub skipped_attempts: &'a [RequestAttemptDraft],
}

pub(in crate::proxy) fn build_candidate_manifest(
    execution_plan: &ExecutionPlan,
) -> RequestLogBundleCandidateManifest {
    RequestLogBundleCandidateManifest {
        items: execution_plan
            .candidates
            .iter()
            .map(|candidate| RequestLogBundleCandidateManifestItem {
                candidate_position: candidate.candidate_position as i32,
                route_id: candidate.route_id,
                route_name: candidate.route_name.clone(),
                provider_id: candidate.provider.id,
                provider_key: candidate.provider.provider_key.clone(),
                model_id: candidate.model.id,
                model_name: candidate.model.model_name.clone(),
                real_model_name: candidate.model.real_model_name.clone(),
                llm_api_type: candidate.llm_api_type,
                provider_api_key_mode: candidate.provider_api_key_mode.clone(),
            })
            .collect(),
    }
}

pub(in crate::proxy) fn new_attempt_log_context(
    input: AttemptLogContextInput<'_>,
) -> RequestLogContext {
    let mut log_context = RequestLogContext::new(
        input.api_key,
        &input.candidate.provider,
        &input.candidate.model,
        None,
        input.requested_model_name,
        input.resolved_name_scope,
        input.resolved_route_id,
        input.resolved_route_name,
        input.start_time,
        input.client_ip_addr,
        input.user_api_type,
        input.candidate.llm_api_type,
    );
    log_context.set_model_resolution_trace(
        input.base_requested_model_name,
        input.resolved_reasoning_suffix,
        input.resolved_reasoning_preset,
    );
    log_context.set_request_snapshot(input.request_snapshot);
    log_context.set_candidate_manifest(input.candidate_manifest);
    log_context.seed_transform_diagnostics(input.prior_transform_diagnostics);
    log_context.user_request_body = Some(LoggedBody::from_bytes(input.original_request_body));
    log_context.set_attempts_for_logging(input.skipped_attempts, Some(input.current_attempt));
    log_context
}

pub(in crate::proxy) fn finalize_attempt_failure_context(
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

pub(in crate::proxy) async fn maybe_record_attempt_failure(
    app_state: &Arc<AppState>,
    log_context: RequestLogContext,
    skipped_attempts: &[RequestAttemptDraft],
    attempt: &RequestAttemptDraft,
    proxy_error: &ProxyError,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
) -> RequestLogContext {
    let log_context =
        finalize_attempt_failure_context(log_context, skipped_attempts, attempt, proxy_error);
    if log_mode.should_record_attempt_failure() {
        record_completion_if_allowed(app_state, log_context.clone(), execution_policy).await;
    }
    log_context
}

pub(in crate::proxy) async fn record_completion_if_allowed(
    app_state: &Arc<AppState>,
    log_context: RequestLogContext,
    execution_policy: RuntimeExecutionPolicy,
) -> bool {
    if execution_policy.records_request_log() {
        record_request_completion_and_log(app_state, log_context).await;
        return true;
    }
    false
}

pub(in crate::proxy) async fn record_immediate_completion_if_allowed(
    app_state: &Arc<AppState>,
    log_context: &RequestLogContext,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
) {
    if log_mode.should_record_immediate() {
        record_completion_if_allowed(app_state, log_context.clone(), execution_policy).await;
    }
}

pub(in crate::proxy) async fn record_streaming_completion_if_allowed(
    app_state: &Arc<AppState>,
    log_context: &RequestLogContext,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
) {
    if log_mode.should_record_streaming() {
        record_completion_if_allowed(app_state, log_context.clone(), execution_policy).await;
    }
}

pub(in crate::proxy) fn finalize_non_streaming_log_context(
    context: &mut RequestLogContext,
    url: &str,
    status_code: StatusCode,
    completion_ts: i64,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    overall_status: RequestStatus,
    usage: Option<UsageInfo>,
    usage_normalization: Option<UsageNormalization>,
    llm_response_body: Bytes,
    user_response_body: Bytes,
) {
    context.request_url = Some(url.to_string());
    context.llm_status = Some(status_code);
    context.completion_ts = Some(completion_ts);
    context.usage = usage;
    context.usage_normalization = usage_normalization;
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = overall_status;
    context.llm_response_body = Some(LoggedBody::from_bytes(llm_response_body));
    context.user_response_body = Some(LoggedBody::from_bytes(user_response_body));
}

pub(in crate::proxy) fn finalize_streaming_log_context(
    context: &mut RequestLogContext,
    url: &str,
    status_code: StatusCode,
    completion_ts: i64,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    overall_status: RequestStatus,
    final_error: Option<&ProxyError>,
) {
    context.request_url = Some(url.to_string());
    context.llm_status = Some(status_code);
    context.completion_ts = Some(completion_ts);
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = overall_status;
    if let Some(proxy_error) = final_error {
        context.final_error_code = Some(proxy_error.error_code().to_string());
        context.final_error_message = Some(proxy_error.message().to_string());
    }
}

pub(in crate::proxy) async fn finalize_cancelled_log_context(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: Option<StatusCode>,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    llm_response_body: Option<LoggedBody>,
    user_response_body: Option<LoggedBody>,
    execution_policy: RuntimeExecutionPolicy,
) -> bool {
    let mut context = log_context.lock().await;
    context.request_url = Some(url.to_string());
    context.llm_status = status_code;
    context.completion_ts = Some(Utc::now().timestamp_millis());
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = RequestStatus::Cancelled;
    if llm_response_body.is_some() {
        context.llm_response_body = llm_response_body;
    }
    if user_response_body.is_some() {
        context.user_response_body = user_response_body;
    }
    record_completion_if_allowed(app_state, context.clone(), execution_policy).await
}

pub(in crate::proxy) async fn record_request_drop_cancellation_if_allowed(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
) {
    if !log_mode.should_record_immediate() || !execution_policy.records_request_log() {
        return;
    }

    let mut context = log_context.lock().await;
    crate::debug_event!("proxy.client_cancelled", log_id = context.id);
    context.overall_status = RequestStatus::Cancelled;
    context.completion_ts = Some(Utc::now().timestamp_millis());
    record_request_completion_and_log(app_state, context.clone()).await;
}

pub(in crate::proxy) struct NoCandidateFailureLogInput<'a> {
    pub app_state: &'a Arc<AppState>,
    pub api_key: &'a CacheApiKey,
    pub execution_plan: &'a ExecutionPlan,
    pub skipped_attempts: Vec<RequestAttemptDraft>,
    pub user_api_type: LlmApiType,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
    pub candidate_manifest: RequestLogBundleCandidateManifest,
    pub original_request_body: Bytes,
    pub start_time: i64,
    pub client_ip_addr: &'a Option<String>,
    pub message: &'a str,
    pub execution_policy: RuntimeExecutionPolicy,
}

pub(in crate::proxy) async fn record_no_candidate_failure(
    input: NoCandidateFailureLogInput<'_>,
) -> Option<RequestLogContext> {
    let first_skipped_attempt = input.skipped_attempts.first()?;

    let mut log_context = RequestLogContext::new_for_skipped_candidates(
        input.api_key,
        &input.execution_plan.requested_name,
        input.execution_plan.resolved_scope.as_str(),
        input.execution_plan.resolved_route_id,
        input.execution_plan.resolved_route_name.as_deref(),
        input.start_time,
        input.client_ip_addr,
        input.user_api_type,
        first_skipped_attempt,
    );
    log_context.set_model_resolution_trace(
        &input.execution_plan.base_requested_name,
        input.execution_plan.resolved_reasoning_suffix.as_deref(),
        input
            .execution_plan
            .resolved_reasoning_preset
            .map(|preset| preset.as_key()),
    );
    log_context.set_request_snapshot(input.request_snapshot);
    log_context.set_candidate_manifest(input.candidate_manifest);
    log_context.user_request_body = Some(LoggedBody::from_bytes(input.original_request_body));
    log_context.completion_ts = Some(Utc::now().timestamp_millis());
    log_context.overall_status = RequestStatus::Error;
    log_context.final_error_code = Some(NO_CANDIDATE_AVAILABLE_ERROR.to_string());
    log_context.final_error_message = Some(input.message.to_string());
    log_context.set_attempts_for_logging(&input.skipped_attempts, None);
    log_request_failed(
        &log_context,
        input.skipped_attempts.last(),
        &ProxyError::BadRequest(input.message.to_string()),
    );
    record_completion_if_allowed(input.app_state, log_context.clone(), input.execution_policy)
        .await;
    Some(log_context)
}

fn scheduler_action_name(action: SchedulerAction) -> &'static str {
    match action {
        SchedulerAction::ReturnSuccess => "return_success",
        SchedulerAction::FailFast => "fail_fast",
        SchedulerAction::RetrySameCandidate => "retry_same_candidate",
        SchedulerAction::FallbackNextCandidate => "fallback_next_candidate",
    }
}

fn request_latency_ms(log_context: &RequestLogContext) -> i64 {
    log_context
        .completion_ts
        .unwrap_or(log_context.request_received_at)
        .saturating_sub(log_context.request_received_at)
}

struct RequestEventBase<'a> {
    log_id: i64,
    requested_model: &'a str,
    resolved_scope: &'a str,
    route_id: Option<i64>,
    route_name: Option<&'a str>,
    upstream_status: Option<u16>,
    provider_id: i64,
    provider_key: &'a str,
    model_id: i64,
    model_name: &'a str,
    latency_ms: i64,
    candidate_position: Option<i32>,
    scheduler_action: Option<&'static str>,
    error_code: Option<&'a str>,
}

fn request_event_base<'a>(
    log_context: &'a RequestLogContext,
    attempt: Option<&'a RequestAttemptDraft>,
) -> RequestEventBase<'a> {
    RequestEventBase {
        log_id: log_context.id,
        requested_model: &log_context.requested_model_name,
        resolved_scope: &log_context.resolved_name_scope,
        route_id: log_context.resolved_route_id,
        route_name: log_context.resolved_route_name.as_deref(),
        upstream_status: log_context.llm_status.map(|status| status.as_u16()),
        provider_id: log_context.provider_id,
        provider_key: &log_context.provider_key,
        model_id: log_context.model_id,
        model_name: &log_context.model_name,
        latency_ms: request_latency_ms(log_context),
        candidate_position: attempt.map(|attempt| attempt.candidate_position),
        scheduler_action: attempt.map(|attempt| scheduler_action_name(attempt.scheduler_action)),
        error_code: attempt
            .and_then(|attempt| attempt.error_code.as_deref())
            .or(log_context.final_error_code.as_deref()),
    }
}

pub(in crate::proxy) fn log_retry_scheduled(
    log_context: &RequestLogContext,
    attempt: &RequestAttemptDraft,
    proxy_error: &ProxyError,
) {
    let event = request_event_base(log_context, Some(attempt));
    crate::warn_event!(
        "proxy.retry_scheduled",
        log_id = event.log_id,
        requested_model = event.requested_model,
        resolved_scope = event.resolved_scope,
        route_id = event.route_id,
        route_name = event.route_name,
        upstream_status = event.upstream_status,
        provider_id = event.provider_id,
        provider_key = event.provider_key,
        model_id = event.model_id,
        model_name = event.model_name,
        latency_ms = event.latency_ms,
        candidate_position = event.candidate_position,
        scheduler_action = event.scheduler_action,
        error_code = event.error_code,
        reason = proxy_error.error_code(),
        backoff_ms = attempt.backoff_ms,
    );
}

pub(in crate::proxy) fn log_fallback_next_candidate(
    log_context: &RequestLogContext,
    attempt: &RequestAttemptDraft,
    proxy_error: &ProxyError,
) {
    let event = request_event_base(log_context, Some(attempt));
    crate::warn_event!(
        "proxy.fallback_next_candidate",
        log_id = event.log_id,
        requested_model = event.requested_model,
        resolved_scope = event.resolved_scope,
        route_id = event.route_id,
        route_name = event.route_name,
        upstream_status = event.upstream_status,
        provider_id = event.provider_id,
        provider_key = event.provider_key,
        model_id = event.model_id,
        model_name = event.model_name,
        latency_ms = event.latency_ms,
        candidate_position = event.candidate_position,
        scheduler_action = event.scheduler_action,
        error_code = event.error_code,
        reason = proxy_error.error_code(),
    );
}

pub(in crate::proxy) fn log_provider_skipped(
    log_context: &RequestLogContext,
    attempt: &RequestAttemptDraft,
    proxy_error: &ProxyError,
) {
    let event = request_event_base(log_context, Some(attempt));
    crate::warn_event!(
        "proxy.provider_skipped",
        log_id = event.log_id,
        requested_model = event.requested_model,
        resolved_scope = event.resolved_scope,
        route_id = event.route_id,
        route_name = event.route_name,
        upstream_status = event.upstream_status,
        provider_id = event.provider_id,
        provider_key = event.provider_key,
        model_id = event.model_id,
        model_name = event.model_name,
        latency_ms = event.latency_ms,
        candidate_position = event.candidate_position,
        scheduler_action = event.scheduler_action,
        error_code = event.error_code,
        reason = proxy_error.error_code(),
    );
}

pub(in crate::proxy) fn log_request_failed(
    log_context: &RequestLogContext,
    attempt: Option<&RequestAttemptDraft>,
    proxy_error: &ProxyError,
) {
    let event = request_event_base(log_context, attempt);
    match proxy_error.operator_log_level() {
        ProxyLogLevel::Debug => crate::debug_event!(
            "proxy.request_failed",
            log_id = event.log_id,
            requested_model = event.requested_model,
            resolved_scope = event.resolved_scope,
            route_id = event.route_id,
            route_name = event.route_name,
            upstream_status = event.upstream_status,
            provider_id = event.provider_id,
            provider_key = event.provider_key,
            model_id = event.model_id,
            model_name = event.model_name,
            latency_ms = event.latency_ms,
            candidate_position = event.candidate_position,
            scheduler_action = event.scheduler_action,
            error_code = event.error_code,
            reason = proxy_error.error_code(),
        ),
        ProxyLogLevel::Warn => crate::warn_event!(
            "proxy.request_failed",
            log_id = event.log_id,
            requested_model = event.requested_model,
            resolved_scope = event.resolved_scope,
            route_id = event.route_id,
            route_name = event.route_name,
            upstream_status = event.upstream_status,
            provider_id = event.provider_id,
            provider_key = event.provider_key,
            model_id = event.model_id,
            model_name = event.model_name,
            latency_ms = event.latency_ms,
            candidate_position = event.candidate_position,
            scheduler_action = event.scheduler_action,
            error_code = event.error_code,
            reason = proxy_error.error_code(),
        ),
        ProxyLogLevel::Error => crate::error_event!(
            "proxy.request_failed",
            log_id = event.log_id,
            requested_model = event.requested_model,
            resolved_scope = event.resolved_scope,
            route_id = event.route_id,
            route_name = event.route_name,
            upstream_status = event.upstream_status,
            provider_id = event.provider_id,
            provider_key = event.provider_key,
            model_id = event.model_id,
            model_name = event.model_name,
            latency_ms = event.latency_ms,
            candidate_position = event.candidate_position,
            scheduler_action = event.scheduler_action,
            error_code = event.error_code,
            reason = proxy_error.error_code(),
        ),
    }
}

pub(in crate::proxy) fn append_response_transform_diagnostics(
    log_context: &mut RequestLogContext,
    diagnostics: &[UnifiedTransformDiagnostic],
) {
    log_context.append_transform_diagnostics(
        RequestLogBundleTransformDiagnosticPhase::Response,
        diagnostics,
    );
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        proxy::logging::RequestLogContext,
        proxy::requested_model::RequestedModelParseStatus,
        proxy::runtime::{
            attempt::RequestAttemptDraft,
            log_writer::{build_candidate_manifest, finalize_attempt_failure_context},
            route_resolver::{
                CandidateRuntimeFeatures, ExecutionCandidate, ExecutionPlan, ResolvedNameScope,
                RuntimeFeatureConfigSource,
            },
        },
        schema::enum_def::{
            LlmApiType, ProviderApiKeyMode, ProviderType, RequestAttemptStatus, SchedulerAction,
        },
        service::cache::types::{CacheApiKey, CacheModel, CacheProvider},
        utils::storage::RequestLogBundleRequestSnapshot,
    };
    use axum::body::Bytes;

    use super::{AttemptLogContextInput, new_attempt_log_context};

    fn test_api_key() -> CacheApiKey {
        CacheApiKey {
            id: 1,
            api_key_hash: "hash".to_string(),
            key_prefix: "cyder-prefix".to_string(),
            key_last4: "1234".to_string(),
            name: "system".to_string(),
            description: None,
            default_action: crate::schema::enum_def::Action::Allow,
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
            acl_rules: vec![],
        }
    }

    fn test_provider() -> Arc<CacheProvider> {
        Arc::new(CacheProvider {
            id: 2,
            provider_key: "provider".to_string(),
            name: "Provider".to_string(),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
        })
    }

    fn test_model() -> Arc<CacheModel> {
        Arc::new(CacheModel {
            id: 3,
            provider_id: 2,
            model_name: "gpt-test".to_string(),
            real_model_name: Some("real-gpt-test".to_string()),
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

    fn test_candidate() -> ExecutionCandidate {
        ExecutionCandidate {
            candidate_position: 1,
            route_id: Some(10),
            route_name: Some("primary".to_string()),
            route_candidate_priority: Some(1),
            provider: test_provider(),
            model: test_model(),
            llm_api_type: LlmApiType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            reasoning_config_id: None,
            reasoning_config_scope: None,
            reasoning_config_source: None,
            reasoning_config_preset_id: None,
            reasoning_family: None,
            reasoning_preset: None,
            reasoning_suffix: None,
            runtime_features: CandidateRuntimeFeatures {
                openai_reasoning_content_repair_enabled: false,
                openai_reasoning_content_repair_source: RuntimeFeatureConfigSource::DefaultFalse,
            },
        }
    }

    fn empty_snapshot() -> RequestLogBundleRequestSnapshot {
        RequestLogBundleRequestSnapshot {
            request_path: "/ai/openai/v1/chat/completions".to_string(),
            operation_kind: "chat_completions".to_string(),
            query_params: vec![],
            sanitized_original_headers: vec![],
        }
    }

    #[test]
    fn candidate_manifest_uses_execution_plan_candidates() {
        let candidate = test_candidate();
        let plan = ExecutionPlan {
            requested_name: "route-a".to_string(),
            base_requested_name: "route-a".to_string(),
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            requested_model_parse_status: RequestedModelParseStatus::Exact,
            resolved_scope: ResolvedNameScope::GlobalRoute,
            resolved_route_id: Some(10),
            resolved_route_name: Some("primary".to_string()),
            candidates: vec![candidate],
        };

        let manifest = build_candidate_manifest(&plan);

        assert_eq!(manifest.items.len(), 1);
        assert_eq!(manifest.items[0].candidate_position, 1);
        assert_eq!(manifest.items[0].route_id, Some(10));
        assert_eq!(manifest.items[0].provider_id, 2);
        assert_eq!(manifest.items[0].model_id, 3);
        assert_eq!(
            manifest.items[0].real_model_name.as_deref(),
            Some("real-gpt-test")
        );
    }

    #[test]
    fn attempt_log_context_seeds_manifest_snapshot_and_attempts() {
        let api_key = test_api_key();
        let candidate = test_candidate();
        let manifest = build_candidate_manifest(&ExecutionPlan {
            requested_name: "provider/gpt-test".to_string(),
            base_requested_name: "provider/gpt-test".to_string(),
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            requested_model_parse_status: RequestedModelParseStatus::Exact,
            resolved_scope: ResolvedNameScope::Direct,
            resolved_route_id: None,
            resolved_route_name: None,
            candidates: vec![candidate.clone()],
        });
        let attempt = RequestAttemptDraft::pending_for_candidate(&candidate);

        let context = new_attempt_log_context(AttemptLogContextInput {
            api_key: &api_key,
            candidate: &candidate,
            requested_model_name: "provider/gpt-test",
            base_requested_model_name: "provider/gpt-test",
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            resolved_name_scope: "direct",
            resolved_route_id: None,
            resolved_route_name: None,
            request_snapshot: empty_snapshot(),
            candidate_manifest: manifest,
            prior_transform_diagnostics: &[],
            original_request_body: Bytes::from_static(br#"{"model":"provider/gpt-test"}"#),
            client_ip_addr: &None,
            start_time: 1234,
            user_api_type: LlmApiType::Openai,
            current_attempt: attempt,
            skipped_attempts: &[],
        });

        assert_eq!(context.requested_model_name, "provider/gpt-test");
        assert!(context.request_snapshot.is_some());
        assert_eq!(
            context
                .candidate_manifest
                .as_ref()
                .map(|manifest| manifest.items.len()),
            Some(1)
        );
        assert_eq!(
            context.overall_status,
            crate::schema::enum_def::RequestStatus::Pending
        );
    }

    #[test]
    fn finalize_attempt_failure_marks_cancelled_or_error() {
        let api_key = test_api_key();
        let candidate = test_candidate();
        let attempt = RequestAttemptDraft {
            attempt_status: RequestAttemptStatus::Error,
            scheduler_action: SchedulerAction::FailFast,
            error_code: Some("bad_gateway".to_string()),
            ..RequestAttemptDraft::pending_for_candidate(&candidate)
        };
        let context = RequestLogContext::new(
            &api_key,
            &candidate.provider,
            &candidate.model,
            None,
            "provider/gpt-test",
            "direct",
            None,
            None,
            1234,
            &None,
            LlmApiType::Openai,
            LlmApiType::Openai,
        );

        let context = finalize_attempt_failure_context(
            context,
            &[],
            &attempt,
            &crate::proxy::ProxyError::BadGateway("upstream failed".to_string()),
        );

        assert_eq!(
            context.overall_status,
            crate::schema::enum_def::RequestStatus::Error
        );
        assert_eq!(context.final_error_code.as_deref(), Some("upstream_error"));
    }
}
