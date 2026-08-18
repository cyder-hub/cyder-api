use serde_json::{Value, json};

use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::service::transform::{StreamTransformer, TransformReasonCode, unified::*};

use super::*;

#[test]
fn gemini_terminal_classifier_covers_frozen_finish_and_block_families() {
    for reason in [
        "STOP",
        "MAX_TOKENS",
        "SAFETY",
        "RECITATION",
        "LANGUAGE",
        "BLOCKLIST",
        "PROHIBITED_CONTENT",
        "SPII",
        "IMAGE_SAFETY",
        "IMAGE_PROHIBITED_CONTENT",
        "IMAGE_RECITATION",
        "ESCALATION",
        "MODEL_ARMOR",
    ] {
        let classified = classify_gemini_terminal(&json!({
            "candidates":[{"index":0,"finishReason":reason}]
        }));
        let expected = match reason {
            "STOP" => GeminiTerminalKind::Stop,
            "MAX_TOKENS" => GeminiTerminalKind::MaxTokens,
            _ => GeminiTerminalKind::Safety,
        };
        assert_eq!(classified.kind, expected, "{reason}");
        assert_eq!(classified.reason_code, None, "{reason}");
    }

    for reason in [
        "MALFORMED_FUNCTION_CALL",
        "UNEXPECTED_TOOL_CALL",
        "TOO_MANY_TOOL_CALLS",
        "MISSING_THOUGHT_SIGNATURE",
        "MALFORMED_RESPONSE",
        "NO_IMAGE",
        "IMAGE_OTHER",
        "OTHER",
    ] {
        let classified = classify_gemini_terminal(&json!({
            "candidates":[{"index":0,"finishReason":reason}]
        }));
        assert_eq!(
            classified.kind,
            GeminiTerminalKind::ApplicationFailure,
            "{reason}"
        );
        assert_eq!(classified.reason_code, None, "{reason}");
    }

    for reason in [
        "SAFETY",
        "BLOCKLIST",
        "PROHIBITED_CONTENT",
        "IMAGE_SAFETY",
        "JAILBREAK",
        "MODEL_ARMOR",
        "OTHER",
    ] {
        let classified = classify_gemini_terminal(&json!({
            "promptFeedback":{"blockReason":reason}
        }));
        assert_eq!(classified.kind, GeminiTerminalKind::PromptBlock, "{reason}");
    }
}

#[test]
fn gemini_terminal_classifier_degrades_unknown_missing_unspecified_and_cardinality() {
    for (case, response, reason_code) in [
        (
            "missing",
            json!({"candidates":[{"index":0}]}),
            TransformReasonCode::IllegalUpstreamTerminal,
        ),
        (
            "unspecified",
            json!({"candidates":[{"index":0,"finishReason":"FINISH_REASON_UNSPECIFIED"}]}),
            TransformReasonCode::IllegalUpstreamTerminal,
        ),
        (
            "unknown",
            json!({"candidates":[{"index":0,"finishReason":"FUTURE_REASON"}]}),
            TransformReasonCode::UnknownStopReason,
        ),
        (
            "tool-use-is-not-a-gemini-finish",
            json!({"candidates":[{"index":0,"finishReason":"TOOL_USE"}]}),
            TransformReasonCode::UnknownStopReason,
        ),
        (
            "empty",
            json!({"candidates":[]}),
            TransformReasonCode::IllegalUpstreamTerminal,
        ),
        (
            "multi-candidate",
            json!({"candidates":[
                {"index":0,"finishReason":"STOP"},
                {"index":1,"finishReason":"STOP"}
            ]}),
            TransformReasonCode::UnsupportedContent,
        ),
        (
            "nonzero-index",
            json!({"candidates":[{"index":1,"finishReason":"STOP"}]}),
            TransformReasonCode::UnsupportedContent,
        ),
        (
            "unknown-block",
            json!({"promptFeedback":{"blockReason":"FUTURE_BLOCK"}}),
            TransformReasonCode::UnknownStopReason,
        ),
    ] {
        let classified = classify_gemini_terminal(&response);
        assert_eq!(
            classified.kind,
            GeminiTerminalKind::ObservationDegraded,
            "{case}"
        );
        assert_eq!(classified.reason_code, Some(reason_code), "{case}");
    }
}

#[test]
fn gemini_prompt_block_and_stop_tool_call_map_without_inventing_tool_use() {
    let prompt_block: UnifiedResponse = serde_json::from_value::<GeminiResponse>(json!({
        "promptFeedback":{"blockReason":"JAILBREAK"}
    }))
    .expect("prompt block payload")
    .into();
    assert_eq!(prompt_block.choices.len(), 1);
    assert_eq!(
        prompt_block.choices[0].finish_reason.as_deref(),
        Some("content_filter")
    );
    assert!(prompt_block.choices[0].message.content.is_empty());

    let tool_call: UnifiedResponse = serde_json::from_value::<GeminiResponse>(json!({
        "responseId":"response-tool",
        "candidates":[{
            "index":0,
            "content":{"role":"model","parts":[{
                "functionCall":{"id":"call-1","name":"lookup","args":{}}
            }]},
            "finishReason":"STOP"
        }]
    }))
    .expect("tool response")
    .into();
    assert_eq!(
        tool_call.choices[0].finish_reason.as_deref(),
        Some("tool_calls")
    );
    assert_eq!(map_openai_finish_reason_to_gemini("tool_calls"), "STOP");
}

