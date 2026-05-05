use serde_json::{Value, json};

use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::unified::*;
use crate::utils::sse::SseEvent;

use super::lifecycle::*;
use super::payload::*;
use super::response::*;

fn responses_item_id(item: &ItemField) -> Option<String> {
    match item {
        ItemField::Message(item) => Some(item.id.clone()),
        ItemField::FunctionCall(item) => Some(item.id.clone()),
        ItemField::FunctionCallOutput(item) => Some(item.id.clone()),
        ItemField::Reasoning(item) => Some(item.id.clone()),
        ItemField::Unknown(_) => None,
    }
}

fn responses_item_to_unified_item(item: &ItemField) -> Option<UnifiedItem> {
    match item {
        ItemField::Message(message) => {
            let (content, annotations, _) =
                message_content_parts_to_unified(message.content.clone());
            Some(UnifiedItem::Message(UnifiedMessageItem {
                role: message_role_to_unified(message.role.clone()),
                content,
                annotations,
            }))
        }
        ItemField::FunctionCall(call) => Some(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
            id: call.call_id.clone(),
            name: call.name.clone(),
            arguments: parse_function_arguments(&call.arguments),
        })),
        ItemField::FunctionCallOutput(output) => Some(UnifiedItem::FunctionCallOutput(
            UnifiedFunctionCallOutputItem {
                tool_call_id: output.call_id.clone(),
                name: None,
                output: function_output_payload_to_unified(output.output.clone()),
            },
        )),
        ItemField::Reasoning(reasoning) => {
            let (content, annotations, _) = reasoning_parts_to_unified(reasoning.clone());
            Some(UnifiedItem::Reasoning(UnifiedReasoningItem {
                content,
                annotations,
            }))
        }
        ItemField::Unknown(_) => None,
    }
}

