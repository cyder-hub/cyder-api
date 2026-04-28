use std::io::Read;

use axum::{
    body::Bytes,
    http::{
        HeaderMap, HeaderName, StatusCode,
        header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING},
        response::Builder as HttpResponseBuilder,
    },
    response::Response,
};
use cyder_tools::log::error;
use flate2::read::GzDecoder;
use serde_json::Value;

use crate::{
    cost::UsageNormalization,
    proxy::util::{json_top_level_field_count_from_bytes, sha256_hex},
    schema::enum_def::LlmApiType,
    service::transform::{
        transform_result_with_cost_and_diagnostics, unified::UnifiedTransformDiagnostic,
    },
    utils::usage::UsageInfo,
};

pub(super) fn should_forward_response_header(name: &HeaderName) -> bool {
    name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING
}

pub(super) fn response_content_type(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
}

pub(super) fn build_response_builder(
    status_code: StatusCode,
    response_headers: &HeaderMap,
) -> HttpResponseBuilder {
    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        if should_forward_response_header(name) {
            response_builder = response_builder.header(name, value);
        }
    }
    response_builder
}

pub(crate) fn decode_response_body(body_bytes: Bytes, is_gzip: bool) -> Bytes {
    if !is_gzip {
        return body_bytes;
    }

    if body_bytes.is_empty() {
        return Bytes::new();
    }

    let mut gz = GzDecoder::new(&body_bytes[..]);
    let mut decompressed_data = Vec::new();
    match gz.read_to_end(&mut decompressed_data) {
        Ok(_) => Bytes::from(decompressed_data),
        Err(e) => {
            error!("Gzip decoding failed: {}", e);
            body_bytes
        }
    }
}

pub(crate) fn process_success_response_body(
    decompressed_body: &Bytes,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
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
                target_api_type,
                api_type,
            );

            let body_bytes = if api_type == target_api_type {
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
