use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::TransformProtocol;
use super::capability::TransformValueKind;
use super::policy::{PolicyDecision, PolicyEngine};
use super::stream::StreamTransformContext;

pub(crate) const TRANSFORM_DIAGNOSTIC_LIMIT: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransformOutcomeKind {
    Passthrough,
    Lossless,
    ControlledLossMinor,
    ControlledLossMajor,
    ExplicitReject,
    FatalError,
    ObservationDegraded,
}

impl TransformOutcomeKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::Lossless => "lossless",
            Self::ControlledLossMinor => "controlled_loss_minor",
            Self::ControlledLossMajor => "controlled_loss_major",
            Self::ExplicitReject => "explicit_reject",
            Self::FatalError => "fatal_error",
            Self::ObservationDegraded => "observation_degraded",
        }
    }

    const fn severity(self) -> TransformSeverity {
        match self {
            Self::Passthrough | Self::Lossless => TransformSeverity::Debug,
            Self::ControlledLossMinor | Self::ControlledLossMajor | Self::ObservationDegraded => {
                TransformSeverity::Warning
            }
            Self::ExplicitReject | Self::FatalError => TransformSeverity::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransformAction {
    PassThrough,
    Send,
    Drop,
    Synthesize,
    Reject,
    Terminate,
}

impl TransformAction {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::PassThrough => "pass_through",
            Self::Send => "send",
            Self::Drop => "drop",
            Self::Synthesize => "synthesize",
            Self::Reject => "reject",
            Self::Terminate => "terminate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransformPhase {
    RequestDecode,
    RequestEncode,
    ResponseObserve,
    ResponseDecode,
    ResponseEncode,
    StreamDecode,
    StreamEncode,
}

impl TransformPhase {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RequestDecode => "request_decode",
            Self::RequestEncode => "request_encode",
            Self::ResponseObserve => "response_observe",
            Self::ResponseDecode => "response_decode",
            Self::ResponseEncode => "response_encode",
            Self::StreamDecode => "stream_decode",
            Self::StreamEncode => "stream_encode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransformSemanticUnit {
    RequestEnvelope,
    ResponseEnvelope,
    StreamFrame,
    TopKParameter,
    ToolDefinitions,
    ToolRoleMessage,
    ResponsesUnknownItem,
    Text,
    Refusal,
    ImageUrl,
    ImageData,
    AudioData,
    FileUrl,
    FileData,
    FileId,
    ExecutableCode,
    ToolCall,
    ToolResult,
    ReasoningContent,
    ImageDelta,
    ToolCallDelta,
    ReasoningDelta,
    BlobDelta,
    StreamError,
    Usage,
    Model,
    Role,
    StructuredOutput,
    Metadata,
    Lifecycle,
}

impl TransformSemanticUnit {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RequestEnvelope => "request_envelope",
            Self::ResponseEnvelope => "response_envelope",
            Self::StreamFrame => "stream_frame",
            Self::TopKParameter => "top_k_parameter",
            Self::ToolDefinitions => "tool_definitions",
            Self::ToolRoleMessage => "tool_role_message",
            Self::ResponsesUnknownItem => "responses_unknown_item",
            Self::Text => "text",
            Self::Refusal => "refusal",
            Self::ImageUrl => "image_url",
            Self::ImageData => "image_data",
            Self::AudioData => "audio_data",
            Self::FileUrl => "file_url",
            Self::FileData => "file_data",
            Self::FileId => "file_id",
            Self::ExecutableCode => "executable_code",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::ReasoningContent => "reasoning_content",
            Self::ImageDelta => "image_delta",
            Self::ToolCallDelta => "tool_call_delta",
            Self::ReasoningDelta => "reasoning_delta",
            Self::BlobDelta => "blob_delta",
            Self::StreamError => "stream_error",
            Self::Usage => "usage",
            Self::Model => "model",
            Self::Role => "role",
            Self::StructuredOutput => "structured_output",
            Self::Metadata => "metadata",
            Self::Lifecycle => "lifecycle",
        }
    }
}

