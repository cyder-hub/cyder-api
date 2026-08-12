use serde_json::Value;

use super::adapter::{downstream_adapter_for, upstream_adapter_for};
use super::diagnostics::{
    capture_transform_diagnostics, merge_transform_summaries, record_captured_transform_fact,
    transform_failure, transform_success, upstream_usage_missing_summary,
};
use super::{
    TransformAction, TransformDiagnosticCollector, TransformDiagnosticFact, TransformFailure,
    TransformFailureOrigin, TransformOutcomeKind, TransformOutcomeSummary, TransformPhase,
    TransformReasonCode, TransformResult, TransformSafeSummary, TransformSemanticUnit,
    TransformSuccess,
};
use crate::cost::UsageNormalization;
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::utils::usage::UsageInfo;

#[derive(Debug, Clone)]
pub struct ResponseTransformValue {
    pub value: Value,
    pub usage_info: Option<UsageInfo>,
    pub usage_normalization: Option<UsageNormalization>,
    pub application_outcome: ResponseApplicationOutcome,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResponseApplicationOutcome {
    #[default]
    Success,
    Failed,
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
    let application_outcome =
        observe_application_outcome(&data, upstream_protocol, downstream_protocol)?;

    if protocols_share_wire_format(upstream_protocol, downstream_protocol) {
        let source_adapter = upstream_adapter_for(upstream_protocol);
        let observation = (source_adapter.response.decode)(data.clone());
        let (usage_info, usage_normalization, observation_summary) = match observation {
            Ok(decoded) => {
                let observed_usage = match upstream_protocol {
                    UpstreamProtocol::Responses => observe_responses_usage(&data),
                    UpstreamProtocol::Anthropic => observe_anthropic_usage(&data).ok().flatten(),
                    _ => decoded.value.usage,
                };
                let usage_info = observed_usage
                    .as_ref()
                    .and_then(|usage| UsageInfo::try_from(usage).ok());
                let usage_normalization = observed_usage.as_ref().map(Into::into);
                let observation_summary = if observed_usage.is_some() {
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
                let observed_usage = match upstream_protocol {
                    UpstreamProtocol::Responses => observe_responses_usage(&data),
                    UpstreamProtocol::Anthropic => observe_anthropic_usage(&data).ok().flatten(),
                    _ => None,
                };
                let usage_info = observed_usage
                    .as_ref()
                    .and_then(|usage| UsageInfo::try_from(usage).ok());
                let usage_normalization = observed_usage.as_ref().map(Into::into);
                let summary = merge_transform_summaries([
                    collector.into_summary(),
                    if observed_usage.is_some() {
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
                    },
                ]);
                (usage_info, usage_normalization, summary)
            }
        };
        let passthrough = transform_success(
            ResponseTransformValue {
                value: data,
                usage_info,
                usage_normalization,
                application_outcome,
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
        transform_result_with_cost_inner(
            data,
            upstream_protocol,
            downstream_protocol,
            application_outcome,
        )
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
    application_outcome: ResponseApplicationOutcome,
) -> TransformResult<ResponseTransformValue> {
    if upstream_protocol == UpstreamProtocol::Anthropic
        && downstream_protocol != DownstreamProtocol::Anthropic
    {
        for _ in data
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|block| {
                block.get("type").and_then(Value::as_str) == Some("thinking")
                    && block
                        .get("signature")
                        .is_some_and(|signature| !signature.is_null())
            })
        {
            record_captured_transform_fact(TransformDiagnosticFact {
                sequence: 0,
                phase: TransformPhase::ResponseDecode,
                semantic_unit: TransformSemanticUnit::ReasoningContent,
                outcome: TransformOutcomeKind::ControlledLossMinor,
                action: TransformAction::Drop,
                reason_code: TransformReasonCode::UnsupportedReasoning,
                safe_summary: None,
            });
        }
    }
    let source_adapter = upstream_adapter_for(upstream_protocol);
    let target_adapter = downstream_adapter_for(downstream_protocol);
    let decoded = (source_adapter.response.decode)(data)?;
    let usage_info = decoded
        .value
        .usage
        .as_ref()
        .map(UsageInfo::try_from)
        .transpose()
        .map_err(|_| {
            transform_failure(
                TransformFailureOrigin::UpstreamPayload,
                TransformPhase::ResponseObserve,
                TransformSemanticUnit::Usage,
                TransformReasonCode::UsageOverflow,
                None,
            )
        })?;
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
                application_outcome,
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

fn observe_application_outcome(
    data: &Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> Result<ResponseApplicationOutcome, TransformFailure> {
    let fail = |reason_code| {
        transform_failure(
            TransformFailureOrigin::UpstreamPayload,
            TransformPhase::ResponseObserve,
            TransformSemanticUnit::Lifecycle,
            reason_code,
            Some(TransformSafeSummary::from_json(data)),
        )
    };

    if upstream_protocol == UpstreamProtocol::Anthropic {
        if downstream_protocol == DownstreamProtocol::Anthropic {
            return Ok(ResponseApplicationOutcome::Success);
        }
        return match data.get("stop_reason").and_then(Value::as_str) {
            Some(
                "end_turn"
                | "stop_sequence"
                | "tool_use"
                | "max_tokens"
                | "model_context_window_exceeded"
                | "refusal",
            ) => Ok(ResponseApplicationOutcome::Success),
            Some("pause_turn") | None => Err(fail(TransformReasonCode::IllegalUpstreamTerminal)),
            Some(_) => Err(fail(TransformReasonCode::UnknownStopReason)),
        };
    }

    if upstream_protocol != UpstreamProtocol::Responses {
        return Ok(ResponseApplicationOutcome::Success);
    }

    let status = data
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| fail(TransformReasonCode::IllegalUpstreamTerminal))?;
    let has_error = data.get("error").is_some_and(|error| !error.is_null());

    match status {
        "completed" if !has_error => Ok(ResponseApplicationOutcome::Success),
        "incomplete" if !has_error => {
            let reason = data
                .get("incomplete_details")
                .and_then(|details| details.get("reason"))
                .and_then(Value::as_str)
                .ok_or_else(|| fail(TransformReasonCode::IllegalUpstreamTerminal))?;
            if matches!(
                reason,
                "max_tokens" | "max_output_tokens" | "content_filter"
            ) || downstream_protocol == DownstreamProtocol::Responses
            {
                Ok(ResponseApplicationOutcome::Success)
            } else {
                Err(fail(TransformReasonCode::UnknownIncompleteReason))
            }
        }
        "failed" if downstream_protocol == DownstreamProtocol::Responses => {
            Ok(ResponseApplicationOutcome::Failed)
        }
        "failed" => Err(fail(TransformReasonCode::UpstreamApplicationFailed)),
        "queued" | "in_progress" | "cancelled" => {
            Err(fail(TransformReasonCode::IllegalUpstreamTerminal))
        }
        _ => Err(fail(TransformReasonCode::IllegalUpstreamTerminal)),
    }
}

pub(in crate::service::transform) fn observe_responses_usage(
    data: &Value,
) -> Option<super::unified::UnifiedUsage> {
    let usage = data.get("usage").or_else(|| {
        data.get("response")
            .and_then(|response| response.get("usage"))
    })?;
    if usage.is_null() {
        return None;
    }
    let token_count = |field: &str| {
        usage
            .get(field)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    };

    Some(super::unified::UnifiedUsage {
        input_tokens: token_count("input_tokens")?,
        output_tokens: token_count("output_tokens")?,
        total_tokens: token_count("total_tokens")?,
        cached_tokens: usage
            .get("input_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        reasoning_tokens: usage
            .get("output_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        ..Default::default()
    })
}

pub(in crate::service::transform) fn observe_anthropic_usage(
    data: &Value,
) -> Result<Option<super::unified::UnifiedUsage>, TransformReasonCode> {
    let Some(usage) = data.get("usage") else {
        return Ok(None);
    };
    if usage.is_null() {
        return Ok(None);
    }
    let usage: super::providers::anthropic::AnthropicUsage = serde_json::from_value(usage.clone())
        .map_err(|_| TransformReasonCode::SourceDecodeFailed)?;
    super::providers::anthropic::anthropic_usage_to_unified(&usage)
        .map(Some)
        .ok_or(TransformReasonCode::UsageOverflow)
}
