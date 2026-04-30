use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    http::HeaderMap,
    response::Response,
};
use chrono::Utc;
use serde_json::Value;

use crate::{
    cost::UsageNormalization,
    proxy::{
        self, ProxyError,
        runtime::replay_adapter::{
            GatewayReplayAttemptKind as RuntimeGatewayReplayAttemptKind,
            GatewayReplayCandidateDecision as RuntimeGatewayReplayCandidateDecision,
            GatewayReplayExecutionFailure as RuntimeGatewayReplayExecutionFailure,
            GatewayReplayExecutionMetadata as RuntimeGatewayReplayExecutionMetadata,
            GatewayReplayExecutionSuccess as RuntimeGatewayReplayExecutionSuccess,
            GatewayReplayFinalAttempt as RuntimeGatewayReplayFinalAttempt,
            GatewayReplayInput as RuntimeGatewayReplayInput,
            GatewayReplayPreparedRequest as RuntimeGatewayReplayPreparedRequest,
        },
    },
    schema::enum_def::{LlmApiType, RequestAttemptStatus},
    service::{
        app_state::AppState,
        diagnostics::replay::{
            source::{GatewayReplaySource, GatewayReplaySourceKind},
            types::RequestReplayCandidateDecision,
        },
        transform::unified::UnifiedTransformDiagnostic,
    },
    utils::storage::RequestLogBundleCandidateManifest,
};

#[derive(Debug, Clone)]
pub(crate) struct GatewayReplayPreparedRequest {
    pub(crate) requested_model_name: String,
    pub(crate) base_requested_model_name: String,
    pub(crate) resolved_reasoning_suffix: Option<String>,
    pub(crate) resolved_reasoning_preset: Option<String>,
    pub(crate) resolved_name_scope: String,
    pub(crate) resolved_route_id: Option<i64>,
    pub(crate) resolved_route_name: Option<String>,
    pub(crate) candidate_position: i32,
    pub(crate) provider_id: i64,
    pub(crate) provider_api_key_id: i64,
    pub(crate) model_id: i64,
    pub(crate) llm_api_type: LlmApiType,
    pub(crate) applied_request_patch_summary: Option<Value>,
    pub(crate) final_request_uri: String,
    pub(crate) final_request_headers: HeaderMap,
    pub(crate) final_request_body: Bytes,
    pub(crate) transform_diagnostics: Vec<UnifiedTransformDiagnostic>,
    pub(crate) candidate_manifest: RequestLogBundleCandidateManifest,
    pub(crate) candidate_decisions: Vec<RequestReplayCandidateDecision>,
}

#[derive(Debug)]
pub(crate) struct GatewayReplayExecutionSuccess {
    pub(crate) response: Response<Body>,
    pub(crate) metadata: GatewayReplayExecutionMetadata,
}

#[derive(Debug)]
pub(crate) struct GatewayReplayExecutionFailure {
    pub(crate) error: ProxyError,
    pub(crate) metadata: Option<GatewayReplayExecutionMetadata>,
    pub(crate) candidate_decisions: Vec<RequestReplayCandidateDecision>,
}

#[derive(Debug, Clone)]
pub(crate) struct GatewayReplayExecutionMetadata {
    pub(crate) requested_model_name: String,
    pub(crate) base_requested_model_name: String,
    pub(crate) resolved_reasoning_suffix: Option<String>,
    pub(crate) resolved_reasoning_preset: Option<String>,
    pub(crate) resolved_route_id: Option<i64>,
    pub(crate) resolved_route_name: Option<String>,
    pub(crate) final_attempt: GatewayReplayFinalAttempt,
    pub(crate) candidate_decisions: Vec<RequestReplayCandidateDecision>,
    pub(crate) transform_diagnostics: Vec<UnifiedTransformDiagnostic>,
    pub(crate) usage_normalization: Option<UsageNormalization>,
}

