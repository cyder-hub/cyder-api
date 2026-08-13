use serde_json::{Value, json};

use super::payload::*;

use crate::schema::enum_def::DownstreamProtocol;
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::stream::session::try_append_tool_arguments;
use crate::service::transform::{
    AnthropicActiveBlockKind, AnthropicActiveBlockState, TransformProtocol,
    record_stream_diagnostic, unified::*,
};
use crate::utils::sse::SseEvent;

fn anthropic_usage_from_info(usage: &crate::utils::usage::UsageInfo) -> AnthropicUsage {
    let total_input = u32::try_from(usage.input_tokens).unwrap_or_default();
    let cache_read_input_tokens = u32::try_from(usage.cached_tokens).unwrap_or_default();
    let cache_creation_input_tokens = u32::try_from(usage.cache_write_tokens).unwrap_or_default();
    AnthropicUsage {
        input_tokens: total_input
            .saturating_sub(cache_read_input_tokens)
            .saturating_sub(cache_creation_input_tokens),
        output_tokens: u32::try_from(usage.output_tokens).unwrap_or_default(),
        cache_read_input_tokens,
        cache_creation_input_tokens,
    }
}

fn build_anthropic_stream_diagnostic(
    context: &mut StreamTransformContext<'_>,
    kind: TransformValueKind,
) {
    record_stream_diagnostic(
        context,
        TransformProtocol::Unified,
        TransformProtocol::Downstream(DownstreamProtocol::Anthropic),
        kind,
    )
}

fn anthropic_start_block_event(
    index: u32,
    content_block: AnthropicContentBlock,
) -> Result<SseEvent, serde_json::Error> {
    let event = json!({
        "type": "content_block_start",
        "index": index,
        "content_block": content_block,
    });
    Ok(SseEvent {
        event: Some("content_block_start".to_string()),
        data: serde_json::to_string(&event)?,
        ..Default::default()
    })
}

fn anthropic_block_delta_event(
    index: u32,
    delta: AnthropicContentDelta,
) -> Result<SseEvent, serde_json::Error> {
    let event = json!({
        "type": "content_block_delta",
        "index": index,
        "delta": delta,
    });
    Ok(SseEvent {
        event: Some("content_block_delta".to_string()),
        data: serde_json::to_string(&event)?,
        ..Default::default()
    })
}

fn anthropic_block_stop_event(index: u32) -> Result<SseEvent, serde_json::Error> {
    let event = json!({
        "type": "content_block_stop",
        "index": index,
    });
    Ok(SseEvent {
        event: Some("content_block_stop".to_string()),
        data: serde_json::to_string(&event)?,
        ..Default::default()
    })
}

fn close_active_anthropic_blocks(
    context: &mut StreamTransformContext<'_>,
    events: &mut Vec<SseEvent>,
) -> Result<(), serde_json::Error> {
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
            events.push(anthropic_block_stop_event(index)?);
        }
    }
    Ok(())
}

