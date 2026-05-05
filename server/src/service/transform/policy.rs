use super::TransformProtocol;
use super::capability::{ProtocolCapabilityMatrix, TransformValueKind};
use crate::schema::enum_def::LlmApiType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformAction {
    Send,
    Drop,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformLossLevel {
    Lossless,
    LossyMinor,
    LossyMajor,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformDiagnosticKind {
    FatalTransformError,
    LossyTransform,
    CapabilityDowngrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicyDecision {
    pub diagnostic_kind: TransformDiagnosticKind,
    pub level: TransformLossLevel,
    pub action: TransformAction,
    pub reason: &'static str,
}

pub(crate) struct PolicyEngine;

impl PolicyEngine {
    fn target_capabilities(target: TransformProtocol) -> Option<ProtocolCapabilityMatrix> {
        match target {
            TransformProtocol::Api(api) => Some(ProtocolCapabilityMatrix::for_api(api)),
            TransformProtocol::Unified => None,
        }
    }

    fn evaluate_capability_matrix(
        target: TransformProtocol,
        kind: TransformValueKind,
    ) -> Option<PolicyDecision> {
        let capabilities = Self::target_capabilities(target)?;

        match kind {
            TransformValueKind::TopKParameter if !capabilities.request.top_k_parameter => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMinor,
                    action: TransformAction::Drop,
                    reason: "The target request capability matrix marks top_k as unsupported.",
                })
            }
            TransformValueKind::ToolDefinitions if !capabilities.request.tool_definitions => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target request capability matrix marks tool definitions as unsupported.",
                })
            }
            TransformValueKind::ToolRoleMessage if !capabilities.request.tool_role_messages => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target request capability matrix marks tool role messages as unsupported.",
                })
            }
            TransformValueKind::ToolCallDelta if !capabilities.stream.tool_call_deltas => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target stream capability matrix marks tool call deltas as unsupported.",
                })
            }
            TransformValueKind::ReasoningDelta if !capabilities.stream.reasoning_deltas => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target stream capability matrix marks reasoning deltas as unsupported.",
                })
            }
            TransformValueKind::BlobDelta if !capabilities.stream.blob_deltas => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target stream capability matrix marks blob deltas as unsupported.",
                })
            }
            TransformValueKind::StreamError if !capabilities.stream.structured_errors => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target stream capability matrix marks structured stream errors as unsupported.",
                })
            }
            TransformValueKind::ReasoningContent if !capabilities.response.reasoning_content => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target response capability matrix marks reasoning content as unsupported.",
                })
            }
            TransformValueKind::Refusal
                if !capabilities.response.refusal || !capabilities.structured_content.refusal =>
            {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target capability matrix marks refusal content as unsupported.",
                })
            }
            _ => None,
        }
    }

    pub(crate) fn evaluate(
        source: TransformProtocol,
        target: TransformProtocol,
        kind: TransformValueKind,
    ) -> PolicyDecision {
        if let Some(decision) = Self::evaluate_capability_matrix(target, kind) {
            return decision;
        }

        match (source, target, kind) {
            (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::ImageUrl)
            | (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::FileUrl)
            | (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::FileData)
            | (
                _,
                TransformProtocol::Api(LlmApiType::Anthropic),
                TransformValueKind::ExecutableCode,
            ) => PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                level: TransformLossLevel::LossyMajor,
                action: TransformAction::Send,
                reason: "Anthropic adapter preserves this request content with native image blocks or recoverable text downgrade.",
            },
            (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::ImageData) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                    level: TransformLossLevel::Lossless,
                    action: TransformAction::Send,
                    reason: "Anthropic adapter can preserve inline image data natively in request blocks.",
                }
            }
            (_, TransformProtocol::Api(LlmApiType::Gemini), TransformValueKind::ImageUrl) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Send,
                    reason: "Gemini adapter preserves remote image URLs as recoverable text when inline bytes are unavailable.",
                }
            }
            (_, TransformProtocol::Api(LlmApiType::Responses), TransformValueKind::ImageData)
            | (_, TransformProtocol::Api(LlmApiType::Responses), TransformValueKind::FileUrl)
            | (_, TransformProtocol::Api(LlmApiType::Responses), TransformValueKind::FileData)
            | (
                _,
                TransformProtocol::Api(LlmApiType::Responses),
                TransformValueKind::ExecutableCode,
            ) => PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                level: TransformLossLevel::LossyMajor,
                action: TransformAction::Send,
                reason: "Responses adapter preserves this content in item inputs or recoverable instruction text.",
            },
            (
                TransformProtocol::Api(LlmApiType::Responses),
                TransformProtocol::Unified,
                TransformValueKind::ResponsesUnknownItem,
            ) => PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                level: TransformLossLevel::LossyMajor,
                action: TransformAction::Drop,
                reason: "Responses item is structured and not formally supported by the current unified response adapter.",
            },
            (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::ImageData)
            | (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::FileUrl)
            | (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::FileData)
            | (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::ExecutableCode) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "OpenAI chat adapter cannot encode this unified content type in this path.",
                }
            }
            (
                _,
                TransformProtocol::Api(LlmApiType::Ollama),
                TransformValueKind::ToolRoleMessage,
            )
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ToolCall)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ToolResult)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ImageUrl)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ImageData)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::FileUrl)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::FileData)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ExecutableCode) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Send,
                    reason: "Ollama adapter preserves structured request content as base64 images plus recoverable plain text.",
                }
            }
            (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::ImageDelta)
            | (_, TransformProtocol::Api(LlmApiType::Gemini), TransformValueKind::ImageDelta)
            | (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::ImageDelta) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target streaming adapter cannot express image deltas yet.",
                }
            }
            _ => PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                level: TransformLossLevel::Lossless,
                action: TransformAction::Send,
                reason: "lossless",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_drop(
        target_api: LlmApiType,
        kind: TransformValueKind,
        expected_level: TransformLossLevel,
        expected_reason: &'static str,
    ) {
        let decision = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(target_api),
            kind,
        );

        assert_eq!(
            decision.diagnostic_kind,
            TransformDiagnosticKind::CapabilityDowngrade
        );
        assert_eq!(decision.level, expected_level);
        assert_eq!(decision.action, TransformAction::Drop);
        assert_eq!(decision.reason, expected_reason);
    }

    #[test]
    fn test_policy_engine_marks_top_k_as_lossy_minor_for_non_anthropic_targets() {
        assert_drop(
            LlmApiType::Openai,
            TransformValueKind::TopKParameter,
            TransformLossLevel::LossyMinor,
            "The target request capability matrix marks top_k as unsupported.",
        );
    }

    #[test]
    fn test_policy_engine_uses_capability_matrix_for_tool_definitions_and_tool_streaming() {
        assert_drop(
            LlmApiType::Ollama,
            TransformValueKind::ToolDefinitions,
            TransformLossLevel::LossyMajor,
            "The target request capability matrix marks tool definitions as unsupported.",
        );
        assert_drop(
            LlmApiType::Ollama,
            TransformValueKind::ToolCallDelta,
            TransformLossLevel::LossyMajor,
            "The target stream capability matrix marks tool call deltas as unsupported.",
        );
    }

    #[test]
    fn test_policy_engine_uses_capability_matrix_for_responses_reasoning_stream() {
        let decision = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Responses),
            TransformValueKind::ReasoningDelta,
        );

        assert_eq!(decision.level, TransformLossLevel::Lossless);
        assert_eq!(decision.action, TransformAction::Send);
    }

    #[test]
    fn test_policy_engine_uses_capability_matrix_for_response_refusal_and_reasoning() {
        assert_drop(
            LlmApiType::Gemini,
            TransformValueKind::Refusal,
            TransformLossLevel::LossyMajor,
            "The target capability matrix marks refusal content as unsupported.",
        );
        assert_drop(
            LlmApiType::Openai,
            TransformValueKind::ReasoningContent,
            TransformLossLevel::LossyMajor,
            "The target response capability matrix marks reasoning content as unsupported.",
        );
    }

    #[test]
    fn test_policy_engine_uses_capability_matrix_for_blob_delta_and_structured_stream_errors() {
        assert_drop(
            LlmApiType::Openai,
            TransformValueKind::BlobDelta,
            TransformLossLevel::LossyMajor,
            "The target stream capability matrix marks blob deltas as unsupported.",
        );
        assert_drop(
            LlmApiType::Gemini,
            TransformValueKind::StreamError,
            TransformLossLevel::LossyMajor,
            "The target stream capability matrix marks structured stream errors as unsupported.",
        );
    }

    #[test]
    fn test_policy_engine_tracks_image_file_and_executable_code_boundaries() {
        let image_delta = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Openai),
            TransformValueKind::ImageDelta,
        );
        assert_eq!(
            image_delta.diagnostic_kind,
            TransformDiagnosticKind::CapabilityDowngrade
        );
        assert_eq!(image_delta.level, TransformLossLevel::LossyMajor);
        assert_eq!(image_delta.action, TransformAction::Drop);

        let file_data = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Openai),
            TransformValueKind::FileData,
        );
        assert_eq!(
            file_data.diagnostic_kind,
            TransformDiagnosticKind::LossyTransform
        );
        assert_eq!(file_data.level, TransformLossLevel::LossyMajor);
        assert_eq!(file_data.action, TransformAction::Drop);

        let executable_code = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Responses),
            TransformValueKind::ExecutableCode,
        );
        assert_eq!(
            executable_code.diagnostic_kind,
            TransformDiagnosticKind::LossyTransform
        );
        assert_eq!(executable_code.level, TransformLossLevel::LossyMajor);
        assert_eq!(executable_code.action, TransformAction::Send);
    }
}
