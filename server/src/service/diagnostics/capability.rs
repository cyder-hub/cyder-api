use serde::Serialize;

use crate::{database::request_attempt::RequestAttemptDetail, utils::storage::RequestLogBundleV2};

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayCapabilitySummary {
    pub attempt_upstream: ReplayKindCapabilitySummary,
    pub gateway_request: ReplayKindCapabilitySummary,
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayKindCapabilitySummary {
    pub available: bool,
    pub reasons: Vec<String>,
    pub attempt_ids: Vec<i64>,
}

pub(crate) fn build_replay_capability(
    bundle: Option<&RequestLogBundleV2>,
    attempts: &[RequestAttemptDetail],
) -> ReplayCapabilitySummary {
    let attempt_upstream = build_attempt_upstream_capability(bundle, attempts);
    let gateway_request = build_gateway_request_capability(bundle);

    ReplayCapabilitySummary {
        attempt_upstream,
        gateway_request,
    }
}

fn build_attempt_upstream_capability(
    bundle: Option<&RequestLogBundleV2>,
    attempts: &[RequestAttemptDetail],
) -> ReplayKindCapabilitySummary {
    let Some(bundle) = bundle else {
        return unavailable("bundle_missing");
    };

    let attempt_ids = attempts
        .iter()
        .filter(|attempt| {
            has_non_empty(attempt.request_uri.as_deref())
                && has_non_empty(attempt.request_headers_json.as_deref())
                && attempt_request_body_available(bundle, attempt)
        })
        .map(|attempt| attempt.id)
        .collect::<Vec<_>>();

    if attempt_ids.is_empty() {
        unavailable("no_attempt_with_complete_downstream_request_snapshot")
    } else {
        ReplayKindCapabilitySummary {
            available: true,
            reasons: Vec::new(),
            attempt_ids,
        }
    }
}

fn build_gateway_request_capability(
    bundle: Option<&RequestLogBundleV2>,
) -> ReplayKindCapabilitySummary {
    let Some(bundle) = bundle else {
        return unavailable("request_snapshot_missing");
    };

    if bundle
        .request_snapshot
        .as_ref()
        .is_none_or(|snapshot| snapshot.request_path.is_empty())
    {
        return unavailable("request_snapshot_missing");
    }

    if !bundle
        .request_section
        .user_request_blob_id
        .is_some_and(|blob_id| blob_exists(bundle, blob_id))
    {
        return unavailable("user_request_body_missing");
    }

    ReplayKindCapabilitySummary {
        available: true,
        reasons: Vec::new(),
        attempt_ids: Vec::new(),
    }
}

fn attempt_request_body_available(
    bundle: &RequestLogBundleV2,
    attempt: &RequestAttemptDetail,
) -> bool {
    bundle
        .attempt_sections
        .iter()
        .find(|section| {
            section
                .attempt_id
                .is_some_and(|attempt_id| attempt_id == attempt.id)
                || section.attempt_index == attempt.attempt_index
        })
        .is_some_and(|section| {
            section
                .llm_request_blob_id
                .is_some_and(|blob_id| blob_exists(bundle, blob_id))
                && section
                    .llm_request_patch_id
                    .is_none_or(|patch_id| patch_exists(bundle, patch_id))
        })
}

fn blob_exists(bundle: &RequestLogBundleV2, blob_id: i32) -> bool {
    bundle.blob_pool.iter().any(|blob| blob.blob_id == blob_id)
}

fn patch_exists(bundle: &RequestLogBundleV2, patch_id: i32) -> bool {
    bundle
        .patch_pool
        .iter()
        .any(|patch| patch.patch_id == patch_id)
}

fn unavailable(reason: &str) -> ReplayKindCapabilitySummary {
    ReplayKindCapabilitySummary {
        available: false,
        reasons: vec![reason.to_string()],
        attempt_ids: Vec::new(),
    }
}

fn has_non_empty(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use crate::{
        database::request_attempt::RequestAttemptDetail,
        schema::enum_def::LlmApiType,
        utils::storage::{
            LogBodyCaptureState, RequestLogBundleAttemptSection, RequestLogBundleHttpHeader,
            RequestLogBundleRequestSection, RequestLogBundleRequestSnapshot, RequestLogBundleV2,
            RequestLogBundleV2Builder, RequestLogBundleV2DiagnosticAssets,
        },
    };

    use super::build_replay_capability;

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
                ..Default::default()
            },
        )
    }

    #[test]
    fn replay_capability_is_available_when_snapshots_and_bodies_exist() {
        let bundle = bundle();
        let capability = build_replay_capability(Some(&bundle), &[attempt()]);

        assert!(capability.attempt_upstream.available);
        assert_eq!(capability.attempt_upstream.attempt_ids, vec![101]);
        assert!(capability.gateway_request.available);
    }

    #[test]
    fn replay_capability_keeps_bundle_missing_reason_for_attempt_replay() {
        let capability = build_replay_capability(None, &[attempt()]);

        assert!(!capability.attempt_upstream.available);
        assert_eq!(
            capability.attempt_upstream.reasons,
            vec!["bundle_missing".to_string()]
        );
        assert!(!capability.gateway_request.available);
        assert_eq!(
            capability.gateway_request.reasons,
            vec!["request_snapshot_missing".to_string()]
        );
    }

    #[test]
    fn replay_capability_keeps_attempt_snapshot_reason_key() {
        let mut bundle = bundle();
        bundle.attempt_sections[0].llm_request_blob_id = None;

        let capability = build_replay_capability(Some(&bundle), &[attempt()]);

        assert!(!capability.attempt_upstream.available);
        assert_eq!(
            capability.attempt_upstream.reasons,
            vec!["no_attempt_with_complete_downstream_request_snapshot".to_string()]
        );
        assert!(capability.gateway_request.available);
    }

    #[test]
    fn replay_capability_keeps_gateway_request_snapshot_reason_key() {
        let mut bundle = bundle();
        bundle.request_snapshot = None;

        let capability = build_replay_capability(Some(&bundle), &[attempt()]);

        assert!(capability.attempt_upstream.available);
        assert!(!capability.gateway_request.available);
        assert_eq!(
            capability.gateway_request.reasons,
            vec!["request_snapshot_missing".to_string()]
        );
    }

    #[test]
    fn replay_capability_keeps_gateway_user_body_reason_key() {
        let mut bundle = bundle();
        bundle.request_section.user_request_blob_id = None;

        let capability = build_replay_capability(Some(&bundle), &[attempt()]);

        assert!(capability.attempt_upstream.available);
        assert!(!capability.gateway_request.available);
        assert_eq!(
            capability.gateway_request.reasons,
            vec!["user_request_body_missing".to_string()]
        );
    }
}
