use super::*;
use crate::schema::enum_def::{LlmApiType, ProviderType};
use crate::service::transform::{StreamTransformer, unified::*};
use serde_json::{Value, json};

#[test]
fn test_openai_request_to_unified() {
    let openai_req = OpenAiRequestPayload {
        model: "gpt-4".to_string(),
        messages: vec![
            OpenAiMessage {
                role: "system".to_string(),
                content: Some(OpenAiContent::Text(
                    "You are a helpful assistant.".to_string(),
                )),
                tool_calls: None,
                name: None,
                tool_call_id: None,
                refusal: None,
            },
            OpenAiMessage {
                role: "user".to_string(),
                content: Some(OpenAiContent::Text("Hello".to_string())),
                tool_calls: None,
                name: None,
                tool_call_id: None,
                refusal: None,
            },
        ],
        tools: None,
        tool_choice: None,
        stream: Some(false),
        temperature: Some(0.8),
        max_tokens: Some(100),
        top_p: Some(0.9),
        stop: Some(OpenAiStop::String("stop".to_string())),
        n: None,
        seed: None,
        presence_penalty: None,
        frequency_penalty: None,
        logit_bias: None,
        logprobs: None,
        top_logprobs: None,
        response_format: None,
        user: None,
        parallel_tool_calls: None,
        reasoning_effort: None,
    };

    let unified_req: UnifiedRequest = openai_req.into();

    assert_eq!(unified_req.model, Some("gpt-4".to_string()));
    assert_eq!(unified_req.messages.len(), 2);
    assert_eq!(unified_req.messages[0].role, UnifiedRole::System);
    assert_eq!(
        unified_req.messages[0].content,
        vec![UnifiedContentPart::Text {
            text: "You are a helpful assistant.".to_string()
        }]
    );
    assert_eq!(unified_req.messages[1].role, UnifiedRole::User);
    assert_eq!(
        unified_req.messages[1].content,
        vec![UnifiedContentPart::Text {
            text: "Hello".to_string()
        }]
    );
    assert_eq!(unified_req.temperature, Some(0.8));
    assert_eq!(unified_req.max_tokens, Some(100));
    assert_eq!(unified_req.top_p, Some(0.9));
    assert_eq!(unified_req.stop, Some(vec!["stop".to_string()]));
    assert!(unified_req.openai_extension().is_none());
}

#[test]
fn test_unified_request_to_openai() {
    let unified_req = UnifiedRequest {
        model: Some("gpt-4".to_string()),
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
                    text: "Hello".to_string(),
                }],
            },
        ],
        tools: None,
        stream: false,
        temperature: Some(0.8),
        max_tokens: Some(100),
        top_p: Some(0.9),
        stop: Some(vec!["stop".to_string()]),
        seed: None,
        presence_penalty: None,
        frequency_penalty: None,
        ..Default::default()
    };

    let openai_req: OpenAiRequestPayload = unified_req.into();

    assert_eq!(openai_req.model, "gpt-4".to_string());
    assert_eq!(openai_req.messages.len(), 2);
    assert_eq!(openai_req.messages[0].role, "system");
    match openai_req.messages[0].content.as_ref().unwrap() {
        OpenAiContent::Text(t) => assert_eq!(t, "You are a helpful assistant."),
        _ => panic!("Expected text content"),
    }
    assert_eq!(openai_req.messages[1].role, "user");
    match openai_req.messages[1].content.as_ref().unwrap() {
        OpenAiContent::Text(t) => assert_eq!(t, "Hello"),
        _ => panic!("Expected text content"),
    }
    assert_eq!(openai_req.temperature, Some(0.8));
    assert_eq!(openai_req.max_tokens, Some(100));
    assert_eq!(openai_req.top_p, Some(0.9));
    match openai_req.stop.as_ref().unwrap() {
        OpenAiStop::String(s) => assert_eq!(s, "stop"),
        _ => panic!("Expected string stop"),
    }
}

#[test]
fn test_unified_request_to_openai_preserves_reasoning_as_text() {
    let unified_req = UnifiedRequest {
        model: Some("gpt-4".to_string()),
        messages: vec![UnifiedMessage {
            role: UnifiedRole::User,
            content: vec![
                UnifiedContentPart::Text {
                    text: "Question".to_string(),
                },
                UnifiedContentPart::Reasoning {
                    text: "hidden reasoning".to_string(),
                },
            ],
        }],
        ..Default::default()
    };

    let openai_req: OpenAiRequestPayload = unified_req.into();
    match openai_req.messages[0].content.as_ref().unwrap() {
        OpenAiContent::Parts(parts) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(
                &parts[1],
                OpenAiContentPart::Text { text } if text == "hidden reasoning"
            ));
        }
        other => panic!("Expected multipart OpenAI content, got {:?}", other),
    }
}

