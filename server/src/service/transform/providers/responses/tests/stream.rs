use super::*;

#[test]
fn test_unified_chunk_to_responses_uses_formal_stream_events() {
    let unified_chunk = UnifiedChunkResponse {
        id: "chunk_1".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![UnifiedContentPartDelta::TextDelta {
                    index: 0,
                    text: "hello".to_string(),
                }],
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(UnifiedUsage {
            input_tokens: 3,
            output_tokens: 5,
            total_tokens: 8,
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
    let sse =
        transform_unified_chunk_to_responses_events(unified_chunk, &mut state.stream_context())
            .unwrap();
    let chunks: Vec<Value> = sse
        .iter()
        .map(|event| serde_json::from_str(&event.data).unwrap())
        .collect();

    assert_eq!(chunks[0]["type"], json!("response.created"));
    assert_eq!(chunks[0]["sequence_number"], json!(0));
    assert_eq!(chunks[1]["type"], json!("response.output_item.added"));
    assert_eq!(chunks[1]["sequence_number"], json!(1));
    assert_eq!(chunks[1]["item"]["role"], json!("assistant"));
    assert_eq!(chunks[2]["type"], json!("response.output_text.delta"));
    assert_eq!(chunks[2]["sequence_number"], json!(2));
    assert_eq!(chunks[2]["delta"], json!("hello"));
    assert_eq!(chunks[2]["logprobs"], json!([]));
    assert_eq!(chunks[3]["type"], json!("response.output_item.done"));
    assert_eq!(chunks[3]["sequence_number"], json!(3));
    assert_eq!(chunks[4]["type"], json!("response.completed"));
    assert_eq!(chunks[4]["sequence_number"], json!(4));
    assert_eq!(
        chunks[4]["response"]["usage"],
        json!({
            "input_tokens": 3,
            "output_tokens": 5,
            "total_tokens": 8,
            "input_tokens_details": {
                "cached_tokens": 0
            },
            "output_tokens_details": {
                "reasoning_tokens": 0
            }
        })
    );
}

#[test]
fn test_responses_chunk_to_unified_stream_events_preserves_response_incomplete() {
    let chunk = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::ResponseIncomplete {
            response: ResponsesResponse {
                id: "resp_1".to_string(),
                object: ResponseObject::Response,
                created_at: 1,
                completed_at: None,
                status: ResponseStatus::Incomplete,
                incomplete_details: Some(IncompleteDetails {
                    reason: "max_output_tokens".to_string(),
                }),
                model: "gpt-4.1".to_string(),
                previous_response_id: None,
                instructions: None,
                output: vec![],
                error: None,
                tools: vec![],
                tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
                truncation: Truncation::Disabled,
                parallel_tool_calls: true,
                text: TextField {
                    format: TextResponseFormat::Text,
                    verbosity: None,
                },
                top_p: 1.0,
                presence_penalty: 0.0,
                frequency_penalty: 0.0,
                top_logprobs: 0,
                temperature: 1.0,
                reasoning: None,
                usage: Some(Usage {
                    input_tokens: 3,
                    output_tokens: 5,
                    total_tokens: 8,
                    input_tokens_details: InputTokensDetails { cached_tokens: 0 },
                    output_tokens_details: OutputTokensDetails {
                        reasoning_tokens: 0,
                    },
                }),
                max_output_tokens: None,
                max_tool_calls: None,
                store: false,
                background: false,
                service_tier: ServiceTier::Default,
                metadata: json!({}),
                safety_identifier: None,
                prompt_cache_key: None,
            },
        },
    };

    let events = responses_chunk_to_unified_stream_events(chunk);

    assert_eq!(
        events,
        vec![
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("length".to_string()),
            },
            UnifiedStreamEvent::Usage {
                usage: UnifiedUsage {
                    input_tokens: 3,
                    output_tokens: 5,
                    total_tokens: 8,
                    cached_tokens: Some(0),
                    reasoning_tokens: Some(0),
                    ..Default::default()
                },
            },
        ]
    );
}

#[test]
fn test_responses_chunk_to_unified_stream_events_maps_function_call_item() {
    let chunk = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::Item(ItemField::FunctionCall(FunctionCall {
            _type: "function_call".to_string(),
            id: "fc_1".to_string(),
            call_id: "call_1".to_string(),
            name: "lookup_weather".to_string(),
            arguments: "{\"city\":\"Boston\"}".to_string(),
            status: MessageStatus::Completed,
        })),
    };

    let events = responses_chunk_to_unified_stream_events(chunk);

    assert_eq!(
        events,
        vec![
            UnifiedStreamEvent::ItemAdded {
                item_index: Some(0),
                item_id: Some("fc_1".to_string()),
                item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                    id: "call_1".to_string(),
                    name: "lookup_weather".to_string(),
                    arguments: json!({"city":"Boston"}),
                }),
            },
            UnifiedStreamEvent::MessageStart {
                id: Some("resp_1".to_string()),
                model: Some("gpt-4.1".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ContentBlockStart {
                index: 0,
                kind: UnifiedBlockKind::ToolCall,
            },
            UnifiedStreamEvent::ToolCallStart {
                index: 0,
                id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
            },
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: Some(0),
                item_id: Some("fc_1".to_string()),
                id: Some("call_1".to_string()),
                name: Some("lookup_weather".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
            UnifiedStreamEvent::ToolCallStop { index: 0, id: None },
            UnifiedStreamEvent::ContentBlockStop { index: 0 },
            UnifiedStreamEvent::ItemDone {
                item_index: Some(0),
                item_id: Some("fc_1".to_string()),
                item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                    id: "call_1".to_string(),
                    name: "lookup_weather".to_string(),
                    arguments: json!({"city":"Boston"}),
                }),
            },
        ]
    );
}

