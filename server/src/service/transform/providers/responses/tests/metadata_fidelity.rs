use super::*;

#[test]
fn test_responses_response_to_unified_preserves_provider_metadata() {
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
        output: vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: "msg_1".to_string(),
            status: MessageStatus::Completed,
            role: MessageRole::Assistant,
            content: vec![
                ItemContentPart::Refusal {
                    refusal: "refused".to_string(),
                },
                ItemContentPart::OutputText {
                    text: "final answer".to_string(),
                    annotations: vec![Annotation::UrlCitation {
                        url: "https://example.com".to_string(),
                        start_index: 0,
                        end_index: 5,
                        title: "Example".to_string(),
                    }],
                    logprobs: None,
                },
            ],
        })],
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
        metadata: json!({"trace_id":"abc"}),
        safety_identifier: Some("safe-1".to_string()),
        prompt_cache_key: Some("cache-1".to_string()),
    };

    let unified_res: UnifiedResponse = responses_res.into();
    let metadata = unified_res.provider_response_metadata().unwrap();
    let responses_metadata = metadata.responses.as_ref().unwrap();
    assert_eq!(
        responses_metadata.safety_identifier.as_deref(),
        Some("safe-1")
    );
    assert_eq!(
        responses_metadata.prompt_cache_key.as_deref(),
        Some("cache-1")
    );
    assert_eq!(responses_metadata.citations.len(), 1);
    assert_eq!(responses_metadata.refusals.len(), 1);
    assert_eq!(
        responses_metadata
            .metadata
            .as_ref()
            .unwrap()
            .get("trace_id")
            .and_then(Value::as_str),
        Some("abc")
    );
    assert!(matches!(
        &unified_res.choices[0].items[0],
        UnifiedItem::Message(UnifiedMessageItem { content, annotations, .. })
        if matches!(
            &content[..],
            [UnifiedContentPart::Refusal { text }, UnifiedContentPart::Text { text: answer }]
            if text == "refused" && answer == "final answer"
        ) && matches!(
            &annotations[..],
            [UnifiedAnnotation::Citation(UnifiedCitation { url, title, start_index, end_index, .. })]
            if url.as_deref() == Some("https://example.com")
            && title.as_deref() == Some("Example")
            && *start_index == Some(0)
            && *end_index == Some(5)
        )
    ));
}

#[test]
fn test_responses_response_to_unified_preserves_incomplete_status_metadata() {
    let responses_res = ResponsesResponse {
        id: "resp_incomplete".to_string(),
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
        output: vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: "msg_1".to_string(),
            status: MessageStatus::Incomplete,
            role: MessageRole::Assistant,
            content: vec![ItemContentPart::OutputText {
                text: "partial answer".to_string(),
                annotations: vec![],
                logprobs: None,
            }],
        })],
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
    let responses_metadata = unified_res
        .provider_response_metadata()
        .and_then(|metadata| metadata.responses.as_ref())
        .unwrap();

    assert_eq!(responses_metadata.status.as_deref(), Some("incomplete"));
    assert_eq!(
        responses_metadata
            .incomplete_details
            .as_ref()
            .map(|details| details.reason.as_str()),
        Some("max_output_tokens")
    );
}

#[test]
fn test_unified_response_to_responses_preserves_provider_metadata() {
    let unified_res = UnifiedResponse {
        id: "resp_1".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![],
        usage: None,
        created: Some(1),
        object: Some("chat.completion".to_string()),
        system_fingerprint: None,
        provider_response_metadata: Some(UnifiedProviderResponseMetadata {
            responses: Some(UnifiedResponsesResponseMetadata {
                safety_identifier: Some("safe-1".to_string()),
                prompt_cache_key: Some("cache-1".to_string()),
                citations: vec![],
                refusals: vec![],
                files: vec![],
                metadata: Some(serde_json::Map::from_iter([(
                    "trace_id".to_string(),
                    json!("abc"),
                )])),
                reasoning: None,
                status: None,
                incomplete_details: None,
            }),
            ..Default::default()
        }),
        synthetic_metadata: None,
    };

    let responses_res: ResponsesResponse = unified_res.into();
    assert_eq!(responses_res.safety_identifier.as_deref(), Some("safe-1"));
    assert_eq!(responses_res.prompt_cache_key.as_deref(), Some("cache-1"));
    assert_eq!(responses_res.metadata["trace_id"], json!("abc"));
}