#[test]
fn test_unified_request_to_openai_preserves_image_data_file_and_code() {
    let unified_req = UnifiedRequest {
        model: Some("gpt-4".to_string()),
        messages: vec![UnifiedMessage {
            role: UnifiedRole::User,
            content: vec![
                UnifiedContentPart::ImageData {
                    mime_type: "image/png".to_string(),
                    data: "ZmFrZQ==".to_string(),
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
        ..Default::default()
    };

    let openai_req: OpenAiRequestPayload = unified_req.into();
    match openai_req.messages[0].content.as_ref().unwrap() {
        OpenAiContent::Parts(parts) => {
            assert!(matches!(
                &parts[0],
                OpenAiContentPart::ImageUrl { image_url }
                if image_url.url == "data:image/png;base64,ZmFrZQ==" && image_url.detail.as_deref() == Some("auto")
            ));
            assert!(matches!(
                &parts[1],
                OpenAiContentPart::Text { text }
                if text == "file_url: https://files.example.com/report.pdf\nmime_type: application/pdf"
            ));
            assert!(matches!(
                &parts[2],
                OpenAiContentPart::Text { text }
                if text == "```python\nprint(1)\n```"
            ));
        }
        other => panic!("Expected multipart OpenAI content, got {:?}", other),
    }
}

#[test]
fn test_openai_response_to_unified() {
    let openai_res = OpenAiResponse {
        id: "chatcmpl-123".to_string(),
        object: "chat.completion".to_string(),
        created: 12345,
        model: "gpt-4".to_string(),
        system_fingerprint: None,
        choices: vec![OpenAiChoice {
            index: 0,
            message: OpenAiMessage {
                role: "assistant".to_string(),
                content: Some(OpenAiContent::Text("Hi there!".to_string())),
                tool_calls: None,
                name: None,
                tool_call_id: None,
                refusal: None,
            },
            logprobs: None,
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(OpenAiUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
    };

    let unified_res: UnifiedResponse = openai_res.into();

    assert_eq!(unified_res.choices.len(), 1);
    let choice = &unified_res.choices[0];
    assert_eq!(choice.message.role, UnifiedRole::Assistant);
    assert_eq!(
        choice.message.content,
        vec![UnifiedContentPart::Text {
            text: "Hi there!".to_string()
        }]
    );
    assert_eq!(choice.finish_reason, Some("stop".to_string()));
    assert!(unified_res.usage.is_some());
    let usage = unified_res.usage.unwrap();
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 20);
    assert_eq!(usage.total_tokens, 30);
}

#[test]
fn test_unified_response_to_openai() {
    let unified_res = UnifiedResponse {
        id: "chatcmpl-123".to_string(),
        model: Some("gpt-4".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::Text {
                    text: "Hi there!".to_string(),
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

    let openai_res: OpenAiResponse = unified_res.into();

    assert_eq!(openai_res.choices.len(), 1);
    let choice = &openai_res.choices[0];
    assert_eq!(choice.message.role, "assistant");
    match choice.message.content.as_ref().unwrap() {
        OpenAiContent::Text(t) => assert_eq!(t, "Hi there!"),
        _ => panic!("Expected text content"),
    }
    assert_eq!(choice.finish_reason, Some("stop".to_string()));
    assert!(openai_res.usage.is_some());
    let usage = openai_res.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 10);
    assert_eq!(usage.completion_tokens, 20);
    assert_eq!(usage.total_tokens, 30);
}

#[test]
fn test_openai_response_to_unified_promotes_refusal() {
    let openai_res = OpenAiResponse {
        id: "chatcmpl-123".to_string(),
        object: "chat.completion".to_string(),
        created: 12345,
        model: "gpt-4".to_string(),
        system_fingerprint: None,
        choices: vec![OpenAiChoice {
            index: 0,
            message: OpenAiMessage {
                role: "assistant".to_string(),
                content: Some(OpenAiContent::Text("safe answer".to_string())),
                tool_calls: None,
                name: None,
                tool_call_id: None,
                refusal: Some("cannot comply".to_string()),
            },
            logprobs: None,
            finish_reason: Some("stop".to_string()),
        }],
        usage: None,
    };

    let unified_res: UnifiedResponse = openai_res.into();

    assert!(matches!(
        &unified_res.choices[0].message.content[..],
        [UnifiedContentPart::Refusal { text }, UnifiedContentPart::Text { text: answer }]
        if text == "cannot comply" && answer == "safe answer"
    ));
}

#[test]
fn test_unified_response_to_openai_preserves_refusal_field() {
    let unified_res = UnifiedResponse {
        id: "chatcmpl-123".to_string(),
        model: Some("gpt-4".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![
                    UnifiedContentPart::Refusal {
                        text: "cannot comply".to_string(),
                    },
                    UnifiedContentPart::Text {
                        text: "safe answer".to_string(),
                    },
                ],
                ..Default::default()
            },
            items: Vec::new(),
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: None,
        created: Some(12345),
        object: Some("chat.completion".to_string()),
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let openai_res: OpenAiResponse = unified_res.into();

    assert_eq!(
        openai_res.choices[0].message.refusal.as_deref(),
        Some("cannot comply")
    );
    match openai_res.choices[0].message.content.as_ref().unwrap() {
        OpenAiContent::Text(text) => assert_eq!(text, "safe answer"),
        other => panic!("Expected text content, got {:?}", other),
    }
}

#[test]
fn test_openai_chunk_to_unified() {
    let openai_chunk = OpenAiChunkResponse {
        id: "chatcmpl-123".to_string(),
        object: "chat.completion.chunk".to_string(),
        created: 12345,
        model: "gpt-4".to_string(),
        system_fingerprint: None,
        choices: vec![OpenAiChunkChoice {
            index: 0,
            delta: OpenAiChunkDelta {
                role: Some("assistant".to_string()),
                content: Some("Hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                refusal: None,
                name: None,
            },
            finish_reason: None,
            logprobs: None,
        }],
        usage: None,
    };

    let unified_chunk: UnifiedChunkResponse = openai_chunk.into();

    assert_eq!(unified_chunk.choices.len(), 1);
    let choice = &unified_chunk.choices[0];
    assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
    assert_eq!(
        choice.delta.content,
        vec![UnifiedContentPartDelta::TextDelta {
            index: 0,
            text: "Hello".to_string()
        }]
    );
    assert!(choice.finish_reason.is_none());
}

#[test]
fn test_openai_chunk_to_unified_stream_events_with_reasoning_and_text() {
    let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);

    let reasoning_events = openai_chunk_to_unified_stream_events_with_state(
        OpenAiChunkResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 12345,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: Some("assistant".to_string()),
                    content: Some(String::new()),
                    reasoning_content: Some("step one".to_string()),
                    tool_calls: None,
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        },
        &mut transformer.stream_context(),
    );

    assert_eq!(
        reasoning_events,
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("chatcmpl-123".to_string()),
                model: Some("gpt-4".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ReasoningStart { index: 0 },
            UnifiedStreamEvent::ReasoningDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "step one".to_string(),
            },
        ]
    );
    transformer.update_session_from_stream_events(&reasoning_events);

    let text_events = openai_chunk_to_unified_stream_events_with_state(
        OpenAiChunkResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 12346,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: Some("Hello".to_string()),
                    reasoning_content: None,
                    tool_calls: None,
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        },
        &mut transformer.stream_context(),
    );

    assert_eq!(
        text_events,
        vec![
            UnifiedStreamEvent::ReasoningStop { index: 0 },
            UnifiedStreamEvent::ContentBlockStart {
                index: 1,
                kind: UnifiedBlockKind::Text,
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 1,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "Hello".to_string(),
            },
        ]
    );
    transformer.update_session_from_stream_events(&text_events);

    let finish_events = openai_chunk_to_unified_stream_events_with_state(
        OpenAiChunkResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 12347,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: Some(String::new()),
                    reasoning_content: None,
                    tool_calls: None,
                    refusal: None,
                    name: None,
                },
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
        },
        &mut transformer.stream_context(),
    );

    assert_eq!(
        finish_events,
        vec![
            UnifiedStreamEvent::ContentBlockStop { index: 1 },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("stop".to_string()),
            },
        ]
    );
}

#[test]
fn test_openai_chunk_to_unified_stream_events_drops_late_reasoning_after_text_started() {
    let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);
    transformer.session.set_current_content_block_index(Some(0));

    let events = openai_chunk_to_unified_stream_events_with_state(
        OpenAiChunkResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 12345,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: None,
                    reasoning_content: Some("too late".to_string()),
                    tool_calls: None,
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        },
        &mut transformer.stream_context(),
    );

    assert!(events.is_empty());
}

#[test]
fn test_openai_chunk_to_unified_stream_events_emits_tool_call_stop_on_tool_calls_finish() {
    let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);

    let start_events = openai_chunk_to_unified_stream_events_with_state(
        OpenAiChunkResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 12345,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: Some("assistant".to_string()),
                    content: None,
                    reasoning_content: None,
                    tool_calls: Some(vec![OpenAiChunkToolCall {
                        index: 0,
                        id: Some("call_1".to_string()),
                        type_: Some("function".to_string()),
                        function: OpenAiChunkFunction {
                            name: Some("search_web".to_string()),
                            arguments: Some("{".to_string()),
                        },
                    }]),
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        },
        &mut transformer.stream_context(),
    );

    assert_eq!(
        start_events,
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("chatcmpl-123".to_string()),
                model: Some("gpt-4".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ToolCallStart {
                index: 0,
                id: "call_1".to_string(),
                name: "search_web".to_string(),
            },
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: None,
                item_id: None,
                id: Some("call_1".to_string()),
                name: Some("search_web".to_string()),
                arguments: "{".to_string(),
            },
        ]
    );
    transformer.update_session_from_stream_events(&start_events);

    let finish_events = openai_chunk_to_unified_stream_events_with_state(
        OpenAiChunkResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 12346,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: None,
                    reasoning_content: None,
                    tool_calls: None,
                    refusal: None,
                    name: None,
                },
                finish_reason: Some("tool_calls".to_string()),
                logprobs: None,
            }],
            usage: None,
        },
        &mut transformer.stream_context(),
    );

    assert_eq!(
        finish_events,
        vec![
            UnifiedStreamEvent::ToolCallStop {
                index: 0,
                id: Some("call_1".to_string()),
            },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("tool_calls".to_string()),
            },
        ]
    );
}