#[test]
fn test_gemini_request_to_unified() {
    let gemini_req = GeminiRequestPayload {
        contents: vec![GeminiRequestContent {
            role: Some("user".to_string()),
            parts: vec![GeminiPart::Text {
                text: "Hello".to_string(),
            }],
        }],
        system_instruction: Some(GeminiSystemInstruction::String(
            "You are a helpful assistant.".to_string(),
        )),
        tools: None,
        tool_config: None,
        generation_config: Some(GeminiGenerationConfig {
            candidate_count: None,
            temperature: Some(0.8),
            max_output_tokens: Some(100),
            top_p: Some(0.9),
            seed: Some(7),
            presence_penalty: Some(0.1),
            frequency_penalty: Some(-0.2),
            top_k: Some(32),
            stop_sequences: Some(vec!["stop".to_string()]),
            thinking_config: None,
            response_mime_type: None,
            response_schema: None,
            response_json_schema: None,
            response_format: None,
        }),
        safety_settings: None,
    };

    let unified_req: UnifiedRequest = gemini_req.into();

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
    assert_eq!(unified_req.seed, Some(7));
    assert_eq!(unified_req.presence_penalty, Some(0.1));
    assert_eq!(unified_req.frequency_penalty, Some(-0.2));
    assert_eq!(unified_req.top_k(), Some(32));
}

#[test]
fn test_unified_request_to_gemini() {
    let unified_req = UnifiedRequest {
        model: Some("test-model".to_string()),
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
        seed: Some(7),
        presence_penalty: Some(0.1),
        frequency_penalty: Some(-0.2),
        extensions: Some(UnifiedRequestExtensions {
            anthropic: Some(UnifiedAnthropicRequestExtension {
                top_k: Some(32),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let gemini_req: GeminiRequestPayload = unified_req.into();

    assert!(gemini_req.system_instruction.is_some());
    let system_instruction = gemini_req.system_instruction.unwrap();
    match system_instruction {
        GeminiSystemInstruction::String(text) => {
            assert_eq!(text, "You are a helpful assistant.");
        }
        GeminiSystemInstruction::Object { parts } => {
            assert_eq!(parts.len(), 1);
            if let GeminiPart::Text { text } = &parts[0] {
                assert_eq!(text, "You are a helpful assistant.");
            } else {
                panic!("Expected text part in system instruction");
            }
        }
    }

    assert_eq!(gemini_req.contents.len(), 1);
    assert_eq!(gemini_req.contents[0].role, Some("user".to_string()));
    assert_eq!(gemini_req.contents[0].parts.len(), 1);
    if let GeminiPart::Text { text } = &gemini_req.contents[0].parts[0] {
        assert_eq!(text, "Hello");
    } else {
        panic!("Expected text part in user content");
    }

    assert!(gemini_req.generation_config.is_some());
    let config = gemini_req.generation_config.unwrap();
    assert_eq!(config.temperature, Some(0.8));
    assert_eq!(config.max_output_tokens, Some(100));
    assert_eq!(config.top_p, Some(0.9));
    assert_eq!(config.stop_sequences, Some(vec!["stop".to_string()]));
    assert_eq!(config.seed, Some(7));
    assert_eq!(config.presence_penalty, Some(0.1));
    assert_eq!(config.frequency_penalty, Some(-0.2));
    assert_eq!(config.top_k, Some(32));
}

#[test]
fn test_unified_request_to_gemini_encodes_inline_image_without_remote_text_downgrade() {
    let unified_req = UnifiedRequest {
        messages: vec![UnifiedMessage {
            role: UnifiedRole::User,
            content: vec![
                UnifiedContentPart::Text {
                    text: "Describe this".to_string(),
                },
                UnifiedContentPart::ImageUrl {
                    url: "data:image/png;base64,ZmFrZQ==".to_string(),
                    detail: Some("high".to_string()),
                },
                UnifiedContentPart::ImageUrl {
                    url: "https://example.com/cat.png".to_string(),
                    detail: None,
                },
            ],
        }],
        ..Default::default()
    };

    let gemini_req: GeminiRequestPayload = unified_req.into();

    assert_eq!(gemini_req.contents.len(), 1);
    assert_eq!(gemini_req.contents[0].parts.len(), 2);
    assert!(matches!(
        &gemini_req.contents[0].parts[0],
        GeminiPart::Text { text } if text == "Describe this"
    ));
    assert!(matches!(
        &gemini_req.contents[0].parts[1],
        GeminiPart::InlineData { inline_data }
            if inline_data.mime_type == "image/png" && inline_data.data == "ZmFrZQ=="
    ));
}

#[test]
fn test_unified_request_to_gemini_recovers_tool_result_name_from_tool_call_id() {
    let unified_req = UnifiedRequest {
        messages: vec![
            UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::ToolCall(UnifiedToolCall {
                    id: "call_123".to_string(),
                    name: "get_current_weather".to_string(),
                    arguments: json!({ "location": "Boston" }),
                })],
            },
            UnifiedMessage {
                role: UnifiedRole::Tool,
                content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                    tool_call_id: "call_123".to_string(),
                    name: None,
                    output: UnifiedToolResultOutput::Json {
                        value: json!({"temperature": 22}),
                    },
                })],
            },
        ],
        ..Default::default()
    };

    let gemini_req: GeminiRequestPayload = unified_req.into();

    assert_eq!(gemini_req.contents.len(), 2);
    match &gemini_req.contents[1].parts[0] {
        GeminiPart::FunctionResponse { function_response } => {
            assert_eq!(function_response.name, "get_current_weather");
            assert_eq!(function_response.response, json!({ "temperature": 22 }));
        }
        other => panic!("Expected function response part, got {:?}", other),
    }
}

