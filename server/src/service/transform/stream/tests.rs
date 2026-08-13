use super::*;
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::service::transform::providers::{anthropic, openai, responses};
use crate::service::transform::unified::*;
use crate::service::transform::{
    TransformAction, TransformFailureOrigin, TransformOutcomeKind, TransformPhase,
    TransformReasonCode, TransformSemanticUnit,
};
use crate::utils::sse::SseEvent;
use crate::utils::usage::UsageInfo;
use serde_json::{Value, json};

const STREAM_DIAGNOSTIC_WINDOW: usize = 32;

fn sse(data: impl Into<String>) -> SseEvent {
    SseEvent {
        data: data.into(),
        ..Default::default()
    }
}

fn responses_response_value(
    status: &str,
    output: Value,
    error: Value,
    incomplete_reason: Option<&str>,
) -> Value {
    json!({
        "id": "resp_portable",
        "object": "response",
        "created_at": 1,
        "completed_at": if status == "completed" { json!(1) } else { Value::Null },
        "status": status,
        "incomplete_details": incomplete_reason.map(|reason| json!({"reason": reason})),
        "model": "responses-model",
        "output": output,
        "error": error,
        "store": false,
        "background": false
    })
}

fn responses_event(event_type: &str, sequence_number: Option<u64>, mut fields: Value) -> SseEvent {
    fields["type"] = json!(event_type);
    if let Some(sequence_number) = sequence_number {
        fields["sequence_number"] = json!(sequence_number);
    }
    sse(fields.to_string())
}

fn anthropic_event(event_type: &str, mut fields: Value) -> SseEvent {
    fields["type"] = json!(event_type);
    SseEvent {
        event: Some(event_type.to_string()),
        data: fields.to_string(),
        ..Default::default()
    }
}

fn anthropic_message_start() -> SseEvent {
    anthropic_event(
        "message_start",
        json!({
            "message": {
                "id": "msg_state_machine",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-test",
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": 3, "output_tokens": 0}
            }
        }),
    )
}

fn assert_anthropic_lifecycle_failure(events: Vec<SseEvent>) {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic);
    let mut events = events;
    let failure_event = events.pop().expect("failure fixture event");
    for event in events {
        transformer
            .transform_event(event)
            .expect("fixture prefix must be a legal Anthropic stream");
    }
    let failure = transformer
        .transform_event(failure_event)
        .expect_err("illegal Anthropic lifecycle must fail");
    assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
    assert_eq!(failure.phase, TransformPhase::StreamDecode);
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::InvalidProtocolShape
    );
}

#[test]
fn meaningful_output_observation_is_shared_by_four_source_protocols_and_targets() {
    let openai_text = "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}";
    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini,
    ] {
        let mut transformer = StreamTransformer::new(UpstreamProtocol::Openai, downstream);
        let output = transformer
            .transform_event_with_observation(sse(openai_text))
            .expect("same-wire observation must succeed");
        assert!(output.value.meaningful_output_observed);
    }

    for (upstream, source_frame) in [
        (UpstreamProtocol::Openai, openai_text),
        (
            UpstreamProtocol::Responses,
            "{\"type\":\"response.content_block.delta\",\"index\":0,\"text\":\"hello\",\"sequence_number\":1}",
        ),
        (
            UpstreamProtocol::Anthropic,
            "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}",
        ),
        (
            UpstreamProtocol::Gemini,
            "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hello\"}]},\"index\":0}]}",
        ),
    ] {
        let mut transformer = StreamTransformer::new(upstream, DownstreamProtocol::Openai);
        if upstream == UpstreamProtocol::Anthropic {
            transformer
                .transform_event(sse("{\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-test\"}}"))
                .expect("Anthropic message lifecycle must start");
            transformer
                .transform_event(sse("{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}"))
                .expect("Anthropic text block lifecycle must start");
        }
        let output = transformer
            .transform_event_with_observation(sse(source_frame))
            .expect("cross-wire observation must succeed");
        assert!(
            output.value.meaningful_output_observed,
            "source protocol {upstream:?} must classify text at the typed source layer"
        );
    }
}

#[test]
fn meaningful_output_observation_ignores_passthrough_lifecycle_and_decode_failure() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Openai);
    let role_only = transformer
        .transform_event_with_observation(sse(
            "{\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}",
        ))
        .expect("same-wire role observation must succeed");
    assert!(!role_only.value.meaningful_output_observed);

    let invalid = transformer
        .transform_event_with_observation(sse("{not-json}"))
        .expect("same-wire observation failure must preserve the event");
    assert!(!invalid.value.meaningful_output_observed);
    assert_eq!(invalid.value.events[0].data, "{not-json}");
    assert_eq!(
        invalid.value.disposition,
        StreamFrameDisposition::ObservationDegraded
    );
    assert_eq!(
        invalid.summary.facts[0].outcome,
        TransformOutcomeKind::ObservationDegraded
    );
}

#[test]
fn same_wire_openai_done_is_a_clean_lifecycle_marker() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Openai);

    let output = transformer
        .transform_event(sse("[DONE]"))
        .expect("same-wire OpenAI completion marker must pass through");

    assert_eq!(output.value.events, vec![sse("[DONE]")]);
    assert_eq!(
        output.value.disposition,
        StreamFrameDisposition::LifecycleSent
    );
    assert!(
        !output
            .summary
            .outcome_counts
            .contains_key(&TransformOutcomeKind::ObservationDegraded)
    );
    assert!(output.summary.facts.iter().any(|fact| {
        fact.semantic_unit == TransformSemanticUnit::Lifecycle
            && fact.reason_code == TransformReasonCode::LosslessConversion
    }));
}

#[test]
fn gemini_usage_without_candidate_tokens_defaults_output_to_zero() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);

    let finish = transformer
        .transform_event(sse(
            json!({"candidates":[{"index":0,"finishReason":"STOP"}]}).to_string(),
        ))
        .expect("formal Gemini terminal");
    assert!(finish.value.events.is_empty());
    let output = transformer
        .transform_event(sse(json!({
            "candidates": [],
            "usageMetadata": {
                "promptTokenCount": 7,
                "totalTokenCount": 7
            }
        })
        .to_string()))
        .expect("Gemini may omit candidate tokens for a zero-output stream frame");

    assert!(output.value.events.is_empty());
    transformer
        .validate_source_termination()
        .expect("STOP plus a usage tail is a complete stream");
    let terminal = transformer
        .finalize_source_eof_events()
        .expect("OpenAI target terminal")
        .value;
    let payload = terminal
        .iter()
        .filter(|event| event.data != "[DONE]")
        .find_map(|event| {
            let value = serde_json::from_str::<Value>(&event.data).unwrap();
            value.get("usage").is_some().then_some(value)
        })
        .expect("terminal usage event");
    assert_eq!(payload["usage"]["prompt_tokens"], 7);
    assert_eq!(payload["usage"]["completion_tokens"], 0);
    assert_eq!(payload["usage"]["total_tokens"], 7);
}

#[test]
fn gemini_stream_usage_uses_last_cumulative_snapshot_with_inclusive_components() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    for usage in [
        json!({
            "promptTokenCount":5,
            "candidatesTokenCount":3,
            "cachedContentTokenCount":1,
            "thoughtsTokenCount":1,
            "totalTokenCount":9
        }),
        json!({
            "promptTokenCount":11,
            "candidatesTokenCount":7,
            "cachedContentTokenCount":3,
            "thoughtsTokenCount":2,
            "toolUsePromptTokenCount":1,
            "totalTokenCount":21,
            "promptTokensDetails":[
                {"modality":"TEXT","tokenCount":8},
                {"modality":"IMAGE","tokenCount":3}
            ],
            "cacheTokensDetails":[{"modality":"IMAGE","tokenCount":3}],
            "candidatesTokensDetails":[
                {"modality":"TEXT","tokenCount":5},
                {"modality":"IMAGE","tokenCount":2}
            ],
            "toolUsePromptTokensDetails":[{"modality":"TEXT","tokenCount":1}]
        }),
    ] {
        transformer
            .transform_event(sse(
                json!({"candidates":[],"usageMetadata":usage}).to_string()
            ))
            .expect("monotonic Gemini usage snapshot should transform");
    }

    assert_eq!(
        transformer.cached_usage_info(),
        Some(UsageInfo {
            input_tokens: 12,
            output_tokens: 9,
            input_image_tokens: 0,
            output_image_tokens: 2,
            cached_tokens: 3,
            cache_write_tokens: 0,
            reasoning_tokens: 2,
            total_tokens: 21,
        })
    );
    let normalization = transformer
        .cached_usage_normalization()
        .expect("final cumulative normalization");
    assert_eq!(normalization.input_text_tokens, 9);
    assert_eq!(normalization.output_text_tokens, 5);
    assert_eq!(normalization.output_image_tokens, 2);
    assert_eq!(normalization.cache_read_tokens, 3);
    assert_eq!(normalization.reasoning_tokens, 2);
    assert_eq!(normalization.normalized_total_tokens(), 21);
}

#[test]
fn gemini_stream_usage_regression_is_cross_wire_failure_and_same_wire_degraded_no_cost() {
    let first = sse(json!({"candidates":[],"usageMetadata":{
        "promptTokenCount":11,"candidatesTokenCount":7,
        "thoughtsTokenCount":2,"toolUsePromptTokenCount":1,
        "totalTokenCount":21
    }})
    .to_string());
    let regressed = sse(json!({"candidates":[],"usageMetadata":{
        "promptTokenCount":11,"candidatesTokenCount":6,
        "thoughtsTokenCount":2,"toolUsePromptTokenCount":1,
        "totalTokenCount":20
    }})
    .to_string());

    let mut cross_wire =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    cross_wire
        .transform_event(first.clone())
        .expect("first usage snapshot");
    let failure = cross_wire
        .transform_event(regressed.clone())
        .expect_err("regressed cross-wire snapshot must fail");
    assert_eq!(failure.semantic_unit, TransformSemanticUnit::Usage);
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::UsageSnapshotRegressed
    );

    let mut same_wire =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Gemini);
    same_wire
        .transform_event(first)
        .expect("first same-wire usage snapshot");
    assert!(same_wire.cached_usage_info().is_some());
    let output = same_wire
        .transform_event(regressed.clone())
        .expect("same-wire regression should preserve raw frame");
    assert_eq!(output.value.events, vec![regressed]);
    assert_eq!(
        output.value.disposition,
        StreamFrameDisposition::ObservationDegraded
    );
    assert!(same_wire.cached_usage_info().is_none());
    assert!(same_wire.cached_usage_normalization().is_none());
}

#[test]
fn gemini_same_wire_terminal_survives_malformed_or_regressed_usage_observation() {
    let malformed_terminal = sse(json!({
        "responseId":"gemini-malformed-terminal-usage",
        "candidates":[{"index":0,"finishReason":"STOP"}],
        "usageMetadata":{"promptTokenCount":"invalid"}
    })
    .to_string());
    let mut malformed =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Gemini);
    let output = malformed
        .transform_event(malformed_terminal.clone())
        .expect("valid terminal core must survive malformed usage");
    assert_eq!(output.value.events, vec![malformed_terminal]);
    assert_eq!(
        output.value.disposition,
        StreamFrameDisposition::ObservationDegraded
    );
    malformed
        .validate_source_termination()
        .expect("malformed usage must not erase the terminal core");
    assert!(!malformed.usage_is_billable());

    let mut regressed =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Gemini);
    regressed
        .transform_event(sse(json!({
            "responseId":"gemini-regressed-terminal-usage",
            "candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"answer"}]}}],
            "usageMetadata":{
                "promptTokenCount":5,"candidatesTokenCount":2,"totalTokenCount":7
            }
        })
        .to_string()))
        .expect("initial Gemini frame");
    let regressed_terminal = sse(json!({
        "responseId":"gemini-regressed-terminal-usage",
        "candidates":[{"index":0,"finishReason":"STOP"}],
        "usageMetadata":{
            "promptTokenCount":5,"candidatesTokenCount":1,"totalTokenCount":6
        }
    })
    .to_string());
    let output = regressed
        .transform_event(regressed_terminal.clone())
        .expect("valid terminal core must survive regressed usage");
    assert_eq!(output.value.events, vec![regressed_terminal]);
    assert_eq!(
        output.value.disposition,
        StreamFrameDisposition::ObservationDegraded
    );
    regressed
        .validate_source_termination()
        .expect("regressed usage must not erase the terminal core");
    assert!(!regressed.usage_is_billable());
}

