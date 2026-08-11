use chrono::Utc;

use super::payload::*;

use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::{
    TransformProtocol, apply_transform_policy, record_stream_diagnostic, unified::*,
};
use crate::utils::sse::SseEvent;

fn build_openai_stream_diagnostic(
    stream_context: &mut StreamTransformContext<'_>,
    kind: TransformValueKind,
) {
    record_stream_diagnostic(
        stream_context,
        TransformProtocol::Unified,
        TransformProtocol::Downstream(DownstreamProtocol::Openai),
        kind,
    )
}

impl From<UnifiedChunkResponse> for OpenAiChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let choices = unified_chunk
            .choices
            .into_iter()
            .map(|choice| {
                let role = choice.delta.role.map(|r| {
                    match r {
                        UnifiedRole::System => "system",
                        UnifiedRole::User => "user",
                        UnifiedRole::Assistant => "assistant",
                        UnifiedRole::Tool => "tool",
                    }
                    .to_string()
                });

                let mut content = String::new();
                let mut tool_calls = Vec::new();

                for part in choice.delta.content {
                    match part {
                        UnifiedContentPartDelta::TextDelta { text, .. } => content.push_str(&text),
                        UnifiedContentPartDelta::ImageDelta { .. } => {
                            apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Downstream(DownstreamProtocol::Openai),
                                TransformValueKind::ImageDelta,
                                "Dropping unsupported image delta from OpenAI stream conversion.",
                            );
                        }
                        UnifiedContentPartDelta::ToolCallDelta(tc) => {
                            tool_calls.push(OpenAiChunkToolCall {
                                index: tc.index,
                                id: tc.id,
                                type_: Some("function".to_string()),
                                function: OpenAiChunkFunction {
                                    name: tc.name,
                                    arguments: tc.arguments,
                                },
                            });
                        }
                    }
                }

                let delta = OpenAiChunkDelta {
                    role,
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content)
                    },
                    reasoning_content: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    refusal: None,
                    name: None,
                };

                OpenAiChunkChoice {
                    index: choice.index,
                    delta,
                    finish_reason: choice.finish_reason,
                    logprobs: None,
                }
            })
            .collect();

        OpenAiChunkResponse {
            id: unified_chunk.id,
            object: unified_chunk
                .object
                .unwrap_or_else(|| "chat.completion.chunk".to_string()),
            created: unified_chunk
                .created
                .unwrap_or_else(|| Utc::now().timestamp()),
            model: unified_chunk
                .model
                .expect("audited OpenAI target chunks must retain a model"),
            system_fingerprint: None,
            choices,
            usage: unified_chunk.usage.map(|u| u.into()),
        }
    }
}

impl From<OpenAiChunkResponse> for UnifiedChunkResponse {
    fn from(openai_chunk: OpenAiChunkResponse) -> Self {
        let choices = openai_chunk
            .choices
            .into_iter()
            .map(|choice| {
                let role = choice.delta.role.map(|r| match r.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    "tool" => UnifiedRole::Tool,
                    _ => UnifiedRole::User,
                });

                let mut content = Vec::new();

                if let Some(text) = choice.delta.content {
                    if !text.is_empty() {
                        // Index 0 for text content for now
                        content.push(UnifiedContentPartDelta::TextDelta { index: 0, text });
                    }
                }

                if let Some(text) = choice.delta.reasoning_content {
                    if !text.is_empty() {
                        content.push(UnifiedContentPartDelta::TextDelta { index: 0, text });
                    }
                }

                if let Some(tool_calls) = choice.delta.tool_calls {
                    for tc in tool_calls {
                        content.push(UnifiedContentPartDelta::ToolCallDelta(
                            UnifiedToolCallDelta {
                                index: tc.index,
                                id: tc.id,
                                name: tc.function.name,
                                arguments: tc.function.arguments,
                            },
                        ));
                    }
                }

                let delta = UnifiedMessageDelta { role, content };

                UnifiedChunkChoice {
                    index: choice.index,
                    delta,
                    finish_reason: choice.finish_reason,
                }
            })
            .collect();

