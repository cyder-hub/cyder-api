use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    http::header::CONTENT_TYPE,
};
use chrono::Utc;

use super::{
    ProxyRequestFailure, ProxyRequestOutcome, ProxyResponseMode,
    body::{
        BODY_FRAME_CHANNEL_CAPACITY, BodyFrame, FrameDeliveryError, GuardedBodyStream,
        guarded_body, send_body_frame,
    },
    client::{capture_error_response_with_deadline, read_complete_response_with_cancellation},
    lifecycle::{
        ProxyTerminationCause, ProxyTerminationCoordinator, TotalWatchdogResult,
        await_cleanup_with_total_watchdog,
    },
    response::{build_response_builder, process_success_response_body, response_content_type},
};
use crate::{
    config::{NonStreamResponseConfig, ProxyTimeoutConfig},
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
        cancellation::ProxyCancellationContext,
        classify_upstream_status_captured,
        logging::RequestLogContext,
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::{apply_final_error_fact, finalize_non_streaming_log_context},
        },
        source_governance::{
            record_source_failure_or_release_probe, record_source_success, release_source_probe,
        },
        util::{
            json_top_level_field_count_from_bytes, parse_utility_usage_normalization, sha256_hex,
        },
    },
    schema::enum_def::RequestStatus,
    service::{
        app_state::AppState,
        cache::types::CacheCostCatalogVersion,
        runtime::SourceCircuitProbePermit,
        upstream_response::parse_content_encoding,
        upstream_response::{
            ResponseBodyReadTimeouts, UpstreamResponseReadError,
            read_complete_response_body_with_timeouts,
        },
    },
};
use tokio::sync::Mutex as TokioMutex;

