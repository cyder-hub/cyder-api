use crate::service::transform::unified::*;

use super::super::payload::*;
use super::compat::*;

pub(in crate::service::transform::providers::responses) fn unified_role_to_message(
    role: UnifiedRole,
) -> MessageRole {
    match role {
        UnifiedRole::User => MessageRole::User,
        UnifiedRole::Assistant | UnifiedRole::Tool => MessageRole::Assistant,
        UnifiedRole::System => MessageRole::System,
    }
}

pub(in crate::service::transform::providers::responses) fn message_role_to_unified(
    role: MessageRole,
) -> UnifiedRole {
    match role {
        MessageRole::User => UnifiedRole::User,
        MessageRole::Assistant => UnifiedRole::Assistant,
        MessageRole::System | MessageRole::Developer => UnifiedRole::System,
    }
}

pub(in crate::service::transform::providers::responses) fn unified_role_to_message_role(
    role: UnifiedRole,
) -> MessageRole {
    match role {
        UnifiedRole::User => MessageRole::User,
        UnifiedRole::Assistant | UnifiedRole::Tool => MessageRole::Assistant,
        UnifiedRole::System => MessageRole::System,
    }
}

pub(in crate::service::transform::providers::responses) fn push_message_item(
    items: &mut Vec<ItemField>,
    role: UnifiedRole,
    buffer: &mut Vec<ItemContentPart>,
) {
    if buffer.is_empty() {
        return;
    }

    items.push(ItemField::Message(Message {
        _type: "message".to_string(),
        id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
        status: MessageStatus::Completed,
        role: unified_role_to_message(role),
        content: std::mem::take(buffer),
    }));
}

pub(in crate::service::transform::providers::responses) fn unified_message_to_responses_input_items(
    message: UnifiedMessage,
) -> Vec<ItemField> {
    let mut items = Vec::new();
    let mut message_buffer = Vec::new();

    for part in message.content {
        match part {
            UnifiedContentPart::Text { text } => {
                message_buffer.push(ItemContentPart::InputText { text });
            }
            UnifiedContentPart::Refusal { text } => {
                message_buffer.push(ItemContentPart::Refusal { refusal: text });
            }
            UnifiedContentPart::Reasoning { text } => {
                push_message_item(&mut items, message.role.clone(), &mut message_buffer);
                items.push(ItemField::Reasoning(ReasoningBody {
                    _type: "reasoning".to_string(),
                    id: format!("rs_{}", crate::utils::ID_GENERATOR.generate_id()),
                    content: None,
                    summary: vec![ItemContentPart::SummaryText { text }],
                    encrypted_content: None,
                }));
            }
            UnifiedContentPart::ImageUrl { url, detail } => {
                message_buffer.push(ItemContentPart::InputImage {
                    image_url: Some(url),
                    detail: detail.unwrap_or_else(|| "auto".to_string()),
                });
            }
            UnifiedContentPart::ImageData { mime_type, data } => {
                message_buffer.push(ItemContentPart::InputImage {
                    image_url: Some(build_data_url(&mime_type, &data)),
                    detail: "auto".to_string(),
                });
            }
            UnifiedContentPart::FileUrl { url, filename, .. } => {
                message_buffer.push(ItemContentPart::InputFile {
                    filename,
                    file_url: Some(url),
                    file_id: None,
                    file_data: None,
                });
            }
            UnifiedContentPart::FileData {
                data,
                mime_type,
                filename,
            } => {
                message_buffer.push(ItemContentPart::InputFile {
                    filename,
                    file_url: None,
                    file_id: None,
                    file_data: Some(build_data_url(&mime_type, &data)),
                });
            }
            UnifiedContentPart::ExecutableCode { language, code } => {
                message_buffer.push(ItemContentPart::InputText {
                    text: render_executable_code_text(&language, &code),
                });
            }
            UnifiedContentPart::ToolCall(call) => {
                push_message_item(&mut items, message.role.clone(), &mut message_buffer);
                items.push(ItemField::FunctionCall(FunctionCall {
                    _type: "function_call".to_string(),
                    id: format!("fc_{}", crate::utils::ID_GENERATOR.generate_id()),
                    call_id: call.id,
                    name: call.name,
                    arguments: stringify_function_arguments(call.arguments),
                    status: MessageStatus::Completed,
                }));
            }
            UnifiedContentPart::ToolResult(result) => {
                push_message_item(&mut items, message.role.clone(), &mut message_buffer);
                items.push(ItemField::FunctionCallOutput(FunctionCallOutput {
                    _type: "function_call_output".to_string(),
                    id: format!("fco_{}", crate::utils::ID_GENERATOR.generate_id()),
                    call_id: result.tool_call_id,
                    output: unified_tool_result_to_function_output_payload(result.output),
                    status: MessageStatus::Completed,
                }));
            }
        }
    }

    push_message_item(&mut items, message.role, &mut message_buffer);
    items
}

