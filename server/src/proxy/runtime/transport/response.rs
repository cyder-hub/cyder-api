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
        crate::service::transform::ResponseApplicationOutcome,
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
                output.value.application_outcome,
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
                crate::service::transform::ResponseApplicationOutcome::Success,
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

    fn responses_body(
        status: &str,
        incomplete_reason: Option<&str>,
        usage: Option<Value>,
    ) -> Bytes {
        let mut body = serde_json::json!({
            "id": "resp_terminal",
            "object": "response",
            "created_at": 1,
            "status": status,
            "model": "responses-model",
            "output": [{
                "type": "message",
                "id": "msg_terminal",
                "status": if status == "completed" { "completed" } else { "incomplete" },
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "partial or complete",
                    "annotations": [],
                    "logprobs": []
                }]
            }],
            "error": if status == "failed" {
                serde_json::json!({"code": "application_failed", "message": "private upstream detail"})
            } else {
                Value::Null
            },
            "store": false,
            "background": false
        });
        if let Some(reason) = incomplete_reason {
            body["incomplete_details"] = serde_json::json!({"reason": reason});
        }
        if let Some(usage) = usage {
            body["usage"] = usage;
        }
        Bytes::from(serde_json::to_vec_pretty(&body).expect("fixture serializes"))
    }

    fn anthropic_body(stop_reason: Value, content: Value) -> Bytes {
        Bytes::from(
            serde_json::to_vec_pretty(&serde_json::json!({
                "id": "msg_terminal",
                "type": "message",
                "role": "assistant",
                "content": content,
                "model": "anthropic-model",
                "stop_reason": stop_reason,
                "stop_sequence": null,
                "usage": {"input_tokens": 11, "output_tokens": 7}
            }))
            .expect("Anthropic fixture serializes"),
        )
    }

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

        let (output, usage, normalization, outcome, summary) = process_success_response_body(
            &body,
            DownstreamProtocol::Openai,
            UpstreamProtocol::Openai,
        )
        .expect("same-wire observation must not affect success");

        assert_eq!(output, body);
        assert_eq!(
            outcome,
            crate::service::transform::ResponseApplicationOutcome::Success
        );
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
            let (output, usage, normalization, outcome, summary) = process_success_response_body(
                &body,
                DownstreamProtocol::Openai,
                UpstreamProtocol::Openai,
            )
            .expect("same-wire observation failure must be non-fatal");

            assert_eq!(output, body);
            assert_eq!(
                outcome,
                crate::service::transform::ResponseApplicationOutcome::Success
            );
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

    #[test]
    fn responses_same_wire_observes_terminal_and_usage_without_rewriting_bytes() {
        let usage = serde_json::json!({
            "input_tokens": 11,
            "output_tokens": 7,
            "total_tokens": 999,
            "input_tokens_details": {"cached_tokens": 3},
            "output_tokens_details": {"reasoning_tokens": 2}
        });

        for (status, reason, expected_outcome) in [
            (
                "completed",
                None,
                crate::service::transform::ResponseApplicationOutcome::Success,
            ),
            (
                "incomplete",
                Some("private_same_wire_reason"),
                crate::service::transform::ResponseApplicationOutcome::Success,
            ),
            (
                "failed",
                None,
                crate::service::transform::ResponseApplicationOutcome::Failed,
            ),
        ] {
            let body = responses_body(status, reason, Some(usage.clone()));
            let (output, usage_info, normalization, outcome, _) = process_success_response_body(
                &body,
                DownstreamProtocol::Responses,
                UpstreamProtocol::Responses,
            )
            .expect("known same-wire terminal should be observed");

            assert_eq!(output, body, "{status}");
            assert_eq!(outcome, expected_outcome, "{status}");
            let usage_info = usage_info.expect("targeted usage observation");
            assert_eq!(usage_info.total_tokens, 999);
            assert_eq!(usage_info.cached_tokens, 3);
            assert_eq!(usage_info.reasoning_tokens, 2);
            let normalization = normalization.expect("usage normalization");
            assert_eq!(normalization.normalized_total_tokens(), 18);
            assert_eq!(normalization.cache_read_tokens, 3);
            assert_eq!(normalization.reasoning_tokens, 2);
            assert_eq!(normalization.warnings.len(), 1);
            assert!(normalization.warnings[0].contains("999"));
            assert!(normalization.warnings[0].contains("18"));
        }
    }

    #[test]
    fn responses_same_wire_unknown_output_degrades_observation_without_masking_core_facts() {
        for (status, expected_outcome) in [
            (
                "completed",
                crate::service::transform::ResponseApplicationOutcome::Success,
            ),
            (
                "failed",
                crate::service::transform::ResponseApplicationOutcome::Failed,
            ),
        ] {
            let body = Bytes::from(format!(
                r#"{{
  "id":"resp_extension","object":"response","status":"{status}","model":"responses-model",
  "output":[{{"type":"vendor_future_item","private":{{"opaque":true}}}}],
  "error":{},
  "usage":{{"input_tokens":11,"output_tokens":7,"total_tokens":18}},
  "vendor_extension":{{"spacing":"must remain exact"}}
}}"#,
                if status == "failed" {
                    r#"{"code":"application_failed","message":"private upstream detail"}"#
                } else {
                    "null"
                }
            ));

            let (output, usage, normalization, outcome, summary) = process_success_response_body(
                &body,
                DownstreamProtocol::Responses,
                UpstreamProtocol::Responses,
            )
            .expect("unknown same-wire output item must only degrade observation");

            assert_eq!(output, body, "{status}");
            assert_eq!(outcome, expected_outcome, "{status}");
            assert_eq!(usage.expect("targeted usage").total_tokens, 18);
            assert_eq!(
                normalization
                    .expect("targeted normalization")
                    .normalized_total_tokens(),
                18
            );
            assert!(summary.facts.iter().any(|fact| {
                fact.outcome == TransformOutcomeKind::ObservationDegraded
                    && fact.action == TransformAction::PassThrough
                    && fact.reason_code == TransformReasonCode::ObservationParseFailed
            }));
            assert!(summary.facts.iter().all(|fact| {
                fact.safe_summary
                    .as_ref()
                    .is_none_or(|summary| summary.bytes > 0 && summary.event_count == 0)
            }));
        }
    }

    #[test]
    fn responses_cross_wire_maps_closed_incomplete_reasons_for_each_target() {
        for (source_reason, openai_reason, anthropic_reason, gemini_reason) in [
            ("max_tokens", "length", "max_tokens", "MAX_TOKENS"),
            ("max_output_tokens", "length", "max_tokens", "MAX_TOKENS"),
            ("content_filter", "content_filter", "refusal", "SAFETY"),
        ] {
            let body = responses_body("incomplete", Some(source_reason), None);
            for (protocol, pointer, expected) in [
                (
                    DownstreamProtocol::Openai,
                    "/choices/0/finish_reason",
                    openai_reason,
                ),
                (
                    DownstreamProtocol::Anthropic,
                    "/stop_reason",
                    anthropic_reason,
                ),
                (
                    DownstreamProtocol::Gemini,
                    "/candidates/0/finishReason",
                    gemini_reason,
                ),
            ] {
                let (output, _, _, outcome, _) =
                    process_success_response_body(&body, protocol, UpstreamProtocol::Responses)
                        .expect("known incomplete reason should transform");
                assert_eq!(
                    outcome,
                    crate::service::transform::ResponseApplicationOutcome::Success
                );
                let output: Value = serde_json::from_slice(&output).expect("target JSON");
                assert_eq!(
                    output.pointer(pointer).and_then(Value::as_str),
                    Some(expected),
                    "{source_reason} -> {protocol:?}"
                );
            }
        }
    }

    #[test]
    fn responses_cross_wire_unknown_or_failed_and_illegal_finals_fail_closed() {
        let unknown = responses_body("incomplete", Some("vendor_private_reason"), None);
        let failure = process_success_response_body(
            &unknown,
            DownstreamProtocol::Openai,
            UpstreamProtocol::Responses,
        )
        .expect_err("unknown cross-wire reason must fail closed");
        assert_eq!(
            failure.reason_code,
            TransformReasonCode::UnknownIncompleteReason
        );

        let failed = responses_body("failed", None, None);
        let failure = process_success_response_body(
            &failed,
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Responses,
        )
        .expect_err("failed cross-wire terminal must become a pre-commit failure");
        assert_eq!(
            failure.reason_code,
            TransformReasonCode::UpstreamApplicationFailed
        );

        for status in ["queued", "in_progress", "cancelled"] {
            let body = responses_body(status, None, None);
            let failure = process_success_response_body(
                &body,
                DownstreamProtocol::Responses,
                UpstreamProtocol::Responses,
            )
            .expect_err("non-terminal final body must fail closed");
            assert_eq!(
                failure.reason_code,
                TransformReasonCode::IllegalUpstreamTerminal,
                "{status}"
            );
        }
    }

    #[test]
    fn anthropic_cross_wire_maps_every_closed_non_stream_terminal() {
        for (source_reason, openai_reason, responses_status, responses_reason, gemini_reason) in [
            ("end_turn", "stop", "completed", None, "STOP"),
            ("stop_sequence", "stop", "completed", None, "STOP"),
            ("tool_use", "tool_calls", "completed", None, "TOOL_USE"),
            (
                "max_tokens",
                "length",
                "incomplete",
                Some("max_output_tokens"),
                "MAX_TOKENS",
            ),
            (
                "model_context_window_exceeded",
                "length",
                "incomplete",
                Some("max_output_tokens"),
                "MAX_TOKENS",
            ),
            (
                "refusal",
                "content_filter",
                "incomplete",
                Some("content_filter"),
                "SAFETY",
            ),
        ] {
            let content = if source_reason == "tool_use" {
                serde_json::json!([{
                    "type": "tool_use",
                    "id": "call_1",
                    "name": "lookup",
                    "input": {"q": "safe"}
                }])
            } else {
                serde_json::json!([{"type": "text", "text": "partial or complete"}])
            };
            let body = anthropic_body(serde_json::json!(source_reason), content);

            for (protocol, pointer, expected) in [
                (
                    DownstreamProtocol::Openai,
                    "/choices/0/finish_reason",
                    openai_reason,
                ),
                (DownstreamProtocol::Responses, "/status", responses_status),
                (
                    DownstreamProtocol::Gemini,
                    "/candidates/0/finishReason",
                    gemini_reason,
                ),
            ] {
                let (output, usage, normalization, outcome, _) =
                    process_success_response_body(&body, protocol, UpstreamProtocol::Anthropic)
                        .expect("known Anthropic terminal should transform");
                assert_eq!(
                    outcome,
                    crate::service::transform::ResponseApplicationOutcome::Success
                );
                let output: Value = serde_json::from_slice(&output).expect("target JSON");
                assert_eq!(
                    output.pointer(pointer).and_then(Value::as_str),
                    Some(expected),
                    "{source_reason} -> {protocol:?}"
                );
                assert_eq!(usage.expect("usage").total_tokens, 18);
                assert_eq!(
                    normalization
                        .expect("usage normalization")
                        .normalized_total_tokens(),
                    18
                );
                if protocol == DownstreamProtocol::Responses {
                    assert_eq!(
                        output
                            .pointer("/incomplete_details/reason")
                            .and_then(Value::as_str),
                        responses_reason,
                        "{source_reason} -> Responses incomplete reason"
                    );
                }
            }
        }
    }

    #[test]
    fn anthropic_same_wire_preserves_known_and_degraded_terminal_bytes() {
        let cases = [
            (
                "known",
                anthropic_body(
                    serde_json::json!("end_turn"),
                    serde_json::json!([{"type":"text","text":"ok"}]),
                ),
                false,
            ),
            (
                "pause",
                anthropic_body(
                    serde_json::json!("pause_turn"),
                    serde_json::json!([{"type":"text","text":"paused"}]),
                ),
                true,
            ),
            (
                "unknown-stop",
                anthropic_body(
                    serde_json::json!("private_future_stop"),
                    serde_json::json!([{"type":"text","text":"future"}]),
                ),
                true,
            ),
            (
                "null-stop",
                anthropic_body(
                    Value::Null,
                    serde_json::json!([{"type":"text","text":"partial"}]),
                ),
                true,
            ),
            (
                "unknown-block",
                anthropic_body(
                    serde_json::json!("end_turn"),
                    serde_json::json!([{"type":"vendor_future_block","private":true}]),
                ),
                true,
            ),
            ("bad-json", Bytes::from_static(b"{private-not-json}"), true),
        ];

        for (case, body, degraded) in cases {
            let (output, _, _, outcome, summary) = process_success_response_body(
                &body,
                DownstreamProtocol::Anthropic,
                UpstreamProtocol::Anthropic,
            )
            .expect("same-wire Anthropic body must remain pass-through");

            assert_eq!(output, body, "{case}: exact bytes");
            assert_eq!(
                outcome,
                crate::service::transform::ResponseApplicationOutcome::Success,
                "{case}"
            );
            assert_eq!(
                summary.facts.iter().any(|fact| {
                    fact.outcome == TransformOutcomeKind::ObservationDegraded
                        && fact.action == TransformAction::PassThrough
                        && fact.reason_code == TransformReasonCode::ObservationParseFailed
                }),
                degraded,
                "{case}: observation degradation"
            );
        }
    }

    #[test]
    fn anthropic_cross_wire_rejects_pause_unknown_null_unknown_block_and_bad_json() {
        let cases = [
            (
                "pause",
                anthropic_body(
                    serde_json::json!("pause_turn"),
                    serde_json::json!([{"type":"text","text":"private"}]),
                ),
                TransformReasonCode::IllegalUpstreamTerminal,
            ),
            (
                "unknown-stop",
                anthropic_body(
                    serde_json::json!("private_future_stop"),
                    serde_json::json!([{"type":"text","text":"private"}]),
                ),
                TransformReasonCode::UnknownStopReason,
            ),
            (
                "null-stop",
                anthropic_body(
                    Value::Null,
                    serde_json::json!([{"type":"text","text":"private"}]),
                ),
                TransformReasonCode::IllegalUpstreamTerminal,
            ),
            (
                "unknown-block",
                anthropic_body(
                    serde_json::json!("end_turn"),
                    serde_json::json!([{"type":"vendor_future_block","private":true}]),
                ),
                TransformReasonCode::UnknownSemanticUnit,
            ),
            (
                "bad-json",
                Bytes::from_static(b"{private-not-json}"),
                TransformReasonCode::SourceDecodeFailed,
            ),
        ];

        for (case, body, expected_reason) in cases {
            let failure = process_success_response_body(
                &body,
                DownstreamProtocol::Openai,
                UpstreamProtocol::Anthropic,
            )
            .expect_err("cross-wire Anthropic terminal must fail closed");
            assert_eq!(
                failure.origin,
                TransformFailureOrigin::UpstreamPayload,
                "{case}"
            );
            assert_eq!(failure.reason_code, expected_reason, "{case}");
            assert!(failure.summary.facts.iter().all(|fact| {
                fact.safe_summary
                    .as_ref()
                    .is_none_or(|summary| summary.bytes > 0 && summary.event_count == 0)
            }));
        }
    }

    #[test]
    fn anthropic_usage_overflow_degrades_same_wire_and_fails_cross_wire() {
        let body = Bytes::from(
            serde_json::to_vec_pretty(&serde_json::json!({
                "id": "msg_usage_overflow",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "private"}],
                "model": "anthropic-model",
                "stop_reason": "end_turn",
                "stop_sequence": null,
                "usage": {
                    "input_tokens": u32::MAX,
                    "output_tokens": 1,
                    "cache_read_input_tokens": 1
                }
            }))
            .expect("overflow fixture serializes"),
        );

        let (output, usage, normalization, outcome, summary) = process_success_response_body(
            &body,
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Anthropic,
        )
        .expect("same-wire usage overflow must not rewrite or reject provider bytes");
        assert_eq!(output, body);
        assert!(usage.is_none());
        assert!(normalization.is_none());
        assert_eq!(
            outcome,
            crate::service::transform::ResponseApplicationOutcome::Success
        );
        assert!(summary.facts.iter().any(|fact| {
            fact.outcome == TransformOutcomeKind::ObservationDegraded
                && fact.action == TransformAction::PassThrough
        }));

        let failure = process_success_response_body(
            &body,
            DownstreamProtocol::Openai,
            UpstreamProtocol::Anthropic,
        )
        .expect_err("cross-wire usage overflow must fail closed");
        assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
        assert_eq!(failure.semantic_unit, TransformSemanticUnit::Usage);
        assert_eq!(failure.reason_code, TransformReasonCode::UsageOverflow);
    }
}
