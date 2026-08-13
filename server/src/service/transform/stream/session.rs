use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use serde_json::Value;

use super::usage::UsageMergeStrategy;
use crate::cost::UsageNormalization;
use crate::schema::enum_def::UpstreamProtocol;
use crate::service::transform::providers::{gemini, responses};
use crate::service::transform::unified::{
    UnifiedBlockKind, UnifiedRole, UnifiedStreamEvent, UnifiedUsage,
};
use crate::service::transform::{
    TransformAction, TransformDiagnosticCollector, TransformDiagnosticFact, TransformOutcomeKind,
    TransformOutcomeSummary, TransformPhase, TransformReasonCode, TransformSemanticUnit,
};
use crate::utils::sse::SseEvent;
use crate::utils::usage::UsageInfo;

const STREAM_DIAGNOSTIC_WINDOW: usize = 32;
pub(in crate::service::transform) const MAX_STREAM_TOOL_ARGUMENT_BYTES: usize = 1024 * 1024;

pub(in crate::service::transform) fn try_append_tool_arguments(
    buffer: &mut String,
    delta: &str,
) -> bool {
    if buffer.len().saturating_add(delta.len()) > MAX_STREAM_TOOL_ARGUMENT_BYTES {
        return false;
    }
    buffer.push_str(delta);
    true
}

