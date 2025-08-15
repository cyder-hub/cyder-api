use axum::body::Bytes;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::unified::*;
use super::StreamTransformer;
use crate::utils::ID_GENERATOR;

// --- Anthropic to Unified ---

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicRequestPayload {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
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

        if let Some(system_prompt) = anthropic_req.system {
            messages.push(UnifiedMessage {
                role: UnifiedRole::System,
                content: UnifiedMessageContent::Text(system_prompt),
                thinking_content: None,
            });
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
                    content: UnifiedMessageContent::Text(s.to_string()),
                    thinking_content: None,
                });
            } else if let Some(blocks) = msg.content.as_array() {
                let mut text_parts = Vec::new();
                let mut tool_calls = Vec::new();
                let mut tool_results = Vec::new();

                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                text_parts.push(text);
                            }
                        }
                        Some("tool_use") if role == UnifiedRole::Assistant => {
                            if let (Some(id), Some(name), Some(input)) = (
                                block.get("id").and_then(|v| v.as_str()),
                                block.get("name").and_then(|v| v.as_str()),
                                block.get("input"),
                            ) {
                                tool_calls.push(UnifiedToolCall {
                                    id: id.to_string(),
                                    name: name.to_string(),
                                    arguments: input.clone(),
                                });
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

                                tool_results.push(UnifiedToolResult {
                                    tool_call_id: tool_use_id.to_string(),
                                    name: "".to_string(), // Anthropic doesn't provide this in the request
                                    content: content_str,
                                });
                            }
                        }
                        _ => {}
                    }
                }

                let text_content = text_parts.join("\n");

                if !tool_calls.is_empty() {
                    messages.push(UnifiedMessage {
                        role: UnifiedRole::Assistant,
                        content: UnifiedMessageContent::ToolCalls(tool_calls),
                        thinking_content: if !text_content.is_empty() {
                            Some(text_content)
                        } else {
                            None
                        },
                    });
                } else if !text_content.is_empty() {
                    messages.push(UnifiedMessage {
                        role, // user or assistant
                        content: UnifiedMessageContent::Text(text_content),
                        thinking_content: None,
                    });
                }

                for result in tool_results {
                    messages.push(UnifiedMessage {
                        role: UnifiedRole::Tool,
                        content: UnifiedMessageContent::ToolResult(result),
                        thinking_content: None,
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
            stop: anthropic_req.stop_sequences,
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
        }
    }
}

