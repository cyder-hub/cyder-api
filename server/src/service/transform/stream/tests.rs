use super::*;
use crate::schema::enum_def::LlmApiType;
use crate::service::transform::providers::{anthropic, openai, responses};
use crate::service::transform::unified::*;
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

fn load_sse_fixture(raw: &str) -> Vec<SseEvent> {
    serde_json::from_str(raw).expect("valid SSE fixture")
}

fn replay_fixture_through_transformer(
    source_api: LlmApiType,
    target_api: LlmApiType,
    fixture: &[SseEvent],
) -> Vec<SseEvent> {
    let mut transformer = StreamTransformer::new(source_api, target_api);
    fixture
        .iter()
        .flat_map(|event| {
            transformer
                .transform_event(event.clone())
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn test_openai_chunk_to_gemini_streamer_preserves_supported_events() {
    let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);

    let transformed = transformer
        .transform_event(sse(
            "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}",
        ))
        .unwrap();
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
        .unwrap();
    let finish_payload: Value = serde_json::from_str(&transformed_finish[0].data).unwrap();
    assert_eq!(finish_payload["candidates"][0]["finishReason"], "STOP");

    assert!(transformer.transform_event(sse("[DONE]")).is_none());

    let transformed_tool = transformer
        .transform_event(sse(
            "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_123\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"location\\\": \\\"Boston\\\"}\"}}]}}]}",
        ))
        .unwrap();
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
            .is_none()
    );
}

#[test]
fn test_gemini_streamer_keeps_tool_ids_stable_and_advances_after_finish() {
    let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);
    let gemini_tool = "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"location\":\"Boston\"}}}]},\"index\":0}]}";
    let gemini_finish = "{\"candidates\":[{\"index\":0,\"finishReason\":\"STOP\"}]}";

    let first = transformer.transform_event(sse(gemini_tool)).unwrap();
    let second = transformer.transform_event(sse(gemini_tool)).unwrap();
    let first_json: Value = serde_json::from_str(&first[0].data).unwrap();
    let second_json: Value = serde_json::from_str(&second[0].data).unwrap();

    assert_eq!(
        first_json["choices"][0]["delta"]["tool_calls"][0]["id"],
        second_json["choices"][0]["delta"]["tool_calls"][0]["id"]
    );

    transformer.transform_event(sse(gemini_finish)).unwrap();
    let after_finish = transformer.transform_event(sse(gemini_tool)).unwrap();
    let after_finish_json: Value = serde_json::from_str(&after_finish[0].data).unwrap();

    assert_ne!(
        first_json["choices"][0]["delta"]["tool_calls"][0]["id"],
        after_finish_json["choices"][0]["delta"]["tool_calls"][0]["id"]
    );
}

#[test]
fn test_gemini_openai_done_to_anthropic_emits_terminal_lifecycle() {
    let mut transformer = StreamTransformer::new(LlmApiType::GeminiOpenai, LlmApiType::Anthropic);

    let transformed_content = transformer
        .transform_event(sse(
            "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gemini-2.5-flash-lite\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"}}]}",
        ))
        .unwrap();
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

    let transformed_done = transformer.transform_event(sse("[DONE]")).unwrap();
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
    let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);

    for index in 0..40 {
        let _ = transformer.transform_event(sse(format!(
            "{{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{}\"}}}}]}}",
            index
        )));
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

    let mut usage_transformer = StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Openai);
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
        .unwrap();

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

    let mut transformer = StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Openai);
    let transformed = transformer
        .transform_event(sse(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"}
        })
        .to_string()))
        .unwrap();

    assert_eq!(transformed.len(), 1);
    let bridged_openai: Value = serde_json::from_str(&transformed[0].data).unwrap();
    assert_eq!(bridged_openai["choices"], legacy_openai["choices"]);
}

