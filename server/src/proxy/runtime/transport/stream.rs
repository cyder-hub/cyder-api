use std::time::Duration;
use std::{collections::BTreeMap, sync::Arc};

use axum::{
    body::{Body, Bytes},
    http::StatusCode,
    response::Response,
};
use chrono::Utc;
use cyder_tools::log::{debug, error};
use futures::StreamExt;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex as TokioMutex, mpsc},
    time::timeout,
};

use super::{
    ReasoningContinuationCaptureContext, cancellation::ResponseStreamCancellationGuard,
    response::build_response_builder,
};
use crate::{
    proxy::{
        ProxyError,
        cancellation::ProxyCancellationContext,
        classify_upstream_status,
        logging::{LogBodyKind, LoggedBody, RequestLogContext, StreamingBodyWriter},
        protocol_transform_error,
        provider_governance::{record_provider_failure, record_provider_success},
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::{
                append_response_transform_diagnostics, finalize_cancelled_log_context,
                finalize_streaming_log_context, record_streaming_completion_if_allowed,
            },
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
            reasoning_content_repair::{
                ReasoningContentRepairResultKey, continuation_snapshot_from_parts,
            },
        },
    },
    schema::enum_def::{LlmApiType, RequestStatus},
    service::{
        app_state::AppState,
        cache::types::CacheCostCatalogVersion,
        runtime::{
            ProviderCircuitProbePermit, ReasoningContinuationCacheKey, ReasoningContinuationScope,
        },
        transform::{
            StreamTransformer,
            unified::{
                UnifiedTransformDiagnostic, UnifiedTransformDiagnosticAction,
                UnifiedTransformDiagnosticKind, UnifiedTransformDiagnosticLossLevel,
            },
        },
    },
    utils::{
        sse::{SseEvent, SseParser},
        storage::LogBodyCaptureState,
    },
};

const POST_DONE_UPSTREAM_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const POST_DONE_UPSTREAM_DRAIN_MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Default, Serialize)]
struct PostDoneUpstreamDrainReport {
    same_chunk_ignored_events: usize,
    observed_chunks: usize,
    observed_bytes: usize,
    reached_eof: bool,
    timed_out: bool,
    max_bytes_reached: bool,
    upstream_error: Option<String>,
    write_error: Option<String>,
}

impl PostDoneUpstreamDrainReport {
    fn capture_state(&self) -> LogBodyCaptureState {
        if self.reached_eof
            && !self.timed_out
            && !self.max_bytes_reached
            && self.upstream_error.is_none()
            && self.write_error.is_none()
        {
            LogBodyCaptureState::Complete
        } else {
            LogBodyCaptureState::Incomplete
        }
    }

