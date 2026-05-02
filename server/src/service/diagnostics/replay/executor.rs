use std::sync::Arc;

use chrono::Utc;
use reqwest::header::{CONTENT_ENCODING, CONTENT_TYPE};

use crate::{
    controller::BaseError,
    database::{
        provider::ProviderApiKey,
        request_replay_run::{RequestReplayRun, RequestReplayRunRecord},
    },
    proxy::{ProxyError, classify_upstream_status, process_success_response_body},
    schema::enum_def::{
        ProviderType, RequestAttemptStatus, RequestReplayKind, RequestReplayMode,
        RequestReplaySemanticBasis, RequestReplayStatus,
    },
    service::{
        app_state::AppState,
        cache::types::CacheProvider,
        diagnostics::{
            body::{
                REPLAY_BODY_CAPTURE_COMPLETE, REPLAY_BODY_CAPTURE_NOT_CAPTURED,
                REPLAY_BODY_CAPTURE_NOT_EXECUTED, body_from_bytes, log_capture_state_to_string,
                parse_name_values_json_map, read_replay_response_body_bounded,
                replay_body_capture_metadata, replay_body_capture_metadata_from_bytes,
                replay_response_capture_limit, serialize_headers_for_output,
            },
            policy::{stripped_preview_request_header_names, stripped_response_header_names},
            replay::artifact_store::{set_replay_artifact_locator, store_replay_artifact_for_run},
            replay::cost::apply_usage_cost_to_outcome,
            replay::diff::{
                build_attempt_replay_diff, build_gateway_replay_diff, dry_run_diff, rejected_diff,
            },
            replay::fingerprint::{
                ensure_replay_preview_confirmation_matches, parse_replay_preview_confirmation,
            },
            replay::gateway_adapter::{
                GatewayReplayExecutionFailure, GatewayReplayExecutionMetadata,
                GatewayReplayFinalAttempt, GatewayReplayPreparedRequest,
                execute_gateway_replay_request, preview_gateway_replay_request,
            },
            replay::preview::{
                ReplayResolvedCredential, build_attempt_replay_preview,
                build_gateway_replay_preview,
            },
            replay::source::{
                AttemptReplaySource, GatewayReplaySource, load_attempt_replay_source,
                load_gateway_replay_source,
            },
            replay::transport::{
                AttemptReplayExecutionOutcome, execution_outcome_from_proxy_error,
                parse_stream_usage_and_diagnostics, perform_attempt_replay_execution,
            },
        },
        storage::{Storage, get_storage},
        transform::unified::UnifiedTransformDiagnostic,
        vertex::get_vertex_token,
    },
    utils::ID_GENERATOR,
};

use crate::service::diagnostics::replay::types::{
    AttemptReplayExecuteParams, AttemptReplayPreviewParams, AttemptReplayPreviewResponse,
    GatewayReplayExecuteParams, GatewayReplayPreviewParams, GatewayReplayPreviewResponse,
    REQUEST_REPLAY_ARTIFACT_VERSION, RequestReplayArtifact, RequestReplayArtifactResult,
    RequestReplayArtifactSource, RequestReplayCandidateDecision, RequestReplayDiffBaselineKind,
    RequestReplayExecutionPreview, RequestReplayNameValue, RequestReplayResolvedCandidate,
    RequestReplayResolvedRoute,
};

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
    let policy = app_state.diagnostics.policy().await;
    let confirmation =
        parse_replay_preview_confirmation(params.preview_fingerprint.as_deref(), &policy)?;
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
    let prepared = preview_gateway_replay_request(Arc::clone(app_state), &source)
        .await
        .map_err(proxy_error_to_param_error)?;

    build_gateway_replay_preview(&source, &prepared, preview_created_at)
}

pub async fn execute_gateway_replay(
    app_state: &Arc<AppState>,
    request_log_id: i64,
    params: GatewayReplayExecuteParams,
) -> Result<RequestReplayRunRecord, BaseError> {
    let policy = app_state.diagnostics.policy().await;
    let confirmation =
        parse_replay_preview_confirmation(params.preview_fingerprint.as_deref(), &policy)?;
    let source = load_gateway_replay_source(request_log_id).await?;
    let prepared = preview_gateway_replay_request(Arc::clone(app_state), &source)
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

    let mut outcome = perform_attempt_replay_execution(app_state, source, credential).await;
    apply_usage_cost_to_outcome(&mut outcome, source.cost_catalog_version.as_ref());
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

fn gateway_replay_status_from_attempt(attempt: &GatewayReplayFinalAttempt) -> RequestReplayStatus {
    match attempt.attempt_status {
        RequestAttemptStatus::Success => RequestReplayStatus::Success,
        RequestAttemptStatus::Cancelled => RequestReplayStatus::Cancelled,
        RequestAttemptStatus::Error | RequestAttemptStatus::Skipped => RequestReplayStatus::Error,
    }
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
        !stripped_preview_request_header_names().contains(&normalized_name.as_str())
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
        !stripped_response_header_names().contains(&normalized_name.as_str())
    })
    .collect()
}

