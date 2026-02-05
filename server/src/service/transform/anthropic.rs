use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::unified::*;
use super::StreamTransformer;
use crate::utils::ID_GENERATOR;
use crate::utils::sse::SseEvent;

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
        let mut tool_id_to_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();

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
                                let content_str = if content_val.is_string() {
                                    content_val.as_str().unwrap_or("").to_string()
                                } else {
                                    serde_json::to_string(content_val).unwrap_or_default()
                                };

                                // Look up the tool name from our mapping
                                let tool_name = tool_id_to_name
                                    .get(tool_use_id)
                                    .cloned()
                                    .unwrap_or_default();

                                content_parts.push(UnifiedContentPart::ToolResult(
                                    UnifiedToolResult {
                                        tool_call_id: tool_use_id.to_string(),
                                        name: tool_name,
                                        content: content_str,
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

        UnifiedRequest {
            model: Some(anthropic_req.model),
            messages,
            tools,
            stream: anthropic_req.stream.unwrap_or(false),
            temperature: anthropic_req.temperature,
            max_tokens: Some(anthropic_req.max_tokens),
            top_p: anthropic_req.top_p,
            top_k: anthropic_req.top_k,
            stop: anthropic_req.stop_sequences,
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            metadata: anthropic_req.metadata,
            ..Default::default()
        }
        .filter_empty() // Filter out empty content and messages
    }
}

impl From<UnifiedRequest> for AnthropicRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
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
                            UnifiedContentPart::ImageUrl { .. } | 
                            UnifiedContentPart::ImageData { .. } | 
                            UnifiedContentPart::FileData { .. } | 
                            UnifiedContentPart::ExecutableCode { .. } => {
                                // Multimodal content not fully supported in Anthropic conversion yet
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
                                    "content": result.content
                                }));
                            }
                        }
                    }

                    // Anthropic's API has a special case for single-text-block messages
                    // where the `content` can be a plain string.
                    let content = if content_blocks.len() == 1
                        && content_blocks[0]
                            .get("type")
                            .and_then(|t| t.as_str())
                            == Some("text")
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
            metadata: None,
            top_k: None,
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum AnthropicContentBlock {
    Text {
        text: String,
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
            .into_iter()
            .map(|block| match block {
                AnthropicContentBlock::Text { text } => UnifiedContentPart::Text { text },
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    UnifiedContentPart::ToolCall(UnifiedToolCall {
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
        };

        let finish_reason = anthropic_res.stop_reason.map(|reason| {
            crate::service::transform::unified::map_anthropic_finish_reason_to_openai(&reason)
        });

        let choice = UnifiedChoice {
            index: 0,
            message,
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
            model: anthropic_res.model,
            choices: vec![choice],
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
        }
    }
}

impl From<UnifiedResponse> for AnthropicResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let choice = unified_res.choices.into_iter().next().unwrap_or_else(|| {
            UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "".to_string(),
                    }],
                },
                finish_reason: None,
                logprobs: None,
            }
        });

        let content: Vec<AnthropicContentBlock> = choice
            .message
            .content
            .into_iter()
            .filter_map(|part| match part {
                UnifiedContentPart::Text { text } => {
                    Some(AnthropicContentBlock::Text { text })
                }
                UnifiedContentPart::ImageUrl { .. } | 
                UnifiedContentPart::ImageData { .. } | 
                UnifiedContentPart::FileData { .. } | 
                UnifiedContentPart::ExecutableCode { .. } => {
                    // Multimodal content not fully supported in Anthropic conversion yet
                    None
                }
                UnifiedContentPart::ToolCall(call) => {
                    Some(AnthropicContentBlock::ToolUse {
                        id: call.id,
                        name: call.name,
                        input: call.arguments,
                    })
                }
                UnifiedContentPart::ToolResult(_) => None, // Not applicable for assistant response
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
            model: unified_res.model,
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
    MessageStart { message: AnthropicStreamMessage },
    ContentBlockStart { index: u32, content_block: AnthropicContentBlock },
    ContentBlockDelta { index: u32, delta: AnthropicContentDelta },
    ContentBlockStop { index: u32 },
    MessageDelta { delta: MessageDelta },
    MessageStop,
    Error { error: Value },
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
#[serde(untagged)]
pub enum AnthropicContentDelta {
    TextDelta { text: String },
    InputJsonDelta {
        #[serde(rename = "type")]
        type_: String,
        partial_json: String,
    },
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
            AnthropicEvent::ContentBlockDelta { index, delta } => {
                if let AnthropicContentDelta::TextDelta { text } = delta {
                    choice
                        .delta
                        .content
                        .push(UnifiedContentPartDelta::TextDelta { index, text });
                }
                // Ignoring tool use deltas for now due to unified model limitations
            }
            AnthropicEvent::MessageDelta { delta } => {
                if let Some(stop_reason) = &delta.stop_reason {
                    choice.finish_reason = Some(
                        crate::service::transform::unified::map_anthropic_finish_reason_to_openai(stop_reason)
                    );
                }
            }
            // Other events don't map to a chunk with content, so we create an empty one.
            _ => {}
        }

        UnifiedChunkResponse {
            id,
            model,
            choices: vec![choice],
            usage: None, // Anthropic provides usage at the end, not per chunk
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
        }
    }
}