#[test]
fn test_unified_chunk_to_openai() {
    let unified_chunk = UnifiedChunkResponse {
        id: "chatcmpl-123".to_string(),
        model: Some("gpt-4".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![UnifiedContentPartDelta::TextDelta {
                    index: 0,
                    text: "Hello".to_string(),
                }],
            },
            finish_reason: None,
        }],
        usage: None,
        created: Some(12345),
        object: Some("chat.completion.chunk".to_string()),
        provider_session_metadata: None,
        synthetic_metadata: None,
    };

    let openai_chunk: OpenAiChunkResponse = unified_chunk.into();

    assert_eq!(openai_chunk.choices.len(), 1);
    let choice = &openai_chunk.choices[0];
    assert_eq!(choice.delta.role, Some("assistant".to_string()));
    assert_eq!(choice.delta.content, Some("Hello".to_string()));
    assert!(choice.finish_reason.is_none());
}

#[test]
fn test_transform_unified_chunk_to_openai_events_emits_diagnostic_for_image_delta() {
    let unified_chunk = UnifiedChunkResponse {
        id: "cmpl-123".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![
                    UnifiedContentPartDelta::ImageDelta {
                        index: 1,
                        url: Some("https://example.com/chart.png".to_string()),
                        data: None,
                    },
                    UnifiedContentPartDelta::TextDelta {
                        index: 0,
                        text: "caption".to_string(),
                    },
                ],
            },
            finish_reason: None,
        }],
        ..Default::default()
    };

    let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);
    let events =
        transform_unified_chunk_to_openai_events(unified_chunk, &mut transformer.stream_context())
            .expect("openai chunk events");

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event.as_deref(), Some("transform_diagnostic"));
    let diagnostic: Value = serde_json::from_str(&events[0].data).unwrap();
    assert_eq!(diagnostic["semantic_unit"], json!("ImageDelta"));

    let chunk: Value = serde_json::from_str(&events[1].data).unwrap();
    assert_eq!(chunk["choices"][0]["delta"]["content"], json!("caption"));
}

