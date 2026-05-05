use serde_json::{Value, json};

use super::payload::*;

use crate::service::transform::unified::*;

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

                if let Some(refusal) = choice.message.refusal {
                    content.insert(0, UnifiedContentPart::Refusal { text: refusal });
                }

                if let Some(tool_calls) = choice.message.tool_calls {
                    for tc in tool_calls {
                        let args: Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
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

                    content.push(UnifiedContentPart::ToolResult(
                        UnifiedToolResult::from_legacy_content(
                            tool_call_id,
                            choice.message.name,
                            result_content,
                        ),
                    ));
                }

                let message = UnifiedMessage {
                    role,
                    content,
                    ..Default::default()
                };

                UnifiedChoice {
                    index: choice.index,
                    message,
                    items: Vec::new(),
                    finish_reason: choice.finish_reason,
                    logprobs: choice
                        .logprobs
                        .map(|lp| serde_json::to_value(lp).unwrap_or(Value::Null)),
                }
            })
            .collect();

        UnifiedResponse {
            id: openai_res.id,
            model: Some(openai_res.model),
            choices,
            usage: openai_res.usage.map(|u| u.into()),
            created: Some(openai_res.created),
            object: Some(openai_res.object),
            system_fingerprint: openai_res.system_fingerprint,
            provider_response_metadata: None,
            synthetic_metadata: None,
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
                let mut refusal = None;
                let mut has_multimodal = false;

                for part in choice.message.content {
                    match part {
                        UnifiedContentPart::Text { text } => {
                            content_parts.push(OpenAiContentPart::Text { text });
                        }
                        UnifiedContentPart::Refusal { text } => {
                            refusal = Some(text);
                        }
                        UnifiedContentPart::ImageUrl { url, detail } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::ImageUrl {
                                image_url: OpenAiImageUrl { url, detail },
                            });
                        }
                        UnifiedContentPart::Reasoning { text } => {
                            content_parts.push(OpenAiContentPart::Text { text });
                        }
                        UnifiedContentPart::ImageData { mime_type, data } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::ImageUrl {
                                image_url: OpenAiImageUrl {
                                    url: build_data_url(&mime_type, &data),
                                    detail: Some("auto".to_string()),
                                },
                            });
                        }
                        UnifiedContentPart::FileUrl {
                            url,
                            mime_type,
                            filename,
                        } => {
                            content_parts.push(OpenAiContentPart::Text {
                                text: render_file_reference_text(
                                    &url,
                                    mime_type.as_deref(),
                                    filename.as_deref(),
                                ),
                            });
                        }
                        UnifiedContentPart::FileData {
                            data,
                            mime_type,
                            filename,
                        } => {
                            content_parts.push(OpenAiContentPart::Text {
                                text: render_inline_file_data_text(
                                    &data,
                                    &mime_type,
                                    filename.as_deref(),
                                ),
                            });
                        }
                        UnifiedContentPart::ExecutableCode { language, code } => {
                            content_parts.push(OpenAiContentPart::Text {
                                text: render_executable_code_text(&language, &code),
                            });
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
                                text: result.legacy_content(),
                            });
                            tool_call_id = Some(result.tool_call_id);
                            name = result.name;
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
                    refusal,
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
            model: unified_res.model.unwrap_or_default(),
            system_fingerprint: unified_res.system_fingerprint,
            choices,
            usage: unified_res.usage.map(|u| u.into()),
        }
    }
}