#[derive(Debug, Clone)]
pub(crate) struct GatewayReplayFinalAttempt {
    pub(crate) candidate_position: i32,
    pub(crate) provider_id: Option<i64>,
    pub(crate) provider_api_key_id: Option<i64>,
    pub(crate) model_id: Option<i64>,
    pub(crate) llm_api_type: Option<LlmApiType>,
    pub(crate) attempt_status: RequestAttemptStatus,
    pub(crate) error_code: Option<String>,
    pub(crate) error_message: Option<String>,
    pub(crate) request_uri: Option<String>,
    pub(crate) request_headers_json: Option<String>,
    pub(crate) request_body: Option<Bytes>,
    pub(crate) request_body_capture_state: Option<String>,
    pub(crate) response_headers_json: Option<String>,
    pub(crate) response_body: Option<Bytes>,
    pub(crate) response_body_capture_state: Option<String>,
    pub(crate) http_status: Option<i32>,
    pub(crate) first_byte_at: Option<i64>,
    pub(crate) applied_request_patch_summary: Option<Value>,
    pub(crate) total_input_tokens: Option<i32>,
    pub(crate) total_output_tokens: Option<i32>,
    pub(crate) reasoning_tokens: Option<i32>,
    pub(crate) total_tokens: Option<i32>,
}

pub(crate) async fn preview_gateway_replay_request(
    app_state: Arc<AppState>,
    source: &GatewayReplaySource,
) -> Result<GatewayReplayPreparedRequest, ProxyError> {
    let input = gateway_replay_input_from_source(source);
    proxy::preview_gateway_replay_request(app_state, input)
        .await
        .map(prepared_request_from_runtime)
}

pub(crate) async fn execute_gateway_replay_request(
    app_state: Arc<AppState>,
    source: &GatewayReplaySource,
) -> Result<GatewayReplayExecutionSuccess, GatewayReplayExecutionFailure> {
    let input = gateway_replay_input_from_source(source);
    proxy::execute_gateway_replay_request(app_state, input)
        .await
        .map(execution_success_from_runtime)
        .map_err(execution_failure_from_runtime)
}

fn gateway_replay_input_from_source(source: &GatewayReplaySource) -> RuntimeGatewayReplayInput {
    RuntimeGatewayReplayInput {
        api_key: Arc::clone(&source.api_key),
        requested_model_name: source.requested_model_name.clone(),
        query_params: source.request_snapshot.query_params.clone(),
        original_headers: source.original_headers.clone(),
        request_snapshot: source.request_snapshot.clone(),
        client_ip_addr: source.request_log.client_ip.clone(),
        start_time: Utc::now().timestamp_millis(),
        original_request_body: source.user_request_body.bytes.clone(),
        kind: runtime_kind_from_source(&source.kind),
    }
}

fn runtime_kind_from_source(kind: &GatewayReplaySourceKind) -> RuntimeGatewayReplayAttemptKind {
    match kind {
        GatewayReplaySourceKind::Generation {
            api_type,
            is_stream,
            data,
            original_request_value,
        } => RuntimeGatewayReplayAttemptKind::Generation {
            api_type: *api_type,
            is_stream: *is_stream,
            data: data.clone(),
            original_request_value: original_request_value.clone(),
        },
        GatewayReplaySourceKind::Utility { operation, data } => {
            RuntimeGatewayReplayAttemptKind::Utility {
                operation: operation.clone(),
                data: data.clone(),
            }
        }
    }
}

fn prepared_request_from_runtime(
    prepared: RuntimeGatewayReplayPreparedRequest,
) -> GatewayReplayPreparedRequest {
    GatewayReplayPreparedRequest {
        requested_model_name: prepared.requested_model_name,
        base_requested_model_name: prepared.base_requested_model_name,
        resolved_reasoning_suffix: prepared.resolved_reasoning_suffix,
        resolved_reasoning_preset: prepared.resolved_reasoning_preset,
        resolved_name_scope: prepared.resolved_name_scope,
        resolved_route_id: prepared.resolved_route_id,
        resolved_route_name: prepared.resolved_route_name,
        candidate_position: prepared.candidate_position,
        provider_id: prepared.provider_id,
        provider_api_key_id: prepared.provider_api_key_id,
        model_id: prepared.model_id,
        llm_api_type: prepared.llm_api_type,
        applied_request_patch_summary: prepared.applied_request_patch_summary,
        final_request_uri: prepared.final_request_uri,
        final_request_headers: prepared.final_request_headers,
        final_request_body: prepared.final_request_body,
        transform_diagnostics: prepared.transform_diagnostics,
        candidate_manifest: prepared.candidate_manifest,
        candidate_decisions: candidate_decisions_from_runtime(&prepared.candidate_decisions),
    }
}