#[test]
fn test_unified_request_to_gemini_preserves_tool_result_with_synthetic_fallback_name() {
    let unified_req = UnifiedRequest {
        messages: vec![UnifiedMessage {
            role: UnifiedRole::Tool,
            content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                tool_call_id: "call:123".to_string(),
                name: None,
                output: UnifiedToolResultOutput::Json {
                    value: json!({"temperature": 22}),
                },
            })],
        }],
        ..Default::default()
    };

    let gemini_req: GeminiRequestPayload = unified_req.into();

    assert_eq!(gemini_req.contents.len(), 1);
    match &gemini_req.contents[0].parts[0] {
        GeminiPart::FunctionResponse { function_response } => {
            assert_eq!(function_response.name, "gemini-tool-result-call_123");
            assert_eq!(function_response.response, json!({ "temperature": 22 }));
        }
        other => panic!("Expected function response part, got {:?}", other),
    }
}

#[test]
fn test_gemini_request_to_unified_preserves_structured_tool_result_output() {
    let gemini_req = GeminiRequestPayload {
        contents: vec![GeminiRequestContent {
            role: Some("user".to_string()),
            parts: vec![GeminiPart::FunctionResponse {
                function_response: GeminiFunctionResponse {
                    id: None,
                    name: "lookup_weather".to_string(),
                    response: json!({
                        "result": [
                            {"type": "text", "text": "hello"},
                            {
                                "type": "file",
                                "filename": "report.pdf",
                                "file_url": "https://files.example.com/report.pdf"
                            }
                        ]
                    }),
                },
            }],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    };

    let unified_req: UnifiedRequest = gemini_req.into();

    assert_eq!(unified_req.messages.len(), 1);
    match &unified_req.messages[0].content[0] {
        UnifiedContentPart::ToolResult(result) => {
            assert_eq!(result.name.as_deref(), Some("lookup_weather"));
            assert!(matches!(
                &result.output,
                UnifiedToolResultOutput::Content { parts }
                if matches!(&parts[0], UnifiedToolResultPart::Text { text } if text == "hello")
                    && matches!(
                        &parts[1],
                        UnifiedToolResultPart::File { filename, file_url }
                        if filename.as_deref() == Some("report.pdf")
                            && file_url.as_deref()
                                == Some("https://files.example.com/report.pdf")
                    )
            ));
        }
        other => panic!("Expected tool result, got {:?}", other),
    }
}

#[test]
fn explicit_gemini_function_response_id_is_consumed_before_name_fallback() {
    let request: GeminiRequestPayload = serde_json::from_value(json!({
        "contents": [
            {"role":"model","parts":[
                {"functionCall":{"id":"call-1","name":"lookup","args":{}}},
                {"functionCall":{"id":"call-2","name":"lookup","args":{}}}
            ]},
            {"role":"user","parts":[
                {"functionResponse":{"id":"call-1","name":"lookup","response":{"order":1}}},
                {"functionResponse":{"name":"lookup","response":{"order":2}}}
            ]}
        ]
    }))
    .expect("Gemini tool history");

    let unified: UnifiedRequest = request.into();
    let output_ids = unified
        .items
        .iter()
        .filter_map(|item| match item {
            UnifiedItem::FunctionCallOutput(output) => Some(output.tool_call_id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(output_ids, vec!["call-1", "call-2"]);
}

#[test]
fn test_unified_request_to_gemini_preserves_reasoning_and_executable_code() {
    let unified_req = UnifiedRequest {
        messages: vec![
            UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::Reasoning {
                        text: "step by step".to_string(),
                    },
                    UnifiedContentPart::ExecutableCode {
                        language: "python".to_string(),
                        code: "print('hi')".to_string(),
                    },
                ],
            },
            UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::Reasoning {
                    text: "internal summary".to_string(),
                }],
            },
        ],
        ..Default::default()
    };

    let gemini_req: GeminiRequestPayload = unified_req.into();
    assert_eq!(gemini_req.contents.len(), 2);
    assert!(matches!(
        &gemini_req.contents[0].parts[0],
        GeminiPart::Thought { text, thought: true, .. } if text == "step by step"
    ));
    assert!(matches!(
        &gemini_req.contents[0].parts[1],
        GeminiPart::ExecutableCode { executable_code }
        if executable_code.language == "python" && executable_code.code == "print('hi')"
    ));
    assert!(matches!(
        &gemini_req.contents[1].parts[0],
        GeminiPart::Thought { text, thought: true, .. } if text == "internal summary"
    ));
}

#[test]
fn test_unified_request_to_gemini_preserves_user_assistant_and_tool_fallback_content() {
    let unified_req = UnifiedRequest {
        messages: vec![
            UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id: "call_user".to_string(),
                        name: "lookup_weather".to_string(),
                        arguments: json!({ "city": "Boston" }),
                    }),
                    UnifiedContentPart::ToolResult(UnifiedToolResult {
                        tool_call_id: "call_user".to_string(),
                        name: None,
                        output: UnifiedToolResultOutput::Json {
                            value: json!({"ok": true}),
                        },
                    }),
                ],
            },
            UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![
                    UnifiedContentPart::ImageUrl {
                        url: "https://example.com/chart.png".to_string(),
                        detail: Some("high".to_string()),
                    },
                    UnifiedContentPart::ToolResult(UnifiedToolResult {
                        tool_call_id: "call_assistant".to_string(),
                        name: Some("summarize".to_string()),
                        output: UnifiedToolResultOutput::Json {
                            value: json!({"summary": "done"}),
                        },
                    }),
                ],
            },
            UnifiedMessage {
                role: UnifiedRole::Tool,
                content: vec![
                    UnifiedContentPart::Text {
                        text: "tool text".to_string(),
                    },
                    UnifiedContentPart::ImageData {
                        mime_type: "image/png".to_string(),
                        data: "ZmFrZQ==".to_string(),
                    },
                    UnifiedContentPart::FileUrl {
                        url: "https://files.example.com/report.pdf".to_string(),
                        mime_type: Some("application/pdf".to_string()),
                        filename: None,
                    },
                ],
            },
        ],
        ..Default::default()
    };

    let gemini_req: GeminiRequestPayload = unified_req.into();

    assert!(matches!(
        &gemini_req.contents[0].parts[0],
        GeminiPart::Text { text }
        if text == "tool_call: lookup_weather\narguments: {\"city\":\"Boston\"}"
    ));
    assert!(matches!(
        &gemini_req.contents[0].parts[1],
        GeminiPart::FunctionResponse { function_response }
        if function_response.name == "lookup_weather"
            && function_response.response == json!({"ok": true})
    ));
    assert!(matches!(
        &gemini_req.contents[1].parts[0],
        GeminiPart::Text { text }
        if text == "tool_result: summarize\ntool_call_id: call_assistant\ncontent: {\"summary\":\"done\"}"
    ));
    assert!(matches!(
        &gemini_req.contents[2].parts[0],
        GeminiPart::Text { text } if text == "tool text"
    ));
    assert_eq!(gemini_req.contents[2].parts.len(), 1);
}