#[test]
fn test_responses_chunk_to_unified_stream_events_maps_content_part_lifecycle() {
    let added = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::ContentPartAdded {
            item_id: "msg_1".to_string(),
            content_index: 2,
        },
    };
    let done = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::ContentPartDone {
            item_id: "msg_1".to_string(),
            content_index: 2,
        },
    };

    assert_eq!(
        responses_chunk_to_unified_stream_events(added),
        vec![UnifiedStreamEvent::ContentPartAdded {
            item_index: None,
            item_id: Some("msg_1".to_string()),
            part_index: 2,
            part: None,
        }]
    );
    assert_eq!(
        responses_chunk_to_unified_stream_events(done),
        vec![UnifiedStreamEvent::ContentPartDone {
            item_index: None,
            item_id: Some("msg_1".to_string()),
            part_index: 2,
        }]
    );
}

#[test]
fn test_responses_chunk_to_unified_stream_events_maps_reasoning_summary_lifecycle() {
    let added = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::ReasoningSummaryPartAdded {
            item_id: "rs_1".to_string(),
            summary_index: 0,
        },
    };
    let done = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::ReasoningSummaryPartDone {
            item_id: "rs_1".to_string(),
            summary_index: 0,
        },
    };

    assert_eq!(
        responses_chunk_to_unified_stream_events(added),
        vec![UnifiedStreamEvent::ReasoningSummaryPartAdded {
            item_index: None,
            item_id: Some("rs_1".to_string()),
            part_index: 0,
            part: None,
        }]
    );
    assert_eq!(
        responses_chunk_to_unified_stream_events(done),
        vec![UnifiedStreamEvent::ReasoningSummaryPartDone {
            item_index: None,
            item_id: Some("rs_1".to_string()),
            part_index: 0,
        }]
    );
}

#[test]
fn test_responses_chunk_response_serializes_as_standard_event() {
    let chunk = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::ContentBlockDelta {
            index: 0,
            item_index: Some(0),
            item_id: Some("msg_1".to_string()),
            part_index: Some(0),
            text: "hello".to_string(),
        },
    };

    let value = serde_json::to_value(chunk).unwrap();

    assert_eq!(
        value,
        json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "hello"
        })
    );
}