    fn should_record_diagnostic(&self) -> bool {
        self.same_chunk_ignored_events > 0
            || self.observed_chunks > 0
            || self.timed_out
            || self.max_bytes_reached
            || self.upstream_error.is_some()
            || self.write_error.is_some()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct StreamReasoningContentCaptureReport {
    pub captured_count: usize,
    pub diagnostics: Vec<StreamReasoningContentCaptureDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct StreamReasoningContentCaptureDiagnostic {
    pub result: ReasoningContentRepairResultKey,
    pub captured_count: usize,
    pub tool_call_ids: Vec<String>,
    pub tool_calls_hash: Option<String>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct OpenAiReasoningStreamCapture {
    scope: Option<ReasoningContinuationScope>,
    feature_enabled: bool,
    target_is_openai_compatible_generation: bool,
    choices: BTreeMap<u32, StreamChoiceCapture>,
    parse_failed_count: usize,
}

#[derive(Clone, Debug, Default)]
struct StreamChoiceCapture {
    reasoning_content: String,
    tool_calls: BTreeMap<u32, PartialToolCall>,
    invalid: bool,
}

#[derive(Clone, Debug, Default)]
struct PartialToolCall {
    id: Option<String>,
    type_: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl OpenAiReasoningStreamCapture {
    pub(super) fn new(
        capture_context: Option<ReasoningContinuationCaptureContext>,
        target_api_type: LlmApiType,
    ) -> Self {
        let (scope, feature_enabled) = match capture_context {
            Some(context) => (Some(context.scope), context.feature_enabled),
            None => (None, false),
        };
        Self {
            scope,
            feature_enabled,
            target_is_openai_compatible_generation: matches!(
                target_api_type,
                LlmApiType::Openai | LlmApiType::GeminiOpenai
            ),
            choices: BTreeMap::new(),
            parse_failed_count: 0,
        }
    }

    pub(super) fn observe_events(&mut self, events: &[SseEvent]) {
        if !self.feature_enabled || !self.target_is_openai_compatible_generation {
            return;
        }

        for event in events {
            let data = event.data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(data) else {
                self.parse_failed_count += 1;
                continue;
            };
            self.observe_chunk_value(&value);
        }
    }

    pub(super) async fn finish(
        self,
        app_state: &Arc<AppState>,
        observed_at_ms: i64,
    ) -> StreamReasoningContentCaptureReport {
        if !self.feature_enabled {
            return stream_single_capture_result(ReasoningContentRepairResultKey::Disabled, None);
        }
        if !self.target_is_openai_compatible_generation {
            return stream_single_capture_result(
                ReasoningContentRepairResultKey::NotApplicable,
                None,
            );
        }
        if self.parse_failed_count > 0 {
            return stream_single_capture_result(
                ReasoningContentRepairResultKey::ParseFailed,
                Some(format!("parse_failed_count={}", self.parse_failed_count)),
            );
        }

        let Some(scope) = self.scope.clone() else {
            return stream_single_capture_result(ReasoningContentRepairResultKey::Disabled, None);
        };
        let snapshots = self.snapshots(scope, observed_at_ms);
        if snapshots.is_empty() {
            return stream_single_capture_result(
                ReasoningContentRepairResultKey::NotApplicable,
                None,
            );
        }

        let mut report = StreamReasoningContentCaptureReport::default();
        for snapshot in snapshots {
            let key = snapshot.key.clone();
            match app_state
                .reasoning_continuation_store
                .insert(snapshot, observed_at_ms)
                .await
            {
                Ok(()) => {
                    report.captured_count += 1;
                    report.diagnostics.push(stream_capture_diagnostic_for_key(
                        ReasoningContentRepairResultKey::Matched,
                        1,
                        &key,
                        None,
                    ));
                }
                Err(err) => report.diagnostics.push(stream_capture_diagnostic_for_key(
                    ReasoningContentRepairResultKey::CacheMiss,
                    0,
                    &key,
                    Some(format!("store_error={err}")),
                )),
            }
        }

        report
    }

    fn observe_chunk_value(&mut self, value: &Value) {
        let Some(choices) = value
            .as_object()
            .and_then(|chunk| chunk.get("choices"))
            .and_then(Value::as_array)
        else {
            self.parse_failed_count += 1;
            return;
        };

        for choice in choices {
            let choice_index = choice
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|index| u32::try_from(index).ok())
                .unwrap_or(0);
            let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
                continue;
            };
            let choice_capture = self.choices.entry(choice_index).or_default();

            if let Some(reasoning_content) = delta
                .get("reasoning_content")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                choice_capture.reasoning_content.push_str(reasoning_content);
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for tool_call in tool_calls {
                    if !choice_capture.observe_tool_call_delta(tool_call) {
                        choice_capture.invalid = true;
                    }
                }
            }
        }
    }

    fn snapshots(
        self,
        scope: ReasoningContinuationScope,
        observed_at_ms: i64,
    ) -> Vec<crate::service::runtime::ReasoningContinuationSnapshot> {
        let mut snapshots = Vec::new();
        for choice in self.choices.into_values() {
            if choice.reasoning_content.is_empty() || choice.invalid {
                continue;
            }
            let Some(tool_calls) = choice.tool_calls_value() else {
                continue;
            };
            match continuation_snapshot_from_parts(
                scope.clone(),
                &choice.reasoning_content,
                &tool_calls,
                observed_at_ms,
            ) {
                Ok(Some(snapshot)) => snapshots.push(snapshot),
                Ok(None) | Err(_) => {}
            }
        }
        snapshots
    }
}

impl StreamChoiceCapture {
    fn observe_tool_call_delta(&mut self, tool_call: &Value) -> bool {
        let Some(index) = tool_call
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|index| u32::try_from(index).ok())
        else {
            return false;
        };
        let partial = self.tool_calls.entry(index).or_default();

        if let Some(id) = tool_call
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            if partial.id.as_deref().is_some_and(|existing| existing != id) {
                return false;
            }
            partial.id = Some(id.to_string());
        }
        if let Some(type_) = tool_call
            .get("type")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            partial.type_ = Some(type_.to_string());
        }
        if let Some(function) = tool_call.get("function").and_then(Value::as_object) {
            if let Some(name_delta) = function
                .get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                partial
                    .name
                    .get_or_insert_with(String::new)
                    .push_str(name_delta);
            }
            if let Some(arguments_delta) = function.get("arguments").and_then(Value::as_str) {
                partial.arguments.push_str(arguments_delta);
            }
        }

        true
    }

    fn tool_calls_value(&self) -> Option<Value> {
        if self.tool_calls.is_empty() {
            return None;
        }

        let mut tool_calls = Vec::with_capacity(self.tool_calls.len());
        for partial in self.tool_calls.values() {
            let id = partial.id.as_deref().filter(|value| !value.is_empty())?;
            let name = partial.name.as_deref().filter(|value| !value.is_empty())?;
            tool_calls.push(json!({
                "id": id,
                "type": partial.type_.as_deref().unwrap_or("function"),
                "function": {
                    "name": name,
                    "arguments": partial.arguments,
                }
            }));
        }

        Some(Value::Array(tool_calls))
    }
}

