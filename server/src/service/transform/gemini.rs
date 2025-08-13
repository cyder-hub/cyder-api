use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::unified::*;
use crate::utils::ID_GENERATOR;

// --- Gemini to Unified ---

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiRequestPayload {
    contents: Vec<GeminiRequestContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTools>>,
    #[serde(rename = "generationConfig")]
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiRequestContent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiResponseContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum GeminiPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiFunctionCall {
    name: String,
    args: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiFunctionResponse {
    name: String,
    response: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiTools {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<UnifiedFunctionDefinition>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(rename = "maxOutputTokens")]
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(rename = "topP")]
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(rename = "stopSequences")]
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
}

impl From<GeminiRequestPayload> for UnifiedRequest {
    fn from(gemini_req: GeminiRequestPayload) -> Self {
        let mut messages = Vec::new();
        let mut tool_call_ids: std::collections::HashMap<String, std::collections::VecDeque<String>> =
            std::collections::HashMap::new();

        if let Some(system_instruction) = gemini_req.system_instruction {
            let content = system_instruction
                .parts
                .into_iter()
                .filter_map(|p| match p {
                    GeminiPart::Text { text } => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !content.is_empty() {
                messages.push(UnifiedMessage {
                    role: UnifiedRole::System,
                    content: UnifiedMessageContent::Text(content),
                    thinking_content: None,
                });
            }
        }

        for content_item in gemini_req.contents {
            let role = content_item.role.as_deref().unwrap_or("user");
            let parts = content_item.parts;

            let has_function_call = parts.iter().any(|p| matches!(p, GeminiPart::FunctionCall { .. }));
            let has_function_response = parts.iter().any(|p| matches!(p, GeminiPart::FunctionResponse { .. }));

            if role == "model" && has_function_call {
                let tool_calls = parts
                    .into_iter()
                    .filter_map(|p| match p {
                        GeminiPart::FunctionCall { function_call } => Some(function_call),
                        _ => None,
                    })
                    .map(|fc| {
                        let tool_id = format!("call_{}", ID_GENERATOR.generate_id());
                        tool_call_ids
                            .entry(fc.name.clone())
                            .or_default()
                            .push_back(tool_id.clone());
                        UnifiedToolCall {
                            id: tool_id,
                            name: fc.name,
                            arguments: fc.args,
                        }
                    })
                    .collect();
                messages.push(UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: UnifiedMessageContent::ToolCalls(tool_calls),
                    thinking_content: None,
                });
            } else if role == "user" && has_function_response {
                parts
                    .into_iter()
                    .filter_map(|p| match p {
                        GeminiPart::FunctionResponse { function_response } => Some(function_response),
                        _ => None,
                    })
                    .for_each(|fr| {
                        let tool_call_id = tool_call_ids
                            .get_mut(&fr.name)
                            .and_then(|ids| ids.pop_front())
                            .unwrap_or_else(|| format!("call_{}", ID_GENERATOR.generate_id()));

                        let content = fr.response
                            .get("result")
                            .and_then(|v| v.as_str().map(String::from))
                            .unwrap_or_else(|| serde_json::to_string(&fr.response).unwrap_or_default());

                        messages.push(UnifiedMessage {
                            role: UnifiedRole::Tool,
                            content: UnifiedMessageContent::ToolResult(UnifiedToolResult {
                                tool_call_id,
                                name: fr.name.clone(),
                                content,
                            }),
                            thinking_content: None,
                        });
                    });
            } else {
                let text = parts
                    .into_iter()
                    .filter_map(|p| match p {
                        GeminiPart::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                if !text.is_empty() {
                    let unified_role = if role == "model" {
                        UnifiedRole::Assistant
                    } else {
                        UnifiedRole::User
                    };
                    messages.push(UnifiedMessage {
                        role: unified_role,
                        content: UnifiedMessageContent::Text(text),
                        thinking_content: None,
                    });
                }
            }
        }

        let tools = gemini_req.tools.map(|ts| {
            ts.into_iter()
                .flat_map(|t| t.function_declarations)
                .map(|f| {
                    let mut params = f.parameters;
                    transform_gemini_tool_params_to_openai(&mut params);
                    UnifiedTool {
                        type_: "function".to_string(),
                        function: UnifiedFunctionDefinition {
                            name: f.name,
                            description: f.description,
                            parameters: params,
                        },
                    }
                })
                .collect()
        });

        let (temperature, max_tokens, top_p, stop) =
            if let Some(config) = gemini_req.generation_config {
                (
                    config.temperature,
                    config.max_output_tokens,
                    config.top_p,
                    config.stop_sequences,
                )
            } else {
                (None, None, None, None)
            };

        UnifiedRequest {
            model: None, // Not in Gemini request body
            messages,
            tools,
            stream: false, // Set by `into_unified_request`
            temperature,
            max_tokens,
            top_p,
            stop,
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
        }
    }
}

impl From<UnifiedRequest> for GeminiRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let mut contents = Vec::new();
        let mut system_instruction: Option<GeminiSystemInstruction> = None;

        for msg in unified_req.messages {
            match msg.role {
                UnifiedRole::System => {
                    if let UnifiedMessageContent::Text(text) = msg.content {
                        if let Some(si) = &mut system_instruction {
                            si.parts.push(GeminiPart::Text { text });
                        } else {
                            system_instruction = Some(GeminiSystemInstruction {
                                parts: vec![GeminiPart::Text { text }],
                            });
                        }
                    }
                }
                UnifiedRole::User => {
                    if let UnifiedMessageContent::Text(text) = msg.content {
                        contents.push(GeminiRequestContent {
                            role: Some("user".to_string()),
                            parts: vec![GeminiPart::Text { text }],
                        });
                    }
                }
                UnifiedRole::Assistant => {
                    let parts = match msg.content {
                        UnifiedMessageContent::Text(text) => vec![GeminiPart::Text { text }],
                        UnifiedMessageContent::ToolCalls(calls) => calls
                            .into_iter()
                            .map(|call| GeminiPart::FunctionCall {
                                function_call: GeminiFunctionCall {
                                    name: call.name,
                                    args: call.arguments,
                                },
                            })
                            .collect(),
                        _ => vec![],
                    };
                    if !parts.is_empty() {
                        contents.push(GeminiRequestContent {
                            role: Some("model".to_string()),
                            parts,
                        });
                    }
                }
                UnifiedRole::Tool => {
                    if let UnifiedMessageContent::ToolResult(result) = msg.content {
                        let response_content = serde_json::from_str(&result.content)
                            .unwrap_or_else(|_| json!({ "result": result.content }));

                        let part = GeminiPart::FunctionResponse {
                            function_response: GeminiFunctionResponse {
                                name: result.name,
                                response: response_content,
                            },
                        };
                        contents.push(GeminiRequestContent {
                            role: Some("user".to_string()), // Gemini expects tool responses under 'user' role, which is the default
                            parts: vec![part],
                        });
                    }
                }
            }
        }

        // Gemini has a specific structure for tools.
        let tools = unified_req.tools.map(|tools| {
            let function_declarations = tools.into_iter().map(|tool| tool.function).collect();
            vec![GeminiTools {
                function_declarations,
            }]
        });

        let generation_config = if unified_req.temperature.is_some()
            || unified_req.max_tokens.is_some()
            || unified_req.top_p.is_some()
            || unified_req.stop.is_some()
        {
            Some(GeminiGenerationConfig {
                temperature: unified_req.temperature,
                max_output_tokens: unified_req.max_tokens,
                top_p: unified_req.top_p,
                stop_sequences: unified_req.stop,
            })
        } else {
            None
        };

        GeminiRequestPayload {
            contents,
            system_instruction,
            tools,
            generation_config,
        }
    }
}

// --- Gemini Response Chunk ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiChunkResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_metadata: Option<GeminiChunkUsageMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiCandidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<GeminiResponseContent>,
    #[serde(rename = "finishReason")]
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
    #[serde(rename = "safetyRatings")]
    #[serde(skip_serializing_if = "Option::is_none")]
    safety_ratings: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: u32,
    #[serde(rename = "totalTokenCount")]
    total_token_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiChunkUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    candidates_token_count: Option<u32>,
    #[serde(rename = "totalTokenCount")]
    total_token_count: u32,
}

