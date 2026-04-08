use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::{
    StreamTransformer, TransformProtocol, TransformValueKind, apply_transform_policy,
    build_stream_diagnostic_sse, unified::*,
};
use crate::schema::enum_def::LlmApiType;
use crate::utils::ID_GENERATOR;
use crate::utils::sse::SseEvent;

fn build_ollama_stream_diagnostic(
    transformer: &mut StreamTransformer,
    kind: TransformValueKind,
    context: String,
) -> SseEvent {
    build_stream_diagnostic_sse(
        transformer,
        TransformProtocol::Unified,
        TransformProtocol::Api(LlmApiType::Ollama),
        kind,
        "ollama_stream_encoding",
        context,
        None,
        Some(
            "Use an OpenAI, Responses, or Anthropic target when structured tool/reasoning/blob stream events must remain recoverable.".to_string(),
        ),
    )
}

fn render_ollama_file_reference_text(
    url: &str,
    mime_type: Option<&str>,
    filename: Option<&str>,
) -> String {
    let mut lines = vec![format!("file_url: {url}")];
    if let Some(filename) = filename.filter(|value| !value.is_empty()) {
        lines.push(format!("filename: {filename}"));
    }
    if let Some(mime_type) = mime_type.filter(|value| !value.is_empty()) {
        lines.push(format!("mime_type: {mime_type}"));
    }
    lines.join("\n")
}

fn render_ollama_inline_file_data_text(
    data: &str,
    mime_type: &str,
    filename: Option<&str>,
) -> String {
    let mut lines = vec![
        format!("file_data: {data}"),
        format!("mime_type: {mime_type}"),
    ];
    if let Some(filename) = filename.filter(|value| !value.is_empty()) {
        lines.push(format!("filename: {filename}"));
    }
    lines.join("\n")
}

fn render_ollama_executable_code_text(language: &str, code: &str) -> String {
    format!("```{language}\n{code}\n```")
}

fn append_ollama_text_segment(buffer: &mut String, segment: String) {
    if buffer.is_empty() {
        buffer.push_str(&segment);
    } else {
        buffer.push_str("\n\n");
        buffer.push_str(&segment);
    }
}

// --- Ollama to Unified ---

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaRequestPayload {
    pub model: String,
    pub messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<OllamaOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>, // Base64 encoded images
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(rename = "num_predict")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
}

impl From<OllamaRequestPayload> for UnifiedRequest {
    fn from(ollama_req: OllamaRequestPayload) -> Self {
        let messages = ollama_req
            .messages
            .into_iter()
            .map(|msg| {
                let role = match msg.role.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    _ => UnifiedRole::User, // Default to user
                };
                // NOTE: Ollama's `images` field is ignored for now.
                // UnifiedRequest would need to be updated to handle multimodal content.
                let content = vec![UnifiedContentPart::Text { text: msg.content }];
                UnifiedMessage { role, content }
            })
            .collect();

        let (temperature, max_tokens, top_p, stop, seed, presence_penalty, frequency_penalty) =
            if let Some(options) = ollama_req.options {
                (
                    options.temperature,
                    options.max_tokens,
                    options.top_p,
                    options.stop,
                    options.seed,
                    options.presence_penalty,
                    options.frequency_penalty,
                )
            } else {
                (None, None, None, None, None, None, None)
            };

        let ollama_extension = UnifiedOllamaRequestExtension {
            format: ollama_req.format,
            keep_alive: ollama_req.keep_alive,
        };

