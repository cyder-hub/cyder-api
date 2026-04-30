use crate::{
    controller::BaseError,
    database::request_attempt::RequestAttemptDetail,
    schema::enum_def::{LlmApiType, RequestReplayKind, RequestReplaySemanticBasis},
    service::diagnostics::{
        body::{body_from_bytes, canonical_name_values, serialize_headers_for_output},
        policy::stripped_preview_request_header_names,
        replay::{
            fingerprint::{
                RequestReplayFingerprintInputSnapshot, RequestReplayPreviewFingerprintInput,
                body_digest_from_decoded_body, build_replay_preview_fingerprint,
                canonical_uri_for_fingerprint, final_body_digest_from_bytes,
            },
            gateway_adapter::GatewayReplayPreparedRequest,
            source::{AttemptReplaySource, DecodedBundleBody, GatewayReplaySource},
            transport::build_attempt_upstream_headers,
            types::{
                AttemptReplayBaselineSummary, AttemptReplayPreviewResponse,
                GatewayReplayBaselineSummary, GatewayReplayPreviewResponse, RequestReplayBody,
                RequestReplayExecutionPreview, RequestReplayInputSnapshot,
                RequestReplayModelSnapshot, RequestReplayNameValue, RequestReplayProviderSnapshot,
                RequestReplayQueryParam, RequestReplayResolvedCandidate,
                RequestReplayResolvedRoute,
            },
        },
    },
};

#[derive(Debug, Clone)]
pub(crate) struct ReplayResolvedCredential {
    pub(crate) provider_api_key_id: i64,
    pub(crate) request_key: String,
    pub(crate) used_override: bool,
}

pub(crate) fn build_gateway_replay_preview(
    source: &GatewayReplaySource,
    prepared: &GatewayReplayPreparedRequest,
    preview_created_at: i64,
) -> Result<GatewayReplayPreviewResponse, BaseError> {
    let mut response = GatewayReplayPreviewResponse {
        source_request_log_id: source.request_log.id,
        replay_kind: RequestReplayKind::GatewayRequest,
        semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
        preview_fingerprint: String::new(),
        preview_created_at,
        input_snapshot: gateway_input_snapshot(source),
        execution_preview: execution_preview_from_gateway_prepared(prepared),
        baseline: GatewayReplayBaselineSummary {
            overall_status: source.request_log.overall_status.clone(),
            final_error_code: source.request_log.final_error_code.clone(),
            final_error_message: source.request_log.final_error_message.clone(),
            total_tokens: source.request_log.total_tokens,
            estimated_cost_nanos: source.request_log.estimated_cost_nanos,
            estimated_cost_currency: source.request_log.estimated_cost_currency.clone(),
            user_response_body_capture_state: source
                .baseline_user_response_body
                .as_ref()
                .and_then(|body| body.capture_state.clone()),
        },
    };
    response.preview_fingerprint = gateway_replay_preview_fingerprint(&response, source, prepared)?;
    Ok(response)
}

fn gateway_input_snapshot(source: &GatewayReplaySource) -> RequestReplayInputSnapshot {
    RequestReplayInputSnapshot::GatewayRequest {
        request_path: source.request_snapshot.request_path.clone(),
        query_params: replay_query_params_from_snapshot(&source.request_snapshot.query_params),
        sanitized_original_headers: source
            .request_snapshot
            .sanitized_original_headers
            .iter()
            .map(|item| RequestReplayNameValue {
                name: item.name.clone(),
                value: Some(item.value.clone()),
            })
            .collect(),
        user_request_body: Some(body_to_replay_body(&source.user_request_body)),
    }
}