#[test]
fn gemini_stream_total_mismatch_is_nonfatal_and_auditable() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    transformer
        .transform_event(sse(json!({"candidates":[],"usageMetadata":{
            "promptTokenCount":11,"candidatesTokenCount":7,
            "thoughtsTokenCount":2,"toolUsePromptTokenCount":1,
            "totalTokenCount":999
        }})
        .to_string()))
        .expect("reported total mismatch must not fail stream");

    let usage = transformer.cached_usage_info().expect("usage cache");
    assert_eq!(usage.total_tokens, 999);
    let normalization = transformer
        .cached_usage_normalization()
        .expect("normalization cache");
    assert_eq!(normalization.normalized_total_tokens(), 21);
    assert_eq!(normalization.warnings.len(), 1);
    assert!(normalization.warnings[0].contains("999"));
    assert!(normalization.warnings[0].contains("21"));
}

#[test]
fn gemini_stream_state_machine_accepts_multiframe_thought_tool_finish_and_usage_tail() {
    let frames = vec![
        sse(json!({
            "responseId":"gemini-state-machine",
            "candidates":[{"index":0,"content":{"role":"model","parts":[
                {"text":"consider","thought":true,"thoughtSignature":"private-signature"},
                {"text":"checking"}
            ]}}]
        })
        .to_string()),
        sse(json!({
            "responseId":"gemini-state-machine",
            "candidates":[{"index":0,"content":{"role":"model","parts":[
                {"functionCall":{"name":"lookup","args":{"city":"Paris"}}}
            ]},"finishReason":"STOP"}]
        })
        .to_string()),
        sse(json!({
            "responseId":"gemini-state-machine",
            "usageMetadata":{
                "promptTokenCount":7,
                "candidatesTokenCount":5,
                "thoughtsTokenCount":2,
                "totalTokenCount":14
            }
        })
        .to_string()),
    ];

    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini,
    ] {
        let mut transformer = StreamTransformer::new(UpstreamProtocol::Gemini, downstream);
        let mut emitted = Vec::new();
        for frame in frames.clone() {
            emitted.extend(
                transformer
                    .transform_event(frame)
                    .unwrap_or_else(|failure| panic!("{downstream:?}: {failure:?}"))
                    .value
                    .events,
            );
        }
        assert_eq!(transformer.source_termination(), None, "{downstream:?}");
        transformer
            .validate_source_termination()
            .unwrap_or_else(|failure| panic!("{downstream:?}: {failure:?}"));
        emitted.extend(
            transformer
                .finalize_source_eof_events()
                .expect("legal Gemini EOF target flush")
                .value,
        );
        assert_eq!(
            transformer
                .cached_usage_info()
                .expect("final usage")
                .total_tokens,
            14,
            "{downstream:?}"
        );

        match downstream {
            DownstreamProtocol::Openai => {
                let values = emitted
                    .iter()
                    .filter(|event| event.data != "[DONE]")
                    .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
                    .collect::<Vec<_>>();
                assert_eq!(
                    values
                        .iter()
                        .filter(|value| value.pointer("/choices/0/finish_reason")
                            == Some(&json!("tool_calls")))
                        .count(),
                    1
                );
                assert_eq!(
                    emitted
                        .iter()
                        .filter(|event| event.data == "[DONE]")
                        .count(),
                    1
                );
            }
            DownstreamProtocol::Responses => {
                let types = emitted
                    .iter()
                    .map(|event| {
                        serde_json::from_str::<Value>(&event.data).unwrap()["type"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    types
                        .iter()
                        .filter(|event_type| event_type.as_str() == "response.completed")
                        .count(),
                    1
                );
            }
            DownstreamProtocol::Anthropic => {
                assert_eq!(
                    emitted
                        .iter()
                        .filter(|event| event.event.as_deref() == Some("message_stop"))
                        .count(),
                    1
                );
                assert!(emitted.iter().any(|event| {
                    event.event.as_deref() == Some("message_delta")
                        && event.data.contains("\"stop_reason\":\"tool_use\"")
                }));
            }
            DownstreamProtocol::Gemini => {
                assert_eq!(emitted, frames);
                assert!(emitted.iter().all(|event| event.data != "[DONE]"));
            }
        }
    }
}

#[test]
fn gemini_stream_prompt_block_maps_to_one_formal_target_terminal() {
    let prompt_block = sse(json!({
        "responseId":"gemini-prompt-block",
        "promptFeedback":{"blockReason":"SAFETY","safetyRatings":[]},
        "usageMetadata":{"promptTokenCount":3,"totalTokenCount":3}
    })
    .to_string());

    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini,
    ] {
        let mut transformer = StreamTransformer::new(UpstreamProtocol::Gemini, downstream);
        let mut emitted = transformer
            .transform_event(prompt_block.clone())
            .unwrap_or_else(|failure| panic!("{downstream:?}: {failure:?}"))
            .value
            .events;
        transformer
            .validate_source_termination()
            .expect("prompt block is a formal successful Gemini terminal");
        emitted.extend(
            transformer
                .finalize_source_eof_events()
                .expect("prompt block EOF flush")
                .value,
        );

        match downstream {
            DownstreamProtocol::Openai => assert_eq!(
                emitted
                    .iter()
                    .filter(|event| serde_json::from_str::<Value>(&event.data)
                        .ok()
                        .is_some_and(|value| value.pointer("/choices/0/finish_reason")
                            == Some(&json!("content_filter"))))
                    .count(),
                1
            ),
            DownstreamProtocol::Responses => assert_eq!(
                emitted
                    .iter()
                    .filter(|event| serde_json::from_str::<Value>(&event.data)
                        .ok()
                        .and_then(|value| value["type"].as_str().map(str::to_string))
                        .as_deref()
                        == Some("response.incomplete"))
                    .count(),
                1
            ),
            DownstreamProtocol::Anthropic => {
                assert_eq!(
                    emitted
                        .iter()
                        .filter(|event| event.event.as_deref() == Some("message_start"))
                        .count(),
                    1
                );
                assert_eq!(
                    emitted
                        .iter()
                        .filter(|event| event.event.as_deref() == Some("message_stop"))
                        .count(),
                    1
                );
                assert!(
                    emitted
                        .iter()
                        .any(|event| event.data.contains("\"refusal\""))
                );
            }
            DownstreamProtocol::Gemini => assert_eq!(emitted, vec![prompt_block.clone()]),
        }
    }
}

#[test]
fn gemini_stream_state_machine_rejects_missing_duplicate_and_illegal_sequences() {
    let finish = sse(json!({
        "responseId":"gemini-illegal",
        "candidates":[{"index":0,"finishReason":"STOP"}]
    })
    .to_string());
    let content = sse(json!({
        "responseId":"gemini-illegal",
        "candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"late"}]}}]
    })
    .to_string());

    let mut eof = StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    eof.transform_event(content.clone())
        .expect("legal content prefix");
    assert_eq!(
        eof.validate_source_termination()
            .expect_err("EOF without a formal Gemini terminal")
            .reason_code,
        TransformReasonCode::IllegalUpstreamTerminal
    );

    let mut empty = StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    assert!(
        empty
            .transform_event(sse(""))
            .expect("empty SSE event")
            .value
            .events
            .is_empty()
    );
    assert!(empty.validate_source_termination().is_err());

    for (case_name, prefix, invalid, expected_reason) in [
        (
            "duplicate-terminal",
            Some(finish.clone()),
            finish.clone(),
            TransformReasonCode::InvalidProtocolShape,
        ),
        (
            "post-terminal-content",
            Some(finish.clone()),
            content.clone(),
            TransformReasonCode::InvalidProtocolShape,
        ),
        (
            "multiple-candidates",
            None,
            sse(json!({"candidates":[{"index":0},{"index":1}]}).to_string()),
            TransformReasonCode::UnsupportedContent,
        ),
        (
            "candidate-index",
            None,
            sse(json!({"candidates":[{"index":1,"finishReason":"STOP"}]}).to_string()),
            TransformReasonCode::UnsupportedContent,
        ),
        (
            "unknown-finish",
            None,
            sse(json!({"candidates":[{"index":0,"finishReason":"FUTURE_STOP"}]}).to_string()),
            TransformReasonCode::UnknownStopReason,
        ),
        (
            "unknown-part",
            None,
            sse(json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"futurePart":{"private":true}}]}}]}).to_string()),
            TransformReasonCode::UnknownSemanticUnit,
        ),
        (
            "malformed-json",
            None,
            sse("{not-json}"),
            TransformReasonCode::SourceDecodeFailed,
        ),
        (
            "openai-done-marker",
            None,
            sse("[DONE]"),
            TransformReasonCode::SourceDecodeFailed,
        ),
    ] {
        let mut transformer =
            StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
        if let Some(prefix) = prefix {
            transformer
                .transform_event(prefix)
                .unwrap_or_else(|failure| panic!("{case_name} prefix: {failure:?}"));
        }
        let failure = transformer
            .transform_event(invalid)
            .expect_err("illegal Gemini stream sequence must fail cross-wire");
        assert_eq!(failure.reason_code, expected_reason, "{case_name}");
    }

    let mut identity = StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    identity
        .transform_event(sse(json!({
            "responseId":"one",
            "candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"a"}]}}]
        })
        .to_string()))
        .expect("first response identity");
    let failure = identity
        .transform_event(sse(json!({
            "responseId":"two",
            "candidates":[{"index":0,"finishReason":"STOP"}]
        })
        .to_string()))
        .expect_err("response identity conflict must fail");
    assert_eq!(failure.semantic_unit, TransformSemanticUnit::Lifecycle);
}

#[test]
fn gemini_same_wire_illegal_frames_are_raw_degraded_and_cannot_commit_success() {
    let unknown = sse(json!({
        "candidates":[{"index":0,"content":{"role":"model","parts":[
            {"futurePart":{"private":"marker"}}
        ]}}]
    })
    .to_string());
    let mut same_wire =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Gemini);
    let output = same_wire
        .transform_event(unknown.clone())
        .expect("same-wire unknown Part must preserve the raw frame");
    assert_eq!(output.value.events, vec![unknown]);
    assert_eq!(
        output.value.disposition,
        StreamFrameDisposition::ObservationDegraded
    );
    assert!(!same_wire.usage_is_billable());
    assert!(same_wire.validate_source_termination().is_err());

    let finish = sse(json!({"candidates":[{"index":0,"finishReason":"STOP"}]}).to_string());
    let late = sse(
        json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"late"}]}}]})
            .to_string(),
    );
    let mut post_terminal =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Gemini);
    post_terminal
        .transform_event(finish)
        .expect("formal same-wire terminal");
    let output = post_terminal
        .transform_event(late.clone())
        .expect("same-wire post-terminal content must remain raw");
    assert_eq!(output.value.events, vec![late]);
    assert_eq!(
        output.value.disposition,
        StreamFrameDisposition::ObservationDegraded
    );
    assert!(post_terminal.validate_source_termination().is_err());
    assert!(!post_terminal.usage_is_billable());
}