// --- Gemini Response to Unified ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

impl From<GeminiResponse> for UnifiedResponse {
    fn from(gemini_res: GeminiResponse) -> Self {
        let choices = gemini_res
            .candidates
            .into_iter()
            .map(|candidate| {
                let mut message_content = UnifiedMessageContent::Text("".to_string());
                let mut role = UnifiedRole::Assistant;
                let mut has_function_call = false;

                let mut thinking_content = None;
                if let Some(content) = candidate.content {
                    role = match content.role.as_str() {
                        "model" => UnifiedRole::Assistant,
                        "user" => UnifiedRole::User, // Should not happen in a response choice
                        _ => UnifiedRole::Assistant,
                    };

                    let text_content = content
                        .parts
                        .iter()
                        .filter_map(|p| match p {
                            GeminiPart::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    let tool_calls = content
                        .parts
                        .into_iter()
                        .filter_map(|p| match p {
                            GeminiPart::FunctionCall { function_call } => {
                                has_function_call = true;
                                Some(UnifiedToolCall {
                                    // Gemini responses don't provide a tool call ID, so we generate one.
                                    id: format!("call_{}", ID_GENERATOR.generate_id()),
                                    name: function_call.name,
                                    arguments: function_call.args,
                                })
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();

                    if !tool_calls.is_empty() {
                        message_content = UnifiedMessageContent::ToolCalls(tool_calls);
                        if !text_content.is_empty() {
                            thinking_content = Some(text_content);
                        }
                    } else if !text_content.is_empty() {
                        message_content = UnifiedMessageContent::Text(text_content);
                    }
                }

                let message = UnifiedMessage {
                    role,
                    content: message_content,
                    thinking_content,
                };

                let finish_reason = candidate.finish_reason.map(|fr| {
                    match fr.as_str() {
                        "STOP" => {
                            if has_function_call {
                                "tool_calls"
                            } else {
                                "stop"
                            }
                        }
                        "TOOL_USE" => "tool_calls",
                        "MAX_TOKENS" => "length",
                        "SAFETY" | "RECITATION" => "content_filter",
                        _ => "stop",
                    }
                    .to_string()
                });

                UnifiedChoice {
                    index: 0,
                    message,
                    finish_reason,
                }
            })
            .collect();

        let usage = gemini_res.usage_metadata.map(|u| UnifiedUsage {
            prompt_tokens: u.prompt_token_count,
            completion_tokens: u.candidates_token_count,
            total_tokens: u.total_token_count,
        });

        UnifiedResponse {
            // Gemini responses don't have these top-level fields, so we create them.
            id: format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
            model: "gemini-transformed-model".to_string(), // Placeholder
            choices,
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
        }
    }
}

impl From<UnifiedResponse> for GeminiResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let candidates = unified_res
            .choices
            .into_iter()
            .filter_map(|choice| {
                let role = match choice.message.role {
                    UnifiedRole::Assistant => "model",
                    _ => "user", // Gemini response content role is either 'model' or 'user'
                }
                .to_string();

                let mut parts = Vec::new();
                if let Some(thinking) = choice.message.thinking_content {
                    if !thinking.is_empty() {
                        parts.push(GeminiPart::Text { text: thinking });
                    }
                }

                match choice.message.content {
                    UnifiedMessageContent::Text(text) => {
                        parts.push(GeminiPart::Text { text });
                    }
                    UnifiedMessageContent::ToolCalls(calls) => {
                        parts.extend(calls.into_iter().map(|call| GeminiPart::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: call.name,
                                args: call.arguments,
                            },
                        }));
                    }
                    // Gemini response doesn't have ToolResult in its candidate content
                    UnifiedMessageContent::ToolResult(_) => {}
                };

                let content = if parts.is_empty() {
                    None
                } else {
                    Some(GeminiResponseContent { role, parts })
                };

                let finish_reason = choice.finish_reason.map(|fr| {
                    match fr.as_str() {
                        "stop" => "STOP",
                        "length" => "MAX_TOKENS",
                        "content_filter" => "SAFETY",
                        "tool_calls" => "TOOL_USE",
                        _ => "FINISH_REASON_UNSPECIFIED",
                    }
                    .to_string()
                });

                // Add placeholder safety ratings as they are expected by some clients
                let safety_ratings = if finish_reason.is_some() {
                    Some(json!([
                        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE" },
                        { "category": "HARM_CATEGORY_HATE_SPEECH", "probability": "NEGLIGIBLE" },
                        { "category": "HARM_CATEGORY_HARASSMENT", "probability": "NEGLIGIBLE" },
                        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "probability": "NEGLIGIBLE" }
                    ]))
                } else {
                    None
                };

                if content.is_some() || finish_reason.is_some() {
                    Some(GeminiCandidate {
                        content,
                        finish_reason,
                        safety_ratings,
                    })
                } else {
                    None
                }
            })
            .collect();

        let usage_metadata = unified_res.usage.map(|u| GeminiUsageMetadata {
            prompt_token_count: u.prompt_tokens,
            candidates_token_count: u.completion_tokens,
            total_token_count: u.total_tokens,
        });

        GeminiResponse {
            candidates,
            usage_metadata,
        }
    }
}

