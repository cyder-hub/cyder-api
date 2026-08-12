use serde_json::Value;

use super::adapter::{downstream_adapter_for, upstream_adapter_for};
use super::diagnostics::{
    capture_transform_diagnostics, merge_transform_summaries, transform_success,
    upstream_usage_missing_summary,
};
use super::{
    TransformAction, TransformDiagnosticCollector, TransformDiagnosticFact, TransformFailure,
    TransformFailureOrigin, TransformOutcomeKind, TransformOutcomeSummary, TransformPhase,
    TransformReasonCode, TransformResult, TransformSemanticUnit, TransformSuccess,
};
use crate::cost::UsageNormalization;
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::utils::usage::UsageInfo;

#[derive(Debug, Clone)]
pub struct ResponseTransformValue {
    pub value: Value,
    pub usage_info: Option<UsageInfo>,
    pub usage_normalization: Option<UsageNormalization>,
}

fn protocols_share_wire_format(
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> bool {
    matches!(
        (upstream_protocol, downstream_protocol),
        (UpstreamProtocol::Openai, DownstreamProtocol::Openai)
            | (UpstreamProtocol::Responses, DownstreamProtocol::Responses)
            | (UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic)
            | (UpstreamProtocol::Gemini, DownstreamProtocol::Gemini)
    )
}

pub(in crate::service::transform) fn transform_result(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> TransformResult<(Value, Option<UsageInfo>)> {
    transform_result_with_cost(data, upstream_protocol, downstream_protocol).map(|success| {
        TransformSuccess {
            value: (success.value.value, success.value.usage_info),
            summary: success.summary,
        }
    })
}

pub(in crate::service::transform) fn transform_result_with_cost(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> TransformResult<ResponseTransformValue> {
    if protocols_share_wire_format(upstream_protocol, downstream_protocol) {
        let source_adapter = upstream_adapter_for(upstream_protocol);
        let observation = (source_adapter.response.decode)(data.clone());
        let (usage_info, usage_normalization, observation_summary) = match observation {
            Ok(decoded) => {
                let usage_info = decoded.value.usage.clone().map(Into::into);
                let usage_normalization = decoded.value.usage.as_ref().map(Into::into);
                let observation_summary = if decoded.value.usage.is_some() {
                    transform_success(
                        (),
                        TransformPhase::ResponseObserve,
                        TransformSemanticUnit::Usage,
                        TransformOutcomeKind::Lossless,
                        TransformAction::PassThrough,
                        TransformReasonCode::LosslessConversion,
                    )
                    .summary
                } else {
                    upstream_usage_missing_summary(TransformPhase::ResponseObserve)
                };
                (usage_info, usage_normalization, observation_summary)
            }
            Err(failure) => {
                let mut collector = TransformDiagnosticCollector::default();
                collector.record(TransformDiagnosticFact {
                    sequence: 0,
                    phase: TransformPhase::ResponseObserve,
                    semantic_unit: TransformSemanticUnit::ResponseEnvelope,
                    outcome: TransformOutcomeKind::ObservationDegraded,
                    action: TransformAction::PassThrough,
                    reason_code: TransformReasonCode::ObservationParseFailed,
                    safe_summary: failure
                        .summary
                        .facts
                        .first()
                        .and_then(|fact| fact.safe_summary.clone()),
                });
                (None, None, collector.into_summary())
            }
        };
        let passthrough = transform_success(
            ResponseTransformValue {
                value: data,
                usage_info,
                usage_normalization,
            },
            TransformPhase::ResponseObserve,
            TransformSemanticUnit::ResponseEnvelope,
            TransformOutcomeKind::Passthrough,
            TransformAction::PassThrough,
            TransformReasonCode::SameWirePassthrough,
        );
        return Ok(TransformSuccess {
            value: passthrough.value,
            summary: merge_transform_summaries([observation_summary, passthrough.summary]),
        });
    }

    let (result, policy_summary) = capture_transform_diagnostics(|| {
        transform_result_with_cost_inner(data, upstream_protocol, downstream_protocol)
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

fn explicit_rejection(summary: &TransformOutcomeSummary) -> Option<TransformDiagnosticFact> {
    summary
        .control_fact()
        .filter(|fact| fact.action == TransformAction::Reject)
        .cloned()
}

fn transform_result_with_cost_inner(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> TransformResult<ResponseTransformValue> {
    let source_adapter = upstream_adapter_for(upstream_protocol);
    let target_adapter = downstream_adapter_for(downstream_protocol);
    let decoded = (source_adapter.response.decode)(data)?;
    let usage_info = decoded.value.usage.clone().map(Into::into);
    let usage_normalization = decoded.value.usage.as_ref().map(Into::into);
    let usage_summary = if decoded.value.usage.is_some() {
        transform_success(
            (),
            TransformPhase::ResponseObserve,
            TransformSemanticUnit::Usage,
            TransformOutcomeKind::Lossless,
            TransformAction::PassThrough,
            TransformReasonCode::LosslessConversion,
        )
        .summary
    } else {
        upstream_usage_missing_summary(TransformPhase::ResponseObserve)
    };

    match (target_adapter.response.encode)(decoded.value) {
        Ok(encoded) => Ok(TransformSuccess {
            value: ResponseTransformValue {
                value: encoded.value,
                usage_info,
                usage_normalization,
            },
            summary: merge_transform_summaries([decoded.summary, usage_summary, encoded.summary]),
        }),
        Err(mut failure) => {
            failure.summary =
                merge_transform_summaries([decoded.summary, usage_summary, failure.summary]);
            Err(failure)
        }
    }
}