fn stream_single_capture_result(
    result: ReasoningContentRepairResultKey,
    detail: Option<String>,
) -> StreamReasoningContentCaptureReport {
    StreamReasoningContentCaptureReport {
        captured_count: 0,
        diagnostics: vec![StreamReasoningContentCaptureDiagnostic {
            result,
            captured_count: 0,
            tool_call_ids: Vec::new(),
            tool_calls_hash: None,
            detail,
        }],
    }
}

fn stream_capture_diagnostic_for_key(
    result: ReasoningContentRepairResultKey,
    captured_count: usize,
    key: &ReasoningContinuationCacheKey,
    detail: Option<String>,
) -> StreamReasoningContentCaptureDiagnostic {
    StreamReasoningContentCaptureDiagnostic {
        result,
        captured_count,
        tool_call_ids: key.tool_call_ids.clone(),
        tool_calls_hash: Some(key.tool_calls_hash.clone()),
        detail,
    }
}

pub(super) fn stream_capture_transform_diagnostics(
    report: &StreamReasoningContentCaptureReport,
) -> Vec<UnifiedTransformDiagnostic> {
    report
        .diagnostics
        .iter()
        .map(stream_capture_transform_diagnostic)
        .collect()
}

fn stream_capture_transform_diagnostic(
    diagnostic: &StreamReasoningContentCaptureDiagnostic,
) -> UnifiedTransformDiagnostic {
    UnifiedTransformDiagnostic {
        type_: "runtime_feature_diagnostic".to_string(),
        diagnostic_kind: UnifiedTransformDiagnosticKind::CapabilityDowngrade,
        provider: "openai_compatible".to_string(),
        target_provider: "openai_compatible".to_string(),
        source: "upstream_response_stream".to_string(),
        target: "continuation_cache".to_string(),
        stream_id: None,
        stage: Some("response_capture".to_string()),
        loss_level: UnifiedTransformDiagnosticLossLevel::Lossless,
        action: UnifiedTransformDiagnosticAction::Send,
        semantic_unit: "reasoning_content".to_string(),
        reason: format!(
            "openai_reasoning_content_capture:{}",
            diagnostic.result.as_key()
        ),
        context: serde_json::to_string(diagnostic).ok(),
        raw_data_summary: None,
        recovery_hint: None,
    }
}

pub(super) async fn sync_stream_usage_to_log_context(
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    transformer: &mut StreamTransformer,
) {
    let usage = transformer.cached_usage_info();
    let usage_normalization = transformer.cached_usage_normalization();
    let diagnostics = transformer.diagnostics_snapshot();

    if usage.is_none() && usage_normalization.is_none() && diagnostics.is_empty() {
        return;
    }

    let mut context = log_context.lock().await;
    context.usage = usage;
    context.usage_normalization = usage_normalization;
    context.replace_transform_diagnostics_phase(
        crate::utils::storage::RequestLogBundleTransformDiagnosticPhase::Stream,
        &diagnostics,
    );
}

pub(super) async fn mark_stream_response_started_to_client(
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    transformed_chunk: &Bytes,
) {
    if transformed_chunk.is_empty() {
        return;
    }

    let mut context = log_context.lock().await;
    if context.first_chunk_ts.is_none() {
        context.first_chunk_ts = Some(Utc::now().timestamp_millis());
    }
}

pub(super) fn next_stream_chunk_timeout_duration(
    first_chunk_received_at_proxy: i64,
    first_byte_timeout: Option<Duration>,
) -> Option<Duration> {
    if first_chunk_received_at_proxy == 0 {
        first_byte_timeout
    } else {
        None
    }
}

async fn finish_incomplete_stream_body(
    writer: &mut Option<StreamingBodyWriter>,
) -> Option<LoggedBody> {
    match writer.take() {
        Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
        None => None,
    }
}

fn is_downstream_openai_done_event(api_type: LlmApiType, event: &SseEvent) -> bool {
    api_type == LlmApiType::Openai && event.data.trim() == "[DONE]"
}

fn append_transformed_event_bytes(
    target_api_type: LlmApiType,
    event: &SseEvent,
    output: &mut Vec<u8>,
) {
    if target_api_type == LlmApiType::Ollama {
        output.extend_from_slice(event.data.as_bytes());
        output.push(b'\n');
    } else {
        output.extend_from_slice(&event.to_bytes());
    }
}