impl From<UnifiedRequest> for AnthropicRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let mut system = None;
        let mut messages = Vec::new();

        for msg in unified_req.messages {
            match msg.role {
                UnifiedRole::System => {
                    if let UnifiedMessageContent::Text(text) = msg.content {
                        system = Some(text);
                    }
                }
                UnifiedRole::User => {
                    if let UnifiedMessageContent::Text(text) = msg.content {
                        messages.push(AnthropicMessage {
                            role: "user".to_string(),
                            content: json!(text),
                        });
                    }
                }
                UnifiedRole::Assistant => {
                    let mut content_blocks: Vec<Value> = Vec::new();

                    if let Some(thinking) = msg.thinking_content {
                        if !thinking.is_empty() {
                            content_blocks.push(json!({
                                "type": "text",
                                "text": thinking
                            }));
                        }
                    }

                    let content = match msg.content {
                        UnifiedMessageContent::Text(text) => {
                            if content_blocks.is_empty() {
                                // Just text, no thinking content. Can be a plain string.
                                json!(text)
                            } else {
                                // Had thinking content, so must be an array of blocks.
                                content_blocks.push(json!({"type": "text", "text": text}));
                                json!(content_blocks)
                            }
                        }
                        UnifiedMessageContent::ToolCalls(calls) => {
                            content_blocks.extend(calls.into_iter().map(|call| {
                                json!({
                                    "type": "tool_use",
                                    "id": call.id,
                                    "name": call.name,
                                    "input": call.arguments
                                })
                            }));
                            json!(content_blocks)
                        }
                        _ => {
                            if content_blocks.is_empty() {
                                json!("")
                            } else {
                                json!(content_blocks)
                            }
                        }
                    };
                    messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content,
                    });
                }
                UnifiedRole::Tool => {
                    if let UnifiedMessageContent::ToolResult(result) = msg.content {
                        let content_block = json!({
                            "type": "tool_result",
                            "tool_use_id": result.tool_call_id,
                            "content": result.content
                        });
                        messages.push(AnthropicMessage {
                            role: "user".to_string(),
                            content: json!([content_block]),
                        });
                    }
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
            system,
            messages,
            max_tokens: unified_req.max_tokens.unwrap_or(4096), // Anthropic requires max_tokens
            tools,
            temperature: unified_req.temperature,
            top_p: unified_req.top_p,
            stop_sequences: unified_req.stop,
            stream: Some(unified_req.stream),
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
        let tool_calls: Vec<UnifiedToolCall> = anthropic_res
            .content
            .iter()
            .filter_map(|block| match block {
                AnthropicContentBlock::ToolUse { id, name, input } => Some(UnifiedToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: input.clone(),
                }),
                _ => None,
            })
            .collect();

        let text = anthropic_res
            .content
            .iter()
            .filter_map(|block| match block {
                AnthropicContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<&str>>()
            .join("");

        let (message_content, thinking_content) = if !tool_calls.is_empty() {
            (
                UnifiedMessageContent::ToolCalls(tool_calls),
                if !text.is_empty() { Some(text) } else { None },
            )
        } else {
            (UnifiedMessageContent::Text(text), None)
        };

        let message = UnifiedMessage {
            role: UnifiedRole::Assistant,
            content: message_content,
            thinking_content,
        };

        let finish_reason = anthropic_res.stop_reason.map(|reason| {
            match reason.as_str() {
                "end_turn" | "stop_sequence" => "stop",
                "tool_use" => "tool_calls",
                "max_tokens" => "length",
                _ => "stop", // Default to stop for other reasons
            }
            .to_string()
        });

        let choice = UnifiedChoice {
            index: 0,
            message,
            finish_reason,
        };

        let usage = Some(UnifiedUsage {
            prompt_tokens: anthropic_res.usage.input_tokens,
            completion_tokens: anthropic_res.usage.output_tokens,
            total_tokens: anthropic_res.usage.input_tokens + anthropic_res.usage.output_tokens,
        });

        UnifiedResponse {
            id: anthropic_res.id,
            model: anthropic_res.model,
            choices: vec![choice],
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
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
                    content: UnifiedMessageContent::Text("".to_string()),
                    thinking_content: None,
                },
                finish_reason: None,
            }
        });

        let mut content: Vec<AnthropicContentBlock> = Vec::new();

        if let Some(thinking) = choice.message.thinking_content {
            if !thinking.is_empty() {
                content.push(AnthropicContentBlock::Text { text: thinking });
            }
        }

        match choice.message.content {
            UnifiedMessageContent::Text(text) => {
                content.push(AnthropicContentBlock::Text { text });
            }
            UnifiedMessageContent::ToolCalls(calls) => {
                content.extend(calls.into_iter().map(|call| {
                    AnthropicContentBlock::ToolUse {
                        id: call.id,
                        name: call.name,
                        input: call.arguments,
                    }
                }));
            }
            UnifiedMessageContent::ToolResult(_) => {} // Not applicable for assistant response
        };

        let stop_reason = choice.finish_reason.map(|reason| {
            match reason.as_str() {
                "stop" => "end_turn",
                "tool_calls" => "tool_use",
                "length" => "max_tokens",
                _ => "end_turn",
            }
            .to_string()
        });

        let usage = unified_res.usage.map_or_else(
            || AnthropicUsage {
                input_tokens: 0,
                output_tokens: 0,
            },
            |u| AnthropicUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
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
    MessageDelta { delta: MessageDelta, usage: MessageDeltaUsage },
    MessageStop,
    Error { error: Value },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicStreamMessage {
    pub id: String,
    pub model: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageDelta {
    pub stop_reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageDeltaUsage {
    pub output_tokens: u32,
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
            AnthropicEvent::ContentBlockDelta { delta, .. } => {
                if let AnthropicContentDelta::TextDelta { text } = delta {
                    choice.delta.content = Some(text);
                }
                // Ignoring tool use deltas for now due to unified model limitations
            }
            AnthropicEvent::MessageDelta { delta, .. } => {
                choice.finish_reason = Some(match delta.stop_reason.as_str() {
                    "end_turn" | "stop_sequence" => "stop".to_string(),
                    "tool_use" => "tool_calls".to_string(),
                    "max_tokens" => "length".to_string(),
                    _ => "stop".to_string(),
                });
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


pub fn transform_unified_chunk_to_anthropic_bytes(unified_chunk: UnifiedChunkResponse, state: &mut StreamTransformer) -> Option<Bytes> {
    let mut events: Vec<String> = Vec::new();

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
                    "model": unified_chunk.model,
                    "usage": {"input_tokens": 0, "output_tokens": 0}
                }
            });
            events.push(format!("event: message_start\ndata: {}\n\n", serde_json::to_string(&event).unwrap()));
        }

        let has_content = choice.delta.content.is_some();

        // If this chunk has content and it's the first content chunk, send content_block_start.
        if has_content && state.is_first_content_chunk {
            state.is_first_content_chunk = false; // Only send this block once for the text block.

            // If there is thinking message sent before, add signature and content_block_stop event
            if state.has_thinking_content {
                // This is a fake signature event that some clients might expect.
                let thinking_signature_event = json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "signature_delta",
                        "signature": "EqQBCgIYAhIM1gbcDa9GJwZA2b3hGgxBdjrkzLoky3dl1pkiMOYds"
                    }
                });
                events.push(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&thinking_signature_event).unwrap()));

                let content_block_stop_event = json!({
                    "type": "content_block_stop",
                    "index": 0
                });
                events.push(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&content_block_stop_event).unwrap()));
            }

            let content_block_start_event = json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            });
            events.push(format!("event: content_block_start\ndata: {}\n\n", serde_json::to_string(&content_block_start_event).unwrap()));
        }

        if let Some(thinking) = &choice.delta.thinking_content {
            if state.is_first_thinking_content {
                state.is_first_thinking_content = false;

                // add content_block_start if first thinking chunk
                let content_block_start_event = json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "thinking",
                        "thinking": ""
                    }
                });
                events.push(format!("event: content_block_start\ndata: {}\n\n", serde_json::to_string(&content_block_start_event).unwrap()));
            }
            state.has_thinking_content = true;

            let delta = json!({"type": "thinking_delta", "thinking": thinking});
            let event = json!({"type": "content_block_delta", "index": 0, "delta": delta});
            events.push(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&event).unwrap()));
        }
        if let Some(content) = &choice.delta.content {
            let delta = json!({"type": "text_delta", "text": content});
            let event = json!({"type": "content_block_delta", "index": 0, "delta": delta});
            events.push(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&event).unwrap()));
        }
        if let Some(finish_reason) = &choice.finish_reason {
            // If a content block was started, it must be stopped.
            if !state.is_first_content_chunk {
                let content_block_stop_event = json!({
                    "type": "content_block_stop",
                    "index": 0
                });
                events.push(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&content_block_stop_event).unwrap()));
            }

            let reason = match finish_reason.as_str() {
                "stop" => "end_turn",
                "length" => "max_tokens",
                "tool_calls" => "tool_use",
                _ => "end_turn",
            };
            let delta = json!({"stop_reason": reason});
            let event = json!({"type": "message_delta", "delta": delta, "usage": {"output_tokens": 0}});
            events.push(format!("event: message_delta\ndata: {}\n\n", serde_json::to_string(&event).unwrap()));
            events.push("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string());
        }
    }

    if events.is_empty() {
        None
    } else {
        Some(Bytes::from(events.join("")))
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
            system: Some("You are a helpful assistant.".to_string()),
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
        };

        let unified_request: UnifiedRequest = anthropic_request.into();

        assert_eq!(unified_request.model, Some("claude-3-opus-20240229".to_string()));
        assert_eq!(unified_request.messages.len(), 2);
        assert_eq!(unified_request.messages[0].role, UnifiedRole::System);
        assert_eq!(
            unified_request.messages[0].content,
            UnifiedMessageContent::Text("You are a helpful assistant.".to_string())
        );
        assert!(unified_request.messages[0].thinking_content.is_none());
        assert_eq!(unified_request.messages[1].role, UnifiedRole::User);
        assert_eq!(
            unified_request.messages[1].content,
            UnifiedMessageContent::Text("Hello, world!".to_string())
        );
        assert!(unified_request.messages[1].thinking_content.is_none());
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
                    content: UnifiedMessageContent::Text("You are a helpful assistant.".to_string()),
                    thinking_content: None,
                },
                UnifiedMessage {
                    role: UnifiedRole::User,
                    content: UnifiedMessageContent::Text("Hello, world!".to_string()),
                    thinking_content: None,
                },
            ],
            max_tokens: Some(100),
            temperature: Some(0.7),
            stream: true,
            ..Default::default()
        };

        let anthropic_request: AnthropicRequestPayload = unified_request.into();

        assert_eq!(anthropic_request.model, "claude-3-opus-20240229");
        assert_eq!(anthropic_request.system, Some("You are a helpful assistant.".to_string()));
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
        assert_eq!(
            choice.message.content,
            UnifiedMessageContent::Text("Hello from Anthropic!".to_string())
        );
        assert!(choice.message.thinking_content.is_none());
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
        let usage = unified_response.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
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
                    content: UnifiedMessageContent::Text("Hello from Anthropic!".to_string()),
                    thinking_content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(UnifiedUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            }),
            created: Some(12345),
            object: Some("chat.completion".to_string()),
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
                model: "claude-3".to_string(),
                role: "assistant".to_string(),
            },
        };
        let unified_chunk_start: UnifiedChunkResponse = event_start.into();
        assert_eq!(unified_chunk_start.id, "msg_123");
        assert_eq!(unified_chunk_start.model, "claude-3");
        assert_eq!(unified_chunk_start.choices[0].delta.role, Some(UnifiedRole::Assistant));
        assert!(unified_chunk_start.choices[0].delta.content.is_none());
        assert!(unified_chunk_start.choices[0].delta.thinking_content.is_none());

        // ContentBlockDelta event
        let event_delta = AnthropicEvent::ContentBlockDelta {
            index: 0,
            delta: AnthropicContentDelta::TextDelta {
                text: "Hello".to_string(),
            },
        };
        let unified_chunk_delta: UnifiedChunkResponse = event_delta.into();
        assert!(unified_chunk_delta.id.starts_with("chatcmpl-"));
        assert_eq!(unified_chunk_delta.choices[0].delta.content, Some("Hello".to_string()));
        assert!(unified_chunk_delta.choices[0].delta.thinking_content.is_none());
        assert!(unified_chunk_delta.choices[0].delta.role.is_none());

        // MessageDelta event (finish reason)
        let event_stop = AnthropicEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: "end_turn".to_string(),
            },
            usage: MessageDeltaUsage { output_tokens: 10 },
        };
        let unified_chunk_stop: UnifiedChunkResponse = event_stop.into();
        assert_eq!(unified_chunk_stop.choices[0].finish_reason, Some("stop".to_string()));
        assert!(unified_chunk_stop.choices[0].delta.content.is_none());
        assert!(unified_chunk_stop.choices[0].delta.thinking_content.is_none());
    }

    #[test]
    fn test_transform_unified_chunk_to_anthropic_bytes() {
        let mut state = StreamTransformer::new(LlmApiType::OpenAI, LlmApiType::Anthropic);

        // Role chunk
        let unified_chunk_role = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: None,
                    tool_calls: None,
                    thinking_content: None,
                },
                finish_reason: None,
            }],
            ..Default::default()
        };
        let bytes_role = transform_unified_chunk_to_anthropic_bytes(unified_chunk_role, &mut state).unwrap();
        let str_role = String::from_utf8(bytes_role.to_vec()).unwrap();
        assert!(str_role.contains("event: message_start"));
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
                    content: Some("Hello".to_string()),
                    tool_calls: None,
                    thinking_content: None,
                },
                finish_reason: None,
            }],
            ..Default::default()
        };
        let bytes_content = transform_unified_chunk_to_anthropic_bytes(unified_chunk_content, &mut state).unwrap();
        let str_content = String::from_utf8(bytes_content.to_vec()).unwrap();
        assert!(str_content.contains("event: content_block_start"));
        assert!(str_content.contains("event: content_block_delta"));
        assert!(str_content.contains("\"text\":\"Hello\""));
        assert!(!state.is_first_content_chunk);

        // Finish chunk
        let unified_chunk_finish = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: None,
                    content: None,
                    thinking_content: None,
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            ..Default::default()
        };
        let bytes_finish = transform_unified_chunk_to_anthropic_bytes(unified_chunk_finish, &mut state).unwrap();
        let str_finish = String::from_utf8(bytes_finish.to_vec()).unwrap();
        assert!(str_finish.contains("event: content_block_stop"));
        assert!(str_finish.contains("event: message_delta"));
        assert!(str_finish.contains("\"stop_reason\":\"end_turn\""));
        assert!(str_finish.contains("event: message_stop"));

        // Thinking content chunk
        let unified_chunk_thinking = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: None,
                    content: None,
                    tool_calls: None,
                    thinking_content: Some("Thinking...".to_string()),
                },
                finish_reason: None,
            }],
            ..Default::default()
        };
        let bytes_thinking = transform_unified_chunk_to_anthropic_bytes(unified_chunk_thinking, &mut state).unwrap();
        let str_thinking = String::from_utf8(bytes_thinking.to_vec()).unwrap();
        assert!(str_thinking.contains("event: content_block_delta"));
        assert!(str_thinking.contains("\"type\":\"text_delta\""));
        assert!(str_thinking.contains("\"text\":\"Thinking...\""));
        assert!(state.has_thinking_content);
        assert!(!state.is_first_thinking_content);
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
        assert_eq!(
            choice.message.thinking_content,
            Some("I'm thinking...".to_string())
        );
        assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));
        match &choice.message.content {
            UnifiedMessageContent::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "get_weather");
            }
            _ => panic!("Expected ToolCalls"),
        }
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
                    content: UnifiedMessageContent::ToolCalls(vec![UnifiedToolCall {
                        id: "tool_123".to_string(),
                        name: "get_weather".to_string(),
                        arguments: json!({"location": "SF"}),
                    }]),
                    thinking_content: Some("I'm thinking...".to_string()),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(UnifiedUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            }),
            created: Some(12345),
            object: Some("chat.completion".to_string()),
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

