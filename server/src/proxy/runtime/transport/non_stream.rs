use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    http::header::CONTENT_ENCODING,
};
use chrono::Utc;

use super::{
    ProxyRequestFailure, ProxyRequestOutcome, ProxyResponseMode,
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
        logging::{LoggedBody, RequestLogContext},
        provider_governance::{record_provider_failure, record_provider_success},
        runtime::{
            log_writer::{
                append_response_transform_diagnostics, finalize_non_streaming_log_context,
                record_immediate_completion_if_allowed,
            },
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
        },
        util::{
            json_top_level_field_count_from_bytes, parse_utility_usage_normalization, sha256_hex,
        },
    },
    schema::enum_def::RequestStatus,
    service::{
        app_state::AppState, cache::types::CacheCostCatalogVersion, runtime::ApiKeyConcurrencyGuard,
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
    _api_key_concurrency_guard: Option<ApiKeyConcurrencyGuard>,
    response_mode: ProxyResponseMode,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
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
            if execution_policy.records_provider_runtime()
                && !matches!(proxy_error, ProxyError::ClientCancelled(_))
            {
                record_provider_failure(app_state, provider_id, &model_str, &proxy_error).await;
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
            context.llm_response_body =
                Some(LoggedBody::from_bytes(Bytes::from(proxy_error.to_string())));
            record_immediate_completion_if_allowed(app_state, &context, log_mode, execution_policy)
                .await;

            return Err(ProxyRequestFailure {
                error: proxy_error,
                log_context: context.clone(),
                response_headers: Some(response_headers),
            });
        }
    };

    let decompressed_body = decode_response_body(body_bytes, is_gzip);
    let llm_response_completed_at = Utc::now().timestamp_millis();

    if status_code.is_success() {
        let (final_body, parsed_usage_info, parsed_usage_normalization, transform_diagnostics) =
            match response_mode {
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
            decompressed_body.clone(),
            final_body.clone(),
        );
        append_response_transform_diagnostics(&mut context, &transform_diagnostics);
        record_immediate_completion_if_allowed(app_state, &context, log_mode, execution_policy)
            .await;
        if execution_policy.records_provider_runtime() {
            record_provider_success(app_state, provider_id, &model_str).await;
        }
        crate::debug_event!(
            "proxy.request_succeeded_debug",
            log_id = context.id,
            model = &model_str,
            status_code = status_code.as_u16(),
            is_stream = false,
            latency_ms = llm_response_completed_at.saturating_sub(context.request_received_at),
        );

        let response = response_builder.body(Body::from(final_body)).unwrap();
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
            decompressed_body.clone(),
            decompressed_body.clone(),
        );
        record_immediate_completion_if_allowed(app_state, &context, log_mode, execution_policy)
            .await;
        let proxy_error = classify_upstream_status(status_code, &decompressed_body);
        if execution_policy.records_provider_runtime() {
            record_provider_failure(app_state, provider_id, &model_str, &proxy_error).await;
        }
        Err(ProxyRequestFailure {
            error: proxy_error,
            log_context: context.clone(),
            response_headers: Some(response_headers),
        })
    }
}