#[test]
fn test_unified_response_to_responses_restores_incomplete_status_metadata() {
    let unified_res = UnifiedResponse {
        id: "resp_incomplete".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::Text {
                    text: "partial answer".to_string(),
                }],
                ..Default::default()
            },
            items: vec![],
            finish_reason: Some("length".to_string()),
            logprobs: None,
        }],
        usage: None,
        created: Some(1),
        object: Some("chat.completion".to_string()),
        system_fingerprint: None,
        provider_response_metadata: Some(UnifiedProviderResponseMetadata {
            responses: Some(UnifiedResponsesResponseMetadata {
                status: Some("incomplete".to_string()),
                incomplete_details: Some(UnifiedResponsesIncompleteDetails {
                    reason: "max_output_tokens".to_string(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        synthetic_metadata: None,
    };

    let responses_res: ResponsesResponse = unified_res.into();

    assert!(matches!(responses_res.status, ResponseStatus::Incomplete));
    assert_eq!(responses_res.completed_at, None);
    assert_eq!(
        responses_res
            .incomplete_details
            .as_ref()
            .map(|details| details.reason.as_str()),
        Some("max_output_tokens")
    );
}

#[test]
fn test_unified_response_to_responses_preserves_file_url_as_input_file() {
    let unified_res = UnifiedResponse {
        id: "resp_1".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::FileUrl {
                    url: "https://files.example.com/report.pdf".to_string(),
                    mime_type: Some("application/pdf".to_string()),
                    filename: None,
                }],
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
        ItemField::Message(message) => match &message.content[0] {
            ItemContentPart::InputFile {
                filename,
                file_url,
                file_id,
                file_data,
            } => {
                assert!(filename.is_none());
                assert_eq!(
                    file_url.as_deref(),
                    Some("https://files.example.com/report.pdf")
                );
                assert!(file_id.is_none());
                assert!(file_data.is_none());
            }
            other => panic!("Expected input_file item, got {:?}", other),
        },
        other => panic!("Expected message output, got {:?}", other),
    }
}

#[test]
fn test_responses_response_to_unified_preserves_file_references_in_metadata() {
    let responses_res = ResponsesResponse {
        id: "resp_file".to_string(),
        object: ResponseObject::Response,
        created_at: 1,
        completed_at: Some(1),
        status: ResponseStatus::Completed,
        incomplete_details: None,
        model: "gpt-4.1".to_string(),
        previous_response_id: None,
        instructions: None,
        output: vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: "msg_1".to_string(),
            status: MessageStatus::Completed,
            role: MessageRole::Assistant,
            content: vec![ItemContentPart::InputFile {
                filename: Some("report.pdf".to_string()),
                file_url: Some("https://files.example.com/report.pdf".to_string()),
                file_id: None,
                file_data: None,
            }],
        })],
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
    let responses_metadata = unified_res
        .provider_response_metadata()
        .and_then(|metadata| metadata.responses.as_ref())
        .unwrap();
    assert_eq!(responses_metadata.files.len(), 1);
    assert_eq!(
        responses_metadata.files[0].filename.as_deref(),
        Some("report.pdf")
    );
    assert_eq!(
        responses_metadata.files[0].file_url.as_deref(),
        Some("https://files.example.com/report.pdf")
    );
    assert!(matches!(
        &unified_res.choices[0].items[..],
        [UnifiedItem::FileReference(UnifiedFileReferenceItem { filename, file_url, .. })]
        if filename.as_deref() == Some("report.pdf")
        && file_url.as_deref() == Some("https://files.example.com/report.pdf")
    ));
}

#[test]
fn test_responses_response_to_unified_preserves_input_file_id_and_data() {
    let responses_res = ResponsesResponse {
        id: "resp_file".to_string(),
        object: ResponseObject::Response,
        created_at: 1,
        completed_at: Some(1),
        status: ResponseStatus::Completed,
        incomplete_details: None,
        model: "gpt-4.1".to_string(),
        previous_response_id: None,
        instructions: None,
        output: vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: "msg_1".to_string(),
            status: MessageStatus::Completed,
            role: MessageRole::Assistant,
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
        })],
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
    let responses_metadata = unified_res
        .provider_response_metadata()
        .and_then(|metadata| metadata.responses.as_ref())
        .unwrap();

    assert!(matches!(
        &responses_metadata.files[..],
        [
            UnifiedResponsesFileReference { filename, file_id, file_url, file_data },
            UnifiedResponsesFileReference { filename: inline_name, file_id: inline_id, file_url: inline_url, file_data: inline_data }
        ]
        if filename.as_deref() == Some("report.pdf")
            && file_id.as_deref() == Some("file_123")
            && file_url.is_none()
            && file_data.is_none()
            && inline_name.as_deref() == Some("inline.pdf")
            && inline_id.is_none()
            && inline_url.is_none()
            && inline_data.as_deref() == Some("data:application/pdf;base64,ZmFrZV9maWxl")
    ));
    assert!(matches!(
        &unified_res.choices[0].items[..],
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
fn test_responses_response_to_unified_drops_input_file_instead_of_placeholder_text() {
    let responses_res = ResponsesResponse {
        id: "resp_file".to_string(),
        object: ResponseObject::Response,
        created_at: 1,
        completed_at: Some(1),
        status: ResponseStatus::Completed,
        incomplete_details: None,
        model: "gpt-4.1".to_string(),
        previous_response_id: None,
        instructions: None,
        output: vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: "msg_1".to_string(),
            status: MessageStatus::Completed,
            role: MessageRole::Assistant,
            content: vec![
                ItemContentPart::OutputText {
                    text: "usable text".to_string(),
                    annotations: vec![],
                    logprobs: None,
                },
                ItemContentPart::InputFile {
                    filename: Some("doc.txt".to_string()),
                    file_url: Some("https://example.com/doc.txt".to_string()),
                    file_id: None,
                    file_data: None,
                },
            ],
        })],
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
    assert_eq!(unified_res.choices[0].message.content.len(), 1);
    match &unified_res.choices[0].message.content[0] {
        UnifiedContentPart::Text { text } => assert_eq!(text, "usable text"),
        _ => panic!("Expected plain text output"),
    }
}

#[test]
fn test_function_call_output_payload_deserializes_content_array() {
    let payload: FunctionCallOutputPayload = serde_json::from_value(json!([
            {"type": "text", "text": "hello"},
            {"type": "file", "filename": "report.pdf", "file_url": "https://files.example.com/report.pdf"},
            {"type": "image", "image_url": "https://images.example.com/1.png"}
        ]))
        .unwrap();

    match payload {
        FunctionCallOutputPayload::Content(parts) => {
            assert!(
                matches!(&parts[0], FunctionCallOutputContent::Text { text } if text == "hello")
            );
            assert!(matches!(
                &parts[1],
                FunctionCallOutputContent::File { filename, file_url }
                if filename.as_deref() == Some("report.pdf")
                && file_url.as_deref() == Some("https://files.example.com/report.pdf")
            ));
            assert!(matches!(
                &parts[2],
                FunctionCallOutputContent::Image { image_url, file_url }
                if image_url.as_deref() == Some("https://images.example.com/1.png")
                && file_url.is_none()
            ));
        }
        other => panic!("Expected content payload, got {:?}", other),
    }
}

#[test]
fn test_responses_response_to_unified_preserves_typed_function_call_output_item() {
    let responses_res = ResponsesResponse {
        id: "resp_function_output".to_string(),
        object: ResponseObject::Response,
        created_at: 1,
        completed_at: Some(1),
        status: ResponseStatus::Completed,
        incomplete_details: None,
        model: "gpt-4.1".to_string(),
        previous_response_id: None,
        instructions: None,
        output: vec![ItemField::FunctionCallOutput(FunctionCallOutput {
            _type: "function_call_output".to_string(),
            id: "fco_1".to_string(),
            call_id: "call_1".to_string(),
            output: FunctionCallOutputPayload::Content(vec![
                FunctionCallOutputContent::Text {
                    text: "hello".to_string(),
                },
                FunctionCallOutputContent::File {
                    filename: Some("report.pdf".to_string()),
                    file_url: Some("https://files.example.com/report.pdf".to_string()),
                },
                FunctionCallOutputContent::Image {
                    image_url: Some("https://images.example.com/1.png".to_string()),
                    file_url: None,
                },
            ]),
            status: MessageStatus::Completed,
        })],
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

    assert!(matches!(
        &unified_res.choices[0].items[0],
        UnifiedItem::FunctionCallOutput(UnifiedFunctionCallOutputItem {
            tool_call_id,
            output: UnifiedToolResultOutput::Content { parts },
            ..
        }) if tool_call_id == "call_1"
            && matches!(&parts[0], UnifiedToolResultPart::Text { text } if text == "hello")
            && matches!(
                &parts[1],
                UnifiedToolResultPart::File { filename, file_url }
                if filename.as_deref() == Some("report.pdf")
                    && file_url.as_deref() == Some("https://files.example.com/report.pdf")
            )
            && matches!(
                &parts[2],
                UnifiedToolResultPart::Image { image_url, file_url }
                if image_url.as_deref() == Some("https://images.example.com/1.png")
                    && file_url.is_none()
            )
    ));
    assert!(matches!(
        &unified_res.choices[0].message.content[0],
        UnifiedContentPart::ToolResult(UnifiedToolResult {
            tool_call_id,
            output: UnifiedToolResultOutput::Content { parts },
            ..
        }) if tool_call_id == "call_1"
            && matches!(&parts[0], UnifiedToolResultPart::Text { text } if text == "hello")
    ));
}

#[test]
fn test_responses_response_to_unified_promotes_refusal_to_content_and_metadata() {
    let responses_res = ResponsesResponse {
        id: "resp_refusal".to_string(),
        object: ResponseObject::Response,
        created_at: 1,
        completed_at: Some(1),
        status: ResponseStatus::Completed,
        incomplete_details: None,
        model: "gpt-4.1".to_string(),
        previous_response_id: None,
        instructions: None,
        output: vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: "msg_1".to_string(),
            status: MessageStatus::Completed,
            role: MessageRole::Assistant,
            content: vec![
                ItemContentPart::Refusal {
                    refusal: "cannot comply".to_string(),
                },
                ItemContentPart::OutputText {
                    text: "safe answer".to_string(),
                    annotations: vec![],
                    logprobs: None,
                },
            ],
        })],
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

    assert!(matches!(
        &unified_res.choices[0].message.content[..],
        [UnifiedContentPart::Refusal { text }, UnifiedContentPart::Text { text: answer }]
        if text == "cannot comply" && answer == "safe answer"
    ));
    let responses_metadata = unified_res
        .provider_response_metadata()
        .and_then(|metadata| metadata.responses.as_ref())
        .unwrap();
    assert_eq!(responses_metadata.refusals.len(), 1);
    assert_eq!(responses_metadata.refusals[0].refusal, "cannot comply");
}

