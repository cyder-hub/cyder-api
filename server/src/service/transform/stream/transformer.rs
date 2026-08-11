use std::collections::BTreeMap;

use cyder_tools::log::{debug, warn};
use serde_json::Value;

use super::session::{SessionContext, StreamTransformContext};
use super::usage::UsageMergeStrategy;
use crate::cost::UsageNormalization;
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::service::transform::adapter::{
    DecodedSourceStreamFrame, DownstreamAdapter, UpstreamAdapter, downstream_adapter_for,
    upstream_adapter_for,
};
use crate::service::transform::diagnostics::{
    capture_transform_diagnostics, merge_transform_summaries, transform_failure, transform_success,
};
use crate::service::transform::unified::*;
use crate::service::transform::{
    TransformAction, TransformDiagnosticFact, TransformFailureOrigin, TransformOutcomeKind,
    TransformOutcomeSummary, TransformPhase, TransformReasonCode, TransformResult,
    TransformSemanticUnit, TransformSuccess,
};
use crate::utils::sse::SseEvent;
use crate::utils::usage::{self, UsageInfo};

pub struct StreamTransformer {
    pub(in crate::service::transform) upstream_protocol: UpstreamProtocol,
    pub(in crate::service::transform) downstream_protocol: DownstreamProtocol,
    pub(in crate::service::transform) session: SessionContext,
    last_meaningful_output_observed: bool,
    terminal_failure: Option<crate::service::transform::TransformFailure>,
    stream_summary: crate::service::transform::TransformDiagnosticCollector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StreamFrameDisposition {
    EmptyFrame,
    LifecycleSent,
    LifecycleNoOutput,
    SemanticNoOutput,
    Passthrough,
    ObservationDegraded,
    ControlledLoss,
    Sent,
}

#[derive(Debug)]
pub struct StreamTransformOutput {
    pub events: Vec<SseEvent>,
    pub meaningful_output_observed: bool,
    pub disposition: StreamFrameDisposition,
}

impl std::ops::Deref for StreamTransformOutput {
    type Target = [SseEvent];

    fn deref(&self) -> &Self::Target {
        &self.events
    }
}

impl IntoIterator for StreamTransformOutput {
    type Item = SseEvent;
    type IntoIter = std::vec::IntoIter<SseEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

pub struct StreamBatchTransformOutput {
    pub events: Vec<SseEvent>,
    pub input_event_count: usize,
    pub accounted_input_count: usize,
    pub disposition_counts: BTreeMap<StreamFrameDisposition, usize>,
}

impl StreamTransformer {
    pub fn new(
        upstream_protocol: UpstreamProtocol,
        downstream_protocol: DownstreamProtocol,
    ) -> Self {
        Self {
            upstream_protocol,
            downstream_protocol,
            session: SessionContext::default(),
            last_meaningful_output_observed: false,
            terminal_failure: None,
            stream_summary: Default::default(),
        }
    }

    pub fn transform_event_with_observation(
        &mut self,
        event: SseEvent,
    ) -> TransformResult<StreamTransformOutput> {
        self.transform_event(event)
    }

    pub fn transform_events(
        &mut self,
        events: Vec<SseEvent>,
    ) -> TransformResult<StreamBatchTransformOutput> {
        let input_event_count = events.len();
        let mut transformed = Vec::new();
        let mut summaries = Vec::with_capacity(input_event_count);
        let mut accounted_input_count = 0;
        let mut disposition_counts = BTreeMap::new();
        for event in events {
            match self.transform_event(event) {
                Ok(success) => {
                    transformed.extend(success.value.events);
                    *disposition_counts
                        .entry(success.value.disposition)
                        .or_default() += 1;
                    summaries.push(success.summary);
                    accounted_input_count += 1;
                }
                Err(mut failure) => {
                    summaries.push(failure.summary);
                    failure.summary = merge_transform_summaries(summaries);
                    return Err(failure);
                }
            }
        }
        Ok(TransformSuccess {
            value: StreamBatchTransformOutput {
                events: transformed,
                input_event_count,
                accounted_input_count,
                disposition_counts,
            },
            summary: merge_transform_summaries(summaries),
        })
    }

