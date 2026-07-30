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
        logging::RequestLogContext,
        protocol_transform_error,
        provider_governance::{record_provider_failure, record_provider_success},
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
    },
    utils::sse::{SseEvent, SseParser},
};

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
        upstream_protocol: UpstreamProtocol,
    ) -> Self {
        let (scope, feature_enabled) = match capture_context {
            Some(context) => (Some(context.scope), context.feature_enabled),
            None => (None, false),
        };
        Self {
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
        let snapshots = self.snapshots(scope, observed_at_ms);
        for snapshot in snapshots {
            if let Err(err) = app_state
                .reasoning_continuation_store
                .insert(snapshot, observed_at_ms)
                .await
            {
                debug!("Failed to cache reasoning continuation: {err}");
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

fn is_downstream_openai_done_event(
    downstream_protocol: DownstreamProtocol,
    event: &SseEvent,
) -> bool {
    downstream_protocol == DownstreamProtocol::Openai && event.data.trim() == "[DONE]"
}

fn append_transformed_event_bytes(event: &SseEvent, output: &mut Vec<u8>) {
    output.extend_from_slice(&event.to_bytes());
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
    record_streaming_completion(app_state, &context).await;
}

async fn abort_and_finalize_cancelled_stream(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
) {
    finalize_cancelled_log_context(
        app_state,
        log_context,
        url,
        Some(status_code),
        cost_catalog_version,
    )
    .await;
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
    api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    provider_circuit_permit: Option<ProviderCircuitProbePermit>,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    reasoning_capture: Option<ReasoningContinuationCaptureContext>,
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

    let mut transformer = StreamTransformer::new(upstream_protocol, downstream_protocol);
    let mut parser = SseParser::new();
    let log_context_clone = log_context.clone();

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
            format!("Client disconnected while receiving streaming response for log_id {}.", log_id),
        );
        let mut first_chunk_received_at_proxy: i64 = 0;
        let mut reasoning_stream_capture =
            OpenAiReasoningStreamCapture::new(reasoning_capture, upstream_protocol);

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
                            &url_owned,
                            status_code,
                            cost_catalog_version_clone.as_ref(),
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
                                &url_owned,
                                status_code,
                                cost_catalog_version_clone.as_ref(),
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
                    if first_chunk_received_at_proxy == 0 {
                        first_chunk_received_at_proxy = Utc::now().timestamp_millis();
                    }

                    let events = parser.process(&chunk);
                    if events.is_empty() {
                        continue;
                    }

                    let mut transformed_chunk_bytes: Vec<u8> = Vec::new();
                    let mut downstream_openai_done = false;

                    for event in events {
                        reasoning_stream_capture.observe_events(std::slice::from_ref(&event));
                        let transformed_events =
                            transformer.transform_event(event).unwrap_or_default();
                        for transformed_event in transformed_events {
                            append_transformed_event_bytes(
                                &transformed_event,
                                &mut transformed_chunk_bytes,
                            );
                            if is_downstream_openai_done_event(
                                downstream_protocol,
                                &transformed_event,
                            ) {
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
                    if !transformed_chunk.is_empty() {
                        mark_stream_response_started_to_client(
                            &log_context_clone,
                            &transformed_chunk,
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
                    yield Err(std::io::Error::other(stream_error_message));
                    return;
                }
            }
        }

        if status_code.is_success()
            && downstream_protocol == DownstreamProtocol::Openai
            && upstream_protocol == UpstreamProtocol::Gemini
        {
            debug!("[handle_streaming_response] Appending [DONE] chunk for OpenAI client.");
            let done_chunk = Bytes::from("data: [DONE]\n\n");
            mark_stream_response_started_to_client(&log_context_clone, &done_chunk).await;
            yield Ok::<_, std::io::Error>(done_chunk);
        }

        let llm_response_completed_at = Utc::now().timestamp_millis();

        if status_code.is_success() {
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