impl From<TransformValueKind> for TransformSemanticUnit {
    fn from(kind: TransformValueKind) -> Self {
        match kind {
            TransformValueKind::TopKParameter => Self::TopKParameter,
            TransformValueKind::ToolDefinitions => Self::ToolDefinitions,
            TransformValueKind::ToolRoleMessage => Self::ToolRoleMessage,
            TransformValueKind::ResponsesUnknownItem => Self::ResponsesUnknownItem,
            TransformValueKind::Text => Self::Text,
            TransformValueKind::Refusal => Self::Refusal,
            TransformValueKind::ImageUrl => Self::ImageUrl,
            TransformValueKind::ImageData => Self::ImageData,
            TransformValueKind::AudioData => Self::AudioData,
            TransformValueKind::FileUrl => Self::FileUrl,
            TransformValueKind::FileData => Self::FileData,
            TransformValueKind::FileId => Self::FileId,
            TransformValueKind::ExecutableCode => Self::ExecutableCode,
            TransformValueKind::ToolCall => Self::ToolCall,
            TransformValueKind::ToolResult => Self::ToolResult,
            TransformValueKind::ReasoningHistory | TransformValueKind::ReasoningContent => {
                Self::ReasoningContent
            }
            TransformValueKind::ImageDelta => Self::ImageDelta,
            TransformValueKind::ToolCallDelta => Self::ToolCallDelta,
            TransformValueKind::ReasoningDelta => Self::ReasoningDelta,
            TransformValueKind::BlobDelta => Self::BlobDelta,
            TransformValueKind::StreamError => Self::StreamError,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransformReasonCode {
    SameWirePassthrough,
    PolicyOverride,
    LosslessConversion,
    UnsupportedTopK,
    UnsupportedToolDefinitions,
    UnsupportedToolRoleMessage,
    UnsupportedToolCallDelta,
    UnsupportedReasoning,
    UnsupportedBlobDelta,
    UnsupportedStructuredError,
    UnsupportedRefusal,
    UnsupportedContent,
    UnsupportedImageDetail,
    MediaDetailNotPortable,
    MediaFilenameNotPortable,
    MediaDataUrlParametersNotPortable,
    TextDocumentMimeNormalized,
    StructuredOutputMetadataDropped,
    StructuredOutputOuterMetadataNotPortable,
    StructuredOutputEnvelopeNotPortable,
    GeminiPropertyOrderingDropped,
    UnsupportedImageDelta,
    UnknownSemanticUnit,
    DeterministicTextDowngrade,
    ObservationParseFailed,
    SourceDecodeFailed,
    TargetEncodeFailed,
    TargetSerializeFailed,
    InvalidProtocolShape,
    DiagnosticOverflow,
    SyntheticEnvelope,
    SyntheticAnthropicMaxTokens,
    SyntheticCorrelationId,
    SyntheticIndex,
    SystemInstructionMerged,
    ConsecutiveRoleMerged,
    ToolStrictnessNotPortable,
    ParallelToolPolicyNotPortable,
    ThoughtSignatureNotPortable,
    ThoughtSignatureRequired,
    ToolCorrelationSeedRequired,
    ReasoningEffortClampedToHigh,
    NoSemanticOutput,
    UpstreamUsageMissing,
    UpstreamApplicationFailed,
    IllegalUpstreamTerminal,
    UnknownIncompleteReason,
    UnknownStopReason,
    UsageComponentConflict,
    UsageSnapshotRegressed,
    UsageOverflow,
    StreamEventTypeMismatch,
}

impl TransformReasonCode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::SameWirePassthrough => "same_wire_passthrough",
            Self::PolicyOverride => "policy_override",
            Self::LosslessConversion => "lossless_conversion",
            Self::UnsupportedTopK => "unsupported_top_k",
            Self::UnsupportedToolDefinitions => "unsupported_tool_definitions",
            Self::UnsupportedToolRoleMessage => "unsupported_tool_role_message",
            Self::UnsupportedToolCallDelta => "unsupported_tool_call_delta",
            Self::UnsupportedReasoning => "unsupported_reasoning",
            Self::UnsupportedBlobDelta => "unsupported_blob_delta",
            Self::UnsupportedStructuredError => "unsupported_structured_error",
            Self::UnsupportedRefusal => "unsupported_refusal",
            Self::UnsupportedContent => "unsupported_content",
            Self::UnsupportedImageDetail => "unsupported_image_detail",
            Self::MediaDetailNotPortable => "media_detail_not_portable",
            Self::MediaFilenameNotPortable => "media_filename_not_portable",
            Self::MediaDataUrlParametersNotPortable => "media_data_url_parameters_not_portable",
            Self::TextDocumentMimeNormalized => "text_document_mime_normalized",
            Self::StructuredOutputMetadataDropped => "structured_output_metadata_dropped",
            Self::StructuredOutputOuterMetadataNotPortable => {
                "structured_output_outer_metadata_not_portable"
            }
            Self::StructuredOutputEnvelopeNotPortable => "structured_output_envelope_not_portable",
            Self::GeminiPropertyOrderingDropped => "gemini_property_ordering_dropped",
            Self::UnsupportedImageDelta => "unsupported_image_delta",
            Self::UnknownSemanticUnit => "unknown_semantic_unit",
            Self::DeterministicTextDowngrade => "deterministic_text_downgrade",
            Self::ObservationParseFailed => "observation_parse_failed",
            Self::SourceDecodeFailed => "source_decode_failed",
            Self::TargetEncodeFailed => "target_encode_failed",
            Self::TargetSerializeFailed => "target_serialize_failed",
            Self::InvalidProtocolShape => "invalid_protocol_shape",
            Self::DiagnosticOverflow => "diagnostic_overflow",
            Self::SyntheticEnvelope => "synthetic_envelope",
            Self::SyntheticAnthropicMaxTokens => "synthetic_anthropic_max_tokens",
            Self::SyntheticCorrelationId => "synthetic_correlation_id",
            Self::SyntheticIndex => "synthetic_index",
            Self::SystemInstructionMerged => "system_instruction_merged",
            Self::ConsecutiveRoleMerged => "consecutive_role_merged",
            Self::ToolStrictnessNotPortable => "tool_strictness_not_portable",
            Self::ParallelToolPolicyNotPortable => "parallel_tool_policy_not_portable",
            Self::ThoughtSignatureNotPortable => "thought_signature_not_portable",
            Self::ThoughtSignatureRequired => "thought_signature_required",
            Self::ToolCorrelationSeedRequired => "tool_correlation_seed_required",
            Self::ReasoningEffortClampedToHigh => "reasoning_effort_clamped_to_high",
            Self::NoSemanticOutput => "no_semantic_output",
            Self::UpstreamUsageMissing => "upstream_usage_missing",
            Self::UpstreamApplicationFailed => "upstream_application_failed",
            Self::IllegalUpstreamTerminal => "illegal_upstream_terminal",
            Self::UnknownIncompleteReason => "unknown_incomplete_reason",
            Self::UnknownStopReason => "unknown_stop_reason",
            Self::UsageComponentConflict => "usage_component_conflict",
            Self::UsageSnapshotRegressed => "usage_snapshot_regressed",
            Self::UsageOverflow => "usage_overflow",
            Self::StreamEventTypeMismatch => "stream_event_type_mismatch",
        }
    }
}