async fn drain_upstream_after_openai_done(
    rx: &mut mpsc::Receiver<Result<Bytes, reqwest::Error>>,
    writer: &mut StreamingBodyWriter,
    same_chunk_ignored_events: usize,
) -> PostDoneUpstreamDrainReport {
    let mut report = PostDoneUpstreamDrainReport {
        same_chunk_ignored_events,
        ..Default::default()
    };
    let timed_out = timeout(POST_DONE_UPSTREAM_DRAIN_TIMEOUT, async {
        loop {
            let Some(chunk_result) = rx.recv().await else {
                report.reached_eof = true;
                break;
            };

            match chunk_result {
                Ok(chunk) => {
                    if chunk.is_empty() {
                        continue;
                    }
                    let next_observed_bytes = report.observed_bytes.saturating_add(chunk.len());
                    if next_observed_bytes > POST_DONE_UPSTREAM_DRAIN_MAX_BYTES {
                        report.max_bytes_reached = true;
                        break;
                    }
                    if let Err(err) = writer.append(&chunk).await {
                        report.write_error = Some(err.to_string());
                        break;
                    }
                    report.observed_chunks += 1;
                    report.observed_bytes = next_observed_bytes;
                }
                Err(err) => {
                    report.upstream_error = Some(err.to_string());
                    break;
                }
            }
        }
    })
    .await
    .is_err();
    report.timed_out = timed_out;
    report
}

fn post_done_drain_transform_diagnostic(
    report: &PostDoneUpstreamDrainReport,
) -> Option<UnifiedTransformDiagnostic> {
    if !report.should_record_diagnostic() {
        return None;
    }

    Some(UnifiedTransformDiagnostic {
        type_: "runtime_feature_diagnostic".to_string(),
        diagnostic_kind: UnifiedTransformDiagnosticKind::CapabilityDowngrade,
        provider: "openai_compatible".to_string(),
        target_provider: "openai".to_string(),
        source: "upstream_response_stream".to_string(),
        target: "downstream_response_stream".to_string(),
        stream_id: None,
        stage: Some("post_done_drain".to_string()),
        loss_level: UnifiedTransformDiagnosticLossLevel::Lossless,
        action: UnifiedTransformDiagnosticAction::Drop,
        semantic_unit: "post_done_upstream_frame".to_string(),
        reason: "ignored OpenAI-compatible upstream data after data: [DONE] while completing downstream response at the DONE boundary".to_string(),
        context: serde_json::to_string(report).ok(),
        raw_data_summary: None,
        recovery_hint: Some(
            "Inspect llm_response for upstream-only post-DONE frames; user_response intentionally ends at data: [DONE].".to_string(),
        ),
    })
}

async fn finalize_openai_done_stream_after_drain(
    app_state: Arc<AppState>,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    mut rx: mpsc::Receiver<Result<Bytes, reqwest::Error>>,
    mut llm_body_writer: Option<StreamingBodyWriter>,
    mut user_body_writer: Option<StreamingBodyWriter>,
    mut transformer: StreamTransformer,
    reasoning_stream_capture: OpenAiReasoningStreamCapture,
    url: String,
    status_code: StatusCode,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
    model_str: String,
    completed_at: i64,
    same_chunk_ignored_events: usize,
) {
    let drain_report = match llm_body_writer.as_mut() {
        Some(writer) => {
            drain_upstream_after_openai_done(&mut rx, writer, same_chunk_ignored_events).await
        }
        None => PostDoneUpstreamDrainReport {
            same_chunk_ignored_events,
            reached_eof: true,
            ..Default::default()
        },
    };
    let llm_capture_state = drain_report.capture_state();
    let llm_response_body = match llm_body_writer.take() {
        Some(writer) => writer.finish(llm_capture_state).await.ok(),
        None => None,
    };
    let user_response_body = match user_body_writer.take() {
        Some(writer) => writer.finish(LogBodyCaptureState::Complete).await.ok(),
        None => None,
    };

    let reasoning_capture_report = reasoning_stream_capture
        .finish(&app_state, completed_at)
        .await;
    let reasoning_capture_diagnostics =
        stream_capture_transform_diagnostics(&reasoning_capture_report);
    let usage = transformer.parse_usage_info();
    let usage_normalization = transformer.parse_usage_normalization();
    let mut stream_diagnostics = transformer.diagnostics_snapshot();
    if let Some(diagnostic) = post_done_drain_transform_diagnostic(&drain_report) {
        stream_diagnostics.push(diagnostic);
    }

    let mut context = log_context.lock().await;
    finalize_streaming_log_context(
        &mut context,
        &url,
        status_code,
        completed_at,
        cost_catalog_version.as_ref(),
        RequestStatus::Success,
        None,
    );
    context.llm_response_body = llm_response_body;
    context.user_response_body = user_response_body;
    context.usage = usage;
    context.usage_normalization = usage_normalization;
    context.replace_transform_diagnostics_phase(
        crate::utils::storage::RequestLogBundleTransformDiagnosticPhase::Stream,
        &stream_diagnostics,
    );
    append_response_transform_diagnostics(&mut context, &reasoning_capture_diagnostics);
    record_streaming_completion_if_allowed(&app_state, &context, log_mode, execution_policy).await;

    crate::debug_event!(
        "proxy.request_succeeded_debug",
        log_id = context.id,
        model = &model_str,
        status_code = status_code.as_u16(),
        is_stream = true,
        latency_ms = completed_at.saturating_sub(context.request_received_at),
    );
}

