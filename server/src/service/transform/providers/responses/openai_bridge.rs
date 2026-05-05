use serde_json::Value;

use crate::service::transform::providers::openai;
use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::unified::*;
use crate::utils::sse::SseEvent;

use super::payload::*;
use super::response::*;

pub(crate) fn transform_responses_chunk_to_openai_events(
    chunk: ResponsesChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let ResponsesChunkResponse { id, model, event } = chunk;
    let mut events = Vec::new();

    let estimated_events = match &event {
        ResponsesStreamEvent::ResponseCreated { .. }
        | ResponsesStreamEvent::ResponseCompleted { .. }
        | ResponsesStreamEvent::ResponseIncomplete { .. }
        | ResponsesStreamEvent::OutputItemAdded { .. }
        | ResponsesStreamEvent::OutputItemDone { .. }
        | ResponsesStreamEvent::ContentPartAdded { .. }
        | ResponsesStreamEvent::ContentPartDone { .. }
        | ResponsesStreamEvent::ReasoningSummaryPartAdded { .. }
        | ResponsesStreamEvent::ReasoningSummaryPartDone { .. }
        | ResponsesStreamEvent::MessageStart { .. }
        | ResponsesStreamEvent::MessageDelta { .. }
        | ResponsesStreamEvent::ContentBlockDelta { .. }
        | ResponsesStreamEvent::ToolCallStart { .. }
        | ResponsesStreamEvent::ToolCallArgumentsDelta { .. }
        | ResponsesStreamEvent::ToolCallArgumentsDone { .. }
        | ResponsesStreamEvent::ReasoningStart { .. }
        | ResponsesStreamEvent::ReasoningDelta { .. }
        | ResponsesStreamEvent::ReasoningStop { .. }
        | ResponsesStreamEvent::Usage { .. }
        | ResponsesStreamEvent::Blob { .. }
        | ResponsesStreamEvent::Error { .. }
        | ResponsesStreamEvent::Unknown(_) => 1,
        ResponsesStreamEvent::MessageStop
        | ResponsesStreamEvent::ContentBlockStart { .. }
        | ResponsesStreamEvent::ContentBlockStop { .. }
        | ResponsesStreamEvent::ToolCallStop { .. } => 0,
        ResponsesStreamEvent::Item(ItemField::Message(message)) => {
            1 + message
                .content
                .iter()
                .map(|part| match part {
                    ItemContentPart::ReasoningText { .. } => 3,
                    _ => 1,
                })
                .sum::<usize>()
        }
        ResponsesStreamEvent::Item(ItemField::FunctionCall(call)) => {
            2 + usize::from(!call.arguments.is_empty())
        }
        ResponsesStreamEvent::Item(ItemField::FunctionCallOutput(_))
        | ResponsesStreamEvent::Item(ItemField::Unknown(_)) => 1,
        ResponsesStreamEvent::Item(ItemField::Reasoning(reasoning)) => {
            let content_len = reasoning
                .content
                .as_ref()
                .map(|parts| parts.len())
                .unwrap_or(0);
            3 + content_len + reasoning.summary.len()
        }
    };
    events.reserve(estimated_events);

    let mut push_event = |event: UnifiedStreamEvent| {
        context.update_session_from_stream_event(&event);
        if let Some(encoded) =
            openai::transform_unified_stream_event_to_openai_event(event, context)
        {
            events.push(encoded);
        }
    };

    match event {
        ResponsesStreamEvent::ResponseCreated { .. } => {}
        ResponsesStreamEvent::ResponseCompleted { response }
        | ResponsesStreamEvent::ResponseIncomplete { response } => {
            for event in response_terminal_stream_events(response) {
                push_event(event);
            }
        }
        ResponsesStreamEvent::OutputItemAdded { output_index, item } => match item {
            ItemField::Message(_) => {}
            ItemField::FunctionCall(call) => {
                push_event(UnifiedStreamEvent::ToolCallStart {
                    index: output_index,
                    id: call.call_id,
                    name: call.name,
                });
            }
            ItemField::Reasoning(_) => {
                push_event(UnifiedStreamEvent::ReasoningStart {
                    index: output_index,
                });
            }
            ItemField::FunctionCallOutput(output) => {
                push_event(UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: serde_json::to_value(output).unwrap_or(Value::Null),
                });
            }
            ItemField::Unknown(value) => {
                push_event(UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: value,
                });
            }
        },
        ResponsesStreamEvent::OutputItemDone { output_index, item } => match item {
            ItemField::FunctionCall(call) => {
                push_event(UnifiedStreamEvent::ToolCallStop {
                    index: output_index,
                    id: Some(call.call_id),
                });
            }
            ItemField::Reasoning(_) => {
                push_event(UnifiedStreamEvent::ReasoningStop {
                    index: output_index,
                });
            }
            _ => {}
        },
        ResponsesStreamEvent::ContentPartAdded {
            item_id,
            content_index,
        } => {
            push_event(UnifiedStreamEvent::ContentPartAdded {
                item_index: None,
                item_id: Some(item_id),
                part_index: content_index,
                part: None,
            });
        }
        ResponsesStreamEvent::ContentPartDone {
            item_id,
            content_index,
        } => {
            push_event(UnifiedStreamEvent::ContentPartDone {
                item_index: None,
                item_id: Some(item_id),
                part_index: content_index,
            });
        }
        ResponsesStreamEvent::ReasoningSummaryPartAdded {
            item_id,
            summary_index,
        } => {
            push_event(UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index: None,
                item_id: Some(item_id),
                part_index: summary_index,
                part: None,
            });
        }
        ResponsesStreamEvent::ReasoningSummaryPartDone {
            item_id,
            summary_index,
        } => {
            push_event(UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index: None,
                item_id: Some(item_id),
                part_index: summary_index,
            });
        }
        ResponsesStreamEvent::MessageStart { id: event_id, role } => {
            push_event(UnifiedStreamEvent::MessageStart {
                id: event_id.or(Some(id)),
                model: Some(model),
                role,
            });
        }
        ResponsesStreamEvent::MessageDelta { finish_reason } => {
            push_event(UnifiedStreamEvent::MessageDelta { finish_reason });
        }
        ResponsesStreamEvent::MessageStop => {}
        ResponsesStreamEvent::ContentBlockStart { .. } => {}
        ResponsesStreamEvent::ContentBlockDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            push_event(UnifiedStreamEvent::ContentBlockDelta {
                index,
                item_index,
                item_id,
                part_index,
                text,
            });
        }
        ResponsesStreamEvent::ContentBlockStop { .. } => {}
        ResponsesStreamEvent::ToolCallStart { index, id, name } => {
            push_event(UnifiedStreamEvent::ToolCallStart { index, id, name });
        }
        ResponsesStreamEvent::ToolCallArgumentsDelta {
            index,
            item_index,
            item_id,
            id,
            name,
            arguments,
        } => {
            push_event(UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                item_index,
                item_id,
                id,
                name,
                arguments,
            });
        }
        ResponsesStreamEvent::ToolCallArgumentsDone { .. } => {}
        ResponsesStreamEvent::ToolCallStop { .. } => {}
        ResponsesStreamEvent::ReasoningStart { index } => {
            push_event(UnifiedStreamEvent::ReasoningStart { index });
        }
        ResponsesStreamEvent::ReasoningDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            push_event(UnifiedStreamEvent::ReasoningDelta {
                index,
                item_index,
                item_id,
                part_index,
                text,
            });
        }
        ResponsesStreamEvent::ReasoningStop { index } => {
            push_event(UnifiedStreamEvent::ReasoningStop { index });
        }
        ResponsesStreamEvent::Usage { usage } => {
            push_event(UnifiedStreamEvent::Usage { usage });
        }
        ResponsesStreamEvent::Blob { index, data } => {
            push_event(UnifiedStreamEvent::BlobDelta { index, data });
        }
        ResponsesStreamEvent::Error { error } => {
            push_event(UnifiedStreamEvent::Error { error });
        }
        ResponsesStreamEvent::Item(item) => match item {
            ItemField::Message(message) => {
                push_event(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });

                for (index, part) in message.content.into_iter().enumerate() {
                    let index = index as u32;
                    match part {
                        ItemContentPart::InputText { text }
                        | ItemContentPart::OutputText { text, .. }
                        | ItemContentPart::Text { text }
                        | ItemContentPart::SummaryText { text } => {
                            push_event(UnifiedStreamEvent::ContentBlockDelta {
                                index,
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: Some(index),
                                text,
                            });
                        }
                        ItemContentPart::ReasoningText { text } => {
                            push_event(UnifiedStreamEvent::ReasoningStart { index });
                            push_event(UnifiedStreamEvent::ReasoningDelta {
                                index,
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: Some(index),
                                text,
                            });
                            push_event(UnifiedStreamEvent::ReasoningStop { index });
                        }
                        other => {
                            push_event(UnifiedStreamEvent::BlobDelta {
                                index: Some(index),
                                data: serde_json::to_value(other).unwrap_or(Value::Null),
                            });
                        }
                    }
                }
            }
            ItemField::FunctionCall(call) => {
                push_event(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });
                push_event(UnifiedStreamEvent::ToolCallStart {
                    index: 0,
                    id: call.call_id.clone(),
                    name: call.name.clone(),
                });
                if !call.arguments.is_empty() {
                    push_event(UnifiedStreamEvent::ToolCallArgumentsDelta {
                        index: 0,
                        item_index: Some(0),
                        item_id: Some(call.id.clone()),
                        id: Some(call.call_id),
                        name: Some(call.name),
                        arguments: call.arguments,
                    });
                }
            }
            ItemField::FunctionCallOutput(output) => {
                push_event(UnifiedStreamEvent::BlobDelta {
                    index: Some(0),
                    data: serde_json::to_value(output).unwrap_or(Value::Null),
                });
            }
            ItemField::Reasoning(reasoning) => {
                push_event(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });
                push_event(UnifiedStreamEvent::ReasoningStart { index: 0 });
                if let Some(content) = reasoning.content {
                    for (part_index, part) in content.into_iter().enumerate() {
                        if let ItemContentPart::ReasoningText { text }
                        | ItemContentPart::SummaryText { text }
                        | ItemContentPart::Text { text } = part
                        {
                            push_event(UnifiedStreamEvent::ReasoningDelta {
                                index: 0,
                                item_index: Some(0),
                                item_id: Some(reasoning.id.clone()),
                                part_index: Some(part_index as u32),
                                text,
                            });
                        }
                    }
                }
                for (offset, part) in reasoning.summary.into_iter().enumerate() {
                    if let ItemContentPart::ReasoningText { text }
                    | ItemContentPart::SummaryText { text }
                    | ItemContentPart::Text { text } = part
                    {
                        push_event(UnifiedStreamEvent::ReasoningDelta {
                            index: 0,
                            item_index: Some(0),
                            item_id: Some(reasoning.id.clone()),
                            part_index: Some(offset as u32),
                            text,
                        });
                    }
                }
                push_event(UnifiedStreamEvent::ReasoningStop { index: 0 });
            }
            ItemField::Unknown(value) => {
                push_event(UnifiedStreamEvent::BlobDelta {
                    index: None,
                    data: value,
                });
            }
        },
        ResponsesStreamEvent::Unknown(value) => {
            push_event(UnifiedStreamEvent::BlobDelta {
                index: None,
                data: value,
            });
        }
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}
