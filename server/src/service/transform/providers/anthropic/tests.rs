use super::*;
use crate::schema::enum_def::LlmApiType;
use crate::service::transform::{AnthropicSessionState, StreamTransformer, unified::*};
use crate::utils::sse::SseEvent;
use serde_json::{Value, json};

#[test]
fn test_anthropic_request_to_unified() {
    let anthropic_request = AnthropicRequestPayload {
        model: "claude-3-opus-20240229".to_string(),
        system: Some(AnthropicSystemPrompt::String(
            "You are a helpful assistant.".to_string(),
        )),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hello, world!"),
        }],
        max_tokens: 100,
        temperature: Some(0.7),
        top_p: None,
        stop_sequences: None,
        stream: Some(true),
        tools: None,
        metadata: None,
        top_k: None,
    };

    let unified_request: UnifiedRequest = anthropic_request.into();

    assert_eq!(
        unified_request.model,
        Some("claude-3-opus-20240229".to_string())
    );
    assert_eq!(unified_request.messages.len(), 2);
    assert_eq!(unified_request.messages[0].role, UnifiedRole::System);
    assert_eq!(unified_request.messages[0].content.len(), 1);
    assert_eq!(
        unified_request.messages[0].content[0],
        UnifiedContentPart::Text {
            text: "You are a helpful assistant.".to_string()
        }
    );
    assert_eq!(unified_request.messages[1].role, UnifiedRole::User);
    assert_eq!(unified_request.messages[1].content.len(), 1);
    assert_eq!(
        unified_request.messages[1].content[0],
        UnifiedContentPart::Text {
            text: "Hello, world!".to_string()
        }
    );
    assert_eq!(unified_request.max_tokens, Some(100));
    assert_eq!(unified_request.temperature, Some(0.7));
    assert_eq!(unified_request.stream, true);
    assert_eq!(
        unified_request
            .anthropic_extension()
            .and_then(|extension| extension.metadata.clone()),
        None
    );
    assert_eq!(unified_request.top_k(), None);
}

#[test]
fn test_unified_request_to_anthropic() {
    let unified_request = UnifiedRequest {
        model: Some("claude-3-opus-20240229".to_string()),
        messages: vec![
            UnifiedMessage {
                role: UnifiedRole::System,
                content: vec![UnifiedContentPart::Text {
                    text: "You are a helpful assistant.".to_string(),
                }],
            },
            UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text {
                    text: "Hello, world!".to_string(),
                }],
            },
        ],
        max_tokens: Some(100),
        temperature: Some(0.7),
        stream: true,
        ..Default::default()
    };

    let anthropic_request: AnthropicRequestPayload = unified_request.into();

    assert_eq!(anthropic_request.model, "claude-3-opus-20240229");
    match anthropic_request.system {
        Some(AnthropicSystemPrompt::String(s)) => {
            assert_eq!(s, "You are a helpful assistant.");
        }
        _ => panic!("Expected string system prompt"),
    }
    assert_eq!(anthropic_request.messages.len(), 1);
    assert_eq!(anthropic_request.messages[0].role, "user");
    assert_eq!(
        anthropic_request.messages[0].content,
        json!("Hello, world!")
    );
    assert_eq!(anthropic_request.max_tokens, 100);
    assert_eq!(anthropic_request.temperature, Some(0.7));
    assert_eq!(anthropic_request.stream, Some(true));
}

#[test]
fn test_anthropic_request_round_trip_preserves_metadata_and_top_k() {
    let anthropic_request = AnthropicRequestPayload {
        model: "claude-3-opus-20240229".to_string(),
        system: Some(AnthropicSystemPrompt::String(
            "You are a helpful assistant.".to_string(),
        )),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hello, world!"),
        }],
        max_tokens: 100,
        temperature: Some(0.7),
        top_p: Some(0.9),
        stop_sequences: Some(vec!["done".to_string()]),
        stream: Some(true),
        tools: None,
        metadata: Some(json!({
            "trace_id": "trace_123",
            "user_tier": "pro"
        })),
        top_k: Some(32),
    };

    let unified_request: UnifiedRequest = anthropic_request.into();
    assert_eq!(
        unified_request
            .anthropic_extension()
            .and_then(|extension| extension.metadata.clone()),
        Some(json!({
            "trace_id": "trace_123",
            "user_tier": "pro"
        }))
    );
    assert_eq!(unified_request.top_k(), Some(32));

    let round_tripped_request: AnthropicRequestPayload = unified_request.into();

    assert_eq!(
        round_tripped_request.metadata,
        Some(json!({
            "trace_id": "trace_123",
            "user_tier": "pro"
        }))
    );
    assert_eq!(round_tripped_request.top_k, Some(32));
    assert_eq!(round_tripped_request.top_p, Some(0.9));
    assert_eq!(
        round_tripped_request.stop_sequences,
        Some(vec!["done".to_string()])
    );
}

