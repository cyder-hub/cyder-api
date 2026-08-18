use axum::http::{HeaderValue, StatusCode};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;
use serde_json::Value;
use std::fmt;

pub(crate) const UPSTREAM_ERROR_TRUNCATION_NOTICE: &str =
    "Upstream error body was truncated by the gateway.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamPayloadKind {
    Json,
    Text,
    Base64,
}

impl UpstreamPayloadKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
            Self::Base64 => "base64",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(untagged)]
enum UpstreamErrorBody {
    Json {
        body: Value,
    },
    Text {
        body_text: String,
    },
    Base64 {
        body_base64: String,
        body_encoding: &'static str,
    },
}

#[derive(Clone, Serialize)]
pub(crate) struct UpstreamErrorPayload {
    status: u16,
    content_type: Option<String>,
    #[serde(flatten)]
    body: UpstreamErrorBody,
    truncated: bool,
    captured_bytes: usize,
    limit_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<&'static str>,
}

impl UpstreamErrorPayload {
    #[cfg(test)]
    pub(crate) fn capture(
        status: StatusCode,
        content_type: Option<&HeaderValue>,
        body: &[u8],
        limit_bytes: usize,
    ) -> Self {
        assert!(
            limit_bytes > 0,
            "upstream error body limit must be positive"
        );

        let truncated = body.len() > limit_bytes;
        let captured_bytes = body.len().min(limit_bytes);
        Self::from_captured_prefix(
            status,
            content_type,
            &body[..captured_bytes],
            limit_bytes,
            truncated,
        )
    }

    pub(crate) fn from_captured_prefix(
        status: StatusCode,
        content_type: Option<&HeaderValue>,
        captured: &[u8],
        limit_bytes: usize,
        truncated: bool,
    ) -> Self {
        assert!(
            limit_bytes > 0,
            "upstream error body limit must be positive"
        );
        assert!(
            captured.len() <= limit_bytes,
            "captured upstream prefix must fit the disclosure limit"
        );
        let captured_bytes = captured.len();
        let body = if !truncated {
            match serde_json::from_slice::<Value>(captured) {
                Ok(body) => UpstreamErrorBody::Json { body },
                Err(_) => encode_non_json(captured),
            }
        } else {
            encode_non_json(captured)
        };

        Self {
            status: status.as_u16(),
            content_type: content_type
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            body,
            truncated,
            captured_bytes,
            limit_bytes,
            notice: truncated.then_some(UPSTREAM_ERROR_TRUNCATION_NOTICE),
        }
    }

    pub(crate) const fn status(&self) -> u16 {
        self.status
    }

    pub(crate) const fn truncated(&self) -> bool {
        self.truncated
    }

    pub(crate) const fn captured_bytes(&self) -> usize {
        self.captured_bytes
    }

    pub(crate) const fn limit_bytes(&self) -> usize {
        self.limit_bytes
    }

    pub(crate) const fn payload_kind(&self) -> UpstreamPayloadKind {
        match &self.body {
            UpstreamErrorBody::Json { .. } => UpstreamPayloadKind::Json,
            UpstreamErrorBody::Text { .. } => UpstreamPayloadKind::Text,
            UpstreamErrorBody::Base64 { .. } => UpstreamPayloadKind::Base64,
        }
    }
}

impl fmt::Debug for UpstreamErrorPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "UpstreamErrorPayload {{ status: {}, content_type: {:?}, payload_kind: {:?}, truncated: {}, captured_bytes: {}, limit_bytes: {} }}",
            self.status,
            self.content_type,
            self.payload_kind().as_str(),
            self.truncated,
            self.captured_bytes,
            self.limit_bytes,
        )
    }
}

