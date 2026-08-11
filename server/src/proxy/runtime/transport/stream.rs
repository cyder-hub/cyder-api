use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use chrono::Utc;
use futures::StreamExt;
use tokio::{
    sync::{Mutex as TokioMutex, mpsc, oneshot},
    time::{Instant, timeout_at},
};

use super::{
    body::{
        BODY_FRAME_CHANNEL_CAPACITY, BodyFrame, FrameDeliveryError, GuardedBodyStream,
        guarded_body, send_body_frame, send_terminal_body_event,
    },
    lifecycle::{
        ProxyTerminationCause, ProxyTerminationCoordinator, TotalWatchdogResult,
        await_cleanup_with_total_watchdog,
    },
    response::build_response_builder,
};
use crate::{
    config::{ProxyTimeoutConfig, SseResponseConfig},
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
        TimeoutPhase,
        cancellation::ProxyCancellationContext,
        classify_transform_failure,
        logging::{
            RequestLogContext, TransformLogStage, log_transform_failure, log_transform_summary,
        },
        proxy_error_category,
        request_context::RequestId,
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::{finalize_streaming_log_context, record_streaming_completion},
        },
    },
    schema::enum_def::{DownstreamProtocol, RequestStatus, UpstreamProtocol},
    service::{
        app_state::AppState,
        cache::types::CacheCostCatalogVersion,
        transform::{
            FatalStreamEncodeError, FatalStreamErrorFact, StreamTransformer, TransformFailure,
            encode_fatal_stream_error,
        },
        upstream_response::{UpstreamContentEncoding, parse_content_encoding},
    },
    utils::sse::{SseEvent, SseFrame, SseParser},
};