#[test]
fn test_unified_request_to_anthropic_preserves_reasoning_as_text() {
    let unified_request = UnifiedRequest {
        model: Some("claude-3-opus-20240229".to_string()),
        messages: vec![UnifiedMessage {
            role: UnifiedRole::User,
            content: vec![
                UnifiedContentPart::Text {
                    text: "Question".to_string(),
                },
                UnifiedContentPart::Reasoning {
                    text: "chain of thought".to_string(),
                },
            ],
        }],
        max_tokens: Some(100),
        ..Default::default()
    };

    let anthropic_request: AnthropicRequestPayload = unified_request.into();
    assert_eq!(
        anthropic_request.messages[0].content,
        json!([
            {"type": "text", "text": "Question"},
            {"type": "text", "text": "chain of thought"}
        ])
    );
}

#[test]
fn test_unified_request_to_anthropic_preserves_image_file_and_code() {
    let unified_request = UnifiedRequest {
        model: Some("claude-3-opus-20240229".to_string()),
        messages: vec![UnifiedMessage {
            role: UnifiedRole::User,
            content: vec![
                UnifiedContentPart::ImageData {
                    mime_type: "image/png".to_string(),
                    data: "ZmFrZQ==".to_string(),
                },
                UnifiedContentPart::ImageUrl {
                    url: "https://example.com/chart.png".to_string(),
                    detail: Some("high".to_string()),
                },
                UnifiedContentPart::FileUrl {
                    url: "https://files.example.com/report.pdf".to_string(),
                    mime_type: Some("application/pdf".to_string()),
                    filename: None,
                },
                UnifiedContentPart::ExecutableCode {
                    language: "python".to_string(),
                    code: "print(1)".to_string(),
                },
            ],
        }],
        max_tokens: Some(100),
        ..Default::default()
    };

    let anthropic_request: AnthropicRequestPayload = unified_request.into();
    assert_eq!(
        anthropic_request.messages[0].content,
        json!([
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "ZmFrZQ=="
                }
            },
            {
                "type": "text",
                "text": "image_url: https://example.com/chart.png\ndetail: high"
            },
            {
                "type": "text",
                "text": "file_url: https://files.example.com/report.pdf\nmime_type: application/pdf"
            },
            {
                "type": "text",
                "text": "```python\nprint(1)\n```"
            }
        ])
    );
}

#[test]
fn test_anthropic_response_to_unified() {
    let anthropic_response = AnthropicResponse {
        id: "msg_123".to_string(),
        type_: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![AnthropicContentBlock::Text {
            text: "Hello from Anthropic!".to_string(),
        }],
        model: "claude-3-opus-20240229".to_string(),
        stop_reason: Some("end_turn".to_string()),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: 10,
            output_tokens: 20,
        },
    };

    let unified_response: UnifiedResponse = anthropic_response.into();

    assert_eq!(unified_response.id, "msg_123");
    assert_eq!(
        unified_response.model,
        Some("claude-3-opus-20240229".to_string())
    );
    assert_eq!(unified_response.choices.len(), 1);
    let choice = &unified_response.choices[0];
    assert_eq!(choice.message.role, UnifiedRole::Assistant);
    assert_eq!(choice.message.content.len(), 1);
    assert_eq!(
        choice.message.content[0],
        UnifiedContentPart::Text {
            text: "Hello from Anthropic!".to_string()
        }
    );
    assert_eq!(choice.finish_reason, Some("stop".to_string()));
    let usage = unified_response.usage.unwrap();
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 20);
    assert_eq!(usage.total_tokens, 30);
}