#[test]
fn test_determine_openai_variant_for_vertex_openai_chat_completions() {
    assert_eq!(
        determine_openai_variant(&ProviderType::VertexOpenai, "chat/completions"),
        OpenAiVariant::GeminiCompat
    );
    assert_eq!(
        determine_openai_variant(&ProviderType::Openai, "chat/completions"),
        OpenAiVariant::Standard
    );
    assert_eq!(
        determine_openai_variant(&ProviderType::VertexOpenai, "embeddings"),
        OpenAiVariant::Standard
    );
}

#[test]
fn test_sanitize_openai_request_payload_for_gemini_variant() {
    let mut payload = json!({
        "model": "gemini-2.5-pro",
        "messages": [{"role": "user", "content": "hello"}],
        "temperature": 0.2,
        "tools": [],
        "stream": true,
        "stream_options": {"include_usage": true},
        "parallel_tool_calls": true,
        "logprobs": true,
        "user": "user-123"
    });

    let report = sanitize_openai_request_payload(&mut payload, OpenAiVariant::GeminiCompat);

    assert_eq!(
        payload,
        json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.2,
            "tools": [],
            "stream": true,
            "stream_options": {"include_usage": true}
        })
    );
    assert_eq!(
        report.removed_fields,
        vec![
            "logprobs".to_string(),
            "parallel_tool_calls".to_string(),
            "user".to_string()
        ]
    );
    assert!(report.injected_defaults.is_empty());
}

