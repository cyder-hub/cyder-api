use chrono::Utc;

use super::payload::*;
use super::request::{
    render_anthropic_executable_code_text, render_anthropic_file_reference_text,
    render_anthropic_image_reference_text, render_anthropic_inline_file_data_text,
};

use crate::schema::enum_def::DownstreamProtocol;
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::{TransformProtocol, apply_transform_policy, unified::*};

fn checked_anthropic_unified_usage(
    input_tokens: u32,
    output_tokens: u32,
    cache_read_input_tokens: u32,
    cache_creation_input_tokens: u32,
) -> Option<UnifiedUsage> {
    let total_input_tokens = input_tokens
        .checked_add(cache_read_input_tokens)?
        .checked_add(cache_creation_input_tokens)?;
    let total_tokens = total_input_tokens.checked_add(output_tokens)?;
    for value in [
        total_input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        total_tokens,
    ] {
        i32::try_from(value).ok()?;
    }
    Some(UnifiedUsage {
        input_tokens: total_input_tokens,
        output_tokens,
        total_tokens,
        cached_tokens: Some(cache_read_input_tokens),
        cache_write_tokens: Some(cache_creation_input_tokens),
        ..Default::default()
    })
}

pub(crate) fn anthropic_usage_to_unified(usage: &AnthropicUsage) -> Option<UnifiedUsage> {
    checked_anthropic_unified_usage(
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_input_tokens,
        usage.cache_creation_input_tokens,
    )
}

pub(crate) fn anthropic_stream_usage_to_unified(
    usage: &AnthropicStreamUsage,
) -> Option<UnifiedUsage> {
    let mut unified = checked_anthropic_unified_usage(
        usage.input_tokens.unwrap_or(0),
        usage.output_tokens.unwrap_or(0),
        usage.cache_read_input_tokens.unwrap_or(0),
        usage.cache_creation_input_tokens.unwrap_or(0),
    )?;
    unified.cached_tokens = usage.cache_read_input_tokens;
    unified.cache_write_tokens = usage.cache_creation_input_tokens;
    Some(unified)
}

impl From<AnthropicResponse> for UnifiedResponse {
    fn from(anthropic_res: AnthropicResponse) -> Self {
        let content: Vec<UnifiedContentPart> = anthropic_res
            .content
            .clone()
            .into_iter()
            .map(|block| match block {
                AnthropicContentBlock::Text { text } => UnifiedContentPart::Text { text },
                AnthropicContentBlock::Thinking { thinking, .. } => {
                    UnifiedContentPart::Reasoning { text: thinking }
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id,
                        name,
                        arguments: input,
                    })
                }
            })
            .collect();
        let items = anthropic_res
            .content
            .into_iter()
            .map(|block| match block {
                AnthropicContentBlock::Text { text } => UnifiedItem::Message(UnifiedMessageItem {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text { text }],
                    annotations: Vec::new(),
                }),
                AnthropicContentBlock::Thinking { thinking, .. } => {
                    UnifiedItem::Reasoning(UnifiedReasoningItem {
                        content: vec![UnifiedContentPart::Reasoning { text: thinking }],
                        annotations: Vec::new(),
                    })
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                        id,
                        name,
                        arguments: input,
                    })
                }
            })
            .collect();

        let message = UnifiedMessage {
            role: UnifiedRole::Assistant,
            content,
            ..Default::default()
        };

        let finish_reason = anthropic_res.stop_reason.map(|reason| {
            crate::service::transform::unified::map_anthropic_finish_reason_to_openai(&reason)
        });

        let choice = UnifiedChoice {
            index: 0,
            message,
            items,
            finish_reason,
            logprobs: None,
        };

        let usage = Some(
            anthropic_usage_to_unified(&anthropic_res.usage)
                .expect("Anthropic usage must be checked before response conversion"),
        );

        UnifiedResponse {
            id: anthropic_res.id,
            model: Some(anthropic_res.model),
            choices: vec![choice],
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: Some(UnifiedProviderResponseMetadata {
                anthropic: Some(UnifiedAnthropicResponseMetadata {
                    provider_type: Some(anthropic_res.type_),
                    role: Some(anthropic_res.role),
                    stop_sequence: anthropic_res.stop_sequence,
                }),
                ..Default::default()
            }),
            synthetic_metadata: None,
        }
    }
}