#[test]
fn test_responses_chunk_response_serializes_tool_arguments_as_standard_event() {
    let chunk = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::ToolCallArgumentsDelta {
            index: 0,
            item_index: Some(0),
            item_id: Some("fc_1".to_string()),
            id: Some("call_1".to_string()),
            name: Some("lookup_weather".to_string()),
            arguments: "{\"city\":\"Boston\"}".to_string(),
        },
    };

    let value = serde_json::to_value(chunk).unwrap();

    assert_eq!(
        value,
        json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_1",
            "output_index": 0,
            "name": "lookup_weather",
            "delta": "{\"city\":\"Boston\"}"
        })
    );
}

#[test]
fn test_responses_chunk_response_serializes_tool_arguments_done_as_standard_event() {
    let chunk = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::ToolCallArgumentsDone {
            index: 0,
            item_index: Some(0),
            item_id: Some("fc_1".to_string()),
            id: Some("call_1".to_string()),
            arguments: "{\"city\":\"Boston\"}".to_string(),
        },
    };

    let value = serde_json::to_value(chunk).unwrap();

    assert_eq!(
        value,
        json!({
            "type": "response.function_call_arguments.done",
            "item_id": "fc_1",
            "output_index": 0,
            "call_id": "call_1",
            "arguments": "{\"city\":\"Boston\"}"
        })
    );
}

#[test]
fn test_responses_chunk_response_serializes_reasoning_delta_as_standard_event() {
    let chunk = ResponsesChunkResponse {
        id: "resp_1".to_string(),
        model: "gpt-4.1".to_string(),
        event: ResponsesStreamEvent::ReasoningDelta {
            index: 1,
            item_index: Some(1),
            item_id: Some("rs_1".to_string()),
            part_index: Some(2),
            text: "step".to_string(),
        },
    };

    let value = serde_json::to_value(chunk).unwrap();

    assert_eq!(
        value,
        json!({
            "type": "response.reasoning_summary_text.delta",
            "item_id": "rs_1",
            "summary_index": 2,
            "delta": "step"
        })
    );
}

#[test]
fn test_responses_chunk_response_deserializes_legacy_wrapped_delta() {
    let chunk: ResponsesChunkResponse = serde_json::from_value(json!({
        "id": "resp_legacy",
        "model": "gpt-4.1",
        "delta": {
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "hello"
        }
    }))
    .unwrap();

    assert_eq!(chunk.id, "resp_legacy");
    assert_eq!(chunk.model, "gpt-4.1");
    assert!(matches!(
        chunk.event,
        ResponsesStreamEvent::ContentBlockDelta {
            index: 0,
            ref text,
            ..
        } if text == "hello"
    ));
}

