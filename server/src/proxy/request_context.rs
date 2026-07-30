use axum::http::{HeaderMap, HeaderName};
use chrono::Utc;
use uuid::Uuid;

pub(crate) const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
pub(crate) const X_CLIENT_REQUEST_ID: HeaderName = HeaderName::from_static("x-client-request-id");

const MAX_CLIENT_REQUEST_ID_LEN: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RequestId(String);

impl RequestId {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4().hyphenated().to_string())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ClientRequestId(String);

impl ClientRequestId {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Option<Self> {
        let mut values = headers.get_all(&X_CLIENT_REQUEST_ID).iter();
        let value = values.next()?;
        if values.next().is_some() {
            return None;
        }

        Self::parse(value.to_str().ok()?)
    }

    fn parse(value: &str) -> Option<Self> {
        if value.is_empty()
            || value.len() > MAX_CLIENT_REQUEST_ID_LEN
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
            })
        {
            return None;
        }

        Some(Self(value.to_string()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ClientRequestId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ProxyRequestContext {
    pub(crate) request_id: RequestId,
    pub(crate) client_request_id: Option<ClientRequestId>,
    pub(crate) received_at_ms: i64,
}

impl ProxyRequestContext {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            request_id: RequestId::new(),
            client_request_id: ClientRequestId::from_headers(headers),
            received_at_ms: Utc::now().timestamp_millis(),
        }
    }
}

pub(super) fn derive_request_operation_kind(request_path: &str) -> String {
    let normalized_path = request_path.trim_end_matches('/');
    if normalized_path.ends_with("/chat/completions") {
        "chat_completions_create".to_string()
    } else if normalized_path.ends_with("/responses") {
        "responses_create".to_string()
    } else if normalized_path.ends_with("/messages") {
        "messages_create".to_string()
    } else if normalized_path.ends_with("/embeddings") {
        "embeddings".to_string()
    } else if normalized_path.ends_with("/rerank") {
        "rerank".to_string()
    } else if normalized_path.ends_with("/models") {
        "models_list".to_string()
    } else {
        normalized_path
            .rsplit('/')
            .next()
            .filter(|segment| !segment.is_empty())
            .map(path_segment_to_operation_kind)
            .unwrap_or_else(|| "request".to_string())
    }
}

fn path_segment_to_operation_kind(segment: &str) -> String {
    if let Some((_, action)) = segment.split_once(':') {
        camel_case_to_snake_case(action)
    } else {
        segment
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect()
    }
}

fn camel_case_to_snake_case(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut prev_is_lower_or_digit = false;

    for ch in value.chars() {
        if ch.is_ascii_uppercase() {
            if prev_is_lower_or_digit && !normalized.ends_with('_') {
                normalized.push('_');
            }
            normalized.push(ch.to_ascii_lowercase());
            prev_is_lower_or_digit = false;
        } else if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            prev_is_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else if !normalized.ends_with('_') {
            normalized.push('_');
            prev_is_lower_or_digit = false;
        }
    }

    normalized.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};
    use uuid::Version;

    use super::{
        ClientRequestId, MAX_CLIENT_REQUEST_ID_LEN, ProxyRequestContext, RequestId,
        X_CLIENT_REQUEST_ID, derive_request_operation_kind,
    };

    #[test]
    fn request_id_is_lowercase_hyphenated_uuid_v4() {
        let request_id = RequestId::new();
        let value = request_id.as_str();
        let parsed = uuid::Uuid::parse_str(value).expect("request id should be a valid UUID");

        assert_eq!(value.len(), 36);
        assert_eq!(value, value.to_ascii_lowercase());
        assert_eq!(parsed.get_version(), Some(Version::Random));
        assert_eq!(parsed.hyphenated().to_string(), value);
    }

    #[test]
    fn proxy_request_context_generates_request_identity_at_ingress() {
        let mut headers = HeaderMap::new();
        headers.insert(
            &X_CLIENT_REQUEST_ID,
            HeaderValue::from_static("caller.trace_1:part-2"),
        );

        let context = ProxyRequestContext::from_headers(&headers);

        assert_eq!(
            context
                .client_request_id
                .as_ref()
                .map(|value| value.as_str()),
            Some("caller.trace_1:part-2")
        );
        assert!(context.received_at_ms > 0);
    }

    #[test]
    fn client_request_id_accepts_single_safe_ascii_value_up_to_limit() {
        let value = "a".repeat(MAX_CLIENT_REQUEST_ID_LEN);
        let mut headers = HeaderMap::new();
        headers.insert(
            &X_CLIENT_REQUEST_ID,
            HeaderValue::from_str(&value).expect("test header should construct"),
        );

        assert_eq!(
            ClientRequestId::from_headers(&headers)
                .as_ref()
                .map(|request_id| request_id.as_str()),
            Some(value.as_str())
        );
    }

    #[test]
    fn client_request_id_rejects_missing_empty_overlong_or_unsafe_values() {
        let missing = HeaderMap::new();
        assert_eq!(ClientRequestId::from_headers(&missing), None);

        for value in ["", "contains space", "comma,separated", "slash/value"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                &X_CLIENT_REQUEST_ID,
                HeaderValue::from_str(value).expect("test header should construct"),
            );
            assert_eq!(
                ClientRequestId::from_headers(&headers),
                None,
                "value should be rejected: {value:?}"
            );
        }

        let mut overlong = HeaderMap::new();
        overlong.insert(
            &X_CLIENT_REQUEST_ID,
            HeaderValue::from_str(&"a".repeat(MAX_CLIENT_REQUEST_ID_LEN + 1))
                .expect("test header should construct"),
        );
        assert_eq!(ClientRequestId::from_headers(&overlong), None);
    }

    #[test]
    fn client_request_id_rejects_multiple_or_non_utf8_header_values() {
        let mut multiple = HeaderMap::new();
        multiple.append(&X_CLIENT_REQUEST_ID, HeaderValue::from_static("caller-one"));
        multiple.append(&X_CLIENT_REQUEST_ID, HeaderValue::from_static("caller-two"));
        assert_eq!(ClientRequestId::from_headers(&multiple), None);

        let mut non_utf8 = HeaderMap::new();
        non_utf8.insert(
            &X_CLIENT_REQUEST_ID,
            HeaderValue::from_bytes(&[0x80]).expect("opaque header should construct"),
        );
        assert_eq!(ClientRequestId::from_headers(&non_utf8), None);
    }

    #[test]
    fn derive_request_operation_kind_covers_common_routes() {
        assert_eq!(
            derive_request_operation_kind("/openai/v1/chat/completions"),
            "chat_completions_create"
        );
        assert_eq!(
            derive_request_operation_kind("/responses/v1/responses"),
            "responses_create"
        );
        assert_eq!(
            derive_request_operation_kind("/gemini/v1beta/models/foo:streamGenerateContent"),
            "stream_generate_content"
        );
    }
}
