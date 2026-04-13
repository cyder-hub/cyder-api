use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    AnthropicActiveBlockKind, AnthropicActiveBlockState, AnthropicSessionState, StreamTransformer,
    TransformProtocol, TransformValueKind, apply_transform_policy, build_stream_diagnostic_sse,
    unified::*,
};
use crate::schema::enum_def::LlmApiType;
use crate::utils::ID_GENERATOR;
use crate::utils::sse::SseEvent;

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

fn build_anthropic_stream_diagnostic(
    transformer: &mut StreamTransformer,
    kind: TransformValueKind,
    context: String,
) -> SseEvent {
    build_stream_diagnostic_sse(
        transformer,
        TransformProtocol::Unified,
        TransformProtocol::Api(LlmApiType::Anthropic),
        kind,
        "anthropic_stream_encoding",
        context,
        None,
        Some(
            "Use Responses or Gemini event-native streaming when multimodal deltas must remain recoverable.".to_string(),
        ),
    )
}

// --- Anthropic to Unified ---

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicRequestPayload {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<AnthropicSystemPrompt>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicSystemPrompt {
    String(String),
    Blocks(Vec<AnthropicSystemBlock>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicSystemBlock {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicCacheControl {
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: Value, // Can be a string or an array of content blocks
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value, // JSON Schema
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

// --- Anthropic Response to Unified ---

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub role: String,
    pub content: Vec<AnthropicContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum AnthropicContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl From<AnthropicResponse> for UnifiedResponse {
    fn from(anthropic_res: AnthropicResponse) -> Self {
        let content: Vec<UnifiedContentPart> = anthropic_res
            .content
            .clone()
            .into_iter()
            .map(|block| match block {
                AnthropicContentBlock::Text { text } => UnifiedContentPart::Text { text },
                AnthropicContentBlock::Thinking { thinking, .. } => {
                    UnifiedContentPart::Reasoning { text: thinking }
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id,
                        name,
                        arguments: input,
                    })
                }
            })
            .collect();
        let items = anthropic_res
            .content
            .into_iter()
            .map(|block| match block {
                AnthropicContentBlock::Text { text } => UnifiedItem::Message(UnifiedMessageItem {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text { text }],
                    annotations: Vec::new(),
                }),
                AnthropicContentBlock::Thinking { thinking, .. } => {
                    UnifiedItem::Reasoning(UnifiedReasoningItem {
                        content: vec![UnifiedContentPart::Reasoning { text: thinking }],
                        annotations: Vec::new(),
                    })
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                        id,
                        name,
                        arguments: input,
                    })
                }
            })
            .collect();

        let message = UnifiedMessage {
            role: UnifiedRole::Assistant,
            content,
            ..Default::default()
        };

        let finish_reason = anthropic_res.stop_reason.map(|reason| {
            crate::service::transform::unified::map_anthropic_finish_reason_to_openai(&reason)
        });

        let choice = UnifiedChoice {
            index: 0,
            message,
            items,
            finish_reason,
            logprobs: None,
        };

        let usage = Some(UnifiedUsage {
            input_tokens: anthropic_res.usage.input_tokens,
            output_tokens: anthropic_res.usage.output_tokens,
            total_tokens: anthropic_res.usage.input_tokens + anthropic_res.usage.output_tokens,
            ..Default::default()
        });

        UnifiedResponse {
            id: anthropic_res.id,
            model: Some(anthropic_res.model),
            choices: vec![choice],
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: Some(UnifiedProviderResponseMetadata {
                anthropic: Some(UnifiedAnthropicResponseMetadata {
                    provider_type: Some(anthropic_res.type_),
                    role: Some(anthropic_res.role),
                    stop_sequence: anthropic_res.stop_sequence,
                }),
                ..Default::default()
            }),
            synthetic_metadata: None,
        }
    }
}

impl From<UnifiedResponse> for AnthropicResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let choice = unified_res
            .choices
            .into_iter()
            .next()
            .unwrap_or_else(|| UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "".to_string(),
                    }],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: None,
                logprobs: None,
            });

        let content: Vec<AnthropicContentBlock> = choice
            .content_items()
            .into_iter()
            .flat_map(|item| match item {
                UnifiedItem::Message(message) => message.content.into_iter().filter_map(|part| match part {
                    UnifiedContentPart::Text { text } => Some(AnthropicContentBlock::Text { text }),
                    UnifiedContentPart::Refusal { text } => {
                        Some(AnthropicContentBlock::Text { text })
                    }
                    UnifiedContentPart::Reasoning { text } => {
                        Some(AnthropicContentBlock::Text { text })
                    }
                    UnifiedContentPart::ImageUrl { .. }
                    | UnifiedContentPart::ImageData { .. }
                    | UnifiedContentPart::FileUrl { .. }
                    | UnifiedContentPart::FileData { .. }
                    | UnifiedContentPart::ExecutableCode { .. } => {
                        apply_transform_policy(
                            TransformProtocol::Unified,
                            TransformProtocol::Api(LlmApiType::Anthropic),
                            TransformValueKind::from(&part),
                            "Dropping unsupported response content from Anthropic conversion.",
                        );
                        None
                    }
                    UnifiedContentPart::ToolCall(call) => Some(AnthropicContentBlock::ToolUse {
                        id: call.id,
                        name: call.name,
                        input: call.arguments,
                    }),
                    UnifiedContentPart::ToolResult(_) => {
                        apply_transform_policy(
                            TransformProtocol::Unified,
                            TransformProtocol::Api(LlmApiType::Anthropic),
                            TransformValueKind::ToolResult,
                            "Dropping tool result from Anthropic assistant response conversion.",
                        );
                        None
                    }
                }).collect::<Vec<_>>(),
                UnifiedItem::Reasoning(item) => item.content.into_iter().filter_map(|part| match part {
                    UnifiedContentPart::Reasoning { text }
                    | UnifiedContentPart::Text { text }
                    | UnifiedContentPart::Refusal { text } => {
                        Some(AnthropicContentBlock::Thinking {
                            thinking: text,
                            signature: None,
                        })
                    }
                    other => {
                        apply_transform_policy(
                            TransformProtocol::Unified,
                            TransformProtocol::Api(LlmApiType::Anthropic),
                            TransformValueKind::from(&other),
                            "Dropping unsupported reasoning content from Anthropic conversion.",
                        );
                        None
                    }
                }).collect::<Vec<_>>(),
                UnifiedItem::FunctionCall(call) => vec![AnthropicContentBlock::ToolUse {
                    id: call.id,
                    name: call.name,
                    input: call.arguments,
                }],
                UnifiedItem::FunctionCallOutput(_) => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Api(LlmApiType::Anthropic),
                        TransformValueKind::ToolResult,
                        "Dropping tool result from Anthropic assistant response conversion.",
                    );
                    vec![]
                }
                UnifiedItem::FileReference(_) => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Api(LlmApiType::Anthropic),
                        TransformValueKind::FileUrl,
                        "Dropping file reference from Anthropic assistant response conversion.",
                    );
                    vec![]
                }
            })
            .collect();

        let stop_reason = choice.finish_reason.map(|reason| {
            crate::service::transform::unified::map_openai_finish_reason_to_anthropic(&reason)
        });

        let usage = unified_res.usage.map_or_else(
            || AnthropicUsage {
                input_tokens: 0,
                output_tokens: 0,
            },
            |u| AnthropicUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
            },
        );

        AnthropicResponse {
            id: unified_res.id,
            type_: "message".to_string(),
            role: "assistant".to_string(),
            content,
            model: unified_res.model.unwrap_or_default(),
            stop_reason,
            stop_sequence: None,
            usage,
        }
    }
}

// --- Anthropic Chunk Response ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicEvent {
    MessageStart {
        message: AnthropicStreamMessage,
    },
    ContentBlockStart {
        index: u32,
        content_block: AnthropicContentBlock,
    },
    ContentBlockDelta {
        index: u32,
        delta: AnthropicContentDelta,
    },
    ContentBlockStop {
        index: u32,
    },
    MessageDelta {
        delta: MessageDelta,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<AnthropicUsage>,
    },
    MessageStop,
    Error {
        error: Value,
    },
    #[serde(other)]
    Ping,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicStreamMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub role: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<AnthropicContentBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
    ThinkingDelta { thinking: String },
    SignatureDelta { signature: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<AnthropicUsage>,
}

fn parse_anthropic_tool_arguments(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(arguments).unwrap_or(Value::String(arguments.to_string()))
    }
}

fn anthropic_signature_blob(index: u32, signature: String) -> UnifiedStreamEvent {
    UnifiedStreamEvent::BlobDelta {
        index: Some(index),
        data: json!({
            "provider": "anthropic",
            "type": "signature_delta",
            "signature": signature,
        }),
    }
}

fn anthropic_start_block_state(
    session: &mut AnthropicSessionState,
    index: u32,
    kind: AnthropicActiveBlockKind,
) -> &mut AnthropicActiveBlockState {
    session
        .active_blocks
        .entry(index)
        .or_insert_with(|| AnthropicActiveBlockState::new(kind))
}

