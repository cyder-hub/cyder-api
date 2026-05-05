use super::*;

#[test]
fn test_unified_request_to_responses_preserves_structured_input_items() {
    let unified_req = UnifiedRequest {
        model: Some("gpt-4.1".to_string()),
        messages: vec![UnifiedMessage {
            role: UnifiedRole::User,
            content: vec![
                UnifiedContentPart::Text {
                    text: "hello".to_string(),
                },
                UnifiedContentPart::ToolCall(UnifiedToolCall {
                    id: "call_1".to_string(),
                    name: "lookup".to_string(),
                    arguments: serde_json::json!({"city": "Boston"}),
                }),
                UnifiedContentPart::ToolResult(UnifiedToolResult {
                    tool_call_id: "call_1".to_string(),
                    name: Some("lookup".to_string()),
                    output: UnifiedToolResultOutput::Json {
                        value: json!({"ok": true}),
                    },
                }),
                UnifiedContentPart::ImageData {
                    mime_type: "image/png".to_string(),
                    data: "ZmFrZQ==".to_string(),
                },
                UnifiedContentPart::FileUrl {
                    url: "https://files.example.com/report.pdf".to_string(),
                    mime_type: Some("application/pdf".to_string()),
                    filename: None,
                },
                UnifiedContentPart::Reasoning {
                    text: "internal reasoning".to_string(),
                },
                UnifiedContentPart::ExecutableCode {
                    language: "python".to_string(),
                    code: "print(1)".to_string(),
                },
            ],
        }],
        extensions: Some(UnifiedRequestExtensions {
            responses: Some(UnifiedResponsesRequestExtension {
                instructions: Some("Be concise".to_string()),
                tool_choice: Some(json!("required")),
                text_format: Some(json!({"type":"json_object"})),
                reasoning: Some(json!({"effort":"medium"})),
                parallel_tool_calls: Some(false),
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let responses_req: ResponsesRequestPayload = unified_req.into();

    assert_eq!(responses_req.instructions.as_deref(), Some("Be concise"));
    assert!(matches!(
        responses_req.tool_choice,
        Some(ToolChoice::Value(ToolChoiceValue::Required))
    ));
    assert!(matches!(
        responses_req.text.as_ref().map(|text| &text.format),
        Some(TextResponseFormat::JsonObject)
    ));
    assert!(matches!(
        responses_req
            .reasoning
            .as_ref()
            .and_then(|r| r.effort.as_ref()),
        Some(ReasoningEffort::Medium)
    ));
    assert_eq!(responses_req.parallel_tool_calls, Some(false));

    let Input::Items(items) = responses_req.input else {
        panic!("Expected item-based responses input");
    };
    assert!(matches!(
        &items[0],
        ItemField::Message(Message { content, .. })
        if matches!(&content[0], ItemContentPart::InputText { text } if text == "hello")
    ));
    assert!(matches!(
        &items[1],
        ItemField::FunctionCall(FunctionCall { call_id, name, arguments, .. })
        if call_id == "call_1" && name == "lookup" && arguments == "{\"city\":\"Boston\"}"
    ));
    assert!(matches!(
        &items[2],
        ItemField::FunctionCallOutput(FunctionCallOutput { call_id, output, .. })
        if call_id == "call_1" && matches!(output, FunctionCallOutputPayload::Unknown(value) if value == &json!({"ok": true}))
    ));
    assert!(matches!(
        &items[3],
        ItemField::Message(Message { content, .. })
        if matches!(&content[0], ItemContentPart::InputImage { image_url: Some(url), .. } if url == "data:image/png;base64,ZmFrZQ==")
            && matches!(&content[1], ItemContentPart::InputFile { file_url: Some(url), .. } if url == "https://files.example.com/report.pdf")
    ));
    assert!(matches!(
        &items[4],
        ItemField::Reasoning(ReasoningBody { summary, .. })
        if matches!(&summary[0], ItemContentPart::SummaryText { text } if text == "internal reasoning")
    ));
    assert!(matches!(
        &items[5],
        ItemField::Message(Message { content, .. })
        if matches!(&content[0], ItemContentPart::InputText { text } if text == "```python\nprint(1)\n```")
    ));
}

#[test]
fn test_unified_request_to_responses_derives_rich_system_instructions() {
    let unified_req = UnifiedRequest {
        model: Some("gpt-4.1".to_string()),
        messages: vec![
            UnifiedMessage {
                role: UnifiedRole::System,
                content: vec![
                    UnifiedContentPart::Text {
                        text: "Follow policy".to_string(),
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
                    UnifiedContentPart::ExecutableCode {
                        language: "python".to_string(),
                        code: "print(1)".to_string(),
                    },
                    UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id: "call_1".to_string(),
                        name: "lookup".to_string(),
                        arguments: json!({"city":"Boston"}),
                    }),
                ],
            },
            UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text {
                    text: "hello".to_string(),
                }],
            },
        ],
        ..Default::default()
    };

    let responses_req: ResponsesRequestPayload = unified_req.into();

    assert_eq!(
        responses_req.instructions.as_deref(),
        Some(
            "Follow policy\ndata:image/png;base64,ZmFrZQ==\nfile_url: https://files.example.com/report.pdf\nmime_type: application/pdf\n```python\nprint(1)\n```\ntool_call: lookup\narguments: {\"city\":\"Boston\"}"
        )
    );
}

#[test]
fn test_responses_request_from_shorthand_input_message_preserves_text() {
    let payload = json!({
        "model": "gemini/gemini-2.5-flash-lite",
        "input": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "你好"
                    }
                ]
            }
        ]
    });

    let unified_req: UnifiedRequest = serde_json::from_value::<ResponsesRequestPayload>(payload)
        .expect("valid responses request")
        .into();

    assert_eq!(unified_req.messages.len(), 1);
    assert_eq!(unified_req.messages[0].role, UnifiedRole::User);
    assert_eq!(
        unified_req.messages[0].content,
        vec![UnifiedContentPart::Text {
            text: "你好".to_string()
        }]
    );
}