#[test]
fn test_unified_stream_events_to_responses_events_are_not_stubbed() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
    let events = vec![
        UnifiedStreamEvent::MessageStart {
            id: Some("resp_1".to_string()),
            model: Some("gpt-4.1".to_string()),
            role: UnifiedRole::Assistant,
        },
        UnifiedStreamEvent::ToolCallStart {
            index: 0,
            id: "call_1".to_string(),
            name: "lookup_weather".to_string(),
        },
        UnifiedStreamEvent::ToolCallArgumentsDelta {
            index: 0,
            item_index: Some(0),
            item_id: Some("fc_1".to_string()),
            id: Some("call_1".to_string()),
            name: Some("lookup_weather".to_string()),
            arguments: "{\"city\":\"Boston\"}".to_string(),
        },
        UnifiedStreamEvent::ReasoningStart { index: 1 },
        UnifiedStreamEvent::ReasoningSummaryPartAdded {
            item_index: Some(1),
            item_id: Some("rs_1".to_string()),
            part_index: 2,
            part: None,
        },
        UnifiedStreamEvent::ReasoningDelta {
            index: 1,
            item_index: Some(1),
            item_id: Some("rs_1".to_string()),
            part_index: Some(2),
            text: "thinking".to_string(),
        },
        UnifiedStreamEvent::ReasoningSummaryPartDone {
            item_index: Some(1),
            item_id: Some("rs_1".to_string()),
            part_index: 2,
        },
        UnifiedStreamEvent::ReasoningStop { index: 1 },
    ];

    let sse = transform_unified_stream_events_to_responses_events(
        events.clone(),
        &mut state.stream_context(),
    )
    .unwrap();
    let chunks: Vec<ResponsesChunkResponse> = sse
        .iter()
        .map(|event| serde_json::from_str(&event.data).unwrap())
        .collect();

    assert!(
        chunks
            .iter()
            .all(|chunk| !matches!(chunk.event, ResponsesStreamEvent::Unknown(_)))
    );

    let rebuilt: Vec<UnifiedStreamEvent> = chunks
        .into_iter()
        .flat_map(responses_chunk_to_unified_stream_events)
        .collect();

    assert!(rebuilt.iter().any(|event| matches!(
        event,
        UnifiedStreamEvent::ToolCallArgumentsDelta {
            index: 0,
            item_index: Some(_),
            item_id: Some(_),
            id: Some(_),
            name: Some(name),
            arguments,
        } if name == "lookup_weather"
            && arguments == "{\"city\":\"Boston\"}"
    )));
    assert!(rebuilt.iter().any(|event| matches!(
        event,
        UnifiedStreamEvent::ReasoningSummaryPartAdded {
            item_index: _,
            item_id: Some(item_id),
            part_index: 2,
            ..
        } if item_id == "rs_1"
    )));
    assert!(rebuilt.iter().any(|event| matches!(
        event,
        UnifiedStreamEvent::ReasoningDelta {
            item_index: _,
            item_id: Some(item_id),
            part_index: Some(2),
            text,
            ..
        } if item_id == "rs_1" && text == "thinking"
    )));
    assert!(rebuilt.iter().any(|event| matches!(
        event,
        UnifiedStreamEvent::ReasoningSummaryPartDone {
            item_index: _,
            item_id: Some(item_id),
            part_index: 2,
        } if item_id == "rs_1"
    )));
}

#[test]
fn test_unified_stream_events_to_responses_completed_includes_all_output_items() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
    let sse = transform_unified_stream_events_to_responses_events(
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("resp_multi".to_string()),
                model: Some("gpt-4.1".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "final answer".to_string(),
            },
            UnifiedStreamEvent::ToolCallStart {
                index: 1,
                id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
            },
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 1,
                item_index: None,
                item_id: None,
                id: Some("call_1".to_string()),
                name: Some("lookup_weather".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
            UnifiedStreamEvent::ToolCallStop {
                index: 1,
                id: Some("call_1".to_string()),
            },
            UnifiedStreamEvent::ReasoningStart { index: 2 },
            UnifiedStreamEvent::ReasoningDelta {
                index: 2,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "checked policy".to_string(),
            },
            UnifiedStreamEvent::ReasoningStop { index: 2 },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("stop".to_string()),
            },
            UnifiedStreamEvent::Usage {
                usage: UnifiedUsage {
                    input_tokens: 3,
                    output_tokens: 5,
                    total_tokens: 8,
                    ..Default::default()
                },
            },
        ],
        &mut state.stream_context(),
    )
    .unwrap();

    let frames: Vec<Value> = sse
        .iter()
        .map(|event| serde_json::from_str(&event.data).unwrap())
        .collect();
    let completed = frames
        .iter()
        .find(|frame| frame["type"] == json!("response.completed"))
        .unwrap();
    let output = completed["response"]["output"].as_array().unwrap();

    assert_eq!(output.len(), 3);
    assert!(matches!(
        &output[0],
        value if value["type"] == json!("message")
            && value["content"][0]["type"] == json!("output_text")
            && value["content"][0]["text"] == json!("final answer")
    ));
    assert!(matches!(
        &output[1],
        value if value["type"] == json!("function_call")
            && value["call_id"] == json!("call_1")
            && value["arguments"] == json!("{\"city\":\"Boston\"}")
            && value["status"] == json!("completed")
    ));
    assert!(matches!(
        &output[2],
        value if value["type"] == json!("reasoning")
            && value["summary"][0]["type"] == json!("summary_text")
            && value["summary"][0]["text"] == json!("checked policy")
    ));
}

