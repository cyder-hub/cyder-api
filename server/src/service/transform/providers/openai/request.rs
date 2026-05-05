use serde_json::{Value, json};

use super::payload::*;

use crate::service::transform::unified::*;

fn register_passthrough_field(
    passthrough_fields: &mut Vec<(String, Value)>,
    key: &str,
    value: Value,
    context: &str,
) {
    if is_registered_passthrough_key(key) {
        passthrough_fields.push((key.to_string(), value));
    } else {
        cyder_tools::log::warn!(
            "[transform][passthrough] rejected_unregistered_key key={} context={} registered_keys={:?}",
            key,
            context,
            REGISTERED_PASSTHROUGH_KEYS
        );
    }
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

                if let Some(refusal) = msg.refusal {
                    content.insert(0, UnifiedContentPart::Refusal { text: refusal });
                }

                if let Some(tool_calls) = msg.tool_calls {
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

                    content.push(UnifiedContentPart::ToolResult(
                        UnifiedToolResult::from_legacy_content(
                            tool_call_id,
                            msg.name,
                            result_content,
                        ),
                    ));
                }

                UnifiedMessage { role, content }
            })
            .collect();

        let stop = openai_req.stop.map(|v| match v {
            OpenAiStop::String(s) => vec![s],
            OpenAiStop::Array(arr) => arr,
        });

        // Store OpenAI-specific fields that don't have unified equivalents in passthrough
        let mut passthrough_fields = Vec::new();
        if let Some(logprobs) = openai_req.logprobs {
            register_passthrough_field(
                &mut passthrough_fields,
                "logprobs",
                json!(logprobs),
                "openai_request_to_unified",
            );
        }
        if let Some(top_logprobs) = openai_req.top_logprobs {
            register_passthrough_field(
                &mut passthrough_fields,
                "top_logprobs",
                json!(top_logprobs),
                "openai_request_to_unified",
            );
        }
        if let Some(parallel_tool_calls) = openai_req.parallel_tool_calls {
            register_passthrough_field(
                &mut passthrough_fields,
                "parallel_tool_calls",
                json!(parallel_tool_calls),
                "openai_request_to_unified",
            );
        }
        if let Some(reasoning_effort) = openai_req.reasoning_effort {
            register_passthrough_field(
                &mut passthrough_fields,
                "reasoning_effort",
                json!(reasoning_effort),
                "openai_request_to_unified",
            );
        }

        let passthrough =
            build_registered_passthrough(passthrough_fields, "openai_request_to_unified");

        let openai_extension = UnifiedOpenAiRequestExtension {
            tool_choice: openai_req.tool_choice,
            n: openai_req.n,
            response_format: openai_req.response_format,
            logit_bias: openai_req.logit_bias,
            user: openai_req.user,
            passthrough,
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
            extensions: (!openai_extension.is_empty()).then_some(UnifiedRequestExtensions {
                openai: Some(openai_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
        .filter_empty() // Filter out empty content and messages
    }
}

impl From<UnifiedRequest> for OpenAiRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let openai_extension = unified_req.openai_extension().cloned().unwrap_or_default();
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
                let mut refusal = None;
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
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls.clone())
                        },
                        name: None,
                        tool_call_id: None,
                        refusal: refusal.clone(),
                    });
                } else if !tool_calls.is_empty() {
                    // Case where there is no text but there are tool calls (Assistant invoking tool)
                    generated_messages.push(OpenAiMessage {
                        role: role.clone(),
                        content: None,
                        tool_calls: Some(tool_calls),
                        name: None,
                        tool_call_id: None,
                        refusal: refusal.clone(),
                    });
                }

                // 2. Add tool results as separate messages with 'tool' role
                for result in tool_results {
                    generated_messages.push(OpenAiMessage {
                        role: "tool".to_string(),
                        content: Some(OpenAiContent::Text(result.legacy_content())),
                        tool_calls: None,
                        name: result.name,
                        tool_call_id: Some(result.tool_call_id),
                        refusal: None,
                    });
                }

                generated_messages
            })
            .collect();

        let stop = unified_req.stop.clone().map(|v| {
            if v.len() == 1 {
                OpenAiStop::String(v.into_iter().next().unwrap())
            } else {
                OpenAiStop::Array(v)
            }
        });

        // Extract OpenAI-specific fields from passthrough if present
        let (logprobs, top_logprobs, parallel_tool_calls, reasoning_effort) =
            if let Some(passthrough) = openai_extension.passthrough.as_ref() {
                audit_passthrough_keys(passthrough, "unified_request_to_openai");
                (
                    passthrough.get("logprobs").and_then(|v| v.as_bool()),
                    passthrough
                        .get("top_logprobs")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    passthrough
                        .get("parallel_tool_calls")
                        .and_then(|v| v.as_bool()),
                    passthrough
                        .get("reasoning_effort")
                        .and_then(|v| serde_json::from_value(v.clone()).ok()),
                )
            } else {
                (None, None, None, None)
            };

        OpenAiRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            messages,
            tools: unified_req.tools,
            tool_choice: openai_extension.tool_choice,
            stream: Some(unified_req.stream),
            temperature: unified_req.temperature,
            max_tokens: unified_req.max_tokens,
            top_p: unified_req.top_p,
            stop,
            n: openai_extension.n,
            seed: unified_req.seed,
            presence_penalty: unified_req.presence_penalty,
            frequency_penalty: unified_req.frequency_penalty,
            logit_bias: openai_extension.logit_bias,
            logprobs,
            top_logprobs,
            response_format: openai_extension.response_format,
            user: openai_extension.user,
            parallel_tool_calls,
            reasoning_effort,
        }
    }
}
