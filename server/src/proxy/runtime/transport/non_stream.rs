use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    http::header::CONTENT_ENCODING,
};
use chrono::Utc;

use super::{
    ProxyRequestFailure, ProxyRequestOutcome, ProxyResponseMode,
    ReasoningContinuationCaptureContext,
    client::read_response_bytes_with_cancellation,
    response::{
        build_response_builder, decode_response_body, process_success_response_body,
        response_content_type,
    },
};
use crate::{
    proxy::{
        ProxyError,
        cancellation::ProxyCancellationContext,
        classify_upstream_status,
        logging::RequestLogContext,
        provider_governance::{record_provider_failure, record_provider_success},
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::finalize_non_streaming_log_context,
            reasoning_content_repair::continuation_snapshots_from_openai_response_body,
        },
        util::{
            json_top_level_field_count_from_bytes, parse_utility_usage_normalization, sha256_hex,
        },
    },
    schema::enum_def::{LlmApiType, RequestStatus},
    service::{
        app_state::AppState, cache::types::CacheCostCatalogVersion,
        runtime::ProviderCircuitProbePermit,
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
) -> Result<ProxyRequestOutcome, ProxyRequestFailure> {
    let status_code = response.status();
    let response_headers = response.headers().clone();
    crate::debug_event!(
        "proxy.response_headers_received",
        status_code = status_code.as_u16(),
        response_header_count = response_headers.len(),
        content_type = response_content_type(&response_headers),
    );
    let is_gzip = response_headers
        .get(CONTENT_ENCODING)
        .map_or(false, |value| value.to_str().unwrap_or("").contains("gzip"));

    let response_builder = build_response_builder(status_code, &response_headers);

    let body_bytes = match read_response_bytes_with_cancellation(
        response,
        "Reading upstream response body",
        cancellation,
    )
    .await
    {
        Ok(b) => b,
        Err(proxy_error) => {
            if !matches!(proxy_error, ProxyError::ClientCancelled(_)) {
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
            context.overall_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
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

    let decompressed_body = decode_response_body(body_bytes, is_gzip);
    let llm_response_completed_at = Utc::now().timestamp_millis();

    if status_code.is_success() {
        capture_non_stream_reasoning_continuation(
            app_state,
            reasoning_capture,
            response_mode,
            &decompressed_body,
            llm_response_completed_at,
        )
        .await;
        let (final_body, parsed_usage_info, parsed_usage_normalization, _) = match response_mode {
            ProxyResponseMode::Generation {
                api_type,
                target_api_type,
            } => process_success_response_body(&decompressed_body, api_type, target_api_type),
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
            log_id = context.id,
            model = &model_str,
            status_code = status_code.as_u16(),
            is_stream = false,
            latency_ms = llm_response_completed_at.saturating_sub(context.request_received_at),
        );

        let response = response_builder.body(Body::from(final_body)).unwrap();
        api_key_request_lease.release().await;
        Ok(ProxyRequestOutcome {
            response,
            log_context: context.clone(),
        })
    } else {
        let mut context = log_context.lock().await;
        crate::error_event!(
            "proxy.upstream_error_body",
            status_code = status_code.as_u16(),
            log_id = context.id,
            response_body_bytes = decompressed_body.len(),
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
        let proxy_error = classify_upstream_status(status_code, &decompressed_body);
        record_provider_failure(
            app_state,
            provider_id,
            &model_str,
            &proxy_error,
            provider_circuit_permit.as_ref(),
        )
        .await;
        api_key_request_lease.release().await;
        Err(ProxyRequestFailure {
            error: proxy_error,
            log_context: context.clone(),
        })
    }
}

pub(super) async fn capture_non_stream_reasoning_continuation(
    app_state: &Arc<AppState>,
    reasoning_capture: Option<&ReasoningContinuationCaptureContext>,
    response_mode: ProxyResponseMode,
    body: &Bytes,
    observed_at_ms: i64,
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
            cyder_tools::log::debug!("Failed to cache reasoning continuation: {err}");
        }
    }
}

fn target_is_openai_compatible_generation(response_mode: ProxyResponseMode) -> bool {
    matches!(
        response_mode,
        ProxyResponseMode::Generation {
            target_api_type: LlmApiType::Openai | LlmApiType::GeminiOpenai,
            ..
        }
    )
}
