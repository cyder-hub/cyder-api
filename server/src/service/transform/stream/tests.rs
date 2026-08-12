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

    assert_eq!(output.value.events.len(), 1);
    let payload: Value = serde_json::from_str(&output.value.events[0].data).unwrap();
    assert_eq!(payload["usage"]["prompt_tokens"], 7);
    assert_eq!(payload["usage"]["completion_tokens"], 0);
    assert_eq!(payload["usage"]["total_tokens"], 7);
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
fn test_gemini_streamer_keeps_tool_ids_stable_and_advances_after_finish() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Openai);
    let gemini_tool = "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"location\":\"Boston\"}}}]},\"index\":0}]}";
    let gemini_finish = "{\"candidates\":[{\"index\":0,\"finishReason\":\"STOP\"}]}";

    let first = transformer.transform_event(sse(gemini_tool)).unwrap().value;
    let second = transformer.transform_event(sse(gemini_tool)).unwrap().value;
    let first_json: Value = serde_json::from_str(&first[0].data).unwrap();
    let second_json: Value = serde_json::from_str(&second[0].data).unwrap();

    assert_eq!(
        first_json["choices"][0]["delta"]["tool_calls"][0]["id"],
        second_json["choices"][0]["delta"]["tool_calls"][0]["id"]
    );

    transformer.transform_event(sse(gemini_finish)).unwrap();
    let after_finish = transformer.transform_event(sse(gemini_tool)).unwrap().value;
    let after_finish_json: Value = serde_json::from_str(&after_finish[0].data).unwrap();

    assert_ne!(
        first_json["choices"][0]["delta"]["tool_calls"][0]["id"],
        after_finish_json["choices"][0]["delta"]["tool_calls"][0]["id"]
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
    usage_transformer
        .transform_event(sse(json!({
            "type": "message_start",
            "message": {
                "id": "msg_usage",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-test"
            }
        })
        .to_string()))
        .expect("Anthropic usage lifecycle must start with a message");
    let transformed = usage_transformer
        .transform_event(sse(json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "end_turn",
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 7,
                    "output_tokens": 11
                }
            }
        })
        .to_string()))
        .unwrap()
        .value;

    assert_eq!(transformed.len(), 2);
    assert_eq!(
        usage_transformer.session.finish_reason_cache(),
        Some("stop")
    );
    assert_eq!(
        usage_transformer.cached_usage_info(),
        Some(UsageInfo {
            input_tokens: 7,
            output_tokens: 11,
            total_tokens: 18,
            ..Default::default()
        })
    );
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
                            == Some(&json!("TOOL_USE")))
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
                assert!(!serialized.contains("visible summary"));
                assert!(transformer.diagnostics_snapshot().facts.iter().any(|fact| {
                    fact.semantic_unit == TransformSemanticUnit::ReasoningDelta
                        && fact.outcome == TransformOutcomeKind::ControlledLossMinor
                        && fact.action == TransformAction::Drop
                        && fact.safe_summary.is_none()
                }));
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
fn test_parse_usage_info_fallback_and_cache_miss_diagnostics() {
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

    assert_eq!(
        transformer.parse_usage_info(),
        Some(UsageInfo {
            input_tokens: 3,
            output_tokens: 5,
            total_tokens: 8,
            ..Default::default()
        })
    );

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
