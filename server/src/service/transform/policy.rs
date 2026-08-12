use super::TransformProtocol;
use super::capability::{ProtocolCapabilityMatrix, TransformValueKind};
use super::diagnostics::{TransformAction, TransformOutcomeKind, TransformReasonCode};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};

impl TransformProtocol {
    fn is_openai(self) -> bool {
        matches!(
            self,
            Self::Downstream(DownstreamProtocol::Openai) | Self::Upstream(UpstreamProtocol::Openai)
        )
    }

    fn is_gemini(self) -> bool {
        matches!(
            self,
            Self::Downstream(DownstreamProtocol::Gemini) | Self::Upstream(UpstreamProtocol::Gemini)
        )
    }

    fn is_anthropic(self) -> bool {
        matches!(
            self,
            Self::Downstream(DownstreamProtocol::Anthropic)
                | Self::Upstream(UpstreamProtocol::Anthropic)
        )
    }

    fn is_responses(self) -> bool {
        matches!(
            self,
            Self::Downstream(DownstreamProtocol::Responses)
                | Self::Upstream(UpstreamProtocol::Responses)
        )
    }

    fn is_ollama(self) -> bool {
        matches!(self, Self::Upstream(UpstreamProtocol::Ollama))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicyDecision {
    pub outcome: TransformOutcomeKind,
    pub action: TransformAction,
    pub reason_code: TransformReasonCode,
}

impl PolicyDecision {
    const fn lossless() -> Self {
        Self {
            outcome: TransformOutcomeKind::Lossless,
            action: TransformAction::Send,
            reason_code: TransformReasonCode::LosslessConversion,
        }
    }

    const fn minor_drop(reason_code: TransformReasonCode) -> Self {
        Self {
            outcome: TransformOutcomeKind::ControlledLossMinor,
            action: TransformAction::Drop,
            reason_code,
        }
    }

    const fn major_reject(reason_code: TransformReasonCode) -> Self {
        Self {
            outcome: TransformOutcomeKind::ExplicitReject,
            action: TransformAction::Reject,
            reason_code,
        }
    }

    const fn deterministic_text_downgrade() -> Self {
        Self {
            outcome: TransformOutcomeKind::ControlledLossMajor,
            action: TransformAction::Send,
            reason_code: TransformReasonCode::DeterministicTextDowngrade,
        }
    }
}

pub(crate) struct PolicyEngine;

impl PolicyEngine {
    fn target_capabilities(target: TransformProtocol) -> Option<ProtocolCapabilityMatrix> {
        match target {
            TransformProtocol::Downstream(protocol) => {
                Some(ProtocolCapabilityMatrix::for_downstream(protocol))
            }
            TransformProtocol::Upstream(protocol) => {
                Some(ProtocolCapabilityMatrix::for_upstream(protocol))
            }
            TransformProtocol::Unified => None,
        }
    }

    fn evaluate_capability_matrix(
        target: TransformProtocol,
        kind: TransformValueKind,
    ) -> Option<PolicyDecision> {
        let capabilities = Self::target_capabilities(target)?;

        match kind {
            TransformValueKind::TopKParameter if !capabilities.request.top_k_parameter => Some(
                PolicyDecision::minor_drop(TransformReasonCode::UnsupportedTopK),
            ),
            TransformValueKind::ToolDefinitions if !capabilities.request.tool_definitions => Some(
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedToolDefinitions),
            ),
            TransformValueKind::ToolRoleMessage if !capabilities.request.tool_role_messages => {
                Some(PolicyDecision::major_reject(
                    TransformReasonCode::UnsupportedToolRoleMessage,
                ))
            }
            TransformValueKind::ToolCallDelta if !capabilities.stream.tool_call_deltas => Some(
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedToolCallDelta),
            ),
            TransformValueKind::ReasoningDelta if !capabilities.stream.reasoning_deltas => Some(
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedReasoning),
            ),
            TransformValueKind::BlobDelta if !capabilities.stream.blob_deltas => Some(
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedBlobDelta),
            ),
            TransformValueKind::StreamError if !capabilities.stream.structured_errors => Some(
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedStructuredError),
            ),
            TransformValueKind::ReasoningContent if !capabilities.response.reasoning_content => {
                Some(PolicyDecision::major_reject(
                    TransformReasonCode::UnsupportedReasoning,
                ))
            }
            TransformValueKind::Refusal
                if !capabilities.response.refusal || !capabilities.structured_content.refusal =>
            {
                Some(PolicyDecision::major_reject(
                    TransformReasonCode::UnsupportedRefusal,
                ))
            }
            _ => None,
        }
    }

    pub(crate) fn evaluate(
        source: TransformProtocol,
        target: TransformProtocol,
        kind: TransformValueKind,
    ) -> PolicyDecision {
        if matches!(
            target,
            TransformProtocol::Downstream(DownstreamProtocol::Openai)
        ) && matches!(
            kind,
            TransformValueKind::ReasoningContent | TransformValueKind::ReasoningDelta
        ) {
            return PolicyDecision::minor_drop(TransformReasonCode::UnsupportedReasoning);
        }
        if let Some(decision) = Self::evaluate_capability_matrix(target, kind) {
            return decision;
        }

        match (source, target, kind) {
            (_, target, TransformValueKind::ImageUrl)
            | (_, target, TransformValueKind::FileUrl)
            | (_, target, TransformValueKind::FileData)
            | (_, target, TransformValueKind::ExecutableCode)
                if target.is_anthropic() =>
            {
                PolicyDecision::deterministic_text_downgrade()
            }
            (_, target, TransformValueKind::ImageUrl) if target.is_gemini() => {
                PolicyDecision::deterministic_text_downgrade()
            }
            (_, target, TransformValueKind::FileUrl | TransformValueKind::ExecutableCode)
                if target.is_responses() =>
            {
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedContent)
            }
            (_, target, TransformValueKind::ToolRoleMessage)
            | (_, target, TransformValueKind::ToolCall)
            | (_, target, TransformValueKind::ToolResult)
            | (_, target, TransformValueKind::ImageUrl)
            | (_, target, TransformValueKind::FileUrl)
            | (_, target, TransformValueKind::FileData)
            | (_, target, TransformValueKind::ExecutableCode)
                if target.is_ollama() =>
            {
                PolicyDecision::deterministic_text_downgrade()
            }
            (source, TransformProtocol::Unified, TransformValueKind::ResponsesUnknownItem)
                if source.is_responses() =>
            {
                PolicyDecision::major_reject(TransformReasonCode::UnknownSemanticUnit)
            }
            (_, target, TransformValueKind::FileUrl)
            | (_, target, TransformValueKind::ExecutableCode)
                if target.is_openai() =>
            {
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedContent)
            }
            (
                _,
                TransformProtocol::Downstream(DownstreamProtocol::Openai),
                TransformValueKind::AudioData
                | TransformValueKind::FileData
                | TransformValueKind::FileId,
            ) => PolicyDecision::major_reject(TransformReasonCode::UnsupportedContent),
            (_, target, TransformValueKind::FileId)
                if !target.is_openai() && !target.is_responses() =>
            {
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedContent)
            }
            (_, target, TransformValueKind::AudioData)
                if target.is_anthropic() || target.is_ollama() =>
            {
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedContent)
            }
            (_, target, TransformValueKind::ImageDelta)
                if target.is_openai() || target.is_gemini() || target.is_anthropic() =>
            {
                PolicyDecision::major_reject(TransformReasonCode::UnsupportedImageDelta)
            }
            _ => PolicyDecision::lossless(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_minor_parameter_is_dropped_with_a_stable_reason() {
        let decision = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Upstream(UpstreamProtocol::Openai),
            TransformValueKind::TopKParameter,
        );
        assert_eq!(decision.outcome, TransformOutcomeKind::ControlledLossMinor);
        assert_eq!(decision.action, TransformAction::Drop);
        assert_eq!(decision.reason_code, TransformReasonCode::UnsupportedTopK);
    }

    #[test]
    fn unsupported_major_capabilities_reject_by_default() {
        for kind in [
            TransformValueKind::ToolDefinitions,
            TransformValueKind::ToolRoleMessage,
            TransformValueKind::ReasoningContent,
            TransformValueKind::BlobDelta,
        ] {
            let decision = PolicyEngine::evaluate(
                TransformProtocol::Unified,
                TransformProtocol::Upstream(UpstreamProtocol::Ollama),
                kind,
            );
            assert_eq!(decision.outcome, TransformOutcomeKind::ExplicitReject);
            assert_eq!(decision.action, TransformAction::Reject);
        }
    }

    #[test]
    fn only_registered_text_downgrades_may_send_major_loss() {
        let allowed = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Upstream(UpstreamProtocol::Anthropic),
            TransformValueKind::FileUrl,
        );
        assert_eq!(allowed.outcome, TransformOutcomeKind::ControlledLossMajor);
        assert_eq!(allowed.action, TransformAction::Send);
        assert_eq!(
            allowed.reason_code,
            TransformReasonCode::DeterministicTextDowngrade
        );

        let rejected = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Upstream(UpstreamProtocol::Openai),
            TransformValueKind::FileUrl,
        );
        assert_eq!(rejected.outcome, TransformOutcomeKind::ExplicitReject);
        assert_eq!(rejected.action, TransformAction::Reject);
    }

    #[test]
    fn ollama_rejects_media_that_its_encoder_cannot_represent() {
        for kind in [TransformValueKind::AudioData, TransformValueKind::FileId] {
            let decision = PolicyEngine::evaluate(
                TransformProtocol::Unified,
                TransformProtocol::Upstream(UpstreamProtocol::Ollama),
                kind,
            );
            assert_eq!(decision.outcome, TransformOutcomeKind::ExplicitReject);
            assert_eq!(decision.action, TransformAction::Reject);
            assert_eq!(
                decision.reason_code,
                TransformReasonCode::UnsupportedContent
            );
        }

        let recoverable_file_data = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Upstream(UpstreamProtocol::Ollama),
            TransformValueKind::FileData,
        );
        assert_eq!(
            recoverable_file_data.outcome,
            TransformOutcomeKind::ControlledLossMajor
        );
        assert_eq!(recoverable_file_data.action, TransformAction::Send);
        assert_eq!(
            recoverable_file_data.reason_code,
            TransformReasonCode::DeterministicTextDowngrade
        );
    }

    #[test]
    fn openai_response_target_rejects_unencodable_media() {
        for kind in [
            TransformValueKind::AudioData,
            TransformValueKind::FileData,
            TransformValueKind::FileId,
        ] {
            let response_decision = PolicyEngine::evaluate(
                TransformProtocol::Unified,
                TransformProtocol::Downstream(DownstreamProtocol::Openai),
                kind,
            );
            assert_eq!(
                response_decision.outcome,
                TransformOutcomeKind::ExplicitReject
            );
            assert_eq!(response_decision.action, TransformAction::Reject);
            assert_eq!(
                response_decision.reason_code,
                TransformReasonCode::UnsupportedContent
            );

            let request_decision = PolicyEngine::evaluate(
                TransformProtocol::Unified,
                TransformProtocol::Upstream(UpstreamProtocol::Openai),
                kind,
            );
            assert_eq!(request_decision.outcome, TransformOutcomeKind::Lossless);
            assert_eq!(request_decision.action, TransformAction::Send);
        }
    }

    #[test]
    fn responses_request_target_sends_native_inline_media_and_rejects_external_file_io() {
        for kind in [
            TransformValueKind::ImageUrl,
            TransformValueKind::ImageData,
            TransformValueKind::AudioData,
            TransformValueKind::FileData,
            TransformValueKind::FileId,
        ] {
            let decision = PolicyEngine::evaluate(
                TransformProtocol::Unified,
                TransformProtocol::Upstream(UpstreamProtocol::Responses),
                kind,
            );
            assert_eq!(decision.outcome, TransformOutcomeKind::Lossless, "{kind:?}");
            assert_eq!(decision.action, TransformAction::Send, "{kind:?}");
        }

        for kind in [
            TransformValueKind::FileUrl,
            TransformValueKind::ExecutableCode,
        ] {
            let decision = PolicyEngine::evaluate(
                TransformProtocol::Unified,
                TransformProtocol::Upstream(UpstreamProtocol::Responses),
                kind,
            );
            assert_eq!(decision.outcome, TransformOutcomeKind::ExplicitReject);
            assert_eq!(decision.action, TransformAction::Reject);
            assert_eq!(
                decision.reason_code,
                TransformReasonCode::UnsupportedContent
            );
        }
    }
}
