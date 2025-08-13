use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::unified::*;

// --- OpenAI to Unified ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiRequestPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<UnifiedTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    type_: String, // "function"
    function: OpenAiFunction,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpenAiFunction {
    name: String,
    arguments: String,
}

impl From<OpenAiRequestPayload> for UnifiedRequest {
    fn from(openai_req: OpenAiRequestPayload) -> Self {
        let messages = openai_req
            .messages
            .into_iter()
            .map(|msg| {
                let role = match msg.role.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    "tool" => UnifiedRole::Tool,
                    _ => UnifiedRole::User, // Default to user for unknown roles
                };

                let content = if let Some(tool_calls) = msg.tool_calls {
                    let calls = tool_calls
                        .into_iter()
                        .map(|tc| {
                            let args: Value = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(json!({}));
                            UnifiedToolCall {
                                id: tc.id,
                                name: tc.function.name,
                                arguments: args,
                            }
                        })
                        .collect();
                    UnifiedMessageContent::ToolCalls(calls)
                } else if let Some(tool_call_id) = msg.tool_call_id {
                    UnifiedMessageContent::ToolResult(UnifiedToolResult {
                        tool_call_id,
                        name: msg.name.unwrap_or_default(),
                        content: msg.content.and_then(|c| c.as_str().map(String::from)).unwrap_or_default(),
                    })
                } else {
                    let text = msg.content.and_then(|c| c.as_str().map(String::from)).unwrap_or_default();
                    UnifiedMessageContent::Text(text)
                };

                UnifiedMessage { role, content, thinking_content: msg.reasoning_content }
            })
            .collect();

        let stop = openai_req.stop.map(|v| {
            if let Some(s) = v.as_str() {
                vec![s.to_string()]
            } else if let Some(arr) = v.as_array() {
                arr.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            } else {
                vec![]
            }
        }).filter(|v| !v.is_empty());

        UnifiedRequest {
            model: openai_req.model,
            messages,
            tools: openai_req.tools,
            stream: openai_req.stream.unwrap_or(false),
            temperature: openai_req.temperature,
            max_tokens: openai_req.max_tokens,
            top_p: openai_req.top_p,
            stop,
            seed: openai_req.seed,
            presence_penalty: openai_req.presence_penalty,
            frequency_penalty: openai_req.frequency_penalty,
        }
    }
}

impl From<UnifiedRequest> for OpenAiRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let messages = unified_req
            .messages
            .into_iter()
            .map(|msg| {
                let role = match msg.role {
                    UnifiedRole::System => "system",
                    UnifiedRole::User => "user",
                    UnifiedRole::Assistant => "assistant",
                    UnifiedRole::Tool => "tool",
                }
                .to_string();

                let (content, tool_calls, name, tool_call_id) = match msg.content {
                    UnifiedMessageContent::Text(text) => (Some(Value::String(text)), None, None, None),
                    UnifiedMessageContent::ToolCalls(calls) => {
                        let tool_calls = calls
                            .into_iter()
                            .map(|call| OpenAiToolCall {
                                id: call.id,
                                type_: "function".to_string(),
                                function: OpenAiFunction {
                                    name: call.name,
                                    arguments: call.arguments.to_string(),
                                },
                            })
                            .collect();
                        (Some(Value::Null), Some(tool_calls), None, None)
                    }
                    UnifiedMessageContent::ToolResult(result) => (
                        Some(Value::String(result.content)),
                        None,
                        Some(result.name),
                        Some(result.tool_call_id),
                    ),
                };

                OpenAiMessage {
                    role,
                    content,
                    tool_calls,
                    name,
                    tool_call_id,
                    reasoning_content: msg.thinking_content,
                }
            })
            .collect();

        let stop = unified_req.stop.map(|v| {
            if v.len() == 1 {
                Value::String(v.into_iter().next().unwrap())
            } else {
                Value::Array(v.into_iter().map(Value::String).collect())
            }
        });

        OpenAiRequestPayload {
            model: unified_req.model,
            messages,
            tools: unified_req.tools,
            stream: Some(unified_req.stream),
            temperature: unified_req.temperature,
            max_tokens: unified_req.max_tokens,
            top_p: unified_req.top_p,
            stop,
            seed: unified_req.seed,
            presence_penalty: unified_req.presence_penalty,
            frequency_penalty: unified_req.frequency_penalty,
        }
    }
}