fn anthropic_block_stop_events(
    index: u32,
    state: Option<AnthropicActiveBlockState>,
) -> Vec<UnifiedStreamEvent> {
    match state {
        Some(AnthropicActiveBlockState {
            kind: AnthropicActiveBlockKind::Text,
            text,
            ..
        }) => vec![
            UnifiedStreamEvent::ContentBlockStop { index },
            UnifiedStreamEvent::ContentPartDone {
                item_index: Some(index),
                item_id: None,
                part_index: 0,
            },
            UnifiedStreamEvent::ItemDone {
                item_index: Some(index),
                item_id: None,
                item: UnifiedItem::Message(UnifiedMessageItem {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text { text }],
                    annotations: Vec::new(),
                }),
            },
        ],
        Some(AnthropicActiveBlockState {
            kind: AnthropicActiveBlockKind::ToolUse,
            text,
            tool_call_id,
            tool_name,
        }) => {
            let id = tool_call_id
                .unwrap_or_else(|| format!("toolu_{}", crate::utils::ID_GENERATOR.generate_id()));
            let name = tool_name.unwrap_or_else(|| "tool".to_string());
            vec![
                UnifiedStreamEvent::ToolCallStop {
                    index,
                    id: Some(id.clone()),
                },
                UnifiedStreamEvent::ContentBlockStop { index },
                UnifiedStreamEvent::ItemDone {
                    item_index: Some(index),
                    item_id: Some(id.clone()),
                    item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                        id,
                        name,
                        arguments: parse_anthropic_tool_arguments(&text),
                    }),
                },
            ]
        }
        Some(AnthropicActiveBlockState {
            kind: AnthropicActiveBlockKind::Thinking,
            text,
            ..
        }) => vec![
            UnifiedStreamEvent::ReasoningStop { index },
            UnifiedStreamEvent::ItemDone {
                item_index: Some(index),
                item_id: None,
                item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                    content: vec![UnifiedContentPart::Reasoning { text }],
                    annotations: Vec::new(),
                }),
            },
        ],
        None => vec![UnifiedStreamEvent::ContentBlockStop { index }],
    }
}

impl From<AnthropicEvent> for UnifiedChunkResponse {
    fn from(event: AnthropicEvent) -> Self {
        let mut choice = UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta::default(),
            finish_reason: None,
        };
        let (id, model) = match &event {
            AnthropicEvent::MessageStart { message } => (message.id.clone(), message.model.clone()),
            _ => (
                format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
                "anthropic-transformed-model".to_string(),
            ),
        };

        match event {
            AnthropicEvent::MessageStart { .. } => {
                choice.delta.role = Some(UnifiedRole::Assistant);
            }
            AnthropicEvent::ContentBlockDelta { index, delta } => match delta {
                AnthropicContentDelta::TextDelta { text } => {
                    choice
                        .delta
                        .content
                        .push(UnifiedContentPartDelta::TextDelta { index, text });
                }
                AnthropicContentDelta::InputJsonDelta { partial_json } => {
                    choice
                        .delta
                        .content
                        .push(UnifiedContentPartDelta::ToolCallDelta(
                            UnifiedToolCallDelta {
                                index,
                                id: None,
                                name: None,
                                arguments: Some(partial_json),
                            },
                        ));
                }
                AnthropicContentDelta::ThinkingDelta { thinking } => {
                    choice
                        .delta
                        .content
                        .push(UnifiedContentPartDelta::TextDelta {
                            index,
                            text: thinking,
                        });
                }
                AnthropicContentDelta::SignatureDelta { .. } => {}
            },
            AnthropicEvent::MessageDelta { delta, usage } => {
                if let Some(stop_reason) = &delta.stop_reason {
                    choice.finish_reason = Some(
                        crate::service::transform::unified::map_anthropic_finish_reason_to_openai(
                            stop_reason,
                        ),
                    );
                }
                let usage = usage.or(delta.usage).map(|usage| UnifiedUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.input_tokens + usage.output_tokens,
                    ..Default::default()
                });

                return UnifiedChunkResponse {
                    id,
                    model: Some(model),
                    choices: vec![choice],
                    usage,
                    created: Some(Utc::now().timestamp()),
                    object: Some("chat.completion.chunk".to_string()),
                    provider_session_metadata: None,
                    synthetic_metadata: None,
                };
            }
            // Other events don't map to a chunk with content, so we create an empty one.
            _ => {}
        }

        UnifiedChunkResponse {
            id,
            model: Some(model),
            choices: vec![choice],
            usage: None, // Anthropic provides usage at the end, not per chunk
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata: None,
            synthetic_metadata: None,
        }
    }
}

