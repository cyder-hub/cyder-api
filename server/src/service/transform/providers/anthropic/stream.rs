use serde_json::{Value, json};

use super::payload::*;

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::{
    AnthropicActiveBlockKind, AnthropicActiveBlockState, TransformProtocol,
    build_stream_diagnostic_sse, unified::*,
};
use crate::utils::sse::SseEvent;

fn build_anthropic_stream_diagnostic(
    context: &mut StreamTransformContext<'_>,
    kind: TransformValueKind,
    context_message: String,
) -> SseEvent {
    build_stream_diagnostic_sse(
        context,
        TransformProtocol::Unified,
        TransformProtocol::Api(LlmApiType::Anthropic),
        kind,
        "anthropic_stream_encoding",
        context_message,
        None,
        Some(
            "Use Responses or Gemini event-native streaming when multimodal deltas must remain recoverable.".to_string(),
        ),
    )
}

fn anthropic_start_block_event(index: u32, content_block: AnthropicContentBlock) -> SseEvent {
    let event = json!({
        "type": "content_block_start",
        "index": index,
        "content_block": content_block,
    });
    SseEvent {
        event: Some("content_block_start".to_string()),
        data: serde_json::to_string(&event).unwrap(),
        ..Default::default()
    }
}

fn anthropic_block_delta_event(index: u32, delta: AnthropicContentDelta) -> SseEvent {
    let event = json!({
        "type": "content_block_delta",
        "index": index,
        "delta": delta,
    });
    SseEvent {
        event: Some("content_block_delta".to_string()),
        data: serde_json::to_string(&event).unwrap(),
        ..Default::default()
    }
}

fn anthropic_block_stop_event(index: u32) -> SseEvent {
    let event = json!({
        "type": "content_block_stop",
        "index": index,
    });
    SseEvent {
        event: Some("content_block_stop".to_string()),
        data: serde_json::to_string(&event).unwrap(),
        ..Default::default()
    }
}

fn close_active_anthropic_blocks(
    context: &mut StreamTransformContext<'_>,
    events: &mut Vec<SseEvent>,
) {
    let mut active_indices = context
        .anthropic_active_blocks()
        .keys()
        .copied()
        .collect::<Vec<_>>();
    active_indices.sort_unstable();

    for index in active_indices {
        if context
            .anthropic_active_blocks_mut()
            .remove(&index)
            .is_some()
        {
            events.push(anthropic_block_stop_event(index));
        }
    }
}