pub(crate) fn execution_preview_from_gateway_prepared(
    prepared: &GatewayReplayPreparedRequest,
) -> RequestReplayExecutionPreview {
    RequestReplayExecutionPreview {
        semantic_basis: RequestReplaySemanticBasis::HistoricalRequestSnapshotWithCurrentConfig,
        requested_model_name: Some(prepared.requested_model_name.clone()),
        base_requested_model_name: Some(prepared.base_requested_model_name.clone()),
        resolved_reasoning_suffix: prepared.resolved_reasoning_suffix.clone(),
        resolved_reasoning_preset: prepared.resolved_reasoning_preset.clone(),
        resolved_route: Some(RequestReplayResolvedRoute {
            route_id: prepared.resolved_route_id,
            route_name: prepared.resolved_route_name.clone(),
        }),
        resolved_candidate: Some(RequestReplayResolvedCandidate {
            candidate_position: Some(prepared.candidate_position),
            provider_id: Some(prepared.provider_id),
            provider_api_key_id: Some(prepared.provider_api_key_id),
            model_id: Some(prepared.model_id),
            llm_api_type: Some(prepared.llm_api_type),
        }),
        candidate_decisions: prepared.candidate_decisions.clone(),
        applied_request_patch_summary: prepared.applied_request_patch_summary.clone(),
        final_request_uri: Some(prepared.final_request_uri.clone()),
        final_request_headers: serialize_headers_for_output(
            &prepared.final_request_headers,
            stripped_preview_request_header_names(),
        ),
        final_request_body: Some(body_from_bytes(
            &prepared.final_request_body,
            Some("application/json".to_string()),
            Some("complete".to_string()),
        )),
    }
}

pub(crate) fn build_attempt_replay_preview(
    source: &AttemptReplaySource,
    credential: &ReplayResolvedCredential,
    preview_created_at: i64,
) -> Result<AttemptReplayPreviewResponse, BaseError> {
    let final_request_headers = build_attempt_upstream_headers(source, credential)?;

    let mut response = AttemptReplayPreviewResponse {
        source_request_log_id: source.request_log_id,
        source_attempt_id: source.attempt.id,
        replay_kind: RequestReplayKind::AttemptUpstream,
        semantic_basis: RequestReplaySemanticBasis::HistoricalAttemptSnapshot,
        preview_fingerprint: String::new(),
        preview_created_at,
        selected_provider_api_key_id: credential.provider_api_key_id,
        used_provider_api_key_override: credential.used_override,
        input_snapshot: RequestReplayInputSnapshot::AttemptUpstream {
            request_uri: source.request_uri.clone(),
            sanitized_request_headers: source.sanitized_request_headers.clone(),
            llm_request_body: Some(body_to_replay_body(&source.llm_request_body)),
            provider: Some(provider_snapshot_from_attempt(&source.attempt)),
            model: Some(model_snapshot_from_attempt(
                &source.attempt,
                source.llm_api_type,
            )),
        },
        execution_preview: RequestReplayExecutionPreview {
            semantic_basis: RequestReplaySemanticBasis::HistoricalAttemptSnapshot,
            requested_model_name: source.requested_model_name.clone(),
            base_requested_model_name: source.base_requested_model_name.clone(),
            resolved_reasoning_suffix: source.resolved_reasoning_suffix.clone(),
            resolved_reasoning_preset: source.resolved_reasoning_preset.clone(),
            resolved_route: Some(RequestReplayResolvedRoute {
                route_id: source.resolved_route_id,
                route_name: source.resolved_route_name.clone(),
            }),
            resolved_candidate: Some(RequestReplayResolvedCandidate {
                candidate_position: Some(source.attempt.candidate_position),
                provider_id: source.attempt.provider_id,
                provider_api_key_id: Some(credential.provider_api_key_id),
                model_id: source.attempt.model_id,
                llm_api_type: Some(source.llm_api_type),
            }),
            candidate_decisions: Vec::new(),
            applied_request_patch_summary: None,
            final_request_uri: Some(source.request_uri.clone()),
            final_request_headers: serialize_headers_for_output(
                &final_request_headers,
                stripped_preview_request_header_names(),
            ),
            final_request_body: Some(body_to_replay_body(&source.llm_request_body)),
        },
        baseline: AttemptReplayBaselineSummary {
            attempt_status: source.attempt.attempt_status,
            http_status: source.attempt.http_status,
            response_headers: source.baseline_response_headers.clone(),
            response_body_capture_state: source
                .baseline_response_body
                .as_ref()
                .and_then(|body| body.capture_state.clone())
                .or_else(|| source.attempt.llm_response_capture_state.clone()),
            total_tokens: source.attempt.total_tokens,
            estimated_cost_nanos: source.attempt.estimated_cost_nanos,
            estimated_cost_currency: source.attempt.estimated_cost_currency.clone(),
        },
    };
    response.preview_fingerprint =
        attempt_replay_preview_fingerprint(&response, source, credential)?;
    Ok(response)
}

