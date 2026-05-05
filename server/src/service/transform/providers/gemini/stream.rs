use chrono::Utc;
use serde_json::{Value, json};

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::unified::*;
use crate::service::transform::{TransformProtocol, TransformValueKind, apply_transform_policy};
use crate::utils::sse::SseEvent;

use super::metadata::*;
use super::payload::*;

impl From<UnifiedChunkResponse> for GeminiChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let synthetic_metadata = unified_chunk.synthetic_metadata.clone();
        let gemini_metadata = unified_chunk
            .provider_session_metadata
            .clone()
            .and_then(|metadata| metadata.gemini);
        let candidates = unified_chunk
            .choices
            .into_iter()
            .filter_map(|choice| {
                let candidate_metadata = gemini_metadata
                    .as_ref()
                    .and_then(|metadata| {
                        metadata
                            .candidates
                            .iter()
                            .find(|candidate| candidate.index == choice.index)
                    })
                    .cloned();
                let mut parts = Vec::new();
                let mut role = "model".to_string(); // Default role for Gemini assistant messages

                if let Some(r) = choice.delta.role {
                    role = match r {
                        UnifiedRole::Assistant => "model".to_string(),
                        UnifiedRole::User => "user".to_string(),
                        // System and Tool roles don't map directly to Gemini chunk roles,
                        // so we'll default to model.
                        _ => "model".to_string(),
                    };
                }

                for part in choice.delta.content {
                    match part {
                        UnifiedContentPartDelta::TextDelta { text, .. } => {
                            parts.push(GeminiPart::Text { text });
                        }
                        UnifiedContentPartDelta::ImageDelta { .. } => {
                            apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Gemini),
                                TransformValueKind::ImageDelta,
                                "Dropping unsupported image delta from Gemini stream conversion.",
                            );
                        }
                        UnifiedContentPartDelta::ToolCallDelta(tc) => {
                            // Gemini doesn't stream partial tool calls in the same way,
                            // but we can try to construct a FunctionCall if we have enough info.
                            // For now, we might need to accumulate or simplify.
                            // Assuming we get a complete call or handle it simplified:
                            if let (Some(name), Some(args_str)) = (tc.name, tc.arguments) {
                                if let Ok(args) = serde_json::from_str(&args_str) {
                                    parts.push(GeminiPart::FunctionCall {
                                        function_call: GeminiFunctionCall { name, args },
                                    });
                                }
                            }
                        }
                    }
                }

                let content = if !parts.is_empty() {
                    Some(GeminiResponseContent { role, parts })
                } else {
                    None
                };

                let finish_reason = choice.finish_reason.as_ref().map(|fr| {
                    // Note: Gemini doesn't have a direct "tool_calls" finish reason,
                    // so we map it to "STOP" which is semantically closest
                    crate::service::transform::unified::map_openai_finish_reason_to_gemini(fr)
                });

                if content.is_some() || finish_reason.is_some() {
                    Some(GeminiCandidate {
                        index: Some(choice.index),
                        content,
                        finish_reason,
                        safety_ratings: candidate_metadata.as_ref().and_then(|metadata| {
                            unified_safety_ratings_to_gemini(metadata.safety_ratings.clone())
                        }),
                        token_count: candidate_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.token_count),
                        citation_metadata: candidate_metadata.and_then(|metadata| {
                            unified_citation_metadata_to_gemini(metadata.citation_metadata)
                        }),
                    })
                } else {
                    None
                }
            })
            .collect();

        let usage_metadata = unified_chunk.usage.map(|u| GeminiChunkUsageMetadata {
            prompt_token_count: u.input_tokens,
            candidates_token_count: Some(u.output_tokens),
            total_token_count: u.total_tokens,
        });

        GeminiChunkResponse {
            candidates,
            prompt_feedback: gemini_metadata
                .and_then(|metadata| unified_prompt_feedback_to_gemini(metadata.prompt_feedback)),
            usage_metadata,
            synthetic_metadata,
        }
    }
}