        UnifiedRequest {
            model: Some(ollama_req.model),
            messages,
            // Ollama doesn't support tools/function calling - always set to None
            tools: None,
            stream: ollama_req.stream.unwrap_or(false),
            temperature,
            max_tokens,
            top_p,
            stop,
            seed,
            presence_penalty,
            frequency_penalty,
            extensions: (!ollama_extension.is_empty()).then_some(UnifiedRequestExtensions {
                ollama: Some(ollama_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
        .filter_empty() // Filter out empty content and messages
    }
}

impl From<UnifiedRequest> for OllamaRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let ollama_extension = unified_req.ollama_extension().cloned().unwrap_or_default();
        let messages = unified_req
            .messages
            .into_iter()
            .filter_map(|msg| {
                let role = match msg.role {
                    UnifiedRole::System => "system",
                    UnifiedRole::User => "user",
                    UnifiedRole::Assistant => "assistant",
                    UnifiedRole::Tool => {
                        apply_transform_policy(
                            TransformProtocol::Unified,
                            TransformProtocol::Api(LlmApiType::Ollama),
                            TransformValueKind::ToolRoleMessage,
                            "Downgrading tool-role message to user text during Ollama request conversion.",
                        );
                        "user"
                    }
                }
                .to_string();

                let mut final_content = String::new();
                let mut images = Vec::new();

                for part in msg.content {
                    match part {
                        UnifiedContentPart::Text { text } => {
                            append_ollama_text_segment(&mut final_content, text);
                        }
                        UnifiedContentPart::Refusal { text } => {
                            if apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Ollama),
                                TransformValueKind::Refusal,
                                "Downgrading refusal content to plain text during Ollama request conversion.",
                            ) {
                                append_ollama_text_segment(&mut final_content, text);
                            }
                        }
                        UnifiedContentPart::Reasoning { text } => {
                            if apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Ollama),
                                TransformValueKind::ReasoningContent,
                                "Downgrading reasoning content to plain text during Ollama request conversion.",
                            ) {
                                append_ollama_text_segment(&mut final_content, text);
                            }
                        }
                        UnifiedContentPart::ImageData { data, .. } => {
                            images.push(data);
                        }
                        UnifiedContentPart::ImageUrl { url, detail } => {
                            if apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Ollama),
                                TransformValueKind::ImageUrl,
                                "Downgrading image URL to recoverable text during Ollama request conversion.",
                            ) {
                                append_ollama_text_segment(
                                    &mut final_content,
                                    match detail {
                                        Some(detail) if !detail.is_empty() => {
                                            format!("image_url: {url}\ndetail: {detail}")
                                        }
                                        _ => format!("image_url: {url}"),
                                    },
                                );
                            }
                        }
                        UnifiedContentPart::FileUrl {
                            url,
                            mime_type,
                            filename,
                        } => {
                            if apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Ollama),
                                TransformValueKind::FileUrl,
                                "Downgrading file reference to recoverable text during Ollama request conversion.",
                            ) {
                                append_ollama_text_segment(
                                    &mut final_content,
                                    render_ollama_file_reference_text(
                                        &url,
                                        mime_type.as_deref(),
                                        filename.as_deref(),
                                    ),
                                );
                            }
                        }
                        UnifiedContentPart::FileData {
                            data,
                            mime_type,
                            filename,
                        } => {
                            if apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Ollama),
                                TransformValueKind::FileData,
                                "Downgrading inline file data to recoverable text during Ollama request conversion.",
                            ) {
                                append_ollama_text_segment(
                                    &mut final_content,
                                    render_ollama_inline_file_data_text(
                                        &data,
                                        &mime_type,
                                        filename.as_deref(),
                                    ),
                                );
                            }
                        }
                        UnifiedContentPart::ExecutableCode { language, code } => {
                            if apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Ollama),
                                TransformValueKind::ExecutableCode,
                                "Downgrading executable code to fenced text during Ollama request conversion.",
                            ) {
                                append_ollama_text_segment(
                                    &mut final_content,
                                    render_ollama_executable_code_text(&language, &code),
                                );
                            }
                        }
                        UnifiedContentPart::ToolCall(call) => {
                            if apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Ollama),
                                TransformValueKind::ToolCall,
                                "Downgrading tool call to recoverable text during Ollama request conversion.",
                            ) {
                                append_ollama_text_segment(
                                    &mut final_content,
                                    format!(
                                        "tool_call: {}\narguments: {}",
                                        call.name,
                                        serde_json::to_string(&call.arguments).unwrap_or_default()
                                    ),
                                );
                            }
                        }
                        UnifiedContentPart::ToolResult(result) => {
                            if apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Ollama),
                                TransformValueKind::ToolResult,
                                "Downgrading tool result to recoverable text during Ollama request conversion.",
                            ) {
                                append_ollama_text_segment(
                                    &mut final_content,
                                    match result.name {
                                        Some(ref name) if !name.is_empty() => format!(
                                            "tool_result: {name}\ntool_call_id: {}\ncontent: {}",
                                            result.tool_call_id,
                                            result.legacy_content()
                                        ),
                                        _ => format!(
                                            "tool_result_id: {}\ncontent: {}",
                                            result.tool_call_id,
                                            result.legacy_content()
                                        ),
                                    },
                                );
                            }
                        }
                    }
                }

                if final_content.is_empty() {
                    // Don't send empty messages? Or send empty string.
                    // Assuming we send what we have.
                }

                Some(OllamaMessage {
                    role,
                    content: final_content,
                    images: (!images.is_empty()).then_some(images),
                })
            })
            .collect();

        let options = if unified_req.temperature.is_some()
            || unified_req.max_tokens.is_some()
            || unified_req.top_p.is_some()
            || unified_req.stop.is_some()
            || unified_req.seed.is_some()
            || unified_req.presence_penalty.is_some()
            || unified_req.frequency_penalty.is_some()
        {
            Some(OllamaOptions {
                temperature: unified_req.temperature,
                max_tokens: unified_req.max_tokens,
                top_p: unified_req.top_p,
                stop: unified_req.stop,
                seed: unified_req.seed,
                presence_penalty: unified_req.presence_penalty,
                frequency_penalty: unified_req.frequency_penalty,
            })
        } else {
            None
        };

        OllamaRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            messages,
            stream: Some(unified_req.stream),
            options,
            format: ollama_extension.format,
            keep_alive: ollama_extension.keep_alive,
        }
    }
}