    fn source_adapter(&self) -> &'static UpstreamAdapter {
        upstream_adapter_for(self.upstream_protocol)
    }

    fn target_adapter(&self) -> &'static DownstreamAdapter {
        downstream_adapter_for(self.downstream_protocol)
    }

    pub(in crate::service::transform) fn stream_context(&mut self) -> StreamTransformContext<'_> {
        StreamTransformContext::new(self.upstream_protocol, &mut self.session)
    }

    fn record_transformed_events(&mut self, events: &[SseEvent]) {
        for event in events {
            self.session.push_transformed_event(event.clone());
        }
    }

    pub(in crate::service::transform) fn usage_merge_strategy(&self) -> UsageMergeStrategy {
        match self.upstream_protocol {
            UpstreamProtocol::Gemini | UpstreamProtocol::Responses => UsageMergeStrategy::Replace,
            UpstreamProtocol::Openai | UpstreamProtocol::Anthropic | UpstreamProtocol::Ollama => {
                UsageMergeStrategy::FinalOnly
            }
        }
    }

    pub fn parse_usage_info(&mut self) -> Option<UsageInfo> {
        if let Some(usage) = self.session.usage_cache_clone() {
            return Some(usage);
        }

        if self.session.original_events_is_empty() {
            self.record_post_transform_diagnostic(TransformDiagnosticFact {
                sequence: 0,
                phase: TransformPhase::ResponseObserve,
                semantic_unit: TransformSemanticUnit::Usage,
                outcome: TransformOutcomeKind::ObservationDegraded,
                action: TransformAction::Drop,
                reason_code: TransformReasonCode::ObservationParseFailed,
                safe_summary: None,
            });
            debug!(
                "[transform][usage] stream_id={:?} provider={:?} no cached usage and no diagnostic events available",
                self.session.stream_id_clone(),
                self.upstream_protocol
            );
            return None;
        }

        let parsed = match self.upstream_protocol {
            UpstreamProtocol::Openai => self.session.original_events().iter().rev().find_map(|e| {
                if e.data == "[DONE]" || e.data.is_empty() {
                    return None;
                }
                serde_json::from_str::<Value>(&e.data)
                    .ok()
                    .and_then(|v| usage::parse_usage_info(&v, self.upstream_protocol))
            }),
            UpstreamProtocol::Gemini | UpstreamProtocol::Ollama | UpstreamProtocol::Responses => {
                self.session.original_events().iter().rev().find_map(|e| {
                    serde_json::from_str::<Value>(&e.data)
                        .ok()
                        .and_then(|v| usage::parse_usage_info(&v, self.upstream_protocol))
                })
            }
            UpstreamProtocol::Anthropic => self
                .session
                .original_events()
                .iter()
                .rev()
                .find(|e| {
                    if let Ok(value) = serde_json::from_str::<Value>(&e.data) {
                        value.get("type").and_then(|t| t.as_str()) == Some("message_stop")
                    } else {
                        false
                    }
                })
                .and_then(|e| {
                    serde_json::from_str::<Value>(&e.data)
                        .ok()
                        .and_then(|v| usage::parse_usage_info(&v, self.upstream_protocol))
                }),
        };

        if parsed.is_none() {
            self.record_post_transform_diagnostic(TransformDiagnosticFact {
                sequence: 0,
                phase: TransformPhase::ResponseObserve,
                semantic_unit: TransformSemanticUnit::Usage,
                outcome: TransformOutcomeKind::ObservationDegraded,
                action: TransformAction::Drop,
                reason_code: TransformReasonCode::ObservationParseFailed,
                safe_summary: None,
            });
            warn!(
                "[transform][usage] stream_id={:?} provider={:?} usage cache miss and diagnostic fallback failed; recent_original_events={}",
                self.session.stream_id_clone(),
                self.upstream_protocol,
                self.session.original_events_len()
            );
        }

        parsed
    }

    pub fn cached_usage_info(&self) -> Option<UsageInfo> {
        self.session.usage_cache_clone()
    }

    pub fn cached_usage_normalization(&self) -> Option<UsageNormalization> {
        self.session.usage_normalization_cache_clone()
    }

    pub fn parse_usage_normalization(&mut self) -> Option<UsageNormalization> {
        self.session.usage_normalization_cache_clone()
    }

    pub fn diagnostics_snapshot(&self) -> TransformOutcomeSummary {
        let summary = self.stream_summary.snapshot();
        if summary.total_fact_count == 0 {
            self.session.diagnostics_snapshot()
        } else {
            summary
        }
    }

    fn record_post_transform_diagnostic(&mut self, fact: TransformDiagnosticFact) {
        self.session.record_diagnostic(fact.clone());
        self.stream_summary.record(fact);
    }

    pub(crate) fn get_or_generate_stream_id(&mut self) -> String {
        self.session
            .get_or_generate_stream_id(self.upstream_protocol)
    }

    pub(crate) fn get_or_default_stream_model(&self) -> String {
        self.session
            .get_or_default_stream_model(self.upstream_protocol)
    }

    pub(in crate::service::transform) fn normalize_unified_chunk_session_state(
        &mut self,
        unified_chunk: &mut UnifiedChunkResponse,
    ) {
        let chunk_core = unified_chunk.core();
        self.session.set_stream_model_if_present(chunk_core.model);
        if self.upstream_protocol == UpstreamProtocol::Gemini {
            for choice in &mut unified_chunk.choices {
                for part in &mut choice.delta.content {
                    if let UnifiedContentPartDelta::ToolCallDelta(tool_call) = part {
                        let stable_id = self.session.get_or_create_gemini_tool_call_id(
                            choice.index,
                            tool_call.index,
                            tool_call.name.as_deref().unwrap_or(""),
                        );
                        tool_call.id = Some(stable_id.clone());
                        self.session.remember_tool_call_id(stable_id);
                    }
                }
                if choice.finish_reason.is_some() {
                    self.session.advance_gemini_message_index(choice.index);
                }
            }
        }

        if let Some(usage) = chunk_core.usage {
            self.session.merge_usage(usage, self.usage_merge_strategy());
        }
        if let Some(finish_reason) = unified_chunk
            .choices
            .iter()
            .find_map(|choice| choice.finish_reason.clone())
        {
            self.session.set_finish_reason_cache(Some(finish_reason));
        }
    }

    pub(crate) fn update_session_from_stream_events(&mut self, events: &[UnifiedStreamEvent]) {
        for event in events {
            self.update_session_from_stream_event(event);
        }
    }

    pub(crate) fn update_session_from_stream_event(&mut self, event: &UnifiedStreamEvent) {
        let strategy = self.usage_merge_strategy();
        self.session.update_from_stream_event(event, strategy);
    }

    fn stream_events_to_target_events(
        &mut self,
        stream_events: Vec<UnifiedStreamEvent>,
    ) -> TransformResult<Vec<SseEvent>> {
        if let Some(error) = stream_events.iter().find_map(|event| match event {
            UnifiedStreamEvent::Error { error } => Some(error),
            _ => None,
        }) {
            self.session.set_last_error(error.clone());
            return Err(transform_failure(
                TransformFailureOrigin::UpstreamPayload,
                TransformPhase::StreamDecode,
                TransformSemanticUnit::StreamError,
                TransformReasonCode::InvalidProtocolShape,
                None,
            ));
        }

        let target_adapter = self.target_adapter();
        let mut context = self.stream_context();
        (target_adapter.stream.encode_events)(stream_events, &mut context)
    }

    pub fn transform_event(&mut self, event: SseEvent) -> TransformResult<StreamTransformOutput> {
        if let Some(failure) = &self.terminal_failure {
            return Err(failure.clone());
        }

        let input_is_empty = event.data.is_empty();
        let input_is_done = event.data == "[DONE]";
        let original_event = (!input_is_empty).then(|| event.clone());
        let session_before = self.session.semantic_snapshot();
        let (result, captured_summary) =
            capture_transform_diagnostics(|| self.transform_event_inner(event));
        match result {
            Ok(mut success) => {
                success.summary = merge_transform_summaries([success.summary, captured_summary]);
                if let Some(fact) = success.summary.control_fact() {
                    let failure = crate::service::transform::TransformFailure {
                        origin: if fact.action == TransformAction::Reject {
                            TransformFailureOrigin::TargetCapability
                        } else {
                            TransformFailureOrigin::TargetEncoding
                        },
                        phase: fact.phase,
                        semantic_unit: fact.semantic_unit,
                        reason_code: fact.reason_code,
                        summary: success.summary,
                    };
                    self.session.restore_semantic_snapshot(session_before);
                    self.stream_summary.absorb(failure.summary.clone());
                    self.terminal_failure = Some(failure.clone());
                    return Err(failure);
                }
                let disposition = classify_stream_disposition(
                    input_is_empty,
                    input_is_done,
                    &success.value,
                    &success.summary,
                );
                if let Some(original_event) = original_event {
                    self.session.push_original_event(original_event);
                }
                self.record_transformed_events(&success.value);
                self.stream_summary.absorb(success.summary.clone());
                Ok(TransformSuccess {
                    value: StreamTransformOutput {
                        events: success.value,
                        meaningful_output_observed: self.last_meaningful_output_observed,
                        disposition,
                    },
                    summary: success.summary,
                })
            }
            Err(mut failure) => {
                failure.summary = merge_transform_summaries([failure.summary, captured_summary]);
                self.session.restore_semantic_snapshot(session_before);
                self.stream_summary.absorb(failure.summary.clone());
                self.terminal_failure = Some(failure.clone());
                Err(failure)
            }
        }
    }

    fn transform_event_inner(&mut self, event: SseEvent) -> TransformResult<Vec<SseEvent>> {
        self.last_meaningful_output_observed = false;
        if event.data.is_empty() {
            return Ok(transform_success(
                Vec::new(),
                TransformPhase::StreamDecode,
                TransformSemanticUnit::StreamFrame,
                TransformOutcomeKind::Lossless,
                TransformAction::Drop,
                TransformReasonCode::NoSemanticOutput,
            ));
        }

        // Handle OpenAI-compatible stream termination before same-wire observation,
        // because `[DONE]` is a valid marker rather than a JSON source frame.
        if self.upstream_protocol == UpstreamProtocol::Openai && event.data == "[DONE]" {
            let transformed = match self.downstream_protocol {
                DownstreamProtocol::Anthropic => {
                    self.stream_events_to_target_events(vec![UnifiedStreamEvent::MessageStop])
                }
                DownstreamProtocol::Gemini => Ok(transform_success(
                    Vec::new(),
                    TransformPhase::StreamDecode,
                    TransformSemanticUnit::Lifecycle,
                    TransformOutcomeKind::Lossless,
                    TransformAction::Drop,
                    TransformReasonCode::NoSemanticOutput,
                )),
                _ => Ok(transform_success(
                    vec![event],
                    TransformPhase::StreamDecode,
                    TransformSemanticUnit::Lifecycle,
                    TransformOutcomeKind::Lossless,
                    TransformAction::Send,
                    TransformReasonCode::LosslessConversion,
                )),
            };
            return transformed;
        }

        if matches!(
            (self.upstream_protocol, self.downstream_protocol),
            (UpstreamProtocol::Openai, DownstreamProtocol::Openai)
                | (UpstreamProtocol::Responses, DownstreamProtocol::Responses)
                | (UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic)
                | (UpstreamProtocol::Gemini, DownstreamProtocol::Gemini)
        ) {
            // Best effort to update session state from passthrough events (e.g. usage info)
            // is transactional as an observation: a rejected DTO must not retain
            // partially inferred lifecycle state, while the original frame remains
            // available for usage/diagnostic fallback.
            let observation_session_before = self.session.semantic_snapshot();
            let source_adapter = self.source_adapter();
            let decoded_frame = {
                let mut context = self.stream_context();
                (source_adapter.stream.decode_source)(&event.data, &mut context)
            };
            let observation_summary = match decoded_frame {
                Ok(success) => {
                    self.last_meaningful_output_observed =
                        success.value.meaningful_output_observed();
                    match success.value {
                        DecodedSourceStreamFrame::Events(stream_events) => {
                            self.update_session_from_stream_events(&stream_events);
                        }
                        DecodedSourceStreamFrame::LegacyChunk(mut unified_chunk) => {
                            self.normalize_unified_chunk_session_state(&mut unified_chunk);
                        }
                    }
                    success.summary
                }
                Err(failure) => {
                    self.session
                        .restore_semantic_snapshot(observation_session_before);
                    let fact = TransformDiagnosticFact {
                        sequence: 0,
                        phase: TransformPhase::ResponseObserve,
                        semantic_unit: TransformSemanticUnit::StreamFrame,
                        outcome: TransformOutcomeKind::ObservationDegraded,
                        action: TransformAction::PassThrough,
                        reason_code: TransformReasonCode::ObservationParseFailed,
                        safe_summary: failure
                            .summary
                            .facts
                            .first()
                            .and_then(|fact| fact.safe_summary.clone()),
                    };
                    self.session.record_diagnostic(fact.clone());
                    let mut collector =
                        crate::service::transform::TransformDiagnosticCollector::default();
                    collector.record(fact);
                    collector.into_summary()
                }
            };
            let passthrough = transform_success(
                vec![event],
                TransformPhase::StreamDecode,
                TransformSemanticUnit::StreamFrame,
                TransformOutcomeKind::Passthrough,
                TransformAction::PassThrough,
                TransformReasonCode::SameWirePassthrough,
            );
            return Ok(TransformSuccess {
                value: passthrough.value,
                summary: merge_transform_summaries([observation_summary, passthrough.summary]),
            });
        }

        let source_adapter = self.source_adapter();
        let target_adapter = self.target_adapter();

        let decoded_frame = {
            let mut context = self.stream_context();
            (source_adapter.stream.decode_source)(&event.data, &mut context)
        };

        let decoded = decoded_frame?;
        self.last_meaningful_output_observed = decoded.value.meaningful_output_observed();
        let decoded_summary = decoded.summary;
        let transformed = match decoded.value {
            DecodedSourceStreamFrame::Events(stream_events) => {
                self.update_session_from_stream_events(&stream_events);
                self.stream_events_to_target_events(stream_events)
            }
            DecodedSourceStreamFrame::LegacyChunk(mut unified_chunk) => {
                let consistent_id = self.get_or_generate_stream_id();
                unified_chunk.id = consistent_id;
                self.normalize_unified_chunk_session_state(&mut unified_chunk);
                let mut context = self.stream_context();
                (target_adapter.stream.encode_legacy_chunk)(unified_chunk, &mut context)
            }
        };

        match transformed {
            Ok(mut success) => {
                success.summary = merge_transform_summaries([decoded_summary, success.summary]);
                Ok(success)
            }
            Err(mut failure) => {
                failure.summary = merge_transform_summaries([decoded_summary, failure.summary]);
                Err(failure)
            }
        }
    }
}

pub(super) fn classify_stream_disposition(
    input_is_empty: bool,
    input_is_done: bool,
    events: &[SseEvent],
    summary: &TransformOutcomeSummary,
) -> StreamFrameDisposition {
    if summary
        .outcome_counts
        .contains_key(&TransformOutcomeKind::ObservationDegraded)
    {
        return StreamFrameDisposition::ObservationDegraded;
    }
    if summary.outcome_counts.keys().any(|outcome| {
        matches!(
            outcome,
            TransformOutcomeKind::ControlledLossMinor | TransformOutcomeKind::ControlledLossMajor
        )
    }) {
        return StreamFrameDisposition::ControlledLoss;
    }
    if input_is_empty {
        return StreamFrameDisposition::EmptyFrame;
    }
    if input_is_done {
        return if events.is_empty() {
            StreamFrameDisposition::LifecycleNoOutput
        } else {
            StreamFrameDisposition::LifecycleSent
        };
    }
    if events.is_empty() {
        return StreamFrameDisposition::SemanticNoOutput;
    }
    if summary
        .outcome_counts
        .contains_key(&TransformOutcomeKind::Passthrough)
    {
        StreamFrameDisposition::Passthrough
    } else {
        StreamFrameDisposition::Sent
    }
}
