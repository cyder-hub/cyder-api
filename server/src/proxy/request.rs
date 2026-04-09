use super::{ProxyError, classify_request_body_error};
use crate::config::CONFIG;
use axum::{body::Body, extract::Request};
use bytes::Bytes;
use cyder_tools::log::debug;
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

    debug!(
        "[proxy] original request data: {}",
        serde_json::to_string(&data).unwrap_or_default()
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