fn anthropic_event_to_unified_stream_events_inner(
    event: AnthropicEvent,
    session: &mut AnthropicSessionState,
) -> Vec<UnifiedStreamEvent> {
    match event {
        AnthropicEvent::MessageStart { message } => {
            let mut events = vec![UnifiedStreamEvent::MessageStart {
                id: Some(message.id),
                model: Some(message.model),
                role: UnifiedRole::Assistant,
            }];

            if let Some(content_blocks) = message.content {
                for (index, block) in content_blocks.into_iter().enumerate() {
                    match block {
                        AnthropicContentBlock::Text { text } => {
                            let index = index as u32;
                            let state = anthropic_start_block_state(
                                session,
                                index,
                                AnthropicActiveBlockKind::Text,
                            );
                            state.text = text.clone();
                            events.push(UnifiedStreamEvent::ItemAdded {
                                item_index: Some(index),
                                item_id: None,
                                item: UnifiedItem::Message(UnifiedMessageItem {
                                    role: UnifiedRole::Assistant,
                                    content: Vec::new(),
                                    annotations: Vec::new(),
                                }),
                            });
                            events.push(UnifiedStreamEvent::ContentPartAdded {
                                item_index: Some(index),
                                item_id: None,
                                part_index: 0,
                                part: Some(UnifiedContentPart::Text { text: text.clone() }),
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStart {
                                index,
                                kind: UnifiedBlockKind::Text,
                            });
                            if !text.is_empty() {
                                events.push(UnifiedStreamEvent::ContentBlockDelta {
                                    index,
                                    item_index: None,
                                    item_id: None,
                                    part_index: None,
                                    text: text.clone(),
                                });
                            }
                            events.push(UnifiedStreamEvent::ContentBlockStop { index });
                            events.push(UnifiedStreamEvent::ContentPartDone {
                                item_index: Some(index),
                                item_id: None,
                                part_index: 0,
                            });
                            events.push(UnifiedStreamEvent::ItemDone {
                                item_index: Some(index),
                                item_id: None,
                                item: UnifiedItem::Message(UnifiedMessageItem {
                                    role: UnifiedRole::Assistant,
                                    content: vec![UnifiedContentPart::Text { text }],
                                    annotations: Vec::new(),
                                }),
                            });
                            session.active_blocks.remove(&index);
                        }
                        AnthropicContentBlock::Thinking {
                            thinking,
                            signature,
                        } => {
                            let index = index as u32;
                            let state = anthropic_start_block_state(
                                session,
                                index,
                                AnthropicActiveBlockKind::Thinking,
                            );
                            state.text = thinking.clone();
                            events.push(UnifiedStreamEvent::ItemAdded {
                                item_index: Some(index),
                                item_id: None,
                                item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                                    content: Vec::new(),
                                    annotations: Vec::new(),
                                }),
                            });
                            events.push(UnifiedStreamEvent::ReasoningStart { index });
                            if !thinking.is_empty() {
                                events.push(UnifiedStreamEvent::ReasoningDelta {
                                    index,
                                    item_index: None,
                                    item_id: None,
                                    part_index: None,
                                    text: thinking,
                                });
                            }
                            if let Some(signature) = signature {
                                events.push(anthropic_signature_blob(index, signature));
                            }
                            events.push(UnifiedStreamEvent::ReasoningStop { index });
                            events.push(UnifiedStreamEvent::ItemDone {
                                item_index: Some(index),
                                item_id: None,
                                item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                                    content: vec![UnifiedContentPart::Reasoning {
                                        text: state.text.clone(),
                                    }],
                                    annotations: Vec::new(),
                                }),
                            });
                            session.active_blocks.remove(&index);
                        }
                        AnthropicContentBlock::ToolUse { id, name, input } => {
                            let index = index as u32;
                            let state = anthropic_start_block_state(
                                session,
                                index,
                                AnthropicActiveBlockKind::ToolUse,
                            );
                            state.tool_call_id = Some(id.clone());
                            state.tool_name = Some(name.clone());
                            state.text = serde_json::to_string(&input).unwrap_or_default();
                            events.push(UnifiedStreamEvent::ItemAdded {
                                item_index: Some(index),
                                item_id: Some(id.clone()),
                                item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments: input.clone(),
                                }),
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStart {
                                index,
                                kind: UnifiedBlockKind::ToolCall,
                            });
                            events.push(UnifiedStreamEvent::ToolCallStart {
                                index,
                                id: id.clone(),
                                name: name.clone(),
                            });
                            let arguments = serde_json::to_string(&input).unwrap_or_default();
                            if !arguments.is_empty() {
                                events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                                    index,
                                    item_index: None,
                                    item_id: None,
                                    id: Some(id.clone()),
                                    name: Some(name.clone()),
                                    arguments,
                                });
                            }
                            events.push(UnifiedStreamEvent::ToolCallStop {
                                index,
                                id: Some(id.clone()),
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStop { index });
                            events.push(UnifiedStreamEvent::ItemDone {
                                item_index: Some(index),
                                item_id: Some(id.clone()),
                                item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                                    id,
                                    name,
                                    arguments: input,
                                }),
                            });
                            session.active_blocks.remove(&index);
                        }
                    }
                }
            }

            events
        }
        AnthropicEvent::ContentBlockStart {
            index,
            content_block,
        } => match content_block {
            AnthropicContentBlock::Text { text } => {
                let state =
                    anthropic_start_block_state(session, index, AnthropicActiveBlockKind::Text);
                state.text = text.clone();
                let mut events = vec![
                    UnifiedStreamEvent::ItemAdded {
                        item_index: Some(index),
                        item_id: None,
                        item: UnifiedItem::Message(UnifiedMessageItem {
                            role: UnifiedRole::Assistant,
                            content: Vec::new(),
                            annotations: Vec::new(),
                        }),
                    },
                    UnifiedStreamEvent::ContentPartAdded {
                        item_index: Some(index),
                        item_id: None,
                        part_index: 0,
                        part: Some(UnifiedContentPart::Text { text: text.clone() }),
                    },
                    UnifiedStreamEvent::ContentBlockStart {
                        index,
                        kind: UnifiedBlockKind::Text,
                    },
                ];
                if !text.is_empty() {
                    events.push(UnifiedStreamEvent::ContentBlockDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text,
                    });
                }
                events
            }
            AnthropicContentBlock::Thinking {
                thinking,
                signature,
            } => {
                let state =
                    anthropic_start_block_state(session, index, AnthropicActiveBlockKind::Thinking);
                state.text = thinking.clone();
                let mut events = vec![
                    UnifiedStreamEvent::ItemAdded {
                        item_index: Some(index),
                        item_id: None,
                        item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                            content: Vec::new(),
                            annotations: Vec::new(),
                        }),
                    },
                    UnifiedStreamEvent::ReasoningStart { index },
                ];
                if !thinking.is_empty() {
                    events.push(UnifiedStreamEvent::ReasoningDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text: thinking,
                    });
                }
                if let Some(signature) = signature {
                    events.push(anthropic_signature_blob(index, signature));
                }
                events
            }
            AnthropicContentBlock::ToolUse { id, name, input } => {
                let state =
                    anthropic_start_block_state(session, index, AnthropicActiveBlockKind::ToolUse);
                state.tool_call_id = Some(id.clone());
                state.tool_name = Some(name.clone());
                state.text = serde_json::to_string(&input).unwrap_or_default();
                vec![
                    UnifiedStreamEvent::ItemAdded {
                        item_index: Some(index),
                        item_id: Some(id.clone()),
                        item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: input.clone(),
                        }),
                    },
                    UnifiedStreamEvent::ContentBlockStart {
                        index,
                        kind: UnifiedBlockKind::ToolCall,
                    },
                    UnifiedStreamEvent::ToolCallStart { index, id, name },
                    UnifiedStreamEvent::ToolCallArgumentsDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        id: state.tool_call_id.clone(),
                        name: state.tool_name.clone(),
                        arguments: serde_json::to_string(&input).unwrap_or_default(),
                    },
                ]
            }
        },
        AnthropicEvent::ContentBlockDelta { index, delta } => match delta {
            AnthropicContentDelta::TextDelta { text } => {
                if let Some(block) = session.active_blocks.get_mut(&index) {
                    block.text.push_str(&text);
                }
                vec![UnifiedStreamEvent::ContentBlockDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text,
                }]
            }
            AnthropicContentDelta::InputJsonDelta { partial_json } => {
                if let Some(block) = session.active_blocks.get_mut(&index) {
                    block.text.push_str(&partial_json);
                }
                vec![UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    id: session
                        .active_blocks
                        .get(&index)
                        .and_then(|block| block.tool_call_id.clone()),
                    name: session
                        .active_blocks
                        .get(&index)
                        .and_then(|block| block.tool_name.clone()),
                    arguments: partial_json,
                }]
            }
            AnthropicContentDelta::ThinkingDelta { thinking } => {
                if let Some(block) = session.active_blocks.get_mut(&index) {
                    block.text.push_str(&thinking);
                }
                vec![UnifiedStreamEvent::ReasoningDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: thinking,
                }]
            }
            AnthropicContentDelta::SignatureDelta { signature } => {
                vec![anthropic_signature_blob(index, signature)]
            }
        },
        AnthropicEvent::ContentBlockStop { index } => {
            anthropic_block_stop_events(index, session.active_blocks.remove(&index))
        }
        AnthropicEvent::MessageDelta { delta, usage } => {
            let mut events = Vec::new();
            if delta.stop_reason.is_some() {
                events.push(UnifiedStreamEvent::MessageDelta {
                    finish_reason: delta.stop_reason.as_deref().map(
                        crate::service::transform::unified::map_anthropic_finish_reason_to_openai,
                    ),
                });
            }
            if let Some(usage) = usage.or(delta.usage) {
                events.push(UnifiedStreamEvent::Usage {
                    usage: UnifiedUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        total_tokens: usage.input_tokens + usage.output_tokens,
                        ..Default::default()
                    },
                });
            }
            events
        }
        AnthropicEvent::MessageStop => vec![UnifiedStreamEvent::MessageStop],
        AnthropicEvent::Error { error } => vec![UnifiedStreamEvent::Error { error }],
        AnthropicEvent::Ping => Vec::new(),
    }
}

pub fn anthropic_event_to_unified_stream_events(event: AnthropicEvent) -> Vec<UnifiedStreamEvent> {
    let mut session = AnthropicSessionState::default();
    anthropic_event_to_unified_stream_events_inner(event, &mut session)
}

pub fn anthropic_event_to_unified_stream_events_with_state(
    event: AnthropicEvent,
    session: &mut AnthropicSessionState,
) -> Vec<UnifiedStreamEvent> {
    anthropic_event_to_unified_stream_events_inner(event, session)
}

fn anthropic_start_block_event(index: u32, content_block: AnthropicContentBlock) -> SseEvent {
    let event = json!({
        "type": "content_block_start",
        "index": index,
        "content_block": content_block,
    });
    SseEvent {
        event: Some("content_block_start".to_string()),
        data: serde_json::to_string(&event).unwrap(),
        ..Default::default()
    }
}

fn anthropic_block_delta_event(index: u32, delta: AnthropicContentDelta) -> SseEvent {
    let event = json!({
        "type": "content_block_delta",
        "index": index,
        "delta": delta,
    });
    SseEvent {
        event: Some("content_block_delta".to_string()),
        data: serde_json::to_string(&event).unwrap(),
        ..Default::default()
    }
}

fn anthropic_block_stop_event(index: u32) -> SseEvent {
    let event = json!({
        "type": "content_block_stop",
        "index": index,
    });
    SseEvent {
        event: Some("content_block_stop".to_string()),
        data: serde_json::to_string(&event).unwrap(),
        ..Default::default()
    }
}

fn close_active_anthropic_blocks(transformer: &mut StreamTransformer, events: &mut Vec<SseEvent>) {
    let mut active_indices = transformer
        .session
        .anthropic
        .active_blocks
        .keys()
        .copied()
        .collect::<Vec<_>>();
    active_indices.sort_unstable();

    for index in active_indices {
        if transformer
            .session
            .anthropic
            .active_blocks
            .remove(&index)
            .is_some()
        {
            events.push(anthropic_block_stop_event(index));
        }
    }
}