#[derive(Debug, Default, Clone)]
pub struct AnthropicSessionState {
    pub(in crate::service::transform) message_started: bool,
    pub(in crate::service::transform) source_message_started: bool,
    pub(in crate::service::transform) source_message_stopped: bool,
    pub(in crate::service::transform) source_message_delta_seen: bool,
    pub(in crate::service::transform) source_error_seen: bool,
    pub(in crate::service::transform) source_next_block_index: u32,
    pub(in crate::service::transform) active_blocks: HashMap<u32, AnthropicActiveBlockState>,
    pub(in crate::service::transform) source_usage: Option<UnifiedUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnthropicActiveBlockKind {
    Text,
    ToolUse,
    Thinking,
}

#[derive(Debug, Clone)]
pub struct AnthropicActiveBlockState {
    pub(crate) kind: AnthropicActiveBlockKind,
    pub(in crate::service::transform) text: String,
    pub(in crate::service::transform) tool_call_id: Option<String>,
    pub(in crate::service::transform) tool_name: Option<String>,
    pub(in crate::service::transform) signature_seen: bool,
}

impl AnthropicActiveBlockState {
    pub(crate) fn new(kind: AnthropicActiveBlockKind) -> Self {
        Self {
            kind,
            text: String::new(),
            tool_call_id: None,
            tool_name: None,
            signature_seen: false,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct GeminiSessionState {
    pub(in crate::service::transform) tool_call_id_map: HashMap<String, String>,
    pub(in crate::service::transform) target_tool_calls: HashMap<u32, StreamToolCallState>,
    pub(in crate::service::transform) source_usage: Option<UnifiedUsage>,
    pub(in crate::service::transform) usage_observation_degraded: bool,
    pub(in crate::service::transform) source_response_id: Option<String>,
    pub(in crate::service::transform) source_candidate_indices: HashSet<u32>,
    pub(in crate::service::transform) source_tool_call_candidate_indices: HashSet<u32>,
    pub(in crate::service::transform) source_terminal_candidate_indices: HashSet<u32>,
    pub(in crate::service::transform) source_prompt_block_seen: bool,
    pub(in crate::service::transform) source_terminal_failed: bool,
    pub(in crate::service::transform) source_observation_degraded: bool,
    pub(in crate::service::transform) source_observation_semantic_unit:
        Option<TransformSemanticUnit>,
    pub(in crate::service::transform) source_observation_reason_code: Option<TransformReasonCode>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct StreamToolCallState {
    pub(in crate::service::transform) id: Option<String>,
    pub(in crate::service::transform) name: Option<String>,
    pub(in crate::service::transform) arguments: String,
}

#[derive(Debug, Default, Clone)]
pub struct ResponsesSessionState {
    pub(in crate::service::transform) created_sent: bool,
    pub(in crate::service::transform) completion_pending: bool,
    pub(in crate::service::transform) next_sequence_number: u64,
    pub(in crate::service::transform) next_output_index: u32,
    pub(in crate::service::transform) current_output_index: u32,
    pub(in crate::service::transform) current_item_id: Option<String>,
    pub(in crate::service::transform) current_item_role: Option<UnifiedRole>,
    pub(in crate::service::transform) output_item_ids: HashMap<u32, String>,
    pub(in crate::service::transform) output_text: String,
    pub(in crate::service::transform) reasoning_item_ids: HashMap<u32, String>,
    pub(in crate::service::transform) reasoning_summaries: HashMap<u32, String>,
    pub(in crate::service::transform) active_tool_calls: HashMap<u32, responses::FunctionCall>,
    pub(in crate::service::transform) completed_output: BTreeMap<u32, responses::ItemField>,
    pub(in crate::service::transform) source_created_seen: bool,
    pub(in crate::service::transform) source_response_id: Option<String>,
    pub(in crate::service::transform) source_response_model: Option<String>,
    pub(in crate::service::transform) source_queued_seen: bool,
    pub(in crate::service::transform) source_in_progress_seen: bool,
    pub(in crate::service::transform) source_terminal_seen: bool,
    pub(in crate::service::transform) source_last_sequence_number: Option<u64>,
    pub(in crate::service::transform) source_output_item_ids: HashMap<u32, String>,
    pub(in crate::service::transform) source_output_items_done: HashSet<u32>,
    pub(in crate::service::transform) source_content_parts: HashSet<(String, u32)>,
    pub(in crate::service::transform) source_reasoning_parts: HashSet<(String, u32)>,
    pub(in crate::service::transform) source_output_text: HashMap<String, String>,
    pub(in crate::service::transform) source_refusal_text: HashMap<String, String>,
    pub(in crate::service::transform) source_reasoning_text: HashMap<(String, u32), String>,
    pub(in crate::service::transform) source_tool_arguments: HashMap<String, String>,
}

#[derive(Debug, Default, Clone)]
pub struct SessionContext {
    stream_id: Option<Arc<str>>,
    stream_model: Option<Arc<str>>,
    openai_reasoning_seen: bool,
    openai_active_tool_calls: HashMap<u32, String>,
    openai_source_tool_calls: HashMap<u32, StreamToolCallState>,
    tool_call_id_map: HashMap<String, String>,
    current_item_index: Option<u32>,
    current_content_block_index: Option<u32>,
    current_content_part_index: Option<u32>,
    current_reasoning_block_index: Option<u32>,
    current_reasoning_part_index: Option<u32>,
    usage_cache: Option<UsageInfo>,
    usage_normalization_cache: Option<UsageNormalization>,
    unified_usage_cache: Option<UnifiedUsage>,
    finish_reason_cache: Option<String>,
    last_error: Option<Value>,
    diagnostics: TransformDiagnosticCollector,
    original_events: VecDeque<SseEvent>,
    transformed_events: VecDeque<SseEvent>,
    anthropic: AnthropicSessionState,
    gemini: GeminiSessionState,
    responses: ResponsesSessionState,
}

impl SessionContext {
    pub(in crate::service::transform) fn semantic_snapshot(&mut self) -> Self {
        let diagnostics = std::mem::take(&mut self.diagnostics);
        let original_events = std::mem::take(&mut self.original_events);
        let transformed_events = std::mem::take(&mut self.transformed_events);
        let snapshot = self.clone();
        self.diagnostics = diagnostics;
        self.original_events = original_events;
        self.transformed_events = transformed_events;
        snapshot
    }

    pub(in crate::service::transform) fn restore_semantic_snapshot(&mut self, mut snapshot: Self) {
        snapshot.diagnostics = std::mem::take(&mut self.diagnostics);
        snapshot.original_events = std::mem::take(&mut self.original_events);
        snapshot.transformed_events = std::mem::take(&mut self.transformed_events);
        *self = snapshot;
    }

    pub(in crate::service::transform) fn stream_id_clone(&self) -> Option<String> {
        self.stream_id.as_deref().map(str::to_owned)
    }

    pub(in crate::service::transform) fn stream_model_clone(&self) -> Option<String> {
        self.stream_model.as_deref().map(str::to_owned)
    }

    pub(in crate::service::transform) fn set_stream_id(&mut self, id: String) {
        self.stream_id = Some(id.into());
    }

    pub(in crate::service::transform) fn set_stream_model(&mut self, model: String) {
        self.stream_model = Some(model.into());
    }

    pub(in crate::service::transform) fn set_stream_model_if_present(
        &mut self,
        model: Option<String>,
    ) {
        if let Some(model) = model.filter(|value| !value.is_empty()) {
            self.stream_model = Some(model.into());
        }
    }

    pub(in crate::service::transform) fn get_or_generate_stream_id(
        &mut self,
        upstream_protocol: UpstreamProtocol,
    ) -> String {
        if let Some(id) = &self.stream_id {
            return id.to_string();
        }

        use crate::utils::ID_GENERATOR;
        let new_id = if upstream_protocol == UpstreamProtocol::Gemini {
            format!("gemini-stream-{}", ID_GENERATOR.generate_id())
        } else {
            format!("chatcmpl-{}", ID_GENERATOR.generate_id())
        };
        self.stream_id = Some(Arc::from(new_id.as_str()));
        new_id
    }

    pub(in crate::service::transform) fn get_or_default_stream_model(
        &self,
        upstream_protocol: UpstreamProtocol,
    ) -> String {
        self.stream_model
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if upstream_protocol == UpstreamProtocol::Gemini {
                    "gemini".to_string()
                } else {
                    "unified-stream-model".to_string()
                }
            })
    }

    pub(in crate::service::transform) fn usage_cache(&self) -> Option<&UsageInfo> {
        self.usage_cache.as_ref()
    }

    pub(in crate::service::transform) fn usage_cache_clone(&self) -> Option<UsageInfo> {
        self.usage_cache.clone()
    }

    pub(in crate::service::transform) fn usage_normalization_cache_clone(
        &self,
    ) -> Option<UsageNormalization> {
        self.usage_normalization_cache.clone()
    }

    pub(in crate::service::transform) fn unified_usage_cache_clone(&self) -> Option<UnifiedUsage> {
        self.unified_usage_cache.clone()
    }

    pub(in crate::service::transform) fn gemini_usage_observation_degraded(&self) -> bool {
        self.gemini.usage_observation_degraded
    }

    pub(in crate::service::transform) fn gemini_source_terminal_seen(&self) -> bool {
        !self.gemini.source_observation_degraded
            && (self.gemini.source_terminal_failed
                || (self.gemini.source_prompt_block_seen
                    && self.gemini.source_candidate_indices.is_empty())
                || (!self.gemini.source_candidate_indices.is_empty()
                    && self.gemini.source_candidate_indices
                        == self.gemini.source_terminal_candidate_indices))
    }

    pub(in crate::service::transform) fn gemini_source_failed(&self) -> bool {
        self.gemini_source_terminal_seen() && self.gemini.source_terminal_failed
    }

    pub(in crate::service::transform) fn mark_gemini_source_tool_call_candidate(
        &mut self,
        candidate_index: u32,
    ) {
        self.gemini
            .source_tool_call_candidate_indices
            .insert(candidate_index);
    }

    pub(in crate::service::transform) fn gemini_source_candidate_has_tool_call(
        &self,
        candidate_index: u32,
    ) -> bool {
        self.gemini
            .source_tool_call_candidate_indices
            .contains(&candidate_index)
    }

    pub(in crate::service::transform) fn gemini_source_observation_degraded(&self) -> bool {
        self.gemini.source_observation_degraded
    }

    pub(in crate::service::transform) fn gemini_source_observation_error(
        &self,
    ) -> Option<(TransformSemanticUnit, TransformReasonCode)> {
        self.gemini
            .source_observation_reason_code
            .map(|reason_code| {
                (
                    self.gemini
                        .source_observation_semantic_unit
                        .unwrap_or(TransformSemanticUnit::Lifecycle),
                    reason_code,
                )
            })
    }

    pub(in crate::service::transform) fn finish_reason_cache(&self) -> Option<&str> {
        self.finish_reason_cache.as_deref()
    }

    pub(in crate::service::transform) fn finish_reason_cache_clone(&self) -> Option<String> {
        self.finish_reason_cache.clone()
    }

    pub(in crate::service::transform) fn set_finish_reason_cache(
        &mut self,
        finish_reason: Option<String>,
    ) {
        self.finish_reason_cache = finish_reason;
    }

    pub(in crate::service::transform) fn set_last_error(&mut self, error: Value) {
        self.last_error = Some(error);
    }

    pub(in crate::service::transform) fn diagnostics_snapshot(&self) -> TransformOutcomeSummary {
        self.diagnostics.snapshot()
    }

    pub(in crate::service::transform) fn responses_source_terminal_seen(&self) -> bool {
        self.responses.source_terminal_seen
    }

    pub(in crate::service::transform) fn responses_source_failed(&self) -> bool {
        self.responses.source_terminal_seen && self.last_error.is_some()
    }

    pub(in crate::service::transform) fn anthropic_source_terminal_seen(&self) -> bool {
        self.anthropic.source_message_stopped || self.anthropic.source_error_seen
    }

    pub(in crate::service::transform) fn anthropic_source_failed(&self) -> bool {
        self.anthropic.source_error_seen
    }

    pub(in crate::service::transform) fn anthropic_target_message_started(&self) -> bool {
        self.anthropic.message_started
    }

    pub(in crate::service::transform) fn responses_source_identity_matches(
        &self,
        response_id: &str,
        response_model: &str,
    ) -> bool {
        self.responses.source_created_seen
            && self.responses.source_response_id.as_deref() == Some(response_id)
            && self.responses.source_response_model.as_deref() == Some(response_model)
    }

    pub(in crate::service::transform) fn mark_responses_source_terminal(
        &mut self,
        error: Option<Value>,
    ) {
        self.responses.source_terminal_seen = true;
        if let Some(error) = error {
            self.last_error = Some(error);
        }
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn diagnostics_len(&self) -> usize {
        self.diagnostics.snapshot().facts.len()
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn latest_diagnostic(
        &self,
    ) -> Option<TransformDiagnosticFact> {
        self.diagnostics.snapshot().facts.last().cloned()
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn last_error_is_some(&self) -> bool {
        self.last_error.is_some()
    }

    pub(in crate::service::transform) fn original_events(&self) -> &VecDeque<SseEvent> {
        &self.original_events
    }

    pub(in crate::service::transform) fn original_events_is_empty(&self) -> bool {
        self.original_events.is_empty()
    }

    pub(in crate::service::transform) fn original_events_len(&self) -> usize {
        self.original_events.len()
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn original_events_front(&self) -> Option<&SseEvent> {
        self.original_events.front()
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn transformed_events_len(&self) -> usize {
        self.transformed_events.len()
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn current_item_index(&self) -> Option<u32> {
        self.current_item_index
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn current_content_part_index(&self) -> Option<u32> {
        self.current_content_part_index
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn current_reasoning_part_index(&self) -> Option<u32> {
        self.current_reasoning_part_index
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn set_current_content_block_index(
        &mut self,
        index: Option<u32>,
    ) {
        self.current_content_block_index = index;
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn tool_call_id(&self, id: &str) -> Option<&String> {
        self.tool_call_id_map.get(id)
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn anthropic_message_started(&self) -> bool {
        self.anthropic.message_started
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn anthropic_active_blocks_is_empty(&self) -> bool {
        self.anthropic.active_blocks.is_empty()
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn anthropic_active_blocks_contains(
        &self,
        index: &u32,
    ) -> bool {
        self.anthropic.active_blocks.contains_key(index)
    }

    pub(in crate::service::transform) fn push_original_event(&mut self, event: SseEvent) {
        Self::push_bounded(&mut self.original_events, event);
    }

    pub(in crate::service::transform) fn push_transformed_event(&mut self, event: SseEvent) {
        Self::push_bounded(&mut self.transformed_events, event);
    }

    pub(in crate::service::transform) fn record_diagnostic(
        &mut self,
        diagnostic: TransformDiagnosticFact,
    ) {
        self.diagnostics.record(diagnostic);
    }

    fn push_bounded(queue: &mut VecDeque<SseEvent>, event: SseEvent) {
        if queue.len() >= STREAM_DIAGNOSTIC_WINDOW {
            queue.pop_front();
        }
        queue.push_back(event);
    }

    pub(in crate::service::transform) fn merge_usage(
        &mut self,
        mut usage: UnifiedUsage,
        strategy: UsageMergeStrategy,
    ) {
        if strategy == UsageMergeStrategy::AnthropicFields
            && let Some(previous) = self.unified_usage_cache.as_ref()
        {
            if usage.input_tokens == 0 {
                usage.input_tokens = previous.input_tokens;
            }
            if usage.cached_tokens.is_none() {
                usage.cached_tokens = previous.cached_tokens;
            }
            if usage.cache_write_tokens.is_none() {
                usage.cache_write_tokens = previous.cache_write_tokens;
            }
            usage.total_tokens = usage
                .input_tokens
                .checked_add(usage.output_tokens)
                .expect("source-validated Anthropic stream usage must remain in range");
        }
        match UsageInfo::try_from(&usage) {
            Ok(usage_info) => {
                self.usage_normalization_cache = Some(UsageNormalization::from(&usage));
                self.usage_cache = Some(usage_info);
                self.unified_usage_cache = Some(usage);
            }
            Err(_) => {
                self.usage_normalization_cache = None;
                self.usage_cache = None;
                self.unified_usage_cache = None;
                self.record_diagnostic(TransformDiagnosticFact {
                    sequence: 0,
                    phase: TransformPhase::ResponseObserve,
                    semantic_unit: TransformSemanticUnit::Usage,
                    outcome: TransformOutcomeKind::ObservationDegraded,
                    action: TransformAction::PassThrough,
                    reason_code: TransformReasonCode::UsageOverflow,
                    safe_summary: None,
                });
            }
        }
    }

    pub(in crate::service::transform) fn remember_tool_call_id(&mut self, id: String) {
        self.tool_call_id_map.insert(id.clone(), id);
    }

    pub(in crate::service::transform) fn track_openai_tool_call(&mut self, index: u32, id: String) {
        self.openai_active_tool_calls.insert(index, id.clone());
        self.tool_call_id_map.insert(id.clone(), id);
    }

    pub(in crate::service::transform) fn forget_openai_tool_call(&mut self, index: &u32) {
        self.openai_active_tool_calls.remove(index);
    }

    pub(in crate::service::transform) fn openai_reasoning_seen(&self) -> bool {
        self.openai_reasoning_seen
    }

    pub(in crate::service::transform) fn openai_active_tool_calls_clone(
        &self,
    ) -> HashMap<u32, String> {
        self.openai_active_tool_calls.clone()
    }

    pub(in crate::service::transform) fn openai_source_tool_calls(
        &self,
    ) -> &HashMap<u32, StreamToolCallState> {
        &self.openai_source_tool_calls
    }

    pub(in crate::service::transform) fn openai_source_tool_calls_mut(
        &mut self,
    ) -> &mut HashMap<u32, StreamToolCallState> {
        &mut self.openai_source_tool_calls
    }

    pub(in crate::service::transform) fn get_or_create_gemini_tool_call_id(
        &mut self,
        provider_order: u32,
        part_index: u32,
        function_name: &str,
    ) -> String {
        let response_id = self.stream_id_clone().unwrap_or_default();
        let key = gemini::build_gemini_tool_call_key(
            &response_id,
            provider_order,
            part_index,
            function_name,
        );
        self.gemini
            .tool_call_id_map
            .entry(key)
            .or_insert_with(|| {
                gemini::build_gemini_synthetic_tool_call_id(
                    &response_id,
                    provider_order,
                    part_index,
                    function_name,
                )
            })
            .clone()
    }

    pub(in crate::service::transform) fn update_from_stream_event(
        &mut self,
        event: &UnifiedStreamEvent,
        usage_strategy: UsageMergeStrategy,
    ) {
        match event {
            UnifiedStreamEvent::ItemAdded {
                item_index,
                item_id,
                ..
            }
            | UnifiedStreamEvent::ItemDone {
                item_index,
                item_id,
                ..
            } => {
                self.current_item_index = *item_index;
                if let Some(item_id) = item_id {
                    self.tool_call_id_map
                        .entry(item_id.clone())
                        .or_insert_with(|| item_id.clone());
                }
            }
            UnifiedStreamEvent::MessageStart { id, model, .. } => {
                if let Some(id) = id {
                    self.stream_id = Some(Arc::from(id.as_str()));
                }
                if let Some(model) = model {
                    self.stream_model = Some(Arc::from(model.as_str()));
                }
            }
            UnifiedStreamEvent::ContentBlockStart { index, kind } => match kind {
                UnifiedBlockKind::Text | UnifiedBlockKind::ToolCall => {
                    self.current_content_block_index = Some(*index);
                }
                UnifiedBlockKind::Reasoning => {
                    self.current_reasoning_block_index = Some(*index);
                }
                UnifiedBlockKind::Blob => {}
            },
            UnifiedStreamEvent::ContentBlockStop { index } => {
                if self.current_content_block_index == Some(*index) {
                    self.current_content_block_index = None;
                }
            }
            UnifiedStreamEvent::ContentPartAdded {
                item_index,
                part_index,
                ..
            }
            | UnifiedStreamEvent::ContentPartDone {
                item_index,
                part_index,
                ..
            } => {
                self.current_item_index = *item_index;
                self.current_content_part_index = Some(*part_index);
            }
            UnifiedStreamEvent::ReasoningStart { index } => {
                self.openai_reasoning_seen = true;
                self.current_reasoning_block_index = Some(*index);
            }
            UnifiedStreamEvent::ReasoningDelta { .. } => {
                self.openai_reasoning_seen = true;
            }
            UnifiedStreamEvent::ReasoningStop { index } => {
                if self.current_reasoning_block_index == Some(*index) {
                    self.current_reasoning_block_index = None;
                }
            }
            UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index,
                part_index,
                ..
            }
            | UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index,
                part_index,
                ..
            } => {
                self.current_item_index = *item_index;
                self.current_reasoning_part_index = Some(*part_index);
            }
            UnifiedStreamEvent::Usage { usage } => {
                self.merge_usage(usage.clone(), usage_strategy);
            }
            UnifiedStreamEvent::MessageDelta { finish_reason } => {
                if let Some(finish_reason) = finish_reason {
                    self.finish_reason_cache = Some(finish_reason.clone());
                }
            }
            UnifiedStreamEvent::ToolCallStart { index, id, .. } => {
                self.track_openai_tool_call(*index, id.clone());
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                id: Some(id),
                ..
            } => {
                self.track_openai_tool_call(*index, id.clone());
            }
            UnifiedStreamEvent::ToolCallStop { index, id } => {
                self.forget_openai_tool_call(index);
                if let Some(id) = id {
                    self.remember_tool_call_id(id.clone());
                }
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta { id: None, .. } => {}
            UnifiedStreamEvent::Error { error } => {
                self.last_error = Some(error.clone());
            }
            UnifiedStreamEvent::MessageStop
            | UnifiedStreamEvent::ContentBlockDelta { .. }
            | UnifiedStreamEvent::RefusalDelta { .. }
            | UnifiedStreamEvent::BlobDelta { .. } => {}
        }
    }
}

pub(crate) struct StreamTransformContext<'a> {
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: crate::schema::enum_def::DownstreamProtocol,
    session: &'a mut SessionContext,
}

impl<'a> StreamTransformContext<'a> {
    pub(in crate::service::transform) fn new(
        upstream_protocol: UpstreamProtocol,
        downstream_protocol: crate::schema::enum_def::DownstreamProtocol,
        session: &'a mut SessionContext,
    ) -> Self {
        Self {
            upstream_protocol,
            downstream_protocol,
            session,
        }
    }

    pub(in crate::service::transform) fn is_same_wire(&self) -> bool {
        matches!(
            (self.upstream_protocol, self.downstream_protocol),
            (
                UpstreamProtocol::Openai,
                crate::schema::enum_def::DownstreamProtocol::Openai
            ) | (
                UpstreamProtocol::Responses,
                crate::schema::enum_def::DownstreamProtocol::Responses
            ) | (
                UpstreamProtocol::Anthropic,
                crate::schema::enum_def::DownstreamProtocol::Anthropic
            ) | (
                UpstreamProtocol::Gemini,
                crate::schema::enum_def::DownstreamProtocol::Gemini
            )
        )
    }

    pub(in crate::service::transform) fn get_or_generate_stream_id(&mut self) -> String {
        self.session
            .get_or_generate_stream_id(self.upstream_protocol)
    }

    pub(in crate::service::transform) fn get_or_default_stream_model(&self) -> String {
        self.session
            .get_or_default_stream_model(self.upstream_protocol)
    }

    pub(in crate::service::transform) fn native_gemini_response_id_clone(&self) -> Option<String> {
        (self.upstream_protocol == UpstreamProtocol::Gemini)
            .then(|| self.session.stream_id_clone())
            .flatten()
    }

    pub(in crate::service::transform) fn stream_model_clone(&self) -> Option<String> {
        self.session.stream_model_clone()
    }

    pub(in crate::service::transform) fn set_stream_id(&mut self, id: String) {
        self.session.set_stream_id(id);
    }

    pub(in crate::service::transform) fn set_stream_model(&mut self, model: String) {
        self.session.set_stream_model(model);
    }

    pub(in crate::service::transform) fn usage_cache(&self) -> Option<&UsageInfo> {
        self.session.usage_cache()
    }

    pub(in crate::service::transform) fn usage_cache_clone(&self) -> Option<UsageInfo> {
        self.session.usage_cache_clone()
    }

    pub(in crate::service::transform) fn set_usage(&mut self, usage: UnifiedUsage) {
        self.session.merge_usage(usage, self.usage_merge_strategy());
    }

    pub(in crate::service::transform) fn gemini_source_usage(&self) -> Option<&UnifiedUsage> {
        self.session.gemini.source_usage.as_ref()
    }

    pub(in crate::service::transform) fn set_gemini_source_usage(&mut self, usage: UnifiedUsage) {
        self.session.gemini.source_usage = Some(usage);
    }

    pub(in crate::service::transform) fn invalidate_gemini_usage_observation(&mut self) {
        self.session.gemini.usage_observation_degraded = true;
        self.session.gemini.source_usage = None;
        self.session.unified_usage_cache = None;
        self.session.usage_cache = None;
        self.session.usage_normalization_cache = None;
    }

    pub(in crate::service::transform) fn invalidate_gemini_stream_observation(
        &mut self,
        semantic_unit: TransformSemanticUnit,
        reason_code: TransformReasonCode,
    ) {
        self.session.gemini.source_observation_degraded = true;
        if self.session.gemini.source_observation_reason_code.is_none() {
            self.session.gemini.source_observation_semantic_unit = Some(semantic_unit);
            self.session.gemini.source_observation_reason_code = Some(reason_code);
        }
        self.invalidate_gemini_usage_observation();
    }

    pub(in crate::service::transform) fn finish_reason_cache_clone(&self) -> Option<String> {
        self.session.finish_reason_cache_clone()
    }

    pub(in crate::service::transform) fn finish_reason_cache(&self) -> Option<&str> {
        self.session.finish_reason_cache()
    }

    pub(in crate::service::transform) fn set_finish_reason_cache(
        &mut self,
        finish_reason: Option<String>,
    ) {
        self.session.set_finish_reason_cache(finish_reason);
    }

    pub(in crate::service::transform) fn record_diagnostic(
        &mut self,
        diagnostic: TransformDiagnosticFact,
    ) {
        self.session.record_diagnostic(diagnostic);
    }

    pub(in crate::service::transform) fn usage_merge_strategy(&self) -> UsageMergeStrategy {
        match self.upstream_protocol {
            UpstreamProtocol::Gemini | UpstreamProtocol::Responses => UsageMergeStrategy::Replace,
            UpstreamProtocol::Anthropic => UsageMergeStrategy::AnthropicFields,
            UpstreamProtocol::Openai | UpstreamProtocol::Ollama => UsageMergeStrategy::FinalOnly,
        }
    }

    pub(in crate::service::transform) fn current_content_block_index(&self) -> Option<u32> {
        self.session.current_content_block_index
    }

    pub(in crate::service::transform) fn current_content_part_index(&self) -> Option<u32> {
        self.session.current_content_part_index
    }

    pub(in crate::service::transform) fn current_reasoning_block_index(&self) -> Option<u32> {
        self.session.current_reasoning_block_index
    }

    pub(in crate::service::transform) fn current_reasoning_part_index(&self) -> Option<u32> {
        self.session.current_reasoning_part_index
    }

    pub(in crate::service::transform) fn openai_reasoning_seen(&self) -> bool {
        self.session.openai_reasoning_seen()
    }

    pub(in crate::service::transform) fn openai_active_tool_calls_clone(
        &self,
    ) -> HashMap<u32, String> {
        self.session.openai_active_tool_calls_clone()
    }

    pub(in crate::service::transform) fn anthropic_message_started(&self) -> bool {
        self.session.anthropic.message_started
    }

    pub(in crate::service::transform) fn mark_anthropic_message_started(&mut self) {
        self.session.anthropic.message_started = true;
    }

    pub(in crate::service::transform) fn anthropic_active_blocks_mut(
        &mut self,
    ) -> &mut HashMap<u32, AnthropicActiveBlockState> {
        &mut self.session.anthropic.active_blocks
    }

    pub(in crate::service::transform) fn anthropic_active_blocks(
        &self,
    ) -> &HashMap<u32, AnthropicActiveBlockState> {
        &self.session.anthropic.active_blocks
    }

    pub(in crate::service::transform) fn anthropic_session_mut(
        &mut self,
    ) -> &mut AnthropicSessionState {
        &mut self.session.anthropic
    }

    pub(in crate::service::transform) fn gemini_target_tool_calls_mut(
        &mut self,
    ) -> &mut HashMap<u32, StreamToolCallState> {
        &mut self.session.gemini.target_tool_calls
    }

    pub(in crate::service::transform) fn gemini_target_tool_calls(
        &self,
    ) -> &HashMap<u32, StreamToolCallState> {
        &self.session.gemini.target_tool_calls
    }

    pub(in crate::service::transform) fn gemini(&self) -> &GeminiSessionState {
        &self.session.gemini
    }

    pub(in crate::service::transform) fn gemini_mut(&mut self) -> &mut GeminiSessionState {
        &mut self.session.gemini
    }

    pub(in crate::service::transform) fn openai_source_tool_calls(
        &self,
    ) -> &HashMap<u32, StreamToolCallState> {
        self.session.openai_source_tool_calls()
    }

    pub(in crate::service::transform) fn openai_source_tool_calls_mut(
        &mut self,
    ) -> &mut HashMap<u32, StreamToolCallState> {
        self.session.openai_source_tool_calls_mut()
    }

    pub(in crate::service::transform) fn responses(&self) -> &ResponsesSessionState {
        &self.session.responses
    }

    pub(in crate::service::transform) fn responses_mut(&mut self) -> &mut ResponsesSessionState {
        &mut self.session.responses
    }

    pub(in crate::service::transform) fn update_session_from_stream_event(
        &mut self,
        event: &UnifiedStreamEvent,
    ) {
        let strategy = self.usage_merge_strategy();
        self.session.update_from_stream_event(event, strategy);
    }
}