impl From<UnifiedResponse> for AnthropicResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let choice = unified_res
            .choices
            .into_iter()
            .next()
            .unwrap_or_else(|| UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "".to_string(),
                    }],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: None,
                logprobs: None,
            });

        let content: Vec<AnthropicContentBlock> = choice
            .content_items()
            .into_iter()
            .flat_map(|item| match item {
                UnifiedItem::Message(message) => message.content.into_iter().filter_map(|part| match part {
                    UnifiedContentPart::Text { text } => Some(AnthropicContentBlock::Text { text }),
                    UnifiedContentPart::Refusal { text } => {
                        Some(AnthropicContentBlock::Text { text })
                    }
                    UnifiedContentPart::Reasoning { text } => {
                        Some(AnthropicContentBlock::Text { text })
                    }
                    UnifiedContentPart::ImageUrl { url, detail } => Some(AnthropicContentBlock::Text {
                        text: render_anthropic_image_reference_text(&url, detail.as_deref()),
                    }),
                    UnifiedContentPart::FileUrl { url, mime_type, filename } => Some(AnthropicContentBlock::Text {
                        text: render_anthropic_file_reference_text(&url, mime_type.as_deref(), filename.as_deref()),
                    }),
                    UnifiedContentPart::FileData { data, mime_type, filename } => Some(AnthropicContentBlock::Text {
                        text: render_anthropic_inline_file_data_text(&data, &mime_type, filename.as_deref()),
                    }),
                    UnifiedContentPart::ExecutableCode { language, code } => Some(AnthropicContentBlock::Text {
                        text: render_anthropic_executable_code_text(&language, &code),
                    }),
                    UnifiedContentPart::ImageData { .. }
                    | UnifiedContentPart::AudioData { .. }
                    | UnifiedContentPart::FileId { .. } => {
                        apply_transform_policy(
                            TransformProtocol::Unified,
                            TransformProtocol::Downstream(DownstreamProtocol::Anthropic),
                            TransformValueKind::from(&part),
                            "Dropping unsupported response content from Anthropic conversion.",
                        );
                        None
                    }
                    UnifiedContentPart::ToolCall(call) => Some(AnthropicContentBlock::ToolUse {
                        id: call.id,
                        name: call.name,
                        input: call.arguments,
                    }),
                    UnifiedContentPart::ToolResult(_) => {
                        apply_transform_policy(
                            TransformProtocol::Unified,
                            TransformProtocol::Downstream(DownstreamProtocol::Anthropic),
                            TransformValueKind::ToolResult,
                            "Dropping tool result from Anthropic assistant response conversion.",
                        );
                        None
                    }
                }).collect::<Vec<_>>(),
                UnifiedItem::Reasoning(item) => item.content.into_iter().filter_map(|part| match part {
                    UnifiedContentPart::Reasoning { text }
                    | UnifiedContentPart::Text { text }
                    | UnifiedContentPart::Refusal { text } => {
                        Some(AnthropicContentBlock::Thinking {
                            thinking: text,
                            signature: None,
                        })
                    }
                    other => {
                        apply_transform_policy(
                            TransformProtocol::Unified,
                            TransformProtocol::Downstream(DownstreamProtocol::Anthropic),
                            TransformValueKind::from(&other),
                            "Dropping unsupported reasoning content from Anthropic conversion.",
                        );
                        None
                    }
                }).collect::<Vec<_>>(),
                UnifiedItem::FunctionCall(call) => vec![AnthropicContentBlock::ToolUse {
                    id: call.id,
                    name: call.name,
                    input: call.arguments,
                }],
                UnifiedItem::FunctionCallOutput(_) => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Downstream(DownstreamProtocol::Anthropic),
                        TransformValueKind::ToolResult,
                        "Dropping tool result from Anthropic assistant response conversion.",
                    );
                    vec![]
                }
                UnifiedItem::FileReference(file) => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Downstream(DownstreamProtocol::Anthropic),
                        TransformValueKind::FileUrl,
                        "Dropping file reference from Anthropic assistant response conversion.",
                    );
                    file.file_url
                        .map(|url| AnthropicContentBlock::Text {
                            text: render_anthropic_file_reference_text(
                                &url,
                                file.mime_type.as_deref(),
                                file.filename.as_deref(),
                            ),
                        })
                        .into_iter()
                        .collect()
                }
            })
            .collect();

        let stop_reason = choice.finish_reason.map(|reason| {
            crate::service::transform::unified::map_openai_finish_reason_to_anthropic(&reason)
        });

        let usage = unified_res.usage.map_or_else(AnthropicUsage::default, |u| {
            let cache_read_input_tokens = u.cached_tokens.unwrap_or(0);
            let cache_creation_input_tokens = u.cache_write_tokens.unwrap_or(0);
            let input_tokens = u
                .input_tokens
                .checked_sub(cache_read_input_tokens)
                .and_then(|value| value.checked_sub(cache_creation_input_tokens))
                .unwrap_or(u.input_tokens);
            AnthropicUsage {
                input_tokens,
                output_tokens: u.output_tokens,
                cache_read_input_tokens,
                cache_creation_input_tokens,
            }
        });

        AnthropicResponse {
            id: unified_res.id,
            type_: "message".to_string(),
            role: "assistant".to_string(),
            content,
            model: unified_res.model.unwrap_or_default(),
            stop_reason,
            stop_sequence: None,
            usage,
        }
    }
}