fn attempt_replay_preview_fingerprint(
    response: &AttemptReplayPreviewResponse,
    source: &AttemptReplaySource,
    credential: &ReplayResolvedCredential,
) -> Result<String, BaseError> {
    let input = RequestReplayPreviewFingerprintInput {
        replay_kind: response.replay_kind,
        source_request_log_id: response.source_request_log_id,
        source_attempt_id: Some(response.source_attempt_id),
        provider_api_key_id_override: credential
            .used_override
            .then_some(credential.provider_api_key_id),
        selected_provider_api_key_id: Some(credential.provider_api_key_id),
        used_provider_api_key_override: credential.used_override,
        semantic_basis: response.semantic_basis,
        requested_model_name: response.execution_preview.requested_model_name.clone(),
        base_requested_model_name: response.execution_preview.base_requested_model_name.clone(),
        resolved_reasoning_suffix: response.execution_preview.resolved_reasoning_suffix.clone(),
        resolved_reasoning_preset: response.execution_preview.resolved_reasoning_preset.clone(),
        input_snapshot: RequestReplayFingerprintInputSnapshot::AttemptUpstream {
            request_uri: canonical_uri_for_fingerprint(&source.request_uri),
            sanitized_request_headers: canonical_name_values(
                &source.sanitized_request_headers,
                true,
            ),
            llm_request_body: Some(body_digest_from_decoded_body(&source.llm_request_body)),
            provider: Some(provider_snapshot_from_attempt(&source.attempt)),
            model: Some(model_snapshot_from_attempt(
                &source.attempt,
                source.llm_api_type,
            )),
        },
        resolved_route: response.execution_preview.resolved_route.clone(),
        resolved_name_scope: None,
        resolved_candidate: response.execution_preview.resolved_candidate.clone(),
        candidate_manifest: None,
        candidate_decisions: response.execution_preview.candidate_decisions.clone(),
        applied_request_patch_summary: response
            .execution_preview
            .applied_request_patch_summary
            .clone(),
        final_request_uri: response
            .execution_preview
            .final_request_uri
            .as_deref()
            .map(canonical_uri_for_fingerprint),
        final_request_headers: canonical_name_values(
            &response.execution_preview.final_request_headers,
            true,
        ),
        final_request_body: Some(body_digest_from_decoded_body(&source.llm_request_body)),
    };

    build_replay_preview_fingerprint(response.preview_created_at, &input)
}

fn gateway_replay_preview_fingerprint(
    response: &GatewayReplayPreviewResponse,
    source: &GatewayReplaySource,
    prepared: &GatewayReplayPreparedRequest,
) -> Result<String, BaseError> {
    let input = RequestReplayPreviewFingerprintInput {
        replay_kind: response.replay_kind,
        source_request_log_id: response.source_request_log_id,
        source_attempt_id: None,
        provider_api_key_id_override: None,
        selected_provider_api_key_id: Some(prepared.provider_api_key_id),
        used_provider_api_key_override: false,
        semantic_basis: response.semantic_basis,
        requested_model_name: response.execution_preview.requested_model_name.clone(),
        base_requested_model_name: response.execution_preview.base_requested_model_name.clone(),
        resolved_reasoning_suffix: response.execution_preview.resolved_reasoning_suffix.clone(),
        resolved_reasoning_preset: response.execution_preview.resolved_reasoning_preset.clone(),
        input_snapshot: RequestReplayFingerprintInputSnapshot::GatewayRequest {
            request_path: source.request_snapshot.request_path.clone(),
            query_params: replay_query_params_from_snapshot(&source.request_snapshot.query_params),
            sanitized_original_headers: canonical_name_values(
                &source
                    .request_snapshot
                    .sanitized_original_headers
                    .iter()
                    .map(|item| RequestReplayNameValue {
                        name: item.name.clone(),
                        value: Some(item.value.clone()),
                    })
                    .collect::<Vec<_>>(),
                true,
            ),
            user_request_body: Some(body_digest_from_decoded_body(&source.user_request_body)),
        },
        resolved_route: response.execution_preview.resolved_route.clone(),
        resolved_name_scope: Some(prepared.resolved_name_scope.clone()),
        resolved_candidate: response.execution_preview.resolved_candidate.clone(),
        candidate_manifest: (!prepared.candidate_manifest.items.is_empty())
            .then_some(prepared.candidate_manifest.clone()),
        candidate_decisions: response.execution_preview.candidate_decisions.clone(),
        applied_request_patch_summary: response
            .execution_preview
            .applied_request_patch_summary
            .clone(),
        final_request_uri: response
            .execution_preview
            .final_request_uri
            .as_deref()
            .map(canonical_uri_for_fingerprint),
        final_request_headers: canonical_name_values(
            &response.execution_preview.final_request_headers,
            true,
        ),
        final_request_body: Some(final_body_digest_from_bytes(
            &prepared.final_request_body,
            Some("application/json".to_string()),
            Some("complete".to_string()),
        )),
    };

    build_replay_preview_fingerprint(response.preview_created_at, &input)
}