#[test]
fn test_unified_response_to_anthropic() {
    let unified_response = UnifiedResponse {
        id: "msg_123".to_string(),
        model: Some("claude-3-opus-20240229".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::Text {
                    text: "Hello from Anthropic!".to_string(),
                }],
                ..Default::default()
            },
            items: Vec::new(),
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: Some(UnifiedUsage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
            ..Default::default()
        }),
        created: Some(12345),
        object: Some("chat.completion".to_string()),
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let anthropic_response: AnthropicResponse = unified_response.into();

    assert_eq!(anthropic_response.id, "msg_123");
    assert_eq!(anthropic_response.model, "claude-3-opus-20240229");
    assert_eq!(anthropic_response.content.len(), 1);
    match &anthropic_response.content[0] {
        AnthropicContentBlock::Text { text } => assert_eq!(text, "Hello from Anthropic!"),
        _ => panic!("Incorrect content block type"),
    }
    assert_eq!(anthropic_response.stop_reason, Some("end_turn".to_string()));
    assert_eq!(anthropic_response.usage.input_tokens, 10);
    assert_eq!(anthropic_response.usage.output_tokens, 20);
}

#[test]
fn test_anthropic_event_to_unified_chunk() {
    // MessageStart event
    let event_start = AnthropicEvent::MessageStart {
        message: AnthropicStreamMessage {
            id: "msg_123".to_string(),
            type_: "message".to_string(),
            role: "assistant".to_string(),
            model: "claude-3".to_string(),
            content: None,
            stop_reason: None,
            stop_sequence: None,
            usage: None,
        },
    };
    let unified_chunk_start: UnifiedChunkResponse = event_start.into();
    assert_eq!(unified_chunk_start.id, "msg_123");
    assert_eq!(unified_chunk_start.model, Some("claude-3".to_string()));
    assert_eq!(
        unified_chunk_start.choices[0].delta.role,
        Some(UnifiedRole::Assistant)
    );
    assert!(unified_chunk_start.choices[0].delta.content.is_empty());

    // ContentBlockDelta event
    let event_delta = AnthropicEvent::ContentBlockDelta {
        index: 0,
        delta: AnthropicContentDelta::TextDelta {
            text: "Hello".to_string(),
        },
    };
    let unified_chunk_delta: UnifiedChunkResponse = event_delta.into();
    assert!(unified_chunk_delta.id.starts_with("chatcmpl-"));
    assert_eq!(unified_chunk_delta.choices[0].delta.content.len(), 1);
    assert_eq!(
        unified_chunk_delta.choices[0].delta.content[0],
        UnifiedContentPartDelta::TextDelta {
            index: 0,
            text: "Hello".to_string()
        }
    );
    assert!(unified_chunk_delta.choices[0].delta.role.is_none());

    // MessageDelta event (finish reason)
    let event_stop = AnthropicEvent::MessageDelta {
        delta: MessageDelta {
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: None,
        },
        usage: Some(AnthropicUsage {
            input_tokens: 0,
            output_tokens: 10,
        }),
    };
    let unified_chunk_stop: UnifiedChunkResponse = event_stop.into();
    assert_eq!(
        unified_chunk_stop.choices[0].finish_reason,
        Some("stop".to_string())
    );
    assert!(unified_chunk_stop.choices[0].delta.content.is_empty());
}

#[test]
fn test_anthropic_event_to_unified_stream_events_preserves_tool_use_lifecycle() {
    let events = anthropic_event_to_unified_stream_events(AnthropicEvent::ContentBlockStart {
        index: 2,
        content_block: AnthropicContentBlock::ToolUse {
            id: "toolu_123".to_string(),
            name: "lookup_weather".to_string(),
            input: json!({"city": "Boston"}),
        },
    });

    assert_eq!(
        events,
        vec![
            UnifiedStreamEvent::ItemAdded {
                item_index: Some(2),
                item_id: Some("toolu_123".to_string()),
                item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                    id: "toolu_123".to_string(),
                    name: "lookup_weather".to_string(),
                    arguments: json!({"city": "Boston"}),
                }),
            },
            UnifiedStreamEvent::ContentBlockStart {
                index: 2,
                kind: UnifiedBlockKind::ToolCall,
            },
            UnifiedStreamEvent::ToolCallStart {
                index: 2,
                id: "toolu_123".to_string(),
                name: "lookup_weather".to_string(),
            },
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 2,
                item_index: None,
                item_id: None,
                id: Some("toolu_123".to_string()),
                name: Some("lookup_weather".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
        ]
    );
}