async fn finalize_streaming_error(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    llm_body_writer: &mut Option<StreamingBodyWriter>,
    user_body_writer: &mut Option<StreamingBodyWriter>,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    proxy_error: &ProxyError,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
) {
    let llm_response_body = finish_incomplete_stream_body(llm_body_writer).await;
    let user_response_body = finish_incomplete_stream_body(user_body_writer).await;
    let mut context = log_context.lock().await;
    finalize_streaming_log_context(
        &mut context,
        url,
        status_code,
        Utc::now().timestamp_millis(),
        cost_catalog_version,
        RequestStatus::Error,
        Some(proxy_error),
    );
    context.llm_response_body = llm_response_body;
    context.user_response_body = user_response_body;
    record_streaming_completion_if_allowed(app_state, &context, log_mode, execution_policy).await;
}

async fn abort_and_finalize_cancelled_stream(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    llm_body_writer: &mut Option<StreamingBodyWriter>,
    user_body_writer: &mut Option<StreamingBodyWriter>,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    execution_policy: RuntimeExecutionPolicy,
) {
    let llm_response_body = finish_incomplete_stream_body(llm_body_writer).await;
    let user_response_body = finish_incomplete_stream_body(user_body_writer).await;
    if execution_policy.records_request_log() {
        finalize_cancelled_log_context(
            app_state,
            log_context,
            url,
            Some(status_code),
            cost_catalog_version,
            llm_response_body,
            user_response_body,
            execution_policy,
        )
        .await;
    }
}

