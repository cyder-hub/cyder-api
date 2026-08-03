use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE, response::Builder as HttpResponseBuilder},
    response::Response,
};
use cyder_tools::log::error;
use serde_json::Value;

use crate::{
    cost::UsageNormalization,
    proxy::util::{json_top_level_field_count_from_bytes, sha256_hex},
    schema::enum_def::{DownstreamProtocol, UpstreamProtocol},
    service::{
        transform::{
            transform_result_with_cost_and_diagnostics, unified::UnifiedTransformDiagnostic,
        },
        upstream_response::normalize_content_type,
    },
    utils::usage::UsageInfo,
};

pub(super) fn response_content_type(headers: &HeaderMap) -> Option<String> {
    normalize_content_type(headers).map(|content_type| content_type.value)
}

pub(super) fn build_response_builder(
    status_code: StatusCode,
    response_headers: &HeaderMap,
) -> HttpResponseBuilder {
    let mut response_builder = Response::builder().status(status_code);
    if let Some(content_type) = response_content_type(response_headers) {
        response_builder = response_builder.header(CONTENT_TYPE, content_type);
    }
    response_builder
}

pub(crate) fn process_success_response_body(
    decompressed_body: &Bytes,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
) -> (
    Bytes,
    Option<UsageInfo>,
    Option<UsageNormalization>,
    Vec<UnifiedTransformDiagnostic>,
) {
    match serde_json::from_slice::<Value>(decompressed_body) {
        Ok(original_value) => {
            let output = transform_result_with_cost_and_diagnostics(
                original_value,
                upstream_protocol,
                downstream_protocol,
            );

            let body_bytes = if matches!(
                (upstream_protocol, downstream_protocol),
                (UpstreamProtocol::Openai, DownstreamProtocol::Openai)
                    | (UpstreamProtocol::Responses, DownstreamProtocol::Responses)
                    | (UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic)
                    | (UpstreamProtocol::Gemini, DownstreamProtocol::Gemini)
            ) {
                decompressed_body.clone()
            } else {
                match serde_json::to_vec(&output.value) {
                    Ok(b) => Bytes::from(b),
                    Err(e) => {
                        error!(
                            "Failed to serialize transformed response: {}. Returning original body.",
                            e
                        );
                        decompressed_body.clone()
                    }
                }
            };
            (
                body_bytes,
                output.usage_info,
                output.usage_normalization,
                output.diagnostics,
            )
        }
        Err(e) => {
            crate::debug_event!(
                "proxy.response_non_json_passthrough",
                response_body_bytes = decompressed_body.len(),
                response_body_sha256 = sha256_hex(decompressed_body),
                parse_error = e,
                json_top_level_fields = json_top_level_field_count_from_bytes(decompressed_body),
            );
            (decompressed_body.clone(), None, None, Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{
            HeaderValue,
            header::{
                CACHE_CONTROL, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, LOCATION,
                RETRY_AFTER, SET_COOKIE, TRANSFER_ENCODING, WWW_AUTHENTICATE,
            },
        },
    };

    use super::*;

    #[test]
    fn downstream_success_builder_inherits_only_normalized_content_type() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("Application/JSON; Charset=UTF-8; profile=private"),
        );
        for (name, value) in [
            (CONTENT_LENGTH, "123"),
            (CONTENT_ENCODING, "gzip"),
            (TRANSFER_ENCODING, "chunked"),
            (SET_COOKIE, "session=secret"),
            (RETRY_AFTER, "60"),
            (LOCATION, "https://private.example/redirect"),
            (CACHE_CONTROL, "public"),
            (WWW_AUTHENTICATE, "Bearer private"),
        ] {
            headers.insert(name, HeaderValue::from_str(value).unwrap());
        }
        headers.insert(
            "x-provider-request-id",
            HeaderValue::from_static("private-id"),
        );

        let response = build_response_builder(StatusCode::OK, &headers)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/json; charset=utf-8"
        );
        assert_eq!(response.headers().len(), 1);
    }

    #[test]
    fn invalid_duplicate_or_oversized_content_type_is_not_inherited() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("invalid media type"));
        let response = build_response_builder(StatusCode::OK, &headers)
            .body(Body::empty())
            .unwrap();
        assert!(response.headers().is_empty());

        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.append(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        let response = build_response_builder(StatusCode::OK, &headers)
            .body(Body::empty())
            .unwrap();
        assert!(response.headers().is_empty());

        headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(&format!("text/plain; private={}", "x".repeat(257))).unwrap(),
        );
        let response = build_response_builder(StatusCode::OK, &headers)
            .body(Body::empty())
            .unwrap();
        assert!(response.headers().is_empty());
    }
}