#[test]
fn test_anthropic_event_to_unified_stream_events_preserves_thinking_lifecycle() {
    let mut session = AnthropicSessionState::default();
    let start = anthropic_event_to_unified_stream_events_with_state(
        AnthropicEvent::ContentBlockStart {
            index: 1,
            content_block: AnthropicContentBlock::Thinking {
                thinking: String::new(),
                signature: None,
            },
        },
        &mut session,
    );
    assert_eq!(
        start,
        vec![
            UnifiedStreamEvent::ItemAdded {
                item_index: Some(1),
                item_id: None,
                item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                    content: Vec::new(),
                    annotations: Vec::new(),
                }),
            },
            UnifiedStreamEvent::ReasoningStart { index: 1 },
        ]
    );

    let delta = anthropic_event_to_unified_stream_events_with_state(
        AnthropicEvent::ContentBlockDelta {
            index: 1,
            delta: AnthropicContentDelta::ThinkingDelta {
                thinking: "step one".to_string(),
            },
        },
        &mut session,
    );
    assert_eq!(
        delta,
        vec![UnifiedStreamEvent::ReasoningDelta {
            index: 1,
            item_index: None,
            item_id: None,
            part_index: None,
            text: "step one".to_string(),
        }]
    );

    let signature = anthropic_event_to_unified_stream_events_with_state(
        AnthropicEvent::ContentBlockDelta {
            index: 1,
            delta: AnthropicContentDelta::SignatureDelta {
                signature: "sig_123".to_string(),
            },
        },
        &mut session,
    );
    assert_eq!(
        signature,
        vec![UnifiedStreamEvent::BlobDelta {
            index: Some(1),
            data: json!({
                "provider": "anthropic",
                "type": "signature_delta",
                "signature": "sig_123",
            }),
        }]
    );

    let stop = anthropic_event_to_unified_stream_events_with_state(
        AnthropicEvent::ContentBlockStop { index: 1 },
        &mut session,
    );
    assert_eq!(
        stop,
        vec![
            UnifiedStreamEvent::ReasoningStop { index: 1 },
            UnifiedStreamEvent::ItemDone {
                item_index: Some(1),
                item_id: None,
                item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                    content: vec![UnifiedContentPart::Reasoning {
                        text: "step one".to_string(),
                    }],
                    annotations: Vec::new(),
                }),
            },
        ]
    );
}