#[test]
fn test_responses_request_from_shorthand_assistant_message_uses_output_text_family() {
    let payload = json!({
        "model": "gemini-2.5-flash-lite",
        "input": [
            {
                "type": "message",
                "role": "assistant",
                "content": "Hello Alice! Nice to meet you. How can I help you today?"
            }
        ]
    });

    let responses_req: ResponsesRequestPayload =
        serde_json::from_value(payload).expect("valid responses request");

    let Input::Items(items) = responses_req.input else {
        panic!("expected item-based responses input");
    };

    assert!(matches!(
        &items[0],
        ItemField::Message(Message { role, content, .. })
            if matches!(role, MessageRole::Assistant)
                && matches!(
                    &content[0],
                    ItemContentPart::OutputText { text, annotations, logprobs }
                        if text == "Hello Alice! Nice to meet you. How can I help you today?"
                            && annotations.is_empty()
                            && logprobs.is_none()
                )
    ));
}

#[test]
fn test_unified_request_to_responses_splits_file_url_and_inline_file_data_paths() {
    let unified_req = UnifiedRequest {
        messages: vec![UnifiedMessage {
            role: UnifiedRole::User,
            content: vec![
                UnifiedContentPart::FileUrl {
                    url: "https://files.example.com/report.pdf".to_string(),
                    mime_type: Some("application/pdf".to_string()),
                    filename: Some("report.pdf".to_string()),
                },
                UnifiedContentPart::FileData {
                    data: "ZmFrZV9maWxl".to_string(),
                    mime_type: "application/pdf".to_string(),
                    filename: Some("inline.pdf".to_string()),
                },
            ],
        }],
        ..Default::default()
    };

    let responses_req: ResponsesRequestPayload = unified_req.into();
    let Input::Items(items) = responses_req.input else {
        panic!("Expected item-based responses input");
    };

    assert!(matches!(
        &items[0],
        ItemField::Message(Message { content, .. })
        if matches!(
            &content[0],
            ItemContentPart::InputFile { filename, file_url, file_id, file_data }
            if filename.as_deref() == Some("report.pdf")
                && file_url.as_deref() == Some("https://files.example.com/report.pdf")
                && file_id.is_none()
                && file_data.is_none()
        ) && matches!(
            &content[1],
            ItemContentPart::InputFile { filename, file_url, file_id, file_data }
            if filename.as_deref() == Some("inline.pdf")
                && file_url.is_none()
                && file_id.is_none()
                && file_data.as_deref()
                    == Some("data:application/pdf;base64,ZmFrZV9maWxl")
        )
    ));
}

