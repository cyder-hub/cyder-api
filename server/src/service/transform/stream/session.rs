use std::collections::{BTreeMap, HashMap, VecDeque};

use serde_json::Value;

use super::usage::UsageMergeStrategy;
use crate::cost::UsageNormalization;
use crate::schema::enum_def::LlmApiType;
use crate::service::transform::providers::{gemini, responses};
use crate::service::transform::unified::{
    UnifiedBlockKind, UnifiedRole, UnifiedStreamEvent, UnifiedTransformDiagnostic, UnifiedUsage,
};
use crate::utils::sse::SseEvent;
use crate::utils::usage::UsageInfo;

const STREAM_DIAGNOSTIC_WINDOW: usize = 32;

#[derive(Debug, Default, Clone)]
pub struct AnthropicSessionState {
    pub(in crate::service::transform) message_started: bool,
    pub(in crate::service::transform) active_blocks: HashMap<u32, AnthropicActiveBlockState>,
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
}

impl AnthropicActiveBlockState {
    pub(crate) fn new(kind: AnthropicActiveBlockKind) -> Self {
        Self {
            kind,
            text: String::new(),
            tool_call_id: None,
            tool_name: None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct GeminiSessionState {
    pub(in crate::service::transform) tool_call_id_map: HashMap<String, String>,
    pub(in crate::service::transform) next_message_index_by_choice: HashMap<u32, u32>,
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
}

#[derive(Debug, Default, Clone)]
pub struct SessionContext {
    stream_id: Option<String>,
    stream_model: Option<String>,
    openai_reasoning_seen: bool,
    openai_active_tool_calls: HashMap<u32, String>,
    tool_call_id_map: HashMap<String, String>,
    current_item_index: Option<u32>,
    current_content_block_index: Option<u32>,
    current_content_part_index: Option<u32>,
    current_reasoning_block_index: Option<u32>,
    current_reasoning_part_index: Option<u32>,
    usage_cache: Option<UsageInfo>,
    usage_normalization_cache: Option<UsageNormalization>,
    finish_reason_cache: Option<String>,
    last_error: Option<Value>,
    diagnostics: VecDeque<UnifiedTransformDiagnostic>,
    original_events: VecDeque<SseEvent>,
    transformed_events: VecDeque<SseEvent>,
    anthropic: AnthropicSessionState,
    gemini: GeminiSessionState,
    responses: ResponsesSessionState,
}

impl SessionContext {
    pub(in crate::service::transform) fn stream_id_clone(&self) -> Option<String> {
        self.stream_id.clone()
    }

    pub(in crate::service::transform) fn stream_model_clone(&self) -> Option<String> {
        self.stream_model.clone()
    }

    pub(in crate::service::transform) fn set_stream_id(&mut self, id: String) {
        self.stream_id = Some(id);
    }

    pub(in crate::service::transform) fn set_stream_model(&mut self, model: String) {
        self.stream_model = Some(model);
    }

    pub(in crate::service::transform) fn set_stream_model_if_present(
        &mut self,
        model: Option<String>,
    ) {
        if let Some(model) = model.filter(|value| !value.is_empty()) {
            self.stream_model = Some(model);
        }
    }

    pub(in crate::service::transform) fn get_or_generate_stream_id(
        &mut self,
        source_api: LlmApiType,
    ) -> String {
        if let Some(id) = &self.stream_id {
            return id.clone();
        }

        use crate::utils::ID_GENERATOR;
        let new_id = if source_api == LlmApiType::Gemini {
            format!("gemini-stream-{}", ID_GENERATOR.generate_id())
        } else {
            format!("chatcmpl-{}", ID_GENERATOR.generate_id())
        };
        self.stream_id = Some(new_id.clone());
        new_id
    }

    pub(in crate::service::transform) fn get_or_default_stream_model(
        &self,
        source_api: LlmApiType,
    ) -> String {
        self.stream_model.clone().unwrap_or_else(|| {
            if source_api == LlmApiType::Gemini {
                "".to_string()
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

    pub(in crate::service::transform) fn diagnostics_snapshot(
        &self,
    ) -> Vec<UnifiedTransformDiagnostic> {
        self.diagnostics.iter().cloned().collect()
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn diagnostics_len(&self) -> usize {
        self.diagnostics.len()
    }

    #[cfg(test)]
    pub(in crate::service::transform) fn latest_diagnostic(
        &self,
    ) -> Option<&UnifiedTransformDiagnostic> {
        self.diagnostics.back()
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
        diagnostic: UnifiedTransformDiagnostic,
    ) {
        if self.diagnostics.len() >= STREAM_DIAGNOSTIC_WINDOW {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(diagnostic);
    }

    fn push_bounded(queue: &mut VecDeque<SseEvent>, event: SseEvent) {
        if queue.len() >= STREAM_DIAGNOSTIC_WINDOW {
            queue.pop_front();
        }
        queue.push_back(event);
    }

    pub(in crate::service::transform) fn merge_usage(
        &mut self,
        usage: UnifiedUsage,
        strategy: UsageMergeStrategy,
    ) {
        let _ = strategy;
        self.usage_normalization_cache = Some(UsageNormalization::from(&usage));
        self.usage_cache = Some(usage.into());
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

    fn gemini_message_index(&self, provider_order: u32) -> u32 {
        self.gemini
            .next_message_index_by_choice
            .get(&provider_order)
            .copied()
            .unwrap_or(0)
    }

    pub(in crate::service::transform) fn get_or_create_gemini_tool_call_id(
        &mut self,
        provider_order: u32,
        part_index: u32,
        function_name: &str,
    ) -> String {
        let message_index = self.gemini_message_index(provider_order);
        let key = gemini::build_gemini_tool_call_key(
            provider_order,
            message_index,
            part_index,
            function_name,
        );
        self.gemini
            .tool_call_id_map
            .entry(key)
            .or_insert_with(|| {
                gemini::build_gemini_synthetic_tool_call_id(
                    provider_order,
                    message_index,
                    part_index,
                    function_name,
                )
            })
            .clone()
    }

    pub(in crate::service::transform) fn advance_gemini_message_index(
        &mut self,
        provider_order: u32,
    ) {
        self.gemini
            .next_message_index_by_choice
            .entry(provider_order)
            .and_modify(|index| *index += 1)
            .or_insert(1);
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
                    self.stream_id = Some(id.clone());
                }
                if let Some(model) = model {
                    self.stream_model = Some(model.clone());
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
                if let Ok(diagnostic) =
                    serde_json::from_value::<UnifiedTransformDiagnostic>(error.clone())
                {
                    self.record_diagnostic(diagnostic);
                }
            }
            UnifiedStreamEvent::MessageStop
            | UnifiedStreamEvent::ContentBlockDelta { .. }
            | UnifiedStreamEvent::BlobDelta { .. } => {}
        }
    }
}

pub(crate) struct StreamTransformContext<'a> {
    source_api: LlmApiType,
    session: &'a mut SessionContext,
}

impl<'a> StreamTransformContext<'a> {
    pub(in crate::service::transform) fn new(
        source_api: LlmApiType,
        _target_api: LlmApiType,
        session: &'a mut SessionContext,
    ) -> Self {
        Self {
            source_api,
            session,
        }
    }

    pub(in crate::service::transform) fn get_or_generate_stream_id(&mut self) -> String {
        self.session.get_or_generate_stream_id(self.source_api)
    }

    pub(in crate::service::transform) fn get_or_default_stream_model(&self) -> String {
        self.session.get_or_default_stream_model(self.source_api)
    }

    pub(in crate::service::transform) fn stream_id_clone(&self) -> Option<String> {
        self.session.stream_id_clone()
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
        diagnostic: UnifiedTransformDiagnostic,
    ) {
        self.session.record_diagnostic(diagnostic);
    }

    pub(in crate::service::transform) fn usage_merge_strategy(&self) -> UsageMergeStrategy {
        match self.source_api {
            LlmApiType::Gemini | LlmApiType::Responses => UsageMergeStrategy::Replace,
            LlmApiType::Openai
            | LlmApiType::Anthropic
            | LlmApiType::Ollama
            | LlmApiType::GeminiOpenai => UsageMergeStrategy::FinalOnly,
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
