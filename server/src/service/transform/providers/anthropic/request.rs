use serde_json::{Map, Value, json};

use super::payload::*;

use crate::service::transform::unified::*;

fn build_anthropic_image_block(mime_type: &str, data: &str) -> Value {
    let mime_type = if mime_type == "image/jpg" {
        "image/jpeg"
    } else {
        mime_type
    };
    json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": mime_type,
            "data": data,
        }
    })
}

fn build_anthropic_image_url_block(url: &str) -> Value {
    if let Some(data_url) = crate::service::transform::media::parse_base64_data_url(url) {
        return build_anthropic_image_block(
            if data_url.mime_type == "image/jpg" {
                "image/jpeg"
            } else {
                data_url.mime_type
            },
            data_url.data,
        );
    }
    json!({
        "type": "image",
        "source": {"type": "url", "url": url}
    })
}

fn build_anthropic_document_block(source: Value, filename: Option<&str>) -> Value {
    let mut block = Map::from_iter([
        ("type".to_string(), Value::String("document".to_string())),
        ("source".to_string(), source),
    ]);
    if let Some(filename) = filename.filter(|filename| !filename.trim().is_empty()) {
        block.insert("title".to_string(), Value::String(filename.to_string()));
    }
    Value::Object(block)
}

fn anthropic_tool_result_content(output: &UnifiedToolResultOutput) -> String {
    match output {
        UnifiedToolResultOutput::Error {
            error: Value::String(text),
        } => text.clone(),
        UnifiedToolResultOutput::Error { error } => {
            serde_json::to_string(error).unwrap_or_else(|_| error.to_string())
        }
        output => stringify_unified_tool_result_output(output),
    }
}

pub(super) fn render_anthropic_image_reference_text(url: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) if !detail.is_empty() => format!("image_url: {url}\ndetail: {detail}"),
        _ => format!("image_url: {url}"),
    }
}