#[test]
fn test_unified_stream_events_to_responses_emit_function_call_arguments_done() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
    let sse = transform_unified_stream_events_to_responses_events(
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("resp_tool".to_string()),
                model: Some("gpt-4.1".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ToolCallStart {
                index: 0,
                id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
            },
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: Some(0),
                item_id: Some("fc_1".to_string()),
                id: Some("call_1".to_string()),
                name: Some("lookup_weather".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
            UnifiedStreamEvent::ToolCallStop {
                index: 0,
                id: Some("call_1".to_string()),
            },
        ],
        &mut state.stream_context(),
    )
    .unwrap();

    let frames: Vec<Value> = sse
        .iter()
        .map(|event| serde_json::from_str(&event.data).unwrap())
        .collect();

    assert!(frames.iter().any(|frame| {
        frame["type"] == json!("response.function_call_arguments.done")
            && frame["item_id"] == json!("fc_1")
            && frame["output_index"] == json!(0)
            && frame["call_id"] == json!("call_1")
            && frame["arguments"] == json!("{\"city\":\"Boston\"}")
    }));
}

#[test]
fn test_unified_stream_events_to_responses_ignores_duplicate_message_start_for_active_item() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
    let sse = transform_unified_stream_events_to_responses_events(
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("resp_repeat".to_string()),
                model: Some("deepseek-ai/DeepSeek-V3.2".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: Some(0),
                text: "1".to_string(),
            },
            UnifiedStreamEvent::MessageStart {
                id: Some("resp_repeat".to_string()),
                model: Some("deepseek-ai/DeepSeek-V3.2".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: Some(0),
                text: "2".to_string(),
            },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("stop".to_string()),
            },
            UnifiedStreamEvent::Usage {
                usage: UnifiedUsage {
                    input_tokens: 12,
                    output_tokens: 2,
                    total_tokens: 14,
                    ..Default::default()
                },
            },
        ],
        &mut state.stream_context(),
    )
    .unwrap();

    let frames: Vec<Value> = sse
        .iter()
        .map(|event| serde_json::from_str(&event.data).unwrap())
        .collect();

    let added_frames: Vec<&Value> = frames
        .iter()
        .filter(|frame| frame["type"] == json!("response.output_item.added"))
        .collect();
    assert_eq!(added_frames.len(), 1);

    let completed = frames
        .iter()
        .find(|frame| frame["type"] == json!("response.completed"))
        .unwrap();
    assert_eq!(completed["response"]["output"].as_array().unwrap().len(), 1);
    assert_eq!(
        completed["response"]["output"][0]["content"][0]["text"],
        json!("12")
    );
}

#[test]
fn test_unified_stream_events_to_responses_uses_explicit_content_part_lifecycle_for_text_delta() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
    let sse = transform_unified_stream_events_to_responses_events(
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("resp_parts".to_string()),
                model: Some("gpt-4.1".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ContentPartAdded {
                item_index: Some(0),
                item_id: Some("msg_part".to_string()),
                part_index: 3,
                part: None,
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: Some(3),
                text: "hello".to_string(),
            },
            UnifiedStreamEvent::ContentPartDone {
                item_index: Some(0),
                item_id: Some("msg_part".to_string()),
                part_index: 3,
            },
        ],
        &mut state.stream_context(),
    )
    .unwrap();

    let frames: Vec<Value> = sse
        .iter()
        .map(|event| serde_json::from_str(&event.data).unwrap())
        .collect();

    assert!(frames.iter().any(|frame| {
        frame["type"] == json!("response.content_part.added") && frame["content_index"] == json!(3)
    }));
    assert!(frames.iter().any(|frame| {
        frame["type"] == json!("response.output_text.delta")
            && frame["content_index"] == json!(3)
            && frame["delta"] == json!("hello")
    }));
    assert!(frames.iter().any(|frame| {
        frame["type"] == json!("response.content_part.done") && frame["content_index"] == json!(3)
    }));
}