        UnifiedChunkResponse {
            id: openai_chunk.id,
            model: Some(openai_chunk.model),
            choices,
            usage: openai_chunk.usage.map(|u| u.into()),
            created: Some(openai_chunk.created),
            object: Some(openai_chunk.object),
            provider_session_metadata: None,
            synthetic_metadata: None,
        }
    }
}

pub(crate) fn openai_chunk_to_unified_stream_events_with_state(
    openai_chunk: OpenAiChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Vec<UnifiedStreamEvent> {
    let OpenAiChunkResponse {
        id,
        model,
        choices,
        usage,
        ..
    } = openai_chunk;

    let mut events = Vec::with_capacity(choices.len() * 4 + usize::from(usage.is_some()));

    let mut reasoning_open = context.current_reasoning_block_index().is_some();
    let mut text_block_index = context.current_content_block_index();
    let mut reasoning_seen = context.openai_reasoning_seen();
    let mut active_tool_calls = context.openai_active_tool_calls_clone();

    for choice in choices {
        if let Some(role) = choice.delta.role {
            events.push(UnifiedStreamEvent::MessageStart {
                id: Some(id.clone()),
                model: Some(model.clone()),
                role: match role.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    "tool" => UnifiedRole::Tool,
                    _ => UnifiedRole::User,
                },
            });
        }

        if let Some(reasoning_text) = choice.delta.reasoning_content {
            if !reasoning_text.is_empty() {
                if text_block_index.is_some() {
                    apply_transform_policy(
                        TransformProtocol::Upstream(UpstreamProtocol::Openai),
                        TransformProtocol::Unified,
                        TransformValueKind::ReasoningDelta,
                        "Dropping OpenAI reasoning delta that arrived after the text block started.",
                    );
                } else {
                    if !reasoning_open {
                        events.push(UnifiedStreamEvent::ReasoningStart { index: 0 });
                        reasoning_open = true;
                        reasoning_seen = true;
                    }
                    events.push(UnifiedStreamEvent::ReasoningDelta {
                        index: 0,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text: reasoning_text,
                    });
                }
            }
        }

        if let Some(text) = choice.delta.content {
            if !text.is_empty() {
                if reasoning_open {
                    events.push(UnifiedStreamEvent::ReasoningStop { index: 0 });
                    reasoning_open = false;
                }

                let index = if reasoning_seen { 1 } else { 0 };
                if text_block_index != Some(index) {
                    events.push(UnifiedStreamEvent::ContentBlockStart {
                        index,
                        kind: UnifiedBlockKind::Text,
                    });
                    text_block_index = Some(index);
                }
                events.push(UnifiedStreamEvent::ContentBlockDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text,
                });
            }
        }

        if let Some(tool_calls) = choice.delta.tool_calls {
            for tool_call in tool_calls {
                let OpenAiChunkToolCall {
                    index,
                    id,
                    function,
                    ..
                } = tool_call;
                let OpenAiChunkFunction { name, arguments } = function;

                if let (Some(id), Some(name)) = (id.clone(), name.clone()) {
                    active_tool_calls.insert(index, id.clone());
                    events.push(UnifiedStreamEvent::ToolCallStart { index, id, name });
                }

                if let Some(arguments) = arguments {
                    if let Some(id) = id.clone() {
                        active_tool_calls.insert(index, id);
                    }
                    events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        id,
                        name,
                        arguments,
                    });
                }
            }
        }

        if choice.finish_reason.is_some() {
            if reasoning_open {
                events.push(UnifiedStreamEvent::ReasoningStop { index: 0 });
                reasoning_open = false;
            }
            if let Some(index) = text_block_index.take() {
                events.push(UnifiedStreamEvent::ContentBlockStop { index });
            }
            if choice.finish_reason.as_deref() == Some("tool_calls") {
                let mut tool_call_indices: Vec<u32> = active_tool_calls.keys().copied().collect();
                tool_call_indices.sort_unstable();
                for tool_call_index in tool_call_indices {
                    let tool_call_id = active_tool_calls.remove(&tool_call_index);
                    events.push(UnifiedStreamEvent::ToolCallStop {
                        index: tool_call_index,
                        id: tool_call_id,
                    });
                }
            }
            events.push(UnifiedStreamEvent::MessageDelta {
                finish_reason: choice.finish_reason,
            });
        }
    }

    if let Some(usage) = usage {
        events.push(UnifiedStreamEvent::Usage {
            usage: usage.into(),
        });
    }

    events
}