#[test]
fn test_transform_unified_chunk_to_anthropic_events() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);

    // Role chunk
    let unified_chunk_role = UnifiedChunkResponse {
        id: "cmpl-123".to_string(),
        model: Some("test-model".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![],
            },
            finish_reason: None,
        }],
        ..Default::default()
    };
    let events_role = transform_unified_chunk_to_anthropic_events(
        unified_chunk_role,
        &mut state.stream_context(),
    )
    .unwrap();
    assert_eq!(events_role.len(), 1);
    assert_eq!(events_role[0].event.as_deref(), Some("message_start"));
    assert!(
        events_role[0]
            .data
            .contains("\"usage\":{\"input_tokens\":0,\"output_tokens\":0}")
    );
    assert!(state.session.anthropic_message_started());
    assert!(state.session.anthropic_active_blocks_is_empty());

    // Content chunk
    let unified_chunk_content = UnifiedChunkResponse {
        id: "cmpl-123".to_string(),
        model: Some("test-model".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: None,
                content: vec![UnifiedContentPartDelta::TextDelta {
                    index: 0,
                    text: "Hello".to_string(),
                }],
            },
            finish_reason: None,
        }],
        ..Default::default()
    };
    let events_content = transform_unified_chunk_to_anthropic_events(
        unified_chunk_content,
        &mut state.stream_context(),
    )
    .unwrap();
    assert_eq!(events_content.len(), 2);
    assert_eq!(
        events_content[0].event.as_deref(),
        Some("content_block_start")
    );
    assert_eq!(
        events_content[1].event.as_deref(),
        Some("content_block_delta")
    );
    assert!(
        events_content[1]
            .data
            .contains("\"delta\":{\"text\":\"Hello\",\"type\":\"text_delta\"}")
    );
    assert!(state.session.anthropic_active_blocks_contains(&0));

    // Finish chunk
    let unified_chunk_finish = UnifiedChunkResponse {
        id: "cmpl-123".to_string(),
        model: Some("test-model".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: None,
                content: vec![],
            },
            finish_reason: Some("stop".to_string()),
        }],
        ..Default::default()
    };
    let events_finish = transform_unified_chunk_to_anthropic_events(
        unified_chunk_finish,
        &mut state.stream_context(),
    )
    .unwrap();
    assert_eq!(events_finish.len(), 3);
    assert_eq!(
        events_finish[0].event.as_deref(),
        Some("content_block_stop")
    );
    assert_eq!(events_finish[1].event.as_deref(), Some("message_delta"));
    assert!(
        events_finish[1]
            .data
            .contains("\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}")
    );
    assert!(
        events_finish[1]
            .data
            .contains("\"usage\":{\"input_tokens\":0,\"output_tokens\":0}")
    );
    assert_eq!(events_finish[2].event.as_deref(), Some("message_stop"));

    // Thinking content chunk - NOTE: This behavior is no longer supported directly
    // with the new model. Text parts should be used instead. This test may need
    // to be re-evaluated based on desired behavior for "thinking" messages.
    let unified_chunk_thinking = UnifiedChunkResponse {
        id: "cmpl-123".to_string(),
        model: Some("test-model".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: None,
                content: vec![UnifiedContentPartDelta::TextDelta {
                    index: 1, // Assuming a different content block for thinking
                    text: "Thinking...".to_string(),
                }],
            },
            finish_reason: None,
        }],
        ..Default::default()
    };
    let events_thinking = transform_unified_chunk_to_anthropic_events(
        unified_chunk_thinking,
        &mut state.stream_context(),
    );
    // Depending on the new logic, this might produce a regular text delta or be handled differently.
    // For now, let's assume it becomes a normal text block.
    assert!(events_thinking.is_some());
    let events = events_thinking.unwrap();
    assert_eq!(events[0].event.as_deref(), Some("content_block_start"));
    assert_eq!(events[1].event.as_deref(), Some("content_block_delta"));
    assert!(
        events[1]
            .data
            .contains("\"delta\":{\"text\":\"Thinking...\",\"type\":\"text_delta\"}")
    );
}

#[test]
fn test_transform_unified_stream_events_to_anthropic_events_preserves_tool_and_thinking_native_lifecycle()
 {
    let mut state = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Anthropic);
    state.session.set_stream_id("msg_native".to_string());
    state
        .session
        .set_stream_model("claude-3-7-sonnet".to_string());

    let events = transform_unified_stream_events_to_anthropic_events(
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("msg_native".to_string()),
                model: Some("claude-3-7-sonnet".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ToolCallStart {
                index: 0,
                id: "toolu_456".to_string(),
                name: "lookup_weather".to_string(),
            },
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: None,
                item_id: None,
                id: Some("toolu_456".to_string()),
                name: Some("lookup_weather".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
            UnifiedStreamEvent::ToolCallStop {
                index: 0,
                id: Some("toolu_456".to_string()),
            },
            UnifiedStreamEvent::ReasoningStart { index: 1 },
            UnifiedStreamEvent::ReasoningDelta {
                index: 1,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "step one".to_string(),
            },
            UnifiedStreamEvent::BlobDelta {
                index: Some(1),
                data: json!({
                    "provider": "anthropic",
                    "type": "signature_delta",
                    "signature": "sig_456",
                }),
            },
            UnifiedStreamEvent::ReasoningStop { index: 1 },
        ],
        &mut state.stream_context(),
    )
    .unwrap();

    assert_eq!(events[0].event.as_deref(), Some("message_start"));
    assert_eq!(events[1].event.as_deref(), Some("content_block_start"));
    assert!(events[1].data.contains("\"type\":\"tool_use\""));
    assert_eq!(events[2].event.as_deref(), Some("content_block_delta"));
    assert!(events[2].data.contains("\"type\":\"input_json_delta\""));
    assert_eq!(events[3].event.as_deref(), Some("content_block_stop"));
    assert_eq!(events[4].event.as_deref(), Some("content_block_start"));
    assert!(events[4].data.contains("\"type\":\"thinking\""));
    assert_eq!(events[5].event.as_deref(), Some("content_block_delta"));
    assert!(events[5].data.contains("\"type\":\"thinking_delta\""));
    assert_eq!(events[6].event.as_deref(), Some("content_block_delta"));
    assert!(events[6].data.contains("\"type\":\"signature_delta\""));
    assert_eq!(events[7].event.as_deref(), Some("content_block_stop"));
}