impl From<UnifiedChunkResponse> for GeminiChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let candidates = unified_chunk
            .choices
            .into_iter()
            .filter_map(|choice| {
                let mut parts = Vec::new();
                let mut role = "model".to_string(); // Default role for Gemini assistant messages

                if let Some(r) = choice.delta.role {
                    role = match r {
                        UnifiedRole::Assistant => "model".to_string(),
                        UnifiedRole::User => "user".to_string(),
                        // System and Tool roles don't map directly to Gemini chunk roles,
                        // so we'll default to model.
                        _ => "model".to_string(),
                    };
                }

                if let Some(thinking) = choice.delta.thinking_content {
                    if !thinking.is_empty() {
                        parts.push(GeminiPart::Text { text: thinking });
                    }
                }

                if let Some(content) = choice.delta.content {
                    if !content.is_empty() {
                        parts.push(GeminiPart::Text { text: content });
                    }
                }

                if let Some(tool_calls) = choice.delta.tool_calls {
                    for tc in tool_calls {
                        parts.push(GeminiPart::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: tc.name,
                                args: tc.arguments,
                            },
                        });
                    }
                }

                let content = if !parts.is_empty() {
                    Some(GeminiResponseContent { role, parts })
                } else {
                    None
                };

                let finish_reason = choice.finish_reason.as_ref().map(|fr| {
                    match fr.as_str() {
                        "stop" => "STOP",
                        "length" => "MAX_TOKENS",
                        "content_filter" => "SAFETY",
                        "tool_calls" => "STOP", // Gemini doesn't have a direct tool_calls reason, STOP is used.
                        _ => "FINISH_REASON_UNSPECIFIED",
                    }
                    .to_string()
                });

                let safety_ratings = if choice.finish_reason.is_some() {
                    Some(json!([
                        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE" },
                        { "category": "HARM_CATEGORY_HATE_SPEECH", "probability": "NEGLIGIBLE" },
                        { "category": "HARM_CATEGORY_HARASSMENT", "probability": "NEGLIGIBLE" },
                        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "probability": "NEGLIGIBLE" }
                    ]))
                } else {
                    None
                };

                if content.is_some() || finish_reason.is_some() {
                    Some(GeminiCandidate {
                        content,
                        finish_reason,
                        safety_ratings,
                    })
                } else {
                    None
                }
            })
            .collect();

        let usage_metadata = unified_chunk.usage.map(|u| GeminiChunkUsageMetadata {
            prompt_token_count: u.prompt_tokens,
            candidates_token_count: Some(u.completion_tokens),
            total_token_count: u.total_tokens,
        });

        GeminiChunkResponse {
            candidates,
            usage_metadata,
        }
    }
}