// --- Ollama Response to Unified ---

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaResponse {
    pub model: String,
    pub created_at: String,
    pub message: OllamaMessage,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_reason: Option<String>,
    #[serde(rename = "prompt_eval_count")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(rename = "eval_count")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_duration: Option<u64>,
}

impl From<OllamaResponse> for UnifiedResponse {
    fn from(ollama_res: OllamaResponse) -> Self {
        let message = UnifiedMessage {
            role: UnifiedRole::Assistant, // Ollama response is always assistant
            content: vec![UnifiedContentPart::Text {
                text: ollama_res.message.content,
            }],
        };

        let finish_reason = if ollama_res.done {
            ollama_res.done_reason.or_else(|| Some("stop".to_string()))
        } else {
            None
        };

        // Map Ollama's done_reason to unified finish_reason
        let finish_reason = finish_reason.map(|reason| {
            match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(), // Default to stop for other reasons
            }
        });

        let choice = UnifiedChoice {
            index: 0,
            message,
            items: Vec::new(),
            finish_reason,
            logprobs: None,
        };

        let usage = if let (Some(prompt_tokens), Some(completion_tokens)) =
            (ollama_res.prompt_tokens, ollama_res.completion_tokens)
        {
            Some(UnifiedUsage {
                input_tokens: prompt_tokens,
                output_tokens: completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                ..Default::default()
            })
        } else {
            None
        };

        UnifiedResponse {
            id: format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
            model: Some(ollama_res.model),
            choices: vec![choice],
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        }
    }
}

impl From<UnifiedResponse> for OllamaResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let choice = unified_res
            .choices
            .into_iter()
            .next()
            .unwrap_or_else(|| UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: None,
                logprobs: None,
            });

        let mut content = String::new();
        for part in choice.message.content {
            if let UnifiedContentPart::Text { text } = part {
                content.push_str(&text);
            }
        }

        let message = OllamaMessage {
            role: "assistant".to_string(),
            content,
            images: None,
        };

        let (prompt_tokens, completion_tokens) = if let Some(usage) = unified_res.usage {
            (Some(usage.input_tokens), Some(usage.output_tokens))
        } else {
            (None, None)
        };

        let done_reason = choice
            .finish_reason
            .as_ref()
            .map(|reason| match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(),
            });

        OllamaResponse {
            model: unified_res.model.unwrap_or_default(),
            created_at: Utc::now().to_rfc3339(),
            message,
            done: choice.finish_reason.is_some(),
            done_reason,
            prompt_tokens,
            completion_tokens,
            total_duration: None,
            load_duration: None,
            prompt_eval_duration: None,
            eval_duration: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}

