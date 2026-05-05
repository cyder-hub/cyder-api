use chrono::Utc;

use super::payload::*;

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::{TransformProtocol, apply_transform_policy, unified::*};

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

        let usage = Some(UnifiedUsage {
            input_tokens: anthropic_res.usage.input_tokens,
            output_tokens: anthropic_res.usage.output_tokens,
            total_tokens: anthropic_res.usage.input_tokens + anthropic_res.usage.output_tokens,
            ..Default::default()
        });

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
                    UnifiedContentPart::ImageUrl { .. }
                    | UnifiedContentPart::ImageData { .. }
                    | UnifiedContentPart::FileUrl { .. }
                    | UnifiedContentPart::FileData { .. }
                    | UnifiedContentPart::ExecutableCode { .. } => {
                        apply_transform_policy(
                            TransformProtocol::Unified,
                            TransformProtocol::Api(LlmApiType::Anthropic),
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
                            TransformProtocol::Api(LlmApiType::Anthropic),
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
                            TransformProtocol::Api(LlmApiType::Anthropic),
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
                        TransformProtocol::Api(LlmApiType::Anthropic),
                        TransformValueKind::ToolResult,
                        "Dropping tool result from Anthropic assistant response conversion.",
                    );
                    vec![]
                }
                UnifiedItem::FileReference(_) => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Api(LlmApiType::Anthropic),
                        TransformValueKind::FileUrl,
                        "Dropping file reference from Anthropic assistant response conversion.",
                    );
                    vec![]
                }
            })
            .collect();

        let stop_reason = choice.finish_reason.map(|reason| {
            crate::service::transform::unified::map_openai_finish_reason_to_anthropic(&reason)
        });

        let usage = unified_res.usage.map_or_else(
            || AnthropicUsage {
                input_tokens: 0,
                output_tokens: 0,
            },
            |u| AnthropicUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
            },
        );

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
