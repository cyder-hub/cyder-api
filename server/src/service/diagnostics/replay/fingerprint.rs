use chrono::Utc;
use serde::Serialize;
use serde_json::Value;

use crate::{
    controller::BaseError,
    schema::enum_def::{RequestReplayKind, RequestReplaySemanticBasis},
    service::diagnostics::{
        body::sha256_hex,
        policy::DiagnosticsPolicy,
        replay::{
            source::DecodedBundleBody,
            types::{
                RequestReplayCandidateDecision, RequestReplayModelSnapshot, RequestReplayNameValue,
                RequestReplayQueryParam, RequestReplayResolvedCandidate,
                RequestReplayResolvedRoute,
            },
        },
    },
    utils::storage::RequestLogBundleCandidateManifest,
};

pub(crate) const REPLAY_PREVIEW_FINGERPRINT_VERSION: &str = "request-replay-preview-v1";

#[derive(Serialize)]
struct RequestReplayPreviewFingerprintEnvelope<'a> {
    version: &'static str,
    preview_created_at: i64,
    input: &'a RequestReplayPreviewFingerprintInput,
}

#[derive(Serialize)]
pub(crate) struct RequestReplayPreviewFingerprintInput {
    pub(crate) replay_kind: RequestReplayKind,
    pub(crate) source_request_log_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_attempt_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider_api_key_id_override: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) selected_provider_api_key_id: Option<i64>,
    pub(crate) used_provider_api_key_override: bool,
    pub(crate) semantic_basis: RequestReplaySemanticBasis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) requested_model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) base_requested_model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_reasoning_suffix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_reasoning_preset: Option<String>,
    pub(crate) input_snapshot: RequestReplayFingerprintInputSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_route: Option<RequestReplayResolvedRoute>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_name_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_candidate: Option<RequestReplayResolvedCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) candidate_manifest: Option<RequestLogBundleCandidateManifest>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidate_decisions: Vec<RequestReplayCandidateDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) applied_request_patch_summary: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) final_request_uri: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) final_request_headers: Vec<RequestReplayNameValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) final_request_body: Option<RequestReplayFingerprintBodyDigest>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RequestReplayFingerprintInputSnapshot {
    AttemptUpstream {
        request_uri: String,
        sanitized_request_headers: Vec<RequestReplayNameValue>,
        #[serde(skip_serializing_if = "Option::is_none")]
        llm_request_body: Option<RequestReplayFingerprintBodyDigest>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider: Option<super::types::RequestReplayProviderSnapshot>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<RequestReplayModelSnapshot>,
    },
    GatewayRequest {
        request_path: String,
        query_params: Vec<RequestReplayQueryParam>,
        sanitized_original_headers: Vec<RequestReplayNameValue>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_request_body: Option<RequestReplayFingerprintBodyDigest>,
    },
}

#[derive(Serialize)]
pub(crate) struct RequestReplayFingerprintBodyDigest {
    pub(crate) sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) capture_state: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParsedReplayPreviewConfirmation {
    pub(crate) preview_created_at: i64,
}

pub(crate) fn body_digest_from_decoded_body(
    body: &DecodedBundleBody,
) -> RequestReplayFingerprintBodyDigest {
    RequestReplayFingerprintBodyDigest {
        sha256: sha256_hex(&body.bytes),
        media_type: body.media_type.clone(),
        capture_state: body.capture_state.clone(),
    }
}

pub(crate) fn final_body_digest_from_bytes(
    body: &[u8],
    media_type: Option<String>,
    capture_state: Option<String>,
) -> RequestReplayFingerprintBodyDigest {
    RequestReplayFingerprintBodyDigest {
        sha256: sha256_hex(body),
        media_type,
        capture_state,
    }
}

pub(crate) fn canonical_uri_for_fingerprint(uri: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(uri) else {
        return uri.to_string();
    };
    let mut pairs = parsed
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return parsed.to_string();
    }

    pairs.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    parsed.set_query(None);
    {
        let mut query = parsed.query_pairs_mut();
        for (name, value) in pairs {
            query.append_pair(&name, &value);
        }
    }
    parsed.to_string()
}

pub(crate) fn build_replay_preview_fingerprint(
    preview_created_at: i64,
    input: &RequestReplayPreviewFingerprintInput,
) -> Result<String, BaseError> {
    let envelope = RequestReplayPreviewFingerprintEnvelope {
        version: REPLAY_PREVIEW_FINGERPRINT_VERSION,
        preview_created_at,
        input,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|err| {
        BaseError::InternalServerError(Some(format!(
            "Failed to build replay preview fingerprint: {}",
            err
        )))
    })?;
    Ok(format!(
        "{}:{}:{}",
        REPLAY_PREVIEW_FINGERPRINT_VERSION,
        preview_created_at,
        sha256_hex(&bytes)
    ))
}