#[test]
fn test_unified_response_to_responses_preserves_structured_annotations_and_file_reference_items() {
    let unified_res = UnifiedResponse {
        id: "resp_structured".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::Text {
                    text: "legacy".to_string(),
                }],
                ..Default::default()
            },
            items: vec![
                UnifiedItem::Message(UnifiedMessageItem {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "final answer".to_string(),
                    }],
                    annotations: vec![UnifiedAnnotation::Citation(UnifiedCitation {
                        part_index: Some(0),
                        start_index: Some(0),
                        end_index: Some(5),
                        url: Some("https://example.com".to_string()),
                        title: Some("Example".to_string()),
                        license: None,
                    })],
                }),
                UnifiedItem::FileReference(UnifiedFileReferenceItem {
                    filename: Some("report.pdf".to_string()),
                    mime_type: None,
                    file_url: Some("https://files.example.com/report.pdf".to_string()),
                    file_id: None,
                }),
            ],
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
        ItemField::Message(Message { content, .. })
        if matches!(
            &content[0],
            ItemContentPart::OutputText { text, annotations, .. }
            if text == "final answer"
            && matches!(
                &annotations[..],
                [Annotation::UrlCitation { url, title, start_index, end_index }]
                if url == "https://example.com"
                && title == "Example"
                && *start_index == 0
                && *end_index == 5
            )
        )
    ));
    assert!(matches!(
        &responses_res.output[1],
        ItemField::Message(Message { content, .. })
        if matches!(
            &content[0],
            ItemContentPart::InputFile { filename, file_url, file_id, file_data }
            if filename.as_deref() == Some("report.pdf")
            && file_url.as_deref() == Some("https://files.example.com/report.pdf")
            && file_id.is_none()
            && file_data.is_none()
        )
    ));
}