#[test]
fn test_transform_unified_stream_events_to_anthropic_events_delays_usage_until_terminal_message_delta()
 {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);
    let events = transform_unified_stream_events_to_anthropic_events(
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("msg_123".to_string()),
                model: Some("gemini-2.5-flash-lite".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "I am Claude Code, Anth".to_string(),
            },
            UnifiedStreamEvent::Usage {
                usage: UnifiedUsage {
                    input_tokens: 26,
                    output_tokens: 6,
                    total_tokens: 32,
                    ..Default::default()
                },
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "ropic's official CLI for Claude.".to_string(),
            },
            UnifiedStreamEvent::Usage {
                usage: UnifiedUsage {
                    input_tokens: 26,
                    output_tokens: 22,
                    total_tokens: 48,
                    ..Default::default()
                },
            },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("stop".to_string()),
            },
            UnifiedStreamEvent::MessageStop,
        ],
        &mut state.stream_context(),
    )
    .unwrap();

    assert_eq!(events.len(), 7);
    assert_eq!(events[0].event.as_deref(), Some("message_start"));
    assert_eq!(events[1].event.as_deref(), Some("content_block_start"));
    assert_eq!(events[2].event.as_deref(), Some("content_block_delta"));
    assert_eq!(events[3].event.as_deref(), Some("content_block_delta"));
    assert_eq!(events[4].event.as_deref(), Some("content_block_stop"));
    assert_eq!(events[5].event.as_deref(), Some("message_delta"));
    assert_eq!(events[6].event.as_deref(), Some("message_stop"));
    assert!(
        events[5]
            .data
            .contains("\"usage\":{\"input_tokens\":26,\"output_tokens\":22}")
    );
    assert!(state.session.anthropic_active_blocks_is_empty());
}

#[test]
fn test_openai_reasoning_stream_transforms_to_anthropic_thinking_then_text_blocks() {
    let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);

    let frames = vec![
        SseEvent {
            data: serde_json::to_string(&json!({
                "id": "019d716629d1f4eb9470f60bc35eb311",
                "object": "chat.completion.chunk",
                "created": 1775724014_i64,
                "model": "deepseek-ai/DeepSeek-V3.2",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": "",
                        "reasoning_content": null,
                        "role": "assistant",
                    },
                    "finish_reason": null
                }],
                "usage": {
                    "prompt_tokens": 98,
                    "completion_tokens": 0,
                    "total_tokens": 98,
                    "completion_tokens_details": {
                        "reasoning_tokens": 0
                    }
                }
            }))
            .unwrap(),
            ..Default::default()
        },
        SseEvent {
            data: serde_json::to_string(&json!({
                "id": "019d716629d1f4eb9470f60bc35eb311",
                "object": "chat.completion.chunk",
                "created": 1775724014_i64,
                "model": "deepseek-ai/DeepSeek-V3.2",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": "",
                        "reasoning_content": "嗯",
                    },
                    "finish_reason": null
                }]
            }))
            .unwrap(),
            ..Default::default()
        },
        SseEvent {
            data: serde_json::to_string(&json!({
                "id": "019d716629d1f4eb9470f60bc35eb311",
                "object": "chat.completion.chunk",
                "created": 1775724014_i64,
                "model": "deepseek-ai/DeepSeek-V3.2",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": "你好",
                        "reasoning_content": null,
                    },
                    "finish_reason": null
                }]
            }))
            .unwrap(),
            ..Default::default()
        },
        SseEvent {
            data: serde_json::to_string(&json!({
                "id": "019d716629d1f4eb9470f60bc35eb311",
                "object": "chat.completion.chunk",
                "created": 1775724014_i64,
                "model": "deepseek-ai/DeepSeek-V3.2",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": "！",
                        "reasoning_content": null,
                    },
                    "finish_reason": null
                }]
            }))
            .unwrap(),
            ..Default::default()
        },
        SseEvent {
            data: serde_json::to_string(&json!({
                "id": "019d716629d1f4eb9470f60bc35eb311",
                "object": "chat.completion.chunk",
                "created": 1775724014_i64,
                "model": "deepseek-ai/DeepSeek-V3.2",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": "",
                        "reasoning_content": null,
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 98,
                    "completion_tokens": 134,
                    "total_tokens": 232,
                    "completion_tokens_details": {
                        "reasoning_tokens": 98
                    }
                }
            }))
            .unwrap(),
            ..Default::default()
        },
        SseEvent {
            data: "[DONE]".to_string(),
            ..Default::default()
        },
    ];

    let events: Vec<SseEvent> = frames
        .into_iter()
        .flat_map(|event| transformer.transform_event(event).unwrap_or_default())
        .collect();

    assert_eq!(events[0].event.as_deref(), Some("message_start"));
    assert_eq!(events[1].event.as_deref(), Some("content_block_start"));
    assert!(events[1].data.contains("\"type\":\"thinking\""));
    assert!(events[1].data.contains("\"signature\":\"\""));
    assert_eq!(events[2].event.as_deref(), Some("content_block_delta"));
    assert!(events[2].data.contains("\"type\":\"thinking_delta\""));
    assert!(events[2].data.contains("\"thinking\":\"嗯\""));
    assert_eq!(events[3].event.as_deref(), Some("content_block_stop"));
    assert_eq!(events[4].event.as_deref(), Some("content_block_start"));
    assert!(events[4].data.contains("\"index\":1"));
    assert!(events[4].data.contains("\"type\":\"text\""));
    assert_eq!(events[5].event.as_deref(), Some("content_block_delta"));
    assert!(events[5].data.contains("\"text\":\"你好\""));
    assert_eq!(events[6].event.as_deref(), Some("content_block_delta"));
    assert!(events[6].data.contains("\"text\":\"！\""));
    assert_eq!(events[7].event.as_deref(), Some("content_block_stop"));
    assert_eq!(events[8].event.as_deref(), Some("message_delta"));
    assert!(
        events[8]
            .data
            .contains("\"usage\":{\"input_tokens\":98,\"output_tokens\":134}")
    );
    assert_eq!(events[9].event.as_deref(), Some("message_stop"));
}