pub(crate) fn parse_replay_preview_confirmation(
    fingerprint: Option<&str>,
    policy: &DiagnosticsPolicy,
) -> Result<ParsedReplayPreviewConfirmation, BaseError> {
    let raw = fingerprint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(replay_preview_confirmation_missing)?;
    let mut parts = raw.split(':');
    let version = parts.next();
    let created_at = parts.next();
    let digest = parts.next();

    if parts.next().is_some()
        || version != Some(REPLAY_PREVIEW_FINGERPRINT_VERSION)
        || digest.is_none_or(|value| {
            value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err(BaseError::ParamInvalid(Some(
            "Replay preview confirmation is invalid; regenerate preview before execute."
                .to_string(),
        )));
    }

    let preview_created_at = created_at
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(
                "Replay preview confirmation is invalid; regenerate preview before execute."
                    .to_string(),
            ))
        })?;
    let now = Utc::now().timestamp_millis();
    if preview_created_at <= 0
        || now.saturating_sub(preview_created_at) > policy.replay_preview_confirmation_ttl_ms()
        || preview_created_at.saturating_sub(now)
            > policy.replay_preview_confirmation_clock_skew_ms()
    {
        return Err(BaseError::ParamInvalid(Some(
            "Replay preview confirmation expired; regenerate preview before execute.".to_string(),
        )));
    }

    Ok(ParsedReplayPreviewConfirmation { preview_created_at })
}

pub(crate) fn ensure_replay_preview_confirmation_matches(
    provided: Option<&str>,
    expected: &str,
) -> Result<(), BaseError> {
    let provided = provided
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(replay_preview_confirmation_missing)?;
    if provided != expected {
        return Err(BaseError::ParamInvalid(Some(
            "Replay preview confirmation mismatch; regenerate preview before execute.".to_string(),
        )));
    }
    Ok(())
}

fn replay_preview_confirmation_missing() -> BaseError {
    BaseError::ParamInvalid(Some(
        "Replay preview confirmation is missing; regenerate preview before execute.".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_uri_orders_query_pairs_for_stable_fingerprints() {
        assert_eq!(
            canonical_uri_for_fingerprint(
                "https://upstream.example/v1/models/gemini:generate?b=2&a=1"
            ),
            canonical_uri_for_fingerprint(
                "https://upstream.example/v1/models/gemini:generate?a=1&b=2"
            )
        );
    }

    #[test]
    fn replay_preview_confirmation_distinguishes_missing_expired_invalid_and_mismatch() {
        let policy = DiagnosticsPolicy::default();
        let missing = parse_replay_preview_confirmation(None, &policy)
            .expect_err("missing confirmation should be rejected");
        assert!(
            matches!(missing, BaseError::ParamInvalid(Some(message)) if message.contains("missing"))
        );

        let invalid = parse_replay_preview_confirmation(Some("not-a-fingerprint"), &policy)
            .expect_err("invalid confirmation should be rejected");
        assert!(
            matches!(invalid, BaseError::ParamInvalid(Some(message)) if message.contains("invalid"))
        );

        let expired = format!(
            "{}:{}:{}",
            REPLAY_PREVIEW_FINGERPRINT_VERSION,
            1,
            "0".repeat(64)
        );
        let expired = parse_replay_preview_confirmation(Some(&expired), &policy)
            .expect_err("expired confirmation should be rejected");
        assert!(
            matches!(expired, BaseError::ParamInvalid(Some(message)) if message.contains("expired"))
        );

        let created_at = Utc::now().timestamp_millis();
        let current = format!(
            "{}:{}:{}",
            REPLAY_PREVIEW_FINGERPRINT_VERSION,
            created_at,
            "a".repeat(64)
        );
        let parsed = parse_replay_preview_confirmation(Some(&current), &policy)
            .expect("current confirmation should parse");
        assert_eq!(parsed.preview_created_at, created_at);

        let mismatch = ensure_replay_preview_confirmation_matches(Some(&current), "other")
            .expect_err("mismatched confirmation should be rejected");
        assert!(
            matches!(mismatch, BaseError::ParamInvalid(Some(message)) if message.contains("mismatch"))
        );
    }
}