// --- Ollama Chunk Response ---

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaChunkResponse {
    pub model: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<OllamaMessage>,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_reason: Option<String>,
    // Usage stats are only in the final chunk
    #[serde(rename = "prompt_eval_count")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(rename = "eval_count")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_duration: Option<u64>,
}

impl From<OllamaChunkResponse> for UnifiedChunkResponse {
    fn from(ollama_chunk: OllamaChunkResponse) -> Self {
        let delta = if let Some(message) = ollama_chunk.message {
            UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![UnifiedContentPartDelta::TextDelta {
                    index: 0,
                    text: message.content,
                }],
            }
        } else {
            UnifiedMessageDelta::default()
        };

        let finish_reason = if ollama_chunk.done {
            ollama_chunk
                .done_reason
                .or_else(|| Some("stop".to_string()))
        } else {
            None
        };

        // Map Ollama's done_reason to unified finish_reason
        let finish_reason = finish_reason.map(|reason| {
            match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(), // Default to stop for other reasons
            }
        });

        let choice = UnifiedChunkChoice {
            index: 0,
            delta,
            finish_reason,
        };

        let usage = if let (Some(prompt_tokens), Some(completion_tokens)) =
            (ollama_chunk.prompt_tokens, ollama_chunk.completion_tokens)
        {
            Some(UnifiedUsage {
                input_tokens: prompt_tokens,
                output_tokens: completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                ..Default::default()
            })
        } else {
            None
        };

        UnifiedChunkResponse {
            id: format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
            model: Some(ollama_chunk.model),
            choices: vec![choice],
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata: None,
            synthetic_metadata: None,
        }
    }
}

impl From<UnifiedChunkResponse> for OllamaChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let choice =
            unified_chunk
                .choices
                .into_iter()
                .next()
                .unwrap_or_else(|| UnifiedChunkChoice {
                    index: 0,
                    delta: UnifiedMessageDelta::default(),
                    finish_reason: None,
                });

        let mut final_content = String::new();
        for part in choice.delta.content {
            if let UnifiedContentPartDelta::TextDelta { text, .. } = part {
                final_content.push_str(&text);
            }
        }

        let message = if !final_content.is_empty() {
            Some(OllamaMessage {
                role: "assistant".to_string(),
                content: final_content,
                images: None,
            })
        } else {
            None
        };

        let (prompt_tokens, completion_tokens) = if let Some(usage) = unified_chunk.usage {
            (Some(usage.input_tokens), Some(usage.output_tokens))
        } else {
            (None, None)
        };

        let done_reason = choice
            .finish_reason
            .as_ref()
            .map(|reason| match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(),
            });

        OllamaChunkResponse {
            model: unified_chunk.model.unwrap_or_default(),
            created_at: Utc::now().to_rfc3339(),
            message,
            done: choice.finish_reason.is_some(),
            done_reason,
            prompt_tokens,
            completion_tokens,
            total_duration: None,
            load_duration: None,
            prompt_eval_duration: None,
            eval_duration: None,
        }
    }
}