#[test]
fn test_unified_request_to_gemini_preserves_inline_file_data_as_inline_data() {
    let unified_req = UnifiedRequest {
        messages: vec![UnifiedMessage {
            role: UnifiedRole::User,
            content: vec![UnifiedContentPart::FileData {
                data: "dGVzdA==".to_string(),
                mime_type: "application/pdf".to_string(),
                filename: Some("report.pdf".to_string()),
            }],
        }],
        ..Default::default()
    };

    let gemini_req: GeminiRequestPayload = unified_req.into();

    assert!(matches!(
        &gemini_req.contents[0].parts[0],
        GeminiPart::InlineData { inline_data }
        if inline_data.mime_type == "application/pdf" && inline_data.data == "dGVzdA=="
    ));
}

#[test]
fn test_gemini_response_to_unified() {
    let gemini_res = GeminiResponse {
        response_id: None,
        candidates: vec![GeminiCandidate {
            index: Some(0),
            content: Some(GeminiResponseContent {
                role: "model".to_string(),
                parts: vec![GeminiPart::Text {
                    text: "Hi there!".to_string(),
                }],
            }),
            finish_reason: Some("STOP".to_string()),
            safety_ratings: None,
            token_count: Some(20),
            citation_metadata: Some(GeminiCitationMetadata {
                citation_sources: vec![GeminiCitationSource {
                    start_index: Some(0),
                    end_index: Some(4),
                    uri: Some("https://example.com".to_string()),
                    license: None,
                }],
            }),
        }],
        prompt_feedback: None,
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
            thoughts_token_count: None,
            cached_content_token_count: None,
            tool_use_prompt_token_count: None,
            prompt_tokens_details: vec![],
            cache_tokens_details: vec![],
            candidates_tokens_details: vec![],
            tool_use_prompt_tokens_details: vec![],
        }),
        synthetic_metadata: None,
    };

    let unified_res: UnifiedResponse = gemini_res.into();

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
    let usage = unified_res.usage.as_ref().unwrap();
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 20);
    assert_eq!(usage.total_tokens, 30);
    assert!(unified_res.id.starts_with("gemini-response-"));
    assert_eq!(unified_res.model.as_deref(), Some("gemini"));
    let synthetic = unified_res.synthetic_metadata().unwrap();
    assert!(synthetic.id);
    assert!(synthetic.model);
    assert!(!synthetic.gemini_safety_ratings);
    let metadata = unified_res.provider_response_metadata().unwrap();
    let gemini_metadata = metadata.gemini.as_ref().unwrap();
    assert_eq!(gemini_metadata.candidates.len(), 1);
    assert_eq!(gemini_metadata.candidates[0].token_count, Some(20));
    assert!(matches!(
        &unified_res.choices[0].items[0],
        UnifiedItem::Message(UnifiedMessageItem { annotations, .. })
        if matches!(
            &annotations[..],
            [UnifiedAnnotation::Citation(UnifiedCitation { url, start_index, end_index, .. })]
            if url.as_deref() == Some("https://example.com")
            && *start_index == Some(0)
            && *end_index == Some(4)
        )
    ));
}

