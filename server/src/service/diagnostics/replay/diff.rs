use crate::{
    schema::enum_def::{RequestAttemptStatus, RequestReplayStatus, RequestStatus},
    service::diagnostics::{
        body::{compare_replay_body_capture, normalized_name_values, parse_name_values_json_map},
        replay::{
            source::{AttemptReplaySource, GatewayReplaySource},
            transport::AttemptReplayExecutionOutcome,
            types::{
                RequestReplayArtifactDiff, RequestReplayDiffBaselineKind,
                RequestReplayExecutionPreview, RequestReplayNameValue,
            },
        },
    },
};

pub(crate) fn build_attempt_replay_diff(
    source: &AttemptReplaySource,
    outcome: &AttemptReplayExecutionOutcome,
) -> RequestReplayArtifactDiff {
    let status_changed = if source.attempt.http_status.is_some() || outcome.http_status.is_some() {
        Some(source.attempt.http_status != outcome.http_status)
    } else {
        Some(!attempt_status_matches_replay_status(
            source.attempt.attempt_status,
            outcome.status,
        ))
    };

    let headers_changed =
        if !source.baseline_response_headers.is_empty() || !outcome.response_headers.is_empty() {
            Some(
                normalized_name_values(&source.baseline_response_headers)
                    != normalized_name_values(&outcome.response_headers),
            )
        } else {
            None
        };

    let body_comparison = compare_replay_body_capture(
        source
            .baseline_response_body
            .as_ref()
            .map(|body| (body.bytes.as_ref(), body.capture_state.as_deref())),
        outcome.response_body_bytes.as_ref().map(|body| {
            (
                body.as_ref(),
                outcome.response_body_capture_state.as_deref(),
            )
        }),
        "response body comparison was partial because one side lacked a captured body",
        "response body comparison was partial because one side had an incomplete capture",
    );
    let body_changed = body_comparison.changed;

    let token_delta = match (source.attempt.total_tokens, outcome.total_tokens) {
        (Some(left), Some(right)) => Some(right - left),
        _ => None,
    };
    let cost_delta = match (
        source.attempt.estimated_cost_nanos,
        outcome.estimated_cost_nanos,
    ) {
        (Some(left), Some(right)) => Some(right - left),
        _ => None,
    };

    let mut summary_lines = Vec::new();
    match (
        source.attempt.http_status,
        outcome.http_status,
        status_changed,
    ) {
        (Some(left), Some(right), Some(true)) => {
            summary_lines.push(format!("status changed: {} -> {}", left, right));
        }
        (Some(code), Some(_), Some(false)) => {
            summary_lines.push(format!("status unchanged: {}", code));
        }
        _ => summary_lines.push(
            "status comparison was partial because one side lacked an upstream HTTP status"
                .to_string(),
        ),
    }

    summary_lines.push(match headers_changed {
        Some(true) => "response headers changed".to_string(),
        Some(false) => "response headers unchanged".to_string(),
        None => "response header comparison was partial because one side lacked captured headers"
            .to_string(),
    });
    summary_lines.push(match body_changed {
        Some(true) if body_comparison.partial => {
            format!("response body changed; {}", body_comparison.reason)
        }
        Some(true) => "response body changed".to_string(),
        Some(false) => "response body unchanged".to_string(),
        None => body_comparison.reason,
    });
    summary_lines.push(match token_delta {
        Some(delta) => format!("total_tokens delta: {}", delta),
        None => "token comparison unavailable".to_string(),
    });
    summary_lines.push(match cost_delta {
        Some(delta) => format!("estimated_cost_nanos delta: {}", delta),
        None => "cost comparison unavailable".to_string(),
    });

    RequestReplayArtifactDiff {
        baseline_kind: RequestReplayDiffBaselineKind::OriginalAttempt,
        status_changed,
        headers_changed,
        body_changed,
        token_delta,
        cost_delta,
        summary_lines,
    }
}