pub(super) async fn handle_non_streaming_response(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    source_id: i64,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    mut api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    source_circuit_permit: Option<SourceCircuitProbePermit>,
    response_mode: ProxyResponseMode,
    upstream_error_body_limit_bytes: usize,
    non_stream_response_limits: &NonStreamResponseConfig,
    response_visibility: ResponseVisibilityTracker,
    proxy_timeouts: &ProxyTimeoutConfig,
) -> Result<ProxyRequestOutcome, ProxyRequestFailure> {
    let status_code = response.status();
    let response_headers = response.headers().clone();
    let (request_id, log_id) = {
        let context = log_context.lock().await;
        (context.request_id.clone(), context.id)
    };
    let content_encoding = parse_content_encoding(&response_headers)
        .map(|encoding| encoding.as_str())
        .unwrap_or("invalid");
    crate::debug_event!(
        "proxy.response_headers_received",
        request_id = &request_id,
        log_id = log_id,
        status_code = status_code.as_u16(),
        response_header_count = response_headers.len(),
        content_type = response_content_type(&response_headers),
        content_encoding = content_encoding,
    );
    let response_builder = build_response_builder(status_code, &response_headers);

    let body = if status_code.is_success() {
        read_complete_response_with_cancellation(response, cancellation, non_stream_response_limits)
            .await
            .map(NonStreamResponseBody::Complete)
    } else {
        capture_error_response_with_deadline(
            response,
            cancellation,
            proxy_timeouts,
            non_stream_response_limits,
            upstream_error_body_limit_bytes,
        )
        .await
        .map(NonStreamResponseBody::Captured)
    };
    let body = match body {
        Ok(body) => body,
        Err(proxy_error) => {
            cancellation.try_terminate_error(&proxy_error);
            record_source_failure_or_release_probe(
                app_state,
                cancellation,
                source_id,
                &model_str,
                &proxy_error,
                source_circuit_permit.as_ref(),
            )
            .await;
            let completed_at = Utc::now().timestamp_millis();

            let mut context = log_context.lock().await;
            context.request_url = Some(url.to_string());
            context.llm_status = Some(status_code);
            context.completed_at = Some(completed_at);
            context.cost_catalog_version = cost_catalog_version.cloned();
            context.overall_status = if proxy_error.code() == ProxyErrorCode::ClientCancelledError {
                RequestStatus::Cancelled
            } else {
                RequestStatus::Error
            };
            api_key_request_lease.release().await;

            return Err(ProxyRequestFailure {
                error: proxy_error,
                log_context: context.clone(),
            });
        }
    };
    let completed_at = Utc::now().timestamp_millis();

    if status_code.is_success() {
        let NonStreamResponseBody::Complete(complete_body) = body else {
            unreachable!("successful upstream response must use complete read mode")
        };
        let decompressed_body = complete_body.bytes;
        crate::debug_event!(
            "proxy.response_body_read",
            request_id = &request_id,
            log_id = log_id,
            raw_response_body_bytes = complete_body.raw_bytes,
            decoded_response_body_bytes = complete_body.decoded_bytes,
            content_encoding = complete_body.encoding.as_str(),
        );
        let (final_body, parsed_usage_info, parsed_usage_normalization, _) = match response_mode {
            ProxyResponseMode::Generation {
                downstream_protocol,
                upstream_protocol,
            } => process_success_response_body(
                &decompressed_body,
                downstream_protocol,
                upstream_protocol,
            ),
            ProxyResponseMode::Utility { .. } => {
                let usage_normalization =
                    serde_json::from_slice::<serde_json::Value>(&decompressed_body)
                        .ok()
                        .and_then(|val| parse_utility_usage_normalization(&val));
                (
                    decompressed_body.clone(),
                    None,
                    usage_normalization,
                    Vec::new(),
                )
            }
        };

        let mut context = log_context.lock().await;
        finalize_non_streaming_log_context(
            &mut context,
            url,
            status_code,
            completed_at,
            cost_catalog_version,
            RequestStatus::Success,
            parsed_usage_info,
            parsed_usage_normalization,
        );
        if cancellation.try_provider_success() {
            record_source_success(
                app_state,
                source_id,
                &model_str,
                source_circuit_permit.as_ref(),
            )
            .await;
        }
        crate::debug_event!(
            "proxy.request_succeeded_debug",
            request_id = &context.request_id,
            log_id = context.id,
            model = &model_str,
            status_code = status_code.as_u16(),
            is_stream = false,
            latency_ms = completed_at.saturating_sub(context.request_received_at),
        );

        let response = match response_builder.body(Body::from(final_body)) {
            Ok(response) => response,
            Err(error) => {
                let proxy_error = ProxyError::gateway(
                    ProxyErrorCode::DownstreamSendError,
                    ExecutionStage::DownstreamSend,
                    response_visibility.current(),
                    None,
                    format!("Failed to build non-streaming downstream response: {error}"),
                );
                context.overall_status = RequestStatus::Error;
                apply_final_error_fact(&mut context, &proxy_error);
                cancellation.try_terminate_error(&proxy_error);
                api_key_request_lease.release().await;
                return Err(ProxyRequestFailure {
                    error: proxy_error,
                    log_context: context.clone(),
                });
            }
        };
        response_visibility.advance_to(ResponseVisibility::HeadersCommitted);
        api_key_request_lease.release().await;
        Ok(ProxyRequestOutcome {
            response,
            log_context: context.clone(),
        })
    } else {
        let NonStreamResponseBody::Captured(captured_body) = body else {
            unreachable!("failed upstream response must use capture mode")
        };
        let disclosure_truncated = captured_body.truncated;
        let hard_limit_reached = captured_body.hard_limit_reached.is_some();
        let hard_limit = captured_body.hard_limit_reached.map(|limit| limit.as_str());
        let decompressed_body = captured_body.captured;
        let mut context = log_context.lock().await;
        crate::error_event!(
            "proxy.upstream_error_body",
            request_id = &context.request_id,
            status_code = status_code.as_u16(),
            log_id = context.id,
            response_body_bytes = decompressed_body.len(),
            raw_response_body_bytes = captured_body.raw_bytes,
            decoded_response_body_bytes = captured_body.decoded_bytes,
            response_body_truncated = disclosure_truncated,
            response_body_hard_limit = hard_limit,
            content_encoding = captured_body.encoding.as_str(),
            response_body_sha256 = sha256_hex(&decompressed_body),
            json_top_level_fields = json_top_level_field_count_from_bytes(&decompressed_body),
            content_type = response_content_type(&response_headers),
        );

        finalize_non_streaming_log_context(
            &mut context,
            url,
            status_code,
            completed_at,
            cost_catalog_version,
            RequestStatus::Error,
            None,
            None,
        );
        let proxy_error = classify_upstream_status_captured(
            status_code,
            response_headers.get(CONTENT_TYPE),
            &decompressed_body,
            upstream_error_body_limit_bytes,
            disclosure_truncated,
            ResponseVisibility::NotVisible,
        );
        cancellation.try_terminate_error(&proxy_error);
        if hard_limit_reached {
            let governance_error = ProxyError::gateway(
                ProxyErrorCode::UpstreamResponseError,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
                None,
                format!(
                    "Provider error response reached the configured {} body limit",
                    hard_limit.unwrap_or("response")
                ),
            );
            record_source_failure_or_release_probe(
                app_state,
                cancellation,
                source_id,
                &model_str,
                &governance_error,
                source_circuit_permit.as_ref(),
            )
            .await;
        } else {
            record_source_failure_or_release_probe(
                app_state,
                cancellation,
                source_id,
                &model_str,
                &proxy_error,
                source_circuit_permit.as_ref(),
            )
            .await;
        }
        api_key_request_lease.release().await;
        Err(ProxyRequestFailure {
            error: proxy_error,
            log_context: context.clone(),
        })
    }
}