pub(crate) fn upstream_usage_missing_summary(phase: TransformPhase) -> TransformOutcomeSummary {
    let mut collector = TransformDiagnosticCollector::default();
    collector.record(TransformDiagnosticFact {
        sequence: 0,
        phase,
        semantic_unit: TransformSemanticUnit::Usage,
        outcome: TransformOutcomeKind::ObservationDegraded,
        action: TransformAction::PassThrough,
        reason_code: TransformReasonCode::UpstreamUsageMissing,
        safe_summary: None,
    });
    collector.into_summary()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TransformSeverity {
    Debug,
    Warning,
    Error,
}

impl TransformSeverity {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformSafeSummary {
    pub bytes: usize,
    pub sha256: String,
    pub top_level_field_count: usize,
    pub event_count: usize,
}

impl TransformSafeSummary {
    pub(crate) fn from_json(value: &Value) -> Self {
        let serialized = serde_json::to_vec(value)
            .expect("serde_json::Value serialization is structurally infallible");
        Self {
            bytes: serialized.len(),
            sha256: sha256_hex(&serialized),
            top_level_field_count: top_level_json_field_count(value),
            event_count: 0,
        }
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        let top_level_field_count = serde_json::from_slice::<Value>(bytes)
            .map(|value| top_level_json_field_count(&value))
            .unwrap_or(0);
        Self {
            bytes: bytes.len(),
            sha256: sha256_hex(bytes),
            top_level_field_count,
            event_count: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformDiagnosticFact {
    pub sequence: u64,
    pub phase: TransformPhase,
    pub semantic_unit: TransformSemanticUnit,
    pub outcome: TransformOutcomeKind,
    pub action: TransformAction,
    pub reason_code: TransformReasonCode,
    pub safe_summary: Option<TransformSafeSummary>,
}

impl TransformDiagnosticFact {
    pub(crate) fn from_policy(
        phase: TransformPhase,
        semantic_unit: TransformSemanticUnit,
        decision: PolicyDecision,
    ) -> Self {
        Self {
            sequence: 0,
            phase,
            semantic_unit,
            outcome: decision.outcome,
            action: decision.action,
            reason_code: decision.reason_code,
            safe_summary: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransformOutcomeSummary {
    pub facts: Vec<TransformDiagnosticFact>,
    first_control_fact: Option<TransformDiagnosticFact>,
    pub dropped_diagnostic_count: u64,
    pub total_fact_count: u64,
    pub max_severity: Option<TransformSeverity>,
    pub outcome_counts: BTreeMap<TransformOutcomeKind, u64>,
    pub action_counts: BTreeMap<TransformAction, u64>,
    pub semantic_counts: BTreeMap<TransformSemanticUnit, u64>,
    pub overflow_outcome_counts: BTreeMap<TransformOutcomeKind, u64>,
    pub overflow_semantic_counts: BTreeMap<TransformSemanticUnit, u64>,
}

impl TransformOutcomeSummary {
    pub(crate) fn control_fact(&self) -> Option<&TransformDiagnosticFact> {
        self.first_control_fact.as_ref().or_else(|| {
            self.facts.iter().find(|fact| {
                matches!(
                    fact.action,
                    TransformAction::Reject | TransformAction::Terminate
                )
            })
        })
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TransformDiagnosticCollector {
    summary: TransformOutcomeSummary,
}

impl TransformDiagnosticCollector {
    pub(crate) fn record(&mut self, mut fact: TransformDiagnosticFact) {
        fact.sequence = self.summary.total_fact_count;
        self.summary.total_fact_count += 1;
        *self.summary.outcome_counts.entry(fact.outcome).or_default() += 1;
        *self.summary.action_counts.entry(fact.action).or_default() += 1;
        *self
            .summary
            .semantic_counts
            .entry(fact.semantic_unit)
            .or_default() += 1;
        self.summary.max_severity = Some(
            self.summary
                .max_severity
                .map_or(fact.outcome.severity(), |severity| {
                    severity.max(fact.outcome.severity())
                }),
        );

        if self.summary.first_control_fact.is_none()
            && matches!(
                fact.action,
                TransformAction::Reject | TransformAction::Terminate
            )
        {
            self.summary.first_control_fact = Some(fact.clone());
        }

        if self.summary.facts.len() < TRANSFORM_DIAGNOSTIC_LIMIT {
            self.summary.facts.push(fact);
        } else {
            self.summary.dropped_diagnostic_count += 1;
            *self
                .summary
                .overflow_outcome_counts
                .entry(fact.outcome)
                .or_default() += 1;
            *self
                .summary
                .overflow_semantic_counts
                .entry(fact.semantic_unit)
                .or_default() += 1;
        }
    }

    pub(crate) fn snapshot(&self) -> TransformOutcomeSummary {
        self.summary.clone()
    }

    pub(crate) fn absorb(&mut self, summary: TransformOutcomeSummary) {
        let sequence_offset = self.summary.total_fact_count;
        let incoming_control_fact = summary.control_fact().cloned();
        if summary.dropped_diagnostic_count == 0
            && summary.total_fact_count == summary.facts.len() as u64
            && summary.overflow_outcome_counts.is_empty()
            && summary.overflow_semantic_counts.is_empty()
        {
            for fact in summary.facts {
                self.record(fact);
            }
            return;
        }

        let retained_outcomes = count_retained_outcomes(&summary.facts);
        let retained_actions = count_retained_actions(&summary.facts);
        let retained_semantics = count_retained_semantics(&summary.facts);

        for fact in summary.facts {
            self.record(fact);
        }

        let missing = summary
            .total_fact_count
            .saturating_sub(retained_outcomes.values().sum());
        self.summary.total_fact_count += missing;
        self.summary.dropped_diagnostic_count += missing;
        self.summary.max_severity = self.summary.max_severity.max(summary.max_severity);
        if self.summary.first_control_fact.is_none()
            && let Some(mut fact) = incoming_control_fact
        {
            fact.sequence = sequence_offset.saturating_add(fact.sequence);
            self.summary.first_control_fact = Some(fact);
        }
        absorb_count_delta(
            &mut self.summary.outcome_counts,
            &mut self.summary.overflow_outcome_counts,
            summary.outcome_counts,
            retained_outcomes,
        );
        absorb_count_delta(
            &mut self.summary.action_counts,
            &mut BTreeMap::new(),
            summary.action_counts,
            retained_actions,
        );
        absorb_count_delta(
            &mut self.summary.semantic_counts,
            &mut self.summary.overflow_semantic_counts,
            summary.semantic_counts,
            retained_semantics,
        );
    }

    pub(crate) fn into_summary(self) -> TransformOutcomeSummary {
        self.summary
    }
}

fn count_retained_outcomes(
    facts: &[TransformDiagnosticFact],
) -> BTreeMap<TransformOutcomeKind, u64> {
    let mut counts = BTreeMap::new();
    for fact in facts {
        *counts.entry(fact.outcome).or_default() += 1;
    }
    counts
}

fn count_retained_actions(facts: &[TransformDiagnosticFact]) -> BTreeMap<TransformAction, u64> {
    let mut counts = BTreeMap::new();
    for fact in facts {
        *counts.entry(fact.action).or_default() += 1;
    }
    counts
}

fn count_retained_semantics(
    facts: &[TransformDiagnosticFact],
) -> BTreeMap<TransformSemanticUnit, u64> {
    let mut counts = BTreeMap::new();
    for fact in facts {
        *counts.entry(fact.semantic_unit).or_default() += 1;
    }
    counts
}

fn absorb_count_delta<K: Ord + Copy>(
    totals: &mut BTreeMap<K, u64>,
    overflow: &mut BTreeMap<K, u64>,
    incoming: BTreeMap<K, u64>,
    retained: BTreeMap<K, u64>,
) {
    for (key, incoming_count) in incoming {
        let delta = incoming_count.saturating_sub(retained.get(&key).copied().unwrap_or(0));
        if delta > 0 {
            *totals.entry(key).or_default() += delta;
            *overflow.entry(key).or_default() += delta;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformFailureOrigin {
    DownstreamInput,
    TargetCapability,
    UpstreamPayload,
    TargetEncoding,
    InternalInvariant,
}

impl TransformFailureOrigin {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::DownstreamInput => "downstream_input",
            Self::TargetCapability => "target_capability",
            Self::UpstreamPayload => "upstream_payload",
            Self::TargetEncoding => "target_encoding",
            Self::InternalInvariant => "internal_invariant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformFailure {
    pub origin: TransformFailureOrigin,
    pub phase: TransformPhase,
    pub semantic_unit: TransformSemanticUnit,
    pub reason_code: TransformReasonCode,
    pub summary: TransformOutcomeSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformSuccess<T> {
    pub value: T,
    pub summary: TransformOutcomeSummary,
}

pub type TransformResult<T> = Result<TransformSuccess<T>, TransformFailure>;

pub(in crate::service::transform) fn transform_success<T>(
    value: T,
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
    outcome: TransformOutcomeKind,
    action: TransformAction,
    reason_code: TransformReasonCode,
) -> TransformSuccess<T> {
    let mut collector = TransformDiagnosticCollector::default();
    collector.record(TransformDiagnosticFact {
        sequence: 0,
        phase,
        semantic_unit,
        outcome,
        action,
        reason_code,
        safe_summary: None,
    });
    TransformSuccess {
        value,
        summary: collector.into_summary(),
    }
}

pub(crate) fn transform_failure(
    origin: TransformFailureOrigin,
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
    reason_code: TransformReasonCode,
    safe_summary: Option<TransformSafeSummary>,
) -> TransformFailure {
    let (outcome, action) = match origin {
        TransformFailureOrigin::DownstreamInput | TransformFailureOrigin::TargetCapability => (
            TransformOutcomeKind::ExplicitReject,
            TransformAction::Reject,
        ),
        TransformFailureOrigin::UpstreamPayload
        | TransformFailureOrigin::TargetEncoding
        | TransformFailureOrigin::InternalInvariant => {
            (TransformOutcomeKind::FatalError, TransformAction::Terminate)
        }
    };
    let mut collector = TransformDiagnosticCollector::default();
    collector.record(TransformDiagnosticFact {
        sequence: 0,
        phase,
        semantic_unit,
        outcome,
        action,
        reason_code,
        safe_summary,
    });
    TransformFailure {
        origin,
        phase,
        semantic_unit,
        reason_code,
        summary: collector.into_summary(),
    }
}

pub(in crate::service::transform) fn merge_transform_summaries(
    summaries: impl IntoIterator<Item = TransformOutcomeSummary>,
) -> TransformOutcomeSummary {
    let mut summaries = summaries
        .into_iter()
        .filter(|summary| summary.total_fact_count != 0);
    let Some(first) = summaries.next() else {
        return TransformOutcomeSummary::default();
    };
    let mut collector = TransformDiagnosticCollector { summary: first };
    for summary in summaries {
        collector.absorb(summary);
    }
    collector.into_summary()
}

fn sha256_hex(body: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(body.as_ref()))
}

fn top_level_json_field_count(value: &Value) -> usize {
    match value {
        Value::Object(map) => map.len(),
        Value::Array(items) => items.len(),
        Value::Null => 0,
        Value::Bool(_) | Value::Number(_) | Value::String(_) => 1,
    }
}

thread_local! {
    static TRANSFORM_DIAGNOSTIC_STACK: RefCell<Vec<TransformDiagnosticCollector>> = const {
        RefCell::new(Vec::new())
    };
}

pub(in crate::service::transform) fn capture_transform_diagnostics<T>(
    f: impl FnOnce() -> T,
) -> (T, TransformOutcomeSummary) {
    TRANSFORM_DIAGNOSTIC_STACK.with(|stack| {
        stack
            .borrow_mut()
            .push(TransformDiagnosticCollector::default());
    });

    let result = f();
    let collector = TRANSFORM_DIAGNOSTIC_STACK.with(|stack| {
        stack
            .borrow_mut()
            .pop()
            .expect("transform diagnostic capture stack must remain balanced")
    });

    (result, collector.into_summary())
}

pub(in crate::service::transform) fn record_captured_transform_fact(fact: TransformDiagnosticFact) {
    TRANSFORM_DIAGNOSTIC_STACK.with(|stack| {
        if let Some(active_capture) = stack.borrow_mut().last_mut() {
            active_capture.record(fact);
        }
    });
}

pub(in crate::service::transform) fn record_stream_diagnostic(
    stream_context: &mut StreamTransformContext<'_>,
    source: TransformProtocol,
    target: TransformProtocol,
    kind: TransformValueKind,
) {
    let decision = PolicyEngine::evaluate(source, target, kind);
    let fact =
        TransformDiagnosticFact::from_policy(TransformPhase::StreamEncode, kind.into(), decision);
    stream_context.record_diagnostic(fact.clone());
    record_captured_transform_fact(fact);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(
        outcome: TransformOutcomeKind,
        action: TransformAction,
        semantic_unit: TransformSemanticUnit,
    ) -> TransformDiagnosticFact {
        TransformDiagnosticFact {
            sequence: 0,
            phase: TransformPhase::StreamEncode,
            semantic_unit,
            outcome,
            action,
            reason_code: TransformReasonCode::LosslessConversion,
            safe_summary: None,
        }
    }

    #[test]
    fn collector_preserves_order_caps_details_and_counts_overflow() {
        let mut collector = TransformDiagnosticCollector::default();
        for index in 0..35 {
            collector.record(fact(
                if index == 34 {
                    TransformOutcomeKind::FatalError
                } else {
                    TransformOutcomeKind::Lossless
                },
                if index == 34 {
                    TransformAction::Terminate
                } else {
                    TransformAction::Send
                },
                TransformSemanticUnit::StreamFrame,
            ));
        }

        let summary = collector.into_summary();
        assert_eq!(summary.facts.len(), TRANSFORM_DIAGNOSTIC_LIMIT);
        assert_eq!(summary.dropped_diagnostic_count, 3);
        assert_eq!(summary.total_fact_count, 35);
        assert_eq!(summary.facts[0].sequence, 0);
        assert_eq!(summary.facts[31].sequence, 31);
        assert_eq!(summary.outcome_counts[&TransformOutcomeKind::Lossless], 34);
        assert_eq!(
            summary.overflow_outcome_counts[&TransformOutcomeKind::FatalError],
            1
        );
        assert_eq!(summary.max_severity, Some(TransformSeverity::Error));
        let control_fact = summary
            .control_fact()
            .expect("terminal control fact must survive the detail cap");
        assert_eq!(control_fact.sequence, 34);
        assert_eq!(control_fact.action, TransformAction::Terminate);
    }

    #[test]
    fn merge_preserves_control_fact_when_both_detail_windows_are_full() {
        let mut first = TransformDiagnosticCollector::default();
        let mut second = TransformDiagnosticCollector::default();
        for _ in 0..TRANSFORM_DIAGNOSTIC_LIMIT {
            first.record(fact(
                TransformOutcomeKind::Lossless,
                TransformAction::Send,
                TransformSemanticUnit::StreamFrame,
            ));
            second.record(fact(
                TransformOutcomeKind::Lossless,
                TransformAction::Send,
                TransformSemanticUnit::StreamFrame,
            ));
        }
        second.record(fact(
            TransformOutcomeKind::ExplicitReject,
            TransformAction::Reject,
            TransformSemanticUnit::ReasoningContent,
        ));

        let summary = merge_transform_summaries([first.into_summary(), second.into_summary()]);

        assert_eq!(summary.facts.len(), TRANSFORM_DIAGNOSTIC_LIMIT);
        assert_eq!(summary.total_fact_count, 65);
        assert_eq!(summary.dropped_diagnostic_count, 33);
        let control_fact = summary
            .control_fact()
            .expect("merged rejection must remain available to control flow");
        assert_eq!(control_fact.sequence, 64);
        assert_eq!(control_fact.action, TransformAction::Reject);
        assert_eq!(
            control_fact.semantic_unit,
            TransformSemanticUnit::ReasoningContent
        );
    }

    #[test]
    fn safe_summary_contains_shape_not_payload_values() {
        let summary = TransformSafeSummary::from_json(&serde_json::json!({
            "secret": "do-not-log",
            "nested": {"arguments": "private"}
        }));

        assert_eq!(summary.top_level_field_count, 2);
        assert_eq!(summary.sha256.len(), 64);
        assert_ne!(summary.sha256, "do-not-log");
    }

    #[test]
    fn nested_captures_remain_isolated_and_balanced() {
        let (_, outer_summary) = capture_transform_diagnostics(|| {
            record_captured_transform_fact(fact(
                TransformOutcomeKind::Lossless,
                TransformAction::Send,
                TransformSemanticUnit::RequestEnvelope,
            ));
            let (_, inner_summary) = capture_transform_diagnostics(|| {
                record_captured_transform_fact(fact(
                    TransformOutcomeKind::ControlledLossMinor,
                    TransformAction::Drop,
                    TransformSemanticUnit::TopKParameter,
                ));
            });
            assert_eq!(inner_summary.total_fact_count, 1);
        });

        assert_eq!(outer_summary.total_fact_count, 1);
        assert_eq!(outer_summary.facts[0].sequence, 0);
    }
}