impl From<GeminiChunkResponse> for UnifiedChunkResponse {
    fn from(gemini_chunk: GeminiChunkResponse) -> Self {
        let choices = gemini_chunk
            .candidates
            .into_iter()
            .map(|candidate| {
                let mut delta = UnifiedMessageDelta::default();
                let mut has_function_call = false;

                if let Some(content) = candidate.content {
                    delta.role = Some(match content.role.as_str() {
                        "model" => UnifiedRole::Assistant,
                        "user" => UnifiedRole::User,
                        _ => UnifiedRole::User,
                    });

                    let text_content = content
                        .parts
                        .iter()
                        .filter_map(|p| match p {
                            GeminiPart::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    let tool_calls = content
                        .parts
                        .into_iter()
                        .filter_map(|p| match p {
                            GeminiPart::FunctionCall { function_call } => {
                                has_function_call = true;
                                Some(UnifiedToolCall {
                                    // Gemini chunks don't provide a tool call ID, so we generate one.
                                    id: format!("call_{}", ID_GENERATOR.generate_id()),
                                    name: function_call.name,
                                    arguments: function_call.args,
                                })
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();

                    if !tool_calls.is_empty() {
                        delta.tool_calls = Some(tool_calls);
                        if !text_content.is_empty() {
                            delta.thinking_content = Some(text_content);
                        }
                    } else if !text_content.is_empty() {
                        delta.content = Some(text_content);
                    }
                }

                let finish_reason = candidate.finish_reason.map(|fr| {
                    match fr.as_str() {
                        "STOP" => {
                            if has_function_call {
                                "tool_calls"
                            } else {
                                "stop"
                            }
                        }
                        "TOOL_USE" => "tool_calls",
                        "MAX_TOKENS" => "length",
                        "SAFETY" | "RECITATION" => "content_filter",
                        _ => "stop",
                    }
                    .to_string()
                });

                UnifiedChunkChoice {
                    index: 0,
                    delta,
                    finish_reason,
                }
            })
            .collect();

        let usage = gemini_chunk.usage_metadata.map(|u| UnifiedUsage {
            prompt_tokens: u.prompt_token_count,
            completion_tokens: u.candidates_token_count.unwrap_or(0),
            total_tokens: u.total_token_count,
        });

        UnifiedChunkResponse {
            // Gemini chunks don't have these top-level fields, so we create them.
            id: format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
            model: "gemini-transformed-model".to_string(),
            choices,
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
        }
    }
}

// Helper to recursively transform Gemini tool parameter types to lowercase for OpenAI.
pub(super) fn transform_gemini_tool_params_to_openai(params: &mut Value) {
    if let Some(obj) = params.as_object_mut() {
        // Transform "type" field
        if let Some(type_val) = obj.get_mut("type") {
            if let Some(type_str) = type_val.as_str() {
                *type_val = json!(type_str.to_lowercase());
            }
        }
        // Recurse into "properties"
        if let Some(properties) = obj.get_mut("properties") {
            if let Some(props_obj) = properties.as_object_mut() {
                for (_, prop_val) in props_obj.iter_mut() {
                    transform_gemini_tool_params_to_openai(prop_val);
                }
            }
        }
        // Recurse into "items" for arrays
        if let Some(items) = obj.get_mut("items") {
            transform_gemini_tool_params_to_openai(items);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_request_to_unified() {
        let gemini_req = GeminiRequestPayload {
            contents: vec![GeminiRequestContent {
                role: Some("user".to_string()),
                parts: vec![GeminiPart::Text {
                    text: "Hello".to_string(),
                }],
            }],
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiPart::Text {
                    text: "You are a helpful assistant.".to_string(),
                }],
            }),
            tools: None,
            generation_config: Some(GeminiGenerationConfig {
                temperature: Some(0.8),
                max_output_tokens: Some(100),
                top_p: Some(0.9),
                stop_sequences: Some(vec!["stop".to_string()]),
            }),
        };

        let unified_req: UnifiedRequest = gemini_req.into();

        assert_eq!(unified_req.messages.len(), 2);
        assert_eq!(unified_req.messages[0].role, UnifiedRole::System);
        assert_eq!(
            unified_req.messages[0].content,
            UnifiedMessageContent::Text("You are a helpful assistant.".to_string())
        );
        assert!(unified_req.messages[0].thinking_content.is_none());
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
    fn test_unified_request_to_gemini() {
        let unified_req = UnifiedRequest {
            model: Some("test-model".to_string()),
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

        let gemini_req: GeminiRequestPayload = unified_req.into();

        assert!(gemini_req.system_instruction.is_some());
        let system_instruction = gemini_req.system_instruction.unwrap();
        assert_eq!(system_instruction.parts.len(), 1);
        if let GeminiPart::Text { text } = &system_instruction.parts[0] {
            assert_eq!(text, "You are a helpful assistant.");
        } else {
            panic!("Expected text part in system instruction");
        }

        assert_eq!(gemini_req.contents.len(), 1);
        assert_eq!(gemini_req.contents[0].role, Some("user".to_string()));
        assert_eq!(gemini_req.contents[0].parts.len(), 1);
        if let GeminiPart::Text { text } = &gemini_req.contents[0].parts[0] {
            assert_eq!(text, "Hello");
        } else {
            panic!("Expected text part in user content");
        }

        assert!(gemini_req.generation_config.is_some());
        let config = gemini_req.generation_config.unwrap();
        assert_eq!(config.temperature, Some(0.8));
        assert_eq!(config.max_output_tokens, Some(100));
        assert_eq!(config.top_p, Some(0.9));
        assert_eq!(config.stop_sequences, Some(vec!["stop".to_string()]));
    }

    #[test]
    fn test_gemini_response_to_unified() {
        let gemini_res = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::Text {
                        text: "Hi there!".to_string(),
                    }],
                }),
                finish_reason: Some("STOP".to_string()),
                safety_ratings: None,
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 20,
                total_token_count: 30,
            }),
        };

        let unified_res: UnifiedResponse = gemini_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(
            choice.message.content,
            UnifiedMessageContent::Text("Hi there!".to_string())
        );
        assert!(choice.message.thinking_content.is_none());
        assert_eq!(choice.finish_reason, Some("stop".to_string()));

        assert!(unified_res.usage.is_some());
        let usage = unified_res.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_unified_response_to_gemini() {
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
            created: Some(1234567890),
            object: Some("chat.completion".to_string()),
        };

        let gemini_res: GeminiResponse = unified_res.into();

        assert_eq!(gemini_res.candidates.len(), 1);
        let candidate = &gemini_res.candidates[0];
        assert!(candidate.content.is_some());
        let content = candidate.content.as_ref().unwrap();
        assert_eq!(content.role, "model");
        assert_eq!(content.parts.len(), 1);
        if let GeminiPart::Text { text } = &content.parts[0] {
            assert_eq!(text, "Hi there!");
        } else {
            panic!("Expected text part");
        }
        assert_eq!(candidate.finish_reason, Some("STOP".to_string()));

        assert!(gemini_res.usage_metadata.is_some());
        let usage = gemini_res.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, 10);
        assert_eq!(usage.candidates_token_count, 20);
        assert_eq!(usage.total_token_count, 30);
    }

    #[test]
    fn test_gemini_chunk_to_unified() {
        let gemini_chunk = GeminiChunkResponse {
            candidates: vec![GeminiCandidate {
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::Text {
                        text: "Hello".to_string(),
                    }],
                }),
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        };

        let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
        assert_eq!(choice.delta.content, Some("Hello".to_string()));
        assert!(choice.delta.thinking_content.is_none());
        assert!(choice.finish_reason.is_none());
    }

    #[test]
    fn test_unified_chunk_to_gemini() {
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
            created: Some(1234567890),
            object: Some("chat.completion.chunk".to_string()),
        };

        let gemini_chunk: GeminiChunkResponse = unified_chunk.into();

        assert_eq!(gemini_chunk.candidates.len(), 1);
        let candidate = &gemini_chunk.candidates[0];
        assert!(candidate.content.is_some());
        let content = candidate.content.as_ref().unwrap();
        assert_eq!(content.role, "model");
        assert_eq!(content.parts.len(), 1);
        if let GeminiPart::Text { text } = &content.parts[0] {
            assert_eq!(text, "Hello");
        } else {
            panic!("Expected text part");
        }
        assert!(candidate.finish_reason.is_none());
    }

    #[test]
    fn test_gemini_response_to_unified_with_thinking() {
        let gemini_res = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![
                        GeminiPart::Text {
                            text: "I should call a tool".to_string(),
                        },
                        GeminiPart::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: "get_weather".to_string(),
                                args: json!({"location": "Boston"}),
                            },
                        },
                    ],
                }),
                finish_reason: Some("TOOL_USE".to_string()),
                safety_ratings: None,
            }],
            usage_metadata: None,
        };

        let unified_res: UnifiedResponse = gemini_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(
            choice.message.thinking_content,
            Some("I should call a tool".to_string())
        );
        assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));
        match &choice.message.content {
            UnifiedMessageContent::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "get_weather");
            }
            _ => panic!("Expected ToolCalls content"),
        }
    }

    #[test]
    fn test_unified_response_to_gemini_with_thinking() {
        let unified_res = UnifiedResponse {
            id: "chatcmpl-123".to_string(),
            model: "gpt-4".to_string(),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: UnifiedMessageContent::ToolCalls(vec![UnifiedToolCall {
                        id: "call_123".to_string(),
                        name: "get_weather".to_string(),
                        arguments: json!({"location": "Boston"}),
                    }]),
                    thinking_content: Some("I will call a tool".to_string()),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: None,
            created: None,
            object: None,
        };

        let gemini_res: GeminiResponse = unified_res.into();

        assert_eq!(gemini_res.candidates.len(), 1);
        let candidate = &gemini_res.candidates[0];
        assert!(candidate.content.is_some());
        let content = candidate.content.as_ref().unwrap();
        assert_eq!(content.role, "model");
        assert_eq!(content.parts.len(), 2);
        assert!(
            matches!(&content.parts[0], GeminiPart::Text { text } if text == "I will call a tool")
        );
        assert!(
            matches!(&content.parts[1], GeminiPart::FunctionCall { function_call } if function_call.name == "get_weather")
        );
        assert_eq!(candidate.finish_reason, Some("TOOL_USE".to_string()));
    }

    #[test]
    fn test_gemini_chunk_to_unified_with_thinking() {
        let gemini_chunk = GeminiChunkResponse {
            candidates: vec![GeminiCandidate {
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![
                        GeminiPart::Text {
                            text: "Thinking...".to_string(),
                        },
                        GeminiPart::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: "search".to_string(),
                                args: json!({"query": "stuff"}),
                            },
                        },
                    ],
                }),
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        };

        let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
        assert_eq!(
            choice.delta.thinking_content,
            Some("Thinking...".to_string())
        );
        assert!(choice.delta.content.is_none());
        assert!(choice.delta.tool_calls.is_some());
        let tool_calls = choice.delta.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "search");
    }

    #[test]
    fn test_unified_chunk_to_gemini_with_thinking() {
        let unified_chunk = UnifiedChunkResponse {
            id: "chatcmpl-123".to_string(),
            model: "gpt-4".to_string(),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: None,
                    tool_calls: Some(vec![UnifiedToolCall {
                        id: "call_123".to_string(),
                        name: "search".to_string(),
                        arguments: json!({"query": "stuff"}),
                    }]),
                    thinking_content: Some("Thinking...".to_string()),
                },
                finish_reason: None,
            }],
            usage: None,
            created: Some(1234567890),
            object: Some("chat.completion.chunk".to_string()),
        };

        let gemini_chunk: GeminiChunkResponse = unified_chunk.into();

        assert_eq!(gemini_chunk.candidates.len(), 1);
        let candidate = &gemini_chunk.candidates[0];
        assert!(candidate.content.is_some());
        let content = candidate.content.as_ref().unwrap();
        assert_eq!(content.role, "model");
        assert_eq!(content.parts.len(), 2);
        assert!(matches!(&content.parts[0], GeminiPart::Text { text } if text == "Thinking..."));
        assert!(
            matches!(&content.parts[1], GeminiPart::FunctionCall { function_call } if function_call.name == "search")
        );
    }
}