enum NonStreamResponseBody {
    Complete(crate::service::upstream_response::CompleteResponseBody),
    Captured(crate::service::upstream_response::CapturedErrorBody),
}

fn non_stream_body_io_error(proxy_error: &ProxyError) -> std::io::Error {
    let kind = if proxy_error.code() == ProxyErrorCode::UpstreamTimeoutError {
        std::io::ErrorKind::TimedOut
    } else if proxy_error.code() == ProxyErrorCode::ClientCancelledError {
        std::io::ErrorKind::ConnectionAborted
    } else {
        std::io::ErrorKind::Other
    };
    std::io::Error::new(kind, proxy_error.operator_message().to_string())
}

fn classify_guarded_non_stream_read_error(
    error: UpstreamResponseReadError,
    response_visibility: ResponseVisibility,
) -> ProxyError {
    if error.is_timeout() {
        ProxyError::upstream_timeout(
            error
                .timeout_phase()
                .unwrap_or(crate::proxy::TimeoutPhase::ResponseIdle),
            ExecutionStage::UpstreamResponse,
            response_visibility,
            format!("Failed to read complete upstream response: {error}"),
        )
    } else {
        ProxyError::gateway(
            ProxyErrorCode::UpstreamResponseError,
            ExecutionStage::UpstreamResponse,
            response_visibility,
            None,
            format!("Failed to read complete upstream response: {error}"),
        )
    }
}

fn guarded_non_stream_watchdog_phase(
    result: TotalWatchdogResult,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
) -> Option<crate::proxy::TimeoutPhase> {
    match result {
        TotalWatchdogResult::Expired => Some(crate::proxy::TimeoutPhase::Total),
        TotalWatchdogResult::Cancelled => {
            if cancellation.is_cancelled() {
                None
            } else {
                match coordinator.terminal() {
                    Some(ProxyTerminationCause::Timeout { phase }) => Some(phase),
                    Some(ProxyTerminationCause::ClientCancelled) => None,
                    _ => Some(crate::proxy::TimeoutPhase::Total),
                }
            }
        }
    }
}