struct StreamReadFailure {
    operator_message: String,
    response_visibility: ResponseVisibilityTracker,
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

fn upstream_stream_error(
    code: ProxyErrorCode,
    response_visibility: &ResponseVisibilityTracker,
    operator_message: impl Into<String>,
) -> ProxyError {
    if code == ProxyErrorCode::UpstreamTimeoutError {
        ProxyError::upstream_timeout(
            TimeoutPhase::FirstByte,
            ExecutionStage::UpstreamResponse,
            response_visibility.current(),
            operator_message,
        )
    } else {
        ProxyError::gateway(
            code,
            ExecutionStage::UpstreamResponse,
            response_visibility.current(),
            None,
            operator_message,
        )
    }
}

fn transform_failure_to_proxy_error(
    failure: &TransformFailure,
    visibility: ResponseVisibility,
) -> ProxyError {
    classify_transform_failure(failure, visibility)
}

fn encode_guarded_transform_terminal(
    downstream_protocol: DownstreamProtocol,
    request_id: &str,
    proxy_error: &ProxyError,
) -> Result<Bytes, FatalStreamEncodeError> {
    encode_fatal_stream_error(
        downstream_protocol,
        FatalStreamErrorFact {
            request_id,
            code: proxy_error.code().as_str(),
            category: proxy_error_category(proxy_error.code(), downstream_protocol),
            public_message: proxy_error.public_message(),
            http_status: proxy_error.status_code().as_u16(),
        },
    )
    .map(|event| event.to_bytes().freeze())
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

fn is_downstream_openai_done_event(
    downstream_protocol: DownstreamProtocol,
    event: &SseEvent,
) -> bool {
    downstream_protocol == DownstreamProtocol::Openai && event.data.trim() == "[DONE]"
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

async fn finalize_streaming_error_and_release(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    api_key_request_lease: &mut ApiKeyRequestLeaseFinalizer,
    proxy_error: &ProxyError,
) {
    await_cleanup_with_total_watchdog(cancellation, coordinator, async {
        finalize_streaming_error(
            app_state,
            log_context,
            url,
            status_code,
            cost_catalog_version,
            proxy_error,
        )
        .await;
        api_key_request_lease.release().await;
    })
    .await;
}

fn stream_io_error(proxy_error: &ProxyError) -> std::io::Error {
    let kind = if proxy_error.code() == ProxyErrorCode::ClientCancelledError {
        std::io::ErrorKind::ConnectionAborted
    } else if proxy_error.code() == ProxyErrorCode::UpstreamTimeoutError {
        std::io::ErrorKind::TimedOut
    } else {
        std::io::ErrorKind::Other
    };
    std::io::Error::new(kind, proxy_error.operator_message().to_string())
}

fn watchdog_timeout_phase(
    result: TotalWatchdogResult,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
) -> Option<TimeoutPhase> {
    match result {
        TotalWatchdogResult::Expired => Some(TimeoutPhase::Total),
        TotalWatchdogResult::Cancelled => {
            if cancellation.is_cancelled() {
                None
            } else {
                match coordinator.terminal() {
                    Some(ProxyTerminationCause::Timeout { phase }) => Some(phase),
                    Some(ProxyTerminationCause::ClientCancelled) => None,
                    _ => Some(TimeoutPhase::Total),
                }
            }
        }
    }
}

async fn finalize_guarded_stream_failure(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
    sender: &mpsc::Sender<BodyFrame>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    response_visibility: &ResponseVisibilityTracker,
    api_key_request_lease: &mut ApiKeyRequestLeaseFinalizer,
    proxy_error: &ProxyError,
) {
    finalize_guarded_stream_failure_delivery(
        app_state,
        cancellation,
        coordinator,
        sender,
        log_context,
        url,
        status_code,
        cost_catalog_version,
        response_visibility,
        api_key_request_lease,
        proxy_error,
        None,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn finalize_guarded_transform_failure(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
    sender: &mpsc::Sender<BodyFrame>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    response_visibility: &ResponseVisibilityTracker,
    api_key_request_lease: &mut ApiKeyRequestLeaseFinalizer,
    downstream_protocol: DownstreamProtocol,
    request_id: &RequestId,
    proxy_error: &ProxyError,
) {
    let terminal_event =
        encode_guarded_transform_terminal(downstream_protocol, request_id.as_str(), proxy_error);
    let terminal_bytes = match terminal_event {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            crate::warn_event!(
                "proxy.stream_terminal_encode_failed",
                request_id = request_id.as_str(),
                error_code = proxy_error.code().as_str(),
                downstream_protocol = format!("{downstream_protocol:?}"),
                encoder_error = error.to_string(),
            );
            None
        }
    };

    finalize_guarded_stream_failure_delivery(
        app_state,
        cancellation,
        coordinator,
        sender,
        log_context,
        url,
        status_code,
        cost_catalog_version,
        response_visibility,
        api_key_request_lease,
        proxy_error,
        terminal_bytes,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn finalize_guarded_stream_failure_delivery(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
    sender: &mpsc::Sender<BodyFrame>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    response_visibility: &ResponseVisibilityTracker,
    api_key_request_lease: &mut ApiKeyRequestLeaseFinalizer,
    proxy_error: &ProxyError,
    terminal_bytes: Option<Bytes>,
) {
    if coordinator.terminal().is_some() {
        return;
    }

    let delivery = match terminal_bytes {
        Some(bytes) => send_terminal_body_event(sender, bytes, cancellation, coordinator).await,
        None => {
            send_body_frame(
                sender,
                Err(stream_io_error(proxy_error)),
                cancellation,
                coordinator,
            )
            .await
        }
    };
    match delivery {
        Ok(()) => {
            coordinator.try_terminate_error(proxy_error);
            finalize_streaming_error_and_release(
                app_state,
                cancellation,
                coordinator,
                log_context,
                url,
                status_code,
                cost_catalog_version,
                api_key_request_lease,
                proxy_error,
            )
            .await;
        }
        Err(FrameDeliveryError::ClientCancelled | FrameDeliveryError::DownstreamDropped) => {
            cancellation.cancel_now("downstream stopped consuming the guarded response body");
        }
        Err(FrameDeliveryError::Timeout { phase }) => {
            let terminal_error = if proxy_error.timeout_phase() == Some(phase) {
                proxy_error.clone()
            } else {
                ProxyError::upstream_timeout(
                    phase,
                    ExecutionStage::DownstreamSend,
                    response_visibility.current(),
                    format!("guarded response body exceeded the {phase:?} timeout"),
                )
            };
            coordinator.try_terminate_error(&terminal_error);
            finalize_streaming_error_and_release(
                app_state,
                cancellation,
                coordinator,
                log_context,
                url,
                status_code,
                cost_catalog_version,
                api_key_request_lease,
                &terminal_error,
            )
            .await;
        }
    }
}

async fn log_stream_transform_summary_once(
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    transformer: &StreamTransformer,
) {
    let summary = transformer.diagnostics_snapshot();
    let context = log_context.lock().await;
    log_transform_summary(TransformLogStage::Stream, &context, &summary);
}

async fn log_stream_transform_failure_once(
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    transformer: &StreamTransformer,
    failure: &TransformFailure,
) {
    let mut aggregate_failure = failure.clone();
    aggregate_failure.summary = transformer.diagnostics_snapshot();
    let context = log_context.lock().await;
    log_transform_failure(TransformLogStage::Stream, &context, &aggregate_failure);
}

async fn finalize_guarded_stream_success(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    mut transformer: StreamTransformer,
    url: &str,
    status_code: StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    model_str: &str,
    api_key_request_lease: &mut ApiKeyRequestLeaseFinalizer,
) {
    await_cleanup_with_total_watchdog(cancellation, coordinator, async {
        let completed_at = Utc::now().timestamp_millis();
        let usage = transformer.parse_usage_info();
        let usage_normalization = transformer.parse_usage_normalization();
        let transform_summary = transformer.diagnostics_snapshot();
        let context_snapshot = {
            let mut context = log_context.lock().await;
            finalize_streaming_log_context(
                &mut context,
                url,
                status_code,
                completed_at,
                cost_catalog_version,
                RequestStatus::Success,
                None,
            );
            context.usage = usage;
            context.usage_normalization = usage_normalization;
            log_transform_summary(TransformLogStage::Stream, &context, &transform_summary);
            if context.usage.is_none() {
                crate::debug_event!(
                    "proxy.stream_usage_missing_debug",
                    request_id = &context.request_id,
                    log_id = context.id,
                    model = model_str,
                    status_code = status_code.as_u16(),
                );
            }
            crate::debug_event!(
                "proxy.request_succeeded_debug",
                request_id = &context.request_id,
                log_id = context.id,
                model = model_str,
                status_code = status_code.as_u16(),
                is_stream = true,
                latency_ms = completed_at.saturating_sub(context.request_received_at),
            );
            context.clone()
        };
        record_streaming_completion(app_state, &context_snapshot).await;
        api_key_request_lease.release().await;
    })
    .await;
    coordinator.try_terminate(ProxyTerminationCause::Success);
}

#[allow(clippy::too_many_arguments)]
async fn run_guarded_stream_worker(
    app_state: Arc<AppState>,
    cancellation: ProxyCancellationContext,
    coordinator: ProxyTerminationCoordinator,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: String,
    status_code: StatusCode,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    mut api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    proxy_timeouts: ProxyTimeoutConfig,
    sse_response_limits: SseResponseConfig,
    response_visibility: ResponseVisibilityTracker,
    headers_committed: oneshot::Receiver<()>,
    sender: mpsc::Sender<BodyFrame>,
    request_id: RequestId,
    log_id: i64,
) {
    if headers_committed.await.is_err() {
        api_key_request_lease.release().await;
        return;
    }
    debug_assert!(
        response_visibility.current() >= ResponseVisibility::HeadersCommitted,
        "guarded stream worker must not consume upstream bytes before downstream headers commit"
    );

    let mut stream = response.bytes_stream();
    let mut transformer = StreamTransformer::new(upstream_protocol, downstream_protocol);
    let mut parser = SseParser::new(sse_response_limits.clone());
    let mut saw_nonempty_raw_body = false;
    let mut active_read_deadline = None;

    loop {
        let wait_started_at = Instant::now();
        let (read_timeout, read_phase) = if saw_nonempty_raw_body {
            (proxy_timeouts.response_idle(), TimeoutPhase::ResponseIdle)
        } else {
            (proxy_timeouts.first_byte(), TimeoutPhase::FirstByte)
        };
        let read_deadline =
            *active_read_deadline.get_or_insert_with(|| wait_started_at + read_timeout);
        let watchdog = coordinator.arm_total_watchdog(
            cancellation
                .total_deadline()
                .expect("proxy total deadline must be initialized before guarded Body"),
        );
        let read_result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                log_stream_transform_summary_once(&log_context, &transformer).await;
                return;
            },
            result = timeout_at(read_deadline, stream.next()) => match result {
                Ok(Some(Ok(chunk))) => Ok(Some(chunk)),
                Ok(Some(Err(error))) => {
                    if error.is_timeout() {
                        Err(ProxyError::upstream_timeout(
                            read_phase,
                            ExecutionStage::UpstreamResponse,
                            response_visibility.current(),
                            format!("LLM stream transport failed during {read_phase:?}"),
                        ))
                    } else {
                        Err(ProxyError::gateway(
                            ProxyErrorCode::UpstreamResponseError,
                            ExecutionStage::UpstreamResponse,
                            response_visibility.current(),
                            None,
                            format!("LLM stream transport failed: {error}"),
                        ))
                    }
                }
                Ok(None) => Ok(None),
                Err(_) => Err(ProxyError::upstream_timeout(
                    read_phase,
                    ExecutionStage::UpstreamResponse,
                    response_visibility.current(),
                    format!("LLM stream timed out during the {read_phase:?} phase"),
                )),
            },
            watchdog_result = watchdog.wait() => match watchdog_timeout_phase(
                watchdog_result,
                &cancellation,
                &coordinator,
            ) {
                Some(phase) => Err(ProxyError::upstream_timeout(
                    phase,
                    ExecutionStage::UpstreamResponse,
                    response_visibility.current(),
                    format!("LLM stream exceeded the {phase:?} timeout"),
                )),
                None => {
                    log_stream_transform_summary_once(&log_context, &transformer).await;
                    return;
                }
            },
        };

        let at_eof = matches!(read_result, Ok(None));
        let mut next_frame = match read_result {
            Ok(Some(chunk)) => {
                let chunk = match validate_stream_chunk(
                    chunk,
                    sse_response_limits.buffer_limit_bytes,
                    &response_visibility,
                ) {
                    Ok(chunk) => chunk,
                    Err(failure) => {
                        let proxy_error = ProxyError::gateway(
                            ProxyErrorCode::UpstreamResponseError,
                            ExecutionStage::UpstreamResponse,
                            failure.response_visibility.current(),
                            None,
                            failure.operator_message,
                        );
                        log_stream_transform_summary_once(&log_context, &transformer).await;
                        finalize_guarded_stream_failure(
                            &app_state,
                            &cancellation,
                            &coordinator,
                            &sender,
                            &log_context,
                            &url,
                            status_code,
                            cost_catalog_version.as_ref(),
                            &response_visibility,
                            &mut api_key_request_lease,
                            &proxy_error,
                        )
                        .await;
                        return;
                    }
                };
                let received_at = Instant::now();
                cancellation.timing().observe_upstream_raw_chunk(
                    &chunk,
                    Utc::now().timestamp_millis(),
                    wait_started_at,
                    received_at,
                );
                if !chunk.is_empty() {
                    saw_nonempty_raw_body = true;
                    active_read_deadline = None;
                }
                parser.feed(&chunk)
            }
            Ok(None) => parser.finish(),
            Err(proxy_error) => {
                log_stream_transform_summary_once(&log_context, &transformer).await;
                finalize_guarded_stream_failure(
                    &app_state,
                    &cancellation,
                    &coordinator,
                    &sender,
                    &log_context,
                    &url,
                    status_code,
                    cost_catalog_version.as_ref(),
                    &response_visibility,
                    &mut api_key_request_lease,
                    &proxy_error,
                )
                .await;
                return;
            }
        };

        loop {
            let frame = match next_frame {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(parse_error) => {
                    let proxy_error = upstream_stream_error(
                        ProxyErrorCode::UpstreamResponseError,
                        &response_visibility,
                        format!("SSE response parsing failed: {parse_error}"),
                    );
                    log_stream_transform_summary_once(&log_context, &transformer).await;
                    finalize_guarded_stream_failure(
                        &app_state,
                        &cancellation,
                        &coordinator,
                        &sender,
                        &log_context,
                        &url,
                        status_code,
                        cost_catalog_version.as_ref(),
                        &response_visibility,
                        &mut api_key_request_lease,
                        &proxy_error,
                    )
                    .await;
                    return;
                }
            };

            if let SseFrame::Event(event) = frame {
                let transform_output = match transformer.transform_event_with_observation(event) {
                    Ok(output) => output,
                    Err(failure) => {
                        let proxy_error = transform_failure_to_proxy_error(
                            &failure,
                            response_visibility.current(),
                        );
                        log_stream_transform_failure_once(&log_context, &transformer, &failure)
                            .await;
                        finalize_guarded_transform_failure(
                            &app_state,
                            &cancellation,
                            &coordinator,
                            &sender,
                            &log_context,
                            &url,
                            status_code,
                            cost_catalog_version.as_ref(),
                            &response_visibility,
                            &mut api_key_request_lease,
                            downstream_protocol,
                            &request_id,
                            &proxy_error,
                        )
                        .await;
                        return;
                    }
                };
                if transform_output.value.meaningful_output_observed {
                    cancellation.timing().mark_first_token(
                        Utc::now().timestamp_millis(),
                        tokio::time::Instant::now(),
                    );
                }
                let transformed_events = transform_output.value.events;
                sync_stream_usage_to_log_context(&log_context, &mut transformer).await;
                for transformed_event in transformed_events {
                    let downstream_openai_done =
                        is_downstream_openai_done_event(downstream_protocol, &transformed_event);
                    let transformed_chunk = transformed_event.to_bytes().freeze();
                    let delivery = send_body_frame(
                        &sender,
                        Ok(transformed_chunk),
                        &cancellation,
                        &coordinator,
                    )
                    .await;
                    if let Err(delivery_error) = delivery {
                        let proxy_error = match delivery_error {
                            FrameDeliveryError::ClientCancelled
                            | FrameDeliveryError::DownstreamDropped => {
                                log_stream_transform_summary_once(&log_context, &transformer).await;
                                cancellation.cancel_now(
                                    "downstream stopped consuming the guarded stream body",
                                );
                                return;
                            }
                            FrameDeliveryError::Timeout { phase } => ProxyError::upstream_timeout(
                                phase,
                                ExecutionStage::DownstreamSend,
                                response_visibility.current(),
                                format!("guarded stream body exceeded the {phase:?} timeout"),
                            ),
                        };
                        log_stream_transform_summary_once(&log_context, &transformer).await;
                        coordinator.try_terminate_error(&proxy_error);
                        finalize_streaming_error_and_release(
                            &app_state,
                            &cancellation,
                            &coordinator,
                            &log_context,
                            &url,
                            status_code,
                            cost_catalog_version.as_ref(),
                            &mut api_key_request_lease,
                            &proxy_error,
                        )
                        .await;
                        return;
                    }
                    if downstream_openai_done {
                        finalize_guarded_stream_success(
                            &app_state,
                            &cancellation,
                            &coordinator,
                            &log_context,
                            transformer,
                            &url,
                            status_code,
                            cost_catalog_version.as_ref(),
                            &model_str,
                            &mut api_key_request_lease,
                        )
                        .await;
                        return;
                    }
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

    if cancellation.is_cancelled() {
        log_stream_transform_summary_once(&log_context, &transformer).await;
        return;
    }
    if downstream_protocol == DownstreamProtocol::Openai
        && upstream_protocol == UpstreamProtocol::Gemini
    {
        crate::debug_event!(
            "proxy.stream_done_synthesized",
            request_id = &request_id,
            log_id = log_id,
            downstream_protocol = format!("{downstream_protocol:?}"),
            upstream_protocol = format!("{upstream_protocol:?}"),
        );
        let delivery = send_body_frame(
            &sender,
            Ok(Bytes::from_static(b"data: [DONE]\n\n")),
            &cancellation,
            &coordinator,
        )
        .await;
        if let Err(delivery_error) = delivery {
            match delivery_error {
                FrameDeliveryError::ClientCancelled | FrameDeliveryError::DownstreamDropped => {
                    log_stream_transform_summary_once(&log_context, &transformer).await;
                    cancellation.cancel_now("downstream stopped consuming the guarded stream body");
                }
                FrameDeliveryError::Timeout { phase } => {
                    let proxy_error = ProxyError::upstream_timeout(
                        phase,
                        ExecutionStage::DownstreamSend,
                        response_visibility.current(),
                        format!("guarded stream body exceeded the {phase:?} timeout"),
                    );
                    log_stream_transform_summary_once(&log_context, &transformer).await;
                    coordinator.try_terminate_error(&proxy_error);
                    finalize_streaming_error_and_release(
                        &app_state,
                        &cancellation,
                        &coordinator,
                        &log_context,
                        &url,
                        status_code,
                        cost_catalog_version.as_ref(),
                        &mut api_key_request_lease,
                        &proxy_error,
                    )
                    .await;
                }
            }
            return;
        }
    }
    finalize_guarded_stream_success(
        &app_state,
        &cancellation,
        &coordinator,
        &log_context,
        transformer,
        &url,
        status_code,
        cost_catalog_version.as_ref(),
        &model_str,
        &mut api_key_request_lease,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_streaming_response_guarded(
    app_state: &Arc<AppState>,
    cancellation: ProxyCancellationContext,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    proxy_timeouts: ProxyTimeoutConfig,
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
        cancellation.try_terminate_error(&proxy_error);
        crate::logging::log_proxy_error_event(
            "proxy.stream_header_rejected",
            Some(request_id.as_str()),
            Some(log_id),
            &proxy_error,
        );
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
        drop(context);
        let mut lease = api_key_request_lease;
        lease.release().await;
        return Err(proxy_error);
    }

    let coordinator = cancellation.coordinator();
    let (sender, receiver) = mpsc::channel(BODY_FRAME_CHANNEL_CAPACITY);
    let (headers_committed_sender, headers_committed_receiver) = oneshot::channel();
    let worker = app_state
        .infra
        .spawn_background_task(run_guarded_stream_worker(
            Arc::clone(app_state),
            cancellation.clone(),
            coordinator.clone(),
            log_context.clone(),
            model_str,
            response,
            url.to_string(),
            status_code,
            cost_catalog_version.clone(),
            api_key_request_lease.with_coordinator(coordinator.clone()),
            downstream_protocol,
            upstream_protocol,
            proxy_timeouts,
            sse_response_limits,
            response_visibility.clone(),
            headers_committed_receiver,
            sender,
            request_id,
            log_id,
        ));
    let body = guarded_body(GuardedBodyStream::new(
        Arc::clone(app_state),
        cancellation,
        coordinator,
        receiver,
        worker,
        log_context,
        url.to_string(),
        status_code,
        cost_catalog_version,
        response_visibility.clone(),
        format!("Client disconnected while receiving streaming response for log_id {log_id}."),
    ));
    match build_response_builder(status_code, &response_headers).body(body) {
        Ok(response) => {
            response_visibility.advance_to(ResponseVisibility::HeadersCommitted);
            let _ = headers_committed_sender.send(());
            Ok(response)
        }
        Err(error) => Err(downstream_response_build_error(
            &response_visibility,
            format!("Failed to build client response for log_id {log_id}: {error}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use axum::http::{HeaderMap, HeaderValue, header::CONTENT_ENCODING};

    use super::{
        downstream_response_build_error, encode_guarded_transform_terminal, upstream_stream_error,
        validate_sse_content_encoding, validate_stream_chunk,
    };
    use crate::proxy::{
        ExecutionStage, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
    };
    use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
    use crate::service::transform::FatalStreamEncodeError;
    use crate::service::transform::StreamTransformer;
    use crate::utils::sse::SseEvent;
    use tokio::time::Instant;

    #[test]
    fn terminal_encoder_failure_remains_an_explicit_body_error_fallback_signal() {
        let error = crate::proxy::ProxyError::gateway(
            ProxyErrorCode::ProtocolTransformError,
            ExecutionStage::Transform,
            ResponseVisibility::HeadersCommitted,
            None,
            "private transform detail",
        );
        let result = encode_guarded_transform_terminal(
            DownstreamProtocol::Responses,
            "invalid\nrequest-id",
            &error,
        );
        assert_eq!(
            result.unwrap_err(),
            FatalStreamEncodeError::InvalidRequestIdentity
        );
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
    fn source_meaningful_fact_sets_transport_ttft_once_before_downstream_encoding() {
        let mut transformer =
            StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Anthropic);
        let timing = crate::proxy::runtime::transport::timing::TransportTimingState::default();
        let base = Instant::now();
        assert!(timing.mark_upstream_request_sent(1_000, base));

        let role = transformer
            .transform_event_with_observation(SseEvent {
                data:
                    "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}"
                        .to_string(),
                ..Default::default()
            })
            .expect("role observation transform must succeed");
        assert!(!role.value.meaningful_output_observed);
        let content = transformer.transform_event_with_observation(SseEvent {
            data: "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}".to_string(),
            ..Default::default()
        }).expect("content observation transform must succeed");
        assert!(content.value.meaningful_output_observed);
        assert!(timing.mark_first_token(1_120, base + std::time::Duration::from_millis(120)));
        assert!(!timing.mark_first_token(1_130, base + std::time::Duration::from_millis(130)));
        assert_eq!(timing.snapshot().first_token_at, Some(1_120));
        assert_eq!(timing.time_to_first_token_ms(), Some(120));
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