pub(crate) fn transform_unified_stream_events_to_anthropic_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let mut events = Vec::new();

    for stream_event in stream_events {
        match stream_event {
            UnifiedStreamEvent::MessageStart { id, model, .. } => {
                if !context.anthropic_message_started() {
                    context.mark_anthropic_message_started();
                    let event = json!({
                        "type": "message_start",
                        "message": {
                            "id": id.unwrap_or_else(|| context.get_or_generate_stream_id()),
                            "type": "message",
                            "role": "assistant",
                            "content": [],
                            "model": model.unwrap_or_else(|| context.stream_model_clone().unwrap_or_default()),
                            "usage": AnthropicUsage {
                                input_tokens: context.usage_cache().map(|u| u.input_tokens as u32).unwrap_or(0),
                                output_tokens: context.usage_cache().map(|u| u.output_tokens as u32).unwrap_or(0),
                            }
                        }
                    });
                    events.push(SseEvent {
                        event: Some("message_start".to_string()),
                        data: serde_json::to_string(&event).unwrap(),
                        ..Default::default()
                    });
                }
            }
            UnifiedStreamEvent::ContentBlockStart { index, kind } => match kind {
                UnifiedBlockKind::Text => {
                    context.anthropic_active_blocks_mut().insert(
                        index,
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Text),
                    );
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::Text {
                            text: String::new(),
                        },
                    ));
                }
                UnifiedBlockKind::ToolCall | UnifiedBlockKind::Blob => {}
                UnifiedBlockKind::Reasoning => {
                    context.anthropic_active_blocks_mut().insert(
                        index,
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Thinking),
                    );
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::Thinking {
                            thinking: String::new(),
                            signature: Some(String::new()),
                        },
                    ));
                }
            },
            UnifiedStreamEvent::ContentBlockDelta { index, text, .. } => {
                let block_exists = context.anthropic_active_blocks().contains_key(&index);
                context
                    .anthropic_active_blocks_mut()
                    .entry(index)
                    .or_insert_with(|| {
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Text)
                    })
                    .text
                    .push_str(&text);
                if !block_exists {
                    // This path only synthesizes a start when the upstream stream omitted it.
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::Text {
                            text: String::new(),
                        },
                    ));
                }
                events.push(anthropic_block_delta_event(
                    index,
                    AnthropicContentDelta::TextDelta { text },
                ));
            }
            UnifiedStreamEvent::ContentBlockStop { index } => {
                if matches!(
                    context
                        .anthropic_active_blocks()
                        .get(&index)
                        .map(|block| block.kind),
                    Some(AnthropicActiveBlockKind::Text)
                ) {
                    context.anthropic_active_blocks_mut().remove(&index);
                    events.push(anthropic_block_stop_event(index));
                }
            }
            UnifiedStreamEvent::ToolCallStart { index, id, name } => {
                let block = context
                    .anthropic_active_blocks_mut()
                    .entry(index)
                    .or_insert_with(|| {
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::ToolUse)
                    });
                block.kind = AnthropicActiveBlockKind::ToolUse;
                block.tool_call_id = Some(id.clone());
                block.tool_name = Some(name.clone());
                events.push(anthropic_start_block_event(
                    index,
                    AnthropicContentBlock::ToolUse {
                        id,
                        name,
                        input: Value::Object(Default::default()),
                    },
                ));
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                id,
                name,
                arguments,
                ..
            } => {
                let synthesize_start = !context.anthropic_active_blocks().contains_key(&index);
                let block = context
                    .anthropic_active_blocks_mut()
                    .entry(index)
                    .or_insert_with(|| {
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::ToolUse)
                    });
                if block.tool_call_id.is_none() {
                    block.tool_call_id = id.clone();
                }
                if block.tool_name.is_none() {
                    block.tool_name = name.clone();
                }
                if synthesize_start && block.tool_call_id.is_some() && block.tool_name.is_some() {
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::ToolUse {
                            id: block.tool_call_id.clone().unwrap_or_else(|| {
                                format!("toolu_{}", crate::utils::ID_GENERATOR.generate_id())
                            }),
                            name: block
                                .tool_name
                                .clone()
                                .unwrap_or_else(|| "tool".to_string()),
                            input: Value::Object(Default::default()),
                        },
                    ));
                }
                block.text.push_str(&arguments);
                events.push(anthropic_block_delta_event(
                    index,
                    AnthropicContentDelta::InputJsonDelta {
                        partial_json: arguments,
                    },
                ));
            }
            UnifiedStreamEvent::ToolCallStop { index, .. } => {
                if matches!(
                    context
                        .anthropic_active_blocks()
                        .get(&index)
                        .map(|block| block.kind),
                    Some(AnthropicActiveBlockKind::ToolUse)
                ) {
                    context.anthropic_active_blocks_mut().remove(&index);
                    events.push(anthropic_block_stop_event(index));
                }
            }
            UnifiedStreamEvent::ReasoningStart { index } => {
                context.anthropic_active_blocks_mut().insert(
                    index,
                    AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Thinking),
                );
                events.push(anthropic_start_block_event(
                    index,
                    AnthropicContentBlock::Thinking {
                        thinking: String::new(),
                        signature: Some(String::new()),
                    },
                ));
            }
            UnifiedStreamEvent::ReasoningDelta { index, text, .. } => {
                let block_exists = context.anthropic_active_blocks().contains_key(&index);
                context
                    .anthropic_active_blocks_mut()
                    .entry(index)
                    .or_insert_with(|| {
                        AnthropicActiveBlockState::new(AnthropicActiveBlockKind::Thinking)
                    })
                    .text
                    .push_str(&text);
                if !block_exists {
                    events.push(anthropic_start_block_event(
                        index,
                        AnthropicContentBlock::Thinking {
                            thinking: String::new(),
                            signature: Some(String::new()),
                        },
                    ));
                }
                events.push(anthropic_block_delta_event(
                    index,
                    AnthropicContentDelta::ThinkingDelta { thinking: text },
                ));
            }
            UnifiedStreamEvent::ReasoningStop { index } => {
                if matches!(
                    context
                        .anthropic_active_blocks()
                        .get(&index)
                        .map(|block| block.kind),
                    Some(AnthropicActiveBlockKind::Thinking)
                ) {
                    context.anthropic_active_blocks_mut().remove(&index);
                    events.push(anthropic_block_stop_event(index));
                }
            }
            UnifiedStreamEvent::BlobDelta {
                index: Some(index),
                data,
            } if data.get("provider").and_then(Value::as_str) == Some("anthropic")
                && data.get("type").and_then(Value::as_str) == Some("signature_delta") =>
            {
                if let Some(signature) = data.get("signature").and_then(Value::as_str) {
                    events.push(anthropic_block_delta_event(
                        index,
                        AnthropicContentDelta::SignatureDelta {
                            signature: signature.to_string(),
                        },
                    ));
                }
            }
            UnifiedStreamEvent::Usage { usage } => {
                context.set_usage(usage);
            }
            UnifiedStreamEvent::MessageDelta { finish_reason } => {
                if let Some(finish_reason) = finish_reason {
                    close_active_anthropic_blocks(context, &mut events);
                    let event = json!({
                        "type": "message_delta",
                        "delta": {
                            "stop_reason": crate::service::transform::unified::map_openai_finish_reason_to_anthropic(&finish_reason),
                            "stop_sequence": null,
                        },
                        "usage": context.usage_cache().map(|usage| AnthropicUsage {
                            input_tokens: usage.input_tokens as u32,
                            output_tokens: usage.output_tokens as u32,
                        }).unwrap_or(AnthropicUsage {
                            input_tokens: 0,
                            output_tokens: 0,
                        }),
                    });
                    events.push(SseEvent {
                        event: Some("message_delta".to_string()),
                        data: serde_json::to_string(&event).unwrap(),
                        ..Default::default()
                    });
                }
            }
            UnifiedStreamEvent::MessageStop => {
                close_active_anthropic_blocks(context, &mut events);
                events.push(SseEvent {
                    event: Some("message_stop".to_string()),
                    data: "{\"type\":\"message_stop\"}".to_string(),
                    ..Default::default()
                });
            }
            UnifiedStreamEvent::ReasoningSummaryPartAdded { item_index, .. }
            | UnifiedStreamEvent::ReasoningSummaryPartDone { item_index, .. } => {
                events.push(build_anthropic_stream_diagnostic(
                    context,
                    TransformValueKind::ReasoningDelta,
                    format!(
                        "Anthropic SSE does not expose reasoning summary part lifecycle natively; item_index={item_index:?} was downgraded to a structured transform diagnostic."
                    ),
                ));
            }
            UnifiedStreamEvent::BlobDelta { index, data } => {
                events.push(build_anthropic_stream_diagnostic(
                    context,
                    TransformValueKind::BlobDelta,
                    format!(
                        "Anthropic SSE only preserves provider-native signature deltas; index={index:?}, json_type={} was downgraded to a structured transform diagnostic.",
                        data.get("type").and_then(Value::as_str).unwrap_or("unknown"),
                    ),
                ));
            }
            UnifiedStreamEvent::ItemAdded { .. }
            | UnifiedStreamEvent::ItemDone { .. }
            | UnifiedStreamEvent::ContentPartAdded { .. }
            | UnifiedStreamEvent::ContentPartDone { .. } => {}
            UnifiedStreamEvent::Error { .. } => {}
        }
    }

    (!events.is_empty()).then_some(events)
}

