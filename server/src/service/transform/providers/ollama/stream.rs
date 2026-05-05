use chrono::Utc;

use super::payload::{OllamaChunkResponse, OllamaMessage};

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::{TransformProtocol, build_stream_diagnostic_sse, unified::*};
use crate::utils::ID_GENERATOR;
use crate::utils::sse::SseEvent;

fn build_ollama_stream_diagnostic(
    context: &mut StreamTransformContext<'_>,
    kind: TransformValueKind,
    context_message: String,
) -> SseEvent {
    build_stream_diagnostic_sse(
        context,
        TransformProtocol::Unified,
        TransformProtocol::Api(LlmApiType::Ollama),
        kind,
        "ollama_stream_encoding",
        context_message,
        None,
        Some(
            "Use an OpenAI, Responses, or Anthropic target when structured tool/reasoning/blob stream events must remain recoverable.".to_string(),
        ),
    )
}

impl From<OllamaChunkResponse> for UnifiedChunkResponse {
    fn from(ollama_chunk: OllamaChunkResponse) -> Self {
        let delta = if let Some(message) = ollama_chunk.message {
            UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![UnifiedContentPartDelta::TextDelta {
                    index: 0,
                    text: message.content,
                }],
            }
        } else {
            UnifiedMessageDelta::default()
        };

        let finish_reason = if ollama_chunk.done {
            ollama_chunk
                .done_reason
                .or_else(|| Some("stop".to_string()))
        } else {
            None
        };

        // Map Ollama's done_reason to unified finish_reason
        let finish_reason = finish_reason.map(|reason| {
            match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(), // Default to stop for other reasons
            }
        });

        let choice = UnifiedChunkChoice {
            index: 0,
            delta,
            finish_reason,
        };

        let usage = if let (Some(prompt_tokens), Some(completion_tokens)) =
            (ollama_chunk.prompt_tokens, ollama_chunk.completion_tokens)
        {
            Some(UnifiedUsage {
                input_tokens: prompt_tokens,
                output_tokens: completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                ..Default::default()
            })
        } else {
            None
        };

        UnifiedChunkResponse {
            id: format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
            model: Some(ollama_chunk.model),
            choices: vec![choice],
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata: None,
            synthetic_metadata: None,
        }
    }
}

impl From<UnifiedChunkResponse> for OllamaChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let choice =
            unified_chunk
                .choices
                .into_iter()
                .next()
                .unwrap_or_else(|| UnifiedChunkChoice {
                    index: 0,
                    delta: UnifiedMessageDelta::default(),
                    finish_reason: None,
                });

        let mut final_content = String::new();
        for part in choice.delta.content {
            if let UnifiedContentPartDelta::TextDelta { text, .. } = part {
                final_content.push_str(&text);
            }
        }

        let message = if !final_content.is_empty() {
            Some(OllamaMessage {
                role: "assistant".to_string(),
                content: final_content,
                images: None,
            })
        } else {
            None
        };

        let (prompt_tokens, completion_tokens) = if let Some(usage) = unified_chunk.usage {
            (Some(usage.input_tokens), Some(usage.output_tokens))
        } else {
            (None, None)
        };

        let done_reason = choice
            .finish_reason
            .as_ref()
            .map(|reason| match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(),
            });

        OllamaChunkResponse {
            model: unified_chunk.model.unwrap_or_default(),
            created_at: Utc::now().to_rfc3339(),
            message,
            done: choice.finish_reason.is_some(),
            done_reason,
            prompt_tokens,
            completion_tokens,
            total_duration: None,
            load_duration: None,
            prompt_eval_duration: None,
            eval_duration: None,
        }
    }
}