fn execution_success_from_runtime(
    success: RuntimeGatewayReplayExecutionSuccess,
) -> GatewayReplayExecutionSuccess {
    GatewayReplayExecutionSuccess {
        response: success.response,
        metadata: execution_metadata_from_runtime(success.metadata),
    }
}

fn execution_failure_from_runtime(
    failure: RuntimeGatewayReplayExecutionFailure,
) -> GatewayReplayExecutionFailure {
    GatewayReplayExecutionFailure {
        error: failure.error,
        metadata: failure.metadata.map(execution_metadata_from_runtime),
        candidate_decisions: candidate_decisions_from_runtime(&failure.candidate_decisions),
    }
}

fn execution_metadata_from_runtime(
    metadata: RuntimeGatewayReplayExecutionMetadata,
) -> GatewayReplayExecutionMetadata {
    GatewayReplayExecutionMetadata {
        requested_model_name: metadata.requested_model_name,
        base_requested_model_name: metadata.base_requested_model_name,
        resolved_reasoning_suffix: metadata.resolved_reasoning_suffix,
        resolved_reasoning_preset: metadata.resolved_reasoning_preset,
        resolved_route_id: metadata.resolved_route_id,
        resolved_route_name: metadata.resolved_route_name,
        final_attempt: final_attempt_from_runtime(metadata.final_attempt),
        candidate_decisions: candidate_decisions_from_runtime(&metadata.candidate_decisions),
        transform_diagnostics: metadata.transform_diagnostics,
        usage_normalization: metadata.usage_normalization,
    }
}

fn final_attempt_from_runtime(
    attempt: RuntimeGatewayReplayFinalAttempt,
) -> GatewayReplayFinalAttempt {
    GatewayReplayFinalAttempt {
        candidate_position: attempt.candidate_position,
        provider_id: attempt.provider_id,
        provider_api_key_id: attempt.provider_api_key_id,
        model_id: attempt.model_id,
        llm_api_type: attempt.llm_api_type,
        attempt_status: attempt.attempt_status,
        error_code: attempt.error_code,
        error_message: attempt.error_message,
        request_uri: attempt.request_uri,
        request_headers_json: attempt.request_headers_json,
        request_body: attempt.request_body,
        request_body_capture_state: attempt.request_body_capture_state,
        response_headers_json: attempt.response_headers_json,
        response_body: attempt.response_body,
        response_body_capture_state: attempt.response_body_capture_state,
        http_status: attempt.http_status,
        first_byte_at: attempt.first_byte_at,
        applied_request_patch_summary: attempt.applied_request_patch_summary,
        total_input_tokens: attempt.total_input_tokens,
        total_output_tokens: attempt.total_output_tokens,
        reasoning_tokens: attempt.reasoning_tokens,
        total_tokens: attempt.total_tokens,
    }
}

fn candidate_decision_from_runtime(
    decision: &RuntimeGatewayReplayCandidateDecision,
) -> RequestReplayCandidateDecision {
    RequestReplayCandidateDecision {
        candidate_position: decision.candidate_position,
        provider_id: decision.provider_id,
        provider_api_key_id: decision.provider_api_key_id,
        model_id: decision.model_id,
        llm_api_type: decision.llm_api_type,
        attempt_status: decision.attempt_status,
        scheduler_action: decision.scheduler_action,
        error_code: decision.error_code.clone(),
        error_message: decision.error_message.clone(),
        request_uri: decision.request_uri.clone(),
    }
}

