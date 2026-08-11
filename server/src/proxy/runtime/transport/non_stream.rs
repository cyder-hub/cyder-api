use std::sync::Arc;

use axum::{body::Body, http::header::CONTENT_TYPE};
use chrono::Utc;

use super::{
    ProxyRequestFailure, ProxyRequestOutcome, ProxyResponseMode,
    client::{capture_error_response_with_deadline, read_complete_response_with_cancellation},
    response::{build_response_builder, process_success_response_body, response_content_type},
};
use crate::{
    config::{NonStreamResponseConfig, ProxyTimeoutConfig},
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
        cancellation::ProxyCancellationContext,
        classify_transform_failure, classify_upstream_status_captured,
        logging::{
            RequestLogContext, TransformLogStage, log_transform_failure, log_transform_summary,
        },
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::{apply_final_error_fact, finalize_non_streaming_log_context},
        },
        util::{
            json_top_level_field_count_from_bytes, parse_utility_usage_normalization, sha256_hex,
        },
    },
    schema::enum_def::RequestStatus,
    service::{cache::types::CacheCostCatalogVersion, upstream_response::parse_content_encoding},
};
use tokio::sync::Mutex as TokioMutex;

pub(super) async fn handle_non_streaming_response(
    cancellation: &ProxyCancellationContext,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    mut api_key_request_lease: ApiKeyRequestLeaseFinalizer,
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
        let is_generation_response = matches!(response_mode, ProxyResponseMode::Generation { .. });
        let transformed = match response_mode {
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
                Ok((
                    decompressed_body.clone(),
                    None,
                    usage_normalization,
                    Default::default(),
                ))
            }
        };
        let (final_body, parsed_usage_info, parsed_usage_normalization, transform_summary) =
            match transformed {
                Ok(output) => output,
                Err(failure) => {
                    let proxy_error =
                        classify_transform_failure(&failure, response_visibility.current());
                    cancellation.try_terminate_error(&proxy_error);
                    let mut context = log_context.lock().await;
                    context.request_url = Some(url.to_string());
                    context.llm_status = Some(status_code);
                    context.completed_at = Some(completed_at);
                    context.cost_catalog_version = cost_catalog_version.cloned();
                    context.overall_status = RequestStatus::Error;
                    apply_final_error_fact(&mut context, &proxy_error);
                    log_transform_failure(TransformLogStage::Response, &context, &failure);
                    api_key_request_lease.release().await;
                    return Err(ProxyRequestFailure {
                        error: proxy_error,
                        log_context: context.clone(),
                    });
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
        if is_generation_response {
            log_transform_summary(TransformLogStage::Response, &context, &transform_summary);
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
