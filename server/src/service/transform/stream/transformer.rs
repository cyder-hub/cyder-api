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
use crate::service::transform::response::observe_responses_usage;
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

fn skip_json_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
    {
        index += 1;
    }
    index
}

fn parse_json_string_bounds(bytes: &[u8], start: usize) -> Option<(usize, usize, usize, bool)> {
    if bytes.get(start) != Some(&b'"') {
        return None;
    }
    let mut index = start + 1;
    let mut escaped = false;
    while let Some(byte) = bytes.get(index) {
        match byte {
            b'\\' => {
                escaped = true;
                index = index.checked_add(2)?;
            }
            b'"' => return Some((start + 1, index, index + 1, escaped)),
            _ => index += 1,
        }
    }
    None
}

fn skip_json_value(bytes: &[u8], start: usize) -> Option<usize> {
    let start = skip_json_whitespace(bytes, start);
    match *bytes.get(start)? {
        b'"' => parse_json_string_bounds(bytes, start).map(|(_, _, end, _)| end),
        b'{' | b'[' => {
            let mut depth = 0_u32;
            let mut index = start;
            while let Some(byte) = bytes.get(index) {
                match byte {
                    b'"' => {
                        index = parse_json_string_bounds(bytes, index)?.2;
                        continue;
                    }
                    b'{' | b'[' => depth = depth.checked_add(1)?,
                    b'}' | b']' => {
                        depth = depth.checked_sub(1)?;
                        if depth == 0 {
                            return Some(index + 1);
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
            None
        }
        _ => {
            let mut index = start;
            while bytes
                .get(index)
                .is_some_and(|byte| !matches!(byte, b',' | b'}' | b' ' | b'\n' | b'\r' | b'\t'))
            {
                index += 1;
            }
            (index > start).then_some(index)
        }
    }
}

fn anthropic_sse_payload_type(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    let mut index = skip_json_whitespace(bytes, 0);
    if bytes.get(index) != Some(&b'{') {
        return None;
    }
    index += 1;
    loop {
        index = skip_json_whitespace(bytes, index);
        if bytes.get(index) == Some(&b'}') {
            return None;
        }
        let (key_start, key_end, key_next, key_escaped) = parse_json_string_bounds(bytes, index)?;
        index = skip_json_whitespace(bytes, key_next);
        if bytes.get(index) != Some(&b':') {
            return None;
        }
        index = skip_json_whitespace(bytes, index + 1);
        if !key_escaped && &bytes[key_start..key_end] == b"type" {
            let (value_start, value_end, _, value_escaped) =
                parse_json_string_bounds(bytes, index)?;
            if value_escaped {
                return None;
            }
            return std::str::from_utf8(&bytes[value_start..value_end]).ok();
        }
        index = skip_json_whitespace(bytes, skip_json_value(bytes, index)?);
        match bytes.get(index) {
            Some(b',') => index += 1,
            Some(b'}') | None => return None,
            _ => return None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceStreamTermination {
    Succeeded,
    Failed,
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
        StreamTransformContext::new(
            self.upstream_protocol,
            self.downstream_protocol,
            &mut self.session,
        )
    }

    fn record_transformed_events(&mut self, events: &[SseEvent]) {
        for event in events {
            self.session.push_transformed_event(event.clone());
        }
    }

    pub(in crate::service::transform) fn usage_merge_strategy(&self) -> UsageMergeStrategy {
        match self.upstream_protocol {
            UpstreamProtocol::Gemini | UpstreamProtocol::Responses => UsageMergeStrategy::Replace,
            UpstreamProtocol::Anthropic => UsageMergeStrategy::AnthropicFields,
            UpstreamProtocol::Openai => UsageMergeStrategy::FinalOnly,
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
                action: TransformAction::PassThrough,
                reason_code: TransformReasonCode::UpstreamUsageMissing,
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
            UpstreamProtocol::Gemini | UpstreamProtocol::Responses => {
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
                action: TransformAction::PassThrough,
                reason_code: TransformReasonCode::UpstreamUsageMissing,
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

    pub fn usage_is_billable(&self) -> bool {
        self.upstream_protocol != UpstreamProtocol::Gemini
            || (!self.session.gemini_usage_observation_degraded()
                && !self.session.gemini_source_observation_degraded()
                && !self.session.gemini_source_failed())
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

    pub fn validate_source_termination(&self) -> TransformResult<()> {
        if let Some(failure) = &self.terminal_failure {
            return Err(failure.clone());
        }
        if self.upstream_protocol == UpstreamProtocol::Gemini
            && self.session.gemini_source_observation_degraded()
        {
            let (semantic_unit, reason_code) =
                self.session.gemini_source_observation_error().unwrap_or((
                    TransformSemanticUnit::Lifecycle,
                    TransformReasonCode::IllegalUpstreamTerminal,
                ));
            return Err(transform_failure(
                TransformFailureOrigin::UpstreamPayload,
                TransformPhase::StreamDecode,
                semantic_unit,
                reason_code,
                None,
            ));
        }
        let terminal_is_valid = match self.upstream_protocol {
            UpstreamProtocol::Responses => self.session.responses_source_terminal_seen(),
            UpstreamProtocol::Anthropic => self.session.anthropic_source_terminal_seen(),
            UpstreamProtocol::Gemini => self.session.gemini_source_terminal_seen(),
            UpstreamProtocol::Openai => true,
        };
        if terminal_is_valid {
            return Ok(transform_success(
                (),
                TransformPhase::StreamDecode,
                TransformSemanticUnit::Lifecycle,
                TransformOutcomeKind::Lossless,
                TransformAction::PassThrough,
                TransformReasonCode::LosslessConversion,
            ));
        }

        Err(transform_failure(
            TransformFailureOrigin::UpstreamPayload,
            TransformPhase::StreamDecode,
            TransformSemanticUnit::Lifecycle,
            TransformReasonCode::IllegalUpstreamTerminal,
            None,
        ))
    }

    pub(crate) fn source_termination(&self) -> Option<SourceStreamTermination> {
        match self.upstream_protocol {
            UpstreamProtocol::Responses if self.session.responses_source_terminal_seen() => {
                Some(if self.session.responses_source_failed() {
                    SourceStreamTermination::Failed
                } else {
                    SourceStreamTermination::Succeeded
                })
            }
            UpstreamProtocol::Anthropic if self.session.anthropic_source_terminal_seen() => {
                Some(if self.session.anthropic_source_failed() {
                    SourceStreamTermination::Failed
                } else {
                    SourceStreamTermination::Succeeded
                })
            }
            UpstreamProtocol::Gemini if self.session.gemini_source_failed() => {
                Some(SourceStreamTermination::Failed)
            }
            _ => None,
        }
    }

    pub(crate) fn finalize_source_eof_events(&mut self) -> TransformResult<Vec<SseEvent>> {
        if self.upstream_protocol == UpstreamProtocol::Gemini
            && self.downstream_protocol != DownstreamProtocol::Gemini
        {
            let finish_reason = self.session.finish_reason_cache_clone().ok_or_else(|| {
                transform_failure(
                    TransformFailureOrigin::UpstreamPayload,
                    TransformPhase::StreamDecode,
                    TransformSemanticUnit::Lifecycle,
                    TransformReasonCode::IllegalUpstreamTerminal,
                    None,
                )
            })?;
            let mut terminal_events = Vec::new();
            if self.downstream_protocol == DownstreamProtocol::Anthropic
                && !self.session.anthropic_target_message_started()
            {
                terminal_events.push(UnifiedStreamEvent::MessageStart {
                    id: Some(self.get_or_generate_stream_id()),
                    model: self.session.stream_model_clone(),
                    role: UnifiedRole::Assistant,
                });
            }
            terminal_events.push(UnifiedStreamEvent::MessageDelta {
                finish_reason: Some(finish_reason),
            });
            if let Some(usage) = self.session.unified_usage_cache_clone() {
                terminal_events.push(UnifiedStreamEvent::Usage { usage });
            }
            terminal_events.push(UnifiedStreamEvent::MessageStop);
            let success = self.stream_events_to_target_events(terminal_events)?;
            self.record_transformed_events(&success.value);
            self.stream_summary.absorb(success.summary.clone());
            return Ok(success);
        }

        Ok(transform_success(
            Vec::new(),
            TransformPhase::StreamEncode,
            TransformSemanticUnit::Lifecycle,
            TransformOutcomeKind::Lossless,
            TransformAction::Drop,
            TransformReasonCode::NoSemanticOutput,
        ))
    }

    fn record_post_transform_diagnostic(&mut self, fact: TransformDiagnosticFact) {
        self.session.record_diagnostic(fact.clone());
        self.stream_summary.record(fact);
    }

    fn observe_degraded_responses_core(&mut self, raw: &str) -> Result<(), ()> {
        let value = serde_json::from_str::<Value>(raw).map_err(|_| ())?;
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            return Ok(());
        };

        if event_type == "response.usage" {
            if let Some(usage) = observe_responses_usage(&value) {
                self.session.merge_usage(usage, UsageMergeStrategy::Replace);
            }
            return Ok(());
        }

        if event_type == "error" {
            let code_is_valid = value
                .get("code")
                .and_then(Value::as_str)
                .is_some_and(|code| !code.is_empty());
            let message_is_valid = value
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(|message| !message.is_empty());
            if !code_is_valid || !message_is_valid {
                return Err(());
            }
            self.session
                .mark_responses_source_terminal(Some(value.clone()));
            return Ok(());
        }

        if !matches!(
            event_type,
            "response.queued"
                | "response.in_progress"
                | "response.completed"
                | "response.incomplete"
                | "response.failed"
        ) {
            return Ok(());
        }

        let response = value.get("response").ok_or(())?;
        let response_id = response
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or(())?;
        let response_model = response
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .ok_or(())?;
        if !self
            .session
            .responses_source_identity_matches(response_id, response_model)
        {
            return Err(());
        }

        let status = response.get("status").and_then(Value::as_str).ok_or(())?;
        let error = response.get("error").filter(|error| !error.is_null());
        match event_type {
            "response.queued" if status == "queued" && error.is_none() => return Ok(()),
            "response.in_progress" if status == "in_progress" && error.is_none() => {
                return Ok(());
            }
            "response.completed" if status == "completed" && error.is_none() => {}
            "response.incomplete" if status == "incomplete" && error.is_none() => {
                if !response
                    .get("incomplete_details")
                    .and_then(|details| details.get("reason"))
                    .and_then(Value::as_str)
                    .is_some_and(|reason| !reason.is_empty())
                {
                    return Err(());
                }
            }
            "response.failed" if status == "failed" && error.is_some() => {}
            _ => return Err(()),
        }

        if let Some(usage) = observe_responses_usage(&value) {
            self.session.merge_usage(usage, UsageMergeStrategy::Replace);
        }
        self.session.mark_responses_source_terminal(error.cloned());
        Ok(())
    }

    pub(crate) fn get_or_generate_stream_id(&mut self) -> String {
        self.session
            .get_or_generate_stream_id(self.upstream_protocol)
    }

    pub(in crate::service::transform) fn normalize_unified_chunk_session_state(
        &mut self,
        unified_chunk: &mut UnifiedChunkResponse,
    ) {
        let chunk_core = unified_chunk.core();
        self.session.set_stream_model_if_present(chunk_core.model);
        if self.upstream_protocol == UpstreamProtocol::Gemini {
            for choice in &mut unified_chunk.choices {
                let has_tool_call = choice
                    .delta
                    .content
                    .iter()
                    .any(|part| matches!(part, UnifiedContentPartDelta::ToolCallDelta(_)));
                if has_tool_call {
                    self.session
                        .mark_gemini_source_tool_call_candidate(choice.index);
                }
                if choice.finish_reason.as_deref() == Some("stop")
                    && self
                        .session
                        .gemini_source_candidate_has_tool_call(choice.index)
                {
                    choice.finish_reason = Some("tool_calls".to_string());
                }
                for part in &mut choice.delta.content {
                    if let UnifiedContentPartDelta::ToolCallDelta(tool_call) = part {
                        if let Some(id) = tool_call.id.clone() {
                            self.session.remember_tool_call_id(id);
                        } else {
                            let stable_id = self.session.get_or_create_gemini_tool_call_id(
                                choice.index,
                                tool_call.index,
                                tool_call.name.as_deref().unwrap_or(""),
                            );
                            tool_call.id = Some(stable_id.clone());
                            self.session.remember_tool_call_id(stable_id);
                        }
                    }
                }
            }
        }

        if let Some(usage) = chunk_core.usage {
            if self.upstream_protocol != UpstreamProtocol::Gemini
                || !self.session.gemini_usage_observation_degraded()
            {
                self.session.merge_usage(usage, self.usage_merge_strategy());
            }
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

        if self.upstream_protocol == UpstreamProtocol::Anthropic
            && let Some(event_name) = event.event.as_deref()
            && let Some(payload_type) = anthropic_sse_payload_type(&event.data)
            && event_name != payload_type
        {
            return Err(transform_failure(
                TransformFailureOrigin::UpstreamPayload,
                TransformPhase::StreamDecode,
                TransformSemanticUnit::Lifecycle,
                TransformReasonCode::StreamEventTypeMismatch,
                Some(crate::service::transform::TransformSafeSummary::from_bytes(
                    event.data.as_bytes(),
                )),
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
                    let preserve_gemini_core = self.upstream_protocol == UpstreamProtocol::Gemini
                        && failure.semantic_unit == TransformSemanticUnit::Usage;
                    if !preserve_gemini_core {
                        self.session
                            .restore_semantic_snapshot(observation_session_before);
                    }
                    if self.upstream_protocol == UpstreamProtocol::Gemini {
                        let mut context = self.stream_context();
                        if preserve_gemini_core {
                            context.invalidate_gemini_usage_observation();
                        } else {
                            context.invalidate_gemini_stream_observation(
                                failure.semantic_unit,
                                failure.reason_code,
                            );
                        }
                    }
                    if self.upstream_protocol == UpstreamProtocol::Anthropic
                        && failure.reason_code == TransformReasonCode::InvalidProtocolShape
                    {
                        return Err(failure);
                    }
                    if self.upstream_protocol == UpstreamProtocol::Responses
                        && self.observe_degraded_responses_core(&event.data).is_err()
                    {
                        return Err(failure);
                    }
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
                if self.upstream_protocol == UpstreamProtocol::Gemini
                    && unified_chunk
                        .synthetic_metadata
                        .as_ref()
                        .is_none_or(|metadata| !metadata.id)
                    && self.session.stream_id_clone().is_none()
                {
                    self.session.set_stream_id(unified_chunk.id.clone());
                }
                let consistent_id = self.get_or_generate_stream_id();
                unified_chunk.id = consistent_id;
                self.normalize_unified_chunk_session_state(&mut unified_chunk);
                if self.upstream_protocol == UpstreamProtocol::Gemini
                    && self.downstream_protocol != DownstreamProtocol::Gemini
                {
                    for choice in &mut unified_chunk.choices {
                        choice.finish_reason = None;
                    }
                    unified_chunk.usage = None;
                }
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
