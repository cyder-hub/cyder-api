use super::{
    ProxyError, classify_request_body_error,
    util::{sha256_hex, top_level_json_field_count},
};
use crate::config::CONFIG;
use axum::{body::Body, extract::Request};
use bytes::Bytes;
use serde_json::Value;

#[derive(Debug)]
pub(super) struct ParsedProxyRequest {
    pub data: Value,
    pub original_request_value: Value,
    pub original_request_body: Bytes,
}

pub(super) async fn parse_json_request(
    request: Request<Body>,
) -> Result<ParsedProxyRequest, ProxyError> {
    let body_bytes = axum::body::to_bytes(request.into_body(), CONFIG.max_body_size)
        .await
        .map_err(|e| classify_request_body_error(e.to_string()))?;
    let data: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::BadRequest(format!("Failed to parse request body: {}", e)))?;

    crate::debug_event!(
        "proxy.request_parsed",
        request_body_bytes = body_bytes.len(),
        request_body_sha256 = sha256_hex(&body_bytes),
        json_top_level_fields = top_level_json_field_count(&data),
        stream = data.get("stream").and_then(Value::as_bool),
    );

    Ok(ParsedProxyRequest {
        original_request_value: data.clone(),
        data,
        original_request_body: body_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_json_request;
    use crate::proxy::ProxyError;
    use axum::{body::Body, extract::Request};
    use serde_json::json;

    #[tokio::test]
    async fn parse_json_request_keeps_original_value_and_body() {
        let request = Request::builder()
            .uri("/v1/chat/completions")
            .body(Body::from(r#"{"model":"gpt-test","stream":true}"#))
            .unwrap();

        let parsed = parse_json_request(request).await.unwrap();

        assert_eq!(parsed.data, json!({"model":"gpt-test","stream":true}));
        assert_eq!(parsed.original_request_value, parsed.data);
        assert_eq!(
            parsed.original_request_body,
            bytes::Bytes::from_static(br#"{"model":"gpt-test","stream":true}"#)
        );
    }

    #[tokio::test]
    async fn parse_json_request_rejects_invalid_json() {
        let request = Request::builder()
            .uri("/v1/chat/completions")
            .body(Body::from("{not-json"))
            .unwrap();

        let err = parse_json_request(request).await.unwrap_err();

        assert!(matches!(err, ProxyError::BadRequest(_)));
    }
}