fn encode_non_json(body: &[u8]) -> UpstreamErrorBody {
    match std::str::from_utf8(body) {
        Ok(body_text) => UpstreamErrorBody::Text {
            body_text: body_text.to_string(),
        },
        Err(_) => UpstreamErrorBody::Base64 {
            body_base64: STANDARD.encode(body),
            body_encoding: "base64",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{UPSTREAM_ERROR_TRUNCATION_NOTICE, UpstreamErrorPayload, UpstreamPayloadKind};
    use axum::http::{HeaderValue, StatusCode};
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    fn value(payload: &UpstreamErrorPayload) -> serde_json::Value {
        serde_json::to_value(payload).expect("upstream payload should serialize")
    }

    #[test]
    fn complete_json_is_preserved_as_a_semantic_json_value() {
        let content_type = HeaderValue::from_static("application/json; charset=utf-8");
        let payload = UpstreamErrorPayload::capture(
            StatusCode::TOO_MANY_REQUESTS,
            Some(&content_type),
            br#"{"error":{"message":"quota","details":[1,true,null]}}"#,
            65_536,
        );
        let value = value(&payload);

        assert_eq!(value["status"], 429);
        assert_eq!(value["content_type"], "application/json; charset=utf-8");
        assert_eq!(value["body"]["error"]["message"], "quota");
        assert_eq!(value["body"]["error"]["details"][1], true);
        assert_eq!(value["truncated"], false);
        assert!(value.get("body_text").is_none());
        assert!(value.get("notice").is_none());
        assert_eq!(payload.payload_kind(), UpstreamPayloadKind::Json);
    }

    #[test]
    fn non_json_utf8_and_empty_bodies_use_body_text() {
        for body in [b"provider unavailable".as_slice(), b"".as_slice()] {
            let payload =
                UpstreamErrorPayload::capture(StatusCode::BAD_GATEWAY, None, body, 65_536);
            let value = value(&payload);

            assert_eq!(
                value["body_text"],
                std::str::from_utf8(body).expect("fixture is utf-8")
            );
            assert_eq!(payload.payload_kind(), UpstreamPayloadKind::Text);
        }
    }

    #[test]
    fn non_utf8_body_is_byte_reversible_base64() {
        let body = [0, 159, 146, 150, 255, 1];
        let payload = UpstreamErrorPayload::capture(StatusCode::BAD_GATEWAY, None, &body, 65_536);
        let value = value(&payload);
        let encoded = value["body_base64"]
            .as_str()
            .expect("base64 body should be a string");

        assert_eq!(value["body_encoding"], "base64");
        assert_eq!(
            STANDARD.decode(encoded).expect("base64 should decode"),
            body
        );
        assert_eq!(payload.payload_kind(), UpstreamPayloadKind::Base64);
    }

    #[test]
    fn exact_limit_is_complete_but_excess_body_is_a_text_prefix_with_notice() {
        let exact = br#"{"ok":true}"#;
        let complete =
            UpstreamErrorPayload::capture(StatusCode::BAD_REQUEST, None, exact, exact.len());
        let complete_value = value(&complete);
        assert_eq!(complete_value["body"]["ok"], true);
        assert_eq!(complete_value["truncated"], false);

        let truncated =
            UpstreamErrorPayload::capture(StatusCode::BAD_REQUEST, None, exact, exact.len() - 1);
        let truncated_value = value(&truncated);
        assert_eq!(
            truncated_value["body_text"],
            std::str::from_utf8(&exact[..exact.len() - 1]).expect("prefix is utf-8")
        );
        assert_eq!(truncated_value["truncated"], true);
        assert_eq!(truncated_value["captured_bytes"], exact.len() - 1);
        assert_eq!(truncated_value["limit_bytes"], exact.len() - 1);
        assert_eq!(truncated_value["notice"], UPSTREAM_ERROR_TRUNCATION_NOTICE);
        assert!(truncated_value.get("body").is_none());
    }

    #[test]
    fn externally_bounded_prefix_preserves_truncation_even_at_disclosure_limit() {
        let prefix = br#"{"error":"provider prefix"}"#;
        let payload = UpstreamErrorPayload::from_captured_prefix(
            StatusCode::TOO_MANY_REQUESTS,
            None,
            prefix,
            prefix.len(),
            true,
        );
        let value = value(&payload);

        assert_eq!(value["status"], 429);
        assert_eq!(value["body_text"], std::str::from_utf8(prefix).unwrap());
        assert_eq!(value["truncated"], true);
        assert_eq!(value["captured_bytes"], prefix.len());
        assert_eq!(value["limit_bytes"], prefix.len());
        assert_eq!(value["notice"], UPSTREAM_ERROR_TRUNCATION_NOTICE);
        assert!(value.get("body").is_none());
    }

    #[test]
    fn invalid_utf8_truncated_prefix_remains_byte_reversible() {
        let body = [0xff, 0xfe, 0xfd, b'a'];
        let payload = UpstreamErrorPayload::capture(StatusCode::BAD_GATEWAY, None, &body, 3);
        let value = value(&payload);
        let encoded = value["body_base64"]
            .as_str()
            .expect("base64 body should be a string");

        assert_eq!(
            STANDARD.decode(encoded).expect("base64 should decode"),
            body[..3]
        );
        assert_eq!(value["truncated"], true);
    }

    #[test]
    fn absent_or_non_text_content_type_serializes_as_null() {
        let invalid = HeaderValue::from_bytes(&[0xff]).expect("opaque header value is accepted");
        for content_type in [None, Some(&invalid)] {
            let payload =
                UpstreamErrorPayload::capture(StatusCode::BAD_GATEWAY, content_type, b"x", 10);
            assert!(value(&payload)["content_type"].is_null());
        }
    }

    #[test]
    fn debug_output_contains_metadata_but_not_provider_body() {
        let payload = UpstreamErrorPayload::capture(
            StatusCode::UNAUTHORIZED,
            None,
            br#"{"secret":"provider-owned"}"#,
            65_536,
        );
        let debug = format!("{payload:?}");

        assert!(debug.contains("status: 401"));
        assert!(debug.contains("payload_kind: \"json\""));
        assert!(!debug.contains("provider-owned"));
        assert!(!debug.contains("secret"));
    }
}
