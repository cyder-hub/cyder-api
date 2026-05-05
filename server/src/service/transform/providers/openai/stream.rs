use chrono::Utc;
use serde_json::Value;

use super::payload::*;

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::{
    TransformProtocol, apply_transform_policy, build_stream_diagnostic_sse, unified::*,
};
use crate::utils::sse::SseEvent;

fn build_openai_stream_diagnostic(
    stream_context: &mut StreamTransformContext<'_>,
    kind: TransformValueKind,
    context_message: String,
) -> SseEvent {
    build_stream_diagnostic_sse(
        stream_context,
        TransformProtocol::Unified,
        TransformProtocol::Api(LlmApiType::Openai),
        kind,
        "openai_stream_encoding",
        context_message,
        None,
        Some(
            "Use a Responses or Anthropic target when structured reasoning/blob stream events must remain recoverable.".to_string(),
        ),
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
                                TransformProtocol::Api(LlmApiType::Openai),
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
            model: unified_chunk.model.unwrap_or_default(),
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
                        TransformProtocol::Api(LlmApiType::Openai),
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

pub(crate) fn transform_unified_stream_events_to_openai_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let mut transformed = Vec::new();

    for event in stream_events {
        if let Some(event) = transform_unified_stream_event_to_openai_event(event, context) {
            transformed.push(event);
        }
    }

    if transformed.is_empty() {
        None
    } else {
        Some(transformed)
    }
}

pub(crate) fn transform_unified_stream_event_to_openai_event(
    event: UnifiedStreamEvent,
    context: &mut StreamTransformContext<'_>,
) -> Option<SseEvent> {
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
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
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
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
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
        .ok()
        .map(|data| SseEvent {
            data,
            ..Default::default()
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
        .ok()
        .map(|data| SseEvent {
            data,
            ..Default::default()
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
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
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
        .ok()
        .map(|data| SseEvent {
            data,
            ..Default::default()
        }),
        UnifiedStreamEvent::ReasoningStart { index } => Some(build_openai_stream_diagnostic(
            context,
            TransformValueKind::ReasoningDelta,
            format!(
                "OpenAI chat completion chunks do not expose a native reasoning_start event; index={index} was downgraded to a structured transform diagnostic."
            ),
        )),
        UnifiedStreamEvent::ReasoningDelta { index, text, .. } => {
            Some(build_openai_stream_diagnostic(
                context,
                TransformValueKind::ReasoningDelta,
                format!(
                    "OpenAI chat completion chunks do not expose a native reasoning delta; index={index}, chars={} was downgraded to a structured transform diagnostic.",
                    text.chars().count()
                ),
            ))
        }
        UnifiedStreamEvent::ReasoningStop { index } => Some(build_openai_stream_diagnostic(
            context,
            TransformValueKind::ReasoningDelta,
            format!(
                "OpenAI chat completion chunks do not expose a native reasoning_stop event; index={index} was downgraded to a structured transform diagnostic."
            ),
        )),
        UnifiedStreamEvent::BlobDelta { index, data } => Some(build_openai_stream_diagnostic(
            context,
            TransformValueKind::BlobDelta,
            format!(
                "OpenAI chat completion chunks do not expose a native blob delta; index={index:?}, json_type={} was downgraded to a structured transform diagnostic.",
                match &data {
                    Value::Null => "null",
                    Value::Bool(_) => "bool",
                    Value::Number(_) => "number",
                    Value::String(_) => "string",
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                }
            ),
        )),
        UnifiedStreamEvent::Error { error } => Some(SseEvent {
            event: Some("error".to_string()),
            data: serde_json::to_string(&error).unwrap_or_else(|_| {
                "{\"type\":\"transform_error\",\"message\":\"serialization failure\"}".to_string()
            }),
            ..Default::default()
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
        | UnifiedStreamEvent::ToolCallStop { .. } => None,
    }
}

pub(crate) fn transform_unified_chunk_to_openai_events(
    mut unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let mut events = Vec::new();

    for choice in &mut unified_chunk.choices {
        let mut filtered = Vec::new();
        for part in std::mem::take(&mut choice.delta.content) {
            match part {
                UnifiedContentPartDelta::ImageDelta { index, url, data } => {
                    events.push(build_openai_stream_diagnostic(
                        context,
                        TransformValueKind::ImageDelta,
                        format!(
                            "OpenAI chat completion chunks do not expose native image deltas; index={index}, has_url={}, has_data={} was downgraded to a structured transform diagnostic.",
                            url.as_ref().is_some_and(|value| !value.is_empty()),
                            data.as_ref().is_some_and(|value| !value.is_empty())
                        ),
                    ));
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
        if let Ok(data) = serde_json::to_string(&OpenAiChunkResponse::from(unified_chunk)) {
            events.push(SseEvent {
                data,
                ..Default::default()
            });
        }
    }

    (!events.is_empty()).then_some(events)
}
