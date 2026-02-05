use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::unified::*;

// --- OpenAI to Unified ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiRequestPayload {
    #[serde(skip_serializing_if = "String::is_empty")]
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<UnifiedTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<OpenAiStop>,
    #[serde(skip_serializing_if = "Option::is_none")]
    n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logit_bias: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_logprobs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) enum ReasoningEffort {
    #[serde(rename = "none")]
    _None,
    #[serde(rename = "minimal")]
    Minimal,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "xhigh")]
    Xhigh,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) enum OpenAiAudioFormat {
    #[serde(rename = "wav")]
    Wav,
    #[serde(rename = "mp3")]
    Mp3,
    #[serde(rename = "flac")]
    Flac,
    #[serde(rename = "opus")]
    Opus,
    #[serde(rename = "pcm16")]
    Pcm16,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiAudio {
    format: OpenAiAudioFormat,
    voice: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub(super) enum OpenAiStop {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiMessage {
    role: String,
    content: Option<OpenAiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refusal: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub(super) enum OpenAiContent {
    Text(String),
    Parts(Vec<OpenAiContentPart>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum OpenAiContentPart {
    Text { text: String },
    ImageUrl { image_url: OpenAiImageUrl },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiImageUrl {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    type_: String, // "function"
    function: OpenAiFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiLogProbs {
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Vec<OpenAiLogProb>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiLogProb {
    token: String,
    logprob: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_logprobs: Option<Vec<OpenAiTopLogProb>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiTopLogProb {
    token: String,
    logprob: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<Vec<u8>>,
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

                let mut content = Vec::new();

                if let Some(c) = msg.content {
                    match c {
                        OpenAiContent::Text(text) => {
                            content.push(UnifiedContentPart::Text { text });
                        }
                        OpenAiContent::Parts(parts) => {
                            for part in parts {
                                match part {
                                    OpenAiContentPart::Text { text } => {
                                        content.push(UnifiedContentPart::Text { text });
                                    }
                                    OpenAiContentPart::ImageUrl { image_url } => {
                                        content.push(UnifiedContentPart::ImageUrl {
                                            url: image_url.url,
                                            detail: image_url.detail,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        let args: Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(json!({}));
                        content.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: tc.id,
                            name: tc.function.name,
                            arguments: args,
                        }));
                    }
                }

                if let Some(tool_call_id) = msg.tool_call_id {
                    // If content was present, use it as the result content, otherwise empty string
                    let result_content = content
                        .iter()
                        .find_map(|p| match p {
                            UnifiedContentPart::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();

                    // Clear previous text content as it's now part of the tool result
                    content.retain(|p| !matches!(p, UnifiedContentPart::Text { .. }));

                    content.push(UnifiedContentPart::ToolResult(UnifiedToolResult {
                        tool_call_id,
                        name: msg.name.unwrap_or_default(),
                        content: result_content,
                    }));
                }

                UnifiedMessage { role, content }
            })
            .collect();

        let stop = openai_req.stop.map(|v| match v {
            OpenAiStop::String(s) => vec![s],
            OpenAiStop::Array(arr) => arr,
        });

        // Store OpenAI-specific fields that don't have unified equivalents in passthrough
        let mut passthrough_fields = serde_json::Map::new();
        if let Some(logprobs) = openai_req.logprobs {
            passthrough_fields.insert("logprobs".to_string(), json!(logprobs));
        }
        if let Some(top_logprobs) = openai_req.top_logprobs {
            passthrough_fields.insert("top_logprobs".to_string(), json!(top_logprobs));
        }
        if let Some(parallel_tool_calls) = openai_req.parallel_tool_calls {
            passthrough_fields.insert("parallel_tool_calls".to_string(), json!(parallel_tool_calls));
        }
        if let Some(reasoning_effort) = openai_req.reasoning_effort {
            passthrough_fields.insert("reasoning_effort".to_string(), json!(reasoning_effort));
        }
        
        let passthrough = if passthrough_fields.is_empty() {
            None
        } else {
            Some(Value::Object(passthrough_fields))
        };

        UnifiedRequest {
            model: Some(openai_req.model),
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
            tool_choice: openai_req.tool_choice,
            n: openai_req.n,
            response_format: openai_req.response_format,
            logit_bias: openai_req.logit_bias,
            user: openai_req.user,
            passthrough,
            ..Default::default()
        }
        .filter_empty() // Filter out empty content and messages
    }
}

impl From<UnifiedRequest> for OpenAiRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let messages = unified_req
            .messages
            .into_iter()
            .flat_map(|msg| {
                let role = match msg.role {
                    UnifiedRole::System => "system",
                    UnifiedRole::User => "user",
                    UnifiedRole::Assistant => "assistant",
                    UnifiedRole::Tool => "tool",
                }
                .to_string();

                // Group content by type to reconstruct OpenAI message structure
                let mut content_parts = Vec::new();
                let mut tool_calls = Vec::new();
                let mut tool_results = Vec::new();
                let mut has_multimodal = false;

                for part in msg.content {
                    match part {
                        UnifiedContentPart::Text { text } => {
                            if has_multimodal {
                                content_parts.push(OpenAiContentPart::Text { text });
                            } else {
                                content_parts.push(OpenAiContentPart::Text { text });
                            }
                        }
                        UnifiedContentPart::ImageUrl { url, detail } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::ImageUrl {
                                image_url: OpenAiImageUrl { url, detail },
                            });
                        }
                        UnifiedContentPart::ImageData { .. } | 
                        UnifiedContentPart::FileData { .. } | 
                        UnifiedContentPart::ExecutableCode { .. } => {
                            // These content types don't map to OpenAI's format, skip them
                        }
                        UnifiedContentPart::ToolCall(call) => tool_calls.push(OpenAiToolCall {
                            id: call.id,
                            type_: "function".to_string(),
                            function: OpenAiFunction {
                                name: call.name,
                                arguments: call.arguments.to_string(),
                            },
                        }),
                        UnifiedContentPart::ToolResult(result) => tool_results.push(result),
                    }
                }

                let content_val = if content_parts.is_empty() {
                    None
                } else if content_parts.len() == 1 && !has_multimodal {
                    // Single text part - use simple string format
                    if let OpenAiContentPart::Text { text } = &content_parts[0] {
                        Some(OpenAiContent::Text(text.clone()))
                    } else {
                        Some(OpenAiContent::Parts(content_parts.clone()))
                    }
                } else {
                    // Multiple parts or has images - use parts format
                    Some(OpenAiContent::Parts(content_parts.clone()))
                };

                // If there are tool results, they must be separate messages in OpenAI
                // We also need to handle mixed content (e.g. Text + ToolResults) by creating separate messages
                let mut generated_messages = Vec::new();

                // 1. If there is text content, create a message for it first
                if let Some(c) = content_val {
                    generated_messages.push(OpenAiMessage {
                        role: role.clone(),
                        content: Some(c),
                        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls.clone()) },
                        name: None,
                        tool_call_id: None,
                        refusal: None,
                    });
                } else if !tool_calls.is_empty() {
                    // Case where there is no text but there are tool calls (Assistant invoking tool)
                    generated_messages.push(OpenAiMessage {
                        role: role.clone(),
                        content: None,
                        tool_calls: Some(tool_calls),
                        name: None,
                        tool_call_id: None,
                        refusal: None,
                    });
                }

                // 2. Add tool results as separate messages with 'tool' role
                for result in tool_results {
                    generated_messages.push(OpenAiMessage {
                        role: "tool".to_string(),
                        content: Some(OpenAiContent::Text(result.content)),
                        tool_calls: None,
                        name: Some(result.name),
                        tool_call_id: Some(result.tool_call_id),
                        refusal: None,
                    });
                }

                generated_messages
            })
            .collect();

        let stop = unified_req.stop.map(|v| {
            if v.len() == 1 {
                OpenAiStop::String(v.into_iter().next().unwrap())
            } else {
                OpenAiStop::Array(v)
            }
        });

        // Extract OpenAI-specific fields from passthrough if present
        let (logprobs, top_logprobs, parallel_tool_calls, reasoning_effort) = 
            if let Some(passthrough) = &unified_req.passthrough {
                (
                    passthrough.get("logprobs").and_then(|v| v.as_bool()),
                    passthrough.get("top_logprobs").and_then(|v| v.as_u64()).map(|v| v as u32),
                    passthrough.get("parallel_tool_calls").and_then(|v| v.as_bool()),
                    passthrough.get("reasoning_effort").and_then(|v| serde_json::from_value(v.clone()).ok()),
                )
            } else {
                (None, None, None, None)
            };

        OpenAiRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            messages,
            tools: unified_req.tools,
            tool_choice: unified_req.tool_choice,
            stream: Some(unified_req.stream),
            temperature: unified_req.temperature,
            max_tokens: unified_req.max_tokens,
            top_p: unified_req.top_p,
            stop,
            n: unified_req.n,
            seed: unified_req.seed,
            presence_penalty: unified_req.presence_penalty,
            frequency_penalty: unified_req.frequency_penalty,
            logit_bias: unified_req.logit_bias,
            logprobs,
            top_logprobs,
            response_format: unified_req.response_format,
            user: unified_req.user,
            parallel_tool_calls,
            reasoning_effort,
        }
    }
}

// --- OpenAI Response to Unified ---

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiCompletionTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiPromptTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cached_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiUsage {
    completion_tokens: u32,
    prompt_tokens: u32,
    total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_tokens_details: Option<OpenAiCompletionTokenDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_tokens_details: Option<OpenAiPromptTokenDetails>,
}

impl From<OpenAiUsage> for UnifiedUsage {
    fn from(openai_usage: OpenAiUsage) -> Self {
        let mut reasoning_tokens = openai_usage
            .completion_tokens_details
            .as_ref()
            .and_then(|d| d.reasoning_tokens);

        if reasoning_tokens.is_none() {
            let calculated_reasoning = openai_usage
                .total_tokens
                .saturating_sub(openai_usage.prompt_tokens)
                .saturating_sub(openai_usage.completion_tokens);
            if calculated_reasoning > 0 {
                reasoning_tokens = Some(calculated_reasoning);
            }
        }

        let cached_tokens = openai_usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens);

        UnifiedUsage {
            input_tokens: openai_usage.prompt_tokens,
            output_tokens: openai_usage.completion_tokens,
            total_tokens: openai_usage.total_tokens,
            reasoning_tokens,
            cached_tokens,
            ..Default::default()
        }
    }
}

impl From<UnifiedUsage> for OpenAiUsage {
    fn from(unified_usage: UnifiedUsage) -> Self {
        let completion_tokens_details = unified_usage.reasoning_tokens.map(|rt| {
            OpenAiCompletionTokenDetails {
                reasoning_tokens: Some(rt),
                audio_tokens: None, // No source for this
            }
        });

        let prompt_tokens_details = unified_usage.cached_tokens.map(|ct| {
            OpenAiPromptTokenDetails {
                cached_tokens: Some(ct),
                audio_tokens: None, // No source for this
            }
        });

        OpenAiUsage {
            prompt_tokens: unified_usage.input_tokens,
            completion_tokens: unified_usage.output_tokens,
            total_tokens: unified_usage.total_tokens,
            completion_tokens_details,
            prompt_tokens_details,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiResponse {
    id: String,
    object: String, // Usually "chat.completion"
    created: i64,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_fingerprint: Option<String>,
    choices: Vec<OpenAiChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChoice {
    index: u32,
    message: OpenAiMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    logprobs: Option<OpenAiLogProbs>,
    finish_reason: Option<String>, // Can be null in some cases (e.g., content filtering)
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

                let mut content = Vec::new();

                if let Some(c) = choice.message.content {
                    match c {
                        OpenAiContent::Text(text) => {
                            content.push(UnifiedContentPart::Text { text });
                        }
                        OpenAiContent::Parts(parts) => {
                            for part in parts {
                                match part {
                                    OpenAiContentPart::Text { text } => {
                                        content.push(UnifiedContentPart::Text { text });
                                    }
                                    OpenAiContentPart::ImageUrl { image_url } => {
                                        content.push(UnifiedContentPart::ImageUrl {
                                            url: image_url.url,
                                            detail: image_url.detail,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(tool_calls) = choice.message.tool_calls {
                    for tc in tool_calls {
                        let args: Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(json!({}));
                        content.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: tc.id,
                            name: tc.function.name,
                            arguments: args,
                        }));
                    }
                }

                if let Some(tool_call_id) = choice.message.tool_call_id {
                    // Extract text content if available to be the result content
                    let result_content = content
                        .iter()
                        .find_map(|p| match p {
                            UnifiedContentPart::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();

                    // Clear text parts as they are consumed
                    content.retain(|p| !matches!(p, UnifiedContentPart::Text { .. }));

                    content.push(UnifiedContentPart::ToolResult(UnifiedToolResult {
                        tool_call_id,
                        name: choice.message.name.unwrap_or_default(),
                        content: result_content,
                    }));
                }

                let message = UnifiedMessage { role, content };

                UnifiedChoice {
                    index: choice.index,
                    message,
                    finish_reason: choice.finish_reason,
                    logprobs: choice
                        .logprobs
                        .map(|lp| serde_json::to_value(lp).unwrap_or(Value::Null)),
                }
            })
            .collect();

        UnifiedResponse {
            id: openai_res.id,
            model: openai_res.model,
            choices,
            usage: openai_res.usage.map(|u| u.into()),
            created: Some(openai_res.created),
            object: Some(openai_res.object),
            system_fingerprint: openai_res.system_fingerprint,
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

                let mut content_parts = Vec::new();
                let mut tool_calls = Vec::new();
                // Note: OpenAI Response doesn't typically have ToolResult in choices, but handling for completeness
                let mut tool_call_id = None;
                let mut name = None;
                let mut has_multimodal = false;

                for part in choice.message.content {
                    match part {
                        UnifiedContentPart::Text { text } => {
                            content_parts.push(OpenAiContentPart::Text { text });
                        }
                        UnifiedContentPart::ImageUrl { url, detail } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::ImageUrl {
                                image_url: OpenAiImageUrl { url, detail },
                            });
                        }
                        UnifiedContentPart::ImageData { .. }
                        | UnifiedContentPart::FileData { .. }
                        | UnifiedContentPart::ExecutableCode { .. } => {
                            // These content types don't map to OpenAI's format, skip them
                        }
                        UnifiedContentPart::ToolCall(call) => tool_calls.push(OpenAiToolCall {
                            id: call.id,
                            type_: "function".to_string(),
                            function: OpenAiFunction {
                                name: call.name,
                                arguments: call.arguments.to_string(),
                            },
                        }),
                        UnifiedContentPart::ToolResult(result) => {
                            // If there's a tool result in the response, we treat it as content
                            // This is rare for a response object.
                            content_parts.push(OpenAiContentPart::Text {
                                text: result.content,
                            });
                            tool_call_id = Some(result.tool_call_id);
                            name = Some(result.name);
                        }
                    }
                }

                let content = if content_parts.is_empty() {
                    None
                } else if content_parts.len() == 1 && !has_multimodal {
                    // Single text part - use simple string format
                    if let OpenAiContentPart::Text { text } = &content_parts[0] {
                        Some(OpenAiContent::Text(text.clone()))
                    } else {
                        Some(OpenAiContent::Parts(content_parts.clone()))
                    }
                } else {
                    // Multiple parts or has images - use parts format
                    Some(OpenAiContent::Parts(content_parts.clone()))
                };

                let message = OpenAiMessage {
                    role,
                    content,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    name,
                    tool_call_id,
                    refusal: None,
                };

                OpenAiChoice {
                    index: choice.index,
                    message,
                    finish_reason: choice.finish_reason,
                    logprobs: choice.logprobs.and_then(|v| serde_json::from_value(v).ok()),
                }
            })
            .collect();

        OpenAiResponse {
            id: unified_res.id,
            object: unified_res
                .object
                .unwrap_or_else(|| "chat.completion".to_string()),
            created: unified_res
                .created
                .unwrap_or_else(|| chrono::Utc::now().timestamp()),
            model: unified_res.model,
            system_fingerprint: unified_res.system_fingerprint,
            choices,
            usage: unified_res.usage.map(|u| u.into()),
        }
    }
}

// --- OpenAI Response Chunk ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChunkResponse {
    id: String,
    object: String, // Usually "chat.completion.chunk"
    created: i64,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_fingerprint: Option<String>,
    choices: Vec<OpenAiChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<OpenAiUsage>, // Usually only present in the last chunk
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChunkChoice {
    index: u32,
    delta: OpenAiChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logprobs: Option<OpenAiLogProbs>,
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
    refusal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>, // For tool messages
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpenAiChunkToolCall {
    index: u32, // OpenAI includes index in chunk tool calls
    id: Option<String>, // ID is optional in chunks
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    type_: Option<String>,
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

                let mut content = String::new();
                let mut tool_calls = Vec::new();

                for part in choice.delta.content {
                    match part {
                        UnifiedContentPartDelta::TextDelta { text, .. } => content.push_str(&text),
                        UnifiedContentPartDelta::ImageDelta { .. } => {
                            // Image content not fully supported in OpenAI chunk conversion yet
                        }
                        UnifiedContentPartDelta::ToolCallDelta(tc) => {
                            tool_calls.push(OpenAiChunkToolCall {
                                index: tc.index,
                                id: tc.id,
                                type_: Some("function".to_string()),
                                function: OpenAiChunkFunction {
                                    name: tc.name,
                                    arguments: tc.arguments,
                                },
                            });
                        }
                    }
                }

                let delta = OpenAiChunkDelta {
                    role,
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content)
                    },
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    refusal: None,
                    name: None,
                };

                OpenAiChunkChoice {
                    index: choice.index,
                    delta,
                    finish_reason: choice.finish_reason,
                    logprobs: None,
                }
            })
            .collect();

        OpenAiChunkResponse {
            id: unified_chunk.id,
            object: unified_chunk
                .object
                .unwrap_or_else(|| "chat.completion.chunk".to_string()),
            created: unified_chunk
                .created
                .unwrap_or_else(|| Utc::now().timestamp()),
            model: unified_chunk.model,
            system_fingerprint: None,
            choices,
            usage: unified_chunk.usage.map(|u| u.into()),
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

                let mut content = Vec::new();

                if let Some(text) = choice.delta.content {
                    if !text.is_empty() {
                        // Index 0 for text content for now
                        content.push(UnifiedContentPartDelta::TextDelta { index: 0, text });
                    }
                }

                if let Some(tool_calls) = choice.delta.tool_calls {
                    for tc in tool_calls {
                        content.push(UnifiedContentPartDelta::ToolCallDelta(
                            UnifiedToolCallDelta {
                                index: tc.index,
                                id: tc.id,
                                name: tc.function.name,
                                arguments: tc.function.arguments,
                            },
                        ));
                    }
                }

                let delta = UnifiedMessageDelta { role, content };

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
            usage: openai_chunk.usage.map(|u| u.into()),
            created: Some(openai_chunk.created),
            object: Some(openai_chunk.object),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_request_to_unified() {
        let openai_req = OpenAiRequestPayload {
            model: "gpt-4".to_string(),
            messages: vec![
                OpenAiMessage {
                    role: "system".to_string(),
                    content: Some(OpenAiContent::Text("You are a helpful assistant.".to_string())),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    refusal: None,
                },
                OpenAiMessage {
                    role: "user".to_string(),
                    content: Some(OpenAiContent::Text("Hello".to_string())),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    refusal: None,
                },
            ],
            tools: None,
            tool_choice: None,
            stream: Some(false),
            temperature: Some(0.8),
            max_tokens: Some(100),
            top_p: Some(0.9),
            stop: Some(OpenAiStop::String("stop".to_string())),
            n: None,
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            response_format: None,
            user: None,
            parallel_tool_calls: None,
            reasoning_effort: None,
        };

        let unified_req: UnifiedRequest = openai_req.into();

        assert_eq!(unified_req.model, Some("gpt-4".to_string()));
        assert_eq!(unified_req.messages.len(), 2);
        assert_eq!(unified_req.messages[0].role, UnifiedRole::System);
        assert_eq!(
            unified_req.messages[0].content,
            vec![UnifiedContentPart::Text { text: "You are a helpful assistant.".to_string() }]
        );
        assert_eq!(unified_req.messages[1].role, UnifiedRole::User);
        assert_eq!(
            unified_req.messages[1].content,
            vec![UnifiedContentPart::Text { text: "Hello".to_string() }]
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
                    content: vec![UnifiedContentPart::Text { text: "You are a helpful assistant.".to_string() }],
                },
                UnifiedMessage {
                    role: UnifiedRole::User,
                    content: vec![UnifiedContentPart::Text { text: "Hello".to_string() }],
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
            ..Default::default()
        };

        let openai_req: OpenAiRequestPayload = unified_req.into();

        assert_eq!(openai_req.model, "gpt-4".to_string());
        assert_eq!(openai_req.messages.len(), 2);
        assert_eq!(openai_req.messages[0].role, "system");
        match openai_req.messages[0].content.as_ref().unwrap() {
            OpenAiContent::Text(t) => assert_eq!(t, "You are a helpful assistant."),
            _ => panic!("Expected text content"),
        }
        assert_eq!(openai_req.messages[1].role, "user");
        match openai_req.messages[1].content.as_ref().unwrap() {
            OpenAiContent::Text(t) => assert_eq!(t, "Hello"),
            _ => panic!("Expected text content"),
        }
        assert_eq!(openai_req.temperature, Some(0.8));
        assert_eq!(openai_req.max_tokens, Some(100));
        assert_eq!(openai_req.top_p, Some(0.9));
        match openai_req.stop.as_ref().unwrap() {
            OpenAiStop::String(s) => assert_eq!(s, "stop"),
            _ => panic!("Expected string stop"),
        }
    }

    #[test]
    fn test_openai_response_to_unified() {
        let openai_res = OpenAiResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion".to_string(),
            created: 12345,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChoice {
                index: 0,
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some(OpenAiContent::Text("Hi there!".to_string())),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    refusal: None,
                },
                logprobs: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
                completion_tokens_details: None,
                prompt_tokens_details: None,
            }),
        };

        let unified_res: UnifiedResponse = openai_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(
            choice.message.content,
            vec![UnifiedContentPart::Text {
                text: "Hi there!".to_string()
            }]
        );
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
        assert!(unified_res.usage.is_some());
        let usage = unified_res.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
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
                    content: vec![UnifiedContentPart::Text {
                        text: "Hi there!".to_string()
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

        let openai_res: OpenAiResponse = unified_res.into();

        assert_eq!(openai_res.choices.len(), 1);
        let choice = &openai_res.choices[0];
        assert_eq!(choice.message.role, "assistant");
        match choice.message.content.as_ref().unwrap() {
            OpenAiContent::Text(t) => assert_eq!(t, "Hi there!"),
            _ => panic!("Expected text content"),
        }
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
            object: "chat.completion.chunk".to_string(),
            created: 12345,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: Some("assistant".to_string()),
                    content: Some("Hello".to_string()),
                    tool_calls: None,
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        };

        let unified_chunk: UnifiedChunkResponse = openai_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
        assert_eq!(choice.delta.content, vec![UnifiedContentPartDelta::TextDelta { index: 0, text: "Hello".to_string() }]);
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
                    content: vec![UnifiedContentPartDelta::TextDelta { index: 0, text: "Hello".to_string() }],
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