#[test]
fn test_sanitize_openai_request_payload_for_standard_variant_is_noop() {
    let mut payload = json!({
        "model": "gpt-4.1",
        "messages": [{"role": "user", "content": "hello"}],
        "stream_options": {"include_usage": true},
        "parallel_tool_calls": true
    });

    let original = payload.clone();
    let report = sanitize_openai_request_payload(&mut payload, OpenAiVariant::Standard);

    assert_eq!(payload, original);
    assert!(report.removed_fields.is_empty());
    assert!(report.injected_defaults.is_empty());
}

#[test]
fn test_resolve_openai_variant_policy_keeps_standard_and_compat_separate() {
    let standard = resolve_openai_variant_policy(&ProviderType::Openai, "chat/completions");
    let compat = resolve_openai_variant_policy(&ProviderType::VertexOpenai, "chat/completions");

    assert_eq!(standard.variant(), OpenAiVariant::Standard);
    assert_eq!(compat.variant(), OpenAiVariant::GeminiCompat);

    let mut standard_payload = json!({
        "model": "gpt-4.1",
        "messages": [{"role": "user", "content": "hello"}],
        "stream_options": {"include_usage": true},
        "parallel_tool_calls": true
    });
    let standard_report = standard.sanitize_request_payload(&mut standard_payload);
    assert!(standard_report.removed_fields.is_empty());
    assert_eq!(
        standard_payload,
        json!({
            "model": "gpt-4.1",
            "messages": [{"role": "user", "content": "hello"}],
            "stream_options": {"include_usage": true},
            "parallel_tool_calls": true
        })
    );

    let mut compat_payload = json!({
        "model": "gemini-2.5-pro",
        "messages": [{"role": "user", "content": "hello"}],
        "stream_options": {"include_usage": true},
        "parallel_tool_calls": true
    });
    let compat_report = compat.sanitize_request_payload(&mut compat_payload);
    assert_eq!(
        compat_report.removed_fields,
        vec!["parallel_tool_calls".to_string()]
    );
    assert_eq!(
        compat_payload,
        json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "stream_options": {"include_usage": true}
        })
    );
}

#[test]
fn test_finalize_openai_compatible_request_payload_uses_variant_layer_only() {
    let mut payload = json!({
        "model": "gemini-2.5-pro",
        "messages": [{"role": "user", "content": "hello"}],
        "stream_options": {"include_usage": true},
        "user": "user-123"
    });

    let (variant, report) = finalize_openai_compatible_request_payload(
        &mut payload,
        &ProviderType::VertexOpenai,
        "chat/completions",
    );

    assert_eq!(variant, OpenAiVariant::GeminiCompat);
    assert_eq!(report.removed_fields, vec!["user".to_string()]);
    assert_eq!(
        payload,
        json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "stream_options": {"include_usage": true}
        })
    );
}

#[test]
fn test_build_registered_passthrough_filters_unregistered_keys() {
    let passthrough = build_registered_passthrough(
        vec![
            ("logprobs".to_string(), json!(true)),
            ("future_field".to_string(), json!("blocked")),
        ],
        "test_passthrough_registry",
    )
    .unwrap();

    assert_eq!(passthrough, json!({ "logprobs": true }));
}

#[test]
fn test_unified_request_to_openai_ignores_unregistered_passthrough_keys() {
    let request = UnifiedRequest {
        model: Some("gpt-4.1".to_string()),
        messages: vec![UnifiedMessage {
            role: UnifiedRole::User,
            content: vec![UnifiedContentPart::Text {
                text: "hello".to_string(),
            }],
        }],
        extensions: Some(UnifiedRequestExtensions {
            openai: Some(UnifiedOpenAiRequestExtension {
                passthrough: Some(json!({
                    "parallel_tool_calls": true,
                    "future_field": "blocked"
                })),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let openai_request = OpenAiRequestPayload::from(request);

    assert_eq!(openai_request.parallel_tool_calls, Some(true));
    assert_eq!(openai_request.logprobs, None);
    assert_eq!(openai_request.top_logprobs, None);
    assert!(openai_request.reasoning_effort.is_none());
}