#[test]
fn gemini_same_wire_tracks_multiple_candidate_terminals_independently() {
    let content = sse(json!({
        "responseId":"gemini-multi-candidate",
        "candidates":[
            {"index":0,"content":{"role":"model","parts":[{"text":"zero"}]}},
            {"index":1,"content":{"role":"model","parts":[{"text":"one"}]}}
        ]
    })
    .to_string());
    let finish_one = sse(json!({
        "responseId":"gemini-multi-candidate",
        "candidates":[{"index":1,"finishReason":"MAX_TOKENS"}]
    })
    .to_string());
    let finish_zero = sse(json!({
        "responseId":"gemini-multi-candidate",
        "candidates":[{"index":0,"finishReason":"STOP"}]
    })
    .to_string());
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Gemini);

    for frame in [&content, &finish_one, &finish_zero] {
        let output = transformer
            .transform_event(frame.clone())
            .expect("same-wire multi-candidate frame must remain raw");
        assert_eq!(output.value.events, vec![frame.clone()]);
    }
    transformer
        .validate_source_termination()
        .expect("all observed Gemini candidate indices have a formal terminal");

    let mut cross_wire =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    let failure = cross_wire
        .transform_event(content)
        .expect_err("cross-wire Gemini still permits only candidate index zero");
    assert_eq!(failure.reason_code, TransformReasonCode::UnsupportedContent);
}

#[test]
fn gemini_stream_application_failures_are_terminal_without_cross_wire_payload() {
    for failed in [
        sse(json!({
            "responseId":"gemini-app-failed",
            "candidates":[{"index":0,"finishReason":"OTHER"}]
        })
        .to_string()),
        sse(json!({
            "error":{"code":500,"message":"private-upstream-marker"}
        })
        .to_string()),
    ] {
        let mut cross_wire =
            StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
        let failure = cross_wire
            .transform_event(failed.clone())
            .expect_err("Gemini application failure must fail cross-wire");
        assert_eq!(failure.semantic_unit, TransformSemanticUnit::StreamError);
        assert!(!format!("{failure:?}").contains("private-upstream-marker"));

        let mut same_wire =
            StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Gemini);
        let output = same_wire
            .transform_event(failed.clone())
            .expect("same-wire application failure must preserve its frame");
        assert_eq!(output.value.events, vec![failed]);
        assert_eq!(
            same_wire.source_termination(),
            Some(SourceStreamTermination::Failed)
        );
        assert!(!same_wire.usage_is_billable());
    }
}

#[test]
fn gemini_responses_target_flushes_completion_at_eof_when_usage_is_missing() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Responses);
    transformer
        .transform_event(sse(json!({
            "responseId":"gemini-no-usage",
            "candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"ok"}]}}]
        })
        .to_string()))
        .expect("content frame");
    let finish = transformer
        .transform_event(sse(json!({
            "responseId":"gemini-no-usage",
            "candidates":[{"index":0,"finishReason":"STOP"}]
        })
        .to_string()))
        .expect("finish without usage");
    assert!(finish.value.events.is_empty());
    transformer
        .validate_source_termination()
        .expect("STOP is a formal terminal");
    let eof = transformer
        .finalize_source_eof_events()
        .expect("Responses EOF completion flush")
        .value;
    assert_eq!(
        eof.iter()
            .filter(|event| serde_json::from_str::<Value>(&event.data)
                .ok()
                .and_then(|value| value["type"].as_str().map(str::to_string))
                .as_deref()
                == Some("response.completed"))
            .count(),
        1
    );
}

fn load_sse_fixture(raw: &str) -> Vec<SseEvent> {
    serde_json::from_str(raw).expect("valid SSE fixture")
}

fn replay_fixture_through_transformer(
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
    fixture: &[SseEvent],
) -> Vec<SseEvent> {
    let mut transformer = StreamTransformer::new(upstream_protocol, downstream_protocol);
    fixture
        .iter()
        .flat_map(|event| {
            transformer
                .transform_event(event.clone())
                .expect("fixture event transform must succeed")
                .value
        })
        .collect()
}

#[test]
fn test_openai_chunk_to_gemini_streamer_preserves_supported_events() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini);

    let transformed = transformer
        .transform_event(sse(
            "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}",
        ))
        .unwrap()
        .value;
    assert_eq!(transformed.len(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&transformed[0].data).unwrap(),
        json!({
            "candidates": [{
                "index": 0,
                "content": {
                    "parts": [{"text": "Hello"}],
                    "role": "model"
                }
            }]
        })
    );

    let transformed_finish = transformer
        .transform_event(sse(
            "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}",
        ))
        .unwrap()
        .value;
    let finish_payload: Value = serde_json::from_str(&transformed_finish[0].data).unwrap();
    assert_eq!(finish_payload["candidates"][0]["finishReason"], "STOP");

    assert!(
        transformer
            .transform_event(sse("[DONE]"))
            .expect("done marker must be explicitly accounted")
            .value
            .is_empty()
    );

    let transformed_tool_start = transformer
        .transform_event(sse(
            "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_123\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"location\\\": \\\"Boston\\\"}\"}}]}}]}",
        ))
        .unwrap()
        .value;
    assert!(transformed_tool_start.is_empty());
    let transformed_tool = transformer
        .transform_event(sse(
            "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}"
        ))
        .unwrap()
        .value;
    assert_eq!(
        serde_json::from_str::<Value>(&transformed_tool[0].data).unwrap(),
        json!({
            "candidates": [{
                "index": 0,
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "id": "call_123",
                            "name": "get_weather",
                            "args": {"location": "Boston"}
                        }
                    }]
                }
            }]
        })
    );

    assert!(
        transformer
            .transform_event(sse(
                "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"}}]}"
            ))
            .expect("empty semantic frame must be explicitly accounted")
            .value
            .is_empty()
    );
}

#[test]
fn test_gemini_streamer_keeps_tool_ids_stable_for_one_response_id() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    let gemini_tool = "{\"responseId\":\"gemini-response-1\",\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"location\":\"Boston\"}}}]},\"index\":0}]}";
    let gemini_finish = "{\"responseId\":\"gemini-response-1\",\"candidates\":[{\"index\":0,\"finishReason\":\"STOP\"}]}";

    let first = transformer.transform_event(sse(gemini_tool)).unwrap().value;
    let second = transformer.transform_event(sse(gemini_tool)).unwrap().value;
    let first_json: Value = serde_json::from_str(&first[0].data).unwrap();
    let second_json: Value = serde_json::from_str(&second[0].data).unwrap();

    assert_eq!(
        first_json["choices"][0]["delta"]["tool_calls"][0]["id"],
        second_json["choices"][0]["delta"]["tool_calls"][0]["id"]
    );

    assert!(
        transformer
            .transform_event(sse(gemini_finish))
            .unwrap()
            .value
            .is_empty()
    );
    let terminal = transformer.finalize_source_eof_events().unwrap().value;
    assert_eq!(terminal.len(), 2);
    let terminal_json: Value = serde_json::from_str(&terminal[0].data).unwrap();
    assert_eq!(terminal_json["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(terminal[1].data, "[DONE]");

    let failure = transformer
        .transform_event(sse(gemini_tool))
        .expect_err("Gemini content after a formal finish must fail closed");
    assert_eq!(failure.semantic_unit, TransformSemanticUnit::Lifecycle);
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::InvalidProtocolShape
    );
}

#[test]
fn test_gemini_openai_done_to_anthropic_emits_terminal_lifecycle() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Anthropic);

    let transformed_content = transformer
        .transform_event(sse(
            "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gemini-2.5-flash-lite\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"}}]}",
        ))
        .unwrap()
        .value;
    assert_eq!(transformed_content.len(), 3);
    assert_eq!(
        transformed_content[0].event.as_deref(),
        Some("message_start")
    );
    assert_eq!(
        transformed_content[1].event.as_deref(),
        Some("content_block_start")
    );
    assert_eq!(
        transformed_content[2].event.as_deref(),
        Some("content_block_delta")
    );

    let transformed_done = transformer.transform_event(sse("[DONE]")).unwrap().value;
    assert_eq!(transformed_done.len(), 2);
    assert_eq!(
        transformed_done[0].event.as_deref(),
        Some("content_block_stop")
    );
    assert_eq!(transformed_done[1].event.as_deref(), Some("message_stop"));
    assert_eq!(transformed_done[1].data, "{\"type\":\"message_stop\"}");
}

#[test]
fn test_stream_session_records_usage_finish_and_bounded_windows() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini);

    for index in 0..40 {
        transformer.transform_event(sse(format!(
            "{{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{}\"}}}}]}}",
            index
        ))).expect("bounded window fixture must transform");
    }

    assert_eq!(
        transformer.session.original_events_len(),
        STREAM_DIAGNOSTIC_WINDOW
    );
    assert_eq!(
        transformer.session.transformed_events_len(),
        STREAM_DIAGNOSTIC_WINDOW
    );
    assert!(
        transformer
            .session
            .original_events_front()
            .unwrap()
            .data
            .contains("\"8\"")
    );

    let mut usage_transformer =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Openai);
    let start = usage_transformer
        .transform_event(sse(json!({
            "type": "message_start",
            "message": {
                "id": "msg_usage",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-test",
                "usage": {
                    "input_tokens": 11,
                    "output_tokens": 0,
                    "cache_read_input_tokens": 3,
                    "cache_creation_input_tokens": 2
                }
            }
        })
        .to_string()))
        .expect("Anthropic usage lifecycle must start with a message")
        .value;
    assert_eq!(start.len(), 1);
    assert!(
        serde_json::from_str::<Value>(&start[0].data)
            .expect("OpenAI start chunk")
            .get("usage")
            .is_none()
    );
    let transformed = usage_transformer
        .transform_event(sse(json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "end_turn",
                "stop_sequence": null,
                "usage": {
                    "output_tokens": 7
                }
            }
        })
        .to_string()))
        .unwrap()
        .value;

    assert_eq!(transformed.len(), 2);
    let usage_frames = transformed
        .iter()
        .filter_map(|event| {
            let value = serde_json::from_str::<Value>(&event.data).ok()?;
            value.get("usage").cloned()
        })
        .collect::<Vec<_>>();
    assert_eq!(usage_frames.len(), 1);
    assert_eq!(usage_frames[0]["prompt_tokens"], 16);
    assert_eq!(usage_frames[0]["completion_tokens"], 7);
    assert_eq!(usage_frames[0]["total_tokens"], 23);
    assert_eq!(
        usage_transformer.session.finish_reason_cache(),
        Some("stop")
    );
    assert_eq!(
        usage_transformer.cached_usage_info(),
        Some(UsageInfo {
            input_tokens: 16,
            output_tokens: 7,
            cached_tokens: 3,
            cache_write_tokens: 2,
            total_tokens: 23,
            ..Default::default()
        })
    );
    let normalization = usage_transformer
        .cached_usage_normalization()
        .expect("Anthropic fieldwise usage normalization");
    assert_eq!(normalization.input_text_tokens, 11);
    assert_eq!(normalization.cache_read_tokens, 3);
    assert_eq!(normalization.cache_write_tokens, 2);
    assert_eq!(
        usage_transformer.parse_usage_info(),
        usage_transformer.cached_usage_info()
    );
}

#[test]
fn test_anthropic_stream_event_bridge_matches_legacy_text_delta_output() {
    let raw_event = anthropic::AnthropicEvent::ContentBlockDelta {
        index: 0,
        delta: anthropic::AnthropicContentDelta::TextDelta {
            text: "Hello".to_string(),
        },
    };
    let legacy_chunk: UnifiedChunkResponse = raw_event.into();
    let legacy_openai =
        serde_json::to_value(openai::OpenAiChunkResponse::from(legacy_chunk)).unwrap();

    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Openai);
    transformer
        .transform_event(sse(json!({
            "type": "message_start",
            "message": {
                "id": "msg_bridge",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-test"
            }
        })
        .to_string()))
        .unwrap();
    transformer
        .transform_event(sse(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        })
        .to_string()))
        .unwrap();
    let transformed = transformer
        .transform_event(sse(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"}
        })
        .to_string()))
        .unwrap()
        .value;

    assert_eq!(transformed.len(), 1);
    let bridged_openai: Value = serde_json::from_str(&transformed[0].data).unwrap();
    assert_eq!(bridged_openai["choices"], legacy_openai["choices"]);
}

