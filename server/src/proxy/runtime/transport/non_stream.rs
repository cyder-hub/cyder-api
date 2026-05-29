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
        logging::{LoggedBody, RequestLogContext},
        provider_governance::{record_provider_failure, record_provider_success},
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::{
                append_response_transform_diagnostics, finalize_non_streaming_log_context,
                record_immediate_completion_if_allowed,
            },
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
            reasoning_content_repair::{
                ReasoningContentRepairResultKey, continuation_snapshots_from_openai_response_body,
            },
        },
        util::{
            json_top_level_field_count_from_bytes, parse_utility_usage_normalization, sha256_hex,
        },
    },
    schema::enum_def::{LlmApiType, RequestStatus},
    service::{
        app_state::AppState,
        cache::types::CacheCostCatalogVersion,
        runtime::{ProviderCircuitProbePermit, ReasoningContinuationCacheKey},
        transform::unified::{
            UnifiedTransformDiagnostic, UnifiedTransformDiagnosticAction,
            UnifiedTransformDiagnosticKind, UnifiedTransformDiagnosticLossLevel,
        },
    },
};
use serde::Serialize;
use tokio::sync::Mutex as TokioMutex;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct ReasoningContentCaptureReport {
    pub captured_count: usize,
    pub diagnostics: Vec<ReasoningContentCaptureDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct ReasoningContentCaptureDiagnostic {
    pub result: ReasoningContentRepairResultKey,
    pub captured_count: usize,
    pub tool_call_ids: Vec<String>,
    pub tool_calls_hash: Option<String>,
    pub detail: Option<String>,
}

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
            context.llm_response_body =
                Some(LoggedBody::from_bytes(Bytes::from(proxy_error.to_string())));
            record_immediate_completion_if_allowed(app_state, &context, log_mode, execution_policy)
                .await;
            api_key_request_lease.release().await;

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
        let reasoning_capture_report = capture_non_stream_reasoning_continuation(
            app_state,
            reasoning_capture,
            response_mode,
            &decompressed_body,
            llm_response_completed_at,
        )
        .await;
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
        let reasoning_capture_diagnostics =
            reasoning_capture_transform_diagnostics(&reasoning_capture_report);
        append_response_transform_diagnostics(&mut context, &reasoning_capture_diagnostics);
        record_immediate_completion_if_allowed(app_state, &context, log_mode, execution_policy)
            .await;
        if execution_policy.records_provider_runtime() {
            record_provider_success(
                app_state,
                provider_id,
                &model_str,
                provider_circuit_permit.as_ref(),
            )
            .await;
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
            decompressed_body.clone(),
            decompressed_body.clone(),
        );
        record_immediate_completion_if_allowed(app_state, &context, log_mode, execution_policy)
            .await;
        let proxy_error = classify_upstream_status(status_code, &decompressed_body);
        if execution_policy.records_provider_runtime() {
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
            response_headers: Some(response_headers),
        })
    }
}

pub(super) async fn capture_non_stream_reasoning_continuation(
    app_state: &Arc<AppState>,
    reasoning_capture: Option<&ReasoningContinuationCaptureContext>,
    response_mode: ProxyResponseMode,
    body: &Bytes,
    observed_at_ms: i64,
) -> ReasoningContentCaptureReport {
    let Some(reasoning_capture) = reasoning_capture else {
        return single_capture_result(ReasoningContentRepairResultKey::Disabled, 0, None);
    };
    if !reasoning_capture.feature_enabled {
        return single_capture_result(ReasoningContentRepairResultKey::Disabled, 0, None);
    }
    if !target_is_openai_compatible_generation(response_mode) {
        return single_capture_result(ReasoningContentRepairResultKey::NotApplicable, 0, None);
    }

    let snapshots = match continuation_snapshots_from_openai_response_body(
        reasoning_capture.scope.clone(),
        body,
        observed_at_ms,
    ) {
        Ok(snapshots) => snapshots,
        Err(result) => return single_capture_result(result, 0, Some("response_body".to_string())),
    };
    if snapshots.is_empty() {
        return single_capture_result(ReasoningContentRepairResultKey::NotApplicable, 0, None);
    }

    let mut report = ReasoningContentCaptureReport::default();
    for snapshot in snapshots {
        let key = snapshot.key.clone();
        match app_state
            .reasoning_continuation_store
            .insert(snapshot, observed_at_ms)
            .await
        {
            Ok(()) => {
                report.captured_count += 1;
                report.diagnostics.push(capture_diagnostic_for_key(
                    ReasoningContentRepairResultKey::Matched,
                    1,
                    &key,
                    None,
                ));
            }
            Err(err) => {
                report.diagnostics.push(capture_diagnostic_for_key(
                    ReasoningContentRepairResultKey::CacheMiss,
                    0,
                    &key,
                    Some(format!("store_error={err}")),
                ));
            }
        }
    }

    report
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

fn single_capture_result(
    result: ReasoningContentRepairResultKey,
    captured_count: usize,
    detail: Option<String>,
) -> ReasoningContentCaptureReport {
    ReasoningContentCaptureReport {
        captured_count,
        diagnostics: vec![ReasoningContentCaptureDiagnostic {
            result,
            captured_count,
            tool_call_ids: Vec::new(),
            tool_calls_hash: None,
            detail,
        }],
    }
}

fn capture_diagnostic_for_key(
    result: ReasoningContentRepairResultKey,
    captured_count: usize,
    key: &ReasoningContinuationCacheKey,
    detail: Option<String>,
) -> ReasoningContentCaptureDiagnostic {
    ReasoningContentCaptureDiagnostic {
        result,
        captured_count,
        tool_call_ids: key.tool_call_ids.clone(),
        tool_calls_hash: Some(key.tool_calls_hash.clone()),
        detail,
    }
}

pub(super) fn reasoning_capture_transform_diagnostics(
    report: &ReasoningContentCaptureReport,
) -> Vec<UnifiedTransformDiagnostic> {
    report
        .diagnostics
        .iter()
        .map(reasoning_capture_transform_diagnostic)
        .collect()
}

fn reasoning_capture_transform_diagnostic(
    diagnostic: &ReasoningContentCaptureDiagnostic,
) -> UnifiedTransformDiagnostic {
    UnifiedTransformDiagnostic {
        type_: "runtime_feature_diagnostic".to_string(),
        diagnostic_kind: UnifiedTransformDiagnosticKind::CapabilityDowngrade,
        provider: "openai_compatible".to_string(),
        target_provider: "openai_compatible".to_string(),
        source: "upstream_response".to_string(),
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