fn execution_preview_from_gateway_metadata(
    metadata: &GatewayReplayExecutionMetadata,
) -> RequestReplayExecutionPreview {
    let final_attempt = &metadata.final_attempt;
    RequestReplayExecutionPreview {
        semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
        requested_model_name: Some(metadata.requested_model_name.clone()),
        base_requested_model_name: Some(metadata.base_requested_model_name.clone()),
        resolved_reasoning_suffix: metadata.resolved_reasoning_suffix.clone(),
        resolved_reasoning_preset: metadata.resolved_reasoning_preset.clone(),
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
        candidate_decisions: metadata.candidate_decisions.clone(),
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
            requested_model_name: None,
            base_requested_model_name: None,
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            resolved_route: None,
            resolved_candidate: None,
            candidate_decisions: failure.candidate_decisions.clone(),
            applied_request_patch_summary: None,
            final_request_uri: None,
            final_request_headers: Vec::new(),
            final_request_body: None,
        })
}

fn gateway_live_outcome_from_failure(
    failure: GatewayReplayExecutionFailure,
    capture_limit_bytes: usize,
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
                capture_limit_bytes,
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
    let policy = app_state.diagnostics.policy().await;
    let capture_limit_bytes = replay_response_capture_limit(&policy);
    let execution = match execute_gateway_replay_request(Arc::clone(app_state), source).await {
        Ok(execution) => execution,
        Err(failure) => return gateway_live_outcome_from_failure(failure, capture_limit_bytes),
    };

    let first_byte_at = Some(Utc::now().timestamp_millis());
    let execution_preview = execution_preview_from_gateway_metadata(&execution.metadata);
    let attempt_timeline = execution_preview.candidate_decisions.clone();
    let status_code = execution.response.status();
    let response_headers = serialize_headers_for_output(
        execution.response.headers(),
        stripped_response_header_names(),
    );
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
        capture_limit_bytes,
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
            .catalog
            .get_cost_catalog_version_by_model(model_id, Utc::now().timestamp_millis())
            .await
            .ok()
            .flatten()
            .map(|version| (*version).clone()),
        None => None,
    };
    if status_code.is_success() {
        let mut outcome = AttemptReplayExecutionOutcome {
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
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        };
        apply_usage_cost_to_outcome(&mut outcome, cost_catalog_version.as_ref());
        GatewayReplayLiveOutcome {
            execution_preview,
            attempt_timeline,
            outcome,
        }
    } else {
        let proxy_error = classify_upstream_status(status_code, &body_bytes);
        let mut outcome = AttemptReplayExecutionOutcome {
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
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        };
        apply_usage_cost_to_outcome(&mut outcome, cost_catalog_version.as_ref());
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
            app_state.infra.proxy_client().await.as_ref(),
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

fn proxy_error_to_param_error(error: ProxyError) -> BaseError {
    BaseError::ParamInvalid(Some(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_execute_mode_defaults_to_live_and_preserves_explicit_dry_run() {
        assert_eq!(replay_execute_mode(None), RequestReplayMode::Live);
        assert_eq!(
            replay_execute_mode(Some(RequestReplayMode::DryRun)),
            RequestReplayMode::DryRun
        );
    }

    #[test]
    fn dry_run_result_marks_response_as_not_executed() {
        let result = dry_run_result(Vec::new(), Vec::new());

        assert_eq!(result.status, RequestReplayStatus::Success);
        assert_eq!(result.http_status, None);
        assert_eq!(
            result.response_body_capture_state.as_deref(),
            Some(REPLAY_BODY_CAPTURE_NOT_EXECUTED)
        );
        assert!(result.response_body.is_none());
    }

    #[test]
    fn gateway_final_attempt_status_maps_to_replay_terminal_status() {
        assert_eq!(
            gateway_replay_status_from_attempt(&final_attempt(RequestAttemptStatus::Success)),
            RequestReplayStatus::Success
        );
        assert_eq!(
            gateway_replay_status_from_attempt(&final_attempt(RequestAttemptStatus::Cancelled)),
            RequestReplayStatus::Cancelled
        );
        assert_eq!(
            gateway_replay_status_from_attempt(&final_attempt(RequestAttemptStatus::Error)),
            RequestReplayStatus::Error
        );
        assert_eq!(
            gateway_replay_status_from_attempt(&final_attempt(RequestAttemptStatus::Skipped)),
            RequestReplayStatus::Error
        );
    }

    fn final_attempt(attempt_status: RequestAttemptStatus) -> GatewayReplayFinalAttempt {
        GatewayReplayFinalAttempt {
            candidate_position: 1,
            provider_id: None,
            provider_api_key_id: None,
            model_id: None,
            llm_api_type: None,
            attempt_status,
            error_code: None,
            error_message: None,
            request_uri: None,
            request_headers_json: None,
            request_body: None,
            request_body_capture_state: None,
            response_headers_json: None,
            response_body: None,
            response_body_capture_state: None,
            http_status: None,
            first_byte_at: None,
            applied_request_patch_summary: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        }
    }
}