#[test]
fn anthropic_core_state_machine_accepts_text_thinking_tool_ping_and_one_terminal() {
    let events = vec![
        anthropic_message_start(),
        anthropic_event("ping", json!({})),
        anthropic_event(
            "content_block_start",
            json!({"index": 0, "content_block": {"type": "text", "text": ""}}),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 0, "delta": {"type": "text_delta", "text": "hello"}}),
        ),
        anthropic_event("content_block_stop", json!({"index": 0})),
        anthropic_event(
            "content_block_start",
            json!({
                "index": 1,
                "content_block": {"type": "thinking", "thinking": "", "signature": null}
            }),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 1, "delta": {"type": "thinking_delta", "thinking": "consider"}}),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 1, "delta": {"type": "signature_delta", "signature": "sig_1"}}),
        ),
        anthropic_event("content_block_stop", json!({"index": 1})),
        anthropic_event(
            "content_block_start",
            json!({
                "index": 2,
                "content_block": {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {}}
            }),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"city\":"}}),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 2, "delta": {"type": "input_json_delta", "partial_json": "\"Paris\"}"}}),
        ),
        anthropic_event("content_block_stop", json!({"index": 2})),
        anthropic_event(
            "message_delta",
            json!({
                "delta": {"stop_reason": "tool_use", "stop_sequence": null},
                "usage": {"output_tokens": 7}
            }),
        ),
        anthropic_event("message_stop", json!({})),
    ];

    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic);
    for event in events {
        let expected = event.clone();
        let output = transformer
            .transform_event(event)
            .expect("legal Anthropic sequence");
        assert_eq!(output.value.events, vec![expected]);
    }

    assert_eq!(
        transformer.source_termination(),
        Some(SourceStreamTermination::Succeeded)
    );
    transformer
        .validate_source_termination()
        .expect("message_stop completes the source stream");
}

#[test]
fn anthropic_thinking_stream_preserves_signature_only_same_wire() {
    const THINKING: &str = "private-stream-reasoning-marker";
    const SIGNATURE: &str = "private-stream-signature-marker";
    let events = vec![
        anthropic_message_start(),
        anthropic_event(
            "content_block_start",
            json!({
                "index": 0,
                "content_block": {"type": "thinking", "thinking": "", "signature": null}
            }),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 0, "delta": {"type": "thinking_delta", "thinking": THINKING}}),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 0, "delta": {"type": "signature_delta", "signature": SIGNATURE}}),
        ),
        anthropic_event("content_block_stop", json!({"index": 0})),
        anthropic_event(
            "message_delta",
            json!({
                "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                "usage": {"output_tokens": 7}
            }),
        ),
        anthropic_event("message_stop", json!({})),
    ];

    let mut same_wire =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic);
    let mut same_wire_output = String::new();
    for event in events.clone() {
        for output in same_wire
            .transform_event(event)
            .expect("same-wire thinking stream")
            .value
            .events
        {
            same_wire_output.push_str(&output.data);
        }
    }
    assert!(same_wire_output.contains(THINKING));
    assert!(same_wire_output.contains(SIGNATURE));
    same_wire
        .validate_source_termination()
        .expect("same-wire thinking stream terminal");

    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        DownstreamProtocol::Gemini,
    ] {
        let mut transformer = StreamTransformer::new(UpstreamProtocol::Anthropic, downstream);
        let mut output_body = String::new();
        let mut saw_signature_loss = false;
        for event in events.clone() {
            let output = transformer
                .transform_event(event)
                .expect("portable thinking stream must transform");
            output
                .summary
                .facts
                .iter()
                .filter(|fact| {
                    fact.semantic_unit == TransformSemanticUnit::ReasoningContent
                        && fact.outcome == TransformOutcomeKind::ControlledLossMinor
                        && fact.action == TransformAction::Drop
                        && fact.reason_code == TransformReasonCode::UnsupportedReasoning
                })
                .for_each(|fact| {
                    assert!(fact.safe_summary.is_none());
                    saw_signature_loss = true;
                });
            for event in output.value.events {
                output_body.push_str(&event.data);
            }
        }
        assert!(!output_body.contains(SIGNATURE), "{downstream:?}");
        assert!(output_body.contains(THINKING), "{downstream:?}");
        assert!(saw_signature_loss, "{downstream:?}");
        transformer
            .validate_source_termination()
            .expect("portable thinking stream terminal");
    }
}

#[test]
fn anthropic_redacted_and_unknown_thinking_stream_blocks_are_raw_only_same_wire() {
    const PRIVATE_MARKER: &str = "opaque-stream-reasoning-marker";
    for block in [
        json!({"type":"redacted_thinking","data":PRIVATE_MARKER}),
        json!({"type":"future_reasoning","opaque":PRIVATE_MARKER}),
    ] {
        let event = anthropic_event(
            "content_block_start",
            json!({"index":0,"content_block":block}),
        );
        let mut same_wire =
            StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic);
        same_wire
            .transform_event(anthropic_message_start())
            .expect("same-wire opaque thinking start");
        let output = same_wire
            .transform_event(event.clone())
            .expect("same-wire opaque thinking block remains raw");
        assert_eq!(output.value.events, vec![event.clone()]);
        assert_eq!(
            output.value.disposition,
            StreamFrameDisposition::ObservationDegraded
        );
        assert!(output.value.events[0].data.contains(PRIVATE_MARKER));

        for downstream in [
            DownstreamProtocol::Openai,
            DownstreamProtocol::Responses,
            DownstreamProtocol::Gemini,
        ] {
            let mut transformer = StreamTransformer::new(UpstreamProtocol::Anthropic, downstream);
            transformer
                .transform_event(anthropic_message_start())
                .expect("cross-wire message start");
            let failure = transformer
                .transform_event(event.clone())
                .expect_err("opaque thinking block must fail closed cross-wire");
            assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
            assert_eq!(
                failure.reason_code,
                TransformReasonCode::UnknownSemanticUnit
            );
            assert!(!format!("{failure:?}").contains(PRIVATE_MARKER));
        }
    }
}

#[test]
fn anthropic_core_state_machine_rejects_illegal_order_index_and_block_completion() {
    let text_start = || {
        anthropic_event(
            "content_block_start",
            json!({"index": 0, "content_block": {"type": "text", "text": ""}}),
        )
    };
    let message_delta = || {
        anthropic_event(
            "message_delta",
            json!({
                "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                "usage": {"output_tokens": 1}
            }),
        )
    };

    let fixtures = vec![
        vec![text_start()],
        vec![anthropic_message_start(), anthropic_message_start()],
        vec![
            anthropic_message_start(),
            anthropic_event(
                "content_block_start",
                json!({"index": 1, "content_block": {"type": "text", "text": ""}}),
            ),
        ],
        vec![anthropic_message_start(), text_start(), message_delta()],
        vec![
            anthropic_message_start(),
            anthropic_event("message_stop", json!({})),
        ],
        vec![anthropic_event("ping", json!({}))],
        vec![anthropic_message_start(), message_delta(), message_delta()],
        vec![
            anthropic_message_start(),
            message_delta(),
            anthropic_event("message_stop", json!({})),
            anthropic_event("message_stop", json!({})),
        ],
        vec![
            anthropic_message_start(),
            message_delta(),
            anthropic_event("message_stop", json!({})),
            anthropic_event("ping", json!({})),
        ],
    ];

    for fixture in fixtures {
        assert_anthropic_lifecycle_failure(fixture);
    }
}

#[test]
fn anthropic_core_state_machine_validates_tool_json_and_thinking_signature() {
    assert_anthropic_lifecycle_failure(vec![
        anthropic_message_start(),
        anthropic_event(
            "content_block_start",
            json!({
                "index": 0,
                "content_block": {"type": "tool_use", "id": "toolu_bad", "name": "lookup", "input": {}}
            }),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 0, "delta": {"type": "input_json_delta", "partial_json": "{"}}),
        ),
        anthropic_event("content_block_stop", json!({"index": 0})),
    ]);

    assert_anthropic_lifecycle_failure(vec![
        anthropic_message_start(),
        anthropic_event(
            "content_block_start",
            json!({
                "index": 0,
                "content_block": {"type": "thinking", "thinking": "", "signature": null}
            }),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 0, "delta": {"type": "thinking_delta", "thinking": "draft"}}),
        ),
        anthropic_event("content_block_stop", json!({"index": 0})),
    ]);

    assert_anthropic_lifecycle_failure(vec![
        anthropic_message_start(),
        anthropic_event(
            "content_block_start",
            json!({
                "index": 0,
                "content_block": {"type": "thinking", "thinking": "", "signature": null}
            }),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 0, "delta": {"type": "signature_delta", "signature": "sig_1"}}),
        ),
        anthropic_event(
            "content_block_delta",
            json!({"index": 0, "delta": {"type": "signature_delta", "signature": "sig_2"}}),
        ),
    ]);
}

#[test]
fn anthropic_sse_event_name_must_match_json_type_on_same_and_cross_wire() {
    let mismatched = SseEvent {
        event: Some("content_block_stop".to_string()),
        data: json!({"type": "message_stop"}).to_string(),
        ..Default::default()
    };
    for downstream in [
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        DownstreamProtocol::Gemini,
    ] {
        let failure = StreamTransformer::new(UpstreamProtocol::Anthropic, downstream)
            .transform_event(mismatched.clone())
            .expect_err("mismatched Anthropic SSE name and payload type must fail");
        assert_eq!(
            failure.reason_code,
            TransformReasonCode::StreamEventTypeMismatch
        );
        assert!(
            failure
                .summary
                .facts
                .iter()
                .all(|fact| fact.safe_summary.is_some())
        );
    }
}

#[test]
fn anthropic_unknown_and_bad_json_are_raw_only_on_same_wire() {
    let unknown = anthropic_event("vendor_future_event", json!({"opaque": "preserve"}));
    let bad_json = SseEvent {
        event: Some("message_start".to_string()),
        data: "{not-json}".to_string(),
        ..Default::default()
    };

    for event in [unknown.clone(), bad_json.clone()] {
        let output =
            StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic)
                .transform_event(event.clone())
                .expect("same-wire future or undecodable frame must retain raw compatibility");
        assert_eq!(output.value.events, vec![event]);
        assert_eq!(
            output.value.disposition,
            StreamFrameDisposition::ObservationDegraded
        );
    }

    let unknown_failure =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Openai)
            .transform_event(unknown)
            .expect_err("cross-wire unknown Anthropic event must fail");
    assert_eq!(
        unknown_failure.reason_code,
        TransformReasonCode::UnknownSemanticUnit
    );

    let bad_json_failure =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Openai)
            .transform_event(bad_json)
            .expect_err("cross-wire undecodable Anthropic event must fail");
    assert_eq!(
        bad_json_failure.reason_code,
        TransformReasonCode::SourceDecodeFailed
    );
}

#[test]
fn anthropic_error_is_a_unique_failed_terminal_and_eof_is_not_success() {
    let mut partial =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic);
    partial
        .transform_event(anthropic_message_start())
        .expect("message start");
    let eof_failure = partial
        .validate_source_termination()
        .expect_err("Anthropic EOF without message_stop must fail");
    assert_eq!(
        eof_failure.reason_code,
        TransformReasonCode::IllegalUpstreamTerminal
    );

    let mut errored =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic);
    errored
        .transform_event(anthropic_message_start())
        .expect("message start");
    let error = anthropic_event(
        "error",
        json!({"error": {"type": "overloaded_error", "message": "safe fixture"}}),
    );
    assert_eq!(
        errored
            .transform_event(error.clone())
            .expect("same-wire error must be emitted unchanged")
            .value
            .events,
        vec![error]
    );
    assert_eq!(
        errored.source_termination(),
        Some(SourceStreamTermination::Failed)
    );
    let post_error = errored
        .transform_event(anthropic_event("ping", json!({})))
        .expect_err("events after an Anthropic error terminal must fail");
    assert_eq!(
        post_error.reason_code,
        TransformReasonCode::InvalidProtocolShape
    );
}