pub(crate) fn transform_unified_stream_events_to_ollama_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let mut transformed = Vec::new();

    for event in stream_events {
        let model = context.get_or_default_stream_model();
        let maybe_event = match event {
            UnifiedStreamEvent::ContentBlockDelta { text, .. } => {
                serde_json::to_string(&OllamaChunkResponse {
                    model,
                    created_at: Utc::now().to_rfc3339(),
                    message: Some(OllamaMessage {
                        role: "assistant".to_string(),
                        content: text,
                        images: None,
                    }),
                    done: false,
                    done_reason: None,
                    prompt_tokens: None,
                    completion_tokens: None,
                    total_duration: None,
                    load_duration: None,
                    prompt_eval_duration: None,
                    eval_duration: None,
                })
                .ok()
                .map(|data| SseEvent {
                    data,
                    ..Default::default()
                })
            }
            UnifiedStreamEvent::MessageDelta { finish_reason } => {
                serde_json::to_string(&OllamaChunkResponse {
                    model,
                    created_at: Utc::now().to_rfc3339(),
                    message: None,
                    done: finish_reason.is_some(),
                    done_reason: finish_reason.as_ref().map(|reason| match reason.as_str() {
                        "stop" => "stop".to_string(),
                        "length" => "length".to_string(),
                        _ => "stop".to_string(),
                    }),
                    prompt_tokens: None,
                    completion_tokens: None,
                    total_duration: None,
                    load_duration: None,
                    prompt_eval_duration: None,
                    eval_duration: None,
                })
                .ok()
                .map(|data| SseEvent {
                    data,
                    ..Default::default()
                })
            }
            UnifiedStreamEvent::Usage { usage } => serde_json::to_string(&OllamaChunkResponse {
                model,
                created_at: Utc::now().to_rfc3339(),
                message: None,
                done: false,
                done_reason: None,
                prompt_tokens: Some(usage.input_tokens),
                completion_tokens: Some(usage.output_tokens),
                total_duration: None,
                load_duration: None,
                prompt_eval_duration: None,
                eval_duration: None,
            })
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
            }),
            UnifiedStreamEvent::ToolCallStart { index, id, name } => {
                Some(build_ollama_stream_diagnostic(
                    context,
                    TransformValueKind::ToolCallDelta,
                    format!(
                        "Ollama streaming only exposes plain assistant text chunks; tool_call_start index={index}, id={id}, name={name} was downgraded to a structured transform diagnostic."
                    ),
                ))
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                id,
                name,
                arguments,
                ..
            } => Some(build_ollama_stream_diagnostic(
                context,
                TransformValueKind::ToolCallDelta,
                format!(
                    "Ollama streaming only exposes plain assistant text chunks; tool_call_arguments_delta index={index}, id={id:?}, name={name:?}, chars={} was downgraded to a structured transform diagnostic.",
                    arguments.chars().count()
                ),
            )),
            UnifiedStreamEvent::ToolCallStop { index, id } => Some(build_ollama_stream_diagnostic(
                context,
                TransformValueKind::ToolCallDelta,
                format!(
                    "Ollama streaming only exposes plain assistant text chunks; tool_call_stop index={index}, id={id:?} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::ReasoningStart { index } => Some(build_ollama_stream_diagnostic(
                context,
                TransformValueKind::ReasoningDelta,
                format!(
                    "Ollama streaming does not expose reasoning_start; index={index} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::ReasoningDelta { index, text, .. } => {
                Some(build_ollama_stream_diagnostic(
                    context,
                    TransformValueKind::ReasoningDelta,
                    format!(
                        "Ollama streaming does not expose reasoning deltas; index={index}, chars={} was downgraded to a structured transform diagnostic.",
                        text.chars().count()
                    ),
                ))
            }
            UnifiedStreamEvent::ReasoningStop { index } => Some(build_ollama_stream_diagnostic(
                context,
                TransformValueKind::ReasoningDelta,
                format!(
                    "Ollama streaming does not expose reasoning_stop; index={index} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::BlobDelta { index, data } => Some(build_ollama_stream_diagnostic(
                context,
                TransformValueKind::BlobDelta,
                format!(
                    "Ollama streaming only exposes plain assistant text chunks; blob_delta index={index:?}, json_type={} was downgraded to a structured transform diagnostic.",
                    match &data {
                        serde_json::Value::Null => "null",
                        serde_json::Value::Bool(_) => "bool",
                        serde_json::Value::Number(_) => "number",
                        serde_json::Value::String(_) => "string",
                        serde_json::Value::Array(_) => "array",
                        serde_json::Value::Object(_) => "object",
                    }
                ),
            )),
            UnifiedStreamEvent::ItemAdded { .. }
            | UnifiedStreamEvent::ItemDone { .. }
            | UnifiedStreamEvent::MessageStart { .. }
            | UnifiedStreamEvent::MessageStop
            | UnifiedStreamEvent::ContentPartAdded { .. }
            | UnifiedStreamEvent::ContentPartDone { .. }
            | UnifiedStreamEvent::ContentBlockStart { .. }
            | UnifiedStreamEvent::ContentBlockStop { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
            | UnifiedStreamEvent::Error { .. } => None,
        };

        if let Some(event) = maybe_event {
            transformed.push(event);
        }
    }

    if transformed.is_empty() {
        None
    } else {
        Some(transformed)
    }
}

pub(crate) fn transform_unified_chunk_to_ollama_events(
    unified_chunk: UnifiedChunkResponse,
    _context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    serde_json::to_string(&OllamaChunkResponse::from(unified_chunk))
        .ok()
        .map(|data| {
            vec![SseEvent {
                data,
                ..Default::default()
            }]
        })
}