#[test]
fn test_openai_native_stream_encoder_matches_legacy_bridge_for_supported_events() {
    let events = vec![
        UnifiedStreamEvent::MessageStart {
            id: Some("chatcmpl-native".to_string()),
            model: Some("gpt-test".to_string()),
            role: UnifiedRole::Assistant,
        },
        UnifiedStreamEvent::ContentBlockDelta {
            index: 0,
            item_index: None,
            item_id: None,
            part_index: None,
            text: "Hello".to_string(),
        },
        UnifiedStreamEvent::ToolCallStart {
            index: 0,
            id: "call_123".to_string(),
            name: "lookup".to_string(),
        },
        UnifiedStreamEvent::ToolCallArgumentsDelta {
            index: 0,
            item_index: None,
            item_id: None,
            id: Some("call_123".to_string()),
            name: Some("lookup".to_string()),
            arguments: "{\"city\":\"Boston\"}".to_string(),
        },
        UnifiedStreamEvent::MessageDelta {
            finish_reason: Some("tool_calls".to_string()),
        },
        UnifiedStreamEvent::Usage {
            usage: UnifiedUsage {
                input_tokens: 7,
                output_tokens: 11,
                total_tokens: 18,
                ..Default::default()
            },
        },
    ];

    let mut native_transformer = StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Openai);
    native_transformer.update_session_from_stream_events(&events);
    let native = openai::transform_unified_stream_events_to_openai_events(
        events.clone(),
        &mut native_transformer.stream_context(),
    )
    .unwrap();

    let mut legacy_transformer = StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Openai);
    legacy_transformer.update_session_from_stream_events(&events);
    let legacy = legacy_transformer
        .bridge_stream_events_to_legacy_chunks(events)
        .into_iter()
        .map(|chunk| serde_json::to_value(openai::OpenAiChunkResponse::from(chunk)).unwrap())
        .collect::<Vec<_>>();

    let native_values = native
        .into_iter()
        .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(native_values.len(), legacy.len());
    for (native_value, legacy_value) in native_values.iter().zip(legacy.iter()) {
        assert_eq!(native_value["choices"], legacy_value["choices"]);
        assert_eq!(native_value["usage"], legacy_value["usage"]);
    }
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

    let mut optimized = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Openai);
    let optimized_events = optimized.transform_event(event).unwrap();

    let parsed: responses::ResponsesChunkResponse = serde_json::from_value(raw).unwrap();
    let stream_events = responses::responses_chunk_to_unified_stream_events(parsed);
    let mut legacy = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Openai);
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
fn test_stream_transformer_deserialize_failure_returns_controlled_error_event() {
    let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);

    let transformed = transformer.transform_event(sse("{not-json}")).unwrap();

    assert_eq!(transformed.len(), 1);
    assert_eq!(transformed[0].event.as_deref(), Some("error"));
    let payload: Value = serde_json::from_str(&transformed[0].data).unwrap();
    assert_eq!(payload["type"], "transform_error");
    assert_eq!(payload["diagnostic_kind"], "fatal_transform_error");
    assert_eq!(payload["stage"], "deserialize_source_chunk");
    assert_eq!(payload["provider"], "Openai");
    assert_eq!(payload["target_provider"], "Gemini");
    assert_eq!(payload["loss_level"], "reject");
    assert_eq!(payload["semantic_unit"], "StreamError");
    assert!(
        payload["raw_data_summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("bytes=") && summary.contains("sha256="))
    );
    assert_ne!(payload["raw_data_summary"], "{not-json}");
    assert!(transformer.session.last_error_is_some());
    assert_eq!(transformer.session.diagnostics_len(), 1);
}

#[test]
fn test_parse_usage_info_fallback_and_cache_miss_diagnostics() {
    let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);
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

    let mut cache_miss = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);
    assert!(cache_miss.parse_usage_info().is_none());
    assert_eq!(cache_miss.session.diagnostics_len(), 1);
    let diagnostic = cache_miss.session.latest_diagnostic().unwrap();
    assert_eq!(diagnostic.type_, "transform_diagnostic");
    assert_eq!(
        diagnostic.diagnostic_kind,
        UnifiedTransformDiagnosticKind::CapabilityDowngrade
    );
    assert_eq!(diagnostic.stage.as_deref(), Some("parse_usage_info"));
    assert_eq!(
        diagnostic.loss_level,
        UnifiedTransformDiagnosticLossLevel::LossyMinor
    );
}

#[test]
fn test_update_session_from_item_lifecycle_events_tracks_item_and_part_indices() {
    let mut transformer = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Openai);
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

    let transformed =
        replay_fixture_through_transformer(LlmApiType::Openai, LlmApiType::Responses, &fixture);

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
        LlmApiType::GeminiOpenai,
        LlmApiType::Anthropic,
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
fn test_anthropic_unsupported_thinking_fixture_yields_controlled_error() {
    let fixture = load_sse_fixture(include_str!(
        "../testdata/anthropic_unsupported_thinking_stream.json"
    ));

    let transformed =
        replay_fixture_through_transformer(LlmApiType::Anthropic, LlmApiType::Responses, &fixture);

    assert_eq!(transformed.len(), 1);
    assert_eq!(transformed[0].event.as_deref(), Some("error"));
    let payload: Value = serde_json::from_str(&transformed[0].data).expect("error payload");
    assert_eq!(
        payload.get("type").and_then(Value::as_str),
        Some("transform_error")
    );
    assert_eq!(
        payload.get("stage").and_then(Value::as_str),
        Some("deserialize_source_chunk")
    );
    assert!(payload.get("raw_data_summary").is_some());
}