async fn finalize_non_streaming_error_and_release(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: axum::http::StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    api_key_request_lease: &mut ApiKeyRequestLeaseFinalizer,
    proxy_error: &ProxyError,
) {
    await_cleanup_with_total_watchdog(cancellation, coordinator, async {
        let context_snapshot = {
            let mut context = log_context.lock().await;
            finalize_non_streaming_log_context(
                &mut context,
                url,
                status_code,
                Utc::now().timestamp_millis(),
                cost_catalog_version,
                RequestStatus::Error,
                None,
                None,
            );
            apply_final_error_fact(&mut context, proxy_error);
            context.clone()
        };
        crate::logging::log_proxy_error_event(
            "proxy.non_stream_terminal_error",
            Some(context_snapshot.request_id.as_str()),
            Some(context_snapshot.id),
            proxy_error,
        );
        crate::proxy::runtime::log_writer::record_completion(app_state, context_snapshot).await;
        api_key_request_lease.release().await;
    })
    .await;
}

async fn finalize_guarded_non_stream_failure(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
    sender: &tokio::sync::mpsc::Sender<BodyFrame>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: axum::http::StatusCode,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    response_visibility: &ResponseVisibilityTracker,
    api_key_request_lease: &mut ApiKeyRequestLeaseFinalizer,
    proxy_error: &ProxyError,
) {
    if coordinator.terminal().is_some() {
        finalize_non_streaming_error_and_release(
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
        return;
    }

    let delivery = send_body_frame(
        sender,
        Err(non_stream_body_io_error(proxy_error)),
        cancellation,
        coordinator,
    )
    .await;
    let terminal_error = match delivery {
        Ok(()) => proxy_error.clone(),
        Err(FrameDeliveryError::ClientCancelled | FrameDeliveryError::DownstreamDropped) => {
            cancellation.cancel_now("downstream stopped consuming the guarded response body");
            return;
        }
        Err(FrameDeliveryError::Timeout { phase }) => {
            if proxy_error.timeout_phase() == Some(phase) {
                proxy_error.clone()
            } else {
                ProxyError::upstream_timeout(
                    phase,
                    ExecutionStage::DownstreamSend,
                    response_visibility.current(),
                    format!("guarded response body exceeded the {phase:?} timeout"),
                )
            }
        }
    };
    coordinator.try_terminate_error(&terminal_error);
    finalize_non_streaming_error_and_release(
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

#[allow(clippy::too_many_arguments)]
async fn run_guarded_non_stream_worker(
    app_state: Arc<AppState>,
    cancellation: ProxyCancellationContext,
    coordinator: ProxyTerminationCoordinator,
    source_id: i64,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: String,
    status_code: axum::http::StatusCode,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    mut api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    source_circuit_permit: Option<SourceCircuitProbePermit>,
    response_mode: ProxyResponseMode,
    proxy_timeouts: ProxyTimeoutConfig,
    non_stream_response_limits: NonStreamResponseConfig,
    response_visibility: ResponseVisibilityTracker,
    sender: tokio::sync::mpsc::Sender<BodyFrame>,
    request_id: crate::proxy::request_context::RequestId,
    log_id: i64,
) {
    let timing = cancellation.timing();
    let mut observe_chunk = move |chunk: &Bytes,
                                  wait_started_at: tokio::time::Instant,
                                  received_at: tokio::time::Instant| {
        timing.observe_upstream_raw_chunk(
            chunk,
            Utc::now().timestamp_millis(),
            wait_started_at,
            received_at,
        );
    };
    let read_future = read_complete_response_body_with_timeouts(
        response,
        &non_stream_response_limits,
        Some(ResponseBodyReadTimeouts {
            first_byte: proxy_timeouts.first_byte(),
            response_idle: proxy_timeouts.response_idle(),
        }),
        &mut observe_chunk,
    );
    tokio::pin!(read_future);
    let watchdog = coordinator.arm_total_watchdog(
        cancellation
            .total_deadline()
            .expect("proxy total deadline must be initialized before guarded Body"),
    );
    let body_result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            release_source_probe(
                &app_state,
                source_id,
                source_circuit_permit.as_ref(),
            )
            .await;
            return;
        },
        result = &mut read_future => result.map_err(|error| {
            classify_guarded_non_stream_read_error(error, response_visibility.current())
        }),
        watchdog_result = watchdog.wait() => match guarded_non_stream_watchdog_phase(
            watchdog_result,
            &cancellation,
            &coordinator,
        ) {
            Some(phase) => Err(ProxyError::upstream_timeout(
                phase,
                ExecutionStage::UpstreamResponse,
                response_visibility.current(),
                format!("complete upstream response exceeded the {phase:?} timeout"),
            )),
            None => return,
        },
    };
    let complete_body = match body_result {
        Ok(body) => body,
        Err(proxy_error) => {
            coordinator.try_terminate_error(&proxy_error);
            record_source_failure_or_release_probe(
                &app_state,
                &cancellation,
                source_id,
                &model_str,
                &proxy_error,
                source_circuit_permit.as_ref(),
            )
            .await;
            finalize_guarded_non_stream_failure(
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
    let upstream_completed_at = Utc::now().timestamp_millis();
    crate::debug_event!(
        "proxy.response_body_read",
        request_id = &request_id,
        log_id = log_id,
        raw_response_body_bytes = complete_body.raw_bytes,
        decoded_response_body_bytes = complete_body.decoded_bytes,
        content_encoding = complete_body.encoding.as_str(),
    );
    let (final_body, parsed_usage_info, parsed_usage_normalization, _) = match response_mode {
        ProxyResponseMode::Generation {
            downstream_protocol,
            upstream_protocol,
        } => process_success_response_body(
            &complete_body.bytes,
            downstream_protocol,
            upstream_protocol,
        ),
        ProxyResponseMode::Utility { .. } => {
            let usage_normalization =
                serde_json::from_slice::<serde_json::Value>(&complete_body.bytes)
                    .ok()
                    .and_then(|value| parse_utility_usage_normalization(&value));
            (
                complete_body.bytes.clone(),
                None,
                usage_normalization,
                Vec::new(),
            )
        }
    };
    if cancellation.try_provider_success() {
        record_source_success(
            &app_state,
            source_id,
            &model_str,
            source_circuit_permit.as_ref(),
        )
        .await;
    }
    let delivery = send_body_frame(&sender, Ok(final_body), &cancellation, &coordinator).await;
    match delivery {
        Ok(()) => {
            await_cleanup_with_total_watchdog(&cancellation, &coordinator, async {
                let context_snapshot = {
                    let mut context = log_context.lock().await;
                    finalize_non_streaming_log_context(
                        &mut context,
                        &url,
                        status_code,
                        Utc::now().timestamp_millis(),
                        cost_catalog_version.as_ref(),
                        RequestStatus::Success,
                        parsed_usage_info,
                        parsed_usage_normalization,
                    );
                    crate::debug_event!(
                        "proxy.request_succeeded_debug",
                        request_id = &context.request_id,
                        log_id = context.id,
                        model = &model_str,
                        status_code = status_code.as_u16(),
                        is_stream = false,
                        latency_ms = context
                            .completed_at
                            .unwrap_or_default()
                            .saturating_sub(context.request_received_at),
                    );
                    context.clone()
                };
                crate::proxy::runtime::log_writer::record_completion(&app_state, context_snapshot)
                    .await;
                api_key_request_lease.release().await;
            })
            .await;
            coordinator.try_terminate(ProxyTerminationCause::Success);
        }
        Err(FrameDeliveryError::ClientCancelled | FrameDeliveryError::DownstreamDropped) => {
            cancellation.cancel_now("downstream stopped consuming the guarded response body");
        }
        Err(FrameDeliveryError::Timeout { phase }) => {
            let proxy_error = ProxyError::upstream_timeout(
                phase,
                ExecutionStage::DownstreamSend,
                response_visibility.current(),
                format!("guarded response body exceeded the {phase:?} timeout"),
            );
            coordinator.try_terminate_error(&proxy_error);
            let context_snapshot = {
                let mut context = log_context.lock().await;
                finalize_non_streaming_log_context(
                    &mut context,
                    &url,
                    status_code,
                    Utc::now().timestamp_millis(),
                    cost_catalog_version.as_ref(),
                    RequestStatus::Error,
                    parsed_usage_info,
                    parsed_usage_normalization,
                );
                apply_final_error_fact(&mut context, &proxy_error);
                context.clone()
            };
            crate::logging::log_proxy_error_event(
                "proxy.non_stream_terminal_error",
                Some(context_snapshot.request_id.as_str()),
                Some(context_snapshot.id),
                &proxy_error,
            );
            crate::proxy::runtime::log_writer::record_completion(&app_state, context_snapshot)
                .await;
            api_key_request_lease.release().await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_non_streaming_response_guarded(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    source_id: i64,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    source_circuit_permit: Option<SourceCircuitProbePermit>,
    response_mode: ProxyResponseMode,
    upstream_error_body_limit_bytes: usize,
    non_stream_response_limits: &NonStreamResponseConfig,
    proxy_timeouts: ProxyTimeoutConfig,
    response_visibility: ResponseVisibilityTracker,
) -> Result<ProxyRequestOutcome, ProxyRequestFailure> {
    if !response.status().is_success() {
        return handle_non_streaming_response(
            app_state,
            cancellation,
            source_id,
            log_context,
            model_str,
            response,
            url,
            cost_catalog_version,
            api_key_request_lease,
            source_circuit_permit,
            response_mode,
            upstream_error_body_limit_bytes,
            non_stream_response_limits,
            response_visibility,
            &proxy_timeouts,
        )
        .await;
    }

    let status_code = response.status();
    let response_headers = response.headers().clone();
    let response_builder = build_response_builder(status_code, &response_headers);
    let (request_id, log_id) = {
        let context = log_context.lock().await;
        (context.request_id.clone(), context.id)
    };
    log_context.lock().await.defer_completion();
    response_visibility.advance_to(ResponseVisibility::HeadersCommitted);
    let coordinator = cancellation.coordinator();
    let (sender, receiver) = tokio::sync::mpsc::channel(BODY_FRAME_CHANNEL_CAPACITY);
    let worker = app_state
        .infra
        .spawn_background_task(run_guarded_non_stream_worker(
            Arc::clone(app_state),
            cancellation.clone(),
            coordinator.clone(),
            source_id,
            log_context.clone(),
            model_str,
            response,
            url.to_string(),
            status_code,
            cost_catalog_version.cloned(),
            api_key_request_lease.with_coordinator(coordinator.clone()),
            source_circuit_permit,
            response_mode,
            proxy_timeouts,
            non_stream_response_limits.clone(),
            response_visibility.clone(),
            sender,
            request_id,
            log_id,
        ));
    let body = guarded_body(GuardedBodyStream::new(
        Arc::clone(app_state),
        cancellation.clone(),
        coordinator,
        receiver,
        worker,
        log_context.clone(),
        url.to_string(),
        status_code,
        cost_catalog_version.cloned(),
        response_visibility.clone(),
        format!("Client disconnected while receiving response body for log_id {log_id}."),
    ));
    match response_builder.body(body) {
        Ok(response) => Ok(ProxyRequestOutcome {
            response,
            log_context: log_context.lock().await.clone(),
        }),
        Err(error) => {
            let proxy_error = ProxyError::gateway(
                ProxyErrorCode::DownstreamSendError,
                ExecutionStage::DownstreamSend,
                response_visibility.current(),
                None,
                format!("Failed to build non-streaming downstream response: {error}"),
            );
            cancellation.try_terminate_error(&proxy_error);
            Err(ProxyRequestFailure {
                error: proxy_error,
                log_context: log_context.lock().await.clone(),
            })
        }
    }
}