#[test]
fn test_unified_request_to_responses_preserves_file_reference_id() {
    let unified_req = UnifiedRequest {
        model: Some("gpt-4.1".to_string()),
        messages: Vec::new(),
        items: vec![UnifiedItem::FileReference(UnifiedFileReferenceItem {
            filename: Some("report.pdf".to_string()),
            mime_type: None,
            file_url: None,
            file_id: Some("file_123".to_string()),
        })],
        ..Default::default()
    };

    let responses_req: ResponsesRequestPayload = unified_req.into();
    let Input::Items(items) = responses_req.input else {
        panic!("Expected item-based responses input");
    };

    assert!(matches!(
        &items[0],
        ItemField::Message(Message { content, .. })
        if matches!(
            &content[0],
            ItemContentPart::InputFile { filename, file_url, file_id, file_data }
            if filename.as_deref() == Some("report.pdf")
                && file_url.is_none()
                && file_id.as_deref() == Some("file_123")
                && file_data.is_none()
        )
    ));
}

#[test]
fn test_responses_request_to_unified_preserves_input_file_id_and_data() {
    let request = ResponsesRequestPayload {
        model: "gpt-4.1".to_string(),
        input: Input::Items(vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: "msg_1".to_string(),
            status: MessageStatus::Completed,
            role: MessageRole::User,
            content: vec![
                ItemContentPart::InputFile {
                    filename: Some("report.pdf".to_string()),
                    file_url: None,
                    file_id: Some("file_123".to_string()),
                    file_data: None,
                },
                ItemContentPart::InputFile {
                    filename: Some("inline.pdf".to_string()),
                    file_url: None,
                    file_id: None,
                    file_data: Some("data:application/pdf;base64,ZmFrZV9maWxl".to_string()),
                },
            ],
        })]),
        instructions: None,
        tools: None,
        tool_choice: None,
        text: None,
        reasoning: None,
        parallel_tool_calls: None,
        stream: Some(false),
        max_tokens: None,
        temperature: None,
        top_p: None,
    };

    let unified_req: UnifiedRequest = request.into();

    assert!(matches!(
        &unified_req.items[..],
        [
            UnifiedItem::Message(UnifiedMessageItem { content, .. }),
            UnifiedItem::FileReference(UnifiedFileReferenceItem { filename, file_id, file_url, .. })
        ]
        if matches!(
            &content[..],
            [UnifiedContentPart::FileData { data, mime_type, filename }]
            if data == "ZmFrZV9maWxl"
                && mime_type == "application/pdf"
                && filename.as_deref() == Some("inline.pdf")
        )
        && filename.as_deref() == Some("report.pdf")
        && file_id.as_deref() == Some("file_123")
        && file_url.is_none()
    ));
}

#[test]
fn test_unified_request_to_responses_preserves_multimodal_tool_result_output() {
    let unified_req = UnifiedRequest {
        messages: vec![UnifiedMessage {
            role: UnifiedRole::Tool,
            content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                tool_call_id: "call_1".to_string(),
                name: Some("lookup".to_string()),
                output: UnifiedToolResultOutput::Content {
                    parts: vec![
                        UnifiedToolResultPart::Text {
                            text: "hello".to_string(),
                        },
                        UnifiedToolResultPart::File {
                            filename: Some("report.pdf".to_string()),
                            file_url: Some("https://files.example.com/report.pdf".to_string()),
                        },
                        UnifiedToolResultPart::Image {
                            image_url: Some("https://images.example.com/1.png".to_string()),
                            file_url: None,
                        },
                    ],
                },
            })],
        }],
        ..Default::default()
    };

    let responses_req: ResponsesRequestPayload = unified_req.into();
    let Input::Items(items) = responses_req.input else {
        panic!("Expected item-based responses input");
    };

    match &items[0] {
        ItemField::FunctionCallOutput(FunctionCallOutput {
            call_id, output, ..
        }) => {
            assert_eq!(call_id, "call_1");
            match output {
                FunctionCallOutputPayload::Content(parts) => {
                    assert!(matches!(
                        &parts[0],
                        FunctionCallOutputContent::Text { text } if text == "hello"
                    ));
                    assert!(matches!(
                        &parts[1],
                        FunctionCallOutputContent::File { filename, file_url }
                        if filename.as_deref() == Some("report.pdf")
                            && file_url.as_deref()
                                == Some("https://files.example.com/report.pdf")
                    ));
                    assert!(matches!(
                        &parts[2],
                        FunctionCallOutputContent::Image { image_url, file_url }
                        if image_url.as_deref()
                            == Some("https://images.example.com/1.png")
                            && file_url.is_none()
                    ));
                }
                other => panic!("Expected content payload, got {:?}", other),
            }
        }
        other => panic!("Expected function_call_output item, got {:?}", other),
    }
}

