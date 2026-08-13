use crate::schema::enum_def::UpstreamProtocol;
use crate::service::transform::unified::*;
use crate::service::transform::{TransformProtocol, TransformValueKind, apply_transform_policy};
use serde_json::Value;

use super::metadata::*;
use super::payload::*;

fn gemini_inline_part(mime_type: &str, data: String) -> GeminiPart {
    GeminiPart::InlineData {
        inline_data: GeminiInlineData {
            mime_type: crate::service::transform::media::portable_gemini_inline_mime(mime_type)
                .expect("Gemini inline media is target-audited")
                .to_string(),
            data,
            display_name: None,
        },
    }
}

fn gemini_structured_output(schema: Option<Value>) -> UnifiedStructuredOutput {
    match schema {
        Some(schema) => {
            let schema = crate::service::transform::structured::without_property_ordering(schema);
            UnifiedStructuredOutput::JsonSchema {
                name: crate::service::transform::structured::stable_schema_name(&schema),
                description: None,
                schema,
                strict: true,
            }
        }
        None => UnifiedStructuredOutput::JsonObject,
    }
}

fn gemini_tool_config(choice: Option<UnifiedToolChoice>) -> Option<GeminiToolConfig> {
    let (mode, allowed_function_names) = match choice {
        Some(UnifiedToolChoice::None) => (GeminiFunctionCallingMode::None, Vec::new()),
        Some(UnifiedToolChoice::Required) => (GeminiFunctionCallingMode::Any, Vec::new()),
        Some(UnifiedToolChoice::Named { name }) => (GeminiFunctionCallingMode::Any, vec![name]),
        Some(UnifiedToolChoice::Allowed {
            names,
            mode: UnifiedAllowedToolMode::Required,
        }) => (GeminiFunctionCallingMode::Any, names),
        Some(UnifiedToolChoice::Allowed {
            names,
            mode: UnifiedAllowedToolMode::Auto,
        }) => (GeminiFunctionCallingMode::Validated, names),
        Some(UnifiedToolChoice::Auto) => (GeminiFunctionCallingMode::Auto, Vec::new()),
        None => return None,
    };
    Some(GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode,
            allowed_function_names,
        },
    })
}

fn push_gemini_content(
    contents: &mut Vec<GeminiRequestContent>,
    role: &'static str,
    parts: Vec<GeminiPart>,
) {
    if parts.is_empty() {
        return;
    }
    if let Some(previous) = contents.last_mut()
        && previous.role.as_deref() == Some(role)
    {
        previous.parts.extend(parts);
    } else {
        contents.push(GeminiRequestContent {
            role: Some(role.to_string()),
            parts,
        });
    }
}

