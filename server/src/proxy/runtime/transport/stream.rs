use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::{Body, Bytes},
    http::StatusCode,
    response::Response,
};
use chrono::Utc;
use cyder_tools::log::{debug, error};
use futures::StreamExt;
use tokio::{
    sync::{Mutex as TokioMutex, mpsc},
    time::timeout,
};

use super::{cancellation::ResponseStreamCancellationGuard, response::build_response_builder};
use crate::{
    config::CONFIG,
    proxy::{
        ProxyError,
        cancellation::ProxyCancellationContext,
        classify_upstream_status,
        logging::{LogBodyKind, LoggedBody, RequestLogContext, StreamingBodyWriter},
        protocol_transform_error,
        provider_governance::{record_provider_failure, record_provider_success},
        runtime::{
            log_writer::{
                finalize_cancelled_log_context, finalize_streaming_log_context,
                record_streaming_completion_if_allowed,
            },
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
        },
    },
    schema::enum_def::{LlmApiType, RequestStatus},
    service::{
        app_state::AppState, cache::types::CacheCostCatalogVersion,
        runtime::ApiKeyConcurrencyGuard, transform::StreamTransformer,
    },
    utils::{sse::SseParser, storage::LogBodyCaptureState},
};

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
) -> Option<Duration> {
    if first_chunk_received_at_proxy == 0 {
        CONFIG.proxy_request.first_byte_timeout()
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

async fn abort_stream_body_writers(
    llm_body_writer: &mut Option<StreamingBodyWriter>,
    user_body_writer: &mut Option<StreamingBodyWriter>,
) {
    if let Some(writer) = llm_body_writer.take() {
        let _ = writer.abort().await;
    }
    if let Some(writer) = user_body_writer.take() {
        let _ = writer.abort().await;
    }
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
    abort_stream_body_writers(llm_body_writer, user_body_writer).await;
    if execution_policy.records_request_log() {
        finalize_cancelled_log_context(
            app_state,
            log_context,
            url,
            Some(status_code),
            cost_catalog_version,
            None,
            None,
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
    api_key_concurrency_guard: Option<ApiKeyConcurrencyGuard>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
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
    let llm_body_writer = StreamingBodyWriter::new(LogBodyKind::LlmResponse, log_id)
        .await
        .map_err(|e| {
            ProxyError::InternalError(format!("Failed to create LLM stream spool writer: {e}"))
        })?;
    let user_body_writer = StreamingBodyWriter::new(LogBodyKind::UserResponse, log_id)
        .await
        .map_err(|e| {
            ProxyError::InternalError(format!("Failed to create user stream spool writer: {e}"))
        })?;

    let monitored_stream = async_stream::stream! {
        let _api_key_concurrency_guard = api_key_concurrency_guard;
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

        loop {
            let chunk_result = match next_stream_chunk_timeout_duration(first_chunk_received_at_proxy) {
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
                                )
                                .await;
                            }

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
                            )
                            .await;
                        }

                        yield Err(std::io::Error::other(stream_error_message));
                        return;
                    }

                    {
                        let mut context = log_context_clone.lock().await;
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

                    let transformed_events = transformer.transform_events(events);
                    sync_stream_usage_to_log_context(&log_context_clone, &mut transformer).await;
                    let mut transformed_chunk_bytes: Vec<u8> = Vec::new();

                    for transformed_event in transformed_events {
                        if target_api_type == LlmApiType::Ollama {
                            transformed_chunk_bytes
                                .extend_from_slice(transformed_event.data.as_bytes());
                            transformed_chunk_bytes.push(b'\n');
                        } else {
                            transformed_chunk_bytes.extend_from_slice(&transformed_event.to_bytes());
                        }
                    }

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
                            )
                            .await;
                        }

                        yield Err(std::io::Error::other(stream_error_message));
                        return;
                    }

                    {
                        let mut context = log_context_clone.lock().await;
                        context.user_response_body =
                            Some(user_body_writer.as_ref().expect("user stream writer should exist").snapshot(LogBodyCaptureState::Incomplete));
                    }

                    if !transformed_chunk.is_empty() {
                        mark_stream_response_started_to_client(
                            &log_context_clone,
                            &transformed_chunk,
                        )
                        .await;
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
                        )
                        .await;
                    }

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
                    )
                    .await;
                }

                yield Err(std::io::Error::other(stream_error_message));
                return;
            }
            {
                let mut context = log_context_clone.lock().await;
                context.user_response_body =
                    Some(user_body_writer.as_ref().expect("user stream writer should exist").snapshot(LogBodyCaptureState::Incomplete));
            }
            mark_stream_response_started_to_client(&log_context_clone, &done_chunk).await;
            yield Ok::<_, std::io::Error>(done_chunk);
        }

        let llm_response_completed_at = Utc::now().timestamp_millis();

        if status_code.is_success() {
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
            record_streaming_completion_if_allowed(
                &app_state_clone,
                &context,
                log_mode,
                execution_policy,
            )
            .await;
            if execution_policy.records_provider_runtime() {
                record_provider_success(&app_state_clone, provider_id, &model_str).await;
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
                record_provider_failure(&app_state_clone, provider_id, &model_str, &proxy_error).await;
            }
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
