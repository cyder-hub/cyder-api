use serde_json::{Value, json};

use crate::schema::enum_def::UpstreamProtocol;
use crate::service::transform::unified::*;
use crate::service::transform::{TransformProtocol, TransformValueKind, apply_transform_policy};

use super::payload::*;
use super::response::*;

impl From<ReasoningEffort> for UnifiedReasoningEffort {
    fn from(value: ReasoningEffort) -> Self {
        match value {
            ReasoningEffort::None => Self::None,
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
            UnifiedReasoningEffort::None => Self::None,
            UnifiedReasoningEffort::Minimal => Self::Minimal,
            UnifiedReasoningEffort::Low => Self::Low,
            UnifiedReasoningEffort::Medium => Self::Medium,
            UnifiedReasoningEffort::High => Self::High,
            UnifiedReasoningEffort::Xhigh => Self::Xhigh,
        }
    }
}

fn unified_structured_output_to_responses(output: UnifiedStructuredOutput) -> TextResponseFormat {
    match output {
        UnifiedStructuredOutput::JsonObject => TextResponseFormat::JsonObject,
        UnifiedStructuredOutput::JsonSchema {
            name,
            description,
            schema,
            strict,
        } => TextResponseFormat::JsonSchema {
            name,
            description,
            schema: Some(schema),
            strict,
        },
    }
}

fn split_responses_text(
    text: Option<TextField>,
) -> (Option<UnifiedStructuredOutput>, Option<TextField>) {
    let Some(text) = text else {
        return (None, None);
    };
    match text.format {
        TextResponseFormat::JsonObject => (Some(UnifiedStructuredOutput::JsonObject), None),
        TextResponseFormat::JsonSchema {
            name,
            description,
            schema,
            strict,
        } => (
            Some(UnifiedStructuredOutput::JsonSchema {
                name,
                description,
                schema: schema.expect("Responses JSON schema is adapter-validated"),
                strict,
            }),
            None,
        ),
        format @ TextResponseFormat::Text => (
            None,
            Some(TextField {
                format,
                verbosity: text.verbosity,
            }),
        ),
    }
}

fn unified_tool_choice_to_responses(choice: UnifiedToolChoice) -> ToolChoice {
    match choice {
        UnifiedToolChoice::None => ToolChoice::Value(ToolChoiceValue::None),
        UnifiedToolChoice::Auto => ToolChoice::Value(ToolChoiceValue::Auto),
        UnifiedToolChoice::Required => ToolChoice::Value(ToolChoiceValue::Required),
        UnifiedToolChoice::Named { name } => ToolChoice::Specific(SpecificToolChoice {
            _type: "function".to_string(),
            name,
        }),
        UnifiedToolChoice::Allowed { names, mode } => ToolChoice::Allowed(AllowedToolChoice {
            _type: "allowed_tools".to_string(),
            tools: names
                .into_iter()
                .map(|name| SpecificToolChoice {
                    _type: "function".to_string(),
                    name,
                })
                .collect(),
            mode: match mode {
                UnifiedAllowedToolMode::Auto => ToolChoiceValue::Auto,
                UnifiedAllowedToolMode::Required => ToolChoiceValue::Required,
            },
        }),
    }
}

fn unified_tool_choice_from_responses(choice: ToolChoice) -> UnifiedToolChoice {
    match choice {
        ToolChoice::Value(ToolChoiceValue::None) => UnifiedToolChoice::None,
        ToolChoice::Value(ToolChoiceValue::Auto) => UnifiedToolChoice::Auto,
        ToolChoice::Value(ToolChoiceValue::Required) => UnifiedToolChoice::Required,
        ToolChoice::Specific(choice) => UnifiedToolChoice::Named { name: choice.name },
        ToolChoice::Allowed(choice) => UnifiedToolChoice::Allowed {
            names: choice.tools.into_iter().map(|tool| tool.name).collect(),
            mode: match choice.mode {
                ToolChoiceValue::Auto | ToolChoiceValue::None => UnifiedAllowedToolMode::Auto,
                ToolChoiceValue::Required => UnifiedAllowedToolMode::Required,
            },
        },
    }
}

