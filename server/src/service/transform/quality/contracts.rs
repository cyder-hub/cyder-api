use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::super::diagnostics::merge_transform_summaries;
use super::super::{
    StreamTransformer, TransformAction, TransformFailure, TransformFailureOrigin,
    TransformOutcomeKind, TransformOutcomeSummary, TransformPhase, TransformReasonCode,
    TransformResult, TransformSemanticUnit, transform_request_data, transform_result,
};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::utils::sse::SseEvent;

const CONTRACT_FORMAT_VERSION: u32 = 1;

const REQUIRED_CONTRACTS: [&str; 11] = [
    "request_decode_failure",
    "request_encode_failure",
    "response_decode_failure",
    "response_encode_failure",
    "stream_decode_failure",
    "stream_encode_failure",
    "unknown_semantic_reject",
    "minor_controlled_loss",
    "major_loss_reject",
    "legal_synthesis",
    "diagnostic_cap_overflow",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformContractCaseReport {
    pub contract_name: String,
    pub expected_success: bool,
    pub expected_origin: Option<String>,
    pub expected_phase: String,
    pub expected_semantic_unit: String,
    pub expected_outcome: String,
    pub expected_action: String,
    pub expected_reason_code: String,
    pub observed_success: bool,
    pub observed_origin: Option<String>,
    pub observed_phase: Option<String>,
    pub observed_semantic_unit: Option<String>,
    pub observed_reason_code: Option<String>,
    pub input_count: u64,
    pub accounted_input_count: u64,
    pub total_fact_count: u64,
    pub retained_fact_count: u64,
    pub dropped_diagnostic_count: u64,
    pub outcome_counts: BTreeMap<String, u64>,
    pub action_counts: BTreeMap<String, u64>,
    pub semantic_counts: BTreeMap<String, u64>,
    pub accounting_closed: bool,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformContractSummary {
    pub format_version: u32,
    pub required_case_count: usize,
    pub case_count: usize,
    pub passed_case_count: usize,
    pub input_count: u64,
    pub accounted_input_count: u64,
    pub total_fact_count: u64,
    pub retained_fact_count: u64,
    pub dropped_diagnostic_count: u64,
    pub outcome_counts: BTreeMap<String, u64>,
    pub action_counts: BTreeMap<String, u64>,
    pub semantic_counts: BTreeMap<String, u64>,
    pub accounting_closed: bool,
    pub payload_free: bool,
    pub passed: bool,
    pub cases: Vec<TransformContractCaseReport>,
}

#[derive(Clone, Copy)]
struct ContractExpectation {
    name: &'static str,
    input_count: u64,
    success: bool,
    origin: Option<TransformFailureOrigin>,
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
    outcome: TransformOutcomeKind,
    action: TransformAction,
    reason_code: TransformReasonCode,
}

#[derive(Clone)]
struct FailureObservation {
    origin: TransformFailureOrigin,
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
    reason_code: TransformReasonCode,
}

struct ContractExecution {
    accounted_input_count: u64,
    successful_input_count: u64,
    failed_input_count: u64,
    summary: TransformOutcomeSummary,
    failure: Option<FailureObservation>,
}

impl ContractExecution {
    fn from_result<T>(result: TransformResult<T>) -> Self {
        match result {
            Ok(success) => Self {
                accounted_input_count: 1,
                successful_input_count: 1,
                failed_input_count: 0,
                summary: success.summary,
                failure: None,
            },
            Err(failure) => Self::from_failure(failure),
        }
    }

    fn from_failure(failure: TransformFailure) -> Self {
        let observation = FailureObservation {
            origin: failure.origin,
            phase: failure.phase,
            semantic_unit: failure.semantic_unit,
            reason_code: failure.reason_code,
        };
        Self {
            accounted_input_count: 1,
            successful_input_count: 0,
            failed_input_count: 1,
            summary: failure.summary,
            failure: Some(observation),
        }
    }
}

pub fn build_transform_contract_summary() -> TransformContractSummary {
    summarize_contract_cases(build_contract_cases())
}

fn build_contract_cases() -> Vec<TransformContractCaseReport> {
    vec![
        build_case_report(
            ContractExpectation {
                name: "request_decode_failure",
                input_count: 1,
                success: false,
                origin: Some(TransformFailureOrigin::DownstreamInput),
                phase: TransformPhase::RequestDecode,
                semantic_unit: TransformSemanticUnit::RequestEnvelope,
                outcome: TransformOutcomeKind::ExplicitReject,
                action: TransformAction::Reject,
                reason_code: TransformReasonCode::InvalidProtocolShape,
            },
            ContractExecution::from_result(transform_request_data(
                json!({"model":"gpt-4.1","messages":"QUALITY_PRIVATE_SENTINEL"}),
                DownstreamProtocol::Openai,
                UpstreamProtocol::Gemini,
                false,
            )),
        ),
        build_case_report(
            ContractExpectation {
                name: "request_encode_failure",
                input_count: 1,
                success: false,
                origin: Some(TransformFailureOrigin::TargetCapability),
                phase: TransformPhase::RequestEncode,
                semantic_unit: TransformSemanticUnit::ToolDefinitions,
                outcome: TransformOutcomeKind::ExplicitReject,
                action: TransformAction::Reject,
                reason_code: TransformReasonCode::UnsupportedToolDefinitions,
            },
            ContractExecution::from_result(transform_request_data(
                json!({
                    "model":"gpt-4.1",
                    "messages":[{"role":"user","content":"hello"}],
                    "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]
                }),
                DownstreamProtocol::Openai,
                UpstreamProtocol::Ollama,
                false,
            )),
        ),
        build_case_report(
            ContractExpectation {
                name: "response_decode_failure",
                input_count: 1,
                success: false,
                origin: Some(TransformFailureOrigin::UpstreamPayload),
                phase: TransformPhase::ResponseDecode,
                semantic_unit: TransformSemanticUnit::ResponseEnvelope,
                outcome: TransformOutcomeKind::FatalError,
                action: TransformAction::Terminate,
                reason_code: TransformReasonCode::SourceDecodeFailed,
            },
            ContractExecution::from_result(transform_result(
                json!({
                    "id":"chatcmpl-quality",
                    "object":"chat.completion",
                    "created":1,
                    "model":"gpt-4.1",
                    "choices":"QUALITY_PRIVATE_SENTINEL"
                }),
                UpstreamProtocol::Openai,
                DownstreamProtocol::Gemini,
            )),
        ),
        build_case_report(
            ContractExpectation {
                name: "response_encode_failure",
                input_count: 1,
                success: false,
                origin: Some(TransformFailureOrigin::TargetCapability),
                phase: TransformPhase::ResponseEncode,
                semantic_unit: TransformSemanticUnit::ReasoningContent,
                outcome: TransformOutcomeKind::ExplicitReject,
                action: TransformAction::Reject,
                reason_code: TransformReasonCode::UnsupportedReasoning,
            },
            ContractExecution::from_result(transform_result(
                json!({
                    "id":"msg_quality",
                    "type":"message",
                    "role":"assistant",
                    "content":[{"type":"thinking","thinking":"private","signature":"sig"}],
                    "model":"claude-sonnet",
                    "stop_reason":"end_turn",
                    "stop_sequence":null,
                    "usage":{"input_tokens":1,"output_tokens":1}
                }),
                UpstreamProtocol::Anthropic,
                DownstreamProtocol::Openai,
            )),
        ),
        build_case_report(
            ContractExpectation {
                name: "stream_decode_failure",
                input_count: 1,
                success: false,
                origin: Some(TransformFailureOrigin::UpstreamPayload),
                phase: TransformPhase::StreamDecode,
                semantic_unit: TransformSemanticUnit::StreamFrame,
                outcome: TransformOutcomeKind::FatalError,
                action: TransformAction::Terminate,
                reason_code: TransformReasonCode::SourceDecodeFailed,
            },
            ContractExecution::from_result(
                StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Gemini)
                    .transform_event(sse("{QUALITY_PRIVATE_SENTINEL}")),
            ),
        ),
        build_case_report(
            ContractExpectation {
                name: "stream_encode_failure",
                input_count: 2,
                success: false,
                origin: Some(TransformFailureOrigin::TargetCapability),
                phase: TransformPhase::StreamEncode,
                semantic_unit: TransformSemanticUnit::ReasoningDelta,
                outcome: TransformOutcomeKind::ExplicitReject,
                action: TransformAction::Reject,
                reason_code: TransformReasonCode::UnsupportedReasoning,
            },
            execute_stream_encode_failure(),
        ),
        build_case_report(
            ContractExpectation {
                name: "unknown_semantic_reject",
                input_count: 1,
                success: false,
                origin: Some(TransformFailureOrigin::DownstreamInput),
                phase: TransformPhase::RequestDecode,
                semantic_unit: TransformSemanticUnit::Role,
                outcome: TransformOutcomeKind::ExplicitReject,
                action: TransformAction::Reject,
                reason_code: TransformReasonCode::UnknownSemanticUnit,
            },
            ContractExecution::from_result(transform_request_data(
                json!({"model":"gpt-4.1","messages":[{"role":"alien","content":"hello"}]}),
                DownstreamProtocol::Openai,
                UpstreamProtocol::Gemini,
                false,
            )),
        ),
        build_case_report(
            ContractExpectation {
                name: "minor_controlled_loss",
                input_count: 1,
                success: true,
                origin: None,
                phase: TransformPhase::RequestEncode,
                semantic_unit: TransformSemanticUnit::Metadata,
                outcome: TransformOutcomeKind::ControlledLossMinor,
                action: TransformAction::Drop,
                reason_code: TransformReasonCode::UnsupportedContent,
            },
            ContractExecution::from_result(transform_request_data(
                json!({
                    "model":"gpt-4.1",
                    "messages":[{"role":"user","content":"hello"}],
                    "user":"operator-tag"
                }),
                DownstreamProtocol::Openai,
                UpstreamProtocol::Gemini,
                false,
            )),
        ),
        build_case_report(
            ContractExpectation {
                name: "major_loss_reject",
                input_count: 1,
                success: false,
                origin: Some(TransformFailureOrigin::TargetCapability),
                phase: TransformPhase::RequestEncode,
                semantic_unit: TransformSemanticUnit::StructuredOutput,
                outcome: TransformOutcomeKind::ExplicitReject,
                action: TransformAction::Reject,
                reason_code: TransformReasonCode::UnsupportedContent,
            },
            ContractExecution::from_result(transform_request_data(
                json!({
                    "model":"gpt-4.1",
                    "messages":[{"role":"user","content":"hello"}],
                    "response_format":{"type":"json_object"}
                }),
                DownstreamProtocol::Openai,
                UpstreamProtocol::Gemini,
                false,
            )),
        ),
        build_case_report(
            ContractExpectation {
                name: "legal_synthesis",
                input_count: 1,
                success: true,
                origin: None,
                phase: TransformPhase::RequestDecode,
                semantic_unit: TransformSemanticUnit::Role,
                outcome: TransformOutcomeKind::Lossless,
                action: TransformAction::Synthesize,
                reason_code: TransformReasonCode::SyntheticEnvelope,
            },
            ContractExecution::from_result(transform_request_data(
                json!({"contents":[{"parts":[{"text":"hello"}]}]}),
                DownstreamProtocol::Gemini,
                UpstreamProtocol::Openai,
                false,
            )),
        ),
        build_case_report(
            ContractExpectation {
                name: "diagnostic_cap_overflow",
                input_count: 1,
                success: true,
                origin: None,
                phase: TransformPhase::RequestDecode,
                semantic_unit: TransformSemanticUnit::Metadata,
                outcome: TransformOutcomeKind::Lossless,
                action: TransformAction::Synthesize,
                reason_code: TransformReasonCode::SyntheticCorrelationId,
            },
            ContractExecution::from_result(transform_request_data(
                responses_request_with_missing_item_ids(40),
                DownstreamProtocol::Responses,
                UpstreamProtocol::Openai,
                false,
            )),
        ),
    ]
}

fn execute_stream_encode_failure() -> ContractExecution {
    let mut transformer =
        StreamTransformer::new(UpstreamProtocol::Anthropic, DownstreamProtocol::Openai);
    let frames = [
        sse(json!({
            "type":"message_start",
            "message":{
                "id":"msg_quality",
                "type":"message",
                "role":"assistant",
                "content":[],
                "model":"claude-sonnet"
            }
        })
        .to_string()),
        sse(json!({
            "type":"content_block_start",
            "index":0,
            "content_block":{"type":"thinking","thinking":"","signature":null}
        })
        .to_string()),
    ];

    let mut summaries = Vec::new();
    let mut successful_input_count = 0;
    let mut failed_input_count = 0;
    let mut failure = None;
    for frame in frames {
        match transformer.transform_event(frame) {
            Ok(success) => {
                successful_input_count += 1;
                summaries.push(success.summary);
            }
            Err(error) => {
                failed_input_count += 1;
                summaries.push(error.summary.clone());
                failure = Some(FailureObservation {
                    origin: error.origin,
                    phase: error.phase,
                    semantic_unit: error.semantic_unit,
                    reason_code: error.reason_code,
                });
                break;
            }
        }
    }

    ContractExecution {
        accounted_input_count: successful_input_count + failed_input_count,
        successful_input_count,
        failed_input_count,
        summary: merge_transform_summaries(summaries),
        failure,
    }
}

fn responses_request_with_missing_item_ids(item_count: usize) -> Value {
    let input = (0..item_count)
        .map(|index| {
            json!({
                "type":"message",
                "role":"user",
                "status":"completed",
                "content":[{"type":"input_text","text":format!("item-{index}")}]
            })
        })
        .collect::<Vec<_>>();
    json!({"model":"gpt-4.1","input":input})
}

fn sse(data: impl Into<String>) -> SseEvent {
    SseEvent {
        data: data.into(),
        ..Default::default()
    }
}

fn build_case_report(
    expected: ContractExpectation,
    execution: ContractExecution,
) -> TransformContractCaseReport {
    let outcome_counts = outcome_counts(&execution.summary);
    let action_counts = action_counts(&execution.summary);
    let semantic_counts = semantic_counts(&execution.summary);
    let retained_fact_count = execution.summary.facts.len() as u64;
    let accounting_closed = summary_accounting_closed(&execution.summary)
        && execution.accounted_input_count
            == execution.successful_input_count + execution.failed_input_count
        && execution.accounted_input_count == expected.input_count;
    let observed_success = execution.failed_input_count == 0;
    let expected_fact_observed = execution.summary.facts.iter().any(|fact| {
        fact.phase == expected.phase
            && fact.semantic_unit == expected.semantic_unit
            && fact.outcome == expected.outcome
            && fact.action == expected.action
            && fact.reason_code == expected.reason_code
    });
    let failure_matches = match (expected.origin, execution.failure.as_ref()) {
        (None, None) => true,
        (Some(origin), Some(failure)) => {
            failure.origin == origin
                && failure.phase == expected.phase
                && failure.semantic_unit == expected.semantic_unit
                && failure.reason_code == expected.reason_code
        }
        _ => false,
    };
    let overflow_matches = expected.name != "diagnostic_cap_overflow"
        || (execution.summary.facts.len() == super::super::diagnostics::TRANSFORM_DIAGNOSTIC_LIMIT
            && execution.summary.dropped_diagnostic_count > 0);
    let passed = observed_success == expected.success
        && failure_matches
        && expected_fact_observed
        && accounting_closed
        && overflow_matches;

    TransformContractCaseReport {
        contract_name: expected.name.to_string(),
        expected_success: expected.success,
        expected_origin: expected.origin.map(|origin| origin.as_str().to_string()),
        expected_phase: expected.phase.as_str().to_string(),
        expected_semantic_unit: expected.semantic_unit.as_str().to_string(),
        expected_outcome: expected.outcome.as_str().to_string(),
        expected_action: expected.action.as_str().to_string(),
        expected_reason_code: expected.reason_code.as_str().to_string(),
        observed_success,
        observed_origin: execution
            .failure
            .as_ref()
            .map(|failure| failure.origin.as_str().to_string()),
        observed_phase: execution
            .failure
            .as_ref()
            .map(|failure| failure.phase.as_str().to_string()),
        observed_semantic_unit: execution
            .failure
            .as_ref()
            .map(|failure| failure.semantic_unit.as_str().to_string()),
        observed_reason_code: execution
            .failure
            .as_ref()
            .map(|failure| failure.reason_code.as_str().to_string()),
        input_count: expected.input_count,
        accounted_input_count: execution.accounted_input_count,
        total_fact_count: execution.summary.total_fact_count,
        retained_fact_count,
        dropped_diagnostic_count: execution.summary.dropped_diagnostic_count,
        outcome_counts,
        action_counts,
        semantic_counts,
        accounting_closed,
        passed,
    }
}

pub(super) fn summarize_contract_cases(
    cases: Vec<TransformContractCaseReport>,
) -> TransformContractSummary {
    let case_count = cases.len();
    let passed_case_count = cases.iter().filter(|case| case.passed).count();
    let input_count = cases.iter().map(|case| case.input_count).sum();
    let accounted_input_count = cases.iter().map(|case| case.accounted_input_count).sum();
    let total_fact_count = cases.iter().map(|case| case.total_fact_count).sum();
    let retained_fact_count = cases.iter().map(|case| case.retained_fact_count).sum();
    let dropped_diagnostic_count = cases.iter().map(|case| case.dropped_diagnostic_count).sum();
    let outcome_counts = merge_string_counts(cases.iter().map(|case| &case.outcome_counts));
    let action_counts = merge_string_counts(cases.iter().map(|case| &case.action_counts));
    let semantic_counts = merge_string_counts(cases.iter().map(|case| &case.semantic_counts));
    let required_contracts_present = REQUIRED_CONTRACTS.iter().all(|required| {
        cases
            .iter()
            .any(|case| case.contract_name.as_str() == *required)
    });
    let accounting_closed = cases.iter().all(|case| case.accounting_closed)
        && input_count == accounted_input_count
        && total_fact_count == outcome_counts.values().sum::<u64>()
        && total_fact_count == action_counts.values().sum::<u64>()
        && total_fact_count == semantic_counts.values().sum::<u64>()
        && total_fact_count == retained_fact_count + dropped_diagnostic_count;
    let passed = case_count == REQUIRED_CONTRACTS.len()
        && required_contracts_present
        && passed_case_count == case_count
        && accounting_closed;

    TransformContractSummary {
        format_version: CONTRACT_FORMAT_VERSION,
        required_case_count: REQUIRED_CONTRACTS.len(),
        case_count,
        passed_case_count,
        input_count,
        accounted_input_count,
        total_fact_count,
        retained_fact_count,
        dropped_diagnostic_count,
        outcome_counts,
        action_counts,
        semantic_counts,
        accounting_closed,
        payload_free: true,
        passed,
        cases,
    }
}

fn summary_accounting_closed(summary: &TransformOutcomeSummary) -> bool {
    let retained_fact_count = summary.facts.len() as u64;
    let overflow_outcome_count = summary.overflow_outcome_counts.values().sum::<u64>();
    let overflow_semantic_count = summary.overflow_semantic_counts.values().sum::<u64>();
    summary.total_fact_count == summary.outcome_counts.values().sum::<u64>()
        && summary.total_fact_count == summary.action_counts.values().sum::<u64>()
        && summary.total_fact_count == summary.semantic_counts.values().sum::<u64>()
        && summary.total_fact_count == retained_fact_count + summary.dropped_diagnostic_count
        && summary.dropped_diagnostic_count == overflow_outcome_count
        && summary.dropped_diagnostic_count == overflow_semantic_count
}

fn outcome_counts(summary: &TransformOutcomeSummary) -> BTreeMap<String, u64> {
    summary
        .outcome_counts
        .iter()
        .map(|(kind, count)| (kind.as_str().to_string(), *count))
        .collect()
}

fn action_counts(summary: &TransformOutcomeSummary) -> BTreeMap<String, u64> {
    summary
        .action_counts
        .iter()
        .map(|(action, count)| (action.as_str().to_string(), *count))
        .collect()
}

fn semantic_counts(summary: &TransformOutcomeSummary) -> BTreeMap<String, u64> {
    summary
        .semantic_counts
        .iter()
        .map(|(unit, count)| (unit.as_str().to_string(), *count))
        .collect()
}

fn merge_string_counts<'a>(
    counts: impl IntoIterator<Item = &'a BTreeMap<String, u64>>,
) -> BTreeMap<String, u64> {
    let mut merged = BTreeMap::new();
    for counts in counts {
        for (key, count) in counts {
            *merged.entry(key.clone()).or_default() += count;
        }
    }
    merged
}
