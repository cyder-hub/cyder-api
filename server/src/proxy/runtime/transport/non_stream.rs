use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    http::header::CONTENT_TYPE,
};
use chrono::Utc;

use super::{
    ProxyRequestFailure, ProxyRequestOutcome, ProxyResponseMode,
    ReasoningContinuationCaptureContext,
    client::{capture_error_response_with_cancellation, read_complete_response_with_cancellation},
    response::{build_response_builder, process_success_response_body, response_content_type},
};
use crate::{
    config::NonStreamResponseConfig,
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
        cancellation::ProxyCancellationContext,
        classify_upstream_status_captured,
        logging::RequestLogContext,
        provider_governance::{record_provider_failure, record_provider_success},
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::{apply_final_error_fact, finalize_non_streaming_log_context},
            reasoning_content_repair::continuation_snapshots_from_openai_response_body,
        },
        util::{
            json_top_level_field_count_from_bytes, parse_utility_usage_normalization, sha256_hex,
        },
    },
    schema::enum_def::{RequestStatus, UpstreamProtocol},
    service::{
        app_state::AppState, cache::types::CacheCostCatalogVersion,
        runtime::ProviderCircuitProbePermit, upstream_response::parse_content_encoding,
    },
};
use tokio::sync::Mutex as TokioMutex;

pub(super) async fn handle_non_streaming_response(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    provider_id: i64,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    mut api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    provider_circuit_permit: Option<ProviderCircuitProbePermit>,
    response_mode: ProxyResponseMode,
    reasoning_capture: Option<&ReasoningContinuationCaptureContext>,
    upstream_error_body_limit_bytes: usize,
    non_stream_response_limits: &NonStreamResponseConfig,
    response_visibility: ResponseVisibilityTracker,
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
        capture_error_response_with_cancellation(
            response,
            cancellation,
            non_stream_response_limits,
            upstream_error_body_limit_bytes,
        )
        .await
        .map(NonStreamResponseBody::Captured)
    };
    let body = match body {
        Ok(body) => body,
        Err(proxy_error) => {
            if proxy_error.code() != ProxyErrorCode::ClientCancelledError {
                record_provider_failure(
                    app_state,
                    provider_id,
                    &model_str,
                    &proxy_error,
                    provider_circuit_permit.as_ref(),
                )
                .await;
            }
            let completed_at = Utc::now().timestamp_millis();

            let mut context = log_context.lock().await;
            context.request_url = Some(url.to_string());
            context.llm_status = Some(status_code);
            context.completion_ts = Some(completed_at);
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
    let llm_response_completed_at = Utc::now().timestamp_millis();

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
        capture_non_stream_reasoning_continuation(
            app_state,
            reasoning_capture,
            response_mode,
            &decompressed_body,
            llm_response_completed_at,
            &request_id,
        )
        .await;
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
            llm_response_completed_at,
            cost_catalog_version,
            RequestStatus::Success,
            parsed_usage_info,
            parsed_usage_normalization,
        );
        record_provider_success(
            app_state,
            provider_id,
            &model_str,
            provider_circuit_permit.as_ref(),
        )
        .await;
        crate::debug_event!(
            "proxy.request_succeeded_debug",
            request_id = &context.request_id,
            log_id = context.id,
            model = &model_str,
            status_code = status_code.as_u16(),
            is_stream = false,
            latency_ms = llm_response_completed_at.saturating_sub(context.request_received_at),
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
            llm_response_completed_at,
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
            record_provider_failure(
                app_state,
                provider_id,
                &model_str,
                &governance_error,
                provider_circuit_permit.as_ref(),
            )
            .await;
        } else {
            record_provider_failure(
                app_state,
                provider_id,
                &model_str,
                &proxy_error,
                provider_circuit_permit.as_ref(),
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

pub(super) async fn capture_non_stream_reasoning_continuation(
    app_state: &Arc<AppState>,
    reasoning_capture: Option<&ReasoningContinuationCaptureContext>,
    response_mode: ProxyResponseMode,
    body: &Bytes,
    observed_at_ms: i64,
    request_id: &crate::proxy::request_context::RequestId,
) {
    let Some(reasoning_capture) = reasoning_capture else {
        return;
    };
    if !reasoning_capture.feature_enabled {
        return;
    }
    if !target_is_openai_compatible_generation(response_mode) {
        return;
    }

    let snapshots = match continuation_snapshots_from_openai_response_body(
        reasoning_capture.scope.clone(),
        body,
        observed_at_ms,
    ) {
        Ok(snapshots) => snapshots,
        Err(_) => return,
    };

    for snapshot in snapshots {
        if let Err(err) = app_state
            .reasoning_continuation_store
            .insert(snapshot, observed_at_ms)
            .await
        {
            crate::debug_event!(
                "proxy.reasoning_continuation_cache_failed",
                request_id = request_id,
                error = err,
            );
        }
    }
}

fn target_is_openai_compatible_generation(response_mode: ProxyResponseMode) -> bool {
    matches!(
        response_mode,
        ProxyResponseMode::Generation {
            upstream_protocol: UpstreamProtocol::Openai,
            ..
        }
    )
}
