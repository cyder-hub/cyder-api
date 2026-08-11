use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE, response::Builder as HttpResponseBuilder},
    response::Response,
};
use serde_json::Value;

use crate::{
    cost::UsageNormalization,
    schema::enum_def::{DownstreamProtocol, UpstreamProtocol},
    service::{
        transform::diagnostics::transform_failure,
        transform::{
            TransformAction, TransformDiagnosticCollector, TransformDiagnosticFact,
            TransformFailure, TransformFailureOrigin, TransformOutcomeKind,
            TransformOutcomeSummary, TransformPhase, TransformReasonCode, TransformSafeSummary,
            TransformSemanticUnit, transform_result_with_cost,
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
) -> Result<
    (
        Bytes,
        Option<UsageInfo>,
        Option<UsageNormalization>,
        TransformOutcomeSummary,
    ),
    TransformFailure,
> {
    match serde_json::from_slice::<Value>(decompressed_body) {
        Ok(original_value) => {
            let output =
                transform_result_with_cost(original_value, upstream_protocol, downstream_protocol)?;

            let body_bytes = if matches!(
                (upstream_protocol, downstream_protocol),
                (UpstreamProtocol::Openai, DownstreamProtocol::Openai)
                    | (UpstreamProtocol::Responses, DownstreamProtocol::Responses)
                    | (UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic)
                    | (UpstreamProtocol::Gemini, DownstreamProtocol::Gemini)
            ) {
                decompressed_body.clone()
            } else {
                Bytes::from(
                    serde_json::to_vec(&output.value.value)
                        .expect("serde_json::Value serialization is structurally infallible"),
                )
            };
            Ok((
                body_bytes,
                output.value.usage_info,
                output.value.usage_normalization,
                output.summary,
            ))
        }
        Err(_) if protocols_share_wire_format(upstream_protocol, downstream_protocol) => {
            let mut collector = TransformDiagnosticCollector::default();
            collector.record(TransformDiagnosticFact {
                sequence: 0,
                phase: TransformPhase::ResponseObserve,
                semantic_unit: TransformSemanticUnit::ResponseEnvelope,
                outcome: TransformOutcomeKind::ObservationDegraded,
                action: TransformAction::PassThrough,
                reason_code: TransformReasonCode::ObservationParseFailed,
                safe_summary: Some(TransformSafeSummary::from_bytes(decompressed_body)),
            });
            Ok((
                decompressed_body.clone(),
                None,
                None,
                collector.into_summary(),
            ))
        }
        Err(_) => Err(transform_failure(
            TransformFailureOrigin::UpstreamPayload,
            TransformPhase::ResponseDecode,
            TransformSemanticUnit::ResponseEnvelope,
            TransformReasonCode::SourceDecodeFailed,
            Some(TransformSafeSummary::from_bytes(decompressed_body)),
        )),
    }
}

fn protocols_share_wire_format(
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> bool {
    matches!(
        (upstream_protocol, downstream_protocol),
        (UpstreamProtocol::Openai, DownstreamProtocol::Openai)
            | (UpstreamProtocol::Responses, DownstreamProtocol::Responses)
            | (UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic)
            | (UpstreamProtocol::Gemini, DownstreamProtocol::Gemini)
    )
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

    #[test]
    fn same_wire_observation_preserves_exact_bytes_and_extracts_usage() {
        let body = Bytes::from_static(
            br#"{
  "id":"chatcmpl-observe","object":"chat.completion","created":1,"model":"m",
  "choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],
  "usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}
}"#,
        );

        let (output, usage, normalization, summary) = process_success_response_body(
            &body,
            DownstreamProtocol::Openai,
            UpstreamProtocol::Openai,
        )
        .expect("same-wire observation must not affect success");

        assert_eq!(output, body);
        assert_eq!(usage.expect("usage").total_tokens, 8);
        let normalization = normalization.expect("normalization");
        assert_eq!(normalization.total_input_tokens, 3);
        assert_eq!(normalization.total_output_tokens, 5);
        assert!(
            summary
                .outcome_counts
                .contains_key(&TransformOutcomeKind::Passthrough)
        );
    }

    #[test]
    fn same_wire_observation_failures_preserve_valid_and_invalid_json_bytes() {
        for body in [
            Bytes::from_static(br#"{"choices":"not-an-array"}"#),
            Bytes::from_static(b"{not-json}"),
        ] {
            let (output, usage, normalization, summary) = process_success_response_body(
                &body,
                DownstreamProtocol::Openai,
                UpstreamProtocol::Openai,
            )
            .expect("same-wire observation failure must be non-fatal");

            assert_eq!(output, body);
            assert!(usage.is_none());
            assert!(normalization.is_none());
            assert!(summary.facts.iter().any(|fact| {
                fact.outcome == TransformOutcomeKind::ObservationDegraded
                    && fact.action == TransformAction::PassThrough
                    && fact.reason_code == TransformReasonCode::ObservationParseFailed
            }));
        }
    }

    #[test]
    fn cross_wire_invalid_upstream_body_is_a_typed_failure_without_passthrough() {
        for body in [
            Bytes::from_static(br#"{"choices":"not-an-array"}"#),
            Bytes::from_static(b"upstream-secret-not-json"),
        ] {
            let failure = process_success_response_body(
                &body,
                DownstreamProtocol::Responses,
                UpstreamProtocol::Openai,
            )
            .expect_err("cross-wire malformed response must fail");

            assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
            assert_eq!(failure.phase, TransformPhase::ResponseDecode);
            assert_eq!(failure.reason_code, TransformReasonCode::SourceDecodeFailed);
            assert!(failure.summary.facts[0].safe_summary.is_some());
        }
    }
}