pub(crate) fn build_gateway_replay_diff(
    source: &GatewayReplaySource,
    execution_preview: &RequestReplayExecutionPreview,
    outcome: &AttemptReplayExecutionOutcome,
) -> RequestReplayArtifactDiff {
    let status_comparison = build_gateway_status_comparison(source, outcome);

    let body_comparison = compare_replay_body_capture(
        source
            .baseline_user_response_body
            .as_ref()
            .map(|body| (body.bytes.as_ref(), body.capture_state.as_deref())),
        outcome.response_body_bytes.as_ref().map(|body| {
            (
                body.as_ref(),
                outcome.response_body_capture_state.as_deref(),
            )
        }),
        "user response body comparison was partial because one side lacked a captured body",
        "user response body comparison was partial because one side had an incomplete capture",
    );
    let body_changed = body_comparison.changed;
    let baseline_response_headers = gateway_baseline_response_headers(source);
    let headers_changed =
        if !baseline_response_headers.is_empty() || !outcome.response_headers.is_empty() {
            Some(
                normalized_name_values(&baseline_response_headers)
                    != normalized_name_values(&outcome.response_headers),
            )
        } else {
            None
        };
    let token_delta = match (source.request_log.total_tokens, outcome.total_tokens) {
        (Some(left), Some(right)) => Some(right - left),
        _ => None,
    };
    let cost_delta = match (
        source.request_log.estimated_cost_nanos,
        outcome.estimated_cost_nanos,
    ) {
        (Some(left), Some(right)) => Some(right - left),
        _ => None,
    };

    let mut summary_lines = Vec::new();
    let route_label = execution_preview
        .resolved_route
        .as_ref()
        .and_then(|route| route.route_name.clone())
        .unwrap_or_else(|| "unresolved route".to_string());
    let candidate_count = execution_preview.candidate_decisions.len();
    summary_lines.push(format!(
        "gateway replay executed via '{}' with {} observed candidate decision(s)",
        route_label, candidate_count
    ));
    summary_lines.push(match status_comparison.changed {
        true => format!(
            "gateway result changed: {} -> {}",
            status_comparison.baseline_label, status_comparison.replay_label
        ),
        false => format!(
            "gateway result unchanged: {}",
            status_comparison.replay_label
        ),
    });
    summary_lines.push(match headers_changed {
        Some(true) => "gateway response headers changed".to_string(),
        Some(false) => "gateway response headers unchanged".to_string(),
        None => {
            "gateway response header comparison unavailable because neither side had captured headers"
                .to_string()
        }
    });
    summary_lines.push(match body_changed {
        Some(true) if body_comparison.partial => {
            format!("user response body changed; {}", body_comparison.reason)
        }
        Some(true) => "user response body changed".to_string(),
        Some(false) => "user response body unchanged".to_string(),
        None => body_comparison.reason,
    });
    summary_lines.push(match token_delta {
        Some(delta) => format!("total_tokens delta: {}", delta),
        None => "token comparison unavailable".to_string(),
    });
    summary_lines.push(match cost_delta {
        Some(delta) => format!("estimated_cost_nanos delta: {}", delta),
        None => "cost comparison unavailable".to_string(),
    });

    RequestReplayArtifactDiff {
        baseline_kind: RequestReplayDiffBaselineKind::OriginalRequestResult,
        status_changed: Some(status_comparison.changed),
        headers_changed,
        body_changed,
        token_delta,
        cost_delta,
        summary_lines,
    }
}

pub(crate) fn rejected_diff(
    message: &str,
    baseline_kind: RequestReplayDiffBaselineKind,
) -> RequestReplayArtifactDiff {
    RequestReplayArtifactDiff {
        baseline_kind,
        status_changed: None,
        headers_changed: None,
        body_changed: None,
        token_delta: None,
        cost_delta: None,
        summary_lines: vec![message.to_string()],
    }
}

pub(crate) fn dry_run_diff(
    message: &str,
    baseline_kind: RequestReplayDiffBaselineKind,
) -> RequestReplayArtifactDiff {
    RequestReplayArtifactDiff {
        baseline_kind,
        status_changed: None,
        headers_changed: None,
        body_changed: None,
        token_delta: None,
        cost_delta: None,
        summary_lines: vec![message.to_string()],
    }
}

