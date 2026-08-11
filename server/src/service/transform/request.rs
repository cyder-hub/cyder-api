use serde_json::Value;

use super::adapter::{downstream_adapter_for, noop_finalize_request, upstream_adapter_for};
use super::diagnostics::{
    capture_transform_diagnostics, merge_transform_summaries, transform_success,
};
use super::{
    TransformAction, TransformFailure, TransformFailureOrigin, TransformOutcomeKind,
    TransformOutcomeSummary, TransformPhase, TransformReasonCode, TransformResult,
    TransformSemanticUnit,
};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol};

fn protocols_share_wire_format(
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
) -> bool {
    matches!(
        (downstream_protocol, upstream_protocol),
        (DownstreamProtocol::Openai, UpstreamProtocol::Openai)
            | (DownstreamProtocol::Responses, UpstreamProtocol::Responses)
            | (DownstreamProtocol::Anthropic, UpstreamProtocol::Anthropic)
            | (DownstreamProtocol::Gemini, UpstreamProtocol::Gemini)
    )
}

pub(in crate::service::transform) fn apply_stream_options(data: &mut Value) {
    let is_stream = data.get("stream").and_then(Value::as_bool).unwrap_or(false);
    if !is_stream {
        return;
    }

    if let Some(stream_options) = data.get_mut("stream_options") {
        if let Some(include_usage) = stream_options.get_mut("include_usage") {
            *include_usage = Value::Bool(true);
        } else {
            stream_options["include_usage"] = Value::Bool(true);
        }
    } else {
        data["stream_options"] = serde_json::json!({ "include_usage": true });
    }
}

pub(in crate::service::transform) fn finalize_request_data(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    profile_type: &UpstreamProfileType,
    downstream_path: &str,
) -> Value {
    let adapter = upstream_adapter_for(upstream_protocol);
    let finalize = adapter.request.finalize.unwrap_or(noop_finalize_request);
    finalize(data, profile_type, downstream_path)
}

pub(in crate::service::transform) fn transform_request_data(
    data: Value,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
) -> TransformResult<Value> {
    if protocols_share_wire_format(downstream_protocol, upstream_protocol) {
        return Ok(transform_success(
            data,
            TransformPhase::RequestEncode,
            TransformSemanticUnit::RequestEnvelope,
            TransformOutcomeKind::Passthrough,
            TransformAction::PassThrough,
            TransformReasonCode::SameWirePassthrough,
        ));
    }

    let (result, policy_summary) = capture_transform_diagnostics(|| {
        transform_request_data_inner(data, downstream_protocol, upstream_protocol, is_stream)
    });

    if let Some(rejection) = explicit_rejection(&policy_summary) {
        let result_summary = match &result {
            Ok(success) => success.summary.clone(),
            Err(failure) => failure.summary.clone(),
        };
        return Err(TransformFailure {
            origin: TransformFailureOrigin::TargetCapability,
            phase: rejection.phase,
            semantic_unit: rejection.semantic_unit,
            reason_code: rejection.reason_code,
            summary: merge_transform_summaries([policy_summary, result_summary]),
        });
    }

    match result {
        Ok(mut success) => {
            success.summary = merge_transform_summaries([policy_summary, success.summary]);
            Ok(success)
        }
        Err(mut failure) => {
            failure.summary = merge_transform_summaries([policy_summary, failure.summary]);
            Err(failure)
        }
    }
}

fn explicit_rejection(summary: &TransformOutcomeSummary) -> Option<super::TransformDiagnosticFact> {
    summary
        .control_fact()
        .filter(|fact| fact.action == TransformAction::Reject)
        .cloned()
}

fn transform_request_data_inner(
    data: Value,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
) -> TransformResult<Value> {
    let source_adapter = downstream_adapter_for(downstream_protocol);
    let target_adapter = upstream_adapter_for(upstream_protocol);

    let decoded = (source_adapter.request.decode)(data)?;
    let mut unified_request = decoded.value;
    unified_request.stream = is_stream;

    match (target_adapter.request.encode)(unified_request) {
        Ok(mut encoded) => {
            encoded.summary = merge_transform_summaries([decoded.summary, encoded.summary]);
            Ok(encoded)
        }
        Err(mut failure) => {
            failure.summary = merge_transform_summaries([decoded.summary, failure.summary]);
            Err(failure)
        }
    }
}