pub fn transform_unified_chunk_to_anthropic_events(
    unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let mut stream_events = Vec::new();
    let mut diagnostics = Vec::new();

    if let Some(usage) = unified_chunk.usage.clone() {
        context.set_usage(usage.clone());
    }

    if let Some(choice) = unified_chunk.choices.first() {
        if let Some(role) = choice.delta.role.clone() {
            stream_events.push(UnifiedStreamEvent::MessageStart {
                id: Some(unified_chunk.id),
                model: unified_chunk.model,
                role,
            });
        }

        for part in &choice.delta.content {
            match part {
                UnifiedContentPartDelta::TextDelta { index, text } => {
                    stream_events.push(UnifiedStreamEvent::ContentBlockDelta {
                        index: *index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text: text.clone(),
                    });
                }
                UnifiedContentPartDelta::ToolCallDelta(tool_delta) => {
                    if let (Some(id), Some(name)) = (tool_delta.id.clone(), tool_delta.name.clone())
                    {
                        stream_events.push(UnifiedStreamEvent::ToolCallStart {
                            index: tool_delta.index,
                            id,
                            name,
                        });
                    }
                    if let Some(arguments) = tool_delta.arguments.clone() {
                        stream_events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                            index: tool_delta.index,
                            item_index: None,
                            item_id: None,
                            id: tool_delta.id.clone(),
                            name: tool_delta.name.clone(),
                            arguments,
                        });
                    }
                }
                UnifiedContentPartDelta::ImageDelta { index, url, data } => {
                    diagnostics.push(build_anthropic_stream_diagnostic(
                        context,
                        TransformValueKind::ImageDelta,
                        format!(
                            "Anthropic SSE content blocks do not expose native image deltas; index={index}, has_url={}, has_data={} was downgraded to a structured transform diagnostic.",
                            url.as_ref().is_some_and(|value| !value.is_empty()),
                            data.as_ref().is_some_and(|value| !value.is_empty())
                        ),
                    ));
                }
            }
        }

        if let Some(finish_reason) = choice.finish_reason.clone() {
            for (index, block) in context.anthropic_active_blocks().clone() {
                match block.kind {
                    AnthropicActiveBlockKind::Text => {
                        stream_events.push(UnifiedStreamEvent::ContentBlockStop { index });
                    }
                    AnthropicActiveBlockKind::ToolUse => {
                        stream_events.push(UnifiedStreamEvent::ToolCallStop {
                            index,
                            id: block.tool_call_id,
                        });
                    }
                    AnthropicActiveBlockKind::Thinking => {
                        stream_events.push(UnifiedStreamEvent::ReasoningStop { index });
                    }
                }
            }
            stream_events.push(UnifiedStreamEvent::MessageDelta {
                finish_reason: Some(finish_reason),
            });
            stream_events.push(UnifiedStreamEvent::MessageStop);
        }
    }

    if !stream_events.iter().any(|event| {
        matches!(
            event,
            UnifiedStreamEvent::MessageStart { .. } | UnifiedStreamEvent::MessageDelta { .. }
        )
    }) {
        if let Some(usage) = unified_chunk.usage {
            stream_events.push(UnifiedStreamEvent::Usage { usage });
        }
    }

    let mut encoded = transform_unified_stream_events_to_anthropic_events(stream_events, context)
        .unwrap_or_default();
    encoded.extend(diagnostics);

    if encoded.is_empty() {
        None
    } else {
        Some(encoded)
    }
}