pub(super) fn render_anthropic_file_reference_text(
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

pub(super) fn render_anthropic_inline_file_data_text(
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

pub(super) fn render_anthropic_executable_code_text(language: &str, code: &str) -> String {
    format!("```{language}\n{code}\n```")
}

fn choice_parallel_setting(choice: Option<&AnthropicToolChoice>) -> Option<bool> {
    choice.and_then(|choice| choice.disable_parallel_tool_use.map(|disabled| !disabled))
}

fn anthropic_tool_choice(
    choice: Option<UnifiedToolChoice>,
    parallel_tool_calls: Option<bool>,
) -> Option<AnthropicToolChoice> {
    if choice.is_none() && parallel_tool_calls.is_none() {
        return None;
    }
    let (type_, name) = match choice.unwrap_or(UnifiedToolChoice::Auto) {
        UnifiedToolChoice::None => ("none", None),
        UnifiedToolChoice::Required => ("any", None),
        UnifiedToolChoice::Named { name } => ("tool", Some(name)),
        UnifiedToolChoice::Allowed {
            mode: UnifiedAllowedToolMode::Required,
            ..
        } => ("any", None),
        UnifiedToolChoice::Auto
        | UnifiedToolChoice::Allowed {
            mode: UnifiedAllowedToolMode::Auto,
            ..
        } => ("auto", None),
    };
    Some(AnthropicToolChoice {
        type_: type_.to_string(),
        name,
        disable_parallel_tool_use: parallel_tool_calls.map(|enabled| !enabled),
    })
}

impl From<AnthropicRequestPayload> for UnifiedRequest {
    fn from(anthropic_req: AnthropicRequestPayload) -> Self {
        let (tool_choice, parallel_tool_calls) = anthropic_req
            .tool_choice
            .as_ref()
            .map(|choice| {
                let choice = match choice.type_.as_str() {
                    "none" => UnifiedToolChoice::None,
                    "any" => UnifiedToolChoice::Required,
                    "tool" => UnifiedToolChoice::Named {
                        name: choice
                            .name
                            .clone()
                            .expect("Anthropic named tool choice is adapter-validated"),
                    },
                    _ => UnifiedToolChoice::Auto,
                };
                (
                    Some(choice),
                    choice_parallel_setting(anthropic_req.tool_choice.as_ref()),
                )
            })
            .unwrap_or((None, None));
        let structured_output = anthropic_req
            .output_config
            .as_ref()
            .and_then(|config| config.format.as_ref())
            .map(|format| match format {
                AnthropicOutputFormat::JsonSchema { schema } => {
                    UnifiedStructuredOutput::JsonSchema {
                        name: crate::service::transform::structured::stable_schema_name(schema),
                        description: None,
                        schema: schema.clone(),
                        strict: true,
                    }
                }
            });
        let reasoning_effort = anthropic_req
            .output_config
            .as_ref()
            .and_then(|config| config.effort.as_ref())
            .map(|effort| match effort {
                AnthropicEffort::Low => UnifiedReasoningEffort::Low,
                AnthropicEffort::Medium => UnifiedReasoningEffort::Medium,
                AnthropicEffort::High => UnifiedReasoningEffort::High,
                AnthropicEffort::Xhigh | AnthropicEffort::Max => UnifiedReasoningEffort::Xhigh,
            })
            .or_else(|| {
                anthropic_req
                    .thinking
                    .as_ref()
                    .map(|thinking| match thinking.type_ {
                        AnthropicThinkingType::Disabled => UnifiedReasoningEffort::None,
                        AnthropicThinkingType::Adaptive | AnthropicThinkingType::Enabled => {
                            UnifiedReasoningEffort::High
                        }
                    })
            });
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
                        Some("image") => {
                            let source = block
                                .get("source")
                                .expect("Anthropic image source is adapter-validated");
                            match source.get("type").and_then(Value::as_str) {
                                Some("base64") => {
                                    content_parts.push(UnifiedContentPart::ImageData {
                                        mime_type: source
                                            .get("media_type")
                                            .and_then(Value::as_str)
                                            .expect(
                                                "Anthropic image media type is adapter-validated",
                                            )
                                            .to_string(),
                                        data: source
                                            .get("data")
                                            .and_then(Value::as_str)
                                            .expect("Anthropic image data is adapter-validated")
                                            .to_string(),
                                    });
                                }
                                Some("url") => {
                                    content_parts.push(UnifiedContentPart::ImageUrl {
                                        url: source
                                            .get("url")
                                            .and_then(Value::as_str)
                                            .expect("Anthropic image URL is adapter-validated")
                                            .to_string(),
                                        detail: None,
                                    });
                                }
                                _ => unreachable!("Anthropic image source is adapter-validated"),
                            }
                        }
                        Some("document") => {
                            let source = block
                                .get("source")
                                .expect("Anthropic document source is adapter-validated");
                            let mime_type = source
                                .get("media_type")
                                .and_then(Value::as_str)
                                .expect("Anthropic document media type is adapter-validated")
                                .to_string();
                            let filename = block
                                .get("title")
                                .and_then(Value::as_str)
                                .filter(|title| !title.trim().is_empty())
                                .map(ToString::to_string)
                                .or_else(|| {
                                    crate::service::transform::media::default_filename_for_mime(
                                        &mime_type,
                                    )
                                    .map(ToString::to_string)
                                });
                            content_parts.push(UnifiedContentPart::FileData {
                                data: source
                                    .get("data")
                                    .and_then(Value::as_str)
                                    .expect("Anthropic document data is adapter-validated")
                                    .to_string(),
                                mime_type,
                                filename,
                            });
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
                            if let Some(tool_use_id) =
                                block.get("tool_use_id").and_then(|v| v.as_str())
                            {
                                let content_val = block
                                    .get("content")
                                    .cloned()
                                    .unwrap_or_else(|| Value::String(String::new()));
                                // Look up the tool name from our mapping
                                let tool_name = tool_id_to_name
                                    .get(tool_use_id)
                                    .cloned()
                                    .map(Some)
                                    .unwrap_or(None);

                                let output = unified_tool_result_output_from_value(content_val);
                                let output = if block.get("is_error").and_then(Value::as_bool)
                                    == Some(true)
                                {
                                    match output {
                                        UnifiedToolResultOutput::Error { error } => {
                                            UnifiedToolResultOutput::Error { error }
                                        }
                                        output => UnifiedToolResultOutput::Error {
                                            error: unified_tool_result_output_to_value(&output),
                                        },
                                    }
                                } else {
                                    output
                                };
                                content_parts.push(UnifiedContentPart::ToolResult(
                                    UnifiedToolResult {
                                        tool_call_id: tool_use_id.to_string(),
                                        name: tool_name,
                                        output,
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
                        strict: tool.strict,
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
            tool_choice,
            parallel_tool_calls,
            stream: anthropic_req.stream.unwrap_or(false),
            temperature: anthropic_req.temperature,
            max_tokens: Some(anthropic_req.max_tokens),
            top_p: anthropic_req.top_p,
            stop: anthropic_req.stop_sequences,
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            reasoning_effort,
            structured_output,
            extensions: (!anthropic_extension.is_empty()).then_some(UnifiedRequestExtensions {
                anthropic: Some(anthropic_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
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
                    // Anthropic system instructions are text-only. Rich parts admitted by the
                    // adapter policy are rendered deterministically before reaching this point.
                    let system_text = msg
                        .content
                        .iter()
                        .filter_map(|part| match part {
                            UnifiedContentPart::Text { text }
                            | UnifiedContentPart::Reasoning { text } => Some(text.clone()),
                            UnifiedContentPart::ImageUrl { url, detail } => Some(
                                render_anthropic_image_reference_text(url, detail.as_deref()),
                            ),
                            UnifiedContentPart::FileUrl {
                                url,
                                mime_type,
                                filename,
                            } => Some(render_anthropic_file_reference_text(
                                url,
                                mime_type.as_deref(),
                                filename.as_deref(),
                            )),
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename,
                            } => Some(render_anthropic_inline_file_data_text(
                                data,
                                mime_type,
                                filename.as_deref(),
                            )),
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                Some(render_anthropic_executable_code_text(language, code))
                            }
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
                                let _ = detail;
                                content_blocks.push(build_anthropic_image_url_block(&url));
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                content_blocks.push(build_anthropic_image_block(&mime_type, &data));
                            }
                            UnifiedContentPart::AudioData { .. }
                            | UnifiedContentPart::FileId { .. } => {}
                            UnifiedContentPart::FileUrl {
                                url,
                                mime_type,
                                filename,
                            } => {
                                let resolved_mime = mime_type
                                    .as_deref()
                                    .or_else(|| {
                                        filename.as_deref().and_then(
                                            crate::service::transform::media::mime_type_from_filename,
                                        )
                                    })
                                    .or_else(|| {
                                        crate::service::transform::media::mime_type_from_url(&url)
                                    });
                                if resolved_mime != Some("application/pdf") {
                                    continue;
                                }
                                content_blocks.push(build_anthropic_document_block(
                                    json!({"type":"url","url":url}),
                                    filename.as_deref(),
                                ));
                            }
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename,
                            } => {
                                let source = if mime_type == "application/pdf" {
                                    json!({"type":"base64","media_type":mime_type,"data":data})
                                } else if let Some(text) =
                                    crate::service::transform::media::decode_base64_utf8(&data)
                                {
                                    json!({
                                        "type":"text",
                                        "media_type":"text/plain",
                                        "data":text
                                    })
                                } else {
                                    continue;
                                };
                                content_blocks.push(build_anthropic_document_block(
                                    source,
                                    filename.as_deref(),
                                ));
                            }
                            UnifiedContentPart::ExecutableCode { .. } => {}
                            UnifiedContentPart::ToolCall(call) => {
                                content_blocks.push(json!({
                                    "type": "tool_use",
                                    "id": call.id,
                                    "name": call.name,
                                    "input": call.arguments
                                }));
                            }
                            UnifiedContentPart::ToolResult(result) => {
                                let is_error =
                                    matches!(&result.output, UnifiedToolResultOutput::Error { .. });
                                let content = anthropic_tool_result_content(&result.output);
                                let mut block = json!({
                                    "type": "tool_result",
                                    "tool_use_id": result.tool_call_id,
                                    "content": content
                                });
                                if is_error {
                                    block["is_error"] = Value::Bool(true);
                                }
                                content_blocks.push(block);
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

        let allowed_names = match unified_req.tool_choice.as_ref() {
            Some(UnifiedToolChoice::Allowed { names, .. }) => Some(names),
            _ => None,
        };
        let tools = unified_req.tools.map(|ts| {
            ts.into_iter()
                .filter(|tool| {
                    allowed_names.is_none_or(|names| names.contains(&tool.function.name))
                })
                .map(|tool| AnthropicTool {
                    name: tool.function.name,
                    description: tool.function.description,
                    input_schema: tool.function.parameters,
                    strict: tool.function.strict,
                })
                .collect()
        });
        let tool_choice =
            anthropic_tool_choice(unified_req.tool_choice, unified_req.parallel_tool_calls);
        let (thinking, reasoning_effort) = match unified_req.reasoning_effort {
            None => (None, None),
            Some(UnifiedReasoningEffort::None) => (
                Some(AnthropicThinkingConfig {
                    type_: AnthropicThinkingType::Disabled,
                    budget_tokens: None,
                    display: None,
                }),
                None,
            ),
            Some(effort) => {
                let effort = match effort {
                    UnifiedReasoningEffort::None => unreachable!("handled above"),
                    UnifiedReasoningEffort::Minimal | UnifiedReasoningEffort::Low => {
                        AnthropicEffort::Low
                    }
                    UnifiedReasoningEffort::Medium => AnthropicEffort::Medium,
                    UnifiedReasoningEffort::High => AnthropicEffort::High,
                    UnifiedReasoningEffort::Xhigh => AnthropicEffort::Xhigh,
                };
                (
                    Some(AnthropicThinkingConfig {
                        type_: AnthropicThinkingType::Adaptive,
                        budget_tokens: None,
                        display: None,
                    }),
                    Some(effort),
                )
            }
        };
        let output_format = match unified_req.structured_output {
            Some(UnifiedStructuredOutput::JsonSchema { schema, .. }) => {
                Some(AnthropicOutputFormat::JsonSchema { schema })
            }
            Some(UnifiedStructuredOutput::JsonObject) | None => None,
        };
        let output_config = (reasoning_effort.is_some() || output_format.is_some()).then_some(
            AnthropicOutputConfig {
                effort: reasoning_effort,
                format: output_format,
            },
        );

        AnthropicRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            system: system.map(AnthropicSystemPrompt::String),
            messages,
            max_tokens: unified_req.max_tokens.unwrap_or(4096), // Anthropic requires max_tokens
            tools,
            tool_choice,
            temperature: unified_req.temperature,
            top_p: unified_req.top_p,
            stop_sequences: unified_req.stop,
            stream: Some(unified_req.stream),
            metadata: anthropic_extension.metadata,
            top_k: anthropic_extension.top_k,
            thinking,
            output_config,
        }
    }
}
