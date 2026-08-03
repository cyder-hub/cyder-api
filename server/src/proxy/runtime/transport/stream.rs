use std::time::Duration;
use std::{collections::BTreeMap, sync::Arc};

use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use chrono::Utc;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex as TokioMutex, mpsc},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

use super::{
    ReasoningContinuationCaptureContext, cancellation::ResponseStreamCancellationGuard,
    response::build_response_builder,
};
use crate::{
    config::SseResponseConfig,
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
        cancellation::ProxyCancellationContext,
        logging::RequestLogContext,
        provider_governance::{record_provider_failure, record_provider_success},
        request_context::RequestId,
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::{
                finalize_cancelled_log_context, finalize_streaming_log_context,
                record_streaming_completion,
            },
            reasoning_content_repair::continuation_snapshot_from_parts,
        },
    },
    schema::enum_def::{DownstreamProtocol, RequestStatus, UpstreamProtocol},
    service::{
        app_state::AppState,
        cache::types::CacheCostCatalogVersion,
        runtime::{ProviderCircuitProbePermit, ReasoningContinuationScope},
        transform::StreamTransformer,
        upstream_response::{
            UpstreamContentEncoding, parse_content_encoding, safe_http_error_message,
        },
    },
    utils::sse::{SseEvent, SseFrame, SseParser},
};