#[test]
fn test_gemini_response_to_unified_preserves_inline_file_data_as_typed_file() {
    let gemini_res = GeminiResponse {
        response_id: None,
        candidates: vec![GeminiCandidate {
            index: Some(0),
            content: Some(GeminiResponseContent {
                role: "model".to_string(),
                parts: vec![GeminiPart::InlineData {
                    inline_data: GeminiInlineData {
                        mime_type: "application/pdf".to_string(),
                        data: "dGVzdA==".to_string(),
                        display_name: None,
                    },
                }],
            }),
            finish_reason: Some("STOP".to_string()),
            safety_ratings: None,
            token_count: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        synthetic_metadata: None,
    };

    let unified_res: UnifiedResponse = gemini_res.into();
    assert!(matches!(
        &unified_res.choices[0].message.content[0],
        UnifiedContentPart::FileData { mime_type, data, .. }
        if mime_type == "application/pdf" && data == "dGVzdA=="
    ));
}

#[test]
fn test_unified_response_to_gemini_prefers_typed_items_for_file_and_tool_output() {
    let unified_res = UnifiedResponse {
        id: "chatcmpl-123".to_string(),
        model: Some("gpt-4".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![],
                ..Default::default()
            },
            items: vec![
                UnifiedItem::FileReference(UnifiedFileReferenceItem {
                    filename: Some("report.pdf".to_string()),
                    mime_type: Some("application/pdf".to_string()),
                    file_url: Some("https://files.example.com/report.pdf".to_string()),
                    file_id: None,
                }),
                UnifiedItem::FunctionCallOutput(UnifiedFunctionCallOutputItem {
                    tool_call_id: "call_123".to_string(),
                    name: Some("lookup_weather".to_string()),
                    output: UnifiedToolResultOutput::Json {
                        value: json!({"temperature": 22}),
                    },
                }),
            ],
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: None,
        created: None,
        object: None,
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let gemini_res: GeminiResponse = unified_res.into();
    let parts = &gemini_res.candidates[0].content.as_ref().unwrap().parts;
    assert!(matches!(
        &parts[0],
        GeminiPart::FileData { file_data }
        if file_data.file_uri == "https://files.example.com/report.pdf"
            && file_data.mime_type == "application/pdf"
    ));
    assert!(matches!(
        &parts[1],
        GeminiPart::FunctionResponse { function_response }
        if function_response.name == "lookup_weather"
            && function_response.response == json!({"temperature": 22})
    ));
}

#[test]
fn test_unified_response_to_gemini() {
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
        created: Some(1234567890),
        object: Some("chat.completion".to_string()),
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let gemini_res: GeminiResponse = unified_res.into();

    assert_eq!(gemini_res.candidates.len(), 1);
    let candidate = &gemini_res.candidates[0];
    assert!(candidate.content.is_some());
    let content = candidate.content.as_ref().unwrap();
    assert_eq!(content.role, "model");
    assert_eq!(content.parts.len(), 1);
    if let GeminiPart::Text { text } = &content.parts[0] {
        assert_eq!(text, "Hi there!");
    } else {
        panic!("Expected text part");
    }
    assert_eq!(candidate.finish_reason, Some("STOP".to_string()));

    assert!(gemini_res.usage_metadata.is_some());
    let usage = gemini_res.usage_metadata.unwrap();
    assert_eq!(usage.prompt_token_count, 10);
    assert_eq!(usage.candidates_token_count, 20);
    assert_eq!(usage.total_token_count, 30);
    assert!(gemini_res.synthetic_metadata.is_none());
}

#[test]
fn test_unified_response_to_gemini_restores_citation_metadata_from_structured_annotations() {
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
            items: vec![UnifiedItem::Message(UnifiedMessageItem {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::Text {
                    text: "Hi there!".to_string(),
                }],
                annotations: vec![UnifiedAnnotation::Citation(UnifiedCitation {
                    part_index: None,
                    start_index: Some(0),
                    end_index: Some(4),
                    url: Some("https://example.com".to_string()),
                    title: None,
                    license: Some("CC-BY".to_string()),
                })],
            })],
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: None,
        created: Some(1234567890),
        object: Some("chat.completion".to_string()),
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let gemini_res: GeminiResponse = unified_res.into();

    let citation_metadata = gemini_res.candidates[0].citation_metadata.as_ref().unwrap();
    assert_eq!(citation_metadata.citation_sources.len(), 1);
    assert_eq!(
        citation_metadata.citation_sources[0].uri.as_deref(),
        Some("https://example.com")
    );
    assert_eq!(citation_metadata.citation_sources[0].start_index, Some(0));
    assert_eq!(citation_metadata.citation_sources[0].end_index, Some(4));
    assert_eq!(
        citation_metadata.citation_sources[0].license.as_deref(),
        Some("CC-BY")
    );
}

#[test]
fn test_unified_response_to_gemini_preserves_synthetic_metadata() {
    let unified_res = UnifiedResponse {
        id: "chatcmpl-123".to_string(),
        model: None,
        choices: vec![],
        usage: None,
        created: None,
        object: None,
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: Some(UnifiedSyntheticMetadata {
            id: true,
            model: false,
            gemini_safety_ratings: false,
        }),
    };

    let gemini_res: GeminiResponse = unified_res.into();

    assert!(gemini_res.synthetic_metadata.is_some());
    assert!(gemini_res.synthetic_metadata.as_ref().unwrap().id);
}

#[test]
fn test_unified_response_to_gemini_preserves_provider_metadata() {
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
        usage: None,
        created: None,
        object: None,
        system_fingerprint: None,
        provider_response_metadata: Some(UnifiedProviderResponseMetadata {
            gemini: Some(UnifiedGeminiResponseMetadata {
                prompt_feedback: Some(UnifiedGeminiPromptFeedback {
                    block_reason: Some("SAFETY".to_string()),
                    safety_ratings: vec![UnifiedGeminiSafetyRating {
                        category: "HARM_CATEGORY_HATE_SPEECH".to_string(),
                        probability: "LOW".to_string(),
                    }],
                }),
                candidates: vec![UnifiedGeminiCandidateMetadata {
                    index: 0,
                    thought_signature_present: false,
                    tool_associations: Vec::new(),
                    safety_ratings: vec![UnifiedGeminiSafetyRating {
                        category: "HARM_CATEGORY_HARASSMENT".to_string(),
                        probability: "NEGLIGIBLE".to_string(),
                    }],
                    citation_metadata: Some(UnifiedGeminiCitationMetadata {
                        citation_sources: vec![UnifiedGeminiCitationSource {
                            start_index: Some(0),
                            end_index: Some(5),
                            uri: Some("https://example.com".to_string()),
                            license: Some("CC-BY".to_string()),
                        }],
                    }),
                    token_count: Some(7),
                }],
            }),
            ..Default::default()
        }),
        synthetic_metadata: None,
    };

    let gemini_res: GeminiResponse = unified_res.into();
    assert_eq!(
        gemini_res
            .prompt_feedback
            .as_ref()
            .and_then(|f| f.block_reason.as_deref()),
        Some("SAFETY")
    );
    assert_eq!(
        gemini_res.candidates[0].safety_ratings.as_ref().unwrap()[0].category,
        "HARM_CATEGORY_HARASSMENT"
    );
    assert_eq!(
        gemini_res.candidates[0]
            .citation_metadata
            .as_ref()
            .unwrap()
            .citation_sources[0]
            .uri
            .as_deref(),
        Some("https://example.com")
    );
    assert_eq!(gemini_res.candidates[0].token_count, Some(7));
}

#[test]
fn test_gemini_chunk_to_unified() {
    let gemini_chunk = GeminiChunkResponse {
        response_id: None,
        candidates: vec![GeminiCandidate {
            index: Some(0),
            content: Some(GeminiResponseContent {
                role: "model".to_string(),
                parts: vec![GeminiPart::Text {
                    text: "Hello".to_string(),
                }],
            }),
            finish_reason: None,
            safety_ratings: None,
            token_count: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        synthetic_metadata: None,
    };

    let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

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
    assert!(unified_chunk.id.starts_with("gemini-chunk-"));
    assert_eq!(unified_chunk.model.as_deref(), Some("gemini"));
    let synthetic = unified_chunk.synthetic_metadata().unwrap();
    assert!(synthetic.id);
    assert!(synthetic.model);
    assert!(!synthetic.gemini_safety_ratings);
}

#[test]
fn test_unified_chunk_to_gemini() {
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
        created: Some(1234567890),
        object: Some("chat.completion.chunk".to_string()),
        provider_session_metadata: None,
        synthetic_metadata: None,
    };

    let gemini_chunk: GeminiChunkResponse = unified_chunk.into();

    assert_eq!(gemini_chunk.candidates.len(), 1);
    let candidate = &gemini_chunk.candidates[0];
    assert!(candidate.content.is_some());
    let content = candidate.content.as_ref().unwrap();
    assert_eq!(content.role, "model");
    assert_eq!(content.parts.len(), 1);
    if let GeminiPart::Text { text } = &content.parts[0] {
        assert_eq!(text, "Hello");
    } else {
        panic!("Expected text part");
    }
    assert!(candidate.finish_reason.is_none());
    assert!(gemini_chunk.synthetic_metadata.is_none());
}

#[test]
fn test_unified_chunk_to_gemini_preserves_synthetic_metadata() {
    let unified_chunk = UnifiedChunkResponse {
        id: "chatcmpl-123".to_string(),
        model: None,
        choices: vec![],
        usage: None,
        created: None,
        object: None,
        provider_session_metadata: None,
        synthetic_metadata: Some(UnifiedSyntheticMetadata {
            id: true,
            model: false,
            gemini_safety_ratings: false,
        }),
    };

    let gemini_chunk: GeminiChunkResponse = unified_chunk.into();

    assert!(gemini_chunk.synthetic_metadata.is_some());
    assert!(gemini_chunk.synthetic_metadata.as_ref().unwrap().id);
}

#[test]
fn test_gemini_response_to_unified_with_thinking() {
    let gemini_res = GeminiResponse {
        response_id: Some("response-thinking".to_string()),
        candidates: vec![GeminiCandidate {
            index: Some(0),
            content: Some(GeminiResponseContent {
                role: "model".to_string(),
                parts: vec![
                    GeminiPart::Thought {
                        text: "I should call a tool".to_string(),
                        thought: true,
                        thought_signature: None,
                    },
                    GeminiPart::FunctionCall {
                        function_call: GeminiFunctionCall {
                            id: None,
                            name: "get_weather".to_string(),
                            args: json!({"location": "Boston"}),
                        },
                        thought_signature: None,
                    },
                ],
            }),
            finish_reason: Some("STOP".to_string()),
            safety_ratings: None,
            token_count: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        synthetic_metadata: None,
    };

    let unified_res: UnifiedResponse = gemini_res.into();

    assert_eq!(unified_res.choices.len(), 1);
    let choice = &unified_res.choices[0];
    assert_eq!(choice.message.role, UnifiedRole::Assistant);
    assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));

    match &choice.message.content[0] {
        UnifiedContentPart::Reasoning { text } => assert_eq!(text, "I should call a tool"),
        _ => panic!("Expected reasoning content"),
    }
    match &choice.message.content[1] {
        UnifiedContentPart::ToolCall(tc) => {
            assert_eq!(tc.name, "get_weather");
        }
        _ => panic!("Expected tool call content"),
    }
}

#[test]
fn test_unified_response_to_gemini_with_thinking() {
    let unified_res = UnifiedResponse {
        id: "chatcmpl-123".to_string(),
        model: Some("gpt-4".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![
                    UnifiedContentPart::Reasoning {
                        text: "I will call a tool".to_string(),
                    },
                    UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id: "call_123".to_string(),
                        name: "get_weather".to_string(),
                        arguments: json!({"location": "Boston"}),
                    }),
                ],
                ..Default::default()
            },
            items: Vec::new(),
            finish_reason: Some("tool_calls".to_string()),
            logprobs: None,
        }],
        usage: None,
        created: None,
        object: None,
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let gemini_res: GeminiResponse = unified_res.into();

    assert_eq!(gemini_res.candidates.len(), 1);
    let candidate = &gemini_res.candidates[0];
    assert!(candidate.content.is_some());
    let content = candidate.content.as_ref().unwrap();
    assert_eq!(content.role, "model");
    assert_eq!(content.parts.len(), 2);
    assert!(matches!(
        &content.parts[0],
        GeminiPart::Thought { text, thought: true, .. } if text == "I will call a tool"
    ));
    assert!(
        matches!(&content.parts[1], GeminiPart::FunctionCall { function_call, .. } if function_call.name == "get_weather")
    );
    assert_eq!(candidate.finish_reason, Some("STOP".to_string()));
}