pub(crate) fn try_transform_unified_stream_events_to_openai_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Result<Option<Vec<SseEvent>>, serde_json::Error> {
    let mut transformed = Vec::new();

    for event in stream_events {
        if let Some(event) = transform_unified_stream_event_to_openai_event(event, context)? {
            transformed.push(event);
        }
    }

    if transformed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(transformed))
    }
}

pub(crate) fn transform_unified_stream_event_to_openai_event(
    event: UnifiedStreamEvent,
    context: &mut StreamTransformContext<'_>,
) -> Result<Option<SseEvent>, serde_json::Error> {
    let id = context.get_or_generate_stream_id();
    let model = context.get_or_default_stream_model();
    let created = Utc::now().timestamp();

    match event {
        UnifiedStreamEvent::MessageStart { role, .. } => {
            serde_json::to_string(&OpenAiChunkResponse {
                id,
                object: "chat.completion.chunk".to_string(),
                created,
                model,
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: Some(
                            match role {
                                UnifiedRole::System => "system",
                                UnifiedRole::User => "user",
                                UnifiedRole::Assistant => "assistant",
                                UnifiedRole::Tool => "tool",
                            }
                            .to_string(),
                        ),
                        content: None,
                        reasoning_content: None,
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason: None,
                    logprobs: None,
                }],
                usage: None,
            })
            .map(|data| {
                Some(SseEvent {
                    data,
                    ..Default::default()
                })
            })
        }
        UnifiedStreamEvent::ContentBlockDelta { text, .. } => {
            serde_json::to_string(&OpenAiChunkResponse {
                id,
                object: "chat.completion.chunk".to_string(),
                created,
                model,
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: None,
                        content: Some(text),
                        reasoning_content: None,
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason: None,
                    logprobs: None,
                }],
                usage: None,
            })
            .map(|data| {
                Some(SseEvent {
                    data,
                    ..Default::default()
                })
            })
        }
        UnifiedStreamEvent::ToolCallStart {
            index,
            id: tool_id,
            name,
        } => serde_json::to_string(&OpenAiChunkResponse {
            id,
            object: "chat.completion.chunk".to_string(),
            created,
            model,
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: None,
                    reasoning_content: None,
                    tool_calls: Some(vec![OpenAiChunkToolCall {
                        index,
                        id: Some(tool_id),
                        type_: Some("function".to_string()),
                        function: OpenAiChunkFunction {
                            name: Some(name),
                            arguments: None,
                        },
                    }]),
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        })
        .map(|data| {
            Some(SseEvent {
                data,
                ..Default::default()
            })
        }),
        UnifiedStreamEvent::ToolCallArgumentsDelta {
            index,
            item_index: _,
            item_id: _,
            id: tool_id,
            name,
            arguments,
        } => serde_json::to_string(&OpenAiChunkResponse {
            id,
            object: "chat.completion.chunk".to_string(),
            created,
            model,
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: None,
                    reasoning_content: None,
                    tool_calls: Some(vec![OpenAiChunkToolCall {
                        index,
                        id: tool_id,
                        type_: Some("function".to_string()),
                        function: OpenAiChunkFunction {
                            name,
                            arguments: Some(arguments),
                        },
                    }]),
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        })
        .map(|data| {
            Some(SseEvent {
                data,
                ..Default::default()
            })
        }),
        UnifiedStreamEvent::MessageDelta { finish_reason } => {
            serde_json::to_string(&OpenAiChunkResponse {
                id,
                object: "chat.completion.chunk".to_string(),
                created,
                model,
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: None,
                        content: None,
                        reasoning_content: None,
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason,
                    logprobs: None,
                }],
                usage: None,
            })
            .map(|data| {
                Some(SseEvent {
                    data,
                    ..Default::default()
                })
            })
        }
        UnifiedStreamEvent::Usage { usage } => serde_json::to_string(&OpenAiChunkResponse {
            id,
            object: "chat.completion.chunk".to_string(),
            created,
            model,
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: None,
                    reasoning_content: None,
                    tool_calls: None,
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: Some(usage.into()),
        })
        .map(|data| {
            Some(SseEvent {
                data,
                ..Default::default()
            })
        }),
        UnifiedStreamEvent::ReasoningStart { .. }
        | UnifiedStreamEvent::ReasoningDelta { .. }
        | UnifiedStreamEvent::ReasoningStop { .. } => {
            build_openai_stream_diagnostic(context, TransformValueKind::ReasoningDelta);
            Ok(None)
        }
        UnifiedStreamEvent::BlobDelta { .. } => {
            build_openai_stream_diagnostic(context, TransformValueKind::BlobDelta);
            Ok(None)
        }
        UnifiedStreamEvent::Error { error } => serde_json::to_string(&error).map(|data| {
            Some(SseEvent {
                event: Some("error".to_string()),
                data,
                ..Default::default()
            })
        }),
        UnifiedStreamEvent::ItemAdded { .. }
        | UnifiedStreamEvent::ItemDone { .. }
        | UnifiedStreamEvent::MessageStop
        | UnifiedStreamEvent::ContentPartAdded { .. }
        | UnifiedStreamEvent::ContentPartDone { .. }
        | UnifiedStreamEvent::ContentBlockStart { .. }
        | UnifiedStreamEvent::ContentBlockStop { .. }
        | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
        | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
        | UnifiedStreamEvent::ToolCallStop { .. } => Ok(None),
    }
}