struct GatewayReplayStatusComparison {
    changed: bool,
    baseline_label: String,
    replay_label: String,
}

fn build_gateway_status_comparison(
    source: &GatewayReplaySource,
    outcome: &AttemptReplayExecutionOutcome,
) -> GatewayReplayStatusComparison {
    let baseline_http_status = source
        .baseline_final_attempt
        .as_ref()
        .and_then(|attempt| attempt.http_status);
    let baseline_error_code = source
        .baseline_final_attempt
        .as_ref()
        .and_then(|attempt| attempt.error_code.as_deref())
        .or(source.request_log.final_error_code.as_deref());
    let replay_error_code = outcome.error_code.as_deref();
    let request_status_changed =
        !request_status_matches_replay_status(&source.request_log.overall_status, &outcome.status);
    let http_status_changed = (source.baseline_final_attempt.is_some()
        || baseline_http_status.is_some()
        || outcome.http_status.is_some())
        && baseline_http_status != outcome.http_status;
    let error_code_changed = (baseline_error_code.is_some() || replay_error_code.is_some())
        && baseline_error_code != replay_error_code;

    GatewayReplayStatusComparison {
        changed: request_status_changed || http_status_changed || error_code_changed,
        baseline_label: format_gateway_baseline_result_label(
            &source.request_log.overall_status,
            baseline_http_status,
            baseline_error_code,
        ),
        replay_label: format_gateway_replay_result_label(
            &outcome.status,
            outcome.http_status,
            replay_error_code,
        ),
    }
}

fn gateway_baseline_response_headers(source: &GatewayReplaySource) -> Vec<RequestReplayNameValue> {
    source
        .baseline_final_attempt
        .as_ref()
        .and_then(|attempt| attempt.response_headers_json.as_deref())
        .and_then(|raw| parse_name_values_json_map(raw, "gateway baseline response headers").ok())
        .unwrap_or_default()
}

fn format_gateway_baseline_result_label(
    status: &RequestStatus,
    http_status: Option<i32>,
    error_code: Option<&str>,
) -> String {
    format_gateway_result_label(request_status_label(status), http_status, error_code)
}

fn format_gateway_replay_result_label(
    status: &RequestReplayStatus,
    http_status: Option<i32>,
    error_code: Option<&str>,
) -> String {
    format_gateway_result_label(request_replay_status_label(status), http_status, error_code)
}

fn format_gateway_result_label(
    status_label: &str,
    http_status: Option<i32>,
    error_code: Option<&str>,
) -> String {
    let mut parts = vec![status_label.to_string()];
    if let Some(http_status) = http_status {
        parts.push(format!("http={http_status}"));
    }
    if let Some(error_code) = error_code {
        parts.push(format!("error_code={error_code}"));
    }
    parts.join(" / ")
}

fn request_status_label(status: &RequestStatus) -> &'static str {
    match status {
        RequestStatus::Pending => "pending",
        RequestStatus::Success => "success",
        RequestStatus::Error => "error",
        RequestStatus::Cancelled => "cancelled",
    }
}

fn request_replay_status_label(status: &RequestReplayStatus) -> &'static str {
    match status {
        RequestReplayStatus::Pending => "pending",
        RequestReplayStatus::Running => "running",
        RequestReplayStatus::Success => "success",
        RequestReplayStatus::Error => "error",
        RequestReplayStatus::Cancelled => "cancelled",
        RequestReplayStatus::Rejected => "rejected",
    }
}

fn request_status_matches_replay_status(
    request_status: &RequestStatus,
    replay_status: &RequestReplayStatus,
) -> bool {
    matches!(
        (request_status, replay_status),
        (RequestStatus::Pending, RequestReplayStatus::Pending)
            | (RequestStatus::Pending, RequestReplayStatus::Running)
            | (RequestStatus::Success, RequestReplayStatus::Success)
            | (RequestStatus::Error, RequestReplayStatus::Error)
            | (RequestStatus::Cancelled, RequestReplayStatus::Cancelled)
    )
}

