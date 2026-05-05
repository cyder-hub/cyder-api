use super::*;

#[test]
fn test_unified_response_to_responses_preserves_structured_items() {
    let unified_res = UnifiedResponse {
        id: "resp_1".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![
                    UnifiedContentPart::Text {
                        text: "done".to_string(),
                    },
                    UnifiedContentPart::ImageData {
                        mime_type: "image/png".to_string(),
                        data: "ZmFrZQ==".to_string(),
                    },
                    UnifiedContentPart::Reasoning {
                        text: "checked the tool output".to_string(),
                    },
                    UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id: "call_1".to_string(),
                        name: "lookup".to_string(),
                        arguments: json!({"city":"Boston"}),
                    }),
                    UnifiedContentPart::ToolResult(UnifiedToolResult {
                        tool_call_id: "call_1".to_string(),
                        name: Some("lookup".to_string()),
                        output: UnifiedToolResultOutput::Json {
                            value: json!({"ok": true}),
                        },
                    }),
                    UnifiedContentPart::ExecutableCode {
                        language: "python".to_string(),
                        code: "print(1)".to_string(),
                    },
                ],
                ..Default::default()
            },
            items: Vec::new(),
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: None,
        created: Some(1),
        object: Some("chat.completion".to_string()),
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let responses_res: ResponsesResponse = unified_res.into();

    match &responses_res.output[0] {
        ItemField::Message(message) => {
            assert_eq!(message.content.len(), 2);
            match &message.content[0] {
                ItemContentPart::OutputText { text, .. } => assert_eq!(text, "done"),
                _ => panic!("Expected output_text content"),
            }
            match &message.content[1] {
                ItemContentPart::InputImage { image_url, .. } => {
                    assert_eq!(image_url.as_deref(), Some("data:image/png;base64,ZmFrZQ=="));
                }
                other => panic!("Expected input_image content, got {:?}", other),
            }
        }
        _ => panic!("Expected message output"),
    }
    assert!(matches!(
        &responses_res.output[1],
        ItemField::Reasoning(ReasoningBody { summary, .. })
        if matches!(&summary[0], ItemContentPart::SummaryText { text } if text == "checked the tool output")
    ));
    assert!(matches!(
        &responses_res.output[2],
        ItemField::FunctionCall(FunctionCall { call_id, name, arguments, .. })
        if call_id == "call_1" && name == "lookup" && arguments == "{\"city\":\"Boston\"}"
    ));
    assert!(matches!(
        &responses_res.output[3],
        ItemField::FunctionCallOutput(FunctionCallOutput { call_id, output, .. })
        if call_id == "call_1" && matches!(output, FunctionCallOutputPayload::Unknown(value) if value == &json!({"ok": true}))
    ));
    assert!(matches!(
        &responses_res.output[4],
        ItemField::Message(Message { content, .. })
        if matches!(&content[0], ItemContentPart::OutputText { text, .. } if text == "```python\nprint(1)\n```")
    ));

    let serialized = serde_json::to_value(&responses_res).expect("serialize responses");
    assert_eq!(serialized["output"][0]["content"][0]["logprobs"], json!([]));
    assert_eq!(serialized["output"][4]["content"][0]["logprobs"], json!([]));
}

#[test]
fn test_responses_response_to_unified_preserves_structured_items() {
    let responses_res = ResponsesResponse {
        id: "resp_1".to_string(),
        object: ResponseObject::Response,
        created_at: 1,
        completed_at: Some(1),
        status: ResponseStatus::Completed,
        incomplete_details: None,
        model: "gpt-4.1".to_string(),
        previous_response_id: None,
        instructions: None,
        output: vec![
            ItemField::FunctionCall(FunctionCall {
                _type: "function_call".to_string(),
                id: "fc_1".to_string(),
                call_id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
                arguments: "{\"city\":\"Boston\"}".to_string(),
                status: MessageStatus::Completed,
            }),
            ItemField::Reasoning(ReasoningBody {
                _type: "reasoning".to_string(),
                id: "rs_1".to_string(),
                content: None,
                summary: vec![ItemContentPart::SummaryText {
                    text: "internal reasoning".to_string(),
                }],
                encrypted_content: None,
            }),
            ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::Assistant,
                content: vec![ItemContentPart::OutputText {
                    text: "final answer".to_string(),
                    annotations: vec![],
                    logprobs: None,
                }],
            }),
        ],
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
        usage: None,
        max_output_tokens: None,
        max_tool_calls: None,
        store: true,
        background: false,
        service_tier: ServiceTier::Default,
        metadata: json!({}),
        safety_identifier: None,
        prompt_cache_key: None,
    };

    let unified_res: UnifiedResponse = responses_res.into();

    assert_eq!(unified_res.choices.len(), 1);
    assert_eq!(unified_res.choices[0].message.content.len(), 3);
    assert!(matches!(
        &unified_res.choices[0].message.content[0],
        UnifiedContentPart::ToolCall(UnifiedToolCall { id, name, arguments })
        if id == "call_1" && name == "lookup_weather" && arguments == &json!({"city":"Boston"})
    ));
    assert!(matches!(
        &unified_res.choices[0].message.content[1],
        UnifiedContentPart::Reasoning { text } if text == "internal reasoning"
    ));
    assert!(matches!(
        &unified_res.choices[0].message.content[2],
        UnifiedContentPart::Text { text } if text == "final answer"
    ));
}