pub(super) fn transform_unified_stream_events_to_anthropic_events(
    stream_events: Vec<UnifiedStreamEvent>,
    transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut events = Vec::new();

    for stream_event in stream_events {
        match stream_event {
            UnifiedStreamEvent::MessageStart { id, model, .. } => {
                if !transformer.session.anthropic.message_started {
                    transformer.session.anthropic.message_started = true;
                    let event = json!({
                        "type": "message_start",
                        "message": {
                            "id": id.unwrap_or_else(|| transformer.get_or_generate_stream_id()),
                            "type": "message",
                            "role": "assistant",
                            "content": [],
                            "model": model.unwrap_or_else(|| transformer.session.stream_model.clone().unwrap_or_default()),
                            "usage": AnthropicUsage {
                                input_tokens: transformer.session.usage_cache.as_ref().map(|u| u.input_tokens as u32).unwrap_or(0),
                                output_tokens: transformer.session.usage_cache.as_ref().map(|u| u.output_tokens as u32).unwrap_or(0),
                            }
                        }
                    });
                    events.push(SseEvent {
                        event: Some("message_start".to_string()),
                        data: serde_json::to_string(&event).unwrap(),
                        ..Default::default()
                    });
                }
            }
            UnifiedStreamEvent::ContentBlockStart { index, kind } => match kind {
                UnifiedBlockKind::Text => {
                    transformer.session.anthropic.active_blocks.insert(
                        index,
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Text),
                    );
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::Text {
                            text: String::new(),
                        },
                    ));
                }
                UnifiedBlockKind::ToolCall | UnifiedBlockKind::Blob => {}
                UnifiedBlockKind::Reasoning => {
                    transformer.session.anthropic.active_blocks.insert(
                        index,
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Thinking),
                    );
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::Thinking {
                            thinking: String::new(),
                            signature: Some(String::new()),
                        },
                    ));
                }
            },
            UnifiedStreamEvent::ContentBlockDelta { index, text, .. } => {
                let block_exists = transformer
                    .session
                    .anthropic
                    .active_blocks
                    .contains_key(&index);
                transformer
                    .session
                    .anthropic
                    .active_blocks
                    .entry(index)
                    .or_insert_with(|| {
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Text)
                    })
                    .text
                    .push_str(&text);
                if !block_exists {
                    // This path only synthesizes a start when the upstream stream omitted it.
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::Text {
                            text: String::new(),
                        },
                    ));
                }
                events.push(anthropic_block_delta_event(
                    index,
                    AnthropicContentDelta::TextDelta { text },
                ));
            }
            UnifiedStreamEvent::ContentBlockStop { index } => {
                if matches!(
                    transformer
                        .session
                        .anthropic
                        .active_blocks
                        .get(&index)
                        .map(|block| block.kind),
                    Some(AnthropicActiveBlockKind::Text)
                ) {
                    transformer.session.anthropic.active_blocks.remove(&index);
                    events.push(anthropic_block_stop_event(index));
                }
            }
            UnifiedStreamEvent::ToolCallStart { index, id, name } => {
                let block = transformer
                    .session
                    .anthropic
                    .active_blocks
                    .entry(index)
                    .or_insert_with(|| {
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::ToolUse)
                    });
                block.kind = AnthropicActiveBlockKind::ToolUse;
                block.tool_call_id = Some(id.clone());
                block.tool_name = Some(name.clone());
                events.push(anthropic_start_block_event(
                    index,
                    AnthropicContentBlock::ToolUse {
                        id,
                        name,
                        input: Value::Object(Default::default()),
                    },
                ));
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                id,
                name,
                arguments,
                ..
            } => {
                let synthesize_start = !transformer
                    .session
                    .anthropic
                    .active_blocks
                    .contains_key(&index);
                let block = transformer
                    .session
                    .anthropic
                    .active_blocks
                    .entry(index)
                    .or_insert_with(|| {
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::ToolUse)
                    });
                if block.tool_call_id.is_none() {
                    block.tool_call_id = id.clone();
                }
                if block.tool_name.is_none() {
                    block.tool_name = name.clone();
                }
                if synthesize_start && block.tool_call_id.is_some() && block.tool_name.is_some() {
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::ToolUse {
                            id: block.tool_call_id.clone().unwrap_or_else(|| {
                                format!("toolu_{}", crate::utils::ID_GENERATOR.generate_id())
                            }),
                            name: block
                                .tool_name
                                .clone()
                                .unwrap_or_else(|| "tool".to_string()),
                            input: Value::Object(Default::default()),
                        },
                    ));
                }
                block.text.push_str(&arguments);
                events.push(anthropic_block_delta_event(
                    index,
                    AnthropicContentDelta::InputJsonDelta {
                        partial_json: arguments,
                    },
                ));
            }
            UnifiedStreamEvent::ToolCallStop { index, .. } => {
                if matches!(
                    transformer
                        .session
                        .anthropic
                        .active_blocks
                        .get(&index)
                        .map(|block| block.kind),
                    Some(AnthropicActiveBlockKind::ToolUse)
                ) {
                    transformer.session.anthropic.active_blocks.remove(&index);
                    events.push(anthropic_block_stop_event(index));
                }
            }
            UnifiedStreamEvent::ReasoningStart { index } => {
                transformer.session.anthropic.active_blocks.insert(
                    index,
                    AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Thinking),
                );
                events.push(anthropic_start_block_event(
                    index,
                    AnthropicContentBlock::Thinking {
                        thinking: String::new(),
                        signature: Some(String::new()),
                    },
                ));
            }
            UnifiedStreamEvent::ReasoningDelta { index, text, .. } => {
                let block_exists = transformer
                    .session
                    .anthropic
                    .active_blocks
                    .contains_key(&index);
                transformer
                    .session
                    .anthropic
                    .active_blocks
                    .entry(index)
                    .or_insert_with(|| {
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Thinking)
                    })
                    .text
                    .push_str(&text);
                if !block_exists {
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::Thinking {
                            thinking: String::new(),
                            signature: Some(String::new()),
                        },
                    ));
                }
                events.push(anthropic_block_delta_event(
                    index,
                    AnthropicContentDelta::ThinkingDelta { thinking: text },
                ));
            }
            UnifiedStreamEvent::ReasoningStop { index } => {
                if matches!(
                    transformer
                        .session
                        .anthropic
                        .active_blocks
                        .get(&index)
                        .map(|block| block.kind),
                    Some(AnthropicActiveBlockKind::Thinking)
                ) {
                    transformer.session.anthropic.active_blocks.remove(&index);
                    events.push(anthropic_block_stop_event(index));
                }
            }
            UnifiedStreamEvent::BlobDelta {
                index: Some(index),
                data,
            } if data.get("provider").and_then(Value::as_str) == Some("anthropic")
                && data.get("type").and_then(Value::as_str) == Some("signature_delta") =>
            {
                if let Some(signature) = data.get("signature").and_then(Value::as_str) {
                    events.push(anthropic_block_delta_event(
                        index,
                        AnthropicContentDelta::SignatureDelta {
                            signature: signature.to_string(),
                        },
                    ));
                }
            }
            UnifiedStreamEvent::Usage { usage } => {
                transformer.session.usage_normalization_cache = Some((&usage).into());
                transformer.session.usage_cache = Some(usage.into());
            }
            UnifiedStreamEvent::MessageDelta { finish_reason } => {
                if let Some(finish_reason) = finish_reason {
                    close_active_anthropic_blocks(transformer, &mut events);
                    let event = json!({
                        "type": "message_delta",
                        "delta": {
                            "stop_reason": crate::service::transform::unified::map_openai_finish_reason_to_anthropic(&finish_reason),
                            "stop_sequence": null,
                        },
                        "usage": transformer.session.usage_cache.as_ref().map(|usage| AnthropicUsage {
                            input_tokens: usage.input_tokens as u32,
                            output_tokens: usage.output_tokens as u32,
                        }).unwrap_or(AnthropicUsage {
                            input_tokens: 0,
                            output_tokens: 0,
                        }),
                    });
                    events.push(SseEvent {
                        event: Some("message_delta".to_string()),
                        data: serde_json::to_string(&event).unwrap(),
                        ..Default::default()
                    });
                }
            }
            UnifiedStreamEvent::MessageStop => {
                close_active_anthropic_blocks(transformer, &mut events);
                events.push(SseEvent {
                    event: Some("message_stop".to_string()),
                    data: "{\"type\":\"message_stop\"}".to_string(),
                    ..Default::default()
                });
            }
            UnifiedStreamEvent::ReasoningSummaryPartAdded { item_index, .. }
            | UnifiedStreamEvent::ReasoningSummaryPartDone { item_index, .. } => {
                events.push(build_anthropic_stream_diagnostic(
                    transformer,
                    TransformValueKind::ReasoningDelta,
                    format!(
                        "Anthropic SSE does not expose reasoning summary part lifecycle natively; item_index={item_index:?} was downgraded to a structured transform diagnostic."
                    ),
                ));
            }
            UnifiedStreamEvent::BlobDelta { index, data } => {
                events.push(build_anthropic_stream_diagnostic(
                    transformer,
                    TransformValueKind::BlobDelta,
                    format!(
                        "Anthropic SSE only preserves provider-native signature deltas; index={index:?}, json_type={} was downgraded to a structured transform diagnostic.",
                        data.get("type").and_then(Value::as_str).unwrap_or("unknown"),
                    ),
                ));
            }
            UnifiedStreamEvent::ItemAdded { .. }
            | UnifiedStreamEvent::ItemDone { .. }
            | UnifiedStreamEvent::ContentPartAdded { .. }
            | UnifiedStreamEvent::ContentPartDone { .. } => {}
            UnifiedStreamEvent::Error { .. } => {}
        }
    }

    (!events.is_empty()).then_some(events)
}