#[test]
fn test_responses_source_stream_fast_path_matches_unified_openai_path() {
    let raw = json!({
        "id": "resp_123",
        "model": "gpt-4.1",
        "delta": {
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_123",
            "name": "lookup_weather",
            "arguments": "{\"city\":\"Boston\"}",
            "status": "completed"
        }
    });

    let event = sse(raw.to_string());

    let mut optimized =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
    let optimized_events = optimized.transform_event(event).unwrap().value;

    let parsed: responses::ResponsesChunkResponse = serde_json::from_value(raw).unwrap();
    let stream_events = responses::responses_chunk_to_unified_stream_events(parsed);
    let mut legacy =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
    legacy.update_session_from_stream_events(&stream_events);
    let legacy_events = openai::transform_unified_stream_events_to_openai_events(
        stream_events,
        &mut legacy.stream_context(),
    )
    .unwrap();

    let optimized_values = optimized_events
        .into_iter()
        .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
        .collect::<Vec<_>>();
    let legacy_values = legacy_events
        .into_iter()
        .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(optimized_values, legacy_values);
}

#[test]
fn responses_official_portable_event_families_deserialize_without_catch_all() {
    let in_progress = responses_response_value("in_progress", json!([]), Value::Null, None);
    let completed = responses_response_value("completed", json!([]), Value::Null, None);
    let incomplete = responses_response_value(
        "incomplete",
        json!([]),
        Value::Null,
        Some("max_output_tokens"),
    );
    let failed = responses_response_value(
        "failed",
        json!([]),
        json!({"code": "server_error", "message": "safe fixture"}),
        None,
    );
    let message = json!({
        "type": "message",
        "id": "msg_portable",
        "status": "completed",
        "role": "assistant",
        "content": []
    });
    let events = vec![
        json!({"type":"response.created","response":in_progress}),
        json!({"type":"response.queued","response":responses_response_value("queued", json!([]), Value::Null, None)}),
        json!({"type":"response.in_progress","response":responses_response_value("in_progress", json!([]), Value::Null, None)}),
        json!({"type":"response.completed","response":completed}),
        json!({"type":"response.incomplete","response":incomplete}),
        json!({"type":"response.failed","response":failed}),
        json!({"type":"response.output_item.added","output_index":0,"item":message.clone()}),
        json!({"type":"response.output_item.done","output_index":0,"item":message}),
        json!({"type":"response.content_part.added","item_id":"msg_portable","content_index":0,"part":{"type":"output_text","text":"","annotations":[],"logprobs":[]}}),
        json!({"type":"response.content_part.done","item_id":"msg_portable","content_index":0,"part":{"type":"output_text","text":"ok","annotations":[],"logprobs":[]}}),
        json!({"type":"response.output_text.delta","item_id":"msg_portable","output_index":0,"content_index":0,"delta":"ok"}),
        json!({"type":"response.output_text.done","item_id":"msg_portable","output_index":0,"content_index":0,"text":"ok"}),
        json!({"type":"response.refusal.delta","item_id":"msg_portable","output_index":0,"content_index":1,"delta":"no"}),
        json!({"type":"response.refusal.done","item_id":"msg_portable","output_index":0,"content_index":1,"refusal":"no"}),
        json!({"type":"response.output_text.annotation.added","item_id":"msg_portable","output_index":0,"content_index":0,"annotation":{"type":"url_citation","url":"https://example.test"}}),
        json!({"type":"response.function_call_arguments.delta","item_id":"fc_portable","output_index":1,"delta":"{}"}),
        json!({"type":"response.function_call_arguments.done","item_id":"fc_portable","output_index":1,"call_id":"call_portable","arguments":"{}"}),
        json!({"type":"response.reasoning_summary_part.added","item_id":"rs_portable","summary_index":0,"part":{"type":"summary_text","text":""}}),
        json!({"type":"response.reasoning_summary_text.delta","item_id":"rs_portable","summary_index":0,"delta":"why"}),
        json!({"type":"response.reasoning_summary_text.done","item_id":"rs_portable","summary_index":0,"text":"why"}),
        json!({"type":"response.reasoning_summary_part.done","item_id":"rs_portable","summary_index":0,"part":{"type":"summary_text","text":"why"}}),
        json!({"type":"response.reasoning_text.delta","item_id":"rs_portable","output_index":2,"content_index":0,"delta":"visible"}),
        json!({"type":"response.reasoning_text.done","item_id":"rs_portable","output_index":2,"content_index":0,"text":"visible"}),
        json!({"type":"error","code":"server_error","message":"safe fixture","param":null}),
    ];

    for (sequence, mut event) in events.into_iter().enumerate() {
        event["sequence_number"] = json!(sequence);
        let chunk: responses::ResponsesChunkResponse =
            serde_json::from_value(event).expect("official portable event must deserialize");
        assert_eq!(chunk.sequence_number, Some(sequence as u64));
        assert!(
            !matches!(chunk.event, responses::ResponsesStreamEvent::Unknown(_)),
            "official event must not enter catch-all"
        );
    }
}

#[test]
fn responses_portable_text_refusal_function_reasoning_lifecycle_transforms_once() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
    let message_added = json!({
        "type":"message","id":"msg_portable","status":"in_progress",
        "role":"assistant","content":[]
    });
    let message_done = json!({
        "type":"message","id":"msg_portable","status":"completed",
        "role":"assistant","content":[
            {"type":"output_text","text":"hello","annotations":[],"logprobs":[]},
            {"type":"refusal","refusal":"no"}
        ]
    });
    let function_added = json!({
        "type":"function_call","id":"fc_portable","call_id":"call_portable",
        "name":"lookup","arguments":"","status":"in_progress"
    });
    let function_done = json!({
        "type":"function_call","id":"fc_portable","call_id":"call_portable",
        "name":"lookup","arguments":"{\"x\":1}","status":"completed"
    });
    let reasoning_added = json!({
        "type":"reasoning","id":"rs_portable","content":[],"summary":[],
        "encrypted_content":null
    });
    let reasoning_done = json!({
        "type":"reasoning","id":"rs_portable","content":[],
        "summary":[{"type":"summary_text","text":"why"}],"encrypted_content":null
    });
    let terminal_output = json!([message_done.clone(), function_done.clone()]);
    let mut terminal_response =
        responses_response_value("completed", terminal_output, Value::Null, None);
    terminal_response["usage"] = json!({
        "input_tokens":4,"output_tokens":6,"total_tokens":10,
        "input_tokens_details":{"cached_tokens":1},
        "output_tokens_details":{"reasoning_tokens":2}
    });
    let frames = vec![
        responses_event(
            "response.created",
            Some(1),
            json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)}),
        ),
        responses_event(
            "response.queued",
            Some(2),
            json!({"response":responses_response_value("queued", json!([]), Value::Null, None)}),
        ),
        responses_event(
            "response.in_progress",
            Some(3),
            json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)}),
        ),
        responses_event(
            "response.output_item.added",
            Some(4),
            json!({"output_index":0,"item":message_added}),
        ),
        responses_event(
            "response.content_part.added",
            Some(5),
            json!({"item_id":"msg_portable","content_index":0,"part":{"type":"output_text","text":"","annotations":[],"logprobs":[]}}),
        ),
        responses_event(
            "response.output_text.delta",
            Some(6),
            json!({"item_id":"msg_portable","output_index":0,"content_index":0,"delta":"hello"}),
        ),
        responses_event(
            "response.output_text.done",
            Some(7),
            json!({"item_id":"msg_portable","output_index":0,"content_index":0,"text":"hello"}),
        ),
        responses_event(
            "response.output_text.annotation.added",
            Some(8),
            json!({"item_id":"msg_portable","output_index":0,"content_index":0,"annotation":{"type":"url_citation","url":"https://example.test"}}),
        ),
        responses_event(
            "response.content_part.done",
            Some(9),
            json!({"item_id":"msg_portable","content_index":0,"part":{"type":"output_text","text":"hello","annotations":[],"logprobs":[]}}),
        ),
        responses_event(
            "response.content_part.added",
            Some(10),
            json!({"item_id":"msg_portable","content_index":1,"part":{"type":"refusal","refusal":""}}),
        ),
        responses_event(
            "response.refusal.delta",
            Some(11),
            json!({"item_id":"msg_portable","output_index":0,"content_index":1,"delta":"no"}),
        ),
        responses_event(
            "response.refusal.done",
            Some(12),
            json!({"item_id":"msg_portable","output_index":0,"content_index":1,"refusal":"no"}),
        ),
        responses_event(
            "response.content_part.done",
            Some(13),
            json!({"item_id":"msg_portable","content_index":1,"part":{"type":"refusal","refusal":"no"}}),
        ),
        responses_event(
            "response.output_item.done",
            Some(14),
            json!({"output_index":0,"item":message_done}),
        ),
        responses_event(
            "response.output_item.added",
            Some(15),
            json!({"output_index":1,"item":function_added}),
        ),
        responses_event(
            "response.function_call_arguments.delta",
            Some(16),
            json!({"item_id":"fc_portable","output_index":1,"delta":"{\"x\":"}),
        ),
        responses_event(
            "response.function_call_arguments.delta",
            Some(17),
            json!({"item_id":"fc_portable","output_index":1,"delta":"1}"}),
        ),
        responses_event(
            "response.function_call_arguments.done",
            Some(18),
            json!({"item_id":"fc_portable","output_index":1,"call_id":"call_portable","arguments":"{\"x\":1}"}),
        ),
        responses_event(
            "response.output_item.done",
            Some(19),
            json!({"output_index":1,"item":function_done}),
        ),
        responses_event(
            "response.completed",
            Some(20),
            json!({"response":terminal_response}),
        ),
    ];

    let mut downstream = Vec::new();
    let mut controlled_loss_seen = false;
    for frame in frames {
        let output = transformer
            .transform_event(frame)
            .expect("portable Responses lifecycle should transform");
        controlled_loss_seen |= output.value.disposition == StreamFrameDisposition::ControlledLoss;
        downstream.extend(output.value.events);
    }

    transformer
        .validate_source_termination()
        .expect("completed terminal must close the source stream");
    assert!(
        controlled_loss_seen,
        "annotation must be a typed controlled loss"
    );
    assert_eq!(
        transformer.cached_usage_info().expect("usage").total_tokens,
        10
    );
    let downstream_values = downstream
        .iter()
        .filter(|event| event.data != "[DONE]")
        .map(|event| serde_json::from_str::<Value>(&event.data).expect("OpenAI JSON"))
        .collect::<Vec<_>>();
    assert_eq!(
        downstream
            .iter()
            .filter(|event| event.data == "[DONE]")
            .count(),
        1
    );
    assert!(downstream_values.iter().any(|value| {
        value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            == Some("hello")
    }));
    assert!(downstream_values.iter().any(|value| {
        value
            .pointer("/choices/0/delta/refusal")
            .and_then(Value::as_str)
            == Some("no")
    }));
    assert!(downstream_values.iter().any(|value| {
        value
            .pointer("/choices/0/delta/tool_calls/0/function/arguments")
            .and_then(Value::as_str)
            == Some("{\"x\":")
    }));
    assert_eq!(
        downstream_values
            .iter()
            .filter(|value| value
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
                == Some("tool_calls"))
            .count(),
        1
    );

    let mut reasoning =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Gemini);
    let mut reasoning_terminal = responses_response_value(
        "completed",
        json!([reasoning_done.clone()]),
        Value::Null,
        None,
    );
    reasoning_terminal["usage"] = json!({
        "input_tokens":1,"output_tokens":2,"total_tokens":3,
        "input_tokens_details":{"cached_tokens":0},
        "output_tokens_details":{"reasoning_tokens":2}
    });
    let reasoning_frames = vec![
        responses_event(
            "response.created",
            Some(1),
            json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)}),
        ),
        responses_event(
            "response.output_item.added",
            Some(2),
            json!({"output_index":0,"item":reasoning_added}),
        ),
        responses_event(
            "response.reasoning_summary_part.added",
            Some(3),
            json!({"item_id":"rs_portable","summary_index":0,"part":{"type":"summary_text","text":""}}),
        ),
        responses_event(
            "response.reasoning_summary_text.delta",
            Some(4),
            json!({"item_id":"rs_portable","summary_index":0,"delta":"why"}),
        ),
        responses_event(
            "response.reasoning_summary_text.done",
            Some(5),
            json!({"item_id":"rs_portable","summary_index":0,"text":"why"}),
        ),
        responses_event(
            "response.reasoning_summary_part.done",
            Some(6),
            json!({"item_id":"rs_portable","summary_index":0,"part":{"type":"summary_text","text":"why"}}),
        ),
        responses_event(
            "response.output_item.done",
            Some(7),
            json!({"output_index":0,"item":reasoning_done}),
        ),
        responses_event(
            "response.completed",
            Some(8),
            json!({"response":reasoning_terminal}),
        ),
    ];
    let mut reasoning_values = Vec::new();
    for frame in reasoning_frames {
        let output = reasoning
            .transform_event(frame)
            .expect("portable reasoning lifecycle should transform");
        reasoning_values.extend(
            output
                .value
                .events
                .into_iter()
                .map(|event| serde_json::from_str::<Value>(&event.data).expect("Gemini JSON")),
        );
    }
    reasoning
        .validate_source_termination()
        .expect("reasoning stream terminal");
    assert!(reasoning_values.iter().any(|value| {
        value
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(Value::as_str)
            == Some("why")
            && value
                .pointer("/candidates/0/content/parts/0/thought")
                .and_then(Value::as_bool)
                == Some(true)
    }));
}