#[test]
fn test_transform_unified_chunk_to_gemini_events_keeps_diagnostic_internal_for_image_delta() {
    let unified_chunk = UnifiedChunkResponse {
        id: "cmpl-123".to_string(),
        model: Some("gemini-2.0-flash".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![
                    UnifiedContentPartDelta::ImageDelta {
                        index: 2,
                        url: None,
                        data: Some("ZmFrZQ==".to_string()),
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

    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini);
    let events =
        transform_unified_chunk_to_gemini_events(unified_chunk, &mut transformer.stream_context())
            .expect("gemini chunk events");

    assert_eq!(events.len(), 1);
    assert_ne!(events[0].event.as_deref(), Some("transform_diagnostic"));
    let chunk: Value = serde_json::from_str(&events[0].data).unwrap();
    assert_eq!(
        chunk["candidates"][0]["content"]["parts"][0]["text"],
        json!("caption")
    );
    let summary = transformer.diagnostics_snapshot();
    assert_eq!(summary.total_fact_count, 1);
    assert_eq!(
        summary.facts[0].semantic_unit,
        crate::service::transform::TransformSemanticUnit::ImageDelta
    );
}

#[test]
fn test_unified_reasoning_events_emit_native_gemini_thought_parts() {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini);
    let events = transform_unified_stream_events_to_gemini_events(
        vec![
            UnifiedStreamEvent::ReasoningStart { index: 0 },
            UnifiedStreamEvent::ReasoningDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "considering the answer".to_string(),
            },
            UnifiedStreamEvent::ReasoningStop { index: 0 },
        ],
        &mut transformer.stream_context(),
    )
    .expect("Gemini reasoning delta should produce one thought chunk");

    assert_eq!(events.len(), 1);
    let chunk: Value = serde_json::from_str(&events[0].data).unwrap();
    assert_eq!(
        chunk.pointer("/candidates/0/content/parts/0/text"),
        Some(&json!("considering the answer"))
    );
    assert_eq!(
        chunk.pointer("/candidates/0/content/parts/0/thought"),
        Some(&json!(true))
    );
    assert!(transformer.diagnostics_snapshot().facts.is_empty());
}

#[test]
fn test_gemini_chunk_to_unified_with_thinking() {
    let gemini_chunk = GeminiChunkResponse {
        response_id: Some("response-thinking".to_string()),
        candidates: vec![GeminiCandidate {
            index: Some(0),
            content: Some(GeminiResponseContent {
                role: "model".to_string(),
                parts: vec![
                    GeminiPart::Text {
                        text: "Thinking...".to_string(),
                    },
                    GeminiPart::FunctionCall {
                        function_call: GeminiFunctionCall {
                            id: None,
                            name: "search".to_string(),
                            args: json!({"query": "stuff"}),
                        },
                        thought_signature: None,
                    },
                ],
            }),
            finish_reason: None,
            safety_ratings: None,
            token_count: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        synthetic_metadata: None,
    };

    let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

    assert_eq!(unified_chunk.choices.len(), 1);
    let choice = &unified_chunk.choices[0];
    assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));

    match &choice.delta.content[0] {
        UnifiedContentPartDelta::TextDelta { text, .. } => assert_eq!(text, "Thinking..."),
        _ => panic!("Expected text delta"),
    }

    match &choice.delta.content[1] {
        UnifiedContentPartDelta::ToolCallDelta(tc) => {
            assert!(
                tc.id
                    .as_deref()
                    .is_some_and(|id| id.starts_with("gemini-call-"))
            );
            assert_eq!(tc.name, Some("search".to_string()));
        }
        _ => panic!("Expected tool call delta"),
    }
}

#[test]
fn test_unified_chunk_to_gemini_with_thinking() {
    let unified_chunk = UnifiedChunkResponse {
        id: "chatcmpl-123".to_string(),
        model: Some("gpt-4".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![
                    UnifiedContentPartDelta::TextDelta {
                        index: 0,
                        text: "Thinking...".to_string(),
                    },
                    UnifiedContentPartDelta::ToolCallDelta(UnifiedToolCallDelta {
                        index: 0,
                        id: Some("call_123".to_string()),
                        name: Some("search".to_string()),
                        arguments: Some(json!({"query": "stuff"}).to_string()),
                    }),
                ],
            },
            finish_reason: None,
        }],
        usage: None,
        created: Some(1234567890),
        object: Some("chat.completion.chunk".to_string()),
        provider_session_metadata: None,
        synthetic_metadata: None,
    };

    let gemini_chunk: GeminiChunkResponse = unified_chunk.into();

    assert_eq!(gemini_chunk.candidates.len(), 1);
    let candidate = &gemini_chunk.candidates[0];
    assert!(candidate.content.is_some());
    let content = candidate.content.as_ref().unwrap();
    assert_eq!(content.role, "model");
    assert_eq!(content.parts.len(), 2);
    assert!(matches!(&content.parts[0], GeminiPart::Text { text } if text == "Thinking..."));
    assert!(
        matches!(&content.parts[1], GeminiPart::FunctionCall { function_call, .. } if function_call.name == "search")
    );
}