#[test]
fn test_transform_unified_chunk_to_anthropic_events_emits_diagnostic_for_image_delta() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);
    let unified_chunk = UnifiedChunkResponse {
        id: "cmpl-123".to_string(),
        model: Some("claude-3-7-sonnet".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![UnifiedContentPartDelta::ImageDelta {
                    index: 0,
                    url: Some("https://example.com/chart.png".to_string()),
                    data: None,
                }],
            },
            finish_reason: None,
        }],
        ..Default::default()
    };

    let events =
        transform_unified_chunk_to_anthropic_events(unified_chunk, &mut state.stream_context())
            .unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event.as_deref(), Some("message_start"));
    assert_eq!(events[1].event.as_deref(), Some("transform_diagnostic"));
    let diagnostic: Value = serde_json::from_str(&events[1].data).unwrap();
    assert_eq!(diagnostic["semantic_unit"], json!("ImageDelta"));
    assert_eq!(state.session.diagnostics_len(), 1);
}

#[test]
fn test_transform_unified_chunk_to_anthropic_events_preserves_usage_in_start_and_finish() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);

    let start_chunk = UnifiedChunkResponse {
        id: "cmpl-usage".to_string(),
        model: Some("test-model".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![],
            },
            finish_reason: None,
        }],
        usage: Some(UnifiedUsage {
            input_tokens: 2,
            output_tokens: 0,
            total_tokens: 2,
            ..Default::default()
        }),
        ..Default::default()
    };
    let start_events =
        transform_unified_chunk_to_anthropic_events(start_chunk, &mut state.stream_context())
            .unwrap();

    assert_eq!(start_events.len(), 1);
    assert_eq!(start_events[0].event.as_deref(), Some("message_start"));
    assert!(
        start_events[0]
            .data
            .contains("\"usage\":{\"input_tokens\":2,\"output_tokens\":0}")
    );

    let finish_chunk = UnifiedChunkResponse {
        id: "cmpl-usage".to_string(),
        model: Some("test-model".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta::default(),
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(UnifiedUsage {
            input_tokens: 2,
            output_tokens: 8,
            total_tokens: 10,
            ..Default::default()
        }),
        ..Default::default()
    };
    let finish_events =
        transform_unified_chunk_to_anthropic_events(finish_chunk, &mut state.stream_context())
            .unwrap();

    assert_eq!(finish_events.len(), 2);
    assert_eq!(finish_events[0].event.as_deref(), Some("message_delta"));
    assert!(
        finish_events[0]
            .data
            .contains("\"usage\":{\"input_tokens\":2,\"output_tokens\":8}")
    );
    assert!(
        finish_events[0]
            .data
            .contains("\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}")
    );
    assert_eq!(finish_events[1].event.as_deref(), Some("message_stop"));
}