pub(super) async fn handle_streaming_response(
    app_state: &Arc<AppState>,
    cancellation: ProxyCancellationContext,
    provider_id: i64,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    mut api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    provider_circuit_permit: Option<ProviderCircuitProbePermit>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    reasoning_capture: Option<ReasoningContinuationCaptureContext>,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
    first_byte_timeout: Option<Duration>,
) -> Result<Response<Body>, ProxyError> {
    let status_code = response.status();
    let response_headers = response.headers().clone();
    let log_id = log_context.lock().await.id;
    let response_builder = build_response_builder(status_code, &response_headers);

    let (tx, mut rx) = mpsc::channel::<Result<bytes::Bytes, reqwest::Error>>(10);

    let url_owned = url.to_string();
    let cost_catalog_version_clone = cost_catalog_version.clone();
    let app_state_clone = Arc::clone(app_state);

    let cancellation_for_reader = cancellation.clone();
    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        loop {
            tokio::select! {
                _ = cancellation_for_reader.cancelled() => break,
                maybe_chunk = stream.next() => {
                    let Some(chunk_result) = maybe_chunk else {
                        break;
                    };
                    if tx.send(chunk_result).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut transformer = StreamTransformer::new(target_api_type, api_type);
    let mut parser = SseParser::new();
    let log_context_clone = log_context.clone();
    let llm_body_writer = match StreamingBodyWriter::new(LogBodyKind::LlmResponse, log_id).await {
        Ok(writer) => writer,
        Err(e) => {
            let proxy_error =
                ProxyError::InternalError(format!("Failed to create LLM stream spool writer: {e}"));
            api_key_request_lease.release().await;
            return Err(proxy_error);
        }
    };
    let user_body_writer = match StreamingBodyWriter::new(LogBodyKind::UserResponse, log_id).await {
        Ok(writer) => writer,
        Err(e) => {
            let proxy_error = ProxyError::InternalError(format!(
                "Failed to create user stream spool writer: {e}"
            ));
            api_key_request_lease.release().await;
            return Err(proxy_error);
        }
    };

    let monitored_stream = async_stream::stream! {
        let mut api_key_request_lease = api_key_request_lease;
        let provider_circuit_permit = provider_circuit_permit;
        let mut response_drop_guard = ResponseStreamCancellationGuard::new(
            Arc::clone(&app_state_clone),
            cancellation.clone(),
            log_context_clone.clone(),
            url_owned.clone(),
            status_code,
            cost_catalog_version_clone.clone(),
            execution_policy,
            format!("Client disconnected while receiving streaming response for log_id {}.", log_id),
        );
        let mut first_chunk_received_at_proxy: i64 = 0;
        let mut llm_body_writer = Some(llm_body_writer);
        let mut user_body_writer = Some(user_body_writer);
        let mut reasoning_stream_capture =
            OpenAiReasoningStreamCapture::new(reasoning_capture, target_api_type);

        loop {
            let chunk_result = match next_stream_chunk_timeout_duration(first_chunk_received_at_proxy, first_byte_timeout) {
                Some(timeout_duration) => match tokio::select! {
                    _ = cancellation.cancelled() => Err(cancellation.cancellation_error().await),
                    result = timeout(timeout_duration, rx.recv()) => Ok(result),
                } {
                    Err(proxy_error) => {
                        response_drop_guard.disarm();
                        abort_and_finalize_cancelled_stream(
                            &app_state_clone,
                            &log_context_clone,
                            &mut llm_body_writer,
                            &mut user_body_writer,
                            &url_owned,
                            status_code,
                            cost_catalog_version_clone.as_ref(),
                            execution_policy,
                        ).await;
                        api_key_request_lease.release().await;
                        yield Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, proxy_error.to_string()));
                        return;
                    }
                    Ok(result) => match result {
                        Ok(result) => result,
                        Err(_) => {
                            response_drop_guard.disarm();
                            let stream_error_message = format!(
                                "LLM stream timed out waiting for the first chunk after {:?}",
                                timeout_duration
                            );
                            error!("{}", stream_error_message);
                            let proxy_error = ProxyError::UpstreamTimeout(stream_error_message.clone());
                            finalize_streaming_error(
                                &app_state_clone,
                                &log_context_clone,
                                &mut llm_body_writer,
                                &mut user_body_writer,
                                &url_owned,
                                status_code,
                                cost_catalog_version_clone.as_ref(),
                                &proxy_error,
                                log_mode,
                                execution_policy,
                            )
                            .await;
                            if execution_policy.records_provider_runtime() {
                                record_provider_failure(
                                    &app_state_clone,
                                    provider_id,
                                    &model_str,
                                    &proxy_error,
                                    provider_circuit_permit.as_ref(),
                                )
                                .await;
                            }

                            api_key_request_lease.release().await;
                            yield Err(std::io::Error::new(std::io::ErrorKind::TimedOut, stream_error_message));
                            return;
                        }
                    },
                },
                None => {
                    tokio::select! {
                        _ = cancellation.cancelled() => {
                            response_drop_guard.disarm();
                            abort_and_finalize_cancelled_stream(
                                &app_state_clone,
                                &log_context_clone,
                                &mut llm_body_writer,
                                &mut user_body_writer,
                                &url_owned,
                                status_code,
                                cost_catalog_version_clone.as_ref(),
                                execution_policy,
                            ).await;
                            api_key_request_lease.release().await;
                            yield Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, cancellation.cancellation_error().await.to_string()));
                            return;
                        }
                        result = rx.recv() => result,
                    }
                }
            };

            let Some(chunk_result) = chunk_result else {
                break;
            };

            match chunk_result {
                Ok(chunk) => {
                    if let Err(e) = llm_body_writer.as_mut().expect("llm stream writer should exist").append(&chunk).await {
                        response_drop_guard.disarm();
                        let stream_error_message = format!("Failed to persist LLM stream chunk: {}", e);
                        error!("{}", stream_error_message);
                        let proxy_error = ProxyError::InternalError(stream_error_message.clone());
                        finalize_streaming_error(
                            &app_state_clone,
                            &log_context_clone,
                            &mut llm_body_writer,
                            &mut user_body_writer,
                            &url_owned,
                            status_code,
                            cost_catalog_version_clone.as_ref(),
                            &proxy_error,
                            log_mode,
                            execution_policy,
                        )
                        .await;
                        if execution_policy.records_provider_runtime() {
                            record_provider_failure(
                                &app_state_clone,
                                provider_id,
                                &model_str,
                                &proxy_error,
                                provider_circuit_permit.as_ref(),
                            )
                            .await;
                        }

                        api_key_request_lease.release().await;
                        yield Err(std::io::Error::other(stream_error_message));
                        return;
                    }

                    {
                        let mut context = log_context_clone.lock().await;
                        if execution_policy.records_request_log() {
                            llm_body_writer
                                .as_mut()
                                .expect("llm stream writer should exist")
                                .preserve_on_drop();
                        }
                        context.llm_response_body =
                            Some(llm_body_writer.as_ref().expect("llm stream writer should exist").snapshot(LogBodyCaptureState::Incomplete));
                    }

                    if first_chunk_received_at_proxy == 0 {
                        first_chunk_received_at_proxy = Utc::now().timestamp_millis();
                    }

                    let events = parser.process(&chunk);
                    if events.is_empty() {
                        continue;
                    }

                    let mut transformed_chunk_bytes: Vec<u8> = Vec::new();
                    let mut downstream_openai_done = false;

                    let parsed_event_count = events.len();
                    let mut processed_event_count = 0usize;
                    for event in events {
                        processed_event_count += 1;
                        reasoning_stream_capture.observe_events(std::slice::from_ref(&event));
                        let transformed_events =
                            transformer.transform_event(event).unwrap_or_default();
                        for transformed_event in transformed_events {
                            append_transformed_event_bytes(
                                api_type,
                                &transformed_event,
                                &mut transformed_chunk_bytes,
                            );
                            if is_downstream_openai_done_event(api_type, &transformed_event) {
                                downstream_openai_done = true;
                                break;
                            }
                        }
                        if downstream_openai_done {
                            break;
                        }
                    }
                    sync_stream_usage_to_log_context(&log_context_clone, &mut transformer).await;

                    let transformed_chunk = Bytes::from(transformed_chunk_bytes);
                    if let Err(e) = user_body_writer.as_mut().expect("user stream writer should exist").append(&transformed_chunk).await {
                        response_drop_guard.disarm();
                        let stream_error_message =
                            format!("Failed to persist transformed stream chunk: {}", e);
                        error!("{}", stream_error_message);
                        let proxy_error = ProxyError::InternalError(stream_error_message.clone());
                        finalize_streaming_error(
                            &app_state_clone,
                            &log_context_clone,
                            &mut llm_body_writer,
                            &mut user_body_writer,
                            &url_owned,
                            status_code,
                            cost_catalog_version_clone.as_ref(),
                            &proxy_error,
                            log_mode,
                            execution_policy,
                        )
                        .await;
                        if execution_policy.records_provider_runtime() {
                            record_provider_failure(
                                &app_state_clone,
                                provider_id,
                                &model_str,
                                &proxy_error,
                                provider_circuit_permit.as_ref(),
                            )
                            .await;
                        }

                        api_key_request_lease.release().await;
                        yield Err(std::io::Error::other(stream_error_message));
                        return;
                    }

                    {
                        let mut context = log_context_clone.lock().await;
                        if execution_policy.records_request_log() {
                            user_body_writer
                                .as_mut()
                                .expect("user stream writer should exist")
                                .preserve_on_drop();
                        }
                        context.user_response_body =
                            Some(user_body_writer.as_ref().expect("user stream writer should exist").snapshot(LogBodyCaptureState::Incomplete));
                    }

                    if !transformed_chunk.is_empty() {
                        mark_stream_response_started_to_client(
                            &log_context_clone,
                            &transformed_chunk,
                        )
                        .await;
                        if downstream_openai_done {
                            let same_chunk_ignored_events =
                                parsed_event_count.saturating_sub(processed_event_count);
                            let done_completed_at = Utc::now().timestamp_millis();
                            response_drop_guard.disarm();
                            api_key_request_lease.release().await;
                            if execution_policy.records_provider_runtime() {
                                record_provider_success(
                                    &app_state_clone,
                                    provider_id,
                                    &model_str,
                                    provider_circuit_permit.as_ref(),
                                )
                                .await;
                            }

                            let drain_app_state = Arc::clone(&app_state_clone);
                            let drain_log_context = Arc::clone(&log_context_clone);
                            let drain_url = url_owned.clone();
                            let drain_cost_catalog_version = cost_catalog_version_clone.clone();
                            let drain_model_str = model_str.clone();
                            let drain_llm_body_writer = llm_body_writer.take();
                            let drain_user_body_writer = user_body_writer.take();
                            let drain_transformer = transformer;
                            let drain_reasoning_stream_capture = reasoning_stream_capture;
                            app_state_clone.infra.spawn_background_task(async move {
                                finalize_openai_done_stream_after_drain(
                                    drain_app_state,
                                    drain_log_context,
                                    rx,
                                    drain_llm_body_writer,
                                    drain_user_body_writer,
                                    drain_transformer,
                                    drain_reasoning_stream_capture,
                                    drain_url,
                                    status_code,
                                    drain_cost_catalog_version,
                                    log_mode,
                                    execution_policy,
                                    drain_model_str,
                                    done_completed_at,
                                    same_chunk_ignored_events,
                                )
                                .await;
                            });

                            yield Ok::<_, std::io::Error>(transformed_chunk);
                            return;
                        }
                        yield Ok::<_, std::io::Error>(transformed_chunk);
                    }
                }
                Err(e) => {
                    response_drop_guard.disarm();
                    let stream_error_message = format!("LLM stream error: {}", e);
                    error!("{}", stream_error_message);
                    let proxy_error = ProxyError::BadGateway(stream_error_message.clone());
                    finalize_streaming_error(
                        &app_state_clone,
                        &log_context_clone,
                        &mut llm_body_writer,
                        &mut user_body_writer,
                        &url_owned,
                        status_code,
                        cost_catalog_version_clone.as_ref(),
                        &proxy_error,
                        log_mode,
                        execution_policy,
                    )
                    .await;
                    if execution_policy.records_provider_runtime() {
                        record_provider_failure(
                            &app_state_clone,
                            provider_id,
                            &model_str,
                            &proxy_error,
                            provider_circuit_permit.as_ref(),
                        )
                        .await;
                    }

                    api_key_request_lease.release().await;
                    yield Err(std::io::Error::other(stream_error_message));
                    return;
                }
            }
        }

        if status_code.is_success() && api_type == LlmApiType::Openai && target_api_type == LlmApiType::Gemini {
            debug!("[handle_streaming_response] Appending [DONE] chunk for OpenAI client.");
            let done_chunk = Bytes::from("data: [DONE]\n\n");
            if let Err(e) = user_body_writer.as_mut().expect("user stream writer should exist").append(&done_chunk).await {
                response_drop_guard.disarm();
                let stream_error_message = format!("Failed to persist terminal DONE chunk: {}", e);
                error!("{}", stream_error_message);
                let proxy_error = ProxyError::InternalError(stream_error_message.clone());
                finalize_streaming_error(
                    &app_state_clone,
                    &log_context_clone,
                    &mut llm_body_writer,
                    &mut user_body_writer,
                    &url_owned,
                    status_code,
                    cost_catalog_version_clone.as_ref(),
                    &proxy_error,
                    log_mode,
                    execution_policy,
                )
                .await;
                if execution_policy.records_provider_runtime() {
                    record_provider_failure(
                        &app_state_clone,
                        provider_id,
                        &model_str,
                        &proxy_error,
                        provider_circuit_permit.as_ref(),
                    )
                    .await;
                }

                api_key_request_lease.release().await;
                yield Err(std::io::Error::other(stream_error_message));
                return;
            }
            {
                let mut context = log_context_clone.lock().await;
                if execution_policy.records_request_log() {
                    user_body_writer
                        .as_mut()
                        .expect("user stream writer should exist")
                        .preserve_on_drop();
                }
                context.user_response_body =
                    Some(user_body_writer.as_ref().expect("user stream writer should exist").snapshot(LogBodyCaptureState::Incomplete));
            }
            mark_stream_response_started_to_client(&log_context_clone, &done_chunk).await;
            yield Ok::<_, std::io::Error>(done_chunk);
        }

        let llm_response_completed_at = Utc::now().timestamp_millis();

        if status_code.is_success() {
            let reasoning_capture_report = reasoning_stream_capture
                .finish(&app_state_clone, llm_response_completed_at)
                .await;
            let reasoning_capture_diagnostics =
                stream_capture_transform_diagnostics(&reasoning_capture_report);
            let mut context = log_context_clone.lock().await;
            finalize_streaming_log_context(
                &mut context,
                &url_owned,
                status_code,
                llm_response_completed_at,
                cost_catalog_version_clone.as_ref(),
                RequestStatus::Success,
                None,
            );
            context.llm_response_body = match llm_body_writer.take() {
                Some(writer) => writer.finish(LogBodyCaptureState::Complete).await.ok(),
                None => None,
            };
            context.user_response_body = match user_body_writer.take() {
                Some(writer) => writer.finish(LogBodyCaptureState::Complete).await.ok(),
                None => None,
            };

            context.usage = transformer.parse_usage_info();
            context.usage_normalization = transformer.parse_usage_normalization();
            context.replace_transform_diagnostics_phase(
                crate::utils::storage::RequestLogBundleTransformDiagnosticPhase::Stream,
                &transformer.diagnostics_snapshot(),
            );
            append_response_transform_diagnostics(&mut context, &reasoning_capture_diagnostics);
            record_streaming_completion_if_allowed(
                &app_state_clone,
                &context,
                log_mode,
                execution_policy,
            )
            .await;
            if execution_policy.records_provider_runtime() {
                record_provider_success(
                    &app_state_clone,
                    provider_id,
                    &model_str,
                    provider_circuit_permit.as_ref(),
                )
                .await;
            }
            if context.usage.is_none() {
                crate::debug_event!(
                    "proxy.stream_usage_missing_debug",
                    log_id = context.id,
                    model = &model_str,
                    status_code = status_code.as_u16(),
                );
            }
            crate::debug_event!(
                "proxy.request_succeeded_debug",
                log_id = context.id,
                model = &model_str,
                status_code = status_code.as_u16(),
                is_stream = true,
                latency_ms = llm_response_completed_at.saturating_sub(context.request_received_at),
            );
            api_key_request_lease.release().await;
            response_drop_guard.disarm();
        } else {
            let proxy_error = classify_upstream_status(status_code, &[]);
            finalize_streaming_error(
                &app_state_clone,
                &log_context_clone,
                &mut llm_body_writer,
                &mut user_body_writer,
                &url_owned,
                status_code,
                cost_catalog_version_clone.as_ref(),
                &proxy_error,
                log_mode,
                execution_policy,
            )
            .await;
            if execution_policy.records_provider_runtime() {
                record_provider_failure(
                    &app_state_clone,
                    provider_id,
                    &model_str,
                    &proxy_error,
                    provider_circuit_permit.as_ref(),
                )
                .await;
            }
            api_key_request_lease.release().await;
            response_drop_guard.disarm();
        }
    };

    match response_builder.body(Body::from_stream(monitored_stream)) {
        Ok(final_response) => Ok(final_response),
        Err(e) => {
            let log_id = log_context.lock().await.id;
            let proxy_error = protocol_transform_error(
                &format!("Failed to build client response for log_id {log_id}"),
                e,
            );
            error!("{}", proxy_error);
            Err(proxy_error)
        }
    }
}