#[test]
fn test_unified_stream_events_to_responses_uses_explicit_reasoning_part_lifecycle_without_synthetic_added()
 {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
    let sse = transform_unified_stream_events_to_responses_events(
        vec![
            UnifiedStreamEvent::ReasoningStart { index: 2 },
            UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index: Some(2),
                item_id: None,
                part_index: 4,
                part: None,
            },
            UnifiedStreamEvent::ReasoningDelta {
                index: 2,
                item_index: Some(2),
                item_id: None,
                part_index: Some(4),
                text: "step".to_string(),
            },
            UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index: Some(2),
                item_id: None,
                part_index: 4,
            },
            UnifiedStreamEvent::ReasoningStop { index: 2 },
        ],
        &mut state.stream_context(),
    )
    .unwrap();

    let frames: Vec<Value> = sse
        .iter()
        .map(|event| serde_json::from_str(&event.data).unwrap())
        .collect();

    let added_frames: Vec<&Value> = frames
        .iter()
        .filter(|frame| frame["type"] == json!("response.reasoning_summary_part.added"))
        .collect();
    assert_eq!(added_frames.len(), 1);
    assert_eq!(added_frames[0]["summary_index"], json!(4));

    let delta = frames
        .iter()
        .find(|frame| frame["type"] == json!("response.reasoning_summary_text.delta"))
        .unwrap();
    assert_eq!(delta["summary_index"], json!(4));

    let done = frames
        .iter()
        .find(|frame| frame["type"] == json!("response.reasoning_summary_part.done"))
        .unwrap();
    assert_eq!(done["summary_index"], json!(4));
}

#[test]
fn test_unified_stream_events_to_responses_emits_response_incomplete_for_length_finish_reason() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
    let sse = transform_unified_stream_events_to_responses_events(
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("resp_incomplete".to_string()),
                model: Some("gpt-4.1".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "partial answer".to_string(),
            },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("length".to_string()),
            },
            UnifiedStreamEvent::Usage {
                usage: UnifiedUsage {
                    input_tokens: 3,
                    output_tokens: 5,
                    total_tokens: 8,
                    ..Default::default()
                },
            },
        ],
        &mut state.stream_context(),
    )
    .unwrap();

    let frames: Vec<Value> = sse
        .iter()
        .map(|event| serde_json::from_str(&event.data).unwrap())
        .collect();
    let incomplete = frames
        .iter()
        .find(|frame| frame["type"] == json!("response.incomplete"))
        .unwrap();

    assert_eq!(incomplete["response"]["status"], json!("incomplete"));
    assert_eq!(
        incomplete["response"]["incomplete_details"]["reason"],
        json!("max_output_tokens")
    );
    assert_eq!(incomplete["response"]["completed_at"], Value::Null);
}

#[test]
fn test_unified_stream_events_to_responses_preserve_explicit_stream_id_and_model() {
    let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
    let sse = transform_unified_stream_events_to_responses_events(
        vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("resp_explicit".to_string()),
                model: Some("gpt-4.1-mini".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "hello".to_string(),
            },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("stop".to_string()),
            },
        ],
        &mut state.stream_context(),
    )
    .unwrap();

    let chunks: Vec<ResponsesChunkResponse> = sse
        .iter()
        .map(|event| serde_json::from_str(&event.data).unwrap())
        .collect();

    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].id, "resp_explicit");
    assert_eq!(chunks[0].model, "gpt-4.1-mini");
    assert!(matches!(
        chunks[0].event,
        ResponsesStreamEvent::ResponseCreated { .. }
    ));
    assert!(matches!(
        chunks[1].event,
        ResponsesStreamEvent::OutputItemAdded {
            output_index: 0,
            ..
        }
    ));
    assert!(matches!(
        chunks[2].event,
        ResponsesStreamEvent::ContentBlockDelta {
            index: 0,
            ref text,
            ..
        } if text == "hello"
    ));
}