#[test]
fn test_responses_request_to_unified_preserves_responses_extensions() {
    let request = ResponsesRequestPayload {
        model: "gpt-4.1".to_string(),
        input: Input::Items(vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: "msg_1".to_string(),
            status: MessageStatus::Completed,
            role: MessageRole::User,
            content: vec![ItemContentPart::InputText {
                text: "hello".to_string(),
            }],
        })]),
        instructions: Some("Follow the house style".to_string()),
        tools: Some(vec![Tool::Function(FunctionTool {
            name: "lookup_weather".to_string(),
            description: Some("Weather lookup".to_string()),
            parameters: Some(json!({"type":"object"})),
            strict: Some(true),
        })]),
        tool_choice: Some(ToolChoice::Value(ToolChoiceValue::Required)),
        text: Some(TextField {
            format: TextResponseFormat::JsonObject,
            verbosity: None,
        }),
        reasoning: Some(Reasoning {
            effort: Some(ReasoningEffort::High),
            summary: Some(ReasoningSummary::Detailed),
        }),
        parallel_tool_calls: Some(false),
        stream: Some(true),
        max_tokens: Some(128),
        temperature: Some(0.2),
        top_p: Some(0.9),
    };

    let unified: UnifiedRequest = request.into();

    assert!(matches!(
        unified.messages.first(),
        Some(UnifiedMessage {
            role: UnifiedRole::System,
            ..
        })
    ));
    assert!(unified.tools.as_ref().is_some_and(|tools| tools.len() == 1));
    let ext = unified.responses_extension().expect("responses extension");
    assert_eq!(ext.instructions.as_deref(), Some("Follow the house style"));
    assert_eq!(ext.parallel_tool_calls, Some(false));
    assert_eq!(ext.tool_choice.as_ref(), Some(&json!("required")));
    assert_eq!(
        ext.text_format.as_ref(),
        Some(&json!({"type":"json_object"}))
    );
    assert_eq!(
        ext.reasoning.as_ref(),
        Some(&json!({"effort":"high","summary":"detailed"}))
    );
}

#[test]
fn test_unified_request_items_to_responses_input() {
    let unified_req = UnifiedRequest {
        model: Some("gpt-4.1".to_string()),
        messages: Vec::new(),
        items: vec![
            UnifiedItem::Message(UnifiedMessageItem {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text {
                    text: "hello".to_string(),
                }],
                annotations: Vec::new(),
            }),
            UnifiedItem::Reasoning(UnifiedReasoningItem {
                content: vec![UnifiedContentPart::Reasoning {
                    text: "checked".to_string(),
                }],
                annotations: Vec::new(),
            }),
            UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                id: "call_1".to_string(),
                name: "lookup".to_string(),
                arguments: json!({"city":"Boston"}),
            }),
            UnifiedItem::FunctionCallOutput(UnifiedFunctionCallOutputItem {
                tool_call_id: "call_1".to_string(),
                name: None,
                output: UnifiedToolResultOutput::Text {
                    text: "ok".to_string(),
                },
            }),
        ],
        tools: None,
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        stop: None,
        seed: None,
        presence_penalty: None,
        frequency_penalty: None,
        extensions: None,
    };

    let payload: ResponsesRequestPayload = unified_req.into();
    let Input::Items(items) = payload.input else {
        panic!("expected item input");
    };

    assert_eq!(items.len(), 4);
    assert!(matches!(&items[0], ItemField::Message(_)));
    assert!(matches!(&items[1], ItemField::Reasoning(_)));
    assert!(matches!(&items[2], ItemField::FunctionCall(_)));
    assert!(matches!(
        &items[3],
        ItemField::FunctionCallOutput(FunctionCallOutput {
            call_id,
            output: FunctionCallOutputPayload::Text(text),
            ..
        }) if call_id == "call_1" && text == "ok"
    ));
}
