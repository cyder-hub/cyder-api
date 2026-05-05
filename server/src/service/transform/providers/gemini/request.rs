use crate::schema::enum_def::LlmApiType;
use crate::service::transform::unified::*;
use crate::service::transform::{TransformProtocol, TransformValueKind, apply_transform_policy};
use crate::utils::ID_GENERATOR;

use super::metadata::*;
use super::payload::*;

impl From<GeminiRequestPayload> for UnifiedRequest {
    fn from(gemini_req: GeminiRequestPayload) -> Self {
        let mut messages = Vec::new();
        let mut items = Vec::new();
        let mut tool_call_ids: std::collections::HashMap<
            String,
            std::collections::VecDeque<String>,
        > = std::collections::HashMap::new();

        if let Some(system_instruction) = gemini_req.system_instruction {
            let content = match system_instruction {
                GeminiSystemInstruction::String(text) => text,
                GeminiSystemInstruction::Object { parts } => parts
                    .into_iter()
                    .filter_map(|p| match p {
                        GeminiPart::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            if !content.is_empty() {
                let system_message = UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![UnifiedContentPart::Text {
                        text: content.clone(),
                    }],
                };
                items.extend(legacy_content_to_unified_items(
                    UnifiedRole::System,
                    system_message.content.clone(),
                ));
                messages.push(UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![UnifiedContentPart::Text { text: content }],
                });
            }
        }

        for content_item in gemini_req.contents {
            let role = content_item.role.as_deref().unwrap_or("user");
            let parts = content_item.parts;

            let has_function_call = parts
                .iter()
                .any(|p| matches!(p, GeminiPart::FunctionCall { .. }));
            let has_function_response = parts
                .iter()
                .any(|p| matches!(p, GeminiPart::FunctionResponse { .. }));

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
                            items.push(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                                id: tool_id.clone(),
                                name: function_call.name.clone(),
                                arguments: function_call.args.clone(),
                            }));
                            content_parts.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                                id: tool_id,
                                name: function_call.name,
                                arguments: function_call.args,
                            }));
                        }
                        GeminiPart::ExecutableCode { executable_code } => {
                            items.push(UnifiedItem::Message(UnifiedMessageItem {
                                role: UnifiedRole::Assistant,
                                content: vec![UnifiedContentPart::ExecutableCode {
                                    language: executable_code.language.clone(),
                                    code: executable_code.code.clone(),
                                }],
                                annotations: Vec::new(),
                            }));
                            content_parts.push(UnifiedContentPart::ExecutableCode {
                                language: executable_code.language,
                                code: executable_code.code,
                            });
                        }
                        GeminiPart::Text { text } => {
                            content_parts.push(UnifiedContentPart::Text { text });
                        }
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
                        GeminiPart::FunctionResponse { function_response } => {
                            Some(function_response)
                        }
                        _ => None,
                    })
                    .for_each(|fr| {
                        let tool_call_id = tool_call_ids
                            .get_mut(&fr.name)
                            .and_then(|ids| ids.pop_front())
                            .unwrap_or_else(|| format!("call_{}", ID_GENERATOR.generate_id()));
                        let output = gemini_function_response_to_unified_output(fr.response);
                        items.push(UnifiedItem::FunctionCallOutput(
                            UnifiedFunctionCallOutputItem {
                                tool_call_id: tool_call_id.clone(),
                                name: Some(fr.name.clone()),
                                output: output.clone(),
                            },
                        ));

                        messages.push(UnifiedMessage {
                            role: UnifiedRole::Tool,
                            content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                                tool_call_id,
                                name: Some(fr.name.clone()),
                                output,
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
                            content_parts.push(gemini_inline_data_to_unified_content(inline_data));
                        }
                        GeminiPart::FileData { file_data } => {
                            content_parts.push(UnifiedContentPart::FileUrl {
                                url: file_data.file_uri,
                                mime_type: Some(file_data.mime_type),
                                filename: None,
                            });
                        }
                        _ => {}
                    }
                }

                if !content_parts.is_empty() {
                    items.extend(legacy_content_to_unified_items(
                        unified_role.clone(),
                        content_parts.clone(),
                    ));
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
            items,
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
        let tool_name_by_id = build_unified_tool_name_lookup(&unified_req);
        let mut contents = Vec::new();
        let mut system_instruction: Option<GeminiSystemInstruction> = None;

        for msg in unified_req.messages {
            match msg.role {
                UnifiedRole::System => {
                    let system_texts: Vec<String> = msg
                        .content
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
                            UnifiedContentPart::Refusal { text } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::Refusal,
                                    "Downgrading refusal content to plain text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text { text });
                                }
                            }
                            UnifiedContentPart::ImageUrl { url, detail } => {
                                let keep = apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ImageUrl,
                                    "Downgrading remote image URL to recoverable text during Gemini request conversion.",
                                );
                                if keep {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_image_reference_text(
                                            &url,
                                            detail.as_deref(),
                                        ),
                                    });
                                }
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::Reasoning { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::FileUrl { url, mime_type, .. } => {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type: mime_type.unwrap_or_else(|| {
                                            "application/octet-stream".to_string()
                                        }),
                                        file_uri: url,
                                    },
                                });
                            }
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename: _,
                            } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                parts.push(GeminiPart::ExecutableCode {
                                    executable_code: GeminiExecutableCode { language, code },
                                });
                            }
                            UnifiedContentPart::ToolCall(call) => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ToolCall,
                                    "Downgrading user tool call to recoverable text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_tool_call_text(&call),
                                    });
                                }
                            }
                            UnifiedContentPart::ToolResult(result) => {
                                let name = result
                                    .name
                                    .clone()
                                    .or_else(|| tool_name_by_id.get(&result.tool_call_id).cloned())
                                    .unwrap_or_else(|| {
                                        build_gemini_fallback_tool_name(&result.tool_call_id)
                                    });
                                parts.push(GeminiPart::FunctionResponse {
                                    function_response: GeminiFunctionResponse {
                                        name,
                                        response: unified_tool_result_to_gemini_response(
                                            &result.output,
                                        ),
                                    },
                                });
                            }
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
                            UnifiedContentPart::Refusal { text } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::Refusal,
                                    "Downgrading refusal content to plain text during Gemini assistant conversion.",
                                ) {
                                    parts.push(GeminiPart::Text { text });
                                }
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::Reasoning { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::FileUrl {
                                url,
                                mime_type,
                                filename: _,
                            } => {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type: mime_type.unwrap_or_else(|| {
                                            "application/octet-stream".to_string()
                                        }),
                                        file_uri: url,
                                    },
                                });
                            }
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename: _,
                            } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
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
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                parts.push(GeminiPart::ExecutableCode {
                                    executable_code: GeminiExecutableCode { language, code },
                                });
                            }
                            UnifiedContentPart::ImageUrl { url, detail } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ImageUrl,
                                    "Downgrading assistant image URL to recoverable text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_image_reference_text(
                                            &url,
                                            detail.as_deref(),
                                        ),
                                    });
                                }
                            }
                            UnifiedContentPart::ToolResult(result) => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ToolResult,
                                    "Downgrading assistant tool result to recoverable text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_tool_result_text(&result),
                                    });
                                }
                            }
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
                        match part {
                            UnifiedContentPart::ToolResult(result) => {
                                let name = result
                                    .name
                                    .or_else(|| tool_name_by_id.get(&result.tool_call_id).cloned())
                                    .unwrap_or_else(|| {
                                        apply_transform_policy(
                                            TransformProtocol::Unified,
                                            TransformProtocol::Api(LlmApiType::Gemini),
                                            TransformValueKind::ToolResult,
                                            "Gemini tool result is missing tool name; using explicit synthetic fallback name derived from tool_call_id.",
                                        );
                                        build_gemini_fallback_tool_name(&result.tool_call_id)
                                    });
                                let response_content =
                                    unified_tool_result_to_gemini_response(&result.output);

                                parts.push(GeminiPart::FunctionResponse {
                                    function_response: GeminiFunctionResponse {
                                        name,
                                        response: response_content,
                                    },
                                });
                            }
                            UnifiedContentPart::Text { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::Refusal { text } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::Refusal,
                                    "Downgrading refusal content to plain text during Gemini tool conversion.",
                                ) {
                                    parts.push(GeminiPart::Text { text });
                                }
                            }
                            UnifiedContentPart::Reasoning { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::ImageUrl { url, detail } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ImageUrl,
                                    "Downgrading tool message image URL to recoverable text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_image_reference_text(
                                            &url,
                                            detail.as_deref(),
                                        ),
                                    });
                                }
                            }
                            UnifiedContentPart::FileUrl { url, mime_type, .. } => {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type: mime_type.unwrap_or_else(|| {
                                            "application/octet-stream".to_string()
                                        }),
                                        file_uri: url,
                                    },
                                });
                            }
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename: _,
                            } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                parts.push(GeminiPart::ExecutableCode {
                                    executable_code: GeminiExecutableCode { language, code },
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
