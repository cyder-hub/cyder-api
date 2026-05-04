use chrono::Utc;
use std::collections::BTreeMap;

use openssl::{hash::MessageDigest, pkey::PKey, sign::Signer};
use reqwest::{
    Client, RequestBuilder,
    header::{CONTENT_TYPE, HeaderName, HeaderValue},
};
use serde_json::json;
use tokio::time::{Duration, timeout};

use crate::controller::BaseError;
use crate::database::notification::record_channel_test_result;

use super::service::NotificationService;
use super::types::{NotificationEventType, NotificationWebhookTestResult};

pub const HEADER_CYDER_EVENT: &str = "x-cyder-event";
pub const HEADER_CYDER_ALERT_FINGERPRINT: &str = "x-cyder-alert-fingerprint";
pub const HEADER_CYDER_SIGNATURE: &str = "x-cyder-signature";

#[derive(Debug, Clone, Copy)]
pub struct WebhookRequestOptions<'a> {
    pub event_type: &'a str,
    pub alert_fingerprint: Option<&'a str>,
    pub signing_secret: Option<&'a str>,
    pub headers_json: Option<&'a str>,
}

impl NotificationService {
    pub async fn test_webhook_channel(
        &self,
        channel_id: i64,
        client: &Client,
    ) -> Result<NotificationWebhookTestResult, BaseError> {
        let channel = self.get_channel_secret(channel_id)?;
        let payload = build_test_webhook_payload(channel.id, &channel.channel_key);
        let result = send_webhook_payload(
            client,
            &channel.endpoint_url,
            &payload,
            self.config().webhook_timeout_seconds,
            WebhookRequestOptions {
                event_type: NotificationEventType::Test.as_str(),
                alert_fingerprint: Some("test"),
                signing_secret: channel.signing_secret.as_deref(),
                headers_json: channel.headers_json.as_deref(),
            },
        )
        .await;
        let now_ms = Utc::now().timestamp_millis();
        let success = result.as_ref().is_ok_and(|item| item.success);
        let error = result
            .as_ref()
            .ok()
            .and_then(|item| item.error.clone())
            .or_else(|| result.as_ref().err().map(|err| format!("{err:?}")));
        record_channel_test_result(channel.id, success, error.clone(), now_ms)?;
        result
    }
}

pub fn build_test_webhook_payload(channel_id: i64, channel_key: &str) -> serde_json::Value {
    json!({
        "event_type": NotificationEventType::Test.as_str(),
        "sent_at": Utc::now().timestamp_millis(),
        "channel_id": channel_id,
        "channel_key": channel_key,
        "message": "Cyder notification webhook test",
    })
}

pub async fn send_webhook_payload(
    client: &Client,
    endpoint_url: &str,
    payload: &serde_json::Value,
    timeout_seconds: u64,
    options: WebhookRequestOptions<'_>,
) -> Result<NotificationWebhookTestResult, BaseError> {
    send_webhook_json(client, endpoint_url, payload, timeout_seconds, options).await
}

pub async fn send_webhook_json(
    client: &Client,
    endpoint_url: &str,
    payload: &serde_json::Value,
    timeout_seconds: u64,
    options: WebhookRequestOptions<'_>,
) -> Result<NotificationWebhookTestResult, BaseError> {
    let body = serde_json::to_vec(payload).map_err(|err| {
        BaseError::InternalServerError(Some(format!("failed to serialize webhook payload: {err}")))
    })?;
    let mut request = client
        .post(endpoint_url)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .header(HEADER_CYDER_EVENT, options.event_type)
        .body(body.clone());
    if let Some(fingerprint) = options.alert_fingerprint {
        request = request.header(HEADER_CYDER_ALERT_FINGERPRINT, fingerprint);
    }
    if let Some(headers_json) = options.headers_json {
        for (name, value) in parse_custom_headers(headers_json)? {
            request = request.header(name, value);
        }
    }
    if let Some(secret) = options.signing_secret.filter(|value| !value.is_empty()) {
        let signature = hmac_sha256_header_value(secret, &body)?;
        request = request.header(HEADER_CYDER_SIGNATURE, signature);
    }
    execute_webhook_request(request, timeout_seconds).await
}

async fn execute_webhook_request(
    request: RequestBuilder,
    timeout_seconds: u64,
) -> Result<NotificationWebhookTestResult, BaseError> {
    let response = timeout(Duration::from_secs(timeout_seconds.max(1)), request.send())
        .await
        .map_err(|_| BaseError::InternalServerError(Some("webhook request timed out".to_string())))?
        .map_err(|err| {
            BaseError::InternalServerError(Some(format!("webhook request failed: {err}")))
        })?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let preview = (!body.is_empty()).then(|| body.chars().take(512).collect::<String>());
    Ok(NotificationWebhookTestResult {
        success: status.is_success(),
        status: Some(status.as_u16()),
        error: (!status.is_success()).then(|| format!("webhook returned HTTP {}", status.as_u16())),
        response_body_preview: preview,
    })
}

