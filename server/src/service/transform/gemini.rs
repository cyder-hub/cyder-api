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
    #[serde(rename = "safetySettings")]
    #[serde(skip_serializing_if = "Option::is_none")]
    safety_settings: Option<Vec<GeminiSafetySetting>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum GeminiSystemInstruction {
    String(String),
    Object {
        parts: Vec<GeminiPart>,
    },
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
    ExecutableCode {
        #[serde(rename = "executableCode")]
        executable_code: GeminiExecutableCode,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: GeminiInlineData,
    },
    FileData {
        #[serde(rename = "fileData")]
        file_data: GeminiFileData,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiExecutableCode {
    language: String,
    code: String,
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
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiInlineData {
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiFileData {
    mime_type: String,
    file_uri: String,
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiSafetySetting {
    category: String,
    threshold: String,
}

impl From<GeminiRequestPayload> for UnifiedRequest {
    fn from(gemini_req: GeminiRequestPayload) -> Self {
        let mut messages = Vec::new();
        let mut tool_call_ids: std::collections::HashMap<String, std::collections::VecDeque<String>> =
            std::collections::HashMap::new();

        if let Some(system_instruction) = gemini_req.system_instruction {
            let content = match system_instruction {
                GeminiSystemInstruction::String(text) => text,
                GeminiSystemInstruction::Object { parts } => {
                    parts
                        .into_iter()
                        .filter_map(|p| match p {
                            GeminiPart::Text { text } => Some(text),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            };
            if !content.is_empty() {
                messages.push(UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![UnifiedContentPart::Text { text: content }],
                });
            }
        }

        for content_item in gemini_req.contents {
            let role = content_item.role.as_deref().unwrap_or("user");
            let parts = content_item.parts;

            let has_function_call = parts.iter().any(|p| matches!(p, GeminiPart::FunctionCall { .. }));
            let has_function_response = parts.iter().any(|p| matches!(p, GeminiPart::FunctionResponse { .. }));

            if role == "model" && has_function_call {
                let mut content_parts = Vec::new();
                for p in parts {
                    match p {
                        GeminiPart::FunctionCall { function_call } => {
                            let tool_id = format!("call_{}", ID_GENERATOR.generate_id());
                            tool_call_ids
                                .entry(function_call.name.clone())
                                .or_default()
                                .push_back(tool_id.clone());
                            content_parts.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                                id: tool_id,
                                name: function_call.name,
                                arguments: function_call.args,
                            }));
                        },
                        GeminiPart::Text { text } => {
                             content_parts.push(UnifiedContentPart::Text { text });
                        },
                        _ => {}
                    }
                }
                messages.push(UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: content_parts,
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
                            content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                                tool_call_id,
                                name: fr.name.clone(),
                                content,
                            })],
                        });
                    });
            } else {
                let unified_role = if role == "model" {
                    UnifiedRole::Assistant
                } else {
                    UnifiedRole::User
                };
                
                let mut content_parts = Vec::new();
                for p in parts {
                    match p {
                        GeminiPart::Text { text } => {
                            if !text.is_empty() {
                                content_parts.push(UnifiedContentPart::Text { text });
                            }
                        }
                        GeminiPart::InlineData { inline_data } => {
                            content_parts.push(UnifiedContentPart::ImageData {
                                mime_type: inline_data.mime_type,
                                data: inline_data.data,
                            });
                        }
                        GeminiPart::FileData { file_data } => {
                            content_parts.push(UnifiedContentPart::FileData {
                                file_uri: file_data.file_uri,
                                mime_type: file_data.mime_type,
                            });
                        }
                        _ => {}
                    }
                }

                if !content_parts.is_empty() {
                    messages.push(UnifiedMessage {
                        role: unified_role,
                        content: content_parts,
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
            ..Default::default()
        }
        .filter_empty() // Filter out empty content and messages
    }
}

impl From<UnifiedRequest> for GeminiRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let mut contents = Vec::new();
        let mut system_instruction: Option<GeminiSystemInstruction> = None;

        for msg in unified_req.messages {
            match msg.role {
                UnifiedRole::System => {
                    let system_texts: Vec<String> = msg.content
                        .iter()
                        .filter_map(|part| match part {
                            UnifiedContentPart::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect();
                    
                    if !system_texts.is_empty() {
                        // Use object format with parts to match expected test format
                        let parts: Vec<GeminiPart> = system_texts
                            .into_iter()
                            .map(|text| GeminiPart::Text { text })
                            .collect();
                        system_instruction = Some(GeminiSystemInstruction::Object { parts });
                    }
                }
                UnifiedRole::User => {
                    let mut parts = Vec::new();
                    for part in msg.content {
                        match part {
                            UnifiedContentPart::Text { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::ImageUrl { url, .. } => {
                                // Gemini doesn't support image URLs directly, would need to fetch and convert
                                // For now, skip this or could add a comment in text
                                parts.push(GeminiPart::Text { 
                                    text: format!("[Image: {}]", url) 
                                });
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData {
                                        mime_type,
                                        data,
                                    },
                                });
                            }
                            UnifiedContentPart::FileData { file_uri, mime_type } => {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type,
                                        file_uri,
                                    },
                                });
                            }
                            _ => {}
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(GeminiRequestContent {
                            role: Some("user".to_string()),
                            parts,
                        });
                    }
                }
                UnifiedRole::Assistant => {
                    let mut parts = Vec::new();
                    for part in msg.content {
                        match part {
                            UnifiedContentPart::Text { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData {
                                        mime_type,
                                        data,
                                    },
                                });
                            }
                            UnifiedContentPart::FileData { file_uri, mime_type } => {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type,
                                        file_uri,
                                    },
                                });
                            }
                            UnifiedContentPart::ToolCall(call) => {
                                parts.push(GeminiPart::FunctionCall {
                                    function_call: GeminiFunctionCall {
                                        name: call.name,
                                        args: call.arguments,
                                    },
                                });
                            }
                            _ => {}
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(GeminiRequestContent {
                            role: Some("model".to_string()),
                            parts,
                        });
                    }
                }
                UnifiedRole::Tool => {
                    let mut parts = Vec::new();
                    for part in msg.content {
                        if let UnifiedContentPart::ToolResult(result) = part {
                            let response_content = serde_json::from_str(&result.content)
                                .unwrap_or_else(|_| json!({ "result": result.content }));

                            parts.push(GeminiPart::FunctionResponse {
                                function_response: GeminiFunctionResponse {
                                    name: result.name,
                                    response: response_content,
                                },
                            });
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(GeminiRequestContent {
                            role: Some("user".to_string()), // Gemini expects tool responses under 'user' role
                            parts,
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
            safety_settings: None,
        }
    }
}

// --- Gemini Response Chunk ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiChunkResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "promptFeedback")]
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_feedback: Option<GeminiPromptFeedback>,
    #[serde(rename = "usageMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_metadata: Option<GeminiChunkUsageMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiCandidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<GeminiResponseContent>,
    #[serde(rename = "finishReason")]
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
    #[serde(rename = "safetyRatings")]
    #[serde(skip_serializing_if = "Option::is_none")]
    safety_ratings: Option<Vec<GeminiSafetyRating>>,
    #[serde(rename = "tokenCount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    token_count: Option<u32>,
    #[serde(rename = "citationMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    citation_metadata: Option<GeminiCitationMetadata>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Modality {
    ModalityUnspecified,
    Text,
    Image,
    Video,
    Audio,
    Document,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct ModalityTokenCount {
    modality: Modality,
    token_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    prompt_token_count: u32,
    candidates_token_count: u32,
    total_token_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    thoughts_token_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cached_content_token_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_use_prompt_token_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    prompt_tokens_details: Vec<ModalityTokenCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cache_tokens_details: Vec<ModalityTokenCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    candidates_tokens_details: Vec<ModalityTokenCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_use_prompt_tokens_details: Vec<ModalityTokenCount>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiSafetyRating {
    category: String,
    probability: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiCitationMetadata {
    citation_sources: Vec<GeminiCitationSource>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiCitationSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    start_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiPromptFeedback {
    #[serde(skip_serializing_if = "Option::is_none")]
    block_reason: Option<String>,
    safety_ratings: Vec<GeminiSafetyRating>,
}

// --- Gemini Response to Unified ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "promptFeedback")]
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_feedback: Option<GeminiPromptFeedback>,
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
                let mut content_parts = Vec::new();
                let mut role = UnifiedRole::Assistant;
                let mut has_function_call = false;

                if let Some(content) = candidate.content {
                    role = match content.role.as_str() {
                        "model" => UnifiedRole::Assistant,
                        "user" => UnifiedRole::User, // Should not happen in a response choice
                        _ => UnifiedRole::Assistant,
                    };

                    for p in content.parts {
                        match p {
                            GeminiPart::Text { text } => {
                                content_parts.push(UnifiedContentPart::Text { text });
                            }
                            GeminiPart::InlineData { inline_data } => {
                                content_parts.push(UnifiedContentPart::ImageData {
                                    mime_type: inline_data.mime_type,
                                    data: inline_data.data,
                                });
                            }
                            GeminiPart::FileData { file_data } => {
                                content_parts.push(UnifiedContentPart::FileData {
                                    file_uri: file_data.file_uri,
                                    mime_type: file_data.mime_type,
                                });
                            }
                            GeminiPart::ExecutableCode { executable_code } => {
                                has_function_call = true; // Treat as a tool call
                                content_parts.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                                    id: format!("call_{}", ID_GENERATOR.generate_id()),
                                    name: "code_interpreter".to_string(),
                                    arguments: json!({
                                        "language": executable_code.language,
                                        "code": executable_code.code,
                                    }),
                                }));
                            }
                            GeminiPart::FunctionCall { function_call } => {
                                has_function_call = true;
                                content_parts.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                                    // Gemini responses don't provide a tool call ID, so we generate one.
                                    id: format!("call_{}", ID_GENERATOR.generate_id()),
                                    name: function_call.name,
                                    arguments: function_call.args,
                                }));
                            }
                            _ => {}
                        }
                    }
                }

                let message = UnifiedMessage {
                    role,
                    content: content_parts,
                };

                let finish_reason = candidate.finish_reason.map(|fr| {
                    crate::service::transform::unified::map_gemini_finish_reason_to_openai(&fr, has_function_call)
                });

                UnifiedChoice {
                    index: candidate.index.unwrap_or(0),
                    message,
                    finish_reason,
                    logprobs: None,
                }
            })
            .collect();

        let usage = gemini_res.usage_metadata.map(|u| {
            let mut usage = UnifiedUsage {
                input_tokens: u.prompt_token_count,
                output_tokens: u.candidates_token_count,
                total_tokens: u.total_token_count,
                reasoning_tokens: u.thoughts_token_count,
                cached_tokens: u.cached_content_token_count,
                ..Default::default()
            };

            // Handle image tokens from details
            let input_image_tokens = u
                .prompt_tokens_details
                .iter()
                .find(|d| d.modality == Modality::Image)
                .map(|d| d.token_count);
            if input_image_tokens.is_some() {
                usage.input_image_tokens = input_image_tokens;
            }

            let output_image_tokens = u
                .candidates_tokens_details
                .iter()
                .find(|d| d.modality == Modality::Image)
                .map(|d| d.token_count);
            if output_image_tokens.is_some() {
                usage.output_image_tokens = output_image_tokens;
            }

            usage
        });

        UnifiedResponse {
            // Gemini responses don't have these top-level fields, so we create them.
            id: format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
            model: "gemini-transformed-model".to_string(), // Placeholder
            choices,
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
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
                for part in choice.message.content {
                    match part {
                        UnifiedContentPart::Text { text } => {
                            parts.push(GeminiPart::Text { text });
                        }
                        UnifiedContentPart::ImageData { mime_type, data } => {
                            parts.push(GeminiPart::InlineData {
                                inline_data: GeminiInlineData {
                                    mime_type,
                                    data,
                                },
                            });
                        }
                        UnifiedContentPart::FileData { file_uri, mime_type } => {
                            parts.push(GeminiPart::FileData {
                                file_data: GeminiFileData {
                                    mime_type,
                                    file_uri,
                                },
                            });
                        }
                        UnifiedContentPart::ToolCall(call) => {
                            parts.push(GeminiPart::FunctionCall {
                                function_call: GeminiFunctionCall {
                                    name: call.name,
                                    args: call.arguments,
                                },
                            });
                        }
                        _ => {}
                    }
                }

                let content = if parts.is_empty() {
                    None
                } else {
                    Some(GeminiResponseContent { role, parts })
                };

                let finish_reason = choice.finish_reason.map(|fr| {
                    crate::service::transform::unified::map_openai_finish_reason_to_gemini(&fr)
                });

                // Add placeholder safety ratings as they are expected by some clients
                // Note: Actual safety ratings from Gemini→Unified conversion are not preserved
                // as they don't map to OpenAI-compatible format. These are synthetic placeholders.
                // TODO: Consider storing actual ratings in a Gemini-specific metadata field if needed.
                let safety_ratings = if finish_reason.is_some() {
                    Some(vec![
                        GeminiSafetyRating {
                            category: "HARM_CATEGORY_SEXUALLY_EXPLICIT".to_string(),
                            probability: "NEGLIGIBLE".to_string(),
                        },
                        GeminiSafetyRating {
                            category: "HARM_CATEGORY_HATE_SPEECH".to_string(),
                            probability: "NEGLIGIBLE".to_string(),
                        },
                        GeminiSafetyRating {
                            category: "HARM_CATEGORY_HARASSMENT".to_string(),
                            probability: "NEGLIGIBLE".to_string(),
                        },
                        GeminiSafetyRating {
                            category: "HARM_CATEGORY_DANGEROUS_CONTENT".to_string(),
                            probability: "NEGLIGIBLE".to_string(),
                        },
                    ])
                } else {
                    None
                };

                if content.is_some() || finish_reason.is_some() {
                    Some(GeminiCandidate {
                        index: Some(choice.index),
                        content,
                        finish_reason,
                        safety_ratings,
                        token_count: None,
                        citation_metadata: None,
                    })
                } else {
                    None
                }
            })
            .collect();

        let usage_metadata = unified_res.usage.map(|u| {
            let mut prompt_tokens_details = vec![];
            let text_prompt_tokens = u.input_tokens.saturating_sub(u.input_image_tokens.unwrap_or(0));
            if text_prompt_tokens > 0 {
                prompt_tokens_details.push(ModalityTokenCount {
                    modality: Modality::Text,
                    token_count: text_prompt_tokens,
                });
            }
            if let Some(token_count) = u.input_image_tokens {
                if token_count > 0 {
                    prompt_tokens_details.push(ModalityTokenCount {
                        modality: Modality::Image,
                        token_count,
                    });
                }
            }

            let mut candidates_tokens_details = vec![];
            let text_candidates_tokens = u.output_tokens.saturating_sub(u.output_image_tokens.unwrap_or(0));
            if text_candidates_tokens > 0 {
                candidates_tokens_details.push(ModalityTokenCount {
                    modality: Modality::Text,
                    token_count: text_candidates_tokens,
                });
            }
            if let Some(token_count) = u.output_image_tokens {
                if token_count > 0 {
                    candidates_tokens_details.push(ModalityTokenCount {
                        modality: Modality::Image,
                        token_count,
                    });
                }
            }

            GeminiUsageMetadata {
                prompt_token_count: u.input_tokens,
                candidates_token_count: u.output_tokens,
                total_token_count: u.total_tokens,
                thoughts_token_count: u.reasoning_tokens,
                cached_content_token_count: u.cached_tokens,
                tool_use_prompt_token_count: None,
                prompt_tokens_details,
                candidates_tokens_details,
                cache_tokens_details: vec![],
                tool_use_prompt_tokens_details: vec![],
            }
        });

        GeminiResponse {
            candidates,
            prompt_feedback: None,
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

                for part in choice.delta.content {
                    match part {
                        UnifiedContentPartDelta::TextDelta { text, .. } => {
                            parts.push(GeminiPart::Text { text });
                        },
                        UnifiedContentPartDelta::ImageDelta { .. } => {
                            // Image content not fully supported in Gemini chunk conversion yet
                        }
                        UnifiedContentPartDelta::ToolCallDelta(tc) => {
                            // Gemini doesn't stream partial tool calls in the same way,
                            // but we can try to construct a FunctionCall if we have enough info.
                            // For now, we might need to accumulate or simplify.
                            // Assuming we get a complete call or handle it simplified:
                            if let (Some(name), Some(args_str)) = (tc.name, tc.arguments) {
                                if let Ok(args) = serde_json::from_str(&args_str) {
                                     parts.push(GeminiPart::FunctionCall {
                                        function_call: GeminiFunctionCall {
                                            name,
                                            args,
                                        },
                                    });
                                }
                            }
                        }
                    }
                }

                let content = if !parts.is_empty() {
                    Some(GeminiResponseContent { role, parts })
                } else {
                    None
                };

                let finish_reason = choice.finish_reason.as_ref().map(|fr| {
                    // Note: Gemini doesn't have a direct "tool_calls" finish reason, 
                    // so we map it to "STOP" which is semantically closest
                    crate::service::transform::unified::map_openai_finish_reason_to_gemini(fr)
                });

                let safety_ratings = if choice.finish_reason.is_some() {
                    Some(vec![
                        GeminiSafetyRating {
                            category: "HARM_CATEGORY_SEXUALLY_EXPLICIT".to_string(),
                            probability: "NEGLIGIBLE".to_string(),
                        },
                        GeminiSafetyRating {
                            category: "HARM_CATEGORY_HATE_SPEECH".to_string(),
                            probability: "NEGLIGIBLE".to_string(),
                        },
                        GeminiSafetyRating {
                            category: "HARM_CATEGORY_HARASSMENT".to_string(),
                            probability: "NEGLIGIBLE".to_string(),
                        },
                        GeminiSafetyRating {
                            category: "HARM_CATEGORY_DANGEROUS_CONTENT".to_string(),
                            probability: "NEGLIGIBLE".to_string(),
                        },
                    ])
                } else {
                    None
                };

                if content.is_some() || finish_reason.is_some() {
                    Some(GeminiCandidate {
                        index: Some(choice.index),
                        content,
                        finish_reason,
                        safety_ratings,
                        token_count: None,
                        citation_metadata: None,
                    })
                } else {
                    None
                }
            })
            .collect();

        let usage_metadata = unified_chunk.usage.map(|u| GeminiChunkUsageMetadata {
            prompt_token_count: u.input_tokens,
            candidates_token_count: Some(u.output_tokens),
            total_token_count: u.total_tokens,
        });

        GeminiChunkResponse {
            candidates,
            prompt_feedback: None,
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

                    // Track indices separately for different content types
                    let mut text_index = 0;
                    let mut tool_call_index = 0;
                    let mut image_index = 0;

                    for part in content.parts {
                        match part {
                            GeminiPart::Text { text } => {
                                delta.content.push(UnifiedContentPartDelta::TextDelta { 
                                    index: text_index, 
                                    text 
                                });
                                text_index += 1;
                            }
                            GeminiPart::InlineData { inline_data } => {
                                delta.content.push(UnifiedContentPartDelta::ImageDelta {
                                    index: image_index,
                                    url: None,
                                    data: Some(inline_data.data),
                                });
                                image_index += 1;
                            }
                            GeminiPart::FileData { .. } => {
                                // File data doesn't map well to delta, skip for now
                            }
                            GeminiPart::ExecutableCode { executable_code } => {
                                has_function_call = true;
                                delta.content.push(UnifiedContentPartDelta::ToolCallDelta(UnifiedToolCallDelta {
                                    index: tool_call_index,
                                    id: Some(format!("call_{}", ID_GENERATOR.generate_id())),
                                    name: Some("code_interpreter".to_string()),
                                    arguments: Some(
                                        json!({
                                            "language": executable_code.language,
                                            "code": executable_code.code,
                                        })
                                        .to_string(),
                                    ),
                                }));
                                tool_call_index += 1;
                            }
                            GeminiPart::FunctionCall { function_call } => {
                                has_function_call = true;
                                delta.content.push(UnifiedContentPartDelta::ToolCallDelta(UnifiedToolCallDelta {
                                    index: tool_call_index,
                                    id: Some(format!("call_{}", ID_GENERATOR.generate_id())),
                                    name: Some(function_call.name),
                                    arguments: Some(function_call.args.to_string()),
                                }));
                                tool_call_index += 1;
                            }
                            _ => {}
                        }
                    }
                }

                let finish_reason = candidate.finish_reason.map(|fr| {
                    crate::service::transform::unified::map_gemini_finish_reason_to_openai(&fr, has_function_call)
                });

                UnifiedChunkChoice {
                    index: candidate.index.unwrap_or(0),
                    delta,
                    finish_reason,
                }
            })
            .collect();

        let usage = gemini_chunk.usage_metadata.map(|u| UnifiedUsage {
            input_tokens: u.prompt_token_count,
            output_tokens: u.candidates_token_count.unwrap_or(0),
            total_tokens: u.total_token_count,
            ..Default::default()
        });

        UnifiedChunkResponse {
            // Gemini chunks don't have these top-level fields, so we create them.
            // Note: The actual ID will be set by StreamTransformer to ensure consistency
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
            system_instruction: Some(GeminiSystemInstruction::String(
                "You are a helpful assistant.".to_string(),
            )),
            tools: None,
            generation_config: Some(GeminiGenerationConfig {
                temperature: Some(0.8),
                max_output_tokens: Some(100),
                top_p: Some(0.9),
                stop_sequences: Some(vec!["stop".to_string()]),
            }),
            safety_settings: None,
        };

        let unified_req: UnifiedRequest = gemini_req.into();

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
    fn test_unified_request_to_gemini() {
        let unified_req = UnifiedRequest {
            model: Some("test-model".to_string()),
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

        let gemini_req: GeminiRequestPayload = unified_req.into();

        assert!(gemini_req.system_instruction.is_some());
        let system_instruction = gemini_req.system_instruction.unwrap();
        match system_instruction {
            GeminiSystemInstruction::String(text) => {
                assert_eq!(text, "You are a helpful assistant.");
            }
            GeminiSystemInstruction::Object { parts } => {
                assert_eq!(parts.len(), 1);
                if let GeminiPart::Text { text } = &parts[0] {
                    assert_eq!(text, "You are a helpful assistant.");
                } else {
                    panic!("Expected text part in system instruction");
                }
            }
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
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::Text {
                        text: "Hi there!".to_string(),
                    }],
                }),
                finish_reason: Some("STOP".to_string()),
                safety_ratings: None,
                token_count: Some(20),
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 20,
                total_token_count: 30,
                thoughts_token_count: None,
                cached_content_token_count: None,
            }),
        };

        let unified_res: UnifiedResponse = gemini_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(
            choice.message.content,
            vec![UnifiedContentPart::Text { text: "Hi there!".to_string() }]
        );
        assert_eq!(choice.finish_reason, Some("stop".to_string()));

        assert!(unified_res.usage.is_some());
        let usage = unified_res.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
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
                    content: vec![UnifiedContentPart::Text { text: "Hi there!".to_string() }],
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
            created: Some(1234567890),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
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
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::Text {
                        text: "Hello".to_string(),
                    }],
                }),
                finish_reason: None,
                safety_ratings: None,
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };

        let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
        assert_eq!(choice.delta.content, vec![UnifiedContentPartDelta::TextDelta { index: 0, text: "Hello".to_string() }]);
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
                    content: vec![UnifiedContentPartDelta::TextDelta { index: 0, text: "Hello".to_string() }],
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
                index: Some(0),
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
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };

        let unified_res: UnifiedResponse = gemini_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));
        
        match &choice.message.content[0] {
            UnifiedContentPart::Text { text } => assert_eq!(text, "I should call a tool"),
            _ => panic!("Expected text content"),
        }
        match &choice.message.content[1] {
            UnifiedContentPart::ToolCall(tc) => {
                assert_eq!(tc.name, "get_weather");
            },
            _ => panic!("Expected tool call content"),
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
                    content: vec![
                        UnifiedContentPart::Text { text: "I will call a tool".to_string() },
                        UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: "call_123".to_string(),
                            name: "get_weather".to_string(),
                            arguments: json!({"location": "Boston"}),
                        }),
                    ],
                },
                finish_reason: Some("tool_calls".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: None,
            object: None,
            system_fingerprint: None,
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
                index: Some(0),
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
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };

        let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
        
        match &choice.delta.content[0] {
            UnifiedContentPartDelta::TextDelta { text, .. } => assert_eq!(text, "Thinking..."),
            _ => panic!("Expected text delta"),
        }
        
        match &choice.delta.content[1] {
            UnifiedContentPartDelta::ToolCallDelta(tc) => {
                assert_eq!(tc.name, Some("search".to_string()));
            },
            _ => panic!("Expected tool call delta"),
        }
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
                    content: vec![
                        UnifiedContentPartDelta::TextDelta { index: 0, text: "Thinking...".to_string() },
                        UnifiedContentPartDelta::ToolCallDelta(UnifiedToolCallDelta {
                            index: 0,
                            id: Some("call_123".to_string()),
                            name: Some("search".to_string()),
                            arguments: Some(json!({"query": "stuff"}).to_string()),
                        }),
                    ],
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

    #[test]
    fn test_gemini_response_to_unified_with_executable_code() {
        let gemini_res = GeminiResponse {
            candidates: vec![GeminiCandidate {
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::ExecutableCode {
                        executable_code: GeminiExecutableCode {
                            language: "PYTHON".to_string(),
                            code: "print('Hello World')".to_string(),
                        },
                    }],
                }),
                finish_reason: Some("TOOL_USE".to_string()),
                safety_ratings: None,
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };

        let unified_res: UnifiedResponse = gemini_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));

        match &choice.message.content[0] {
            UnifiedContentPart::ToolCall(tc) => {
                assert_eq!(tc.name, "code_interpreter");
                assert_eq!(
                    tc.arguments,
                    json!({"language": "PYTHON", "code": "print('Hello World')"})
                );
            }
            _ => panic!("Expected tool call content"),
        }
    }

    #[test]
    fn test_gemini_chunk_to_unified_with_executable_code() {
        let gemini_chunk = GeminiChunkResponse {
            candidates: vec![GeminiCandidate {
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::ExecutableCode {
                        executable_code: GeminiExecutableCode {
                            language: "PYTHON".to_string(),
                            code: "print('Hello')".to_string(),
                        },
                    }],
                }),
                finish_reason: None,
                safety_ratings: None,
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };

        let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));

        match &choice.delta.content[0] {
            UnifiedContentPartDelta::ToolCallDelta(tc) => {
                assert_eq!(tc.name, Some("code_interpreter".to_string()));
                assert_eq!(
                    tc.arguments,
                    Some(json!({"language": "PYTHON", "code": "print('Hello')"}).to_string())
                );
            }
            _ => panic!("Expected tool call delta"),
        }
    }
}