impl From<UnifiedRequest> for ResponsesRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let responses_extension = unified_req
            .responses_extension()
            .cloned()
            .unwrap_or_default();
        let openai_extension = unified_req.openai_extension().cloned().unwrap_or_default();

        let mut inferred_instructions = Vec::new();

        let items = if !unified_req.items.is_empty() {
            unified_req
                .items
                .into_iter()
                .flat_map(|item| match item {
                    UnifiedItem::Message(msg) if msg.role == UnifiedRole::System => {
                        let text = msg
                            .content
                            .into_iter()
                            .filter_map(|part| {
                                if matches!(
                                    &part,
                                    UnifiedContentPart::Text { .. }
                                        | UnifiedContentPart::Refusal { .. }
                                        | UnifiedContentPart::Reasoning { .. }
                                ) {
                                    return render_responses_instruction_part(part);
                                }

                                let keep = apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Upstream(UpstreamProtocol::Responses),
                                    TransformValueKind::from(&part),
                                    "Downgrading rich system content to recoverable instruction text during Responses request conversion.",
                                );
                                if matches!(
                                    part,
                                    UnifiedContentPart::ImageUrl { .. }
                                        | UnifiedContentPart::ImageData { .. }
                                        | UnifiedContentPart::AudioData { .. }
                                        | UnifiedContentPart::FileUrl { .. }
                                        | UnifiedContentPart::FileData { .. }
                                        | UnifiedContentPart::FileId { .. }
                                ) {
                                    return None;
                                }
                                keep.then(|| render_responses_instruction_part(part)).flatten()
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        if !text.trim().is_empty() {
                            inferred_instructions.push(text);
                        }
                        Vec::new()
                    }
                    UnifiedItem::Message(msg) => {
                        unified_message_to_responses_input_items(UnifiedMessage {
                            role: msg.role,
                            content: msg.content,
                        })
                    }
                    UnifiedItem::Reasoning(item) => vec![ItemField::Reasoning(ReasoningBody {
                        _type: "reasoning".to_string(),
                        id: format!("rs_{}", crate::utils::ID_GENERATOR.generate_id()),
                        content: Some(
                            item.content
                                .into_iter()
                                .map(unified_reasoning_part_to_responses_part)
                                .collect(),
                        ),
                        summary: Vec::new(),
                        encrypted_content: None,
                    })],
                    UnifiedItem::FunctionCall(call) => vec![ItemField::FunctionCall(FunctionCall {
                        _type: "function_call".to_string(),
                        id: format!("fc_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: call.id,
                        name: call.name,
                        arguments: stringify_function_arguments(call.arguments),
                        status: MessageStatus::Completed,
                    })],
                    UnifiedItem::FunctionCallOutput(output) => vec![ItemField::FunctionCallOutput(
                        FunctionCallOutput {
                            _type: "function_call_output".to_string(),
                            id: format!("fco_{}", crate::utils::ID_GENERATOR.generate_id()),
                            call_id: output.tool_call_id,
                            output: unified_tool_result_to_function_output_payload(output.output),
                            status: MessageStatus::Completed,
                        },
                    )],
        UnifiedItem::FileReference(file) => vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
            role: MessageRole::User,
            status: MessageStatus::Completed,
            content: vec![ItemContentPart::InputFile {
                filename: file.filename,
                file_url: file.file_url,
                file_id: file.file_id,
                file_data: None,
            }],
        })],
                })
                .collect()
        } else {
            let mut items = Vec::new();
            for message in unified_req.messages {
                if message.role == UnifiedRole::System {
                    let text = message
                        .content
                        .into_iter()
                        .filter_map(|part| {
                            if matches!(
                                &part,
                                UnifiedContentPart::Text { .. }
                                    | UnifiedContentPart::Refusal { .. }
                                    | UnifiedContentPart::Reasoning { .. }
                            ) {
                                return render_responses_instruction_part(part);
                            }

                            let keep = apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Upstream(UpstreamProtocol::Responses),
                                TransformValueKind::from(&part),
                                "Downgrading rich system content to recoverable instruction text during Responses request conversion.",
                            );
                            if matches!(
                                part,
                                UnifiedContentPart::ImageUrl { .. }
                                    | UnifiedContentPart::ImageData { .. }
                                    | UnifiedContentPart::AudioData { .. }
                                    | UnifiedContentPart::FileUrl { .. }
                                    | UnifiedContentPart::FileData { .. }
                                    | UnifiedContentPart::FileId { .. }
                            ) {
                                return None;
                            }
                            keep.then(|| render_responses_instruction_part(part)).flatten()
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    if !text.trim().is_empty() {
                        inferred_instructions.push(text);
                    }
                } else {
                    items.extend(unified_message_to_responses_input_items(message));
                }
            }
            items
        };

        let instructions = responses_extension.instructions.or_else(|| {
            if inferred_instructions.is_empty() {
                None
            } else {
                Some(inferred_instructions.join("\n\n"))
            }
        });

        let tools = unified_req.tools.map(|items| {
            items
                .into_iter()
                .map(|tool| {
                    Tool::Function(FunctionTool {
                        name: tool.function.name,
                        description: tool.function.description,
                        parameters: Some(tool.function.parameters),
                        strict: tool.function.strict,
                    })
                })
                .collect()
        });

        let tool_choice = unified_req
            .tool_choice
            .map(unified_tool_choice_to_responses)
            .or_else(|| {
                responses_extension
                    .tool_choice
                    .map(|value| {
                        serde_json::from_value(value)
                            .expect("Responses tool_choice extension is source-validated")
                    })
                    .or_else(|| {
                        openai_extension
                            .tool_choice
                            .and_then(convert_openai_tool_choice_to_responses)
                    })
            });

        let text = unified_req
            .structured_output
            .map(unified_structured_output_to_responses)
            .or_else(|| {
                responses_extension.text_format.map(|value| {
                    serde_json::from_value(value)
                        .expect("Responses text format extension is source-validated")
                })
            })
            .or_else(|| {
                openai_extension
                    .response_format
                    .and_then(convert_openai_response_format_to_responses)
            })
            .map(|format| TextField {
                format,
                verbosity: None,
            });

        let reasoning = responses_extension
            .reasoning
            .map(|value| {
                serde_json::from_value(value)
                    .expect("Responses reasoning extension is source-validated")
            })
            .or_else(|| {
                unified_req.reasoning_effort.map(|effort| Reasoning {
                    effort: Some(effort.into()),
                    summary: None,
                })
            });

        let parallel_tool_calls = unified_req
            .parallel_tool_calls
            .or(responses_extension.parallel_tool_calls)
            .or_else(|| {
                openai_extension
                    .passthrough
                    .as_ref()
                    .and_then(|value| value.get("parallel_tool_calls"))
                    .and_then(Value::as_bool)
            });

        ResponsesRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            input: Input::Items(items),
            instructions,
            tools,
            tool_choice,
            text,
            reasoning,
            parallel_tool_calls,
            stream: Some(unified_req.stream),
            temperature: unified_req.temperature,
            max_tokens: unified_req.max_tokens,
            top_p: unified_req.top_p,
        }
    }
}

