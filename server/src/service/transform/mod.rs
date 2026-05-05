use crate::schema::enum_def::LlmApiType;

pub(crate) mod adapter;
pub(crate) mod capability;
pub(crate) mod diagnostics;
pub(crate) mod facade;
pub(crate) mod policy;
pub(crate) mod providers;
pub mod quality;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod stream;
pub mod unified;
use capability::TransformValueKind;
pub(in crate::service::transform) use diagnostics::build_stream_diagnostic_sse;
use diagnostics::{
    build_transform_diagnostic, log_transform_diagnostic, record_captured_transform_diagnostic,
};
pub use facade::{
    RequestTransformOutput, ResponseTransformOutput, finalize_request_data, transform_request_data,
    transform_request_data_with_diagnostics, transform_result, transform_result_with_cost,
    transform_result_with_cost_and_diagnostics,
};
use policy::{PolicyEngine, TransformAction, TransformLossLevel};
pub(crate) use stream::AnthropicActiveBlockKind;
pub use stream::{
    AnthropicActiveBlockState, AnthropicSessionState, GeminiSessionState, ResponsesSessionState,
    SessionContext, StreamTransformer,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TransformProtocol {
    Unified,
    Api(LlmApiType),
}

pub(crate) fn apply_transform_policy(
    source: TransformProtocol,
    target: TransformProtocol,
    kind: TransformValueKind,
    context: &'static str,
) -> bool {
    let decision = PolicyEngine::evaluate(source, target, kind);
    if decision.level != TransformLossLevel::Lossless {
        let diagnostic = build_transform_diagnostic(
            decision.diagnostic_kind,
            source,
            target,
            kind,
            decision,
            None,
            None,
            Some(context),
            None,
            Some(decision.reason.to_string()),
        );
        record_captured_transform_diagnostic(&diagnostic);
        log_transform_diagnostic(&diagnostic);
    }

    matches!(decision.action, TransformAction::Send)
}