fn responses_message_blob_events(
    parts: &[ItemContentPart],
    output_index: u32,
) -> Vec<UnifiedStreamEvent> {
    parts
        .iter()
        .filter_map(|part| match part {
            ItemContentPart::InputImage { .. } | ItemContentPart::InputFile { .. } => {
                Some(UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: serde_json::to_value(part).unwrap_or(Value::Null),
                })
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn responses_chunk_to_unified_stream_events(
    chunk: ResponsesChunkResponse,
) -> Vec<UnifiedStreamEvent> {
    let ResponsesChunkResponse { id, model, event } = chunk;

    let mut events = Vec::new();

    match event {
        ResponsesStreamEvent::ResponseCreated { response } => {
            return vec![UnifiedStreamEvent::MessageStart {
                id: Some(response.id),
                model: Some(response.model),
                role: UnifiedRole::Assistant,
            }];
        }
        ResponsesStreamEvent::ResponseCompleted { response }
        | ResponsesStreamEvent::ResponseIncomplete { response } => {
            return response_terminal_stream_events(response);
        }
        ResponsesStreamEvent::OutputItemAdded { output_index, item } => match item {
            ItemField::Message(message) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::Message(message.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(output_index),
                        item_id: responses_item_id(&ItemField::Message(message.clone())),
                        item,
                    });
                }
                events.extend(responses_message_blob_events(
                    &message.content,
                    output_index,
                ));
                return events;
            }
            ItemField::FunctionCall(call) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCall(call.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(output_index),
                        item_id: Some(call.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::ToolCallStart {
                    index: output_index,
                    id: call.call_id,
                    name: call.name,
                });
                return events;
            }
            ItemField::Reasoning(reasoning) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::Reasoning(reasoning.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(output_index),
                        item_id: Some(reasoning.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::ReasoningStart {
                    index: output_index,
                });
                return events;
            }
            ItemField::FunctionCallOutput(output) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCallOutput(output.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(output_index),
                        item_id: Some(output.id.clone()),
                        item: item.clone(),
                    });
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(output.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: serde_json::to_value(output).unwrap_or(Value::Null),
                });
                return events;
            }
            ItemField::Unknown(value) => {
                return vec![UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: value,
                }];
            }
        },
        ResponsesStreamEvent::OutputItemDone { output_index, item } => match item {
            ItemField::FunctionCall(call) => {
                let mut events = vec![UnifiedStreamEvent::ToolCallStop {
                    index: output_index,
                    id: Some(call.call_id.clone()),
                }];
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCall(call.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(call.id),
                        item,
                    });
                }
                return events;
            }
            ItemField::Reasoning(reasoning) => {
                let mut events = vec![UnifiedStreamEvent::ReasoningStop {
                    index: output_index,
                }];
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::Reasoning(reasoning.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(reasoning.id),
                        item,
                    });
                }
                return events;
            }
            ItemField::Message(message) => {
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::Message(message.clone()))
                {
                    return vec![UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(message.id),
                        item,
                    }];
                }
                return Vec::new();
            }
            ItemField::FunctionCallOutput(output) => {
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCallOutput(output.clone()))
                {
                    return vec![UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(output.id),
                        item,
                    }];
                }
                return Vec::new();
            }
            ItemField::Unknown(_) => return Vec::new(),
        },
        ResponsesStreamEvent::ContentPartAdded {
            item_id,
            content_index,
        } => {
            return vec![UnifiedStreamEvent::ContentPartAdded {
                item_index: None,
                item_id: Some(item_id),
                part_index: content_index,
                part: None,
            }];
        }
        ResponsesStreamEvent::ContentPartDone {
            item_id,
            content_index,
        } => {
            return vec![UnifiedStreamEvent::ContentPartDone {
                item_index: None,
                item_id: Some(item_id),
                part_index: content_index,
            }];
        }
        ResponsesStreamEvent::ReasoningSummaryPartAdded {
            item_id,
            summary_index,
        } => {
            return vec![UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index: None,
                item_id: Some(item_id),
                part_index: summary_index,
                part: None,
            }];
        }
        ResponsesStreamEvent::ReasoningSummaryPartDone {
            item_id,
            summary_index,
        } => {
            return vec![UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index: None,
                item_id: Some(item_id),
                part_index: summary_index,
            }];
        }
        ResponsesStreamEvent::MessageStart { id: event_id, role } => {
            return vec![UnifiedStreamEvent::MessageStart {
                id: event_id.or(Some(id)),
                model: Some(model),
                role,
            }];
        }
        ResponsesStreamEvent::MessageDelta { finish_reason } => {
            return vec![UnifiedStreamEvent::MessageDelta { finish_reason }];
        }
        ResponsesStreamEvent::MessageStop => return vec![UnifiedStreamEvent::MessageStop],
        ResponsesStreamEvent::ContentBlockStart { index, kind } => {
            return vec![UnifiedStreamEvent::ContentBlockStart { index, kind }];
        }
        ResponsesStreamEvent::ContentBlockDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            return vec![UnifiedStreamEvent::ContentBlockDelta {
                index,
                item_index,
                item_id,
                part_index,
                text,
            }];
        }
        ResponsesStreamEvent::ContentBlockStop { index } => {
            return vec![UnifiedStreamEvent::ContentBlockStop { index }];
        }
        ResponsesStreamEvent::ToolCallStart { index, id, name } => {
            return vec![UnifiedStreamEvent::ToolCallStart { index, id, name }];
        }
        ResponsesStreamEvent::ToolCallArgumentsDelta {
            index,
            item_index,
            item_id,
            id,
            name,
            arguments,
        } => {
            return vec![UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                item_index,
                item_id,
                id,
                name,
                arguments,
            }];
        }
        ResponsesStreamEvent::ToolCallArgumentsDone { .. } => {
            return Vec::new();
        }
        ResponsesStreamEvent::ToolCallStop { index, id } => {
            return vec![UnifiedStreamEvent::ToolCallStop { index, id }];
        }
        ResponsesStreamEvent::ReasoningStart { index } => {
            return vec![UnifiedStreamEvent::ReasoningStart { index }];
        }
        ResponsesStreamEvent::ReasoningDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            return vec![UnifiedStreamEvent::ReasoningDelta {
                index,
                item_index,
                item_id,
                part_index,
                text,
            }];
        }
        ResponsesStreamEvent::ReasoningStop { index } => {
            return vec![UnifiedStreamEvent::ReasoningStop { index }];
        }
        ResponsesStreamEvent::Usage { usage } => return vec![UnifiedStreamEvent::Usage { usage }],
        ResponsesStreamEvent::Blob { index, data } => {
            return vec![UnifiedStreamEvent::BlobDelta { index, data }];
        }
        ResponsesStreamEvent::Error { error } => {
            return vec![UnifiedStreamEvent::Error { error }];
        }
        ResponsesStreamEvent::Item(item) => match item {
            ItemField::Message(message) => {
                let message_item =
                    responses_item_to_unified_item(&ItemField::Message(message.clone()));
                if let Some(item) = message_item.clone() {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(0),
                        item_id: Some(message.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });

                for (index, part) in message.content.clone().into_iter().enumerate() {
                    let index = index as u32;
                    match part {
                        ItemContentPart::InputText { text }
                        | ItemContentPart::OutputText { text, .. }
                        | ItemContentPart::Text { text }
                        | ItemContentPart::SummaryText { text } => {
                            events.push(UnifiedStreamEvent::ContentPartAdded {
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: index,
                                part: None,
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStart {
                                index,
                                kind: UnifiedBlockKind::Text,
                            });
                            events.push(UnifiedStreamEvent::ContentBlockDelta {
                                index,
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: Some(index),
                                text,
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStop { index });
                            events.push(UnifiedStreamEvent::ContentPartDone {
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: index,
                            });
                        }
                        ItemContentPart::ReasoningText { text } => {
                            events.push(UnifiedStreamEvent::ReasoningSummaryPartAdded {
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: index,
                                part: None,
                            });
                            events.push(UnifiedStreamEvent::ReasoningStart { index });
                            events.push(UnifiedStreamEvent::ReasoningDelta {
                                index,
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: Some(index),
                                text,
                            });
                            events.push(UnifiedStreamEvent::ReasoningStop { index });
                            events.push(UnifiedStreamEvent::ReasoningSummaryPartDone {
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: index,
                            });
                        }
                        other => {
                            events.push(UnifiedStreamEvent::BlobDelta {
                                index: Some(index),
                                data: serde_json::to_value(other).unwrap_or(Value::Null),
                            });
                        }
                    }
                }
                if let Some(item) = message_item {
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(0),
                        item_id: Some(message.id.clone()),
                        item,
                    });
                }
                return events;
            }
            ItemField::FunctionCall(call) => {
                let function_call_item =
                    responses_item_to_unified_item(&ItemField::FunctionCall(call.clone()));
                if let Some(item) = function_call_item.clone() {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(0),
                        item_id: Some(call.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });
                events.push(UnifiedStreamEvent::ContentBlockStart {
                    index: 0,
                    kind: UnifiedBlockKind::ToolCall,
                });
                events.push(UnifiedStreamEvent::ToolCallStart {
                    index: 0,
                    id: call.call_id.clone(),
                    name: call.name.clone(),
                });
                if !call.arguments.is_empty() {
                    events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                        index: 0,
                        item_index: Some(0),
                        item_id: Some(call.id.clone()),
                        id: Some(call.call_id),
                        name: Some(call.name),
                        arguments: call.arguments,
                    });
                }
                if matches!(call.status, MessageStatus::Completed) {
                    events.push(UnifiedStreamEvent::ToolCallStop { index: 0, id: None });
                    events.push(UnifiedStreamEvent::ContentBlockStop { index: 0 });
                    if let Some(item) = function_call_item {
                        events.push(UnifiedStreamEvent::ItemDone {
                            item_index: Some(0),
                            item_id: Some(call.id.clone()),
                            item,
                        });
                    }
                }
                return events;
            }
            ItemField::FunctionCallOutput(output) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCallOutput(output.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(0),
                        item_id: Some(output.id.clone()),
                        item: item.clone(),
                    });
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(0),
                        item_id: Some(output.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::BlobDelta {
                    index: Some(0),
                    data: serde_json::to_value(output).unwrap_or(Value::Null),
                });
                return events;
            }
            ItemField::Reasoning(reasoning) => {
                let reasoning_item =
                    responses_item_to_unified_item(&ItemField::Reasoning(reasoning.clone()));
                if let Some(item) = reasoning_item.clone() {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(0),
                        item_id: Some(reasoning.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });
                events.push(UnifiedStreamEvent::ReasoningStart { index: 0 });
                let content_len = reasoning
                    .content
                    .as_ref()
                    .map(|parts| parts.len())
                    .unwrap_or(0);
                if let Some(content) = reasoning.content.clone() {
                    for (part_index, part) in content.into_iter().enumerate() {
                        if let ItemContentPart::ReasoningText { text }
                        | ItemContentPart::SummaryText { text }
                        | ItemContentPart::Text { text } = part
                        {
                            events.push(UnifiedStreamEvent::ReasoningSummaryPartAdded {
                                item_index: Some(0),
                                item_id: Some(reasoning.id.clone()),
                                part_index: part_index as u32,
                                part: None,
                            });
                            events.push(UnifiedStreamEvent::ReasoningDelta {
                                index: 0,
                                item_index: Some(0),
                                item_id: Some(reasoning.id.clone()),
                                part_index: Some(part_index as u32),
                                text,
                            });
                            events.push(UnifiedStreamEvent::ReasoningSummaryPartDone {
                                item_index: Some(0),
                                item_id: Some(reasoning.id.clone()),
                                part_index: part_index as u32,
                            });
                        }
                    }
                }
                let base_index = content_len as u32;
                for (offset, part) in reasoning.summary.clone().into_iter().enumerate() {
                    if let ItemContentPart::ReasoningText { text }
                    | ItemContentPart::SummaryText { text }
                    | ItemContentPart::Text { text } = part
                    {
                        let part_index = base_index + offset as u32;
                        events.push(UnifiedStreamEvent::ReasoningSummaryPartAdded {
                            item_index: Some(0),
                            item_id: Some(reasoning.id.clone()),
                            part_index,
                            part: None,
                        });
                        events.push(UnifiedStreamEvent::ReasoningDelta {
                            index: 0,
                            item_index: Some(0),
                            item_id: Some(reasoning.id.clone()),
                            part_index: Some(part_index),
                            text,
                        });
                        events.push(UnifiedStreamEvent::ReasoningSummaryPartDone {
                            item_index: Some(0),
                            item_id: Some(reasoning.id.clone()),
                            part_index,
                        });
                    }
                }
                events.push(UnifiedStreamEvent::ReasoningStop { index: 0 });
                if let Some(item) = reasoning_item {
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(0),
                        item_id: Some(reasoning.id.clone()),
                        item,
                    });
                }
                return events;
            }
            ItemField::Unknown(value) => {
                return vec![UnifiedStreamEvent::BlobDelta {
                    index: None,
                    data: value,
                }];
            }
        },
        ResponsesStreamEvent::Unknown(value) => {
            if let Some(type_name) = value.get("type").and_then(Value::as_str) {
                match type_name {
                    "response.content_part.added" => {
                        return vec![UnifiedStreamEvent::ContentPartAdded {
                            item_index: None,
                            item_id: value
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            part_index: value
                                .get("content_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                            part: None,
                        }];
                    }
                    "response.content_part.done" => {
                        return vec![UnifiedStreamEvent::ContentPartDone {
                            item_index: None,
                            item_id: value
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            part_index: value
                                .get("content_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                        }];
                    }
                    "response.reasoning_summary_part.added" => {
                        return vec![UnifiedStreamEvent::ReasoningSummaryPartAdded {
                            item_index: None,
                            item_id: value
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            part_index: value
                                .get("summary_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                            part: None,
                        }];
                    }
                    "response.reasoning_summary_part.done" => {
                        return vec![UnifiedStreamEvent::ReasoningSummaryPartDone {
                            item_index: None,
                            item_id: value
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            part_index: value
                                .get("summary_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                        }];
                    }
                    _ => {}
                }
            }
            return vec![UnifiedStreamEvent::BlobDelta {
                index: None,
                data: value,
            }];
        }
    }
}

pub(crate) fn transform_unified_stream_events_to_responses_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let mut events = Vec::new();

    for event in stream_events {
        context.update_session_from_stream_event(&event);
        for frame in encode_formal_responses_stream_event(event, context) {
            let frame = finalize_public_responses_stream_frame(frame, context);
            events.push(SseEvent {
                data: serde_json::to_string(&frame).unwrap_or_default(),
                ..Default::default()
            });
        }
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

pub(crate) fn transform_unified_chunk_to_responses_events(
    unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let mut stream_events = Vec::new();

    for choice in unified_chunk.choices {
        if let Some(role) = choice.delta.role {
            stream_events.push(UnifiedStreamEvent::MessageStart {
                id: Some(unified_chunk.id.clone()),
                model: unified_chunk.model.clone(),
                role,
            });
        }

        for part in choice.delta.content {
            match part {
                UnifiedContentPartDelta::TextDelta { index, text } => {
                    stream_events.push(UnifiedStreamEvent::ContentBlockDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text,
                    });
                }
                UnifiedContentPartDelta::ImageDelta { index, url, data } => {
                    stream_events.push(UnifiedStreamEvent::BlobDelta {
                        index: Some(index),
                        data: json!({
                            "type": "image_delta",
                            "url": url,
                            "data": data
                        }),
                    });
                }
                UnifiedContentPartDelta::ToolCallDelta(tool_call) => {
                    if let (Some(id), Some(name)) = (tool_call.id.clone(), tool_call.name.clone()) {
                        stream_events.push(UnifiedStreamEvent::ToolCallStart {
                            index: tool_call.index,
                            id,
                            name,
                        });
                    }
                    if let Some(arguments) = tool_call.arguments {
                        stream_events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                            index: tool_call.index,
                            item_index: None,
                            item_id: None,
                            id: tool_call.id,
                            name: tool_call.name,
                            arguments,
                        });
                    }
                }
            }
        }

        if choice.finish_reason.is_some() {
            stream_events.push(UnifiedStreamEvent::MessageDelta {
                finish_reason: choice.finish_reason,
            });
        }
    }

    if let Some(usage) = unified_chunk.usage {
        stream_events.push(UnifiedStreamEvent::Usage { usage });
    }

    transform_unified_stream_events_to_responses_events(stream_events, context)
}
