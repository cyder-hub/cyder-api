use chrono::Utc;
use serde_json::{Value, json};

use super::payload::*;

use crate::service::transform::{
    AnthropicActiveBlockKind, AnthropicActiveBlockState, AnthropicSessionState, unified::*,
};
use crate::utils::ID_GENERATOR;

fn parse_anthropic_tool_arguments(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(arguments).unwrap_or(Value::String(arguments.to_string()))
    }
}

fn anthropic_signature_blob(index: u32, signature: String) -> UnifiedStreamEvent {
    UnifiedStreamEvent::BlobDelta {
        index: Some(index),
        data: json!({
            "provider": "anthropic",
            "type": "signature_delta",
            "signature": signature,
        }),
    }
}

fn anthropic_start_block_state(
    session: &mut AnthropicSessionState,
    index: u32,
    kind: AnthropicActiveBlockKind,
) -> &mut AnthropicActiveBlockState {
    session
        .active_blocks
        .entry(index)
        .or_insert_with(|| AnthropicActiveBlockState::new(kind))
}

fn anthropic_block_stop_events(
    index: u32,
    state: Option<AnthropicActiveBlockState>,
) -> Vec<UnifiedStreamEvent> {
    match state {
        Some(AnthropicActiveBlockState {
            kind: AnthropicActiveBlockKind::Text,
            text,
            ..
        }) => vec![
            UnifiedStreamEvent::ContentBlockStop { index },
            UnifiedStreamEvent::ContentPartDone {
                item_index: Some(index),
                item_id: None,
                part_index: 0,
            },
            UnifiedStreamEvent::ItemDone {
                item_index: Some(index),
                item_id: None,
                item: UnifiedItem::Message(UnifiedMessageItem {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text { text }],
                    annotations: Vec::new(),
                }),
            },
        ],
        Some(AnthropicActiveBlockState {
            kind: AnthropicActiveBlockKind::ToolUse,
            text,
            tool_call_id,
            tool_name,
        }) => {
            let id = tool_call_id
                .unwrap_or_else(|| format!("toolu_{}", crate::utils::ID_GENERATOR.generate_id()));
            let name = tool_name.unwrap_or_else(|| "tool".to_string());
            vec![
                UnifiedStreamEvent::ToolCallStop {
                    index,
                    id: Some(id.clone()),
                },
                UnifiedStreamEvent::ContentBlockStop { index },
                UnifiedStreamEvent::ItemDone {
                    item_index: Some(index),
                    item_id: Some(id.clone()),
                    item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                        id,
                        name,
                        arguments: parse_anthropic_tool_arguments(&text),
                    }),
                },
            ]
        }
        Some(AnthropicActiveBlockState {
            kind: AnthropicActiveBlockKind::Thinking,
            text,
            ..
        }) => vec![
            UnifiedStreamEvent::ReasoningStop { index },
            UnifiedStreamEvent::ItemDone {
                item_index: Some(index),
                item_id: None,
                item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                    content: vec![UnifiedContentPart::Reasoning { text }],
                    annotations: Vec::new(),
                }),
            },
        ],
        None => vec![UnifiedStreamEvent::ContentBlockStop { index }],
    }
}