fn candidate_decisions_from_runtime(
    decisions: &[RuntimeGatewayReplayCandidateDecision],
) -> Vec<RequestReplayCandidateDecision> {
    decisions
        .iter()
        .map(candidate_decision_from_runtime)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Bytes;
    use serde_json::json;

    use super::*;
    use crate::{
        database::request_log::RequestLogRecord,
        proxy::UtilityOperation,
        schema::enum_def::{
            Action, LlmApiType, RequestAttemptStatus, RequestStatus, SchedulerAction,
        },
        service::{
            cache::types::CacheApiKey,
            diagnostics::replay::source::{DecodedBundleBody, GatewayReplaySource},
        },
        utils::storage::{RequestLogBundleQueryParam, RequestLogBundleRequestSnapshot},
    };

    fn source_with_query_params(
        query_params: Vec<RequestLogBundleQueryParam>,
    ) -> GatewayReplaySource {
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
                estimated_cost_nanos: None,
                estimated_cost_currency: None,
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
                total_tokens: None,
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
                query_params,
                ..Default::default()
            },
            original_headers: HeaderMap::new(),
            user_request_body: DecodedBundleBody {
                bytes: Bytes::from_static(br#"{"model":"gpt-test"}"#),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            },
            baseline_user_response_body: None,
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

    #[test]
    fn gateway_input_preserves_ordered_query_snapshot_and_flags() {
        let query_params = vec![
            RequestLogBundleQueryParam {
                name: "flag".to_string(),
                value: None,
                value_present: false,
                encoded_name: Some("flag".to_string()),
                encoded_value: None,
            },
            RequestLogBundleQueryParam {
                name: "blank".to_string(),
                value: None,
                value_present: true,
                encoded_name: Some("blank".to_string()),
                encoded_value: Some(String::new()),
            },
            RequestLogBundleQueryParam {
                name: "q".to_string(),
                value: Some("one two".to_string()),
                value_present: true,
                encoded_name: Some("q".to_string()),
                encoded_value: Some("one%20two".to_string()),
            },
        ];
        let source = source_with_query_params(query_params.clone());

        let input = gateway_replay_input_from_source(&source);

        assert_eq!(input.query_params, query_params);
        assert_eq!(input.request_snapshot.query_params, query_params);
        assert_eq!(input.original_request_body, source.user_request_body.bytes);
        match input.kind {
            RuntimeGatewayReplayAttemptKind::Generation {
                api_type,
                is_stream,
                data,
                ..
            } => {
                assert_eq!(api_type, LlmApiType::Openai);
                assert!(!is_stream);
                assert_eq!(data["model"], "gpt-test");
            }
            other => panic!("expected generation kind, got {other:?}"),
        }
    }

    #[test]
    fn source_utility_kind_converts_to_runtime_gateway_kind() {
        let mut source = source_with_query_params(Vec::new());
        source.kind = GatewayReplaySourceKind::Utility {
            operation: UtilityOperation {
                name: "embeddings".to_string(),
                api_type: LlmApiType::Openai,
                protocol: crate::proxy::UtilityProtocol::OpenaiCompatible,
                downstream_path: "embeddings".to_string(),
            },
            data: json!({"model": "text-embedding-3-small", "input": "hello"}),
        };

        let input = gateway_replay_input_from_source(&source);

        match input.kind {
            RuntimeGatewayReplayAttemptKind::Utility { operation, data } => {
                assert_eq!(operation.name, "embeddings");
                assert_eq!(data["model"], "text-embedding-3-small");
            }
            other => panic!("expected utility kind, got {other:?}"),
        }
    }

    #[test]
    fn candidate_decision_conversion_preserves_skip_fallback_order() {
        let decisions = vec![
            RuntimeGatewayReplayCandidateDecision {
                candidate_position: 1,
                provider_id: Some(10),
                provider_api_key_id: None,
                model_id: Some(20),
                llm_api_type: Some(LlmApiType::Openai),
                attempt_status: RequestAttemptStatus::Skipped,
                scheduler_action: SchedulerAction::FallbackNextCandidate,
                error_code: Some("provider_not_open".to_string()),
                error_message: Some("provider is outside open window".to_string()),
                request_uri: None,
            },
            RuntimeGatewayReplayCandidateDecision {
                candidate_position: 2,
                provider_id: Some(11),
                provider_api_key_id: Some(30),
                model_id: Some(21),
                llm_api_type: Some(LlmApiType::Openai),
                attempt_status: RequestAttemptStatus::Success,
                scheduler_action: SchedulerAction::ReturnSuccess,
                error_code: None,
                error_message: None,
                request_uri: Some("https://fallback.example/v1/chat/completions".to_string()),
            },
        ];

        let converted = candidate_decisions_from_runtime(&decisions);

        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].candidate_position, 1);
        assert_eq!(converted[0].attempt_status, RequestAttemptStatus::Skipped);
        assert_eq!(
            converted[0].scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
        assert_eq!(
            converted[0].error_code.as_deref(),
            Some("provider_not_open")
        );
        assert_eq!(converted[1].candidate_position, 2);
        assert_eq!(converted[1].provider_id, Some(11));
        assert_eq!(converted[1].attempt_status, RequestAttemptStatus::Success);
    }
}
