use serde_json::{Value, json};

use super::payload::*;

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::{TransformProtocol, apply_transform_policy, unified::*};

fn build_anthropic_image_block(mime_type: &str, data: &str) -> Value {
    json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": mime_type,
            "data": data,
        }
    })
}

fn render_anthropic_image_reference_text(url: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) if !detail.is_empty() => format!("image_url: {url}\ndetail: {detail}"),
        _ => format!("image_url: {url}"),
    }
}

fn render_anthropic_file_reference_text(
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

fn render_anthropic_inline_file_data_text(
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

fn render_anthropic_executable_code_text(language: &str, code: &str) -> String {
    format!("```{language}\n{code}\n```")
}

impl From<AnthropicRequestPayload> for UnifiedRequest {
    fn from(anthropic_req: AnthropicRequestPayload) -> Self {
        let mut messages = Vec::new();
        // Track tool call ID to name mapping for tool results
        let mut tool_id_to_name: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        if let Some(system_prompt) = anthropic_req.system {
            let text = match system_prompt {
                AnthropicSystemPrompt::String(s) => s,
                AnthropicSystemPrompt::Blocks(blocks) => blocks
                    .into_iter()
                    .filter(|b| b.type_ == "text")
                    .map(|b| b.text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            };

            if !text.is_empty() {
                messages.push(UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![UnifiedContentPart::Text { text }],
                });
            }
        }

        for msg in anthropic_req.messages {
            let role = match msg.role.as_str() {
                "user" => UnifiedRole::User,
                "assistant" => UnifiedRole::Assistant,
                _ => UnifiedRole::User,
            };

            if let Some(s) = msg.content.as_str() {
                messages.push(UnifiedMessage {
                    role,
                    content: vec![UnifiedContentPart::Text {
                        text: s.to_string(),
                    }],
                });
            } else if let Some(blocks) = msg.content.as_array() {
                let mut content_parts = Vec::new();

                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                content_parts.push(UnifiedContentPart::Text {
                                    text: text.to_string(),
                                });
                            }
                        }
                        Some("tool_use") if role == UnifiedRole::Assistant => {
                            if let (Some(id), Some(name), Some(input)) = (
                                block.get("id").and_then(|v| v.as_str()),
                                block.get("name").and_then(|v| v.as_str()),
                                block.get("input"),
                            ) {
                                // Track the tool ID to name mapping
                                tool_id_to_name.insert(id.to_string(), name.to_string());

                                content_parts.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                                    id: id.to_string(),
                                    name: name.to_string(),
                                    arguments: input.clone(),
                                }));
                            }
                        }
                        Some("tool_result") if role == UnifiedRole::User => {
                            if let (Some(tool_use_id), Some(content_val)) = (
                                block.get("tool_use_id").and_then(|v| v.as_str()),
                                block.get("content"),
                            ) {
                                // Look up the tool name from our mapping
                                let tool_name = tool_id_to_name
                                    .get(tool_use_id)
                                    .cloned()
                                    .map(Some)
                                    .unwrap_or(None);

                                content_parts.push(UnifiedContentPart::ToolResult(
                                    UnifiedToolResult {
                                        tool_call_id: tool_use_id.to_string(),
                                        name: tool_name,
                                        output: unified_tool_result_output_from_value(
                                            content_val.clone(),
                                        ),
                                    },
                                ));
                            }
                        }
                        _ => {}
                    }
                }

                if !content_parts.is_empty() {
                    let message_role = if content_parts
                        .iter()
                        .any(|p| matches!(p, UnifiedContentPart::ToolResult(_)))
                    {
                        UnifiedRole::Tool
                    } else {
                        role
                    };

                    messages.push(UnifiedMessage {
                        role: message_role,
                        content: content_parts,
                    });
                }
            }
        }

        let tools = anthropic_req.tools.map(|ts| {
            ts.into_iter()
                .map(|tool| UnifiedTool {
                    type_: "function".to_string(),
                    function: UnifiedFunctionDefinition {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.input_schema,
                    },
                })
                .collect()
        });

        let anthropic_extension = UnifiedAnthropicRequestExtension {
            metadata: anthropic_req.metadata,
            top_k: anthropic_req.top_k,
        };

        UnifiedRequest {
            model: Some(anthropic_req.model),
            messages,
            items: Vec::new(),
            tools,
            stream: anthropic_req.stream.unwrap_or(false),
            temperature: anthropic_req.temperature,
            max_tokens: Some(anthropic_req.max_tokens),
            top_p: anthropic_req.top_p,
            stop: anthropic_req.stop_sequences,
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            extensions: (!anthropic_extension.is_empty()).then_some(UnifiedRequestExtensions {
                anthropic: Some(anthropic_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
        .filter_empty() // Filter out empty content and messages
    }
}

impl From<UnifiedRequest> for AnthropicRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let anthropic_extension = unified_req
            .anthropic_extension()
            .cloned()
            .unwrap_or_default();
        let mut system = None;
        let mut messages = Vec::new();

        for msg in unified_req.messages {
            match msg.role {
                UnifiedRole::System => {
                    // Combine all text parts from the system message content
                    let system_text = msg
                        .content
                        .iter()
                        .filter_map(|part| match part {
                            UnifiedContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !system_text.is_empty() {
                        system = Some(system_text);
                    }
                }
                UnifiedRole::User | UnifiedRole::Assistant | UnifiedRole::Tool => {
                    let role_str = match msg.role {
                        UnifiedRole::User => "user",
                        UnifiedRole::Assistant => "assistant",
                        UnifiedRole::Tool => "user", // Tool results are sent with the user role in Anthropic
                        _ => unreachable!(),
                    };

                    let mut content_blocks: Vec<Value> = Vec::new();
                    for part in msg.content {
                        match part {
                            UnifiedContentPart::Text { text } => {
                                content_blocks.push(json!({ "type": "text", "text": text }));
                            }
                            UnifiedContentPart::Refusal { text } => {
                                content_blocks.push(json!({ "type": "text", "text": text }));
                            }
                            UnifiedContentPart::Reasoning { text } => {
                                content_blocks.push(json!({ "type": "text", "text": text }));
                            }
                            UnifiedContentPart::ImageUrl { url, detail } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Anthropic),
                                    TransformValueKind::ImageUrl,
                                    "Downgrading remote image URL to recoverable text during Anthropic request conversion.",
                                ) {
                                    content_blocks.push(json!({
                                        "type": "text",
                                        "text": render_anthropic_image_reference_text(
                                            &url,
                                            detail.as_deref(),
                                        )
                                    }));
                                }
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                content_blocks.push(build_anthropic_image_block(&mime_type, &data));
                            }
                            UnifiedContentPart::FileUrl {
                                url,
                                mime_type,
                                filename,
                            } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Anthropic),
                                    TransformValueKind::FileUrl,
                                    "Downgrading file reference to recoverable text during Anthropic request conversion.",
                                ) {
                                    content_blocks.push(json!({
                                        "type": "text",
                                        "text": render_anthropic_file_reference_text(&url, mime_type.as_deref(), filename.as_deref())
                                    }));
                                }
                            }
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename,
                            } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Anthropic),
                                    TransformValueKind::FileData,
                                    "Downgrading inline file data to recoverable text during Anthropic request conversion.",
                                ) {
                                    content_blocks.push(json!({
                                        "type": "text",
                                        "text": render_anthropic_inline_file_data_text(&data, &mime_type, filename.as_deref())
                                    }));
                                }
                            }
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Anthropic),
                                    TransformValueKind::ExecutableCode,
                                    "Downgrading executable code to fenced text during Anthropic request conversion.",
                                ) {
                                    content_blocks.push(json!({
                                        "type": "text",
                                        "text": render_anthropic_executable_code_text(&language, &code)
                                    }));
                                }
                            }
                            UnifiedContentPart::ToolCall(call) => {
                                content_blocks.push(json!({
                                    "type": "tool_use",
                                    "id": call.id,
                                    "name": call.name,
                                    "input": call.arguments
                                }));
                            }
                            UnifiedContentPart::ToolResult(result) => {
                                content_blocks.push(json!({
                                    "type": "tool_result",
                                    "tool_use_id": result.tool_call_id,
                                    "content": result.output_value()
                                }));
                            }
                        }
                    }

                    // Anthropic's API has a special case for single-text-block messages
                    // where the `content` can be a plain string.
                    let content = if content_blocks.len() == 1
                        && content_blocks[0].get("type").and_then(|t| t.as_str()) == Some("text")
                    {
                        content_blocks.remove(0)["text"].take()
                    } else {
                        json!(content_blocks)
                    };

                    messages.push(AnthropicMessage {
                        role: role_str.to_string(),
                        content,
                    });
                }
            }
        }

        let tools = unified_req.tools.map(|ts| {
            ts.into_iter()
                .map(|tool| AnthropicTool {
                    name: tool.function.name,
                    description: tool.function.description,
                    input_schema: tool.function.parameters,
                })
                .collect()
        });

        AnthropicRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            system: system.map(AnthropicSystemPrompt::String),
            messages,
            max_tokens: unified_req.max_tokens.unwrap_or(4096), // Anthropic requires max_tokens
            tools,
            temperature: unified_req.temperature,
            top_p: unified_req.top_p,
            stop_sequences: unified_req.stop,
            stream: Some(unified_req.stream),
            metadata: anthropic_extension.metadata,
            top_k: anthropic_extension.top_k,
        }
    }
}