#[test]
fn test_responses_response_deserializes_unknown_item_type_without_failing() {
    let raw = json!({
        "id": "resp_1",
        "object": "response",
        "created_at": 1,
        "completed_at": 1,
        "status": "completed",
        "incomplete_details": null,
        "model": "gpt-4.1",
        "previous_response_id": null,
        "instructions": null,
        "output": [
            {
                "type": "custom_unknown_item",
                "id": "x_1",
                "payload": {"foo": "bar"}
            },
            {
                "type": "message",
                "id": "msg_1",
                "status": "completed",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "ok",
                        "annotations": []
                    }
                ]
            }
        ],
        "error": null,
        "tools": [],
        "tool_choice": "auto",
        "truncation": "disabled",
        "parallel_tool_calls": true,
        "text": {"format": {"type": "text"}},
        "top_p": 1.0,
        "presence_penalty": 0.0,
        "frequency_penalty": 0.0,
        "top_logprobs": 0,
        "temperature": 1.0,
        "reasoning": null,
        "usage": null,
        "max_output_tokens": null,
        "max_tool_calls": null,
        "store": true,
        "background": false,
        "service_tier": "default",
        "metadata": {},
        "safety_identifier": null,
        "prompt_cache_key": null
    });

    let responses_res: ResponsesResponse = serde_json::from_value(raw).unwrap();
    let unified_res: UnifiedResponse = responses_res.into();

    assert_eq!(unified_res.choices.len(), 1);
    match &unified_res.choices[0].message.content[0] {
        UnifiedContentPart::Text { text } => assert_eq!(text, "ok"),
        _ => panic!("Expected text output"),
    }
}

#[test]
fn test_responses_response_serializes_typed_enums_to_schema_strings() {
    let response = ResponsesResponse {
        id: "resp_1".to_string(),
        object: ResponseObject::Response,
        created_at: 1,
        completed_at: Some(2),
        status: ResponseStatus::Completed,
        incomplete_details: None,
        model: "gpt-4.1".to_string(),
        previous_response_id: None,
        instructions: None,
        output: Vec::new(),
        error: None,
        tools: Vec::new(),
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
        usage: None,
        max_output_tokens: None,
        max_tool_calls: None,
        store: false,
        background: false,
        service_tier: ServiceTier::Default,
        metadata: json!({}),
        safety_identifier: None,
        prompt_cache_key: None,
    };

    let value = serde_json::to_value(response).unwrap();

    assert_eq!(value["object"], json!("response"));
    assert_eq!(value["status"], json!("completed"));
    assert_eq!(value["service_tier"], json!("default"));
}

#[test]
fn test_unified_response_to_responses_preserves_multimodal_function_call_output_item() {
    let unified_res = UnifiedResponse {
        id: "resp_function_output_roundtrip".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: Vec::new(),
                ..Default::default()
            },
            items: vec![UnifiedItem::FunctionCallOutput(
                UnifiedFunctionCallOutputItem {
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
                },
            )],
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: None,
        created: Some(1),
        object: Some("response".to_string()),
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let responses_res: ResponsesResponse = unified_res.into();

    assert!(matches!(
        &responses_res.output[0],
        ItemField::FunctionCallOutput(FunctionCallOutput {
            call_id,
            output: FunctionCallOutputPayload::Content(parts),
            ..
        }) if call_id == "call_1"
            && matches!(&parts[0], FunctionCallOutputContent::Text { text } if text == "hello")
            && matches!(
                &parts[1],
                FunctionCallOutputContent::File { filename, file_url }
                if filename.as_deref() == Some("report.pdf")
                    && file_url.as_deref() == Some("https://files.example.com/report.pdf")
            )
            && matches!(
                &parts[2],
                FunctionCallOutputContent::Image { image_url, file_url }
                if image_url.as_deref() == Some("https://images.example.com/1.png")
                    && file_url.is_none()
            )
    ));
}