pub(in crate::service::transform::providers::responses) fn unified_reasoning_part_to_responses_part(
    part: UnifiedContentPart,
) -> ItemContentPart {
    match part {
        UnifiedContentPart::Reasoning { text } => ItemContentPart::ReasoningText { text },
        UnifiedContentPart::Text { text } => ItemContentPart::Text { text },
        UnifiedContentPart::Refusal { text } => ItemContentPart::Refusal { refusal: text },
        UnifiedContentPart::ImageUrl { url, detail } => ItemContentPart::InputImage {
            image_url: Some(url),
            detail: detail.unwrap_or_else(|| "auto".to_string()),
        },
        UnifiedContentPart::ImageData { mime_type, data } => ItemContentPart::InputImage {
            image_url: Some(build_data_url(&mime_type, &data)),
            detail: "auto".to_string(),
        },
        UnifiedContentPart::FileUrl { url, filename, .. } => ItemContentPart::InputFile {
            filename,
            file_url: Some(url),
            file_id: None,
            file_data: None,
        },
        UnifiedContentPart::FileData {
            data,
            mime_type,
            filename,
        } => ItemContentPart::InputFile {
            filename,
            file_url: None,
            file_id: None,
            file_data: Some(build_data_url(&mime_type, &data)),
        },
        UnifiedContentPart::ExecutableCode { language, code } => ItemContentPart::Text {
            text: render_executable_code_text(&language, &code),
        },
        UnifiedContentPart::ToolCall(call) => ItemContentPart::Text {
            text: format!(
                "Tool call {} ({}) with arguments {}",
                call.id, call.name, call.arguments
            ),
        },
        UnifiedContentPart::ToolResult(result) => ItemContentPart::Text {
            text: format!(
                "Tool result {} {}",
                result.tool_call_id,
                result.legacy_content()
            ),
        },
    }
}

pub(in crate::service::transform::providers::responses) fn responses_annotations_to_unified(
    annotations: Vec<Annotation>,
    part_index: u32,
) -> Vec<UnifiedAnnotation> {
    annotations
        .into_iter()
        .map(|annotation| match annotation {
            Annotation::UrlCitation {
                url,
                start_index,
                end_index,
                title,
            } => UnifiedAnnotation::Citation(UnifiedCitation {
                part_index: Some(part_index),
                start_index: Some(start_index),
                end_index: Some(end_index),
                url: Some(url),
                title: Some(title),
                license: None,
            }),
        })
        .collect()
}

pub(in crate::service::transform::providers::responses) fn unified_annotations_to_responses(
    annotations: &[UnifiedAnnotation],
    part_index: u32,
) -> Vec<Annotation> {
    annotations
        .iter()
        .filter_map(|annotation| match annotation {
            UnifiedAnnotation::Citation(citation)
                if citation.part_index.is_none() || citation.part_index == Some(part_index) =>
            {
                Some(Annotation::UrlCitation {
                    url: citation.url.clone().unwrap_or_default(),
                    start_index: citation.start_index.unwrap_or_default(),
                    end_index: citation.end_index.unwrap_or_default(),
                    title: citation.title.clone().unwrap_or_default(),
                })
            }
            _ => None,
        })
        .collect()
}