pub(super) fn transform_unified_stream_events_to_ollama_events(
    stream_events: Vec<UnifiedStreamEvent>,
    transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut transformed = Vec::new();

    for event in stream_events {
        let model = transformer.get_or_default_stream_model();
        let maybe_event = match event {
            UnifiedStreamEvent::ContentBlockDelta { text, .. } => {
                serde_json::to_string(&OllamaChunkResponse {
                    model,
                    created_at: Utc::now().to_rfc3339(),
                    message: Some(OllamaMessage {
                        role: "assistant".to_string(),
                        content: text,
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
                })
                .ok()
                .map(|data| SseEvent {
                    data,
                    ..Default::default()
                })
            }
            UnifiedStreamEvent::MessageDelta { finish_reason } => {
                serde_json::to_string(&OllamaChunkResponse {
                    model,
                    created_at: Utc::now().to_rfc3339(),
                    message: None,
                    done: finish_reason.is_some(),
                    done_reason: finish_reason.as_ref().map(|reason| match reason.as_str() {
                        "stop" => "stop".to_string(),
                        "length" => "length".to_string(),
                        _ => "stop".to_string(),
                    }),
                    prompt_tokens: None,
                    completion_tokens: None,
                    total_duration: None,
                    load_duration: None,
                    prompt_eval_duration: None,
                    eval_duration: None,
                })
                .ok()
                .map(|data| SseEvent {
                    data,
                    ..Default::default()
                })
            }
            UnifiedStreamEvent::Usage { usage } => serde_json::to_string(&OllamaChunkResponse {
                model,
                created_at: Utc::now().to_rfc3339(),
                message: None,
                done: false,
                done_reason: None,
                prompt_tokens: Some(usage.input_tokens),
                completion_tokens: Some(usage.output_tokens),
                total_duration: None,
                load_duration: None,
                prompt_eval_duration: None,
                eval_duration: None,
            })
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
            }),
            UnifiedStreamEvent::ToolCallStart { index, id, name } => {
                Some(build_ollama_stream_diagnostic(
                    transformer,
                    TransformValueKind::ToolCallDelta,
                    format!(
                        "Ollama streaming only exposes plain assistant text chunks; tool_call_start index={index}, id={id}, name={name} was downgraded to a structured transform diagnostic."
                    ),
                ))
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                id,
                name,
                arguments,
                ..
            } => Some(build_ollama_stream_diagnostic(
                transformer,
                TransformValueKind::ToolCallDelta,
                format!(
                    "Ollama streaming only exposes plain assistant text chunks; tool_call_arguments_delta index={index}, id={id:?}, name={name:?}, chars={} was downgraded to a structured transform diagnostic.",
                    arguments.chars().count()
                ),
            )),
            UnifiedStreamEvent::ToolCallStop { index, id } => Some(build_ollama_stream_diagnostic(
                transformer,
                TransformValueKind::ToolCallDelta,
                format!(
                    "Ollama streaming only exposes plain assistant text chunks; tool_call_stop index={index}, id={id:?} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::ReasoningStart { index } => Some(build_ollama_stream_diagnostic(
                transformer,
                TransformValueKind::ReasoningDelta,
                format!(
                    "Ollama streaming does not expose reasoning_start; index={index} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::ReasoningDelta { index, text, .. } => {
                Some(build_ollama_stream_diagnostic(
                    transformer,
                    TransformValueKind::ReasoningDelta,
                    format!(
                        "Ollama streaming does not expose reasoning deltas; index={index}, chars={} was downgraded to a structured transform diagnostic.",
                        text.chars().count()
                    ),
                ))
            }
            UnifiedStreamEvent::ReasoningStop { index } => Some(build_ollama_stream_diagnostic(
                transformer,
                TransformValueKind::ReasoningDelta,
                format!(
                    "Ollama streaming does not expose reasoning_stop; index={index} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::BlobDelta { index, data } => Some(build_ollama_stream_diagnostic(
                transformer,
                TransformValueKind::BlobDelta,
                format!(
                    "Ollama streaming only exposes plain assistant text chunks; blob_delta index={index:?}, json_type={} was downgraded to a structured transform diagnostic.",
                    match &data {
                        serde_json::Value::Null => "null",
                        serde_json::Value::Bool(_) => "bool",
                        serde_json::Value::Number(_) => "number",
                        serde_json::Value::String(_) => "string",
                        serde_json::Value::Array(_) => "array",
                        serde_json::Value::Object(_) => "object",
                    }
                ),
            )),
            UnifiedStreamEvent::ItemAdded { .. }
            | UnifiedStreamEvent::ItemDone { .. }
            | UnifiedStreamEvent::MessageStart { .. }
            | UnifiedStreamEvent::MessageStop
            | UnifiedStreamEvent::ContentPartAdded { .. }
            | UnifiedStreamEvent::ContentPartDone { .. }
            | UnifiedStreamEvent::ContentBlockStart { .. }
            | UnifiedStreamEvent::ContentBlockStop { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
            | UnifiedStreamEvent::Error { .. } => None,
        };

        if let Some(event) = maybe_event {
            transformed.push(event);
        }
    }

    if transformed.is_empty() {
        None
    } else {
        Some(transformed)
    }
}

pub(super) fn transform_unified_chunk_to_ollama_events(
    unified_chunk: UnifiedChunkResponse,
    _transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    serde_json::to_string(&OllamaChunkResponse::from(unified_chunk))
        .ok()
        .map(|data| {
            vec![SseEvent {
                data,
                ..Default::default()
            }]
        })
}