pub fn transform_unified_chunk_to_anthropic_events(
    unified_chunk: UnifiedChunkResponse,
    state: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut stream_events = Vec::new();
    let mut diagnostics = Vec::new();

    if let Some(usage) = unified_chunk.usage.clone() {
        state.session.usage_normalization_cache = Some((&usage).into());
        state.session.usage_cache = Some(usage.clone().into());
    }

    if let Some(choice) = unified_chunk.choices.first() {
        if let Some(role) = choice.delta.role.clone() {
            stream_events.push(UnifiedStreamEvent::MessageStart {
                id: Some(unified_chunk.id),
                model: unified_chunk.model,
                role,
            });
        }

        for part in &choice.delta.content {
            match part {
                UnifiedContentPartDelta::TextDelta { index, text } => {
                    stream_events.push(UnifiedStreamEvent::ContentBlockDelta {
                        index: *index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text: text.clone(),
                    });
                }
                UnifiedContentPartDelta::ToolCallDelta(tool_delta) => {
                    if let (Some(id), Some(name)) = (tool_delta.id.clone(), tool_delta.name.clone())
                    {
                        stream_events.push(UnifiedStreamEvent::ToolCallStart {
                            index: tool_delta.index,
                            id,
                            name,
                        });
                    }
                    if let Some(arguments) = tool_delta.arguments.clone() {
                        stream_events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                            index: tool_delta.index,
                            item_index: None,
                            item_id: None,
                            id: tool_delta.id.clone(),
                            name: tool_delta.name.clone(),
                            arguments,
                        });
                    }
                }
                UnifiedContentPartDelta::ImageDelta { index, url, data } => {
                    diagnostics.push(build_anthropic_stream_diagnostic(
                        state,
                        TransformValueKind::ImageDelta,
                        format!(
                            "Anthropic SSE content blocks do not expose native image deltas; index={index}, has_url={}, has_data={} was downgraded to a structured transform diagnostic.",
                            url.as_ref().is_some_and(|value| !value.is_empty()),
                            data.as_ref().is_some_and(|value| !value.is_empty())
                        ),
                    ));
                }
            }
        }

        if let Some(finish_reason) = choice.finish_reason.clone() {
            for (index, block) in state.session.anthropic.active_blocks.clone() {
                match block.kind {
                    AnthropicActiveBlockKind::Text => {
                        stream_events.push(UnifiedStreamEvent::ContentBlockStop { index });
                    }
                    AnthropicActiveBlockKind::ToolUse => {
                        stream_events.push(UnifiedStreamEvent::ToolCallStop {
                            index,
                            id: block.tool_call_id,
                        });
                    }
                    AnthropicActiveBlockKind::Thinking => {
                        stream_events.push(UnifiedStreamEvent::ReasoningStop { index });
                    }
                }
            }
            stream_events.push(UnifiedStreamEvent::MessageDelta {
                finish_reason: Some(finish_reason),
            });
            stream_events.push(UnifiedStreamEvent::MessageStop);
        }
    }

    if !stream_events.iter().any(|event| {
        matches!(
            event,
            UnifiedStreamEvent::MessageStart { .. } | UnifiedStreamEvent::MessageDelta { .. }
        )
    }) {
        if let Some(usage) = unified_chunk.usage {
            stream_events.push(UnifiedStreamEvent::Usage { usage });
        }
    }

    let mut encoded = transform_unified_stream_events_to_anthropic_events(stream_events, state)
        .unwrap_or_default();
    encoded.extend(diagnostics);

    if encoded.is_empty() {
        None
    } else {
        Some(encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::enum_def::LlmApiType;
    use crate::service::transform::StreamTransformer;
    use serde_json::json;

    #[test]
    fn test_anthropic_request_to_unified() {
        let anthropic_request = AnthropicRequestPayload {
            model: "claude-3-opus-20240229".to_string(),
            system: Some(AnthropicSystemPrompt::String(
                "You are a helpful assistant.".to_string(),
            )),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hello, world!"),
            }],
            max_tokens: 100,
            temperature: Some(0.7),
            top_p: None,
            stop_sequences: None,
            stream: Some(true),
            tools: None,
            metadata: None,
            top_k: None,
        };

        let unified_request: UnifiedRequest = anthropic_request.into();

        assert_eq!(
            unified_request.model,
            Some("claude-3-opus-20240229".to_string())
        );
        assert_eq!(unified_request.messages.len(), 2);
        assert_eq!(unified_request.messages[0].role, UnifiedRole::System);
        assert_eq!(unified_request.messages[0].content.len(), 1);
        assert_eq!(
            unified_request.messages[0].content[0],
            UnifiedContentPart::Text {
                text: "You are a helpful assistant.".to_string()
            }
        );
        assert_eq!(unified_request.messages[1].role, UnifiedRole::User);
        assert_eq!(unified_request.messages[1].content.len(), 1);
        assert_eq!(
            unified_request.messages[1].content[0],
            UnifiedContentPart::Text {
                text: "Hello, world!".to_string()
            }
        );
        assert_eq!(unified_request.max_tokens, Some(100));
        assert_eq!(unified_request.temperature, Some(0.7));
        assert_eq!(unified_request.stream, true);
        assert_eq!(
            unified_request
                .anthropic_extension()
                .and_then(|extension| extension.metadata.clone()),
            None
        );
        assert_eq!(unified_request.top_k(), None);
    }

    #[test]
    fn test_unified_request_to_anthropic() {
        let unified_request = UnifiedRequest {
            model: Some("claude-3-opus-20240229".to_string()),
            messages: vec![
                UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![UnifiedContentPart::Text {
                        text: "You are a helpful assistant.".to_string(),
                    }],
                },
                UnifiedMessage {
                    role: UnifiedRole::User,
                    content: vec![UnifiedContentPart::Text {
                        text: "Hello, world!".to_string(),
                    }],
                },
            ],
            max_tokens: Some(100),
            temperature: Some(0.7),
            stream: true,
            ..Default::default()
        };

        let anthropic_request: AnthropicRequestPayload = unified_request.into();

        assert_eq!(anthropic_request.model, "claude-3-opus-20240229");
        match anthropic_request.system {
            Some(AnthropicSystemPrompt::String(s)) => {
                assert_eq!(s, "You are a helpful assistant.");
            }
            _ => panic!("Expected string system prompt"),
        }
        assert_eq!(anthropic_request.messages.len(), 1);
        assert_eq!(anthropic_request.messages[0].role, "user");
        assert_eq!(
            anthropic_request.messages[0].content,
            json!("Hello, world!")
        );
        assert_eq!(anthropic_request.max_tokens, 100);
        assert_eq!(anthropic_request.temperature, Some(0.7));
        assert_eq!(anthropic_request.stream, Some(true));
    }

    #[test]
    fn test_anthropic_request_round_trip_preserves_metadata_and_top_k() {
        let anthropic_request = AnthropicRequestPayload {
            model: "claude-3-opus-20240229".to_string(),
            system: Some(AnthropicSystemPrompt::String(
                "You are a helpful assistant.".to_string(),
            )),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hello, world!"),
            }],
            max_tokens: 100,
            temperature: Some(0.7),
            top_p: Some(0.9),
            stop_sequences: Some(vec!["done".to_string()]),
            stream: Some(true),
            tools: None,
            metadata: Some(json!({
                "trace_id": "trace_123",
                "user_tier": "pro"
            })),
            top_k: Some(32),
        };

        let unified_request: UnifiedRequest = anthropic_request.into();
        assert_eq!(
            unified_request
                .anthropic_extension()
                .and_then(|extension| extension.metadata.clone()),
            Some(json!({
                "trace_id": "trace_123",
                "user_tier": "pro"
            }))
        );
        assert_eq!(unified_request.top_k(), Some(32));

        let round_tripped_request: AnthropicRequestPayload = unified_request.into();

        assert_eq!(
            round_tripped_request.metadata,
            Some(json!({
                "trace_id": "trace_123",
                "user_tier": "pro"
            }))
        );
        assert_eq!(round_tripped_request.top_k, Some(32));
        assert_eq!(round_tripped_request.top_p, Some(0.9));
        assert_eq!(
            round_tripped_request.stop_sequences,
            Some(vec!["done".to_string()])
        );
    }

    #[test]
    fn test_unified_request_to_anthropic_preserves_reasoning_as_text() {
        let unified_request = UnifiedRequest {
            model: Some("claude-3-opus-20240229".to_string()),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::Text {
                        text: "Question".to_string(),
                    },
                    UnifiedContentPart::Reasoning {
                        text: "chain of thought".to_string(),
                    },
                ],
            }],
            max_tokens: Some(100),
            ..Default::default()
        };

        let anthropic_request: AnthropicRequestPayload = unified_request.into();
        assert_eq!(
            anthropic_request.messages[0].content,
            json!([
                {"type": "text", "text": "Question"},
                {"type": "text", "text": "chain of thought"}
            ])
        );
    }

    #[test]
    fn test_unified_request_to_anthropic_preserves_image_file_and_code() {
        let unified_request = UnifiedRequest {
            model: Some("claude-3-opus-20240229".to_string()),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::ImageData {
                        mime_type: "image/png".to_string(),
                        data: "ZmFrZQ==".to_string(),
                    },
                    UnifiedContentPart::ImageUrl {
                        url: "https://example.com/chart.png".to_string(),
                        detail: Some("high".to_string()),
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
            }],
            max_tokens: Some(100),
            ..Default::default()
        };

        let anthropic_request: AnthropicRequestPayload = unified_request.into();
        assert_eq!(
            anthropic_request.messages[0].content,
            json!([
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "ZmFrZQ=="
                    }
                },
                {
                    "type": "text",
                    "text": "image_url: https://example.com/chart.png\ndetail: high"
                },
                {
                    "type": "text",
                    "text": "file_url: https://files.example.com/report.pdf\nmime_type: application/pdf"
                },
                {
                    "type": "text",
                    "text": "```python\nprint(1)\n```"
                }
            ])
        );
    }

    #[test]
    fn test_anthropic_response_to_unified() {
        let anthropic_response = AnthropicResponse {
            id: "msg_123".to_string(),
            type_: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text {
                text: "Hello from Anthropic!".to_string(),
            }],
            model: "claude-3-opus-20240229".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 20,
            },
        };

        let unified_response: UnifiedResponse = anthropic_response.into();

        assert_eq!(unified_response.id, "msg_123");
        assert_eq!(
            unified_response.model,
            Some("claude-3-opus-20240229".to_string())
        );
        assert_eq!(unified_response.choices.len(), 1);
        let choice = &unified_response.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(choice.message.content.len(), 1);
        assert_eq!(
            choice.message.content[0],
            UnifiedContentPart::Text {
                text: "Hello from Anthropic!".to_string()
            }
        );
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
        let usage = unified_response.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_unified_response_to_anthropic() {
        let unified_response = UnifiedResponse {
            id: "msg_123".to_string(),
            model: Some("claude-3-opus-20240229".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "Hello from Anthropic!".to_string(),
                    }],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
                ..Default::default()
            }),
            created: Some(12345),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let anthropic_response: AnthropicResponse = unified_response.into();

        assert_eq!(anthropic_response.id, "msg_123");
        assert_eq!(anthropic_response.model, "claude-3-opus-20240229");
        assert_eq!(anthropic_response.content.len(), 1);
        match &anthropic_response.content[0] {
            AnthropicContentBlock::Text { text } => assert_eq!(text, "Hello from Anthropic!"),
            _ => panic!("Incorrect content block type"),
        }
        assert_eq!(anthropic_response.stop_reason, Some("end_turn".to_string()));
        assert_eq!(anthropic_response.usage.input_tokens, 10);
        assert_eq!(anthropic_response.usage.output_tokens, 20);
    }

    #[test]
    fn test_anthropic_event_to_unified_chunk() {
        // MessageStart event
        let event_start = AnthropicEvent::MessageStart {
            message: AnthropicStreamMessage {
                id: "msg_123".to_string(),
                type_: "message".to_string(),
                role: "assistant".to_string(),
                model: "claude-3".to_string(),
                content: None,
                stop_reason: None,
                stop_sequence: None,
                usage: None,
            },
        };
        let unified_chunk_start: UnifiedChunkResponse = event_start.into();
        assert_eq!(unified_chunk_start.id, "msg_123");
        assert_eq!(unified_chunk_start.model, Some("claude-3".to_string()));
        assert_eq!(
            unified_chunk_start.choices[0].delta.role,
            Some(UnifiedRole::Assistant)
        );
        assert!(unified_chunk_start.choices[0].delta.content.is_empty());

        // ContentBlockDelta event
        let event_delta = AnthropicEvent::ContentBlockDelta {
            index: 0,
            delta: AnthropicContentDelta::TextDelta {
                text: "Hello".to_string(),
            },
        };
        let unified_chunk_delta: UnifiedChunkResponse = event_delta.into();
        assert!(unified_chunk_delta.id.starts_with("chatcmpl-"));
        assert_eq!(unified_chunk_delta.choices[0].delta.content.len(), 1);
        assert_eq!(
            unified_chunk_delta.choices[0].delta.content[0],
            UnifiedContentPartDelta::TextDelta {
                index: 0,
                text: "Hello".to_string()
            }
        );
        assert!(unified_chunk_delta.choices[0].delta.role.is_none());

        // MessageDelta event (finish reason)
        let event_stop = AnthropicEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some("end_turn".to_string()),
                stop_sequence: None,
                usage: None,
            },
            usage: Some(AnthropicUsage {
                input_tokens: 0,
                output_tokens: 10,
            }),
        };
        let unified_chunk_stop: UnifiedChunkResponse = event_stop.into();
        assert_eq!(
            unified_chunk_stop.choices[0].finish_reason,
            Some("stop".to_string())
        );
        assert!(unified_chunk_stop.choices[0].delta.content.is_empty());
    }

    #[test]
    fn test_anthropic_event_to_unified_stream_events_preserves_tool_use_lifecycle() {
        let events = anthropic_event_to_unified_stream_events(AnthropicEvent::ContentBlockStart {
            index: 2,
            content_block: AnthropicContentBlock::ToolUse {
                id: "toolu_123".to_string(),
                name: "lookup_weather".to_string(),
                input: json!({"city": "Boston"}),
            },
        });

        assert_eq!(
            events,
            vec![
                UnifiedStreamEvent::ItemAdded {
                    item_index: Some(2),
                    item_id: Some("toolu_123".to_string()),
                    item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                        id: "toolu_123".to_string(),
                        name: "lookup_weather".to_string(),
                        arguments: json!({"city": "Boston"}),
                    }),
                },
                UnifiedStreamEvent::ContentBlockStart {
                    index: 2,
                    kind: UnifiedBlockKind::ToolCall,
                },
                UnifiedStreamEvent::ToolCallStart {
                    index: 2,
                    id: "toolu_123".to_string(),
                    name: "lookup_weather".to_string(),
                },
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index: 2,
                    item_index: None,
                    item_id: None,
                    id: Some("toolu_123".to_string()),
                    name: Some("lookup_weather".to_string()),
                    arguments: "{\"city\":\"Boston\"}".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_anthropic_event_to_unified_stream_events_preserves_thinking_lifecycle() {
        let mut session = AnthropicSessionState::default();
        let start = anthropic_event_to_unified_stream_events_with_state(
            AnthropicEvent::ContentBlockStart {
                index: 1,
                content_block: AnthropicContentBlock::Thinking {
                    thinking: String::new(),
                    signature: None,
                },
            },
            &mut session,
        );
        assert_eq!(
            start,
            vec![
                UnifiedStreamEvent::ItemAdded {
                    item_index: Some(1),
                    item_id: None,
                    item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                        content: Vec::new(),
                        annotations: Vec::new(),
                    }),
                },
                UnifiedStreamEvent::ReasoningStart { index: 1 },
            ]
        );

        let delta = anthropic_event_to_unified_stream_events_with_state(
            AnthropicEvent::ContentBlockDelta {
                index: 1,
                delta: AnthropicContentDelta::ThinkingDelta {
                    thinking: "step one".to_string(),
                },
            },
            &mut session,
        );
        assert_eq!(
            delta,
            vec![UnifiedStreamEvent::ReasoningDelta {
                index: 1,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "step one".to_string(),
            }]
        );

        let signature = anthropic_event_to_unified_stream_events_with_state(
            AnthropicEvent::ContentBlockDelta {
                index: 1,
                delta: AnthropicContentDelta::SignatureDelta {
                    signature: "sig_123".to_string(),
                },
            },
            &mut session,
        );
        assert_eq!(
            signature,
            vec![UnifiedStreamEvent::BlobDelta {
                index: Some(1),
                data: json!({
                    "provider": "anthropic",
                    "type": "signature_delta",
                    "signature": "sig_123",
                }),
            }]
        );

        let stop = anthropic_event_to_unified_stream_events_with_state(
            AnthropicEvent::ContentBlockStop { index: 1 },
            &mut session,
        );
        assert_eq!(
            stop,
            vec![
                UnifiedStreamEvent::ReasoningStop { index: 1 },
                UnifiedStreamEvent::ItemDone {
                    item_index: Some(1),
                    item_id: None,
                    item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                        content: vec![UnifiedContentPart::Reasoning {
                            text: "step one".to_string(),
                        }],
                        annotations: Vec::new(),
                    }),
                },
            ]
        );
    }

    #[test]
    fn test_transform_unified_chunk_to_anthropic_events() {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);

        // Role chunk
        let unified_chunk_role = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: Some("test-model".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![],
                },
                finish_reason: None,
            }],
            ..Default::default()
        };
        let events_role =
            transform_unified_chunk_to_anthropic_events(unified_chunk_role, &mut state).unwrap();
        assert_eq!(events_role.len(), 1);
        assert_eq!(events_role[0].event.as_deref(), Some("message_start"));
        assert!(
            events_role[0]
                .data
                .contains("\"usage\":{\"input_tokens\":0,\"output_tokens\":0}")
        );
        assert!(state.session.anthropic.message_started);
        assert!(state.session.anthropic.active_blocks.is_empty());

        // Content chunk
        let unified_chunk_content = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: Some("test-model".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: None,
                    content: vec![UnifiedContentPartDelta::TextDelta {
                        index: 0,
                        text: "Hello".to_string(),
                    }],
                },
                finish_reason: None,
            }],
            ..Default::default()
        };
        let events_content =
            transform_unified_chunk_to_anthropic_events(unified_chunk_content, &mut state).unwrap();
        assert_eq!(events_content.len(), 2);
        assert_eq!(
            events_content[0].event.as_deref(),
            Some("content_block_start")
        );
        assert_eq!(
            events_content[1].event.as_deref(),
            Some("content_block_delta")
        );
        assert!(
            events_content[1]
                .data
                .contains("\"delta\":{\"text\":\"Hello\",\"type\":\"text_delta\"}")
        );
        assert!(state.session.anthropic.active_blocks.contains_key(&0));

        // Finish chunk
        let unified_chunk_finish = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: Some("test-model".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: None,
                    content: vec![],
                },
                finish_reason: Some("stop".to_string()),
            }],
            ..Default::default()
        };
        let events_finish =
            transform_unified_chunk_to_anthropic_events(unified_chunk_finish, &mut state).unwrap();
        assert_eq!(events_finish.len(), 3);
        assert_eq!(
            events_finish[0].event.as_deref(),
            Some("content_block_stop")
        );
        assert_eq!(events_finish[1].event.as_deref(), Some("message_delta"));
        assert!(
            events_finish[1]
                .data
                .contains("\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}")
        );
        assert!(
            events_finish[1]
                .data
                .contains("\"usage\":{\"input_tokens\":0,\"output_tokens\":0}")
        );
        assert_eq!(events_finish[2].event.as_deref(), Some("message_stop"));

        // Thinking content chunk - NOTE: This behavior is no longer supported directly
        // with the new model. Text parts should be used instead. This test may need
        // to be re-evaluated based on desired behavior for "thinking" messages.
        let unified_chunk_thinking = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: Some("test-model".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: None,
                    content: vec![UnifiedContentPartDelta::TextDelta {
                        index: 1, // Assuming a different content block for thinking
                        text: "Thinking...".to_string(),
                    }],
                },
                finish_reason: None,
            }],
            ..Default::default()
        };
        let events_thinking =
            transform_unified_chunk_to_anthropic_events(unified_chunk_thinking, &mut state);
        // Depending on the new logic, this might produce a regular text delta or be handled differently.
        // For now, let's assume it becomes a normal text block.
        assert!(events_thinking.is_some());
        let events = events_thinking.unwrap();
        assert_eq!(events[0].event.as_deref(), Some("content_block_start"));
        assert_eq!(events[1].event.as_deref(), Some("content_block_delta"));
        assert!(
            events[1]
                .data
                .contains("\"delta\":{\"text\":\"Thinking...\",\"type\":\"text_delta\"}")
        );
    }

    #[test]
    fn test_transform_unified_stream_events_to_anthropic_events_preserves_tool_and_thinking_native_lifecycle()
     {
        let mut state = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Anthropic);
        state.session.stream_id = Some("msg_native".to_string());
        state.session.stream_model = Some("claude-3-7-sonnet".to_string());

        let events = transform_unified_stream_events_to_anthropic_events(
            vec![
                UnifiedStreamEvent::MessageStart {
                    id: Some("msg_native".to_string()),
                    model: Some("claude-3-7-sonnet".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ToolCallStart {
                    index: 0,
                    id: "toolu_456".to_string(),
                    name: "lookup_weather".to_string(),
                },
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    id: Some("toolu_456".to_string()),
                    name: Some("lookup_weather".to_string()),
                    arguments: "{\"city\":\"Boston\"}".to_string(),
                },
                UnifiedStreamEvent::ToolCallStop {
                    index: 0,
                    id: Some("toolu_456".to_string()),
                },
                UnifiedStreamEvent::ReasoningStart { index: 1 },
                UnifiedStreamEvent::ReasoningDelta {
                    index: 1,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "step one".to_string(),
                },
                UnifiedStreamEvent::BlobDelta {
                    index: Some(1),
                    data: json!({
                        "provider": "anthropic",
                        "type": "signature_delta",
                        "signature": "sig_456",
                    }),
                },
                UnifiedStreamEvent::ReasoningStop { index: 1 },
            ],
            &mut state,
        )
        .unwrap();

        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[1].event.as_deref(), Some("content_block_start"));
        assert!(events[1].data.contains("\"type\":\"tool_use\""));
        assert_eq!(events[2].event.as_deref(), Some("content_block_delta"));
        assert!(events[2].data.contains("\"type\":\"input_json_delta\""));
        assert_eq!(events[3].event.as_deref(), Some("content_block_stop"));
        assert_eq!(events[4].event.as_deref(), Some("content_block_start"));
        assert!(events[4].data.contains("\"type\":\"thinking\""));
        assert_eq!(events[5].event.as_deref(), Some("content_block_delta"));
        assert!(events[5].data.contains("\"type\":\"thinking_delta\""));
        assert_eq!(events[6].event.as_deref(), Some("content_block_delta"));
        assert!(events[6].data.contains("\"type\":\"signature_delta\""));
        assert_eq!(events[7].event.as_deref(), Some("content_block_stop"));
    }

    #[test]
    fn test_transform_unified_stream_events_to_anthropic_events_delays_usage_until_terminal_message_delta()
     {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);
        let events = transform_unified_stream_events_to_anthropic_events(
            vec![
                UnifiedStreamEvent::MessageStart {
                    id: Some("msg_123".to_string()),
                    model: Some("gemini-2.5-flash-lite".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ContentBlockDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "I am Claude Code, Anth".to_string(),
                },
                UnifiedStreamEvent::Usage {
                    usage: UnifiedUsage {
                        input_tokens: 26,
                        output_tokens: 6,
                        total_tokens: 32,
                        ..Default::default()
                    },
                },
                UnifiedStreamEvent::ContentBlockDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "ropic's official CLI for Claude.".to_string(),
                },
                UnifiedStreamEvent::Usage {
                    usage: UnifiedUsage {
                        input_tokens: 26,
                        output_tokens: 22,
                        total_tokens: 48,
                        ..Default::default()
                    },
                },
                UnifiedStreamEvent::MessageDelta {
                    finish_reason: Some("stop".to_string()),
                },
                UnifiedStreamEvent::MessageStop,
            ],
            &mut state,
        )
        .unwrap();

        assert_eq!(events.len(), 7);
        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[1].event.as_deref(), Some("content_block_start"));
        assert_eq!(events[2].event.as_deref(), Some("content_block_delta"));
        assert_eq!(events[3].event.as_deref(), Some("content_block_delta"));
        assert_eq!(events[4].event.as_deref(), Some("content_block_stop"));
        assert_eq!(events[5].event.as_deref(), Some("message_delta"));
        assert_eq!(events[6].event.as_deref(), Some("message_stop"));
        assert!(
            events[5]
                .data
                .contains("\"usage\":{\"input_tokens\":26,\"output_tokens\":22}")
        );
        assert!(state.session.anthropic.active_blocks.is_empty());
    }

    #[test]
    fn test_openai_reasoning_stream_transforms_to_anthropic_thinking_then_text_blocks() {
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);

        let frames = vec![
            SseEvent {
                data: serde_json::to_string(&json!({
                    "id": "019d716629d1f4eb9470f60bc35eb311",
                    "object": "chat.completion.chunk",
                    "created": 1775724014_i64,
                    "model": "deepseek-ai/DeepSeek-V3.2",
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "",
                            "reasoning_content": null,
                            "role": "assistant",
                        },
                        "finish_reason": null
                    }],
                    "usage": {
                        "prompt_tokens": 98,
                        "completion_tokens": 0,
                        "total_tokens": 98,
                        "completion_tokens_details": {
                            "reasoning_tokens": 0
                        }
                    }
                }))
                .unwrap(),
                ..Default::default()
            },
            SseEvent {
                data: serde_json::to_string(&json!({
                    "id": "019d716629d1f4eb9470f60bc35eb311",
                    "object": "chat.completion.chunk",
                    "created": 1775724014_i64,
                    "model": "deepseek-ai/DeepSeek-V3.2",
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "",
                            "reasoning_content": "嗯",
                        },
                        "finish_reason": null
                    }]
                }))
                .unwrap(),
                ..Default::default()
            },
            SseEvent {
                data: serde_json::to_string(&json!({
                    "id": "019d716629d1f4eb9470f60bc35eb311",
                    "object": "chat.completion.chunk",
                    "created": 1775724014_i64,
                    "model": "deepseek-ai/DeepSeek-V3.2",
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "你好",
                            "reasoning_content": null,
                        },
                        "finish_reason": null
                    }]
                }))
                .unwrap(),
                ..Default::default()
            },
            SseEvent {
                data: serde_json::to_string(&json!({
                    "id": "019d716629d1f4eb9470f60bc35eb311",
                    "object": "chat.completion.chunk",
                    "created": 1775724014_i64,
                    "model": "deepseek-ai/DeepSeek-V3.2",
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "！",
                            "reasoning_content": null,
                        },
                        "finish_reason": null
                    }]
                }))
                .unwrap(),
                ..Default::default()
            },
            SseEvent {
                data: serde_json::to_string(&json!({
                    "id": "019d716629d1f4eb9470f60bc35eb311",
                    "object": "chat.completion.chunk",
                    "created": 1775724014_i64,
                    "model": "deepseek-ai/DeepSeek-V3.2",
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": "",
                            "reasoning_content": null,
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 98,
                        "completion_tokens": 134,
                        "total_tokens": 232,
                        "completion_tokens_details": {
                            "reasoning_tokens": 98
                        }
                    }
                }))
                .unwrap(),
                ..Default::default()
            },
            SseEvent {
                data: "[DONE]".to_string(),
                ..Default::default()
            },
        ];

        let events: Vec<SseEvent> = frames
            .into_iter()
            .flat_map(|event| transformer.transform_event(event).unwrap_or_default())
            .collect();

        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[1].event.as_deref(), Some("content_block_start"));
        assert!(events[1].data.contains("\"type\":\"thinking\""));
        assert!(events[1].data.contains("\"signature\":\"\""));
        assert_eq!(events[2].event.as_deref(), Some("content_block_delta"));
        assert!(events[2].data.contains("\"type\":\"thinking_delta\""));
        assert!(events[2].data.contains("\"thinking\":\"嗯\""));
        assert_eq!(events[3].event.as_deref(), Some("content_block_stop"));
        assert_eq!(events[4].event.as_deref(), Some("content_block_start"));
        assert!(events[4].data.contains("\"index\":1"));
        assert!(events[4].data.contains("\"type\":\"text\""));
        assert_eq!(events[5].event.as_deref(), Some("content_block_delta"));
        assert!(events[5].data.contains("\"text\":\"你好\""));
        assert_eq!(events[6].event.as_deref(), Some("content_block_delta"));
        assert!(events[6].data.contains("\"text\":\"！\""));
        assert_eq!(events[7].event.as_deref(), Some("content_block_stop"));
        assert_eq!(events[8].event.as_deref(), Some("message_delta"));
        assert!(
            events[8]
                .data
                .contains("\"usage\":{\"input_tokens\":98,\"output_tokens\":134}")
        );
        assert_eq!(events[9].event.as_deref(), Some("message_stop"));
    }

    #[test]
    fn test_transform_unified_chunk_to_anthropic_events_emits_diagnostic_for_image_delta() {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);
        let unified_chunk = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: Some("claude-3-7-sonnet".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![UnifiedContentPartDelta::ImageDelta {
                        index: 0,
                        url: Some("https://example.com/chart.png".to_string()),
                        data: None,
                    }],
                },
                finish_reason: None,
            }],
            ..Default::default()
        };

        let events =
            transform_unified_chunk_to_anthropic_events(unified_chunk, &mut state).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[1].event.as_deref(), Some("transform_diagnostic"));
        let diagnostic: Value = serde_json::from_str(&events[1].data).unwrap();
        assert_eq!(diagnostic["semantic_unit"], json!("ImageDelta"));
        assert_eq!(state.session.diagnostics.len(), 1);
    }

    #[test]
    fn test_transform_unified_chunk_to_anthropic_events_preserves_usage_in_start_and_finish() {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);

        let start_chunk = UnifiedChunkResponse {
            id: "cmpl-usage".to_string(),
            model: Some("test-model".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![],
                },
                finish_reason: None,
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 2,
                output_tokens: 0,
                total_tokens: 2,
                ..Default::default()
            }),
            ..Default::default()
        };
        let start_events =
            transform_unified_chunk_to_anthropic_events(start_chunk, &mut state).unwrap();

        assert_eq!(start_events.len(), 1);
        assert_eq!(start_events[0].event.as_deref(), Some("message_start"));
        assert!(
            start_events[0]
                .data
                .contains("\"usage\":{\"input_tokens\":2,\"output_tokens\":0}")
        );

        let finish_chunk = UnifiedChunkResponse {
            id: "cmpl-usage".to_string(),
            model: Some("test-model".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta::default(),
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 2,
                output_tokens: 8,
                total_tokens: 10,
                ..Default::default()
            }),
            ..Default::default()
        };
        let finish_events =
            transform_unified_chunk_to_anthropic_events(finish_chunk, &mut state).unwrap();

        assert_eq!(finish_events.len(), 2);
        assert_eq!(finish_events[0].event.as_deref(), Some("message_delta"));
        assert!(
            finish_events[0]
                .data
                .contains("\"usage\":{\"input_tokens\":2,\"output_tokens\":8}")
        );
        assert!(
            finish_events[0]
                .data
                .contains("\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}")
        );
        assert_eq!(finish_events[1].event.as_deref(), Some("message_stop"));
    }

    #[test]
    fn test_anthropic_response_with_tool_use_and_text_to_unified() {
        let anthropic_response = AnthropicResponse {
            id: "msg_123".to_string(),
            type_: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![
                AnthropicContentBlock::Text {
                    text: "I'm thinking...".to_string(),
                },
                AnthropicContentBlock::ToolUse {
                    id: "tool_123".to_string(),
                    name: "get_weather".to_string(),
                    input: json!({"location": "SF"}),
                },
            ],
            model: "claude-3-opus-20240229".to_string(),
            stop_reason: Some("tool_use".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 20,
            },
        };

        let unified_response: UnifiedResponse = anthropic_response.into();
        let choice = &unified_response.choices[0];
        assert_eq!(choice.message.content.len(), 2);
        assert_eq!(
            choice.message.content[0],
            UnifiedContentPart::Text {
                text: "I'm thinking...".to_string()
            }
        );
        assert_eq!(
            choice.message.content[1],
            UnifiedContentPart::ToolCall(UnifiedToolCall {
                id: "tool_123".to_string(),
                name: "get_weather".to_string(),
                arguments: json!({"location": "SF"}),
            })
        );
        assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));
    }

    #[test]
    fn test_unified_response_with_thinking_content_to_anthropic() {
        let unified_response = UnifiedResponse {
            id: "msg_123".to_string(),
            model: Some("claude-3-opus-20240229".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![
                        UnifiedContentPart::Text {
                            text: "I'm thinking...".to_string(),
                        },
                        UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: "tool_123".to_string(),
                            name: "get_weather".to_string(),
                            arguments: json!({"location": "SF"}),
                        }),
                    ],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: Some("tool_calls".to_string()),
                logprobs: None,
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
                ..Default::default()
            }),
            created: Some(12345),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let anthropic_response: AnthropicResponse = unified_response.into();
        assert_eq!(anthropic_response.content.len(), 2);
        match &anthropic_response.content[0] {
            AnthropicContentBlock::Text { text } => assert_eq!(text, "I'm thinking..."),
            _ => panic!("Expected text content block"),
        }
        match &anthropic_response.content[1] {
            AnthropicContentBlock::ToolUse { name, .. } => assert_eq!(name, "get_weather"),
            _ => panic!("Expected tool use content block"),
        }
        assert_eq!(anthropic_response.stop_reason, Some("tool_use".to_string()));
    }

    #[test]
    fn test_anthropic_response_to_unified_preserves_items() {
        let anthropic_response = AnthropicResponse {
            id: "msg_123".to_string(),
            type_: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![
                AnthropicContentBlock::Text {
                    text: "Thinking".to_string(),
                },
                AnthropicContentBlock::ToolUse {
                    id: "tool_123".to_string(),
                    name: "get_weather".to_string(),
                    input: json!({"location": "SF"}),
                },
            ],
            model: "claude-3-opus-20240229".to_string(),
            stop_reason: Some("tool_use".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 20,
            },
        };

        let unified_response: UnifiedResponse = anthropic_response.into();
        let items = &unified_response.choices[0].items;

        assert_eq!(items.len(), 2);
        match &items[0] {
            UnifiedItem::Message(item) => {
                assert_eq!(item.role, UnifiedRole::Assistant);
                assert_eq!(
                    item.content,
                    vec![UnifiedContentPart::Text {
                        text: "Thinking".to_string()
                    }]
                );
            }
            other => panic!("Expected message item, got {other:?}"),
        }

        match &items[1] {
            UnifiedItem::FunctionCall(item) => {
                assert_eq!(item.id, "tool_123");
                assert_eq!(item.name, "get_weather");
                assert_eq!(item.arguments, json!({"location": "SF"}));
            }
            other => panic!("Expected function call item, got {other:?}"),
        }
    }
}