impl From<ResponsesRequestPayload> for UnifiedRequest {
    fn from(responses_req: ResponsesRequestPayload) -> Self {
        let ResponsesRequestPayload {
            model,
            input,
            instructions,
            tools,
            tool_choice,
            text,
            reasoning,
            parallel_tool_calls,
            stream,
            max_tokens,
            temperature,
            top_p,
        } = responses_req;
        let reasoning_effort = reasoning
            .as_ref()
            .and_then(|reasoning| reasoning.effort.clone())
            .map(Into::into);
        let (structured_output, text) = split_responses_text(text);

        let mut messages = Vec::new();
        if let Some(instructions) = instructions
            .clone()
            .filter(|value| !value.trim().is_empty())
        {
            messages.push(UnifiedMessage {
                role: UnifiedRole::System,
                content: vec![UnifiedContentPart::Text { text: instructions }],
                ..Default::default()
            });
        }

        let mut request_items = Vec::new();
        messages.extend(match input {
            Input::String(text) => vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text { text }],
                ..Default::default()
            }],
            Input::Items(items) => items
                .into_iter()
                .filter_map(|item| match item {
                    ItemField::Message(item) => {
                        let (mut content, annotations, files) =
                            message_content_parts_to_unified(item.content);
                        content.extend(files.iter().filter_map(|file| {
                            if let Some(url) = file.file_url.clone() {
                                Some(UnifiedContentPart::FileUrl {
                                    url,
                                    mime_type: file.mime_type.clone(),
                                    filename: file.filename.clone(),
                                })
                            } else {
                                file.file_id
                                    .clone()
                                    .map(|file_id| UnifiedContentPart::FileId {
                                        file_id,
                                        filename: file.filename.clone(),
                                    })
                            }
                        }));
                        if !content.is_empty() || !annotations.is_empty() {
                            request_items.push(UnifiedItem::Message(UnifiedMessageItem {
                                role: message_role_to_unified(item.role.clone()),
                                content: content.clone(),
                                annotations,
                            }));
                        }
                        request_items.extend(files.into_iter().map(UnifiedItem::FileReference));
                        (!content.is_empty()).then_some(UnifiedMessage {
                            role: message_role_to_unified(item.role),
                            content,
                            ..Default::default()
                        })
                    }
                    ItemField::FunctionCall(call) => {
                        let arguments = parse_validated_function_arguments(&call.arguments);
                        request_items.push(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                            id: call.call_id.clone(),
                            name: call.name.clone(),
                            arguments: arguments.clone(),
                        }));
                        Some(UnifiedMessage {
                            role: UnifiedRole::Assistant,
                            content: vec![UnifiedContentPart::ToolCall(UnifiedToolCall {
                                id: call.call_id,
                                name: call.name,
                                arguments,
                            })],
                            ..Default::default()
                        })
                    }
                    ItemField::FunctionCallOutput(output) => {
                        let typed_output = function_output_payload_to_unified(output.output);
                        request_items.push(UnifiedItem::FunctionCallOutput(
                            UnifiedFunctionCallOutputItem {
                                tool_call_id: output.call_id.clone(),
                                name: None,
                                output: typed_output.clone(),
                            },
                        ));
                        Some(UnifiedMessage {
                            role: UnifiedRole::Tool,
                            content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                                tool_call_id: output.call_id,
                                name: None,
                                output: typed_output,
                            })],
                            ..Default::default()
                        })
                    }
                    ItemField::Reasoning(reasoning) => {
                        let (content, annotations, files) = reasoning_parts_to_unified(reasoning);
                        if !content.is_empty() || !annotations.is_empty() {
                            request_items.push(UnifiedItem::Reasoning(UnifiedReasoningItem {
                                content: content.clone(),
                                annotations,
                            }));
                        }
                        request_items.extend(files.into_iter().map(UnifiedItem::FileReference));
                        (!content.is_empty()).then_some(UnifiedMessage {
                            role: UnifiedRole::Assistant,
                            content,
                            ..Default::default()
                        })
                    }
                    ItemField::Unknown(_) => None,
                })
                .collect(),
        });

        if request_items.is_empty() {
            request_items = messages
                .iter()
                .flat_map(|message| {
                    legacy_content_to_unified_items(message.role.clone(), message.content.clone())
                })
                .collect();
        }

        let tools = tools.map(|items| {
            items
                .into_iter()
                .map(|tool| match tool {
                    Tool::Function(function) => UnifiedTool {
                        type_: "function".to_string(),
                        function: UnifiedFunctionDefinition {
                            name: function.name,
                            description: function.description,
                            parameters: function.parameters.unwrap_or_else(|| json!({})),
                            strict: function.strict,
                        },
                    },
                })
                .collect()
        });

        let tool_choice = tool_choice.map(unified_tool_choice_from_responses);
        let responses_extension = UnifiedResponsesRequestExtension {
            instructions,
            tool_choice: None,
            text_format: text.map(|value| {
                serde_json::to_value(value.format)
                    .expect("Responses text format serialization is structurally infallible")
            }),
            reasoning: reasoning.map(|value| {
                serde_json::to_value(value)
                    .expect("Responses reasoning serialization is structurally infallible")
            }),
            parallel_tool_calls: None,
        };

        UnifiedRequest {
            model: Some(model),
            messages,
            items: request_items,
            tools,
            tool_choice,
            parallel_tool_calls,
            stream: stream.unwrap_or(false),
            temperature,
            max_tokens,
            top_p,
            reasoning_effort,
            structured_output,
            extensions: (!responses_extension.is_empty()).then_some(UnifiedRequestExtensions {
                responses: Some(responses_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}