#[test]
fn test_anthropic_response_with_tool_use_and_text_to_unified() {
    let anthropic_response = AnthropicResponse {
        id: "msg_123".to_string(),
        type_: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![
            AnthropicContentBlock::Text {
                text: "I'm thinking...".to_string(),
            },
            AnthropicContentBlock::ToolUse {
                id: "tool_123".to_string(),
                name: "get_weather".to_string(),
                input: json!({"location": "SF"}),
            },
        ],
        model: "claude-3-opus-20240229".to_string(),
        stop_reason: Some("tool_use".to_string()),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: 10,
            output_tokens: 20,
        },
    };

    let unified_response: UnifiedResponse = anthropic_response.into();
    let choice = &unified_response.choices[0];
    assert_eq!(choice.message.content.len(), 2);
    assert_eq!(
        choice.message.content[0],
        UnifiedContentPart::Text {
            text: "I'm thinking...".to_string()
        }
    );
    assert_eq!(
        choice.message.content[1],
        UnifiedContentPart::ToolCall(UnifiedToolCall {
            id: "tool_123".to_string(),
            name: "get_weather".to_string(),
            arguments: json!({"location": "SF"}),
        })
    );
    assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));
}

#[test]
fn test_unified_response_with_thinking_content_to_anthropic() {
    let unified_response = UnifiedResponse {
        id: "msg_123".to_string(),
        model: Some("claude-3-opus-20240229".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![
                    UnifiedContentPart::Text {
                        text: "I'm thinking...".to_string(),
                    },
                    UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id: "tool_123".to_string(),
                        name: "get_weather".to_string(),
                        arguments: json!({"location": "SF"}),
                    }),
                ],
                ..Default::default()
            },
            items: Vec::new(),
            finish_reason: Some("tool_calls".to_string()),
            logprobs: None,
        }],
        usage: Some(UnifiedUsage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
            ..Default::default()
        }),
        created: Some(12345),
        object: Some("chat.completion".to_string()),
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let anthropic_response: AnthropicResponse = unified_response.into();
    assert_eq!(anthropic_response.content.len(), 2);
    match &anthropic_response.content[0] {
        AnthropicContentBlock::Text { text } => assert_eq!(text, "I'm thinking..."),
        _ => panic!("Expected text content block"),
    }
    match &anthropic_response.content[1] {
        AnthropicContentBlock::ToolUse { name, .. } => assert_eq!(name, "get_weather"),
        _ => panic!("Expected tool use content block"),
    }
    assert_eq!(anthropic_response.stop_reason, Some("tool_use".to_string()));
}

#[test]
fn test_anthropic_response_to_unified_preserves_items() {
    let anthropic_response = AnthropicResponse {
        id: "msg_123".to_string(),
        type_: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![
            AnthropicContentBlock::Text {
                text: "Thinking".to_string(),
            },
            AnthropicContentBlock::ToolUse {
                id: "tool_123".to_string(),
                name: "get_weather".to_string(),
                input: json!({"location": "SF"}),
            },
        ],
        model: "claude-3-opus-20240229".to_string(),
        stop_reason: Some("tool_use".to_string()),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: 10,
            output_tokens: 20,
        },
    };

    let unified_response: UnifiedResponse = anthropic_response.into();
    let items = &unified_response.choices[0].items;

    assert_eq!(items.len(), 2);
    match &items[0] {
        UnifiedItem::Message(item) => {
            assert_eq!(item.role, UnifiedRole::Assistant);
            assert_eq!(
                item.content,
                vec![UnifiedContentPart::Text {
                    text: "Thinking".to_string()
                }]
            );
        }
        other => panic!("Expected message item, got {other:?}"),
    }

    match &items[1] {
        UnifiedItem::FunctionCall(item) => {
            assert_eq!(item.id, "tool_123");
            assert_eq!(item.name, "get_weather");
            assert_eq!(item.arguments, json!({"location": "SF"}));
        }
        other => panic!("Expected function call item, got {other:?}"),
    }
}
