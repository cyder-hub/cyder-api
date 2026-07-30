use super::*;
use crate::service::transform::unified::*;
use serde_json::json;

#[test]
fn test_unified_request_to_ollama_request() {
    let unified_req = UnifiedRequest {
        model: Some("test-model".to_string()),
        messages: vec![
            UnifiedMessage {
                role: UnifiedRole::System,
                content: vec![UnifiedContentPart::Text {
                    text: "You are a bot.".to_string(),
                }],
            },
            UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text {
                    text: "Hello".to_string(),
                }],
            },
        ],
        stream: true,
        temperature: Some(0.8),
        max_tokens: Some(100),
        top_p: Some(0.9),
        stop: Some(vec!["\n".to_string()]),
        seed: Some(123),
        presence_penalty: Some(0.5),
        frequency_penalty: Some(0.6),
        tools: None,
        ..Default::default()
    };

    let ollama_req: OllamaRequestPayload = unified_req.into();

    assert_eq!(ollama_req.model, "test-model");
    assert_eq!(ollama_req.messages.len(), 2);
    assert_eq!(ollama_req.messages[0].role, "system");
    assert_eq!(ollama_req.messages[0].content, "You are a bot.");
    assert_eq!(ollama_req.messages[1].role, "user");
    assert_eq!(ollama_req.messages[1].content, "Hello");
    assert_eq!(ollama_req.stream, Some(true));
    let options = ollama_req.options.unwrap();
    assert_eq!(options.temperature, Some(0.8));
    assert_eq!(options.max_tokens, Some(100));
    assert_eq!(options.top_p, Some(0.9));
    assert_eq!(options.stop, Some(vec!["\n".to_string()]));
    assert_eq!(options.seed, Some(123));
    assert_eq!(options.presence_penalty, Some(0.5));
    assert_eq!(options.frequency_penalty, Some(0.6));
}

#[test]
fn test_unified_request_to_ollama_preserves_images_and_structured_fallback_text() {
    let unified_req = UnifiedRequest {
        model: Some("test-model".to_string()),
        messages: vec![
            UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::Text {
                        text: "Describe this".to_string(),
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
                ],
            },
            UnifiedMessage {
                role: UnifiedRole::Tool,
                content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                    tool_call_id: "call_1".to_string(),
                    name: Some("lookup".to_string()),
                    output: UnifiedToolResultOutput::Json {
                        value: json!({"ok": true}),
                    },
                })],
            },
        ],
        ..Default::default()
    };

    let ollama_req: OllamaRequestPayload = unified_req.into();

    assert_eq!(ollama_req.messages.len(), 2);
    assert_eq!(ollama_req.messages[0].role, "user");
    assert_eq!(
        ollama_req.messages[0].content,
        "Describe this\n\nfile_url: https://files.example.com/report.pdf\nmime_type: application/pdf\n\n```python\nprint(1)\n```"
    );
    assert_eq!(
        ollama_req.messages[0].images.as_ref(),
        Some(&vec!["ZmFrZQ==".to_string()])
    );
    assert_eq!(ollama_req.messages[1].role, "user");
    assert_eq!(
        ollama_req.messages[1].content,
        "tool_result: lookup\ntool_call_id: call_1\ncontent: {\"ok\":true}"
    );
}

#[test]
fn test_ollama_response_to_unified_response() {
    let ollama_res = OllamaResponse {
        model: "test-model".to_string(),
        created_at: "2023-12-12T18:34:13.014Z".to_string(),
        message: OllamaMessage {
            role: "assistant".to_string(),
            content: "Hello there!".to_string(),
            images: None,
        },
        done: true,
        done_reason: Some("stop".to_string()),
        prompt_tokens: Some(10),
        completion_tokens: Some(5),
        total_duration: None,
        load_duration: None,
        prompt_eval_duration: None,
        eval_duration: None,
    };

    let unified_res: UnifiedResponse = ollama_res.into();

    assert_eq!(unified_res.model, Some("test-model".to_string()));
    assert_eq!(unified_res.choices.len(), 1);
    let choice = &unified_res.choices[0];
    assert_eq!(choice.index, 0);
    assert_eq!(choice.message.role, UnifiedRole::Assistant);
    assert_eq!(
        choice.message.content,
        vec![UnifiedContentPart::Text {
            text: "Hello there!".to_string()
        }]
    );
    assert_eq!(choice.finish_reason, Some("stop".to_string()));
    let usage = unified_res.usage.unwrap();
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 5);
    assert_eq!(usage.total_tokens, 15);
}

#[test]
fn test_ollama_chunk_to_unified_chunk() {
    // Content chunk
    let ollama_chunk = OllamaChunkResponse {
        model: "llama2".to_string(),
        created_at: "2023-12-12T18:34:13.014Z".to_string(),
        message: Some(OllamaMessage {
            role: "assistant".to_string(),
            content: "Hello".to_string(),
            images: None,
        }),
        done: false,
        done_reason: None,
        prompt_tokens: None,
        completion_tokens: None,
        total_duration: None,
        load_duration: None,
        prompt_eval_duration: None,
        eval_duration: None,
    };

    let unified_chunk: UnifiedChunkResponse = ollama_chunk.into();

    assert_eq!(unified_chunk.model, Some("llama2".to_string()));
    assert_eq!(unified_chunk.choices.len(), 1);
    let choice = &unified_chunk.choices[0];
    assert_eq!(choice.index, 0);
    assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
    assert_eq!(
        choice.delta.content,
        vec![UnifiedContentPartDelta::TextDelta {
            index: 0,
            text: "Hello".to_string()
        }]
    );
    assert!(choice.finish_reason.is_none());
    assert!(unified_chunk.usage.is_none());

    // Final chunk
    let ollama_final_chunk = OllamaChunkResponse {
        model: "llama2".to_string(),
        created_at: "2023-12-12T18:34:13.014Z".to_string(),
        message: None,
        done: true,
        done_reason: Some("stop".to_string()),
        prompt_tokens: Some(10),
        completion_tokens: Some(5),
        total_duration: None,
        load_duration: None,
        prompt_eval_duration: None,
        eval_duration: None,
    };

    let unified_final_chunk: UnifiedChunkResponse = ollama_final_chunk.into();
    assert_eq!(unified_final_chunk.model, Some("llama2".to_string()));
    assert_eq!(unified_final_chunk.choices.len(), 1);
    let final_choice = &unified_final_chunk.choices[0];
    assert!(final_choice.delta.role.is_none());
    assert!(final_choice.delta.content.is_empty());
    assert_eq!(final_choice.finish_reason, Some("stop".to_string()));
    let usage = unified_final_chunk.usage.unwrap();
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 5);
    assert_eq!(usage.total_tokens, 15);
}