#[test]
fn responses_function_arguments_delta_done_and_tool_finish_reach_every_downstream_once() {
    let function_added = json!({
        "type":"function_call","id":"fc_multi","call_id":"call_multi",
        "name":"lookup","arguments":"","status":"in_progress"
    });
    let function_done = json!({
        "type":"function_call","id":"fc_multi","call_id":"call_multi",
        "name":"lookup","arguments":"{\"city\":\"Paris\"}","status":"completed"
    });
    let mut terminal_response = responses_response_value(
        "completed",
        json!([function_done.clone()]),
        Value::Null,
        None,
    );
    terminal_response["usage"] = json!({
        "input_tokens":2,"output_tokens":3,"total_tokens":5,
        "input_tokens_details":{"cached_tokens":0},
        "output_tokens_details":{"reasoning_tokens":0}
    });
    let frames = vec![
        responses_event(
            "response.created",
            Some(1),
            json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)}),
        ),
        responses_event(
            "response.output_item.added",
            Some(2),
            json!({"output_index":0,"item":function_added}),
        ),
        responses_event(
            "response.function_call_arguments.delta",
            Some(3),
            json!({"item_id":"fc_multi","output_index":0,"delta":"{\"city\":"}),
        ),
        responses_event(
            "response.function_call_arguments.delta",
            Some(4),
            json!({"item_id":"fc_multi","output_index":0,"delta":"\"Paris\"}"}),
        ),
        responses_event(
            "response.function_call_arguments.done",
            Some(5),
            json!({"item_id":"fc_multi","output_index":0,"call_id":"call_multi","arguments":"{\"city\":\"Paris\"}"}),
        ),
        responses_event(
            "response.output_item.done",
            Some(6),
            json!({"output_index":0,"item":function_done}),
        ),
        responses_event(
            "response.completed",
            Some(7),
            json!({"response":terminal_response}),
        ),
    ];

    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini,
    ] {
        let mut transformer = StreamTransformer::new(UpstreamProtocol::Responses, downstream);
        let mut emitted = Vec::new();
        for (frame_index, frame) in frames.clone().into_iter().enumerate() {
            emitted.extend(
                transformer
                    .transform_event(frame)
                    .unwrap_or_else(|error| {
                        panic!(
                            "{downstream:?} portable function frame {frame_index} must transform: {error:?}"
                        )
                    })
                    .value
                    .events,
            );
        }
        transformer
            .validate_source_termination()
            .expect("completed function stream must terminate");

        match downstream {
            DownstreamProtocol::Responses => {
                let names = emitted
                    .iter()
                    .map(|event| {
                        serde_json::from_str::<Value>(&event.data)
                            .expect("Responses JSON")
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    names
                        .iter()
                        .filter(|name| name.as_str() == "response.function_call_arguments.delta")
                        .count(),
                    2
                );
                assert_eq!(
                    names
                        .iter()
                        .filter(|name| name.as_str() == "response.function_call_arguments.done")
                        .count(),
                    1
                );
                assert_eq!(names.last().map(String::as_str), Some("response.completed"));
            }
            DownstreamProtocol::Openai => {
                let values = emitted
                    .iter()
                    .filter(|event| event.data != "[DONE]")
                    .map(|event| serde_json::from_str::<Value>(&event.data).expect("OpenAI JSON"))
                    .collect::<Vec<_>>();
                let arguments = values
                    .iter()
                    .filter_map(|value| {
                        value
                            .pointer("/choices/0/delta/tool_calls/0/function/arguments")
                            .and_then(Value::as_str)
                    })
                    .collect::<String>();
                assert_eq!(arguments, "{\"city\":\"Paris\"}");
                assert_eq!(
                    values
                        .iter()
                        .filter(|value| value.pointer("/choices/0/finish_reason")
                            == Some(&json!("tool_calls")))
                        .count(),
                    1
                );
                assert_eq!(
                    emitted
                        .iter()
                        .filter(|event| event.data == "[DONE]")
                        .count(),
                    1
                );
            }
            DownstreamProtocol::Anthropic => {
                let values = emitted
                    .iter()
                    .map(|event| {
                        serde_json::from_str::<Value>(&event.data).expect("Anthropic JSON")
                    })
                    .collect::<Vec<_>>();
                let arguments = values
                    .iter()
                    .filter_map(|value| {
                        value.pointer("/delta/partial_json").and_then(Value::as_str)
                    })
                    .collect::<String>();
                assert_eq!(arguments, "{\"city\":\"Paris\"}");
                assert_eq!(
                    values
                        .iter()
                        .filter(
                            |value| value.pointer("/delta/stop_reason") == Some(&json!("tool_use"))
                        )
                        .count(),
                    1
                );
                assert_eq!(
                    emitted
                        .iter()
                        .filter(|event| event.event.as_deref() == Some("message_stop"))
                        .count(),
                    1
                );
            }
            DownstreamProtocol::Gemini => {
                let values = emitted
                    .iter()
                    .map(|event| serde_json::from_str::<Value>(&event.data).expect("Gemini JSON"))
                    .collect::<Vec<_>>();
                assert!(values.iter().any(|value| {
                    value.pointer("/candidates/0/content/parts/0/functionCall/name")
                        == Some(&json!("lookup"))
                        && value.pointer("/candidates/0/content/parts/0/functionCall/args")
                            == Some(&json!({"city":"Paris"}))
                }));
                assert_eq!(
                    values
                        .iter()
                        .filter(|value| value.pointer("/candidates/0/finishReason")
                            == Some(&json!("STOP")))
                        .count(),
                    1
                );
            }
        }
    }
}

#[test]
fn responses_visible_reasoning_stream_is_native_or_typed_and_never_relabelled_as_openai_text() {
    let reasoning_added = json!({
        "type":"reasoning","id":"rs_visible","content":[],"summary":[],
        "encrypted_content":null
    });
    let reasoning_done = json!({
        "type":"reasoning","id":"rs_visible","content":[],
        "summary":[{"type":"summary_text","text":"visible summary"}],
        "encrypted_content":null
    });
    let mut terminal_response = responses_response_value(
        "completed",
        json!([reasoning_done.clone()]),
        Value::Null,
        None,
    );
    terminal_response["usage"] = json!({
        "input_tokens":1,"output_tokens":2,"total_tokens":3,
        "input_tokens_details":{"cached_tokens":0},
        "output_tokens_details":{"reasoning_tokens":2}
    });
    let frames = vec![
        responses_event(
            "response.created",
            Some(1),
            json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)}),
        ),
        responses_event(
            "response.output_item.added",
            Some(2),
            json!({"output_index":0,"item":reasoning_added}),
        ),
        responses_event(
            "response.reasoning_summary_part.added",
            Some(3),
            json!({"item_id":"rs_visible","summary_index":0,"part":{"type":"summary_text","text":""}}),
        ),
        responses_event(
            "response.reasoning_summary_text.delta",
            Some(4),
            json!({"item_id":"rs_visible","summary_index":0,"delta":"visible summary"}),
        ),
        responses_event(
            "response.reasoning_summary_text.done",
            Some(5),
            json!({"item_id":"rs_visible","summary_index":0,"text":"visible summary"}),
        ),
        responses_event(
            "response.reasoning_summary_part.done",
            Some(6),
            json!({"item_id":"rs_visible","summary_index":0,"part":{"type":"summary_text","text":"visible summary"}}),
        ),
        responses_event(
            "response.output_item.done",
            Some(7),
            json!({"output_index":0,"item":reasoning_done}),
        ),
        responses_event(
            "response.completed",
            Some(8),
            json!({"response":terminal_response}),
        ),
    ];

    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini,
    ] {
        let mut transformer = StreamTransformer::new(UpstreamProtocol::Responses, downstream);
        let mut emitted = Vec::new();
        for frame in frames.clone() {
            emitted.extend(
                transformer
                    .transform_event(frame)
                    .unwrap_or_else(|error| panic!("{downstream:?}: {error:?}"))
                    .value
                    .events,
            );
        }
        transformer
            .validate_source_termination()
            .expect("reasoning stream must terminate");
        let serialized = emitted
            .iter()
            .map(|event| event.data.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        match downstream {
            DownstreamProtocol::Openai => {
                assert!(serialized.contains("visible summary"));
                assert!(serialized.contains("reasoning_content"));
                assert!(
                    !transformer.diagnostics_snapshot().facts.iter().any(|fact| {
                        fact.semantic_unit == TransformSemanticUnit::ReasoningDelta
                            && matches!(
                                fact.outcome,
                                TransformOutcomeKind::ControlledLossMajor
                                    | TransformOutcomeKind::FatalError
                            )
                    })
                );
                assert_eq!(
                    emitted
                        .iter()
                        .filter(|event| event.data == "[DONE]")
                        .count(),
                    1
                );
            }
            DownstreamProtocol::Responses => {
                assert!(serialized.contains("response.reasoning_summary_text.delta"));
                assert!(serialized.contains("visible summary"));
                assert!(
                    !transformer
                        .diagnostics_snapshot()
                        .facts
                        .iter()
                        .any(|fact| matches!(
                            fact.outcome,
                            TransformOutcomeKind::ControlledLossMinor
                                | TransformOutcomeKind::ControlledLossMajor
                        ))
                );
            }
            DownstreamProtocol::Anthropic => {
                assert!(serialized.contains("thinking_delta"));
                assert!(serialized.contains("visible summary"));
            }
            DownstreamProtocol::Gemini => {
                assert!(serialized.contains("visible summary"));
                assert!(serialized.contains("\"thought\":true"));
            }
        }
    }
}

#[test]
fn responses_unknown_hidden_reasoning_event_fails_closed_without_payload_diagnostics() {
    const PRIVATE_MARKER: &str = "encrypted-reasoning-event-private-marker";
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
    transformer
        .transform_event(responses_event(
            "response.created",
            Some(1),
            json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)}),
        ))
        .expect("created event");

    let failure = transformer
        .transform_event(responses_event(
            "response.reasoning.encrypted_content.delta",
            Some(2),
            json!({"item_id":"rs_hidden","delta":PRIVATE_MARKER}),
        ))
        .expect_err("unknown hidden reasoning event must fail closed cross-wire");
    assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
    assert_eq!(
        failure.semantic_unit,
        TransformSemanticUnit::ResponsesUnknownItem
    );
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::UnknownSemanticUnit
    );
    assert!(!format!("{failure:?}").contains(PRIVATE_MARKER));
    assert!(failure.summary.facts.iter().all(|fact| {
        fact.safe_summary
            .as_ref()
            .is_none_or(|summary| !summary.sha256.contains(PRIVATE_MARKER))
    }));
}