pub(in crate::service::transform::providers::responses) fn message_content_parts_to_unified(
    parts: Vec<ItemContentPart>,
) -> (
    Vec<UnifiedContentPart>,
    Vec<UnifiedAnnotation>,
    Vec<UnifiedFileReferenceItem>,
) {
    let mut content = Vec::new();
    let mut annotations = Vec::new();
    let mut files = Vec::new();

    for part in parts {
        match part {
            ItemContentPart::InputText { text } | ItemContentPart::Text { text } => {
                content.push(UnifiedContentPart::Text { text });
            }
            ItemContentPart::OutputText {
                text,
                annotations: part_annotations,
                ..
            } => {
                let part_index = content.len() as u32;
                content.push(UnifiedContentPart::Text { text });
                annotations.extend(responses_annotations_to_unified(
                    part_annotations,
                    part_index,
                ));
            }
            ItemContentPart::ReasoningText { text } | ItemContentPart::SummaryText { text } => {
                content.push(UnifiedContentPart::Reasoning { text });
            }
            ItemContentPart::Refusal { refusal } => {
                content.push(UnifiedContentPart::Refusal { text: refusal });
            }
            ItemContentPart::InputImage { image_url, detail } => {
                content.push(UnifiedContentPart::ImageUrl {
                    url: image_url.unwrap_or_default(),
                    detail: Some(detail),
                });
            }
            ItemContentPart::InputFile {
                filename,
                file_url,
                file_id,
                file_data,
            } => {
                if let Some(file_data) = file_data {
                    content.push(parse_responses_input_file_data(&file_data, filename));
                } else {
                    files.push(UnifiedFileReferenceItem {
                        filename,
                        mime_type: None,
                        file_url,
                        file_id,
                    });
                }
            }
        }
    }

    (content, annotations, files)
}

pub(in crate::service::transform::providers::responses) fn reasoning_parts_to_unified(
    reasoning: ReasoningBody,
) -> (
    Vec<UnifiedContentPart>,
    Vec<UnifiedAnnotation>,
    Vec<UnifiedFileReferenceItem>,
) {
    let mut content = Vec::new();
    let mut annotations = Vec::new();
    let mut files = Vec::new();

    if let Some(parts) = reasoning.content {
        for part in parts {
            let (mut converted_content, mut converted_annotations, mut converted_files) =
                message_content_parts_to_unified(vec![part]);
            annotations.append(&mut converted_annotations);
            files.append(&mut converted_files);
            content.append(&mut converted_content);
        }
    }

    for part in reasoning.summary {
        let (mut converted_content, mut converted_annotations, mut converted_files) =
            message_content_parts_to_unified(vec![part]);
        annotations.append(&mut converted_annotations);
        files.append(&mut converted_files);
        content.append(&mut converted_content);
    }

    (content, annotations, files)
}

pub(in crate::service::transform::providers::responses) fn flush_message_buffer(
    output: &mut Vec<ItemField>,
    role: UnifiedRole,
    buffer: &mut Vec<ItemContentPart>,
) {
    if buffer.is_empty() {
        return;
    }

    output.push(ItemField::Message(Message {
        _type: "message".to_string(),
        id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
        status: MessageStatus::Completed,
        role: unified_role_to_message(role),
        content: std::mem::take(buffer),
    }));
}

pub(in crate::service::transform::providers::responses) fn push_message_buffer_part(
    buffer: &mut Vec<ItemContentPart>,
    part: UnifiedContentPart,
    annotations: &[UnifiedAnnotation],
    part_index: u32,
) {
    match part {
        UnifiedContentPart::Text { text } => {
            buffer.push(ItemContentPart::OutputText {
                text,
                annotations: unified_annotations_to_responses(annotations, part_index),
                logprobs: None,
            });
        }
        UnifiedContentPart::Refusal { text } => {
            buffer.push(ItemContentPart::Refusal { refusal: text });
        }
        UnifiedContentPart::ImageData { mime_type, data } => {
            buffer.push(ItemContentPart::InputImage {
                image_url: Some(build_data_url(&mime_type, &data)),
                detail: "auto".to_string(),
            });
        }
        UnifiedContentPart::ImageUrl { url, detail } => {
            buffer.push(ItemContentPart::InputImage {
                image_url: Some(url),
                detail: detail.unwrap_or_else(|| "auto".to_string()),
            });
        }
        UnifiedContentPart::FileUrl { url, filename, .. } => {
            buffer.push(ItemContentPart::InputFile {
                filename,
                file_url: Some(url),
                file_id: None,
                file_data: None,
            });
        }
        UnifiedContentPart::FileData {
            data,
            mime_type,
            filename,
        } => {
            buffer.push(ItemContentPart::InputFile {
                filename,
                file_url: None,
                file_id: None,
                file_data: Some(build_data_url(&mime_type, &data)),
            });
        }
        UnifiedContentPart::ExecutableCode { language, code } => {
            buffer.push(ItemContentPart::OutputText {
                text: render_executable_code_text(&language, &code),
                annotations: Vec::new(),
                logprobs: None,
            });
        }
        UnifiedContentPart::Reasoning { .. }
        | UnifiedContentPart::ToolCall(_)
        | UnifiedContentPart::ToolResult(_) => {}
    }
}