#[test]
fn test_gemini_response_to_unified_with_executable_code() {
    let gemini_res = GeminiResponse {
        response_id: None,
        candidates: vec![GeminiCandidate {
            index: Some(0),
            content: Some(GeminiResponseContent {
                role: "model".to_string(),
                parts: vec![GeminiPart::ExecutableCode {
                    executable_code: GeminiExecutableCode {
                        language: "PYTHON".to_string(),
                        code: "print('Hello World')".to_string(),
                    },
                }],
            }),
            finish_reason: Some("STOP".to_string()),
            safety_ratings: None,
            token_count: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        synthetic_metadata: None,
    };

    let unified_res: UnifiedResponse = gemini_res.into();

    assert_eq!(unified_res.choices.len(), 1);
    let choice = &unified_res.choices[0];
    assert_eq!(choice.message.role, UnifiedRole::Assistant);
    assert_eq!(choice.finish_reason, Some("stop".to_string()));

    match &choice.message.content[0] {
        UnifiedContentPart::ExecutableCode { language, code } => {
            assert_eq!(language, "PYTHON");
            assert_eq!(code, "print('Hello World')");
        }
        _ => panic!("Expected executable code content"),
    }
    assert!(matches!(
        &choice.items[0],
        UnifiedItem::Message(UnifiedMessageItem { content, .. })
        if matches!(
            &content[0],
            UnifiedContentPart::ExecutableCode { language, code }
            if language == "PYTHON" && code == "print('Hello World')"
        )
    ));
}

#[test]
fn test_gemini_chunk_to_unified_with_executable_code() {
    let gemini_chunk = GeminiChunkResponse {
        response_id: None,
        candidates: vec![GeminiCandidate {
            index: Some(0),
            content: Some(GeminiResponseContent {
                role: "model".to_string(),
                parts: vec![GeminiPart::ExecutableCode {
                    executable_code: GeminiExecutableCode {
                        language: "PYTHON".to_string(),
                        code: "print('Hello')".to_string(),
                    },
                }],
            }),
            finish_reason: None,
            safety_ratings: None,
            token_count: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        synthetic_metadata: None,
    };

    let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

    assert_eq!(unified_chunk.choices.len(), 1);
    let choice = &unified_chunk.choices[0];
    assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));

    match &choice.delta.content[0] {
        UnifiedContentPartDelta::ToolCallDelta(tc) => {
            assert_eq!(tc.name, Some("code_interpreter".to_string()));
            assert_eq!(
                tc.arguments,
                Some(json!({"language": "PYTHON", "code": "print('Hello')"}).to_string())
            );
        }
        _ => panic!("Expected tool call delta"),
    }
}

#[test]
fn gemini_tool_ids_and_signatures_survive_ir_and_legacy_ids_are_deterministic_across_modes() {
    const SIGNATURE: &str = "opaque-tool-signature";
    let legacy = json!({
        "responseId":"response-tool-seed",
        "candidates":[{
            "index":0,
            "content":{"role":"model","parts":[{
                "functionCall":{"name":"weather","args":{"city":"Shanghai"}},
                "thoughtSignature":SIGNATURE
            }]},
            "finishReason":"STOP"
        }]
    });
    let non_stream: UnifiedResponse = serde_json::from_value::<GeminiResponse>(legacy.clone())
        .expect("legacy Gemini response")
        .into();
    let repeated: UnifiedResponse = serde_json::from_value::<GeminiResponse>(legacy.clone())
        .expect("repeated legacy Gemini response")
        .into();
    let stream: UnifiedChunkResponse = serde_json::from_value::<GeminiChunkResponse>(legacy)
        .expect("legacy Gemini stream chunk")
        .into();

    let non_stream_id = match &non_stream.choices[0].message.content[0] {
        UnifiedContentPart::ToolCall(call) => call.id.as_str(),
        other => panic!("expected non-stream tool call, got {other:?}"),
    };
    let repeated_id = match &repeated.choices[0].message.content[0] {
        UnifiedContentPart::ToolCall(call) => call.id.as_str(),
        other => panic!("expected repeated tool call, got {other:?}"),
    };
    let stream_id = match &stream.choices[0].delta.content[0] {
        UnifiedContentPartDelta::ToolCallDelta(call) => {
            call.id.as_deref().expect("stream tool call id")
        }
        other => panic!("expected stream tool call, got {other:?}"),
    };
    assert_eq!(non_stream_id, repeated_id);
    assert_eq!(non_stream_id, stream_id);
    assert!(non_stream_id.starts_with("gemini-call-"));
    assert!(!non_stream_id.contains(SIGNATURE));
    let association = &non_stream
        .provider_response_metadata
        .as_ref()
        .and_then(|metadata| metadata.gemini.as_ref())
        .expect("Gemini response metadata")
        .candidates[0]
        .tool_associations[0];
    assert_eq!(association.unified_tool_call_id, non_stream_id);
    assert_eq!(association.provider_tool_call_id, None);
    assert_eq!(association.thought_signature.as_deref(), Some(SIGNATURE));

    let native_history = json!({
        "contents":[
            {"role":"model","parts":[{
                "functionCall":{"id":"provider-call","name":"weather","args":{}},
                "thoughtSignature":SIGNATURE
            }]},
            {"role":"user","parts":[{
                "functionResponse":{"id":"provider-call","name":"weather","response":{"ok":true}}
            }]}
        ]
    });
    let unified: UnifiedRequest =
        serde_json::from_value::<GeminiRequestPayload>(native_history.clone())
            .expect("Gemini native tool history")
            .into();
    let encoded = serde_json::to_value(GeminiRequestPayload::from(unified))
        .expect("Gemini tool history re-encode");
    assert_eq!(
        encoded.pointer("/contents/0/parts/0/functionCall/id"),
        Some(&json!("provider-call"))
    );
    assert_eq!(
        encoded.pointer("/contents/0/parts/0/thoughtSignature"),
        Some(&json!(SIGNATURE))
    );
    assert_eq!(
        encoded.pointer("/contents/1/parts/0/functionResponse/id"),
        Some(&json!("provider-call"))
    );
}