impl From<AnthropicEvent> for UnifiedChunkResponse {
    fn from(event: AnthropicEvent) -> Self {
        let mut choice = UnifiedChunkChoice {
            index: 0,
            delta: UnifiedMessageDelta::default(),
            finish_reason: None,
        };
        let (id, model) = match &event {
            AnthropicEvent::MessageStart { message } => (message.id.clone(), message.model.clone()),
            _ => (
                format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
                "anthropic-transformed-model".to_string(),
            ),
        };

        match event {
            AnthropicEvent::MessageStart { .. } => {
                choice.delta.role = Some(UnifiedRole::Assistant);
            }
            AnthropicEvent::ContentBlockDelta { index, delta } => match delta {
                AnthropicContentDelta::TextDelta { text } => {
                    choice
                        .delta
                        .content
                        .push(UnifiedContentPartDelta::TextDelta { index, text });
                }
                AnthropicContentDelta::InputJsonDelta { partial_json } => {
                    choice
                        .delta
                        .content
                        .push(UnifiedContentPartDelta::ToolCallDelta(
                            UnifiedToolCallDelta {
                                index,
                                id: None,
                                name: None,
                                arguments: Some(partial_json),
                            },
                        ));
                }
                AnthropicContentDelta::ThinkingDelta { thinking } => {
                    choice
                        .delta
                        .content
                        .push(UnifiedContentPartDelta::TextDelta {
                            index,
                            text: thinking,
                        });
                }
                AnthropicContentDelta::SignatureDelta { .. } => {}
            },
            AnthropicEvent::MessageDelta { delta, usage } => {
                if let Some(stop_reason) = &delta.stop_reason {
                    choice.finish_reason = Some(
                        crate::service::transform::unified::map_anthropic_finish_reason_to_openai(
                            stop_reason,
                        ),
                    );
                }
                let usage = usage.or(delta.usage).map(|usage| UnifiedUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.input_tokens + usage.output_tokens,
                    ..Default::default()
                });

                return UnifiedChunkResponse {
                    id,
                    model: Some(model),
                    choices: vec![choice],
                    usage,
                    created: Some(Utc::now().timestamp()),
                    object: Some("chat.completion.chunk".to_string()),
                    provider_session_metadata: None,
                    synthetic_metadata: None,
                };
            }
            // Other events don't map to a chunk with content, so we create an empty one.
            _ => {}
        }

        UnifiedChunkResponse {
            id,
            model: Some(model),
            choices: vec![choice],
            usage: None, // Anthropic provides usage at the end, not per chunk
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata: None,
            synthetic_metadata: None,
        }
    }
}