fn replay_query_params_from_snapshot(
    params: &[crate::utils::storage::RequestLogBundleQueryParam],
) -> Vec<RequestReplayQueryParam> {
    params
        .iter()
        .map(|item| RequestReplayQueryParam {
            name: item.name.clone(),
            value: item.value_for_replay(),
            value_present: item.has_value(),
        })
        .collect()
}

fn provider_snapshot_from_attempt(attempt: &RequestAttemptDetail) -> RequestReplayProviderSnapshot {
    RequestReplayProviderSnapshot {
        provider_id: attempt.provider_id,
        provider_api_key_id: attempt.provider_api_key_id,
        provider_key: attempt.provider_key_snapshot.clone(),
        provider_name: attempt.provider_name_snapshot.clone(),
    }
}

fn model_snapshot_from_attempt(
    attempt: &RequestAttemptDetail,
    llm_api_type: LlmApiType,
) -> RequestReplayModelSnapshot {
    RequestReplayModelSnapshot {
        model_id: attempt.model_id,
        model_name: attempt.model_name_snapshot.clone(),
        real_model_name: attempt.real_model_name_snapshot.clone(),
        llm_api_type: Some(attempt.llm_api_type.unwrap_or(llm_api_type)),
    }
}

fn body_to_replay_body(body: &DecodedBundleBody) -> RequestReplayBody {
    body_from_bytes(
        &body.bytes,
        body.media_type.clone(),
        body.capture_state.clone(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Bytes;
    use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
    use serde_json::json;

    use super::*;
    use crate::{
        database::request_log::RequestLogRecord,
        schema::enum_def::{
            Action, ProviderApiKeyMode, ProviderType, RequestAttemptStatus, RequestStatus,
        },
        service::{
            cache::types::{CacheApiKey, CacheProvider},
            diagnostics::{
                body::build_header_map_from_name_values,
                replay::source::{GatewayReplaySource, GatewayReplaySourceKind},
            },
        },
        utils::storage::{RequestLogBundleQueryParam, RequestLogBundleRequestSnapshot},
    };

    fn credential(request_key: &str) -> ReplayResolvedCredential {
        ReplayResolvedCredential {
            provider_api_key_id: 3,
            request_key: request_key.to_string(),
            used_override: false,
        }
    }

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

    fn attempt_source(request_uri: String, provider_type: ProviderType) -> AttemptReplaySource {
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

    fn replay_path(provider_type: &ProviderType) -> &'static str {
        match provider_type {
            ProviderType::Gemini | ProviderType::Vertex => {
                "/v1beta/models/gemini-2.5-pro:generateContent"
            }
            ProviderType::Anthropic => "/v1/messages",
            ProviderType::Ollama => "/api/chat",
            ProviderType::Openai
            | ProviderType::Responses
            | ProviderType::GeminiOpenai
            | ProviderType::VertexOpenai => "/v1/chat/completions",
        }
    }

    fn replay_auth_header_name(provider_type: &ProviderType) -> &'static str {
        match provider_type {
            ProviderType::Gemini => "x-goog-api-key",
            ProviderType::Anthropic => "x-api-key",
            ProviderType::Openai
            | ProviderType::Responses
            | ProviderType::Vertex
            | ProviderType::VertexOpenai
            | ProviderType::Ollama
            | ProviderType::GeminiOpenai => "authorization",
        }
    }

    fn gateway_source() -> GatewayReplaySource {
        GatewayReplaySource {
            request_log: RequestLogRecord {
                id: 42,
                api_key_id: 7,
                requested_model_name: Some("gpt-test".to_string()),
                base_requested_model_name: Some("gpt-test".to_string()),
                resolved_reasoning_suffix: None,
                resolved_reasoning_preset: None,
                resolved_name_scope: None,
                resolved_route_id: None,
                resolved_route_name: None,
                user_api_type: LlmApiType::Openai,
                overall_status: RequestStatus::Success,
                final_error_code: None,
                final_error_message: None,
                attempt_count: 1,
                retry_count: 0,
                fallback_count: 0,
                request_received_at: 100,
                first_attempt_started_at: None,
                response_started_to_client_at: None,
                completed_at: None,
                is_stream: false,
                client_ip: Some("127.0.0.1".to_string()),
                final_attempt_id: None,
                final_provider_id: None,
                final_provider_api_key_id: None,
                final_model_id: None,
                final_provider_key_snapshot: None,
                final_provider_name_snapshot: None,
                final_model_name_snapshot: None,
                final_real_model_name_snapshot: None,
                final_llm_api_type: None,
                estimated_cost_nanos: Some(100),
                estimated_cost_currency: Some("USD".to_string()),
                cost_catalog_id: None,
                cost_catalog_version_id: None,
                cost_snapshot_json: None,
                total_input_tokens: None,
                total_output_tokens: None,
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
                bundle_version: None,
                bundle_storage_type: None,
                bundle_storage_key: None,
                created_at: 100,
                updated_at: 100,
            },
            request_snapshot: RequestLogBundleRequestSnapshot {
                request_path: "/ai/openai/v1/chat/completions".to_string(),
                operation_kind: "chat_completions_create".to_string(),
                query_params: vec![
                    RequestLogBundleQueryParam {
                        name: "flag".to_string(),
                        value: None,
                        value_present: false,
                        encoded_name: Some("flag".to_string()),
                        encoded_value: None,
                    },
                    RequestLogBundleQueryParam {
                        name: "q".to_string(),
                        value: Some("one two".to_string()),
                        value_present: true,
                        encoded_name: Some("q".to_string()),
                        encoded_value: Some("one%20two".to_string()),
                    },
                ],
                sanitized_original_headers: vec![
                    crate::utils::storage::RequestLogBundleHttpHeader {
                        name: "content-type".to_string(),
                        value: "application/json".to_string(),
                    },
                ],
                ..Default::default()
            },
            original_headers: HeaderMap::new(),
            user_request_body: DecodedBundleBody {
                bytes: Bytes::from_static(br#"{"model":"gpt-test"}"#),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            },
            baseline_user_response_body: Some(DecodedBundleBody {
                bytes: Bytes::from_static(br#"{"ok":true}"#),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            }),
            baseline_final_attempt: None,
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
                data: json!({"model": "gpt-test"}),
                original_request_value: json!({"model": "gpt-test"}),
            },
        }
    }

    fn prepared_gateway_request() -> GatewayReplayPreparedRequest {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer sk-live"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        GatewayReplayPreparedRequest {
            requested_model_name: "smart-chat-high".to_string(),
            base_requested_model_name: "smart-chat".to_string(),
            resolved_reasoning_suffix: Some("high".to_string()),
            resolved_reasoning_preset: Some("high".to_string()),
            resolved_name_scope: "direct".to_string(),
            resolved_route_id: Some(8),
            resolved_route_name: Some("primary".to_string()),
            candidate_position: 1,
            provider_id: 2,
            provider_api_key_id: 3,
            model_id: 4,
            llm_api_type: LlmApiType::Openai,
            applied_request_patch_summary: None,
            final_request_uri: "https://upstream.example/v1/chat/completions".to_string(),
            final_request_headers: headers,
            final_request_body: Bytes::from_static(br#"{"model":"gpt-test"}"#),
            transform_diagnostics: Vec::new(),
            candidate_manifest: Default::default(),
            candidate_decisions: Vec::new(),
        }
    }

    #[test]
    fn attempt_replay_preview_redacts_provider_specific_auth_headers() {
        let cases = [
            ProviderType::Openai,
            ProviderType::Responses,
            ProviderType::Gemini,
            ProviderType::Vertex,
            ProviderType::VertexOpenai,
            ProviderType::GeminiOpenai,
            ProviderType::Anthropic,
            ProviderType::Ollama,
        ];

        for provider_type in cases {
            let source = attempt_source(
                format!("https://upstream.example{}", replay_path(&provider_type)),
                provider_type.clone(),
            );

            let preview =
                build_attempt_replay_preview(&source, &credential("sk-live"), 1_776_840_000_000)
                    .expect("preview should build");

            let auth_header = preview
                .execution_preview
                .final_request_headers
                .iter()
                .find(|header| header.name == replay_auth_header_name(&provider_type))
                .expect("provider auth header should be present");
            assert_eq!(auth_header.value, None);
            assert!(
                preview
                    .execution_preview
                    .final_request_headers
                    .iter()
                    .any(|header| {
                        header.name == "content-type"
                            && header.value.as_deref() == Some("application/json")
                    })
            );
            assert_eq!(preview.selected_provider_api_key_id, 3);
            assert_eq!(preview.baseline.total_tokens, Some(7));
            assert_eq!(preview.preview_created_at, 1_776_840_000_000);
            assert!(
                preview
                    .preview_fingerprint
                    .starts_with("request-replay-preview-v1:1776840000000:")
            );
        }
    }

    #[test]
    fn attempt_replay_preview_rejects_provider_protocol_mismatch() {
        let mut source = attempt_source(
            "https://upstream.example/v1/messages".to_string(),
            ProviderType::Anthropic,
        );
        source.llm_api_type = LlmApiType::Openai;
        source.attempt.llm_api_type = Some(LlmApiType::Openai);

        let err = build_attempt_replay_preview(&source, &credential("sk-live"), 1_776_840_000_000)
            .expect_err("preview should reject mismatched provider protocol");

        assert!(matches!(
            err,
            BaseError::ParamInvalid(Some(message))
                if message.contains("does not support downstream protocol")
        ));
    }

    #[test]
    fn replay_preview_fingerprint_is_deterministic_for_equivalent_attempt_preview() {
        let left = attempt_source(
            "https://upstream.example/v1/chat/completions".to_string(),
            ProviderType::Openai,
        );
        let mut right = left.clone();
        right.sanitized_request_headers.reverse();
        right.request_headers =
            build_header_map_from_name_values(&right.sanitized_request_headers).expect("headers");

        let left_preview =
            build_attempt_replay_preview(&left, &credential("sk-live"), 1_776_840_000_000)
                .expect("left preview should build");
        let right_preview =
            build_attempt_replay_preview(&right, &credential("sk-live"), 1_776_840_000_000)
                .expect("right preview should build");

        assert_eq!(
            left_preview.preview_fingerprint,
            right_preview.preview_fingerprint
        );
    }

    #[test]
    fn replay_preview_fingerprint_changes_when_override_or_body_changes() {
        let source = attempt_source(
            "https://upstream.example/v1/chat/completions".to_string(),
            ProviderType::Openai,
        );
        let created_at = 1_776_840_000_000;
        let baseline = build_attempt_replay_preview(&source, &credential("sk-live"), created_at)
            .expect("baseline preview should build");

        let mut override_credential = credential("sk-other");
        override_credential.provider_api_key_id = 9;
        override_credential.used_override = true;
        let override_preview =
            build_attempt_replay_preview(&source, &override_credential, created_at)
                .expect("override preview should build");
        assert_ne!(
            baseline.preview_fingerprint,
            override_preview.preview_fingerprint
        );

        let mut changed_source = source.clone();
        changed_source.llm_request_body.bytes = Bytes::from_static(
            br#"{"model":"gpt-4o-mini","messages":[{"role":"user","content":"changed"}]}"#,
        );
        let changed_preview =
            build_attempt_replay_preview(&changed_source, &credential("sk-live"), created_at)
                .expect("changed preview should build");
        assert_ne!(
            baseline.preview_fingerprint,
            changed_preview.preview_fingerprint
        );
    }

    #[test]
    fn gateway_replay_preview_materializes_final_request_without_leaking_auth() {
        let source = gateway_source();
        let prepared = prepared_gateway_request();

        let preview = build_gateway_replay_preview(&source, &prepared, 1_776_840_000_000)
            .expect("gateway preview should build");

        assert_eq!(
            preview.execution_preview.requested_model_name.as_deref(),
            Some("smart-chat-high")
        );
        assert_eq!(
            preview
                .execution_preview
                .base_requested_model_name
                .as_deref(),
            Some("smart-chat")
        );
        assert_eq!(
            preview.execution_preview.final_request_uri.as_deref(),
            Some("https://upstream.example/v1/chat/completions")
        );
        let authorization = preview
            .execution_preview
            .final_request_headers
            .iter()
            .find(|header| header.name == "authorization")
            .expect("authorization header should be represented");
        assert_eq!(authorization.value, None);
        assert!(
            preview
                .execution_preview
                .final_request_body
                .and_then(|body| body.json)
                .is_some()
        );
        assert!(
            preview
                .preview_fingerprint
                .starts_with("request-replay-preview-v1:1776840000000:")
        );
    }
}