#[derive(Clone, Debug)]
pub(super) struct OpenAiReasoningStreamCapture {
    request_id: RequestId,
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

struct StreamReadFailure {
    operator_message: String,
    response_visibility: ResponseVisibilityTracker,
}

struct StreamReaderCancellationGuard(CancellationToken);

impl StreamReaderCancellationGuard {
    fn cancel(&self) {
        self.0.cancel();
    }
}

impl Drop for StreamReaderCancellationGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn validate_sse_content_encoding(
    headers: &HeaderMap,
    response_visibility: &ResponseVisibilityTracker,
) -> Result<(), ProxyError> {
    match parse_content_encoding(headers) {
        Ok(UpstreamContentEncoding::Identity) => Ok(()),
        Ok(UpstreamContentEncoding::Gzip) | Err(_) => Err(upstream_stream_error(
            ProxyErrorCode::UpstreamResponseError,
            response_visibility,
            "SSE response Content-Encoding is invalid or unsupported",
        )),
    }
}

fn validate_stream_chunk(
    chunk: bytes::Bytes,
    buffer_limit_bytes: usize,
    response_visibility: &ResponseVisibilityTracker,
) -> Result<bytes::Bytes, StreamReadFailure> {
    if chunk.len() <= buffer_limit_bytes {
        return Ok(chunk);
    }

    Err(StreamReadFailure {
        operator_message: format!(
            "SSE response chunk exceeded {buffer_limit_bytes} bytes (observed at least {})",
            buffer_limit_bytes.saturating_add(1),
        ),
        response_visibility: response_visibility.clone(),
    })
}

impl OpenAiReasoningStreamCapture {
    pub(super) fn new(
        capture_context: Option<ReasoningContinuationCaptureContext>,
        upstream_protocol: UpstreamProtocol,
        request_id: RequestId,
    ) -> Self {
        let (scope, feature_enabled) = match capture_context {
            Some(context) => (Some(context.scope), context.feature_enabled),
            None => (None, false),
        };
        Self {
            request_id,
            scope,
            feature_enabled,
            target_is_openai_compatible_generation: upstream_protocol == UpstreamProtocol::Openai,
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

    pub(super) async fn finish(self, app_state: &Arc<AppState>, observed_at_ms: i64) {
        if !self.feature_enabled
            || !self.target_is_openai_compatible_generation
            || self.parse_failed_count > 0
        {
            return;
        }

        let Some(scope) = self.scope.clone() else {
            return;
        };
        let request_id = self.request_id.clone();
        let snapshots = self.snapshots(scope, observed_at_ms);
        for snapshot in snapshots {
            if let Err(err) = app_state
                .reasoning_continuation_store
                .insert(snapshot, observed_at_ms)
                .await
            {
                crate::debug_event!(
                    "proxy.reasoning_continuation_cache_failed",
                    request_id = &request_id,
                    error = err,
                );
            }
        }
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

pub(super) async fn sync_stream_usage_to_log_context(
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    transformer: &mut StreamTransformer,
) {
    let usage = transformer.cached_usage_info();
    let usage_normalization = transformer.cached_usage_normalization();
    if usage.is_none() && usage_normalization.is_none() {
        return;
    }

    let mut context = log_context.lock().await;
    context.usage = usage;
    context.usage_normalization = usage_normalization;
}

pub(super) async fn mark_stream_response_started_to_client(
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    transformed_chunk: &Bytes,
    response_visibility: &ResponseVisibilityTracker,
) {
    if !mark_body_started_if_nonempty(transformed_chunk, response_visibility) {
        return;
    }

    let mut context = log_context.lock().await;
    if context.first_chunk_ts.is_none() {
        context.first_chunk_ts = Some(Utc::now().timestamp_millis());
    }
}

fn mark_body_started_if_nonempty(
    transformed_chunk: &Bytes,
    response_visibility: &ResponseVisibilityTracker,
) -> bool {
    if transformed_chunk.is_empty() {
        return false;
    }
    response_visibility.advance_to(ResponseVisibility::BodyStarted);
    true
}

fn upstream_stream_error(
    code: ProxyErrorCode,
    response_visibility: &ResponseVisibilityTracker,
    operator_message: impl Into<String>,
) -> ProxyError {
    ProxyError::gateway(
        code,
        ExecutionStage::UpstreamResponse,
        response_visibility.current(),
        None,
        operator_message,
    )
}

fn downstream_response_build_error(
    response_visibility: &ResponseVisibilityTracker,
    operator_message: impl Into<String>,
) -> ProxyError {
    ProxyError::gateway(
        ProxyErrorCode::DownstreamSendError,
        ExecutionStage::DownstreamSend,
        response_visibility.current(),
        None,
        operator_message,
    )
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

fn is_downstream_openai_done_event(
    downstream_protocol: DownstreamProtocol,
    event: &SseEvent,
) -> bool {
    downstream_protocol == DownstreamProtocol::Openai && event.data.trim() == "[DONE]"
}

async fn finalize_openai_done_stream(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    mut transformer: StreamTransformer,
    reasoning_stream_capture: OpenAiReasoningStreamCapture,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    model_str: &str,
    completed_at: i64,
) {
    reasoning_stream_capture
        .finish(app_state, completed_at)
        .await;
    let usage = transformer.parse_usage_info();
    let usage_normalization = transformer.parse_usage_normalization();

    let mut context = log_context.lock().await;
    finalize_streaming_log_context(
        &mut context,
        &url,
        status_code,
        completed_at,
        cost_catalog_version,
        RequestStatus::Success,
        None,
    );
    context.usage = usage;
    context.usage_normalization = usage_normalization;
    record_streaming_completion(app_state, &context).await;

    crate::debug_event!(
        "proxy.request_succeeded_debug",
        request_id = &context.request_id,
        log_id = context.id,
        model = model_str,
        status_code = status_code.as_u16(),
        is_stream = true,
        latency_ms = completed_at.saturating_sub(context.request_received_at),
    );
}

async fn finalize_streaming_error(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    proxy_error: &ProxyError,
) {
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
    crate::logging::log_proxy_error_event(
        "proxy.stream_terminal_error",
        Some(context.request_id.as_str()),
        Some(context.id),
        proxy_error,
    );
    record_streaming_completion(app_state, &context).await;
}

async fn abort_and_finalize_cancelled_stream(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    response_visibility: &ResponseVisibilityTracker,
) -> ProxyError {
    let proxy_error = cancellation
        .cancellation_error(
            ExecutionStage::DownstreamSend,
            response_visibility.current(),
        )
        .await;
    finalize_cancelled_log_context(
        app_state,
        log_context,
        url,
        Some(status_code),
        cost_catalog_version,
        &proxy_error,
    )
    .await;
    proxy_error
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
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    reasoning_capture: Option<ReasoningContinuationCaptureContext>,
    first_byte_timeout: Option<Duration>,
    sse_response_limits: SseResponseConfig,
    response_visibility: ResponseVisibilityTracker,
) -> Result<Response<Body>, ProxyError> {
    let status_code = response.status();
    let response_headers = response.headers().clone();
    let (request_id, log_id) = {
        let context = log_context.lock().await;
        (context.request_id.clone(), context.id)
    };
    if let Err(proxy_error) = validate_sse_content_encoding(&response_headers, &response_visibility)
    {
        crate::logging::log_proxy_error_event(
            "proxy.stream_header_rejected",
            Some(request_id.as_str()),
            Some(log_id),
            &proxy_error,
        );
        record_provider_failure(
            app_state,
            provider_id,
            &model_str,
            &proxy_error,
            provider_circuit_permit.as_ref(),
        )
        .await;
        let mut context = log_context.lock().await;
        finalize_streaming_log_context(
            &mut context,
            url,
            status_code,
            Utc::now().timestamp_millis(),
            cost_catalog_version.as_ref(),
            RequestStatus::Error,
            None,
        );
        api_key_request_lease.release().await;
        return Err(proxy_error);
    }
    let response_builder = build_response_builder(status_code, &response_headers);

    let (tx, mut rx) = mpsc::channel::<Result<bytes::Bytes, StreamReadFailure>>(1);

    let url_owned = url.to_string();
    let cost_catalog_version_clone = cost_catalog_version.clone();
    let app_state_clone = Arc::clone(app_state);

    let cancellation_for_reader = cancellation.clone();
    let reader_cancellation = CancellationToken::new();
    let reader_cancellation_for_task = reader_cancellation.clone();
    let response_visibility_for_reader = response_visibility.clone();
    let reader_buffer_limit_bytes = sse_response_limits.buffer_limit_bytes;
    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        loop {
            tokio::select! {
                biased;
                _ = cancellation_for_reader.cancelled() => break,
                _ = reader_cancellation_for_task.cancelled() => break,
                maybe_chunk = stream.next() => {
                    let Some(chunk_result) = maybe_chunk else {
                        break;
                    };
                    let chunk_result = chunk_result
                        .map_err(|error| StreamReadFailure {
                            operator_message: safe_http_error_message(
                                "LLM stream transport failed",
                                &error,
                            ),
                            response_visibility: response_visibility_for_reader.clone(),
                        })
                        .and_then(|chunk| {
                            validate_stream_chunk(
                                chunk,
                                reader_buffer_limit_bytes,
                                &response_visibility_for_reader,
                            )
                        });
                    if tx.send(chunk_result).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut transformer = StreamTransformer::new(upstream_protocol, downstream_protocol);
    let mut parser = SseParser::new(sse_response_limits);
    let log_context_clone = log_context.clone();
    let stream_request_id = request_id.clone();
    let stream_response_visibility = response_visibility.clone();

    let monitored_stream = async_stream::stream! {
        let reader_cancellation_guard = StreamReaderCancellationGuard(reader_cancellation);
        let mut api_key_request_lease = api_key_request_lease;
        let provider_circuit_permit = provider_circuit_permit;
        let mut response_drop_guard = ResponseStreamCancellationGuard::new(
            Arc::clone(&app_state_clone),
            cancellation.clone(),
            log_context_clone.clone(),
            stream_request_id.clone(),
            log_id,
            url_owned.clone(),
            status_code,
            cost_catalog_version_clone.clone(),
            stream_response_visibility.clone(),
            format!("Client disconnected while receiving streaming response for log_id {}.", log_id),
        );
        let mut first_chunk_received_at_proxy: i64 = 0;
        let mut reasoning_stream_capture =
            OpenAiReasoningStreamCapture::new(
                reasoning_capture,
                upstream_protocol,
                stream_request_id.clone(),
            );

        loop {
            let chunk_result = match next_stream_chunk_timeout_duration(first_chunk_received_at_proxy, first_byte_timeout) {
                Some(timeout_duration) => match tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => Err(()),
                    result = timeout(timeout_duration, rx.recv()) => Ok(result),
                } {
                    Err(()) => {
                        response_drop_guard.disarm();
                        let proxy_error = abort_and_finalize_cancelled_stream(
                            &app_state_clone,
                            &cancellation,
                            &log_context_clone,
                            &url_owned,
                            status_code,
                            cost_catalog_version_clone.as_ref(),
                            &stream_response_visibility,
                        ).await;
                        api_key_request_lease.release().await;
                        reader_cancellation_guard.cancel();
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
                            crate::error_event!(
                                "proxy.stream_first_chunk_timeout",
                                request_id = &stream_request_id,
                                log_id = log_id,
                                error = &stream_error_message,
                            );
                            let proxy_error = upstream_stream_error(
                                ProxyErrorCode::UpstreamTimeoutError,
                                &stream_response_visibility,
                                stream_error_message.clone(),
                            );
                            finalize_streaming_error(
                                &app_state_clone,
                                &log_context_clone,
                                &url_owned,
                                status_code,
                                cost_catalog_version_clone.as_ref(),
                                &proxy_error,
                            )
                            .await;
                            record_provider_failure(
                                &app_state_clone,
                                provider_id,
                                &model_str,
                                &proxy_error,
                                provider_circuit_permit.as_ref(),
                            )
                            .await;

                            api_key_request_lease.release().await;
                            reader_cancellation_guard.cancel();
                            yield Err(std::io::Error::new(std::io::ErrorKind::TimedOut, stream_error_message));
                            return;
                        }
                    },
                },
                None => {
                    tokio::select! {
                        biased;
                        _ = cancellation.cancelled() => {
                            response_drop_guard.disarm();
                            let proxy_error = abort_and_finalize_cancelled_stream(
                                &app_state_clone,
                                &cancellation,
                                &log_context_clone,
                                &url_owned,
                                status_code,
                                cost_catalog_version_clone.as_ref(),
                                &stream_response_visibility,
                            ).await;
                            api_key_request_lease.release().await;
                            reader_cancellation_guard.cancel();
                            yield Err(std::io::Error::new(
                                std::io::ErrorKind::ConnectionAborted,
                                proxy_error.to_string(),
                            ));
                            return;
                        }
                        result = rx.recv() => result,
                    }
                }
            };

            let at_eof = chunk_result.is_none();
            let mut next_frame = match chunk_result {
                Some(Ok(chunk)) => {
                    if first_chunk_received_at_proxy == 0 {
                        first_chunk_received_at_proxy = Utc::now().timestamp_millis();
                    }
                    parser.feed(&chunk)
                }
                Some(Err(stream_read_failure)) => {
                    response_drop_guard.disarm();
                    let proxy_error = upstream_stream_error(
                        ProxyErrorCode::UpstreamResponseError,
                        &stream_read_failure.response_visibility,
                        stream_read_failure.operator_message,
                    );
                    crate::error_event!(
                        "proxy.stream_read_failed",
                        request_id = &stream_request_id,
                        log_id = log_id,
                        error = proxy_error.operator_message(),
                    );
                    finalize_streaming_error(
                        &app_state_clone,
                        &log_context_clone,
                        &url_owned,
                        status_code,
                        cost_catalog_version_clone.as_ref(),
                        &proxy_error,
                    )
                    .await;
                    record_provider_failure(
                        &app_state_clone,
                        provider_id,
                        &model_str,
                        &proxy_error,
                        provider_circuit_permit.as_ref(),
                    )
                    .await;

                    api_key_request_lease.release().await;
                    reader_cancellation_guard.cancel();
                    yield Err(std::io::Error::other(proxy_error.to_string()));
                    return;
                }
                None => parser.finish(),
            };

            loop {
                let frame = match next_frame {
                    Ok(Some(frame)) => frame,
                    Ok(None) => break,
                    Err(parse_error) => {
                        response_drop_guard.disarm();
                        let proxy_error = upstream_stream_error(
                            ProxyErrorCode::UpstreamResponseError,
                            &stream_response_visibility,
                            format!("SSE response parsing failed: {parse_error}"),
                        );
                        crate::error_event!(
                            "proxy.stream_parse_failed",
                            request_id = &stream_request_id,
                            log_id = log_id,
                            error = proxy_error.operator_message(),
                        );
                        finalize_streaming_error(
                            &app_state_clone,
                            &log_context_clone,
                            &url_owned,
                            status_code,
                            cost_catalog_version_clone.as_ref(),
                            &proxy_error,
                        )
                        .await;
                        record_provider_failure(
                            &app_state_clone,
                            provider_id,
                            &model_str,
                            &proxy_error,
                            provider_circuit_permit.as_ref(),
                        )
                        .await;
                        api_key_request_lease.release().await;
                        reader_cancellation_guard.cancel();
                        yield Err(std::io::Error::other(proxy_error.to_string()));
                        return;
                    }
                };

                if let SseFrame::Event(event) = frame {
                    reasoning_stream_capture.observe_events(std::slice::from_ref(&event));
                    // R3.14 owns transform error productization; preserve the existing drop behavior.
                    let transformed_events = transformer.transform_event(event).unwrap_or_default();
                    sync_stream_usage_to_log_context(&log_context_clone, &mut transformer).await;
                    for transformed_event in transformed_events {
                        let downstream_openai_done = is_downstream_openai_done_event(
                            downstream_protocol,
                            &transformed_event,
                        );
                        let transformed_chunk = transformed_event.to_bytes().freeze();
                        mark_stream_response_started_to_client(
                            &log_context_clone,
                            &transformed_chunk,
                            &stream_response_visibility,
                        )
                        .await;
                        if downstream_openai_done {
                            let done_completed_at = Utc::now().timestamp_millis();
                            response_drop_guard.disarm();
                            api_key_request_lease.release().await;
                            record_provider_success(
                                &app_state_clone,
                                provider_id,
                                &model_str,
                                provider_circuit_permit.as_ref(),
                            )
                            .await;

                            let drain_app_state = Arc::clone(&app_state_clone);
                            let drain_log_context = Arc::clone(&log_context_clone);
                            let drain_url = url_owned.clone();
                            let drain_cost_catalog_version = cost_catalog_version_clone.clone();
                            let drain_model_str = model_str.clone();
                            let drain_transformer = transformer;
                            let drain_reasoning_stream_capture = reasoning_stream_capture;
                            app_state_clone.infra.spawn_background_task(async move {
                                finalize_openai_done_stream(
                                    &drain_app_state,
                                    &drain_log_context,
                                    drain_transformer,
                                    drain_reasoning_stream_capture,
                                    &drain_url,
                                    status_code,
                                    drain_cost_catalog_version.as_ref(),
                                    &drain_model_str,
                                    done_completed_at,
                                )
                                .await;
                            });

                            reader_cancellation_guard.cancel();
                            yield Ok::<_, std::io::Error>(transformed_chunk);
                            return;
                        }
                        yield Ok::<_, std::io::Error>(transformed_chunk);
                    }
                }

                next_frame = if at_eof {
                    parser.finish()
                } else {
                    parser.feed(&[])
                };
            }
            if at_eof {
                break;
            }
        }

        if downstream_protocol == DownstreamProtocol::Openai
            && upstream_protocol == UpstreamProtocol::Gemini
        {
            crate::debug_event!(
                "proxy.stream_done_synthesized",
                request_id = &stream_request_id,
                log_id = log_id,
                downstream_protocol = format!("{downstream_protocol:?}"),
                upstream_protocol = format!("{upstream_protocol:?}"),
            );
            let done_chunk = Bytes::from("data: [DONE]\n\n");
            mark_stream_response_started_to_client(
                &log_context_clone,
                &done_chunk,
                &stream_response_visibility,
            )
            .await;
            yield Ok::<_, std::io::Error>(done_chunk);
        }

        let llm_response_completed_at = Utc::now().timestamp_millis();

        reasoning_stream_capture
            .finish(&app_state_clone, llm_response_completed_at)
            .await;
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
        context.usage = transformer.parse_usage_info();
        context.usage_normalization = transformer.parse_usage_normalization();
        record_streaming_completion(&app_state_clone, &context).await;
        record_provider_success(
            &app_state_clone,
            provider_id,
            &model_str,
            provider_circuit_permit.as_ref(),
        )
        .await;
        if context.usage.is_none() {
            crate::debug_event!(
                "proxy.stream_usage_missing_debug",
                request_id = &context.request_id,
                log_id = context.id,
                model = &model_str,
                status_code = status_code.as_u16(),
            );
        }
        crate::debug_event!(
            "proxy.request_succeeded_debug",
            request_id = &context.request_id,
            log_id = context.id,
            model = &model_str,
            status_code = status_code.as_u16(),
            is_stream = true,
            latency_ms = llm_response_completed_at.saturating_sub(context.request_received_at),
        );
        api_key_request_lease.release().await;
        response_drop_guard.disarm();
    };

    match response_builder.body(Body::from_stream(monitored_stream)) {
        Ok(final_response) => {
            response_visibility.advance_to(ResponseVisibility::HeadersCommitted);
            Ok(final_response)
        }
        Err(e) => {
            let log_id = log_context.lock().await.id;
            let proxy_error = downstream_response_build_error(
                &response_visibility,
                format!("Failed to build client response for log_id {log_id}: {e}"),
            );
            crate::error_event!(
                "proxy.response_build_failed",
                request_id = &request_id,
                log_id = log_id,
                error = proxy_error.to_string(),
            );
            Err(proxy_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use axum::http::{HeaderMap, HeaderValue, header::CONTENT_ENCODING};

    use super::{
        downstream_response_build_error, mark_body_started_if_nonempty, upstream_stream_error,
        validate_sse_content_encoding, validate_stream_chunk,
    };
    use crate::proxy::{
        ExecutionStage, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
    };

    #[test]
    fn downstream_body_visibility_advances_only_for_non_empty_chunks() {
        let tracker = ResponseVisibilityTracker::new();
        tracker.advance_to(ResponseVisibility::HeadersCommitted);

        assert!(!mark_body_started_if_nonempty(&Bytes::new(), &tracker));
        assert_eq!(tracker.current(), ResponseVisibility::HeadersCommitted);

        assert!(mark_body_started_if_nonempty(
            &Bytes::from_static(b"data: chunk\n\n"),
            &tracker,
        ));
        assert_eq!(tracker.current(), ResponseVisibility::BodyStarted);
    }

    #[test]
    fn upstream_stream_facts_capture_visibility_before_and_after_first_body_chunk() {
        let tracker = ResponseVisibilityTracker::new();
        tracker.advance_to(ResponseVisibility::HeadersCommitted);

        let before_first_chunk = upstream_stream_error(
            ProxyErrorCode::UpstreamTimeoutError,
            &tracker,
            "first chunk timeout",
        );
        assert_eq!(before_first_chunk.stage(), ExecutionStage::UpstreamResponse);
        assert_eq!(
            before_first_chunk.response_visibility(),
            ResponseVisibility::HeadersCommitted
        );

        tracker.advance_to(ResponseVisibility::BodyStarted);
        let after_first_chunk = upstream_stream_error(
            ProxyErrorCode::UpstreamResponseError,
            &tracker,
            "stream interrupted",
        );
        assert_eq!(after_first_chunk.stage(), ExecutionStage::UpstreamResponse);
        assert_eq!(
            after_first_chunk.response_visibility(),
            ResponseVisibility::BodyStarted
        );
    }

    #[test]
    fn response_builder_failure_remains_not_visible() {
        let tracker = ResponseVisibilityTracker::new();
        let error = downstream_response_build_error(&tracker, "response builder failed");

        assert_eq!(error.code(), ProxyErrorCode::DownstreamSendError);
        assert_eq!(error.stage(), ExecutionStage::DownstreamSend);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
        assert!(error.upstream_error().is_none());
    }

    #[test]
    fn sse_content_encoding_accepts_only_absent_or_identity_before_commit() {
        let tracker = ResponseVisibilityTracker::new();
        let absent = HeaderMap::new();
        assert!(validate_sse_content_encoding(&absent, &tracker).is_ok());

        let mut identity = HeaderMap::new();
        identity.insert(CONTENT_ENCODING, HeaderValue::from_static("IDENTITY"));
        assert!(validate_sse_content_encoding(&identity, &tracker).is_ok());

        for value in ["gzip", "br", "gzip, identity", ""] {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_ENCODING, HeaderValue::from_str(value).unwrap());
            let error = validate_sse_content_encoding(&headers, &tracker).unwrap_err();
            assert_eq!(error.code(), ProxyErrorCode::UpstreamResponseError);
            assert_eq!(error.stage(), ExecutionStage::UpstreamResponse);
            assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
            assert!(error.upstream_error().is_none());
        }
    }

    #[test]
    fn stream_chunk_limit_is_checked_before_channel_enqueue() {
        let tracker = ResponseVisibilityTracker::new();
        tracker.advance_to(ResponseVisibility::HeadersCommitted);

        let Ok(exact) = validate_stream_chunk(Bytes::from(vec![b'x'; 1_024]), 1_024, &tracker)
        else {
            panic!("exact chunk limit should pass");
        };
        assert_eq!(exact.len(), 1_024);

        let error = validate_stream_chunk(Bytes::from(vec![b'x'; 1_025]), 1_024, &tracker)
            .expect_err("chunk over the retained-buffer limit should fail before enqueue");
        assert_eq!(
            error.operator_message,
            "SSE response chunk exceeded 1024 bytes (observed at least 1025)"
        );
        assert_eq!(
            error.response_visibility.current(),
            ResponseVisibility::HeadersCommitted
        );
    }
}