pub(crate) fn try_transform_unified_chunk_to_openai_events(
    mut unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Result<Option<Vec<SseEvent>>, serde_json::Error> {
    let mut events = Vec::new();

    for choice in &mut unified_chunk.choices {
        let mut filtered = Vec::new();
        for part in std::mem::take(&mut choice.delta.content) {
            match part {
                UnifiedContentPartDelta::ImageDelta { .. } => {
                    build_openai_stream_diagnostic(context, TransformValueKind::ImageDelta);
                }
                other => filtered.push(other),
            }
        }
        choice.delta.content = filtered;
    }

    let has_chunk_payload = unified_chunk.usage.is_some()
        || unified_chunk.choices.iter().any(|choice| {
            choice.delta.role.is_some()
                || !choice.delta.content.is_empty()
                || choice.finish_reason.is_some()
        });

    if has_chunk_payload {
        let data = serde_json::to_string(&OpenAiChunkResponse::from(unified_chunk))?;
        events.push(SseEvent {
            data,
            ..Default::default()
        });
    }

    Ok((!events.is_empty()).then_some(events))
}

#[cfg(test)]
pub(crate) fn transform_unified_stream_events_to_openai_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    try_transform_unified_stream_events_to_openai_events(stream_events, context)
        .expect("OpenAI test stream payloads must serialize")
}

#[cfg(test)]
pub(crate) fn transform_unified_chunk_to_openai_events(
    unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    try_transform_unified_chunk_to_openai_events(unified_chunk, context)
        .expect("OpenAI test stream payloads must serialize")
}
