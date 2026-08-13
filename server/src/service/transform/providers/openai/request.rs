use serde_json::{Value, json};

use super::payload::*;

use crate::service::transform::unified::*;

impl From<ReasoningEffort> for UnifiedReasoningEffort {
    fn from(value: ReasoningEffort) -> Self {
        match value {
            ReasoningEffort::_None => Self::None,
            ReasoningEffort::Minimal => Self::Minimal,
            ReasoningEffort::Low => Self::Low,
            ReasoningEffort::Medium => Self::Medium,
            ReasoningEffort::High => Self::High,
            ReasoningEffort::Xhigh => Self::Xhigh,
        }
    }
}

impl From<UnifiedReasoningEffort> for ReasoningEffort {
    fn from(value: UnifiedReasoningEffort) -> Self {
        match value {
            UnifiedReasoningEffort::None => Self::_None,
            UnifiedReasoningEffort::Minimal => Self::Minimal,
            UnifiedReasoningEffort::Low => Self::Low,
            UnifiedReasoningEffort::Medium => Self::Medium,
            UnifiedReasoningEffort::High => Self::High,
            UnifiedReasoningEffort::Xhigh => Self::Xhigh,
        }
    }
}

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

fn split_openai_response_format(
    value: Option<Value>,
) -> (Option<UnifiedStructuredOutput>, Option<Value>) {
    let Some(value) = value else {
        return (None, None);
    };
    match value.get("type").and_then(Value::as_str) {
        Some("json_object") => (Some(UnifiedStructuredOutput::JsonObject), None),
        Some("json_schema") => {
            let definition = value
                .get("json_schema")
                .expect("OpenAI json_schema response format is adapter-validated");
            (
                Some(UnifiedStructuredOutput::JsonSchema {
                    name: definition
                        .get("name")
                        .and_then(Value::as_str)
                        .expect("OpenAI schema name is adapter-validated")
                        .to_string(),
                    description: definition
                        .get("description")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    schema: definition
                        .get("schema")
                        .expect("OpenAI response schema is adapter-validated")
                        .clone(),
                    strict: definition
                        .get("strict")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                }),
                None,
            )
        }
        _ => (None, Some(value)),
    }
}

fn unified_tool_choice_from_openai(value: Value) -> UnifiedToolChoice {
    match value {
        Value::String(value) => match value.as_str() {
            "none" => UnifiedToolChoice::None,
            "required" => UnifiedToolChoice::Required,
            _ => UnifiedToolChoice::Auto,
        },
        Value::Object(value)
            if value.get("type").and_then(Value::as_str) == Some("allowed_tools") =>
        {
            let allowed = value
                .get("allowed_tools")
                .expect("OpenAI allowed tool choice is adapter-validated");
            let names = allowed
                .get("tools")
                .and_then(Value::as_array)
                .expect("OpenAI allowed tools are adapter-validated")
                .iter()
                .map(|tool| {
                    tool.get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                        .expect("OpenAI allowed function name is adapter-validated")
                        .to_string()
                })
                .collect();
            let mode = if allowed.get("mode").and_then(Value::as_str) == Some("required") {
                UnifiedAllowedToolMode::Required
            } else {
                UnifiedAllowedToolMode::Auto
            };
            UnifiedToolChoice::Allowed { names, mode }
        }
        Value::Object(value) => UnifiedToolChoice::Named {
            name: value
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .expect("OpenAI named function choice is adapter-validated")
                .to_string(),
        },
        _ => unreachable!("OpenAI tool_choice is adapter-validated"),
    }
}

fn openai_tool_choice(choice: UnifiedToolChoice) -> Value {
    match choice {
        UnifiedToolChoice::None => json!("none"),
        UnifiedToolChoice::Auto => json!("auto"),
        UnifiedToolChoice::Required => json!("required"),
        UnifiedToolChoice::Named { name } => {
            json!({ "type": "function", "function": { "name": name } })
        }
        UnifiedToolChoice::Allowed { names, mode } => json!({
            "type": "allowed_tools",
            "allowed_tools": {
                "mode": match mode {
                    UnifiedAllowedToolMode::Auto => "auto",
                    UnifiedAllowedToolMode::Required => "required",
                },
                "tools": names.into_iter().map(|name| {
                    json!({ "type": "function", "function": { "name": name } })
                }).collect::<Vec<_>>()
            }
        }),
    }
}