#[test]
fn gemini_thought_stream_uses_reasoning_channels_without_text_downgrade() {
    const REASONING: &str = "private-gemini-stream-reasoning-marker";
    const SIGNATURE: &str = "private-gemini-stream-signature-marker";
    let frame = sse(json!({
        "responseId":"gemini-stream-reasoning",
        "candidates":[{"index":0,"content":{"role":"model","parts":[
                {"text":REASONING,"thought":true,"thoughtSignature":SIGNATURE},{"text":"public answer"}
        ]}}]
    })
    .to_string());

    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        DownstreamProtocol::Anthropic,
    ] {
        let mut transformer = StreamTransformer::new(UpstreamProtocol::Gemini, downstream);
        let transformed = transformer
            .transform_event(frame.clone())
            .unwrap_or_else(|failure| panic!("{downstream:?}: {failure:?}"));
        let serialized = transformed
            .value
            .events
            .iter()
            .map(|event| event.data.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(serialized.contains(REASONING), "{downstream:?}");
        assert!(serialized.contains("public answer"), "{downstream:?}");
        assert!(!serialized.contains(SIGNATURE), "{downstream:?}");
        match downstream {
            DownstreamProtocol::Openai => assert!(serialized.contains("reasoning_content")),
            DownstreamProtocol::Responses => assert!(serialized.contains("reasoning")),
            DownstreamProtocol::Anthropic => assert!(serialized.contains("thinking_delta")),
            DownstreamProtocol::Gemini => unreachable!(),
        }
        assert!(!transformed.summary.facts.iter().any(|fact| {
            fact.semantic_unit == TransformSemanticUnit::ReasoningDelta
                && matches!(
                    fact.outcome,
                    TransformOutcomeKind::ControlledLossMajor | TransformOutcomeKind::FatalError
                )
        }));
        assert!(transformed.summary.facts.iter().any(|fact| {
            fact.reason_code == TransformReasonCode::ThoughtSignatureNotPortable
                && fact.safe_summary.is_none()
        }));
    }

    let mut same_wire =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Gemini);
    let passthrough = same_wire
        .transform_event(frame.clone())
        .expect("Gemini thought stream must remain raw same-wire");
    assert_eq!(passthrough.value.events, vec![frame]);
}

#[test]
fn responses_native_media_output_event_fails_closed_cross_wire_without_payload_diagnostics() {
    const PRIVATE_MARKER: &str = "image-output-private-marker";
    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini,
    ] {
        let mut transformer = StreamTransformer::new(UpstreamProtocol::Responses, downstream);
        transformer
            .transform_event(responses_event(
                "response.created",
                Some(1),
                json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)}),
            ))
            .expect("created event");

        let failure = transformer
            .transform_event(responses_event(
                "response.image_generation_call.partial_image",
                Some(2),
                json!({
                    "item_id":"ig_1","output_index":0,"partial_image_index":0,
                    "partial_image_b64":PRIVATE_MARKER
                }),
            ))
            .expect_err("native media output event must fail closed cross-wire");
        assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
        assert_eq!(
            failure.semantic_unit,
            TransformSemanticUnit::ResponsesUnknownItem
        );
        assert_eq!(
            failure.reason_code,
            TransformReasonCode::UnknownSemanticUnit
        );
        assert!(!format!("{failure:?}").contains(PRIVATE_MARKER));
        assert!(failure.summary.facts.iter().all(|fact| {
            fact.safe_summary
                .as_ref()
                .is_none_or(|summary| !summary.sha256.contains(PRIVATE_MARKER))
        }));
    }
}

#[test]
fn responses_stream_sequence_unknown_terminal_and_eof_contracts_fail_closed() {
    let invalid_sequence = json!({
        "type":"response.created",
        "sequence_number":1.5,
        "response":responses_response_value("in_progress", json!([]), Value::Null, None)
    });
    assert!(serde_json::from_value::<responses::ResponsesChunkResponse>(invalid_sequence).is_err());

    let mut ordering =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
    ordering
        .transform_event(responses_event("response.created", Some(5), json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)})))
        .expect("first sequence");
    ordering
        .transform_event(responses_event(
            "response.queued",
            None,
            json!({"response":responses_response_value("queued", json!([]), Value::Null, None)}),
        ))
        .expect("sequence may be omitted");
    let failure = ordering
        .transform_event(responses_event("response.in_progress", Some(5), json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)})))
        .expect_err("duplicate or descending sequence must fail");
    assert_eq!(failure.semantic_unit, TransformSemanticUnit::Lifecycle);

    let mut unknown =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
    let failure = unknown
        .transform_event(responses_event(
            "response.private_tool.delta",
            Some(1),
            json!({"private":"payload-marker"}),
        ))
        .expect_err("unknown tagged event must fail closed");
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::UnknownSemanticUnit
    );

    let mut eof = StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
    eof.transform_event(responses_event(
        "response.created",
        None,
        json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)}),
    ))
    .expect("created event");
    let failure = eof
        .validate_source_termination()
        .expect_err("EOF without terminal must fail closed");
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::IllegalUpstreamTerminal
    );

    let mut terminal =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
    terminal
        .transform_event(responses_event("response.created", Some(1), json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)})))
        .expect("created event");
    terminal
        .transform_event(responses_event(
            "response.completed",
            Some(2),
            json!({"response":responses_response_value("completed", json!([]), Value::Null, None)}),
        ))
        .expect("completed event");
    terminal
        .validate_source_termination()
        .expect("one terminal");
    terminal
        .transform_event(responses_event("response.in_progress", Some(3), json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)})))
        .expect_err("semantic event after terminal must fail closed");

    for terminal_event in [
        responses_event(
            "response.failed",
            Some(2),
            json!({"response":responses_response_value("failed", json!([]), json!({"code":"server_error","message":"private"}), None)}),
        ),
        responses_event(
            "error",
            Some(2),
            json!({"code":"server_error","message":"private","param":null}),
        ),
    ] {
        let mut failed =
            StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
        failed
            .transform_event(responses_event("response.created", Some(1), json!({"response":responses_response_value("in_progress", json!([]), Value::Null, None)})))
            .expect("created event");
        let failure = failed
            .transform_event(terminal_event)
            .expect_err("failed/error terminal must stop cross-wire output");
        assert_eq!(failure.semantic_unit, TransformSemanticUnit::StreamError);
    }
}

#[test]
fn responses_same_wire_preserves_extensions_while_core_usage_and_failures_remain_observable() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Responses);
    let created = responses_event(
        "response.created",
        Some(0),
        json!({
            "response": responses_response_value(
                "in_progress",
                json!([]),
                Value::Null,
                None,
            ),
            "vendor_extension": {"opaque": true},
        }),
    );
    assert_eq!(
        transformer
            .transform_event(created.clone())
            .expect("known created frame")
            .value
            .events,
        vec![created]
    );

    let extension = SseEvent {
        event: Some("response.vendor.extension".to_string()),
        data: "{ \"type\" : \"response.vendor.extension\", \"sequence_number\" : 1, \"private\" : {\"opaque\":true} }".to_string(),
        ..Default::default()
    };
    let extension_output = transformer
        .transform_event(extension.clone())
        .expect("unknown same-wire extension must pass through");
    assert_eq!(extension_output.value.events, vec![extension]);
    assert_eq!(
        extension_output.value.disposition,
        StreamFrameDisposition::ObservationDegraded
    );
    assert!(extension_output.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ObservationDegraded
            && fact.action == TransformAction::PassThrough
            && fact.reason_code == TransformReasonCode::ObservationParseFailed
    }));

    let usage = responses_event(
        "response.usage",
        Some(2),
        json!({"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}),
    );
    assert_eq!(
        transformer
            .transform_event(usage.clone())
            .expect("known usage frame")
            .value
            .events,
        vec![usage]
    );
    assert_eq!(
        transformer.cached_usage_info().expect("observed usage"),
        UsageInfo {
            input_tokens: 1,
            output_tokens: 1,
            total_tokens: 2,
            ..Default::default()
        }
    );

    let failed = responses_event(
        "response.failed",
        Some(3),
        json!({
            "response": responses_response_value(
                "failed",
                json!([{"type":"vendor_future_item","private":true}]),
                json!({"code":"server_error","message":"private"}),
                None,
            ),
            "vendor_extension": {"opaque": true},
        }),
    );
    assert_eq!(
        transformer
            .transform_event(failed.clone())
            .expect("known failed core must not be masked by extensions")
            .value
            .events,
        vec![failed]
    );
    assert_eq!(
        transformer.source_termination(),
        Some(SourceStreamTermination::Failed)
    );
    transformer
        .validate_source_termination()
        .expect("failed is a present source terminal");

    let independent_error = SseEvent {
        event: Some("error".to_string()),
        data: "{ \"type\" : \"error\", \"sequence_number\" : 9, \"code\" : \"server_error\", \"message\" : \"private\", \"param\" : null, \"vendor\" : true }".to_string(),
        ..Default::default()
    };
    let mut error_transformer =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Responses);
    assert_eq!(
        error_transformer
            .transform_event(independent_error.clone())
            .expect("independent error core must remain observable")
            .value
            .events,
        vec![independent_error]
    );
    assert_eq!(
        error_transformer.source_termination(),
        Some(SourceStreamTermination::Failed)
    );
}

#[test]
fn responses_same_wire_degraded_terminal_observation_preserves_core_terminal_and_usage() {
    for (event_type, status, incomplete_reason, add_metadata) in [
        ("response.completed", "completed", None, true),
        (
            "response.incomplete",
            "incomplete",
            Some("future_vendor_limit"),
            false,
        ),
    ] {
        let mut transformer =
            StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Responses);
        transformer
            .transform_event(responses_event(
                "response.created",
                Some(1),
                json!({
                    "response": responses_response_value(
                        "in_progress",
                        json!([]),
                        Value::Null,
                        None,
                    )
                }),
            ))
            .expect("created event");

        let mut response =
            responses_response_value(status, json!([]), Value::Null, incomplete_reason);
        response["usage"] = json!({
            "input_tokens": 11,
            "output_tokens": 7,
            "total_tokens": 18,
            "input_tokens_details": {"cached_tokens": 3},
            "output_tokens_details": {"reasoning_tokens": 2}
        });
        if add_metadata {
            response["metadata"] = json!({"future_vendor_field": true});
        }
        let terminal = responses_event(
            event_type,
            Some(2),
            json!({"response": response, "vendor_extension": true}),
        );

        let output = transformer
            .transform_event(terminal.clone())
            .expect("same-wire terminal observer degradation must preserve the native terminal");
        assert_eq!(output.value.events, vec![terminal]);
        assert_eq!(
            output.value.disposition,
            StreamFrameDisposition::ObservationDegraded
        );
        assert_eq!(
            transformer.source_termination(),
            Some(SourceStreamTermination::Succeeded)
        );
        assert_eq!(
            transformer.cached_usage_info(),
            Some(UsageInfo {
                input_tokens: 11,
                output_tokens: 7,
                total_tokens: 18,
                cached_tokens: 3,
                reasoning_tokens: 2,
                ..Default::default()
            })
        );
        transformer
            .validate_source_termination()
            .expect("degraded observation still has a source terminal");
    }
}