pub fn transform_unified_chunk_to_anthropic_events(
    unified_chunk: UnifiedChunkResponse,
    state: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut events: Vec<SseEvent> = Vec::new();

    if let Some(choice) = unified_chunk.choices.get(0) {
        // Send message_start on the very first chunk that has a role.
        if state.is_first_chunk && choice.delta.role.is_some() {
            state.is_first_chunk = false;
            let event = json!({
                "type": "message_start",
                "message": {
                    "id": unified_chunk.id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": unified_chunk.model
                }
            });
            events.push(SseEvent {
                event: Some("message_start".to_string()),
                data: serde_json::to_string(&event).unwrap(),
                ..Default::default()
            });
        }

        let has_text_delta = choice
            .delta
            .content
            .iter()
            .any(|p| matches!(p, UnifiedContentPartDelta::TextDelta { .. }));

        // If this chunk has content and it's the first content chunk, send content_block_start.
        if has_text_delta && state.is_first_content_chunk {
            state.is_first_content_chunk = false; // Only send this block once for the text block.

            let content_block_start_event = json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            });
            events.push(SseEvent {
                event: Some("content_block_start".to_string()),
                data: serde_json::to_string(&content_block_start_event).unwrap(),
                ..Default::default()
            });
        }

        for part in &choice.delta.content {
            match part {
                UnifiedContentPartDelta::TextDelta { index, text } => {
                    // Anthropic's content_block_delta for text has delta as {"text": "..."}
                    let delta = json!({"text": text});
                    let event = json!({"type": "content_block_delta", "index": *index, "delta": delta});
                    events.push(SseEvent {
                        event: Some("content_block_delta".to_string()),
                        data: serde_json::to_string(&event).unwrap(),
                        ..Default::default()
                    });
                }
                UnifiedContentPartDelta::ImageDelta { .. } => {
                    // Image content not fully supported in Anthropic chunk conversion yet
                }
                UnifiedContentPartDelta::ToolCallDelta(_tool_delta) => {
                    // Handle tool call streaming if necessary in the future
                }
            }
        }

        if let Some(finish_reason) = &choice.finish_reason {
            // If a content block was started, it must be stopped.
            if !state.is_first_content_chunk {
                let content_block_stop_event = json!({
                    "type": "content_block_stop",
                    "index": 0
                });
                events.push(SseEvent {
                    event: Some("content_block_stop".to_string()),
                    data: serde_json::to_string(&content_block_stop_event).unwrap(),
                    ..Default::default()
                });
            }

            let reason = crate::service::transform::unified::map_openai_finish_reason_to_anthropic(finish_reason);
            // Anthropic's message_delta has usage inside delta object
            let delta = json!({
                "stop_reason": reason,
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0
                }
            });
            let event = json!({"type": "message_delta", "delta": delta});
            events.push(SseEvent {
                event: Some("message_delta".to_string()),
                data: serde_json::to_string(&event).unwrap(),
                ..Default::default()
            });
            events.push(SseEvent {
                event: Some("message_stop".to_string()),
                data: "{\"type\":\"message_stop\"}".to_string(),
                ..Default::default()
            });
        }
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::llm_types::LlmApiType;
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

        assert_eq!(unified_request.model, Some("claude-3-opus-20240229".to_string()));
        assert_eq!(unified_request.messages.len(), 2);
        assert_eq!(unified_request.messages[0].role, UnifiedRole::System);
        assert_eq!(
            unified_request.messages[0].content.len(),
            1
        );
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
        assert_eq!(anthropic_request.messages[0].content, json!("Hello, world!"));
        assert_eq!(anthropic_request.max_tokens, 100);
        assert_eq!(anthropic_request.temperature, Some(0.7));
        assert_eq!(anthropic_request.stream, Some(true));
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
        assert_eq!(unified_response.model, "claude-3-opus-20240229");
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
            model: "claude-3-opus-20240229".to_string(),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "Hello from Anthropic!".to_string(),
                    }],
                },
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
        assert_eq!(unified_chunk_start.model, "claude-3");
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
                usage: Some(AnthropicUsage {
                    input_tokens: 0,
                    output_tokens: 10,
                }),
            },
        };
        let unified_chunk_stop: UnifiedChunkResponse = event_stop.into();
        assert_eq!(
            unified_chunk_stop.choices[0].finish_reason,
            Some("stop".to_string())
        );
        assert!(unified_chunk_stop.choices[0].delta.content.is_empty());
    }

    #[test]
    fn test_transform_unified_chunk_to_anthropic_events() {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);

        // Role chunk
        let unified_chunk_role = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
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
        assert!(!state.is_first_chunk);
        assert!(state.is_first_content_chunk); // No content yet, so this should still be true

        // Content chunk
        let unified_chunk_content = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
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
        assert_eq!(events_content[0].event.as_deref(), Some("content_block_start"));
        assert_eq!(events_content[1].event.as_deref(), Some("content_block_delta"));
        assert!(events_content[1].data.contains("\"text\":\"Hello\""));
        assert!(!state.is_first_content_chunk);

        // Finish chunk
        let unified_chunk_finish = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
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
        assert_eq!(events_finish[0].event.as_deref(), Some("content_block_stop"));
        assert_eq!(events_finish[1].event.as_deref(), Some("message_delta"));
        assert!(events_finish[1].data.contains("\"stop_reason\":\"end_turn\""));
        assert_eq!(events_finish[2].event.as_deref(), Some("message_stop"));

        // Thinking content chunk - NOTE: This behavior is no longer supported directly
        // with the new model. Text parts should be used instead. This test may need
        // to be re-evaluated based on desired behavior for "thinking" messages.
        let unified_chunk_thinking = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
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
        assert_eq!(events[0].event.as_deref(), Some("content_block_delta"));
        assert!(events[0].data.contains("\"text\":\"Thinking...\""));
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
            model: "claude-3-opus-20240229".to_string(),
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
                },
                finish_reason: Some("tool_calls".to_string()),
                logprobs: None,
            }],
            usage: Some(UnifiedUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            }),
            created: Some(12345),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
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
        assert_eq!(
            anthropic_response.stop_reason,
            Some("tool_use".to_string())
        );
    }
}