// --- OpenAI Response to Unified ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiResponse {
    id: String,
    model: String,
    choices: Vec<OpenAiChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<UnifiedUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChoice {
    index: u32,
    message: OpenAiMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
}

impl From<OpenAiResponse> for UnifiedResponse {
    fn from(openai_res: OpenAiResponse) -> Self {
        let choices = openai_res
            .choices
            .into_iter()
            .map(|choice| {
                let role = match choice.message.role.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    "tool" => UnifiedRole::Tool,
                    _ => UnifiedRole::Assistant, // Default to assistant for response messages
                };

                let content = if let Some(tool_calls) = choice.message.tool_calls {
                    let calls = tool_calls
                        .into_iter()
                        .map(|tc| {
                            let args: Value = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(json!({}));
                            UnifiedToolCall {
                                id: tc.id,
                                name: tc.function.name,
                                arguments: args,
                            }
                        })
                        .collect();
                    UnifiedMessageContent::ToolCalls(calls)
                } else if let Some(tool_call_id) = choice.message.tool_call_id {
                    UnifiedMessageContent::ToolResult(UnifiedToolResult {
                        tool_call_id,
                        name: choice.message.name.unwrap_or_default(),
                        content: choice.message.content.and_then(|c| c.as_str().map(String::from)).unwrap_or_default(),
                    })
                } else {
                    let text = choice.message.content.and_then(|c| c.as_str().map(String::from)).unwrap_or_default();
                    UnifiedMessageContent::Text(text)
                };

                let message = UnifiedMessage { role, content, thinking_content: choice.message.reasoning_content };

                UnifiedChoice {
                    index: choice.index,
                    message,
                    finish_reason: choice.finish_reason,
                }
            })
            .collect();

        UnifiedResponse {
            id: openai_res.id,
            model: openai_res.model,
            choices,
            usage: openai_res.usage,
            created: openai_res.created,
            object: openai_res.object,
        }
    }
}

impl From<UnifiedResponse> for OpenAiResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let choices = unified_res
            .choices
            .into_iter()
            .map(|choice| {
                let role = match choice.message.role {
                    UnifiedRole::System => "system",
                    UnifiedRole::User => "user",
                    UnifiedRole::Assistant => "assistant",
                    UnifiedRole::Tool => "tool",
                }
                .to_string();

                let (content, tool_calls, name, tool_call_id) = match choice.message.content {
                    UnifiedMessageContent::Text(text) => (Some(Value::String(text)), None, None, None),
                    UnifiedMessageContent::ToolCalls(calls) => {
                        let tool_calls = calls
                            .into_iter()
                            .map(|call| OpenAiToolCall {
                                id: call.id,
                                type_: "function".to_string(),
                                function: OpenAiFunction {
                                    name: call.name,
                                    arguments: call.arguments.to_string(),
                                },
                            })
                            .collect();
                        (Some(Value::Null), Some(tool_calls), None, None)
                    }
                    UnifiedMessageContent::ToolResult(result) => (
                        Some(Value::String(result.content)),
                        None,
                        Some(result.name),
                        Some(result.tool_call_id),
                    ),
                };

                let message = OpenAiMessage {
                    role,
                    content,
                    tool_calls,
                    name,
                    tool_call_id,
                    reasoning_content: choice.message.thinking_content,
                };

                OpenAiChoice {
                    index: choice.index,
                    message,
                    finish_reason: choice.finish_reason,
                }
            })
            .collect();

        OpenAiResponse {
            id: unified_res.id,
            model: unified_res.model,
            choices,
            usage: unified_res.usage,
            created: unified_res.created,
            object: unified_res.object,
        }
    }
}

