use super::payload::{OllamaMessage, OllamaOptions, OllamaRequestPayload};

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::{TransformProtocol, apply_transform_policy, unified::*};

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