impl From<OpenAiRequestPayload> for UnifiedRequest {
    fn from(openai_req: OpenAiRequestPayload) -> Self {
        let messages = openai_req
            .messages
            .into_iter()
            .map(|msg| {
                let role = match msg.role.as_str() {
                    "system" | "developer" => UnifiedRole::System,
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
                                    OpenAiContentPart::InputAudio { input_audio } => {
                                        content.push(UnifiedContentPart::AudioData {
                                            data: input_audio.data,
                                            format: input_audio.format,
                                        });
                                    }
                                    OpenAiContentPart::File { file } => {
                                        if let Some(file_data) = file.file_data {
                                            let mime_type = file
                                                .filename
                                                .as_deref()
                                                .and_then(crate::service::transform::media::mime_type_from_filename)
                                                .unwrap_or("application/octet-stream")
                                                .to_string();
                                            content.push(UnifiedContentPart::FileData {
                                                data: file_data,
                                                mime_type,
                                                filename: file.filename,
                                            });
                                        } else if let Some(file_id) = file.file_id {
                                            content.push(UnifiedContentPart::FileId {
                                                file_id,
                                                filename: file.filename,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(reasoning) = msg.reasoning_content {
                    content.insert(0, UnifiedContentPart::Reasoning { text: reasoning });
                }

                if let Some(refusal) = msg.refusal {
                    content.insert(0, UnifiedContentPart::Refusal { text: refusal });
                }

                if let Some(tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        let args: Value = if tc.function.arguments.trim().is_empty() {
                            json!({})
                        } else {
                            serde_json::from_str(&tc.function.arguments)
                                .expect("OpenAI tool arguments are validated by the adapter")
                        };
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
        let parallel_tool_calls = openai_req.parallel_tool_calls;
        let tool_choice = openai_req.tool_choice.map(unified_tool_choice_from_openai);
        let reasoning_effort = openai_req.reasoning_effort.map(Into::into);

        let passthrough =
            build_registered_passthrough(passthrough_fields, "openai_request_to_unified");

        let (structured_output, response_format) =
            split_openai_response_format(openai_req.response_format);
        let openai_extension = UnifiedOpenAiRequestExtension {
            tool_choice: None,
            n: openai_req.n,
            response_format,
            logit_bias: openai_req.logit_bias,
            user: openai_req.user,
            passthrough,
        };

        UnifiedRequest {
            model: Some(openai_req.model),
            messages,
            tools: openai_req.tools,
            tool_choice,
            parallel_tool_calls,
            stream: openai_req.stream.unwrap_or(false),
            temperature: openai_req.temperature,
            max_tokens: openai_req.max_completion_tokens.or(openai_req.max_tokens),
            top_p: openai_req.top_p,
            stop,
            seed: openai_req.seed,
            presence_penalty: openai_req.presence_penalty,
            frequency_penalty: openai_req.frequency_penalty,
            reasoning_effort,
            structured_output,
            extensions: (!openai_extension.is_empty()).then_some(UnifiedRequestExtensions {
                openai: Some(openai_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
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
                let mut reasoning_content = String::new();
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
                            reasoning_content.push_str(&text);
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
                        UnifiedContentPart::AudioData { data, format } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::InputAudio {
                                input_audio: OpenAiInputAudio { data, format },
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
                            mime_type: _,
                            filename,
                        } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::File {
                                file: OpenAiFile {
                                    file_data: Some(data),
                                    file_id: None,
                                    filename,
                                },
                            });
                        }
                        UnifiedContentPart::FileId { file_id, filename } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::File {
                                file: OpenAiFile {
                                    file_data: None,
                                    file_id: Some(file_id),
                                    filename,
                                },
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
                let reasoning_content =
                    (!reasoning_content.is_empty()).then_some(reasoning_content);

                // If there are tool results, they must be separate messages in OpenAI
                // We also need to handle mixed content (e.g. Text + ToolResults) by creating separate messages
                let mut generated_messages = Vec::new();

                // 1. If there is text content, create a message for it first
                if content_val.is_some()
                    || reasoning_content.is_some()
                    || !tool_calls.is_empty()
                    || refusal.is_some()
                {
                    generated_messages.push(OpenAiMessage {
                        role: role.clone(),
                        content: content_val,
                        reasoning_content,
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls.clone())
                        },
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
                        reasoning_content: None,
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
        let (logprobs, top_logprobs, legacy_parallel_tool_calls) =
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
                        .and_then(Value::as_bool),
                )
            } else {
                (None, None, None)
            };

        let response_format = unified_req
            .structured_output
            .map(crate::service::transform::structured::openai_response_format)
            .or(openai_extension.response_format);

        OpenAiRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            messages,
            tools: unified_req.tools,
            tool_choice: unified_req
                .tool_choice
                .map(openai_tool_choice)
                .or(openai_extension.tool_choice),
            stream: Some(unified_req.stream),
            temperature: unified_req.temperature,
            max_tokens: unified_req.max_tokens,
            max_completion_tokens: None,
            top_p: unified_req.top_p,
            stop,
            n: openai_extension.n,
            seed: unified_req.seed,
            presence_penalty: unified_req.presence_penalty,
            frequency_penalty: unified_req.frequency_penalty,
            logit_bias: openai_extension.logit_bias,
            logprobs,
            top_logprobs,
            response_format,
            user: openai_extension.user,
            parallel_tool_calls: unified_req
                .parallel_tool_calls
                .or(legacy_parallel_tool_calls),
            reasoning_effort: unified_req.reasoning_effort.map(Into::into),
        }
    }
}