pub fn normalize_headers_json(headers_json: Option<String>) -> Result<Option<String>, BaseError> {
    let Some(raw) = headers_json else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let map = parse_header_map(trimmed)?;
    serde_json::to_string(&map).map(Some).map_err(|err| {
        BaseError::InternalServerError(Some(format!("failed to normalize headers_json: {err}")))
    })
}

fn parse_custom_headers(headers_json: &str) -> Result<Vec<(HeaderName, HeaderValue)>, BaseError> {
    parse_header_map(headers_json)?
        .into_iter()
        .map(|(name, value)| {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                BaseError::ParamInvalid(Some(format!("invalid webhook header name '{name}'")))
            })?;
            let header_value = HeaderValue::from_str(&value).map_err(|_| {
                BaseError::ParamInvalid(Some(format!("invalid webhook header value for '{name}'")))
            })?;
            Ok((header_name, header_value))
        })
        .collect()
}

fn parse_header_map(headers_json: &str) -> Result<BTreeMap<String, String>, BaseError> {
    let value = serde_json::from_str::<serde_json::Value>(headers_json).map_err(|err| {
        BaseError::ParamInvalid(Some(format!("headers_json must be a JSON object: {err}")))
    })?;
    let object = value.as_object().ok_or_else(|| {
        BaseError::ParamInvalid(Some("headers_json must be a JSON object".to_string()))
    })?;
    let mut headers = BTreeMap::new();
    for (name, value) in object {
        let normalized_name = name.trim();
        if normalized_name.is_empty() {
            return Err(BaseError::ParamInvalid(Some(
                "headers_json cannot contain empty header names".to_string(),
            )));
        }
        if is_reserved_header(normalized_name) {
            return Err(BaseError::ParamInvalid(Some(format!(
                "headers_json cannot override reserved header '{normalized_name}'"
            ))));
        }
        let Some(header_value) = value.as_str() else {
            return Err(BaseError::ParamInvalid(Some(format!(
                "headers_json value for '{normalized_name}' must be a string"
            ))));
        };
        HeaderName::from_bytes(normalized_name.as_bytes()).map_err(|_| {
            BaseError::ParamInvalid(Some(format!(
                "invalid webhook header name '{normalized_name}'"
            )))
        })?;
        HeaderValue::from_str(header_value).map_err(|_| {
            BaseError::ParamInvalid(Some(format!(
                "invalid webhook header value for '{normalized_name}'"
            )))
        })?;
        headers.insert(normalized_name.to_string(), header_value.to_string());
    }
    Ok(headers)
}

fn is_reserved_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "content-type"
            | HEADER_CYDER_EVENT
            | HEADER_CYDER_ALERT_FINGERPRINT
            | HEADER_CYDER_SIGNATURE
    )
}

pub(crate) fn hmac_sha256_header_value(secret: &str, body: &[u8]) -> Result<String, BaseError> {
    let key = PKey::hmac(secret.as_bytes()).map_err(|err| {
        BaseError::InternalServerError(Some(format!("failed to create webhook signing key: {err}")))
    })?;
    let mut signer = Signer::new(MessageDigest::sha256(), &key).map_err(|err| {
        BaseError::InternalServerError(Some(format!("failed to create webhook signer: {err}")))
    })?;
    signer.update(body).map_err(|err| {
        BaseError::InternalServerError(Some(format!("failed to sign webhook body: {err}")))
    })?;
    let signature = signer.sign_to_vec().map_err(|err| {
        BaseError::InternalServerError(Some(format!("failed to finalize webhook signature: {err}")))
    })?;
    Ok(format!("sha256={}", hex_lower(&signature)))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payload_uses_standard_test_event_type() {
        let payload = build_test_webhook_payload(7, "ops");
        assert_eq!(
            payload.get("event_type").and_then(|value| value.as_str()),
            Some("test")
        );
        assert_eq!(
            payload.get("channel_id").and_then(|value| value.as_i64()),
            Some(7)
        );
        assert_eq!(
            payload.get("channel_key").and_then(|value| value.as_str()),
            Some("ops")
        );
        assert_eq!(
            payload.get("message").and_then(|value| value.as_str()),
            Some("Cyder notification webhook test")
        );
    }

    #[test]
    fn signature_uses_raw_body_bytes() {
        let signature = hmac_sha256_header_value("secret", br#"{"event_type":"test"}"#).unwrap();
        assert_eq!(
            signature,
            "sha256=63a0202d40deb95e436bda3e1e03e0f433389d686c2b947f21cbb976e18ec7a7"
        );
    }

    #[test]
    fn custom_headers_normalize_and_reject_reserved_headers() {
        assert_eq!(
            normalize_headers_json(Some(r#"{"X-Ops":"primary"}"#.to_string())).unwrap(),
            Some(r#"{"X-Ops":"primary"}"#.to_string())
        );
        assert!(
            normalize_headers_json(Some(r#"{"Content-Type":"text/plain"}"#.to_string())).is_err()
        );
    }
}