pub(crate) fn transform_unified_stream_events_to_gemini_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let mut transformed = Vec::new();

    for event in stream_events {
        let maybe_event = match event {
            UnifiedStreamEvent::ContentBlockDelta { text, .. } => {
                serde_json::to_string(&GeminiChunkResponse {
                    candidates: vec![GeminiCandidate {
                        index: Some(0),
                        content: Some(GeminiResponseContent {
                            role: "model".to_string(),
                            parts: vec![GeminiPart::Text { text }],
                        }),
                        finish_reason: None,
                        safety_ratings: None,
                        token_count: None,
                        citation_metadata: None,
                    }],
                    prompt_feedback: None,
                    usage_metadata: None,
                    synthetic_metadata: None,
                })
                .ok()
                .map(|data| SseEvent {
                    data,
                    ..Default::default()
                })
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                name: Some(name),
                arguments,
                ..
            } => serde_json::from_str::<Value>(&arguments)
                .ok()
                .and_then(|args| {
                    serde_json::to_string(&GeminiChunkResponse {
                        candidates: vec![GeminiCandidate {
                            index: Some(0),
                            content: Some(GeminiResponseContent {
                                role: "model".to_string(),
                                parts: vec![GeminiPart::FunctionCall {
                                    function_call: GeminiFunctionCall { name, args },
                                }],
                            }),
                            finish_reason: None,
                            safety_ratings: None,
                            token_count: None,
                            citation_metadata: None,
                        }],
                        prompt_feedback: None,
                        usage_metadata: None,
                        synthetic_metadata: None,
                    })
                    .ok()
                    .map(|data| SseEvent {
                        data,
                        ..Default::default()
                    })
                }),
            UnifiedStreamEvent::MessageDelta { finish_reason } => finish_reason
                .map(|reason| {
                    let finish_reason = map_openai_finish_reason_to_gemini(&reason);
                    GeminiChunkResponse {
                        candidates: vec![GeminiCandidate {
                            index: Some(0),
                            content: None,
                            finish_reason: Some(finish_reason),
                            safety_ratings: None,
                            token_count: None,
                            citation_metadata: None,
                        }],
                        prompt_feedback: None,
                        usage_metadata: None,
                        synthetic_metadata: None,
                    }
                })
                .and_then(|chunk| {
                    serde_json::to_string(&chunk).ok().map(|data| SseEvent {
                        data,
                        ..Default::default()
                    })
                }),
            UnifiedStreamEvent::Usage { usage } => serde_json::to_string(&GeminiChunkResponse {
                candidates: vec![GeminiCandidate {
                    index: Some(0),
                    content: None,
                    finish_reason: None,
                    safety_ratings: None,
                    token_count: None,
                    citation_metadata: None,
                }],
                prompt_feedback: None,
                usage_metadata: Some(GeminiChunkUsageMetadata {
                    prompt_token_count: usage.input_tokens,
                    candidates_token_count: Some(usage.output_tokens),
                    total_token_count: usage.total_tokens,
                }),
                synthetic_metadata: None,
            })
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
            }),
            UnifiedStreamEvent::ReasoningStart { index } => Some(build_gemini_stream_diagnostic(
                context,
                TransformValueKind::ReasoningDelta,
                format!(
                    "Gemini chunk candidates do not expose a native reasoning_start event; index={index} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::ReasoningDelta { index, text, .. } => {
                Some(build_gemini_stream_diagnostic(
                    context,
                    TransformValueKind::ReasoningDelta,
                    format!(
                        "Gemini chunk candidates do not expose a native reasoning delta; index={index}, chars={} was downgraded to a structured transform diagnostic.",
                        text.chars().count()
                    ),
                ))
            }
            UnifiedStreamEvent::ReasoningStop { index } => Some(build_gemini_stream_diagnostic(
                context,
                TransformValueKind::ReasoningDelta,
                format!(
                    "Gemini chunk candidates do not expose a native reasoning_stop event; index={index} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::BlobDelta { index, data } => {
                if let Some(inline_data) = gemini_inline_data_from_blob(&data) {
                    serde_json::to_string(&GeminiChunkResponse {
                        candidates: vec![GeminiCandidate {
                            index: index.or(Some(0)),
                            content: Some(GeminiResponseContent {
                                role: "model".to_string(),
                                parts: vec![GeminiPart::InlineData { inline_data }],
                            }),
                            finish_reason: None,
                            safety_ratings: None,
                            token_count: None,
                            citation_metadata: None,
                        }],
                        prompt_feedback: None,
                        usage_metadata: None,
                        synthetic_metadata: None,
                    })
                    .ok()
                    .map(|data| SseEvent {
                        data,
                        ..Default::default()
                    })
                } else {
                    Some(build_gemini_stream_diagnostic(
                        context,
                        TransformValueKind::BlobDelta,
                        format!(
                            "Gemini stream encoding only preserves blob deltas that carry inline data fields; index={index:?} was downgraded to a structured transform diagnostic."
                        ),
                    ))
                }
            }
            UnifiedStreamEvent::ItemAdded { .. }
            | UnifiedStreamEvent::ItemDone { .. }
            | UnifiedStreamEvent::MessageStart { .. }
            | UnifiedStreamEvent::MessageStop
            | UnifiedStreamEvent::ContentPartAdded { .. }
            | UnifiedStreamEvent::ContentPartDone { .. }
            | UnifiedStreamEvent::ContentBlockStart { .. }
            | UnifiedStreamEvent::ContentBlockStop { .. }
            | UnifiedStreamEvent::ToolCallStart { .. }
            | UnifiedStreamEvent::ToolCallArgumentsDelta { name: None, .. }
            | UnifiedStreamEvent::ToolCallStop { .. }
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

pub(crate) fn transform_unified_chunk_to_gemini_events(
    mut unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    let mut events = Vec::new();

    for choice in &mut unified_chunk.choices {
        let mut filtered = Vec::new();
        for part in std::mem::take(&mut choice.delta.content) {
            match part {
                UnifiedContentPartDelta::ImageDelta { index, url, data } => {
                    events.push(build_gemini_stream_diagnostic(
                        context,
                        TransformValueKind::ImageDelta,
                        format!(
                            "Gemini chunk candidates cannot faithfully encode legacy image deltas without inline mime metadata; index={index}, has_url={}, has_data={} was downgraded to a structured transform diagnostic.",
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
        let value = serde_json::to_value(GeminiChunkResponse::from(unified_chunk)).ok()?;
        if value
            .get("candidates")
            .and_then(|c| c.as_array())
            .is_some_and(|candidates| !candidates.is_empty())
        {
            events.push(SseEvent {
                data: serde_json::to_string(&value).unwrap_or_default(),
                ..Default::default()
            });
        }
    }

    (!events.is_empty()).then_some(events)
}

impl From<GeminiChunkResponse> for UnifiedChunkResponse {
    fn from(gemini_chunk: GeminiChunkResponse) -> Self {
        let GeminiChunkResponse {
            candidates,
            prompt_feedback,
            usage_metadata,
            synthetic_metadata,
        } = gemini_chunk;

        let provider_session_metadata = build_gemini_session_metadata(prompt_feedback, &candidates);

        let choices = candidates
            .into_iter()
            .map(|candidate| {
                let mut delta = UnifiedMessageDelta::default();
                let mut has_function_call = false;

                if let Some(content) = candidate.content {
                    delta.role = Some(match content.role.as_str() {
                        "model" => UnifiedRole::Assistant,
                        "user" => UnifiedRole::User,
                        _ => UnifiedRole::User,
                    });

                    // Track indices separately for different content types
                    let mut text_index = 0;
                    let mut tool_call_index = 0;
                    let mut image_index = 0;

                    for part in content.parts {
                        match part {
                            GeminiPart::Text { text } => {
                                delta.content.push(UnifiedContentPartDelta::TextDelta {
                                    index: text_index,
                                    text,
                                });
                                text_index += 1;
                            }
                            GeminiPart::InlineData { inline_data } => {
                                delta.content.push(UnifiedContentPartDelta::ImageDelta {
                                    index: image_index,
                                    url: None,
                                    data: Some(inline_data.data),
                                });
                                image_index += 1;
                            }
                            GeminiPart::FileData { .. } => {
                                // File data doesn't map well to delta, skip for now
                            }
                            GeminiPart::ExecutableCode { executable_code } => {
                                has_function_call = true;
                                delta.content.push(UnifiedContentPartDelta::ToolCallDelta(
                                    UnifiedToolCallDelta {
                                        index: tool_call_index,
                                        id: None,
                                        name: Some("code_interpreter".to_string()),
                                        arguments: Some(
                                            json!({
                                                "language": executable_code.language,
                                                "code": executable_code.code,
                                            })
                                            .to_string(),
                                        ),
                                    },
                                ));
                                tool_call_index += 1;
                            }
                            GeminiPart::FunctionCall { function_call } => {
                                has_function_call = true;
                                delta.content.push(UnifiedContentPartDelta::ToolCallDelta(
                                    UnifiedToolCallDelta {
                                        index: tool_call_index,
                                        id: None,
                                        name: Some(function_call.name),
                                        arguments: Some(function_call.args.to_string()),
                                    },
                                ));
                                tool_call_index += 1;
                            }
                            _ => {}
                        }
                    }
                }

                let finish_reason = candidate.finish_reason.map(|fr| {
                    crate::service::transform::unified::map_gemini_finish_reason_to_openai(
                        &fr,
                        has_function_call,
                    )
                });

                UnifiedChunkChoice {
                    index: candidate.index.unwrap_or(0),
                    delta,
                    finish_reason,
                }
            })
            .collect();

        let usage = usage_metadata.map(|u| UnifiedUsage {
            input_tokens: u.prompt_token_count,
            output_tokens: u.candidates_token_count.unwrap_or(0),
            total_tokens: u.total_token_count,
            ..Default::default()
        });

        let synthetic_id = true;
        let synthetic_model = false;

        UnifiedChunkResponse {
            // Gemini chunks don't carry top-level id/model fields.
            id: build_gemini_synthetic_response_id("chunk"),
            model: None,
            choices,
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata,
            synthetic_metadata: merge_gemini_synthetic_metadata(
                synthetic_metadata,
                build_gemini_synthetic_metadata(synthetic_id, synthetic_model, false),
            ),
        }
    }
}