#[test]
fn responses_lifecycle_snapshots_are_pinned_to_created_identity() {
    let lifecycle_cases = [
        ("response.queued", "queued", Value::Null, None),
        ("response.in_progress", "in_progress", Value::Null, None),
        ("response.completed", "completed", Value::Null, None),
        (
            "response.incomplete",
            "incomplete",
            Value::Null,
            Some("max_output_tokens"),
        ),
        (
            "response.failed",
            "failed",
            json!({"code":"server_error","message":"private"}),
            None,
        ),
    ];

    for downstream in [DownstreamProtocol::Responses, DownstreamProtocol::Openai] {
        for (event_type, status, error, incomplete_reason) in &lifecycle_cases {
            for mismatched_field in ["id", "model"] {
                let mut transformer =
                    StreamTransformer::new(UpstreamProtocol::Responses, downstream);
                transformer
                    .transform_event(responses_event(
                        "response.created",
                        Some(1),
                        json!({
                            "response": responses_response_value(
                                "in_progress",
                                json!([]),
                                Value::Null,
                                None,
                            )
                        }),
                    ))
                    .expect("created event");

                let mut response =
                    responses_response_value(status, json!([]), error.clone(), *incomplete_reason);
                response[mismatched_field] = json!(if mismatched_field == "id" {
                    "resp_other"
                } else {
                    "responses-model-other"
                });
                let failure = transformer
                    .transform_event(responses_event(
                        event_type,
                        Some(2),
                        json!({"response": response}),
                    ))
                    .expect_err("later lifecycle identity must match response.created");
                assert_eq!(
                    failure.semantic_unit,
                    TransformSemanticUnit::Lifecycle,
                    "{downstream:?} {event_type} mismatched {mismatched_field}"
                );
            }
        }
    }
}

#[test]
fn test_stream_transformer_deserialize_failure_is_typed_and_payload_free() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini);

    let failure = transformer
        .transform_event(sse("{not-json}"))
        .expect_err("cross-wire source decode must fail");

    assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
    assert_eq!(failure.phase, TransformPhase::StreamDecode);
    assert_eq!(failure.summary.total_fact_count, 1);
    let fact = &failure.summary.facts[0];
    assert_eq!(fact.phase, TransformPhase::StreamDecode);
    assert_eq!(fact.outcome, TransformOutcomeKind::FatalError);
    assert_eq!(fact.action, TransformAction::Terminate);
    assert_eq!(fact.reason_code, TransformReasonCode::SourceDecodeFailed);
    assert_eq!(fact.safe_summary.as_ref().unwrap().sha256.len(), 64);
    assert_ne!(fact.safe_summary.as_ref().unwrap().sha256, "{not-json}");
}

#[test]
fn stream_batch_accounts_every_input_without_defaulting_failures() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini);
    let output = transformer
        .transform_events(vec![
            sse(""),
            sse("{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}"),
            sse("[DONE]"),
        ])
        .expect("valid batch must transform");

    assert_eq!(output.value.input_event_count, 3);
    assert_eq!(output.value.accounted_input_count, 3);
    assert_eq!(
        output.value.accounted_input_count,
        output.value.disposition_counts.values().sum::<usize>()
    );
    assert_eq!(
        output.value.disposition_counts[&StreamFrameDisposition::EmptyFrame],
        1
    );
    assert_eq!(
        output.value.disposition_counts[&StreamFrameDisposition::Sent],
        1
    );
    assert_eq!(
        output.value.disposition_counts[&StreamFrameDisposition::LifecycleNoOutput],
        1
    );
    assert!(!output.value.events.is_empty());
}

#[test]
fn fatal_stream_failure_rolls_back_session_and_refuses_follow_up_frames() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini);
    let first = transformer
        .transform_event(sse("{not-json}"))
        .expect_err("invalid source frame must fail");

    assert!(transformer.session.original_events_is_empty());
    assert!(transformer.cached_usage_info().is_none());
    assert!(transformer.session.finish_reason_cache().is_none());

    let second = transformer
        .transform_event(sse(
            "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"must-not-send\"}}]}",
        ))
        .expect_err("terminal transformer must reject follow-up frames");
    assert_eq!(second.origin, first.origin);
    assert_eq!(second.reason_code, first.reason_code);
    assert!(transformer.session.original_events_is_empty());
    assert!(transformer.cached_usage_info().is_none());
}

#[test]
fn semantic_stream_failure_rolls_back_partially_inferred_tool_state() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini);
    let failure = transformer
        .transform_event(sse(
            r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"city\":"}}]},"finish_reason":"tool_calls"}]}"#,
        ))
        .expect_err("malformed terminal tool arguments must reject");

    assert_eq!(failure.semantic_unit, TransformSemanticUnit::ToolCallDelta);
    assert!(transformer.session.original_events_is_empty());
    assert!(transformer.session.openai_source_tool_calls().is_empty());
}

#[test]
fn tool_argument_delta_accumulation_is_bounded() {
    let oversized_arguments = "x".repeat(super::session::MAX_STREAM_TOOL_ARGUMENT_BYTES + 1);
    let raw = json!({
        "id": "c",
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "m",
        "choices": [{
            "index": 0,
            "delta": {"tool_calls": [{
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": {"name": "lookup", "arguments": oversized_arguments}
            }]}
        }]
    });
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini);
    let failure = transformer
        .transform_event(sse(raw.to_string()))
        .expect_err("oversized cumulative tool arguments must reject");

    assert_eq!(failure.semantic_unit, TransformSemanticUnit::ToolCallDelta);
    assert!(transformer.session.openai_source_tool_calls().is_empty());
}

#[test]
fn same_wire_observation_failure_rolls_back_inference_but_preserves_frame() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Openai);
    let raw = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"city\":"}}]},"finish_reason":"tool_calls"}]}"#;
    let output = transformer
        .transform_event(sse(raw))
        .expect("same-wire observation failure must pass the original frame through");

    assert_eq!(output.value.events[0].data, raw);
    assert_eq!(
        output.value.disposition,
        StreamFrameDisposition::ObservationDegraded
    );
    assert!(transformer.session.openai_source_tool_calls().is_empty());
    assert_eq!(transformer.session.original_events().len(), 1);
}

#[test]
fn disposition_classification_separates_controlled_loss_from_no_output() {
    let mut collector = crate::service::transform::TransformDiagnosticCollector::default();
    collector.record(crate::service::transform::TransformDiagnosticFact {
        sequence: 0,
        phase: TransformPhase::StreamEncode,
        semantic_unit: TransformSemanticUnit::ImageDelta,
        outcome: TransformOutcomeKind::ControlledLossMinor,
        action: TransformAction::Drop,
        reason_code: TransformReasonCode::UnsupportedImageDelta,
        safe_summary: None,
    });
    assert_eq!(
        super::transformer::classify_stream_disposition(
            false,
            false,
            &[],
            &collector.into_summary(),
        ),
        StreamFrameDisposition::ControlledLoss
    );
}

#[test]
fn gemini_usage_never_uses_permissive_raw_fallback_and_cache_miss_is_diagnostic() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    transformer.session.push_original_event(sse(json!({
        "candidates": [],
        "usageMetadata": {
            "promptTokenCount": 3,
            "candidatesTokenCount": 5,
            "totalTokenCount": 8
        }
    })
    .to_string()));

    assert!(transformer.parse_usage_info().is_none());
    assert_eq!(transformer.session.diagnostics_len(), 1);

    let mut cache_miss =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    assert!(cache_miss.parse_usage_info().is_none());
    assert_eq!(cache_miss.session.diagnostics_len(), 1);
    let diagnostic = cache_miss.session.latest_diagnostic().unwrap();
    assert_eq!(diagnostic.phase, TransformPhase::ResponseObserve);
    assert_eq!(diagnostic.semantic_unit, TransformSemanticUnit::Usage);
    assert_eq!(
        diagnostic.outcome,
        TransformOutcomeKind::ObservationDegraded
    );
    assert_eq!(
        diagnostic.reason_code,
        TransformReasonCode::UpstreamUsageMissing
    );
    assert_eq!(diagnostic.action, TransformAction::PassThrough);
}

#[test]
fn test_update_session_from_item_lifecycle_events_tracks_item_and_part_indices() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
    transformer.update_session_from_stream_events(&[
        UnifiedStreamEvent::ItemAdded {
            item_index: Some(3),
            item_id: Some("msg_1".to_string()),
            item: UnifiedItem::Message(UnifiedMessageItem {
                role: UnifiedRole::Assistant,
                content: Vec::new(),
                annotations: Vec::new(),
            }),
        },
        UnifiedStreamEvent::ContentPartAdded {
            item_index: Some(3),
            item_id: Some("msg_1".to_string()),
            part_index: 2,
            part: None,
        },
        UnifiedStreamEvent::ReasoningSummaryPartAdded {
            item_index: Some(4),
            item_id: Some("rs_1".to_string()),
            part_index: 1,
            part: None,
        },
    ]);

    assert_eq!(transformer.session.current_item_index(), Some(4));
    assert_eq!(transformer.session.current_content_part_index(), Some(2));
    assert_eq!(transformer.session.current_reasoning_part_index(), Some(1));
    assert_eq!(
        transformer.session.tool_call_id("msg_1"),
        Some(&"msg_1".to_string())
    );
}

#[test]
fn test_openai_compatible_deepseek_tool_stream_to_responses_emits_arguments_done() {
    let fixture = load_sse_fixture(include_str!(
        "../testdata/openai_compatible_deepseek_tool_stream.json"
    ));

    let transformed = replay_fixture_through_transformer(
        UpstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        &fixture,
    );

    let arguments_done = transformed.iter().find_map(|event| {
        let value: Value = serde_json::from_str(&event.data).expect("valid responses event");
        (value["type"] == json!("response.function_call_arguments.done")).then_some(value)
    });

    let arguments_done = arguments_done.expect("expected arguments.done event");

    assert_eq!(arguments_done["item_id"], json!("call_compat_1"));
    assert_eq!(arguments_done["output_index"], json!(0));
    assert_eq!(arguments_done["call_id"], json!("call_compat_1"));
    assert_eq!(arguments_done["arguments"], json!("{\"city\":\"Boston\"}"));
}

#[test]
fn test_gemini_openai_text_fixture_to_anthropic_emits_terminal_lifecycle() {
    let fixture = load_sse_fixture(include_str!(
        "../testdata/gemini_openai_text_stream_with_done.json"
    ));

    let transformed = replay_fixture_through_transformer(
        UpstreamProtocol::Openai,
        DownstreamProtocol::Anthropic,
        &fixture,
    );

    assert_eq!(transformed[0].event.as_deref(), Some("message_start"));
    assert_eq!(transformed[1].event.as_deref(), Some("content_block_start"));

    let content_delta_events = transformed
        .iter()
        .filter(|event| event.event.as_deref() == Some("content_block_delta"))
        .collect::<Vec<_>>();
    assert_eq!(content_delta_events.len(), 3);

    let message_delta_index = transformed
        .iter()
        .position(|event| event.event.as_deref() == Some("message_delta"))
        .expect("message_delta");
    let content_block_stop_index = transformed
        .iter()
        .position(|event| event.event.as_deref() == Some("content_block_stop"))
        .expect("content_block_stop");
    let message_stop_index = transformed
        .iter()
        .position(|event| event.event.as_deref() == Some("message_stop"))
        .expect("message_stop");

    assert!(content_block_stop_index < message_delta_index);
    assert!(message_delta_index < message_stop_index);
    assert!(
        transformed[message_delta_index]
            .data
            .contains("\"usage\":{\"input_tokens\":26,\"output_tokens\":34}")
    );
    assert!(
        transformed[message_delta_index]
            .data
            .contains("\"stop_reason\":\"end_turn\"")
    );
    assert_eq!(
        transformed[message_stop_index].data,
        "{\"type\":\"message_stop\"}"
    );
}

#[test]
fn test_anthropic_unsupported_thinking_fixture_yields_typed_failure() {
    let fixture = load_sse_fixture(include_str!(
        "../testdata/anthropic_unsupported_thinking_stream.json"
    ));

    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Responses);
    let failure = transformer
        .transform_event(fixture[0].clone())
        .expect_err("unsupported thinking must fail explicitly");

    assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
    assert_eq!(failure.phase, TransformPhase::StreamDecode);
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::InvalidProtocolShape
    );
    assert_eq!(failure.summary.total_fact_count, 1);
    assert_eq!(
        failure.summary.facts[0].outcome,
        TransformOutcomeKind::FatalError
    );
    assert_eq!(
        failure.summary.facts[0]
            .safe_summary
            .as_ref()
            .expect("safe summary")
            .sha256
            .len(),
        64
    );
}