pub(crate) fn try_transform_unified_stream_events_to_anthropic_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Result<Option<Vec<SseEvent>>, serde_json::Error> {
    let mut events = Vec::new();

    for stream_event in stream_events {
        match stream_event {
            UnifiedStreamEvent::MessageStart { id, model, .. } => {
                if !context.anthropic_message_started() {
                    context.mark_anthropic_message_started();
                    let mut message = json!({
                        "id": id.unwrap_or_else(|| context.get_or_generate_stream_id()),
                        "type": "message",
                        "role": "assistant",
                        "content": [],
                        "model": model
                            .filter(|model| !model.is_empty())
                            .unwrap_or_else(|| context.get_or_default_stream_model()),
                    });
                    if let Some(usage) = context.usage_cache() {
                        message["usage"] = json!(anthropic_usage_from_info(usage));
                    }
                    let event = json!({
                        "type": "message_start",
                        "message": message
                    });
                    events.push(SseEvent {
                        event: Some("message_start".to_string()),
                        data: serde_json::to_string(&event)?,
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
                    )?);
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
                            signature: None,
                        },
                    )?);
                }
            },
            UnifiedStreamEvent::ContentBlockDelta { index, text, .. }
            | UnifiedStreamEvent::RefusalDelta { index, text, .. } => {
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
                    )?);
                }
                events.push(anthropic_block_delta_event(
                    index,
                    AnthropicContentDelta::TextDelta { text },
                )?);
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
                    events.push(anthropic_block_stop_event(index)?);
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
                )?);
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
                            id: block
                                .tool_call_id
                                .clone()
                                .expect("audited Anthropic synthetic tool start must retain an id"),
                            name: block.tool_name.clone().expect(
                                "audited Anthropic synthetic tool start must retain a name",
                            ),
                            input: Value::Object(Default::default()),
                        },
                    )?);
                }
                try_append_tool_arguments(&mut block.text, &arguments);
                events.push(anthropic_block_delta_event(
                    index,
                    AnthropicContentDelta::InputJsonDelta {
                        partial_json: arguments,
                    },
                )?);
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
                    events.push(anthropic_block_stop_event(index)?);
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
                        signature: None,
                    },
                )?);
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
                            signature: None,
                        },
                    )?);
                }
                events.push(anthropic_block_delta_event(
                    index,
                    AnthropicContentDelta::ThinkingDelta { thinking: text },
                )?);
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
                    events.push(anthropic_block_stop_event(index)?);
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
                    )?);
                }
            }
            UnifiedStreamEvent::Usage { usage } => {
                context.set_usage(usage);
            }
            UnifiedStreamEvent::MessageDelta { finish_reason } => {
                if let Some(finish_reason) = finish_reason {
                    close_active_anthropic_blocks(context, &mut events)?;
                    let mut event = json!({
                        "type": "message_delta",
                        "delta": {
                            "stop_reason": crate::service::transform::unified::map_openai_finish_reason_to_anthropic(&finish_reason),
                            "stop_sequence": null,
                        }
                    });
                    if let Some(usage) = context.usage_cache() {
                        event["usage"] = json!(anthropic_usage_from_info(usage));
                    }
                    events.push(SseEvent {
                        event: Some("message_delta".to_string()),
                        data: serde_json::to_string(&event)?,
                        ..Default::default()
                    });
                }
            }
            UnifiedStreamEvent::MessageStop => {
                close_active_anthropic_blocks(context, &mut events)?;
                events.push(SseEvent {
                    event: Some("message_stop".to_string()),
                    data: "{\"type\":\"message_stop\"}".to_string(),
                    ..Default::default()
                });
            }
            UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartDone { .. } => {
                build_anthropic_stream_diagnostic(context, TransformValueKind::ReasoningDelta);
            }
            UnifiedStreamEvent::BlobDelta { .. } => {
                build_anthropic_stream_diagnostic(context, TransformValueKind::BlobDelta);
            }
            UnifiedStreamEvent::ItemAdded { .. }
            | UnifiedStreamEvent::ItemDone { .. }
            | UnifiedStreamEvent::ContentPartAdded { .. }
            | UnifiedStreamEvent::ContentPartDone { .. } => {}
            UnifiedStreamEvent::Error { .. } => {}
        }
    }

    Ok((!events.is_empty()).then_some(events))
}

pub(crate) fn try_transform_unified_chunk_to_anthropic_events(
    unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Result<Option<Vec<SseEvent>>, serde_json::Error> {
    let mut stream_events = Vec::new();
    let message_id = unified_chunk.id.clone();
    let message_model = unified_chunk.model.clone();

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
                UnifiedContentPartDelta::ReasoningDelta { index, text } => {
                    stream_events.push(UnifiedStreamEvent::ReasoningDelta {
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
                UnifiedContentPartDelta::ImageDelta { .. } => {
                    build_anthropic_stream_diagnostic(context, TransformValueKind::ImageDelta);
                }
            }
        }

        if let Some(finish_reason) = choice.finish_reason.clone() {
            if !context.anthropic_message_started()
                && !stream_events
                    .iter()
                    .any(|event| matches!(event, UnifiedStreamEvent::MessageStart { .. }))
            {
                stream_events.push(UnifiedStreamEvent::MessageStart {
                    id: Some(message_id.clone()),
                    model: message_model.clone(),
                    role: UnifiedRole::Assistant,
                });
            }
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

    let encoded = try_transform_unified_stream_events_to_anthropic_events(stream_events, context)?
        .unwrap_or_default();

    if encoded.is_empty() {
        Ok(None)
    } else {
        Ok(Some(encoded))
    }
}

#[cfg(test)]
pub(crate) fn transform_unified_stream_events_to_anthropic_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    try_transform_unified_stream_events_to_anthropic_events(stream_events, context)
        .expect("Anthropic test stream payloads must serialize")
}

#[cfg(test)]
pub(crate) fn transform_unified_chunk_to_anthropic_events(
    unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    try_transform_unified_chunk_to_anthropic_events(unified_chunk, context)
        .expect("Anthropic test stream payloads must serialize")
}
