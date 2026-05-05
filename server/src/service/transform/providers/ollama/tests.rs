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
fn test_ollama_request_to_unified_request() {
    let ollama_req = OllamaRequestPayload {
        model: "test-model".to_string(),
        messages: vec![
            OllamaMessage {
                role: "system".to_string(),
                content: "You are a bot.".to_string(),
                images: None,
            },
            OllamaMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
                images: None,
            },
        ],
        stream: Some(true),
        options: Some(OllamaOptions {
            temperature: Some(0.8),
            max_tokens: Some(100),
            top_p: Some(0.9),
            stop: Some(vec!["\n".to_string()]),
            seed: Some(123),
            presence_penalty: Some(0.5),
            frequency_penalty: Some(0.6),
        }),
        format: None,
        keep_alive: None,
    };

    let unified_req: UnifiedRequest = ollama_req.into();

    assert_eq!(unified_req.model, Some("test-model".to_string()));
    assert_eq!(unified_req.messages.len(), 2);
    assert_eq!(unified_req.messages[0].role, UnifiedRole::System);
    assert_eq!(
        unified_req.messages[0].content,
        vec![UnifiedContentPart::Text {
            text: "You are a bot.".to_string()
        }]
    );
    assert_eq!(unified_req.messages[1].role, UnifiedRole::User);
    assert_eq!(
        unified_req.messages[1].content,
        vec![UnifiedContentPart::Text {
            text: "Hello".to_string()
        }]
    );
    assert_eq!(unified_req.stream, true);
    assert_eq!(unified_req.temperature, Some(0.8));
    assert_eq!(unified_req.max_tokens, Some(100));
    assert_eq!(unified_req.top_p, Some(0.9));
    assert_eq!(unified_req.stop, Some(vec!["\n".to_string()]));
    assert_eq!(unified_req.seed, Some(123));
    assert_eq!(unified_req.presence_penalty, Some(0.5));
    assert_eq!(unified_req.frequency_penalty, Some(0.6));
    assert_eq!(
        unified_req
            .ollama_extension()
            .and_then(|extension| extension.format.clone()),
        None
    );
    assert_eq!(
        unified_req
            .ollama_extension()
            .and_then(|extension| extension.keep_alive.clone()),
        None
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
fn test_unified_response_to_ollama_response() {
    let unified_res = UnifiedResponse {
        id: "cmpl-123".to_string(),
        model: Some("test-model".to_string()),
        choices: vec![UnifiedChoice {
            index: 0,
            message: UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: vec![UnifiedContentPart::Text {
                    text: "Hello there!".to_string(),
                }],
                ..Default::default()
            },
            items: Vec::new(),
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: Some(UnifiedUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            ..Default::default()
        }),
        created: Some(12345),
        object: Some("chat.completion.chunk".to_string()),
        system_fingerprint: None,
        provider_response_metadata: None,
        synthetic_metadata: None,
    };

    let ollama_res: OllamaResponse = unified_res.into();

    assert_eq!(ollama_res.model, "test-model");
    assert_eq!(ollama_res.message.role, "assistant");
    assert_eq!(ollama_res.message.content, "Hello there!");
    assert!(ollama_res.done);
    assert_eq!(ollama_res.prompt_tokens, Some(10));
    assert_eq!(ollama_res.completion_tokens, Some(5));
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

#[test]
fn test_unified_chunk_to_ollama_chunk() {
    // Content chunk
    let unified_chunk = UnifiedChunkResponse {
        id: "cmpl-123".to_string(),
        model: Some("test-model".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![UnifiedContentPartDelta::TextDelta {
                    index: 0,
                    text: " World".to_string(),
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

    let ollama_chunk: OllamaChunkResponse = unified_chunk.into();

    assert_eq!(ollama_chunk.model, "test-model");
    assert!(!ollama_chunk.done);
    let message = ollama_chunk.message.unwrap();
    assert_eq!(message.role, "assistant");
    assert_eq!(message.content, " World");
    assert!(message.images.is_none());
    assert!(ollama_chunk.prompt_tokens.is_none());
    assert!(ollama_chunk.completion_tokens.is_none());

    // Final chunk
    let unified_final_chunk = UnifiedChunkResponse {
        id: "cmpl-123".to_string(),
        model: Some("test-model".to_string()),
        choices: vec![UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta::default(),
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(UnifiedUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            ..Default::default()
        }),
        created: Some(12345),
        object: Some("chat.completion.chunk".to_string()),
        provider_session_metadata: None,
        synthetic_metadata: None,
    };

    let ollama_final_chunk: OllamaChunkResponse = unified_final_chunk.into();
    assert_eq!(ollama_final_chunk.model, "test-model");
    assert!(ollama_final_chunk.done);
    assert!(ollama_final_chunk.message.is_none());
    assert_eq!(ollama_final_chunk.prompt_tokens, Some(10));
    assert_eq!(ollama_final_chunk.completion_tokens, Some(5));
}
