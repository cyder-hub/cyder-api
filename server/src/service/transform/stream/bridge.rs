use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::stream::StreamTransformer;
use crate::service::transform::unified::*;
use crate::service::transform::{TransformProtocol, apply_transform_policy};

pub(in crate::service::transform) fn bridge_stream_events_to_legacy_chunks(
    transformer: &mut StreamTransformer,
    events: Vec<UnifiedStreamEvent>,
) -> Vec<UnifiedChunkResponse> {
    let mut chunks = Vec::new();

    for event in events {
        let id = transformer.get_or_generate_stream_id();
        let model = Some(transformer.get_or_default_stream_model());

        let maybe_chunk = match event {
            UnifiedStreamEvent::MessageStart { role, .. } => Some(UnifiedChunkResponse {
                id,
                model,
                choices: vec![UnifiedChunkChoice {
                    index: 0,
                    delta: UnifiedMessageDelta {
                        role: Some(role),
                        content: vec![],
                    },
                    finish_reason: None,
                }],
                object: Some("chat.completion.chunk".to_string()),
                ..Default::default()
            }),
            UnifiedStreamEvent::ItemAdded { .. } | UnifiedStreamEvent::ItemDone { .. } => None,
            UnifiedStreamEvent::ContentBlockDelta { index, text, .. } => {
                Some(UnifiedChunkResponse {
                    id,
                    model,
                    choices: vec![UnifiedChunkChoice {
                        index: 0,
                        delta: UnifiedMessageDelta {
                            role: None,
                            content: vec![UnifiedContentPartDelta::TextDelta { index, text }],
                        },
                        finish_reason: None,
                    }],
                    object: Some("chat.completion.chunk".to_string()),
                    ..Default::default()
                })
            }
            UnifiedStreamEvent::ToolCallStart {
                index,
                id: tool_id,
                name,
            } => Some(UnifiedChunkResponse {
                id,
                model,
                choices: vec![UnifiedChunkChoice {
                    index: 0,
                    delta: UnifiedMessageDelta {
                        role: None,
                        content: vec![UnifiedContentPartDelta::ToolCallDelta(
                            UnifiedToolCallDelta {
                                index,
                                id: Some(tool_id),
                                name: Some(name),
                                arguments: None,
                            },
                        )],
                    },
                    finish_reason: None,
                }],
                object: Some("chat.completion.chunk".to_string()),
                ..Default::default()
            }),
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                item_index: _,
                item_id: _,
                id: tool_id,
                name,
                arguments,
            } => Some(UnifiedChunkResponse {
                id,
                model,
                choices: vec![UnifiedChunkChoice {
                    index: 0,
                    delta: UnifiedMessageDelta {
                        role: None,
                        content: vec![UnifiedContentPartDelta::ToolCallDelta(
                            UnifiedToolCallDelta {
                                index,
                                id: tool_id,
                                name,
                                arguments: Some(arguments),
                            },
                        )],
                    },
                    finish_reason: None,
                }],
                object: Some("chat.completion.chunk".to_string()),
                ..Default::default()
            }),
            UnifiedStreamEvent::MessageDelta { finish_reason } => Some(UnifiedChunkResponse {
                id,
                model,
                choices: vec![UnifiedChunkChoice {
                    index: 0,
                    delta: UnifiedMessageDelta::default(),
                    finish_reason,
                }],
                object: Some("chat.completion.chunk".to_string()),
                ..Default::default()
            }),
            UnifiedStreamEvent::Usage { usage } => Some(UnifiedChunkResponse {
                id,
                model,
                choices: vec![UnifiedChunkChoice {
                    index: 0,
                    delta: UnifiedMessageDelta::default(),
                    finish_reason: None,
                }],
                usage: Some(usage),
                object: Some("chat.completion.chunk".to_string()),
                ..Default::default()
            }),
            UnifiedStreamEvent::ReasoningStart { .. }
            | UnifiedStreamEvent::ContentPartAdded { .. }
            | UnifiedStreamEvent::ContentPartDone { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
            | UnifiedStreamEvent::ReasoningDelta { .. }
            | UnifiedStreamEvent::ReasoningStop { .. } => {
                apply_transform_policy(
                    TransformProtocol::Unified,
                    TransformProtocol::Api(transformer.target_api_type),
                    TransformValueKind::ReasoningDelta,
                    "Dropping reasoning stream event while bridging to legacy chunk model.",
                );
                None
            }
            UnifiedStreamEvent::BlobDelta { .. } => {
                apply_transform_policy(
                    TransformProtocol::Unified,
                    TransformProtocol::Api(transformer.target_api_type),
                    TransformValueKind::BlobDelta,
                    "Dropping blob stream event while bridging to legacy chunk model.",
                );
                None
            }
            UnifiedStreamEvent::Error { .. } => {
                apply_transform_policy(
                    TransformProtocol::Unified,
                    TransformProtocol::Api(transformer.target_api_type),
                    TransformValueKind::StreamError,
                    "Dropping structured error event while bridging to legacy chunk model.",
                );
                None
            }
            UnifiedStreamEvent::MessageStop
            | UnifiedStreamEvent::ContentBlockStart { .. }
            | UnifiedStreamEvent::ContentBlockStop { .. }
            | UnifiedStreamEvent::ToolCallStop { .. } => None,
        };

        if let Some(mut chunk) = maybe_chunk {
            transformer.normalize_unified_chunk_session_state(&mut chunk);
            chunks.push(chunk);
        }
    }

    chunks
}
