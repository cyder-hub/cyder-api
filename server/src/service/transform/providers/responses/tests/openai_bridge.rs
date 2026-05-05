use super::*;

#[test]
fn test_responses_refusal_survives_cross_provider_unified_conversion() {
    let responses_res = ResponsesResponse {
        id: "resp_refusal_cross_provider".to_string(),
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
        &unified_res.choices[0].items[0],
        UnifiedItem::Message(UnifiedMessageItem { content, .. })
        if matches!(
            &content[..],
            [UnifiedContentPart::Refusal { text }, UnifiedContentPart::Text { text: answer }]
            if text == "cannot comply" && answer == "safe answer"
        )
    ));

    let openai_res: openai::OpenAiResponse = unified_res.into();
    let openai_json = serde_json::to_value(openai_res).unwrap();
    assert_eq!(
        openai_json["choices"][0]["message"]["refusal"],
        json!("cannot comply")
    );
    assert_eq!(
        openai_json["choices"][0]["message"]["content"],
        json!("safe answer")
    );
}
