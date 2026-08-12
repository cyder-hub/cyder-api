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
