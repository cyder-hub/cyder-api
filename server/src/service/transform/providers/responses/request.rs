use serde_json::{Value, json};

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::unified::*;
use crate::service::transform::{TransformProtocol, TransformValueKind, apply_transform_policy};

use super::payload::*;
use super::response::*;

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
                                    part,
                                    UnifiedContentPart::Text { .. }
                                        | UnifiedContentPart::Refusal { .. }
                                        | UnifiedContentPart::Reasoning { .. }
                                ) {
                                    return render_responses_instruction_part(part);
                                }

                                let keep = apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Responses),
                                    TransformValueKind::from(&part),
                                    "Downgrading rich system content to recoverable instruction text during Responses request conversion.",
                                );
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
                                part,
                                UnifiedContentPart::Text { .. }
                                    | UnifiedContentPart::Refusal { .. }
                                    | UnifiedContentPart::Reasoning { .. }
                            ) {
                                return render_responses_instruction_part(part);
                            }

                            let keep = apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Responses),
                                TransformValueKind::from(&part),
                                "Downgrading rich system content to recoverable instruction text during Responses request conversion.",
                            );
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
                        strict: None,
                    })
                })
                .collect()
        });

        let tool_choice = responses_extension
            .tool_choice
            .and_then(|value| serde_json::from_value(value).ok())
            .or_else(|| {
                openai_extension
                    .tool_choice
                    .and_then(convert_openai_tool_choice_to_responses)
            });

        let text = responses_extension
            .text_format
            .and_then(|value| serde_json::from_value(value).ok())
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
            .and_then(|value| serde_json::from_value(value).ok())
            .or_else(|| {
                openai_extension
                    .passthrough
                    .as_ref()
                    .and_then(convert_openai_passthrough_to_responses_reasoning)
            });

        let parallel_tool_calls = responses_extension.parallel_tool_calls.or_else(|| {
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
                        let (content, annotations, files) =
                            message_content_parts_to_unified(item.content);
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
                        let arguments = parse_function_arguments(&call.arguments);
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
                        },
                    },
                })
                .collect()
        });

        let responses_extension = UnifiedResponsesRequestExtension {
            instructions,
            tool_choice: tool_choice.and_then(|value| serde_json::to_value(value).ok()),
            text_format: text.and_then(|value| serde_json::to_value(value.format).ok()),
            reasoning: reasoning.and_then(|value| serde_json::to_value(value).ok()),
            parallel_tool_calls,
        };

        UnifiedRequest {
            model: Some(model),
            messages,
            items: request_items,
            tools,
            stream: stream.unwrap_or(false),
            temperature,
            max_tokens,
            top_p,
            extensions: (!responses_extension.is_empty()).then_some(UnifiedRequestExtensions {
                responses: Some(responses_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}