fn attempt_status_matches_replay_status(
    attempt_status: RequestAttemptStatus,
    replay_status: RequestReplayStatus,
) -> bool {
    matches!(
        (attempt_status, replay_status),
        (RequestAttemptStatus::Success, RequestReplayStatus::Success)
            | (RequestAttemptStatus::Error, RequestReplayStatus::Error)
            | (
                RequestAttemptStatus::Cancelled,
                RequestReplayStatus::Cancelled
            )
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::Bytes;
    use reqwest::header::HeaderMap;

    use crate::{
        database::{request_attempt::RequestAttemptDetail, request_log::RequestLogRecord},
        proxy::ProxyError,
        schema::enum_def::{
            Action, LlmApiType, ProviderApiKeyMode, ProviderType, RequestAttemptStatus,
            RequestReplaySemanticBasis, SchedulerAction, StorageType,
        },
        service::{
            cache::types::{CacheApiKey, CacheProvider},
            diagnostics::{
                body::{REPLAY_BODY_CAPTURE_INCOMPLETE, build_header_map_from_name_values},
                replay::{
                    source::{
                        AttemptReplaySource, DecodedBundleBody, GatewayReplaySource,
                        GatewayReplaySourceKind,
                    },
                    transport::{
                        AttemptReplayExecutionOutcome, execution_outcome_from_proxy_error,
                    },
                    types::{
                        RequestReplayExecutionPreview, RequestReplayNameValue,
                        RequestReplayResolvedCandidate, RequestReplayResolvedRoute,
                    },
                },
            },
        },
        utils::storage::RequestLogBundleRequestSnapshot,
    };

    use super::*;

    fn llm_api_type_for_provider(provider_type: &ProviderType) -> LlmApiType {
        match provider_type {
            ProviderType::Gemini | ProviderType::Vertex => LlmApiType::Gemini,
            ProviderType::Anthropic => LlmApiType::Anthropic,
            ProviderType::Responses => LlmApiType::Responses,
            ProviderType::GeminiOpenai => LlmApiType::GeminiOpenai,
            ProviderType::Ollama => LlmApiType::Ollama,
            ProviderType::Openai | ProviderType::VertexOpenai => LlmApiType::Openai,
        }
    }

    fn provider_key_and_name(provider_type: &ProviderType) -> (&'static str, &'static str) {
        match provider_type {
            ProviderType::Openai => ("openai", "OpenAI"),
            ProviderType::Gemini => ("gemini", "Gemini"),
            ProviderType::Vertex => ("vertex", "Vertex"),
            ProviderType::VertexOpenai => ("vertex-openai", "Vertex OpenAI"),
            ProviderType::Ollama => ("ollama", "Ollama"),
            ProviderType::Anthropic => ("anthropic", "Anthropic"),
            ProviderType::Responses => ("responses", "Responses"),
            ProviderType::GeminiOpenai => ("gemini-openai", "Gemini OpenAI"),
        }
    }

    fn source(request_uri: String, provider_type: ProviderType) -> AttemptReplaySource {
        let llm_api_type = llm_api_type_for_provider(&provider_type);
        let (provider_key, provider_name) = provider_key_and_name(&provider_type);
        let sanitized_request_headers = vec![
            RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            },
            RequestReplayNameValue {
                name: "x-trace-id".to_string(),
                value: Some("trace-1".to_string()),
            },
        ];
        let request_headers =
            build_header_map_from_name_values(&sanitized_request_headers).expect("headers");
        let baseline_response_body = DecodedBundleBody {
            bytes: Bytes::from_static(
                br#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"message":{"role":"assistant","content":"pong"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":3,"total_tokens":7}}"#,
            ),
            media_type: Some("application/json".to_string()),
            capture_state: Some("complete".to_string()),
        };

        AttemptReplaySource {
            request_log_id: 42,
            attempt: RequestAttemptDetail {
                id: 101,
                request_log_id: 42,
                attempt_index: 1,
                candidate_position: 1,
                provider_id: Some(2),
                provider_api_key_id: Some(3),
                model_id: Some(4),
                provider_key_snapshot: Some(provider_key.to_string()),
                provider_name_snapshot: Some(provider_name.to_string()),
                model_name_snapshot: Some("gpt-test".to_string()),
                real_model_name_snapshot: Some("gpt-4o-mini".to_string()),
                llm_api_type: Some(llm_api_type),
                attempt_status: RequestAttemptStatus::Success,
                http_status: Some(200),
                total_tokens: Some(7),
                estimated_cost_nanos: Some(100),
                estimated_cost_currency: Some("USD".to_string()),
                ..Default::default()
            },
            requested_model_name: Some("primary-high".to_string()),
            base_requested_model_name: Some("primary".to_string()),
            resolved_reasoning_suffix: Some("high".to_string()),
            resolved_reasoning_preset: Some("high".to_string()),
            resolved_route_id: Some(8),
            resolved_route_name: Some("primary".to_string()),
            provider: Arc::new(CacheProvider {
                id: 2,
                provider_key: provider_key.to_string(),
                name: provider_name.to_string(),
                endpoint: "https://upstream.example/v1".to_string(),
                use_proxy: false,
                provider_type,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
                is_enabled: true,
            }),
            llm_api_type,
            request_uri,
            sanitized_request_headers,
            request_headers,
            llm_request_body: DecodedBundleBody {
                bytes: Bytes::from_static(
                    br#"{"model":"gpt-4o-mini","messages":[{"role":"user","content":"ping"}]}"#,
                ),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            },
            baseline_response_headers: vec![RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            }],
            baseline_response_body: Some(baseline_response_body),
            cost_catalog_version: None,
        }
    }

    #[test]
    fn attempt_replay_diff_marks_partial_body_comparison_when_replay_body_missing() {
        let source = source(
            "https://upstream.example/v1/chat/completions".to_string(),
            ProviderType::Openai,
        );
        let outcome = execution_outcome_from_proxy_error(ProxyError::BadGateway(
            "upstream body missing".to_string(),
        ));

        let diff = build_attempt_replay_diff(&source, &outcome);

        assert_eq!(diff.body_changed, None);
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line.contains("partial"))
        );
    }

    #[test]
    fn attempt_replay_diff_marks_incomplete_capture_as_partial_not_unchanged() {
        let source = source(
            "https://upstream.example/v1/chat/completions".to_string(),
            ProviderType::Openai,
        );
        let outcome = AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Success,
            http_status: Some(200),
            first_byte_at: Some(1),
            error_code: None,
            error_message: None,
            response_headers: source.baseline_response_headers.clone(),
            response_body: None,
            response_body_bytes: source
                .baseline_response_body
                .as_ref()
                .map(|body| body.bytes.clone()),
            response_body_capture_state: Some(REPLAY_BODY_CAPTURE_INCOMPLETE.to_string()),
            response_body_capture: None,
            usage_normalization: None,
            transform_diagnostics: Vec::new(),
            estimated_cost_nanos: Some(100),
            estimated_cost_currency: Some("USD".to_string()),
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: Some(7),
        };

        let diff = build_attempt_replay_diff(&source, &outcome);

        assert_eq!(diff.body_changed, None);
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line.contains("partial") && line.contains("incomplete"))
        );
    }

    fn gateway_request_log(
        overall_status: RequestStatus,
        final_error_code: Option<&str>,
        final_attempt_id: Option<i64>,
    ) -> RequestLogRecord {
        RequestLogRecord {
            id: 42,
            api_key_id: 7,
            requested_model_name: Some("gpt-test".to_string()),
            base_requested_model_name: Some("gpt-test".to_string()),
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            resolved_name_scope: Some("direct".to_string()),
            resolved_route_id: Some(8),
            resolved_route_name: Some("primary".to_string()),
            user_api_type: LlmApiType::Openai,
            overall_status,
            final_error_code: final_error_code.map(str::to_string),
            final_error_message: None,
            attempt_count: 1,
            retry_count: 0,
            fallback_count: 0,
            request_received_at: 1,
            first_attempt_started_at: Some(2),
            response_started_to_client_at: Some(3),
            completed_at: Some(4),
            is_stream: false,
            client_ip: None,
            final_attempt_id,
            final_provider_id: Some(2),
            final_provider_api_key_id: Some(3),
            final_model_id: Some(4),
            final_provider_key_snapshot: Some("openai".to_string()),
            final_provider_name_snapshot: Some("OpenAI".to_string()),
            final_model_name_snapshot: Some("gpt-test".to_string()),
            final_real_model_name_snapshot: Some("gpt-4o-mini".to_string()),
            final_llm_api_type: Some(LlmApiType::Openai),
            estimated_cost_nanos: Some(100),
            estimated_cost_currency: Some("USD".to_string()),
            cost_catalog_id: None,
            cost_catalog_version_id: None,
            cost_snapshot_json: None,
            total_input_tokens: Some(4),
            total_output_tokens: Some(3),
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            total_tokens: Some(7),
            has_transform_diagnostics: false,
            transform_diagnostic_count: 0,
            transform_diagnostic_max_loss_level: None,
            bundle_version: Some(2),
            bundle_storage_type: Some(StorageType::FileSystem),
            bundle_storage_key: Some("logs/2026/04/23/42.mp.gz".to_string()),
            created_at: 1,
            updated_at: 4,
        }
    }

    fn gateway_final_attempt(
        http_status: Option<i32>,
        error_code: Option<&str>,
        response_headers_json: Option<&str>,
    ) -> RequestAttemptDetail {
        RequestAttemptDetail {
            id: 101,
            request_log_id: 42,
            attempt_index: 1,
            candidate_position: 1,
            provider_id: Some(2),
            provider_api_key_id: Some(3),
            model_id: Some(4),
            provider_key_snapshot: Some("openai".to_string()),
            provider_name_snapshot: Some("OpenAI".to_string()),
            model_name_snapshot: Some("gpt-test".to_string()),
            real_model_name_snapshot: Some("gpt-4o-mini".to_string()),
            llm_api_type: Some(LlmApiType::Openai),
            attempt_status: if http_status == Some(200) {
                RequestAttemptStatus::Success
            } else {
                RequestAttemptStatus::Error
            },
            scheduler_action: SchedulerAction::ReturnSuccess,
            error_code: error_code.map(str::to_string),
            response_headers_json: response_headers_json.map(str::to_string),
            http_status,
            ..Default::default()
        }
    }

    fn gateway_source_for_diff(
        request_log: RequestLogRecord,
        baseline_final_attempt: Option<RequestAttemptDetail>,
    ) -> GatewayReplaySource {
        GatewayReplaySource {
            request_log,
            request_snapshot: RequestLogBundleRequestSnapshot {
                request_path: "/ai/openai/v1/chat/completions".to_string(),
                operation_kind: "chat_completions_create".to_string(),
                ..Default::default()
            },
            original_headers: HeaderMap::new(),
            user_request_body: DecodedBundleBody {
                bytes: Bytes::from_static(
                    br#"{"model":"gpt-test","messages":[{"role":"user","content":"ping"}]}"#,
                ),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            },
            baseline_user_response_body: Some(DecodedBundleBody {
                bytes: Bytes::from_static(br#"{"ok":false}"#),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            }),
            baseline_final_attempt,
            api_key: Arc::new(CacheApiKey {
                id: 7,
                api_key_hash: "hash".to_string(),
                key_prefix: "ck-test".to_string(),
                key_last4: "1234".to_string(),
                name: "Test".to_string(),
                description: None,
                default_action: Action::Allow,
                is_enabled: true,
                expires_at: None,
                rate_limit_rpm: None,
                max_concurrent_requests: None,
                quota_daily_requests: None,
                quota_daily_tokens: None,
                quota_monthly_tokens: None,
                budget_daily_nanos: None,
                budget_daily_currency: None,
                budget_monthly_nanos: None,
                budget_monthly_currency: None,
                acl_rules: Vec::new(),
            }),
            requested_model_name: "gpt-test".to_string(),
            kind: GatewayReplaySourceKind::Generation {
                api_type: LlmApiType::Openai,
                is_stream: false,
                data: serde_json::json!({"model": "gpt-test"}),
                original_request_value: serde_json::json!({"model": "gpt-test"}),
            },
        }
    }

    fn gateway_execution_preview() -> RequestReplayExecutionPreview {
        RequestReplayExecutionPreview {
            semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
            requested_model_name: Some("gpt-test".to_string()),
            base_requested_model_name: Some("gpt-test".to_string()),
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            resolved_route: Some(RequestReplayResolvedRoute {
                route_id: Some(8),
                route_name: Some("primary".to_string()),
            }),
            resolved_candidate: Some(RequestReplayResolvedCandidate {
                candidate_position: Some(1),
                provider_id: Some(2),
                provider_api_key_id: Some(3),
                model_id: Some(4),
                llm_api_type: Some(LlmApiType::Openai),
            }),
            candidate_decisions: Vec::new(),
            applied_request_patch_summary: None,
            final_request_uri: Some("https://upstream.example/v1/chat/completions".to_string()),
            final_request_headers: Vec::new(),
            final_request_body: None,
        }
    }

    #[test]
    fn gateway_replay_diff_marks_status_and_headers_changed_when_failure_shape_changes() {
        let source = gateway_source_for_diff(
            gateway_request_log(
                RequestStatus::Error,
                Some("upstream_rate_limit_error"),
                Some(101),
            ),
            Some(gateway_final_attempt(
                Some(429),
                Some("upstream_rate_limit_error"),
                Some(r#"{"content-type":"application/json","retry-after":"1"}"#),
            )),
        );
        let outcome = AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Error,
            http_status: Some(503),
            first_byte_at: None,
            error_code: Some("upstream_service_unavailable".to_string()),
            error_message: Some("provider unavailable".to_string()),
            response_headers: vec![RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            }],
            response_body: None,
            response_body_bytes: None,
            response_body_capture_state: Some("not_captured".to_string()),
            response_body_capture: None,
            usage_normalization: None,
            transform_diagnostics: Vec::new(),
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        };

        let diff = build_gateway_replay_diff(&source, &gateway_execution_preview(), &outcome);

        assert_eq!(diff.status_changed, Some(true));
        assert_eq!(diff.headers_changed, Some(true));
        assert!(diff.summary_lines.iter().any(|line| {
            line.contains("gateway result changed")
                && line.contains("http=429")
                && line.contains("http=503")
                && line.contains("upstream_rate_limit_error")
                && line.contains("upstream_service_unavailable")
        }));
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line == "gateway response headers changed")
        );
    }

    #[test]
    fn gateway_replay_diff_keeps_status_unchanged_when_rich_baseline_matches() {
        let source = gateway_source_for_diff(
            gateway_request_log(
                RequestStatus::Error,
                Some("upstream_rate_limit_error"),
                Some(101),
            ),
            Some(gateway_final_attempt(
                Some(429),
                Some("upstream_rate_limit_error"),
                Some(r#"{"content-type":"application/json"}"#),
            )),
        );
        let outcome = AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Error,
            http_status: Some(429),
            first_byte_at: None,
            error_code: Some("upstream_rate_limit_error".to_string()),
            error_message: Some("slow down".to_string()),
            response_headers: vec![RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            }],
            response_body: None,
            response_body_bytes: None,
            response_body_capture_state: Some("not_captured".to_string()),
            response_body_capture: None,
            usage_normalization: None,
            transform_diagnostics: Vec::new(),
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
        };

        let diff = build_gateway_replay_diff(&source, &gateway_execution_preview(), &outcome);

        assert_eq!(diff.status_changed, Some(false));
        assert_eq!(diff.headers_changed, Some(false));
        assert!(diff.summary_lines.iter().any(|line| {
            line == "gateway result unchanged: error / http=429 / error_code=upstream_rate_limit_error"
        }));
        assert!(
            diff.summary_lines
                .iter()
                .any(|line| line == "gateway response headers unchanged")
        );
    }
}