pub(in crate::service::transform::providers::responses) fn unified_choice_to_responses_items(
    choice: UnifiedChoice,
) -> Vec<ItemField> {
    let mut output = Vec::new();
    let mut message_buffer = Vec::new();

    if !choice.items.is_empty() {
        for item in choice.items {
            match item {
                UnifiedItem::Message(message) => {
                    for (part_index, part) in message.content.into_iter().enumerate() {
                        push_message_buffer_part(
                            &mut message_buffer,
                            part,
                            &message.annotations,
                            part_index as u32,
                        );
                    }
                }
                UnifiedItem::Reasoning(item) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::Reasoning(ReasoningBody {
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
                    }));
                }
                UnifiedItem::FunctionCall(call) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::FunctionCall(FunctionCall {
                        _type: "function_call".to_string(),
                        id: format!("fc_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: call.id,
                        name: call.name,
                        arguments: stringify_function_arguments(call.arguments),
                        status: MessageStatus::Completed,
                    }));
                }
                UnifiedItem::FunctionCallOutput(result) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::FunctionCallOutput(FunctionCallOutput {
                        _type: "function_call_output".to_string(),
                        id: format!("fco_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: result.tool_call_id,
                        output: unified_tool_result_to_function_output_payload(result.output),
                        status: MessageStatus::Completed,
                    }));
                }
                UnifiedItem::FileReference(file) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::Message(Message {
                        _type: "message".to_string(),
                        id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
                        status: MessageStatus::Completed,
                        role: MessageRole::Assistant,
                        content: vec![ItemContentPart::InputFile {
                            filename: file.filename,
                            file_url: file.file_url,
                            file_id: file.file_id,
                            file_data: None,
                        }],
                    }));
                }
            }
        }
    } else {
        for part in choice.message.content {
            match part {
                UnifiedContentPart::Text { text } => {
                    message_buffer.push(ItemContentPart::OutputText {
                        text,
                        annotations: Vec::new(),
                        logprobs: None,
                    });
                }
                UnifiedContentPart::Refusal { text } => {
                    message_buffer.push(ItemContentPart::Refusal { refusal: text });
                }
                UnifiedContentPart::ImageData { mime_type, data } => {
                    message_buffer.push(ItemContentPart::InputImage {
                        image_url: Some(build_data_url(&mime_type, &data)),
                        detail: "auto".to_string(),
                    });
                }
                UnifiedContentPart::ImageUrl { url, detail } => {
                    message_buffer.push(ItemContentPart::InputImage {
                        image_url: Some(url),
                        detail: detail.unwrap_or_else(|| "auto".to_string()),
                    });
                }
                UnifiedContentPart::FileUrl { url, filename, .. } => {
                    message_buffer.push(ItemContentPart::InputFile {
                        filename,
                        file_url: Some(url),
                        file_id: None,
                        file_data: None,
                    });
                }
                UnifiedContentPart::FileData {
                    data,
                    mime_type,
                    filename,
                } => {
                    message_buffer.push(ItemContentPart::InputFile {
                        filename,
                        file_url: None,
                        file_id: None,
                        file_data: Some(build_data_url(&mime_type, &data)),
                    });
                }
                UnifiedContentPart::Reasoning { text } => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::Reasoning(ReasoningBody {
                        _type: "reasoning".to_string(),
                        id: format!("rs_{}", crate::utils::ID_GENERATOR.generate_id()),
                        content: None,
                        summary: vec![ItemContentPart::SummaryText { text }],
                        encrypted_content: None,
                    }));
                }
                UnifiedContentPart::ToolCall(call) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::FunctionCall(FunctionCall {
                        _type: "function_call".to_string(),
                        id: format!("fc_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: call.id,
                        name: call.name,
                        arguments: stringify_function_arguments(call.arguments),
                        status: MessageStatus::Completed,
                    }));
                }
                UnifiedContentPart::ToolResult(result) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::FunctionCallOutput(FunctionCallOutput {
                        _type: "function_call_output".to_string(),
                        id: format!("fco_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: result.tool_call_id,
                        output: unified_tool_result_to_function_output_payload(result.output),
                        status: MessageStatus::Completed,
                    }));
                }
                UnifiedContentPart::ExecutableCode { language, code } => {
                    message_buffer.push(ItemContentPart::OutputText {
                        text: render_executable_code_text(&language, &code),
                        annotations: Vec::new(),
                        logprobs: None,
                    });
                }
            }
        }
    }

    flush_message_buffer(&mut output, choice.message.role, &mut message_buffer);
    output
}
