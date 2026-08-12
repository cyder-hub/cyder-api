use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};

pub(crate) mod adapter;
pub(crate) mod audit;
pub(crate) mod capability;
pub(crate) mod diagnostics;
pub(crate) mod facade;
pub(crate) mod media;
pub(crate) mod policy;
pub(crate) mod providers;
pub mod quality;
pub(crate) mod request;
mod request_conflict;
pub(crate) mod response;
pub(crate) mod stream;
mod stream_audit;
pub(crate) mod structured;
pub mod unified;
use capability::TransformValueKind;
pub(crate) use diagnostics::TransformDiagnosticCollector;
pub(in crate::service::transform) use diagnostics::record_stream_diagnostic;
pub use diagnostics::{
    TransformAction, TransformDiagnosticFact, TransformFailure, TransformFailureOrigin,
    TransformOutcomeKind, TransformOutcomeSummary, TransformPhase, TransformReasonCode,
    TransformResult, TransformSafeSummary, TransformSemanticUnit, TransformSeverity,
    TransformSuccess,
};
use diagnostics::{TransformDiagnosticFact as DiagnosticFact, record_captured_transform_fact};
pub(crate) use facade::validate_final_generation_request;
pub use facade::{
    ResponseTransformValue, finalize_request_data, transform_request_data, transform_result,
    transform_result_with_cost,
};
use policy::PolicyEngine;
pub(crate) use stream::AnthropicActiveBlockKind;
pub use stream::{
    AnthropicActiveBlockState, AnthropicSessionState, GeminiSessionState, ResponsesSessionState,
    SessionContext, StreamBatchTransformOutput, StreamFrameDisposition, StreamTransformOutput,
    StreamTransformer,
};
pub(crate) use stream::{FatalStreamEncodeError, FatalStreamErrorFact, encode_fatal_stream_error};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TransformProtocol {
    Unified,
    Downstream(DownstreamProtocol),
    Upstream(UpstreamProtocol),
}

pub(crate) fn apply_transform_policy(
    source: TransformProtocol,
    target: TransformProtocol,
    kind: TransformValueKind,
    _context: &'static str,
) -> bool {
    let decision = PolicyEngine::evaluate(source, target, kind);
    if decision.outcome != TransformOutcomeKind::Lossless {
        let stream_semantic = matches!(
            kind,
            TransformValueKind::ImageDelta
                | TransformValueKind::ToolCallDelta
                | TransformValueKind::ReasoningDelta
                | TransformValueKind::BlobDelta
                | TransformValueKind::StreamError
        );
        let phase = if stream_semantic {
            if matches!(target, TransformProtocol::Unified) {
                TransformPhase::StreamDecode
            } else {
                TransformPhase::StreamEncode
            }
        } else {
            match (source, target) {
                (TransformProtocol::Downstream(_), TransformProtocol::Unified) => {
                    TransformPhase::RequestDecode
                }
                (TransformProtocol::Unified, TransformProtocol::Upstream(_)) => {
                    TransformPhase::RequestEncode
                }
                (TransformProtocol::Upstream(_), TransformProtocol::Unified) => {
                    TransformPhase::ResponseDecode
                }
                (TransformProtocol::Unified, TransformProtocol::Downstream(_)) => {
                    TransformPhase::ResponseEncode
                }
                _ => TransformPhase::ResponseEncode,
            }
        };
        record_captured_transform_fact(DiagnosticFact::from_policy(phase, kind.into(), decision));
    }

    matches!(decision.action, TransformAction::Send)
}
