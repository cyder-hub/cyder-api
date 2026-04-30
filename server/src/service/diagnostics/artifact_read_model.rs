use std::fmt::Debug;

use serde::Serialize;

use crate::{
    database::{request_attempt::RequestAttemptDetail, request_log::RequestLogRecord},
    schema::enum_def::{LlmApiType, ProviderApiKeyMode},
    service::{
        diagnostics::capability::{ReplayCapabilitySummary, build_replay_capability},
        transform::unified::UnifiedTransformDiagnostic,
    },
    utils::storage::{
        LogBodyCaptureState, RequestLogBundleCandidateManifest, RequestLogBundleRequestSnapshot,
        RequestLogBundleTransformDiagnostics, RequestLogBundleV2,
    },
};

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestLogArtifactResponse {
    pub payload_manifest: PayloadManifestResponse,
    pub request_snapshot: Option<RequestSnapshotResponse>,
    pub candidate_manifest: CandidateManifestResponse,
    pub transform_diagnostics: TransformDiagnosticsSummaryResponse,
    pub replay_capability: ReplayCapabilitySummary,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct PayloadManifestResponse {
    pub bundle_version: Option<u32>,
    pub log_id: i64,
    pub created_at: Option<i64>,
    pub request: PayloadRequestManifestResponse,
    pub attempts: Vec<PayloadAttemptManifestResponse>,
    pub blob_count: usize,
    pub patch_count: usize,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct PayloadRequestManifestResponse {
    pub has_user_request_body: bool,
    pub user_request_blob_id: Option<i32>,
    pub has_user_response_body: bool,
    pub user_response_blob_id: Option<i32>,
    pub user_response_capture_state: Option<String>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct PayloadAttemptManifestResponse {
    pub attempt_id: Option<i64>,
    pub attempt_index: i32,
    pub has_llm_request_body: bool,
    pub llm_request_blob_id: Option<i32>,
    pub llm_request_patch_id: Option<i32>,
    pub has_llm_response_body: bool,
    pub llm_response_blob_id: Option<i32>,
    pub llm_response_capture_state: Option<String>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestSnapshotResponse {
    pub request_path: String,
    pub operation_kind: String,
    pub query_params: Vec<NameValueResponse>,
    pub sanitized_original_headers: Vec<NameValueResponse>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct NameValueResponse {
    pub name: String,
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_present: Option<bool>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateManifestResponse {
    pub has_asset: bool,
    pub items: Vec<CandidateManifestItemResponse>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateManifestItemResponse {
    pub candidate_position: i32,
    pub route_id: Option<i64>,
    pub route_name: Option<String>,
    pub provider_id: i64,
    pub provider_key: String,
    pub model_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub llm_api_type: String,
    pub provider_api_key_mode: String,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct TransformDiagnosticsSummaryResponse {
    pub has_asset: bool,
    pub summary: TransformDiagnosticsSummaryBodyResponse,
    pub items: Vec<TransformDiagnosticItemResponse>,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct TransformDiagnosticsSummaryBodyResponse {
    pub count: u32,
    pub max_loss_level: Option<String>,
    pub kinds: Vec<String>,
    pub phases: Vec<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct TransformDiagnosticItemResponse {
    pub phase: String,
    pub diagnostic: UnifiedTransformDiagnostic,
}

pub(crate) fn build_request_log_artifact_response(
    record: &RequestLogRecord,
    attempts: &[RequestAttemptDetail],
    bundle: Option<&RequestLogBundleV2>,
) -> RequestLogArtifactResponse {
    match bundle {
        Some(bundle) => build_v2_artifact_response(attempts, bundle),
        None => build_empty_artifact_response(record, attempts),
    }
}

fn build_empty_artifact_response(
    record: &RequestLogRecord,
    attempts: &[RequestAttemptDetail],
) -> RequestLogArtifactResponse {
    RequestLogArtifactResponse {
        payload_manifest: PayloadManifestResponse {
            bundle_version: record.bundle_version.map(|version| version as u32),
            log_id: record.id,
            ..Default::default()
        },
        replay_capability: build_replay_capability(None, attempts),
        ..Default::default()
    }
}

fn build_v2_artifact_response(
    attempts: &[RequestAttemptDetail],
    bundle: &RequestLogBundleV2,
) -> RequestLogArtifactResponse {
    RequestLogArtifactResponse {
        payload_manifest: build_v2_payload_manifest(bundle),
        request_snapshot: bundle
            .request_snapshot
            .as_ref()
            .map(request_snapshot_response),
        candidate_manifest: candidate_manifest_response(bundle.candidate_manifest.as_ref()),
        transform_diagnostics: transform_diagnostics_response(
            bundle.transform_diagnostics.as_ref(),
        ),
        replay_capability: build_replay_capability(Some(bundle), attempts),
    }
}

fn build_v2_payload_manifest(bundle: &RequestLogBundleV2) -> PayloadManifestResponse {
    PayloadManifestResponse {
        bundle_version: Some(bundle.version),
        log_id: bundle.log_id,
        created_at: Some(bundle.created_at),
        request: PayloadRequestManifestResponse {
            has_user_request_body: bundle.request_section.user_request_blob_id.is_some(),
            user_request_blob_id: bundle.request_section.user_request_blob_id,
            has_user_response_body: bundle.request_section.user_response_blob_id.is_some(),
            user_response_blob_id: bundle.request_section.user_response_blob_id,
            user_response_capture_state: bundle
                .request_section
                .user_response_capture_state
                .as_ref()
                .map(capture_state_response),
        },
        attempts: bundle
            .attempt_sections
            .iter()
            .map(|attempt| PayloadAttemptManifestResponse {
                attempt_id: attempt.attempt_id,
                attempt_index: attempt.attempt_index,
                has_llm_request_body: attempt.llm_request_blob_id.is_some(),
                llm_request_blob_id: attempt.llm_request_blob_id,
                llm_request_patch_id: attempt.llm_request_patch_id,
                has_llm_response_body: attempt.llm_response_blob_id.is_some(),
                llm_response_blob_id: attempt.llm_response_blob_id,
                llm_response_capture_state: attempt
                    .llm_response_capture_state
                    .as_ref()
                    .map(capture_state_response),
            })
            .collect(),
        blob_count: bundle.blob_pool.len(),
        patch_count: bundle.patch_pool.len(),
    }
}

fn request_snapshot_response(
    snapshot: &RequestLogBundleRequestSnapshot,
) -> RequestSnapshotResponse {
    RequestSnapshotResponse {
        request_path: snapshot.request_path.clone(),
        operation_kind: snapshot.operation_kind.clone(),
        query_params: snapshot
            .query_params
            .iter()
            .map(|param| NameValueResponse {
                name: param.name.clone(),
                value: param.value_for_replay(),
                value_present: Some(param.has_value()),
            })
            .collect(),
        sanitized_original_headers: snapshot
            .sanitized_original_headers
            .iter()
            .map(|header| NameValueResponse {
                name: header.name.clone(),
                value: Some(header.value.clone()),
                value_present: None,
            })
            .collect(),
    }
}

fn candidate_manifest_response(
    manifest: Option<&RequestLogBundleCandidateManifest>,
) -> CandidateManifestResponse {
    let Some(manifest) = manifest else {
        return CandidateManifestResponse::default();
    };

    CandidateManifestResponse {
        has_asset: true,
        items: manifest
            .items
            .iter()
            .map(|item| CandidateManifestItemResponse {
                candidate_position: item.candidate_position,
                route_id: item.route_id,
                route_name: item.route_name.clone(),
                provider_id: item.provider_id,
                provider_key: item.provider_key.clone(),
                model_id: item.model_id,
                model_name: item.model_name.clone(),
                real_model_name: item.real_model_name.clone(),
                llm_api_type: llm_api_type_response(item.llm_api_type),
                provider_api_key_mode: provider_api_key_mode_response(&item.provider_api_key_mode),
            })
            .collect(),
    }
}

fn transform_diagnostics_response(
    diagnostics: Option<&RequestLogBundleTransformDiagnostics>,
) -> TransformDiagnosticsSummaryResponse {
    let Some(diagnostics) = diagnostics else {
        return TransformDiagnosticsSummaryResponse::default();
    };

    TransformDiagnosticsSummaryResponse {
        has_asset: true,
        summary: TransformDiagnosticsSummaryBodyResponse {
            count: diagnostics.summary.count,
            max_loss_level: diagnostics
                .summary
                .max_loss_level
                .as_ref()
                .map(serialized_enum_response),
            kinds: diagnostics
                .summary
                .kinds
                .iter()
                .map(serialized_enum_response)
                .collect(),
            phases: diagnostics
                .summary
                .phases
                .iter()
                .map(serialized_enum_response)
                .collect(),
        },
        items: diagnostics
            .items
            .iter()
            .map(|item| TransformDiagnosticItemResponse {
                phase: serialized_enum_response(&item.phase),
                diagnostic: item.diagnostic.clone(),
            })
            .collect(),
    }
}

fn capture_state_response(state: &LogBodyCaptureState) -> String {
    match state {
        LogBodyCaptureState::Complete => "complete",
        LogBodyCaptureState::Incomplete => "incomplete",
        LogBodyCaptureState::NotCaptured => "not_captured",
    }
    .to_string()
}

fn llm_api_type_response(value: LlmApiType) -> String {
    serialized_enum_response(&value)
}

fn provider_api_key_mode_response(value: &ProviderApiKeyMode) -> String {
    serialized_enum_response(value)
}

fn serialized_enum_response<T>(value: &T) -> String
where
    T: Serialize + Debug,
{
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{value:?}"))
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use crate::{
        database::{request_attempt::RequestAttemptDetail, request_log::RequestLogRecord},
        schema::enum_def::{LlmApiType, ProviderApiKeyMode},
        service::transform::unified::{
            UnifiedTransformDiagnostic, UnifiedTransformDiagnosticAction,
            UnifiedTransformDiagnosticKind, UnifiedTransformDiagnosticLossLevel,
        },
        utils::storage::{
            LogBodyCaptureState, RequestLogBundleAttemptSection, RequestLogBundleCandidateManifest,
            RequestLogBundleCandidateManifestItem, RequestLogBundleHttpHeader,
            RequestLogBundleRequestSection, RequestLogBundleRequestSnapshot,
            RequestLogBundleTransformDiagnosticItem, RequestLogBundleTransformDiagnosticPhase,
            RequestLogBundleTransformDiagnostics, RequestLogBundleTransformDiagnosticsSummary,
            RequestLogBundleV2, RequestLogBundleV2Builder, RequestLogBundleV2DiagnosticAssets,
        },
    };

    use super::build_request_log_artifact_response;

    fn record() -> RequestLogRecord {
        RequestLogRecord {
            id: 42,
            api_key_id: 7,
            bundle_version: Some(2),
            ..Default::default()
        }
    }

    fn attempt() -> RequestAttemptDetail {
        RequestAttemptDetail {
            id: 101,
            request_log_id: 42,
            attempt_index: 1,
            request_uri: Some("https://upstream.example/v1/chat/completions".to_string()),
            request_headers_json: Some("{\"content-type\":\"application/json\"}".to_string()),
            ..Default::default()
        }
    }

    fn diagnostic() -> UnifiedTransformDiagnostic {
        UnifiedTransformDiagnostic {
            type_: "transform_diagnostic".to_string(),
            diagnostic_kind: UnifiedTransformDiagnosticKind::CapabilityDowngrade,
            provider: "Responses".to_string(),
            target_provider: "OpenAI".to_string(),
            source: "responses".to_string(),
            target: "openai".to_string(),
            stream_id: None,
            stage: Some("request".to_string()),
            loss_level: UnifiedTransformDiagnosticLossLevel::LossyMajor,
            action: UnifiedTransformDiagnosticAction::Drop,
            semantic_unit: "reasoning".to_string(),
            reason: "reasoning summary was dropped".to_string(),
            context: None,
            raw_data_summary: None,
            recovery_hint: None,
        }
    }

    fn bundle() -> RequestLogBundleV2 {
        let mut builder = RequestLogBundleV2Builder::new();
        let user_request_blob_id =
            builder.add_user_request_body(Bytes::from_static(br#"{"model":"route"}"#));
        let llm_request = builder.add_llm_request_body(
            LlmApiType::Openai,
            LlmApiType::Openai,
            1,
            Bytes::from_static(br#"{"model":"upstream"}"#),
        );
        let llm_response_blob_id = builder.add_response_body(Bytes::from_static(br#"{"ok":true}"#));

        builder.finish(
            42,
            1_776_840_000_000,
            RequestLogBundleRequestSection {
                user_request_blob_id: Some(user_request_blob_id),
                user_response_blob_id: None,
                user_response_capture_state: None,
            },
            vec![RequestLogBundleAttemptSection {
                attempt_id: Some(101),
                attempt_index: 1,
                llm_request_blob_id: Some(llm_request.blob_id),
                llm_request_patch_id: llm_request.patch_id,
                llm_response_blob_id: Some(llm_response_blob_id),
                llm_response_capture_state: Some(LogBodyCaptureState::Complete),
            }],
            RequestLogBundleV2DiagnosticAssets {
                request_snapshot: Some(RequestLogBundleRequestSnapshot {
                    request_path: "/ai/openai/v1/chat/completions".to_string(),
                    operation_kind: "chat_completions".to_string(),
                    query_params: Vec::new(),
                    sanitized_original_headers: vec![RequestLogBundleHttpHeader {
                        name: "x-trace-id".to_string(),
                        value: "trace-1".to_string(),
                    }],
                }),
                candidate_manifest: Some(RequestLogBundleCandidateManifest {
                    items: vec![RequestLogBundleCandidateManifestItem {
                        candidate_position: 1,
                        route_id: Some(8),
                        route_name: Some("primary".to_string()),
                        provider_id: 2,
                        provider_key: "openai".to_string(),
                        model_id: 3,
                        model_name: "gpt-test".to_string(),
                        real_model_name: Some("gpt-real".to_string()),
                        llm_api_type: LlmApiType::Openai,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    }],
                }),
                transform_diagnostics: Some(RequestLogBundleTransformDiagnostics {
                    summary: RequestLogBundleTransformDiagnosticsSummary {
                        count: 1,
                        max_loss_level: Some(UnifiedTransformDiagnosticLossLevel::LossyMajor),
                        kinds: vec![UnifiedTransformDiagnosticKind::CapabilityDowngrade],
                        phases: vec![RequestLogBundleTransformDiagnosticPhase::Request],
                    },
                    items: vec![RequestLogBundleTransformDiagnosticItem {
                        phase: RequestLogBundleTransformDiagnosticPhase::Request,
                        diagnostic: diagnostic(),
                    }],
                }),
            },
        )
    }

    #[test]
    fn artifact_response_from_v2_bundle_reports_manifest_and_diagnostics() {
        let record = record();
        let attempt = attempt();
        let bundle = bundle();

        let response = build_request_log_artifact_response(&record, &[attempt], Some(&bundle));

        assert_eq!(response.payload_manifest.bundle_version, Some(2));
        assert!(response.payload_manifest.blob_count >= 2);
        assert_eq!(
            response.request_snapshot.unwrap().request_path,
            "/ai/openai/v1/chat/completions"
        );
        assert!(response.candidate_manifest.has_asset);
        assert_eq!(response.candidate_manifest.items[0].llm_api_type, "OPENAI");
        assert_eq!(
            response.candidate_manifest.items[0].provider_api_key_mode,
            "QUEUE"
        );
        assert!(response.transform_diagnostics.has_asset);
        assert_eq!(
            response
                .transform_diagnostics
                .summary
                .max_loss_level
                .as_deref(),
            Some("lossy_major")
        );
        assert!(response.replay_capability.attempt_upstream.available);
        assert!(response.replay_capability.gateway_request.available);
    }

    #[test]
    fn artifact_response_without_bundle_is_stable_and_replay_disabled() {
        let response = build_request_log_artifact_response(&record(), &[attempt()], None);

        assert_eq!(response.payload_manifest.log_id, 42);
        assert_eq!(response.payload_manifest.bundle_version, Some(2));
        assert!(!response.candidate_manifest.has_asset);
        assert!(!response.transform_diagnostics.has_asset);
        assert!(!response.replay_capability.attempt_upstream.available);
        assert_eq!(
            response.replay_capability.attempt_upstream.reasons,
            vec!["bundle_missing".to_string()]
        );
        assert!(!response.replay_capability.gateway_request.available);
    }
}