// --- OpenAI Response Chunk ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChunkResponse {
    id: String,
    model: String,
    choices: Vec<OpenAiChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<UnifiedUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChunkChoice {
    index: u32,
    delta: OpenAiChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiChunkToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpenAiChunkToolCall {
    index: u32, // OpenAI includes index in chunk tool calls
    id: String,
    #[serde(rename = "type")]
    type_: String,
    function: OpenAiChunkFunction,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpenAiChunkFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<String>,
}

impl From<UnifiedChunkResponse> for OpenAiChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let choices = unified_chunk
            .choices
            .into_iter()
            .map(|choice| {
                let role = choice.delta.role.map(|r| {
                    match r {
                        UnifiedRole::System => "system",
                        UnifiedRole::User => "user",
                        UnifiedRole::Assistant => "assistant",
                        UnifiedRole::Tool => "tool",
                    }
                    .to_string()
                });

                let tool_calls = choice.delta.tool_calls.map(|tcs| {
                    tcs.into_iter()
                        .enumerate() // OpenAI chunk tool calls have an index
                        .map(|(i, tc)| OpenAiChunkToolCall {
                            index: i as u32,
                            id: tc.id,
                            type_: "function".to_string(),
                            function: OpenAiChunkFunction {
                                name: Some(tc.name),
                                arguments: Some(tc.arguments.to_string()),
                            },
                        })
                        .collect()
                });

                let delta = OpenAiChunkDelta {
                    role,
                    content: choice.delta.content,
                    tool_calls,
                    reasoning_content: choice.delta.thinking_content,
                };

                OpenAiChunkChoice {
                    index: choice.index,
                    delta,
                    finish_reason: choice.finish_reason,
                }
            })
            .collect();

        OpenAiChunkResponse {
            id: unified_chunk.id,
            model: unified_chunk.model,
            choices,
            usage: unified_chunk.usage,
            created: unified_chunk.created,
            object: unified_chunk.object,
        }
    }
}

impl From<OpenAiChunkResponse> for UnifiedChunkResponse {
    fn from(openai_chunk: OpenAiChunkResponse) -> Self {
        let choices = openai_chunk
            .choices
            .into_iter()
            .map(|choice| {
                let role = choice.delta.role.map(|r| match r.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    "tool" => UnifiedRole::Tool,
                    _ => UnifiedRole::User,
                });

                let tool_calls = choice.delta.tool_calls.map(|tcs| {
                    tcs.into_iter()
                        .filter_map(|tc| {
                            if let (Some(name), Some(arguments)) = (tc.function.name, tc.function.arguments) {
                                Some(UnifiedToolCall {
                                    id: tc.id,
                                    name,
                                    arguments: serde_json::from_str(&arguments).unwrap_or(json!({})),
                                })
                            } else {
                                None
                            }
                        })
                        .collect()
                });

                let delta = UnifiedMessageDelta {
                    role,
                    content: choice.delta.content,
                    tool_calls,
                    thinking_content: choice.delta.reasoning_content,
                };

                UnifiedChunkChoice {
                    index: choice.index,
                    delta,
                    finish_reason: choice.finish_reason,
                }
            })
            .collect();