#[test]
fn test_unified_response_to_responses_restores_refusal_and_reasoning_metadata() {
    let unified_res = UnifiedResponse {
        id: "resp_restore".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![
                    UnifiedContentPart::Reasoning {
                        text: "checked policy".to_string(),
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
        created: Some(1),
        object: Some("response".to_string()),
        system_fingerprint: None,
        provider_response_metadata: Some(UnifiedProviderResponseMetadata {
            responses: Some(UnifiedResponsesResponseMetadata {
                safety_identifier: None,
                prompt_cache_key: None,
                citations: vec![],
                refusals: vec![UnifiedResponsesRefusal {
                    refusal: "cannot comply".to_string(),
                }],
                files: vec![],
                metadata: None,
                reasoning: Some(json!({
                    "encrypted_contents": ["enc_1"]
                })),
                status: None,
                incomplete_details: None,
            }),
            ..Default::default()
        }),
        synthetic_metadata: None,
    };

    let responses_res: ResponsesResponse = unified_res.into();

    assert!(matches!(
        &responses_res.output[1],
        ItemField::Message(Message { content, .. })
        if matches!(&content[0], ItemContentPart::Refusal { refusal } if refusal == "cannot comply")
        && matches!(&content[1], ItemContentPart::OutputText { text, .. } if text == "safe answer")
    ));
    assert!(matches!(
        &responses_res.output[0],
        ItemField::Reasoning(ReasoningBody { encrypted_content, .. })
        if encrypted_content.as_deref() == Some("enc_1")
    ));
}

#[test]
fn test_unified_response_to_responses_restores_file_input_metadata() {
    let unified_res = UnifiedResponse {
        id: "resp_restore_files".to_string(),
        model: Some("gpt-4.1".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::Text {
                    text: "safe answer".to_string(),
                }],
                ..Default::default()
            },
            items: Vec::new(),
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: None,
        created: Some(1),
        object: Some("response".to_string()),
        system_fingerprint: None,
        provider_response_metadata: Some(UnifiedProviderResponseMetadata {
            responses: Some(UnifiedResponsesResponseMetadata {
                safety_identifier: None,
                prompt_cache_key: None,
                citations: vec![],
                refusals: vec![],
                files: vec![
                    UnifiedResponsesFileReference {
                        filename: Some("report.pdf".to_string()),
                        file_url: None,
                        file_id: Some("file_123".to_string()),
                        file_data: None,
                    },
                    UnifiedResponsesFileReference {
                        filename: Some("inline.pdf".to_string()),
                        file_url: None,
                        file_id: None,
                        file_data: Some("data:application/pdf;base64,ZmFrZV9maWxl".to_string()),
                    },
                ],
                metadata: None,
                reasoning: None,
                status: None,
                incomplete_details: None,
            }),
            ..Default::default()
        }),
        synthetic_metadata: None,
    };

    let responses_res: ResponsesResponse = unified_res.into();

    assert!(matches!(
        &responses_res.output[1],
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
    assert!(matches!(
        &responses_res.output[2],
        ItemField::Message(Message { content, .. })
        if matches!(
            &content[0],
            ItemContentPart::InputFile { filename, file_url, file_id, file_data }
            if filename.as_deref() == Some("inline.pdf")
                && file_url.is_none()
                && file_id.is_none()
                && file_data.as_deref() == Some("data:application/pdf;base64,ZmFrZV9maWxl")
        )
    ));
}

#[test]
fn test_responses_response_to_unified_preserves_item_family() {
    let responses_res = ResponsesResponse {
        id: "resp_123".to_string(),
        object: ResponseObject::Response,
        created_at: 1,
        completed_at: Some(2),
        status: ResponseStatus::Completed,
        incomplete_details: None,
        model: "gpt-4.1".to_string(),
        previous_response_id: None,
        instructions: None,
        output: vec![
            ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::Assistant,
                content: vec![ItemContentPart::OutputText {
                    text: "done".to_string(),
                    annotations: Vec::new(),
                    logprobs: None,
                }],
            }),
            ItemField::Reasoning(ReasoningBody {
                _type: "reasoning".to_string(),
                id: "rs_1".to_string(),
                content: Some(vec![ItemContentPart::ReasoningText {
                    text: "checked".to_string(),
                }]),
                summary: Vec::new(),
                encrypted_content: None,
            }),
            ItemField::FunctionCall(FunctionCall {
                _type: "function_call".to_string(),
                id: "fc_1".to_string(),
                call_id: "call_1".to_string(),
                name: "lookup".to_string(),
                arguments: "{\"city\":\"Boston\"}".to_string(),
                status: MessageStatus::Completed,
            }),
            ItemField::FunctionCallOutput(FunctionCallOutput {
                _type: "function_call_output".to_string(),
                id: "fco_1".to_string(),
                call_id: "call_1".to_string(),
                output: FunctionCallOutputPayload::Text("ok".to_string()),
                status: MessageStatus::Completed,
            }),
        ],
        error: None,
        tools: Vec::new(),
        tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
        truncation: Truncation::Disabled,
        parallel_tool_calls: false,
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
        prompt_cache_key: None,
        safety_identifier: None,
        metadata: json!({}),
    };

    let unified_res: UnifiedResponse = responses_res.into();
    let items = &unified_res.choices[0].items;

    assert_eq!(items.len(), 4);
    assert!(matches!(&items[0], UnifiedItem::Message(_)));
    assert!(matches!(&items[1], UnifiedItem::Reasoning(_)));
    assert!(matches!(&items[2], UnifiedItem::FunctionCall(_)));
    assert!(matches!(
        &items[3],
        UnifiedItem::FunctionCallOutput(UnifiedFunctionCallOutputItem {
            tool_call_id,
            output: UnifiedToolResultOutput::Text { text },
            ..
        }) if tool_call_id == "call_1" && text == "ok"
    ));
}