fn anthropic_event_to_unified_stream_events_inner(
    event: AnthropicEvent,
    session: &mut AnthropicSessionState,
) -> Vec<UnifiedStreamEvent> {
    match event {
        AnthropicEvent::MessageStart { message } => {
            let mut events = vec![UnifiedStreamEvent::MessageStart {
                id: Some(message.id),
                model: Some(message.model),
                role: UnifiedRole::Assistant,
            }];

            if let Some(content_blocks) = message.content {
                for (index, block) in content_blocks.into_iter().enumerate() {
                    match block {
                        AnthropicContentBlock::Text { text } => {
                            let index = index as u32;
                            let state = anthropic_start_block_state(
                                session,
                                index,
                                AnthropicActiveBlockKind::Text,
                            );
                            state.text = text.clone();
                            events.push(UnifiedStreamEvent::ItemAdded {
                                item_index: Some(index),
                                item_id: None,
                                item: UnifiedItem::Message(UnifiedMessageItem {
                                    role: UnifiedRole::Assistant,
                                    content: Vec::new(),
                                    annotations: Vec::new(),
                                }),
                            });
                            events.push(UnifiedStreamEvent::ContentPartAdded {
                                item_index: Some(index),
                                item_id: None,
                                part_index: 0,
                                part: Some(UnifiedContentPart::Text { text: text.clone() }),
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStart {
                                index,
                                kind: UnifiedBlockKind::Text,
                            });
                            if !text.is_empty() {
                                events.push(UnifiedStreamEvent::ContentBlockDelta {
                                    index,
                                    item_index: None,
                                    item_id: None,
                                    part_index: None,
                                    text: text.clone(),
                                });
                            }
                            events.push(UnifiedStreamEvent::ContentBlockStop { index });
                            events.push(UnifiedStreamEvent::ContentPartDone {
                                item_index: Some(index),
                                item_id: None,
                                part_index: 0,
                            });
                            events.push(UnifiedStreamEvent::ItemDone {
                                item_index: Some(index),
                                item_id: None,
                                item: UnifiedItem::Message(UnifiedMessageItem {
                                    role: UnifiedRole::Assistant,
                                    content: vec![UnifiedContentPart::Text { text }],
                                    annotations: Vec::new(),
                                }),
                            });
                            session.active_blocks.remove(&index);
                        }
                        AnthropicContentBlock::Thinking {
                            thinking,
                            signature,
                        } => {
                            let index = index as u32;
                            let state = anthropic_start_block_state(
                                session,
                                index,
                                AnthropicActiveBlockKind::Thinking,
                            );
                            state.text = thinking.clone();
                            events.push(UnifiedStreamEvent::ItemAdded {
                                item_index: Some(index),
                                item_id: None,
                                item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                                    content: Vec::new(),
                                    annotations: Vec::new(),
                                }),
                            });
                            events.push(UnifiedStreamEvent::ReasoningStart { index });
                            if !thinking.is_empty() {
                                events.push(UnifiedStreamEvent::ReasoningDelta {
                                    index,
                                    item_index: None,
                                    item_id: None,
                                    part_index: None,
                                    text: thinking,
                                });
                            }
                            if let Some(signature) = signature {
                                events.push(anthropic_signature_blob(index, signature));
                            }
                            events.push(UnifiedStreamEvent::ReasoningStop { index });
                            events.push(UnifiedStreamEvent::ItemDone {
                                item_index: Some(index),
                                item_id: None,
                                item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                                    content: vec![UnifiedContentPart::Reasoning {
                                        text: state.text.clone(),
                                    }],
                                    annotations: Vec::new(),
                                }),
                            });
                            session.active_blocks.remove(&index);
                        }
                        AnthropicContentBlock::ToolUse { id, name, input } => {
                            let index = index as u32;
                            let state = anthropic_start_block_state(
                                session,
                                index,
                                AnthropicActiveBlockKind::ToolUse,
                            );
                            state.tool_call_id = Some(id.clone());
                            state.tool_name = Some(name.clone());
                            state.text = serde_json::to_string(&input).unwrap_or_default();
                            events.push(UnifiedStreamEvent::ItemAdded {
                                item_index: Some(index),
                                item_id: Some(id.clone()),
                                item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments: input.clone(),
                                }),
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStart {
                                index,
                                kind: UnifiedBlockKind::ToolCall,
                            });
                            events.push(UnifiedStreamEvent::ToolCallStart {
                                index,
                                id: id.clone(),
                                name: name.clone(),
                            });
                            let arguments = serde_json::to_string(&input).unwrap_or_default();
                            if !arguments.is_empty() {
                                events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                                    index,
                                    item_index: None,
                                    item_id: None,
                                    id: Some(id.clone()),
                                    name: Some(name.clone()),
                                    arguments,
                                });
                            }
                            events.push(UnifiedStreamEvent::ToolCallStop {
                                index,
                                id: Some(id.clone()),
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStop { index });
                            events.push(UnifiedStreamEvent::ItemDone {
                                item_index: Some(index),
                                item_id: Some(id.clone()),
                                item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                                    id,
                                    name,
                                    arguments: input,
                                }),
                            });
                            session.active_blocks.remove(&index);
                        }
                    }
                }
            }

            events
        }
        AnthropicEvent::ContentBlockStart {
            index,
            content_block,
        } => match content_block {
            AnthropicContentBlock::Text { text } => {
                let state =
                    anthropic_start_block_state(session, index, AnthropicActiveBlockKind::Text);
                state.text = text.clone();
                let mut events = vec![
                    UnifiedStreamEvent::ItemAdded {
                        item_index: Some(index),
                        item_id: None,
                        item: UnifiedItem::Message(UnifiedMessageItem {
                            role: UnifiedRole::Assistant,
                            content: Vec::new(),
                            annotations: Vec::new(),
                        }),
                    },
                    UnifiedStreamEvent::ContentPartAdded {
                        item_index: Some(index),
                        item_id: None,
                        part_index: 0,
                        part: Some(UnifiedContentPart::Text { text: text.clone() }),
                    },
                    UnifiedStreamEvent::ContentBlockStart {
                        index,
                        kind: UnifiedBlockKind::Text,
                    },
                ];
                if !text.is_empty() {
                    events.push(UnifiedStreamEvent::ContentBlockDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text,
                    });
                }
                events
            }
            AnthropicContentBlock::Thinking {
                thinking,
                signature,
            } => {
                let state =
                    anthropic_start_block_state(session, index, AnthropicActiveBlockKind::Thinking);
                state.text = thinking.clone();
                let mut events = vec![
                    UnifiedStreamEvent::ItemAdded {
                        item_index: Some(index),
                        item_id: None,
                        item: UnifiedItem::Reasoning(UnifiedReasoningItem {
                            content: Vec::new(),
                            annotations: Vec::new(),
                        }),
                    },
                    UnifiedStreamEvent::ReasoningStart { index },
                ];
                if !thinking.is_empty() {
                    events.push(UnifiedStreamEvent::ReasoningDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text: thinking,
                    });
                }
                if let Some(signature) = signature {
                    events.push(anthropic_signature_blob(index, signature));
                }
                events
            }
            AnthropicContentBlock::ToolUse { id, name, input } => {
                let state =
                    anthropic_start_block_state(session, index, AnthropicActiveBlockKind::ToolUse);
                state.tool_call_id = Some(id.clone());
                state.tool_name = Some(name.clone());
                state.text = serde_json::to_string(&input).unwrap_or_default();
                vec![
                    UnifiedStreamEvent::ItemAdded {
                        item_index: Some(index),
                        item_id: Some(id.clone()),
                        item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: input.clone(),
                        }),
                    },
                    UnifiedStreamEvent::ContentBlockStart {
                        index,
                        kind: UnifiedBlockKind::ToolCall,
                    },
                    UnifiedStreamEvent::ToolCallStart { index, id, name },
                    UnifiedStreamEvent::ToolCallArgumentsDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        id: state.tool_call_id.clone(),
                        name: state.tool_name.clone(),
                        arguments: serde_json::to_string(&input).unwrap_or_default(),
                    },
                ]
            }
        },
        AnthropicEvent::ContentBlockDelta { index, delta } => match delta {
            AnthropicContentDelta::TextDelta { text } => {
                if let Some(block) = session.active_blocks.get_mut(&index) {
                    block.text.push_str(&text);
                }
                vec![UnifiedStreamEvent::ContentBlockDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text,
                }]
            }
            AnthropicContentDelta::InputJsonDelta { partial_json } => {
                if let Some(block) = session.active_blocks.get_mut(&index) {
                    block.text.push_str(&partial_json);
                }
                vec![UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    id: session
                        .active_blocks
                        .get(&index)
                        .and_then(|block| block.tool_call_id.clone()),
                    name: session
                        .active_blocks
                        .get(&index)
                        .and_then(|block| block.tool_name.clone()),
                    arguments: partial_json,
                }]
            }
            AnthropicContentDelta::ThinkingDelta { thinking } => {
                if let Some(block) = session.active_blocks.get_mut(&index) {
                    block.text.push_str(&thinking);
                }
                vec![UnifiedStreamEvent::ReasoningDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: thinking,
                }]
            }
            AnthropicContentDelta::SignatureDelta { signature } => {
                vec![anthropic_signature_blob(index, signature)]
            }
        },
        AnthropicEvent::ContentBlockStop { index } => {
            anthropic_block_stop_events(index, session.active_blocks.remove(&index))
        }
        AnthropicEvent::MessageDelta { delta, usage } => {
            let mut events = Vec::new();
            if delta.stop_reason.is_some() {
                events.push(UnifiedStreamEvent::MessageDelta {
                    finish_reason: delta.stop_reason.as_deref().map(
                        crate::service::transform::unified::map_anthropic_finish_reason_to_openai,
                    ),
                });
            }
            if let Some(usage) = usage.or(delta.usage) {
                events.push(UnifiedStreamEvent::Usage {
                    usage: UnifiedUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        total_tokens: usage.input_tokens + usage.output_tokens,
                        ..Default::default()
                    },
                });
            }
            events
        }
        AnthropicEvent::MessageStop => vec![UnifiedStreamEvent::MessageStop],
        AnthropicEvent::Error { error } => vec![UnifiedStreamEvent::Error { error }],
        AnthropicEvent::Ping => Vec::new(),
    }
}

pub fn anthropic_event_to_unified_stream_events(event: AnthropicEvent) -> Vec<UnifiedStreamEvent> {
    let mut session = AnthropicSessionState::default();
    anthropic_event_to_unified_stream_events_inner(event, &mut session)
}

pub fn anthropic_event_to_unified_stream_events_with_state(
    event: AnthropicEvent,
    session: &mut AnthropicSessionState,
) -> Vec<UnifiedStreamEvent> {
    anthropic_event_to_unified_stream_events_inner(event, session)
}