impl From<GeminiRequestPayload> for UnifiedRequest {
    fn from(gemini_req: GeminiRequestPayload) -> Self {
        let tool_mode = gemini_req
            .tool_config
            .as_ref()
            .map(|config| config.function_calling_config.mode);
        let tool_choice = gemini_req.tool_config.as_ref().map(|config| {
            let config = &config.function_calling_config;
            match (config.mode, config.allowed_function_names.is_empty()) {
                (GeminiFunctionCallingMode::None, _) => UnifiedToolChoice::None,
                (GeminiFunctionCallingMode::Any, true) => UnifiedToolChoice::Required,
                (GeminiFunctionCallingMode::Any, false) => UnifiedToolChoice::Allowed {
                    names: config.allowed_function_names.clone(),
                    mode: UnifiedAllowedToolMode::Required,
                },
                (GeminiFunctionCallingMode::Auto | GeminiFunctionCallingMode::Validated, true) => {
                    UnifiedToolChoice::Auto
                }
                (GeminiFunctionCallingMode::Auto | GeminiFunctionCallingMode::Validated, false) => {
                    UnifiedToolChoice::Allowed {
                        names: config.allowed_function_names.clone(),
                        mode: UnifiedAllowedToolMode::Auto,
                    }
                }
            }
        });
        let mut messages = Vec::new();
        let mut items = Vec::new();
        let mut tool_associations = Vec::new();
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
                        GeminiPart::Text { text } | GeminiPart::Thought { text, .. } => Some(text),
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

        for (message_index, content_item) in gemini_req.contents.into_iter().enumerate() {
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
                for (part_index, p) in parts.into_iter().enumerate() {
                    match p {
                        GeminiPart::FunctionCall {
                            function_call,
                            thought_signature,
                        } => {
                            let provider_tool_call_id = function_call.id.clone();
                            let tool_id = provider_tool_call_id.clone().unwrap_or_else(|| {
                                build_gemini_synthetic_tool_call_id(
                                    "request-history",
                                    message_index as u32,
                                    part_index as u32,
                                    &function_call.name,
                                )
                            });
                            tool_associations.push(UnifiedGeminiToolAssociation {
                                unified_tool_call_id: tool_id.clone(),
                                provider_tool_call_id,
                                thought_signature,
                            });
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
                        GeminiPart::Thought { text, thought, .. } => {
                            content_parts.push(if thought {
                                UnifiedContentPart::Reasoning { text }
                            } else {
                                UnifiedContentPart::Text { text }
                            });
                        }
                        _ => {}
                    }
                }
                messages.push(UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: content_parts,
                });
            } else if role == "user" && has_function_response {
                let mut result_parts = Vec::new();
                let mut trailing_parts = Vec::new();
                for (part_index, part) in parts.into_iter().enumerate() {
                    match part {
                        GeminiPart::FunctionResponse { function_response } => {
                            let fr = function_response;
                            let tool_call_id = if let Some(explicit_id) = fr.id.clone() {
                                if let Some(ids) = tool_call_ids.get_mut(&fr.name)
                                    && let Some(position) =
                                        ids.iter().position(|id| id == &explicit_id)
                                {
                                    ids.remove(position);
                                }
                                explicit_id
                            } else {
                                tool_call_ids
                                    .get_mut(&fr.name)
                                    .and_then(|ids| ids.pop_front())
                                    .unwrap_or_else(|| {
                                        build_gemini_synthetic_tool_call_id(
                                            "request-history",
                                            message_index as u32,
                                            part_index as u32,
                                            &fr.name,
                                        )
                                    })
                            };
                            let output = gemini_function_response_to_unified_output(fr.response);
                            items.push(UnifiedItem::FunctionCallOutput(
                                UnifiedFunctionCallOutputItem {
                                    tool_call_id: tool_call_id.clone(),
                                    name: Some(fr.name.clone()),
                                    output: output.clone(),
                                },
                            ));
                            result_parts.push(UnifiedContentPart::ToolResult(UnifiedToolResult {
                                tool_call_id,
                                name: Some(fr.name),
                                output,
                            }));
                        }
                        GeminiPart::Text { text } => {
                            trailing_parts.push(UnifiedContentPart::Text { text });
                        }
                        GeminiPart::Thought { text, thought, .. } => {
                            trailing_parts.push(if thought {
                                UnifiedContentPart::Reasoning { text }
                            } else {
                                UnifiedContentPart::Text { text }
                            });
                        }
                        _ => {}
                    }
                }
                if !trailing_parts.is_empty() {
                    items.extend(legacy_content_to_unified_items(
                        UnifiedRole::User,
                        trailing_parts.clone(),
                    ));
                }
                result_parts.extend(trailing_parts);
                messages.push(UnifiedMessage {
                    role: UnifiedRole::Tool,
                    content: result_parts,
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
                            content_parts.push(UnifiedContentPart::Text { text });
                        }
                        GeminiPart::Thought { text, thought, .. } => {
                            content_parts.push(if thought {
                                UnifiedContentPart::Reasoning { text }
                            } else {
                                UnifiedContentPart::Text { text }
                            });
                        }
                        GeminiPart::InlineData { inline_data } => {
                            content_parts.push(gemini_inline_data_to_unified_content(inline_data));
                        }
                        GeminiPart::FileData { file_data } => {
                            if crate::service::transform::media::classify_inline_mime(
                                &file_data.mime_type,
                            ) == Some(crate::service::transform::media::InlineMediaKind::Image)
                                && crate::service::transform::media::is_valid_http_url(
                                    &file_data.file_uri,
                                )
                            {
                                content_parts.push(UnifiedContentPart::ImageUrl {
                                    url: file_data.file_uri,
                                    detail: None,
                                });
                            } else {
                                content_parts.push(UnifiedContentPart::FileUrl {
                                    url: file_data.file_uri,
                                    mime_type: Some(file_data.mime_type),
                                    filename: file_data.display_name,
                                });
                            }
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
                            strict: matches!(
                                tool_mode,
                                Some(
                                    GeminiFunctionCallingMode::Any
                                        | GeminiFunctionCallingMode::Validated
                                )
                            )
                            .then_some(true),
                        },
                    }
                })
                .collect()
        });

        let (
            temperature,
            max_tokens,
            top_p,
            stop,
            seed,
            presence_penalty,
            frequency_penalty,
            top_k,
            reasoning_effort,
            reasoning_budget_tokens,
            structured_output,
        ) = if let Some(config) = gemini_req.generation_config {
            let reasoning_effort = config.thinking_config.as_ref().and_then(|thinking| {
                thinking
                    .thinking_level
                    .as_ref()
                    .map(|level| match level {
                        GeminiThinkingLevel::Minimal => UnifiedReasoningEffort::Minimal,
                        GeminiThinkingLevel::Low => UnifiedReasoningEffort::Low,
                        GeminiThinkingLevel::Medium => UnifiedReasoningEffort::Medium,
                        GeminiThinkingLevel::High => UnifiedReasoningEffort::High,
                    })
                    .or_else(|| match thinking.thinking_budget {
                        Some(0) => Some(UnifiedReasoningEffort::None),
                        Some(-1) | None => None,
                        Some(_) => None,
                    })
            });
            let structured_output = config
                .response_format
                .and_then(|format| format.text)
                .map(|text| gemini_structured_output(text.schema))
                .or_else(|| {
                    config
                        .response_json_schema
                        .or(config.response_schema)
                        .map(Some)
                        .or_else(|| {
                            (config.response_mime_type.as_deref() == Some("application/json"))
                                .then_some(None)
                        })
                        .map(gemini_structured_output)
                });
            (
                config.temperature,
                config.max_output_tokens,
                config.top_p,
                config.stop_sequences,
                config.seed.map(i64::from),
                config.presence_penalty,
                config.frequency_penalty,
                config.top_k,
                reasoning_effort,
                config
                    .thinking_config
                    .as_ref()
                    .and_then(|thinking| thinking.thinking_budget)
                    .and_then(|budget| u32::try_from(budget).ok()),
                structured_output,
            )
        } else {
            (
                None, None, None, None, None, None, None, None, None, None, None,
            )
        };

        let gemini_extension = UnifiedGeminiRequestExtension {
            tool_associations,
            top_k,
        };

        UnifiedRequest {
            model: None, // Not in Gemini request body
            messages,
            items,
            tools,
            tool_choice,
            parallel_tool_calls: None,
            stream: false, // Set by `into_unified_request`
            temperature,
            max_tokens,
            top_p,
            stop,
            seed,
            presence_penalty,
            frequency_penalty,
            reasoning_effort,
            reasoning_budget_tokens,
            structured_output,
            extensions: (!gemini_extension.is_empty()).then_some(UnifiedRequestExtensions {
                gemini: Some(gemini_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

impl From<UnifiedRequest> for GeminiRequestPayload {
    fn from(mut unified_req: UnifiedRequest) -> Self {
        let tool_name_by_id = build_unified_tool_name_lookup(&unified_req);
        let top_k = unified_req.top_k();
        let tool_association_by_id = unified_req
            .gemini_extension()
            .into_iter()
            .flat_map(|extension| extension.tool_associations.iter())
            .map(|association| {
                (
                    association.unified_tool_call_id.clone(),
                    association.clone(),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let mut contents = Vec::new();
        let mut system_parts = Vec::new();

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
                        system_parts.extend(
                            system_texts
                                .into_iter()
                                .map(|text| GeminiPart::Text { text }),
                        );
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
                                    TransformProtocol::Upstream(UpstreamProtocol::Gemini),
                                    TransformValueKind::Refusal,
                                    "Downgrading refusal content to plain text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text { text });
                                }
                            }
                            UnifiedContentPart::ImageUrl { url, detail } => {
                                if let Some(data_url) =
                                    crate::service::transform::media::parse_base64_data_url(&url)
                                {
                                    parts.push(gemini_inline_part(
                                        data_url.mime_type,
                                        data_url.data.to_string(),
                                    ));
                                } else {
                                    apply_transform_policy(
                                        TransformProtocol::Unified,
                                        TransformProtocol::Upstream(UpstreamProtocol::Gemini),
                                        TransformValueKind::ImageUrl,
                                        "Rejecting non-inline image URL during Gemini request conversion.",
                                    );
                                }
                                let _ = detail;
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                parts.push(gemini_inline_part(&mime_type, data));
                            }
                            UnifiedContentPart::AudioData { data, format } => {
                                let mime_type = if format == "mp3" {
                                    "audio/mpeg"
                                } else {
                                    "audio/wav"
                                };
                                parts.push(gemini_inline_part(mime_type, data));
                            }
                            UnifiedContentPart::Reasoning { text } => {
                                parts.push(GeminiPart::Thought {
                                    text,
                                    thought: true,
                                    thought_signature: None,
                                });
                            }
                            UnifiedContentPart::FileUrl { .. } => {}
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename: _,
                            } => {
                                parts.push(gemini_inline_part(&mime_type, data));
                            }
                            UnifiedContentPart::FileId { .. } => {}
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                parts.push(GeminiPart::ExecutableCode {
                                    executable_code: GeminiExecutableCode { language, code },
                                });
                            }
                            UnifiedContentPart::ToolCall(call) => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Upstream(UpstreamProtocol::Gemini),
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
                                        id: Some(
                                            tool_association_by_id
                                                .get(&result.tool_call_id)
                                                .and_then(|association| {
                                                    association.provider_tool_call_id.clone()
                                                })
                                                .unwrap_or_else(|| result.tool_call_id.clone()),
                                        ),
                                        name,
                                        response: unified_tool_result_to_gemini_response(
                                            &result.output,
                                        ),
                                    },
                                });
                            }
                        }
                    }
                    push_gemini_content(&mut contents, "user", parts);
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
                                    TransformProtocol::Upstream(UpstreamProtocol::Gemini),
                                    TransformValueKind::Refusal,
                                    "Downgrading refusal content to plain text during Gemini assistant conversion.",
                                ) {
                                    parts.push(GeminiPart::Text { text });
                                }
                            }
                            UnifiedContentPart::ImageData { .. }
                            | UnifiedContentPart::AudioData { .. } => {}
                            UnifiedContentPart::Reasoning { text } => {
                                parts.push(GeminiPart::Thought {
                                    text,
                                    thought: true,
                                    thought_signature: None,
                                });
                            }
                            UnifiedContentPart::FileUrl { .. }
                            | UnifiedContentPart::FileData { .. } => {}
                            UnifiedContentPart::FileId { .. } => {}
                            UnifiedContentPart::ToolCall(call) => {
                                let association = tool_association_by_id.get(&call.id);
                                parts.push(GeminiPart::FunctionCall {
                                    function_call: GeminiFunctionCall {
                                        id: Some(
                                            association
                                                .and_then(|association| {
                                                    association.provider_tool_call_id.clone()
                                                })
                                                .unwrap_or_else(|| call.id.clone()),
                                        ),
                                        name: call.name,
                                        args: call.arguments,
                                    },
                                    thought_signature: association.and_then(|association| {
                                        association.thought_signature.clone()
                                    }),
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
                                    TransformProtocol::Upstream(UpstreamProtocol::Gemini),
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
                                    TransformProtocol::Upstream(UpstreamProtocol::Gemini),
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
                    push_gemini_content(&mut contents, "model", parts);
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
                                            TransformProtocol::Upstream(UpstreamProtocol::Gemini),
                                            TransformValueKind::ToolResult,
                                            "Gemini tool result is missing tool name; using explicit synthetic fallback name derived from tool_call_id.",
                                        );
                                        build_gemini_fallback_tool_name(&result.tool_call_id)
                                    });
                                let response_content =
                                    unified_tool_result_to_gemini_response(&result.output);

                                parts.push(GeminiPart::FunctionResponse {
                                    function_response: GeminiFunctionResponse {
                                        id: Some(
                                            tool_association_by_id
                                                .get(&result.tool_call_id)
                                                .and_then(|association| {
                                                    association.provider_tool_call_id.clone()
                                                })
                                                .unwrap_or_else(|| result.tool_call_id.clone()),
                                        ),
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
                                    TransformProtocol::Upstream(UpstreamProtocol::Gemini),
                                    TransformValueKind::Refusal,
                                    "Downgrading refusal content to plain text during Gemini tool conversion.",
                                ) {
                                    parts.push(GeminiPart::Text { text });
                                }
                            }
                            UnifiedContentPart::Reasoning { text } => {
                                parts.push(GeminiPart::Thought {
                                    text,
                                    thought: true,
                                    thought_signature: None,
                                });
                            }
                            UnifiedContentPart::ImageData { .. }
                            | UnifiedContentPart::AudioData { .. } => {}
                            UnifiedContentPart::ImageUrl { url, detail } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Upstream(UpstreamProtocol::Gemini),
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
                            UnifiedContentPart::FileUrl { .. }
                            | UnifiedContentPart::FileData { .. } => {}
                            UnifiedContentPart::FileId { .. } => {}
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                parts.push(GeminiPart::ExecutableCode {
                                    executable_code: GeminiExecutableCode { language, code },
                                });
                            }
                            UnifiedContentPart::ToolCall(call) => {
                                let association = tool_association_by_id.get(&call.id);
                                parts.push(GeminiPart::FunctionCall {
                                    function_call: GeminiFunctionCall {
                                        id: Some(
                                            association
                                                .and_then(|association| {
                                                    association.provider_tool_call_id.clone()
                                                })
                                                .unwrap_or_else(|| call.id.clone()),
                                        ),
                                        name: call.name,
                                        args: call.arguments,
                                    },
                                    thought_signature: association.and_then(|association| {
                                        association.thought_signature.clone()
                                    }),
                                });
                            }
                        }
                    }
                    // Gemini represents function responses under the user role.
                    push_gemini_content(&mut contents, "user", parts);
                }
            }
        }

        // Gemini has a specific structure for tools.
        let tools = unified_req.tools.map(|tools| {
            let function_declarations = tools
                .into_iter()
                .map(|tool| GeminiFunctionDefinition {
                    name: tool.function.name,
                    description: tool.function.description,
                    parameters: tool.function.parameters,
                })
                .collect();
            vec![GeminiTools {
                function_declarations,
            }]
        });
        let tool_config = gemini_tool_config(unified_req.tool_choice);

        let thinking_config = unified_req
            .reasoning_budget_tokens
            .map(|budget| GeminiThinkingConfig {
                thinking_level: None,
                thinking_budget: Some(i64::from(budget)),
                include_thoughts: Some(true),
            })
            .or_else(|| {
                unified_req.reasoning_effort.map(|effort| match effort {
                    UnifiedReasoningEffort::None => GeminiThinkingConfig {
                        thinking_level: None,
                        thinking_budget: Some(0),
                        include_thoughts: None,
                    },
                    UnifiedReasoningEffort::Minimal => GeminiThinkingConfig {
                        thinking_level: Some(GeminiThinkingLevel::Minimal),
                        thinking_budget: None,
                        include_thoughts: Some(true),
                    },
                    UnifiedReasoningEffort::Low => GeminiThinkingConfig {
                        thinking_level: Some(GeminiThinkingLevel::Low),
                        thinking_budget: None,
                        include_thoughts: Some(true),
                    },
                    UnifiedReasoningEffort::Medium => GeminiThinkingConfig {
                        thinking_level: Some(GeminiThinkingLevel::Medium),
                        thinking_budget: None,
                        include_thoughts: Some(true),
                    },
                    UnifiedReasoningEffort::High | UnifiedReasoningEffort::Xhigh => {
                        GeminiThinkingConfig {
                            thinking_level: Some(GeminiThinkingLevel::High),
                            thinking_budget: None,
                            include_thoughts: Some(true),
                        }
                    }
                })
            });
        let (response_mime_type, response_json_schema) = match unified_req.structured_output.take()
        {
            Some(UnifiedStructuredOutput::JsonObject) => {
                (Some("application/json".to_string()), None)
            }
            Some(UnifiedStructuredOutput::JsonSchema { schema, .. }) => {
                (Some("application/json".to_string()), Some(schema))
            }
            None => (None, None),
        };
        let generation_config = Some(GeminiGenerationConfig {
            candidate_count: Some(1),
            temperature: unified_req.temperature,
            max_output_tokens: unified_req.max_tokens,
            top_p: unified_req.top_p,
            seed: unified_req.seed.and_then(|seed| i32::try_from(seed).ok()),
            presence_penalty: unified_req.presence_penalty,
            frequency_penalty: unified_req.frequency_penalty,
            top_k,
            stop_sequences: unified_req.stop,
            thinking_config,
            response_mime_type,
            response_schema: None,
            response_json_schema,
            response_format: None,
        });

        let system_instruction =
            (!system_parts.is_empty()).then_some(GeminiSystemInstruction::Object {
                parts: system_parts,
            });

        GeminiRequestPayload {
            contents,
            system_instruction,
            tools,
            tool_config,
            generation_config,
            safety_settings: None,
        }
    }
}