        UnifiedChunkResponse {
            id: openai_chunk.id,
            model: openai_chunk.model,
            choices,
            usage: openai_chunk.usage,
            created: openai_chunk.created,
            object: openai_chunk.object,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_request_to_unified() {
        let openai_req = OpenAiRequestPayload {
            model: Some("gpt-4".to_string()),
            messages: vec![
                OpenAiMessage {
                    role: "system".to_string(),
                    content: Some(Value::String("You are a helpful assistant.".to_string())),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    reasoning_content: None,
                },
                OpenAiMessage {
                    role: "user".to_string(),
                    content: Some(Value::String("Hello".to_string())),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    reasoning_content: None,
                },
            ],
            tools: None,
            stream: Some(false),
            temperature: Some(0.8),
            max_tokens: Some(100),
            top_p: Some(0.9),
            stop: Some(Value::String("stop".to_string())),
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
        };

        let unified_req: UnifiedRequest = openai_req.into();

        assert_eq!(unified_req.messages.len(), 2);
        assert_eq!(unified_req.messages[0].role, UnifiedRole::System);
        assert_eq!(
            unified_req.messages[0].content,
            UnifiedMessageContent::Text("You are a helpful assistant.".to_string())
        );
        assert_eq!(unified_req.messages[1].role, UnifiedRole::User);
        assert_eq!(
            unified_req.messages[1].content,
            UnifiedMessageContent::Text("Hello".to_string())
        );
        assert_eq!(unified_req.temperature, Some(0.8));
        assert_eq!(unified_req.max_tokens, Some(100));
        assert_eq!(unified_req.top_p, Some(0.9));
        assert_eq!(unified_req.stop, Some(vec!["stop".to_string()]));
    }

    #[test]
    fn test_unified_request_to_openai() {
        let unified_req = UnifiedRequest {
            model: Some("gpt-4".to_string()),
            messages: vec![
                UnifiedMessage {
                    role: UnifiedRole::System,
                    content: UnifiedMessageContent::Text("You are a helpful assistant.".to_string()),
                    thinking_content: None,
                },
                UnifiedMessage {
                    role: UnifiedRole::User,
                    content: UnifiedMessageContent::Text("Hello".to_string()),
                    thinking_content: None,
                },
            ],
            tools: None,
            stream: false,
            temperature: Some(0.8),
            max_tokens: Some(100),
            top_p: Some(0.9),
            stop: Some(vec!["stop".to_string()]),
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
        };

        let openai_req: OpenAiRequestPayload = unified_req.into();

        assert_eq!(openai_req.messages.len(), 2);
        assert_eq!(openai_req.messages[0].role, "system");
        assert_eq!(
            openai_req.messages[0].content,
            Some(Value::String("You are a helpful assistant.".to_string()))
        );
        assert_eq!(openai_req.messages[1].role, "user");
        assert_eq!(
            openai_req.messages[1].content,
            Some(Value::String("Hello".to_string()))
        );
        assert_eq!(openai_req.temperature, Some(0.8));
        assert_eq!(openai_req.max_tokens, Some(100));
        assert_eq!(openai_req.top_p, Some(0.9));
        assert_eq!(openai_req.stop, Some(Value::String("stop".to_string())));
    }

    #[test]
    fn test_openai_response_to_unified() {
        let openai_res = OpenAiResponse {
            id: "chatcmpl-123".to_string(),
            model: "gpt-4".to_string(),
            choices: vec![OpenAiChoice {
                index: 0,
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some(Value::String("Hi there!".to_string())),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    reasoning_content: None,
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

        let unified_res: UnifiedResponse = openai_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(
            choice.message.content,
            UnifiedMessageContent::Text("Hi there!".to_string())
        );
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
        assert!(unified_res.usage.is_some());
        let usage = unified_res.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_unified_response_to_openai() {
        let unified_res = UnifiedResponse {
            id: "chatcmpl-123".to_string(),
            model: "gpt-4".to_string(),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: UnifiedMessageContent::Text("Hi there!".to_string()),
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

        let openai_res: OpenAiResponse = unified_res.into();

        assert_eq!(openai_res.choices.len(), 1);
        let choice = &openai_res.choices[0];
        assert_eq!(choice.message.role, "assistant");
        assert_eq!(
            choice.message.content,
            Some(Value::String("Hi there!".to_string()))
        );
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
        assert!(openai_res.usage.is_some());
        let usage = openai_res.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_openai_chunk_to_unified() {
        let openai_chunk = OpenAiChunkResponse {
            id: "chatcmpl-123".to_string(),
            model: "gpt-4".to_string(),
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: Some("assistant".to_string()),
                    content: Some("Hello".to_string()),
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: None,
            }],
            usage: None,
            created: Some(12345),
            object: Some("chat.completion.chunk".to_string()),
        };

        let unified_chunk: UnifiedChunkResponse = openai_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
        assert_eq!(choice.delta.content, Some("Hello".to_string()));
        assert!(choice.finish_reason.is_none());
    }

    #[test]
    fn test_unified_chunk_to_openai() {
        let unified_chunk = UnifiedChunkResponse {
            id: "chatcmpl-123".to_string(),
            model: "gpt-4".to_string(),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: Some("Hello".to_string()),
                    tool_calls: None,
                    thinking_content: None,
                },
                finish_reason: None,
            }],
            usage: None,
            created: Some(12345),
            object: Some("chat.completion.chunk".to_string()),
        };

        let openai_chunk: OpenAiChunkResponse = unified_chunk.into();

        assert_eq!(openai_chunk.choices.len(), 1);
        let choice = &openai_chunk.choices[0];
        assert_eq!(choice.delta.role, Some("assistant".to_string()));
        assert_eq!(choice.delta.content, Some("Hello".to_string()));
        assert!(choice.finish_reason.is_none());
    }
}
