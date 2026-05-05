use cyder_tools::log::{debug, warn};
use serde_json::Value;

use super::error;
use super::session::{SessionContext, StreamTransformContext};
use super::usage::UsageMergeStrategy;
use crate::cost::UsageNormalization;
use crate::schema::enum_def::LlmApiType;
use crate::service::transform::TransformProtocol;
use crate::service::transform::adapter::{DecodedSourceStreamFrame, TransformAdapter, adapter_for};
use crate::service::transform::capability::TransformValueKind;
use crate::service::transform::diagnostics::build_transform_diagnostic;
use crate::service::transform::policy::{
    PolicyDecision, TransformAction, TransformDiagnosticKind, TransformLossLevel,
};
use crate::service::transform::providers::responses;
use crate::service::transform::unified::*;
use crate::utils::sse::SseEvent;
use crate::utils::usage::{self, UsageInfo};

pub struct StreamTransformer {
    pub(in crate::service::transform) api_type: LlmApiType,
    pub(in crate::service::transform) target_api_type: LlmApiType,
    pub(in crate::service::transform) session: SessionContext,
}

impl StreamTransformer {
    pub fn new(api_type: LlmApiType, target_api_type: LlmApiType) -> Self {
        Self {
            api_type,
            target_api_type,
            session: SessionContext::default(),
        }
    }

    pub fn transform_events(&mut self, events: Vec<SseEvent>) -> Vec<SseEvent> {
        events
            .into_iter()
            .flat_map(|event| self.transform_event(event).unwrap_or_default())
            .collect()
    }

    fn source_adapter(&self) -> &'static TransformAdapter {
        adapter_for(self.api_type)
    }

    fn target_adapter(&self) -> &'static TransformAdapter {
        adapter_for(self.target_api_type)
    }

    pub(in crate::service::transform) fn stream_context(&mut self) -> StreamTransformContext<'_> {
        StreamTransformContext::new(self.api_type, self.target_api_type, &mut self.session)
    }

    fn record_transformed_events(&mut self, events: &[SseEvent]) {
        for event in events {
            self.session.push_transformed_event(event.clone());
        }
    }

    pub(in crate::service::transform) fn usage_merge_strategy(&self) -> UsageMergeStrategy {
        match self.api_type {
            LlmApiType::Gemini | LlmApiType::Responses => UsageMergeStrategy::Replace,
            LlmApiType::Openai
            | LlmApiType::Anthropic
            | LlmApiType::Ollama
            | LlmApiType::GeminiOpenai => UsageMergeStrategy::FinalOnly,
        }
    }

    pub fn parse_usage_info(&mut self) -> Option<UsageInfo> {
        if let Some(usage) = self.session.usage_cache_clone() {
            return Some(usage);
        }

        if self.session.original_events_is_empty() {
            let decision = PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                level: TransformLossLevel::LossyMinor,
                action: TransformAction::Drop,
                reason: "Usage cache miss and empty diagnostic window prevented usage recovery.",
            };
            self.session.record_diagnostic(build_transform_diagnostic(
                TransformDiagnosticKind::CapabilityDowngrade,
                TransformProtocol::Api(self.api_type),
                TransformProtocol::Api(self.target_api_type),
                TransformValueKind::StreamError,
                decision,
                self.session.stream_id_clone(),
                Some("parse_usage_info"),
                Some("Unable to recover usage because no original stream events were retained."),
                Some("recent_original_events=0".to_string()),
                Some("Preserve upstream usage frames or widen the diagnostic window for this stream.".to_string()),
            ));
            debug!(
                "[transform][usage] stream_id={:?} provider={:?} no cached usage and no diagnostic events available",
                self.session.stream_id_clone(),
                self.api_type
            );
            return None;
        }

        let parsed = match self.api_type {
            LlmApiType::Openai | LlmApiType::GeminiOpenai => {
                self.session.original_events().iter().rev().find_map(|e| {
                    if e.data == "[DONE]" || e.data.is_empty() {
                        return None;
                    }
                    serde_json::from_str::<Value>(&e.data)
                        .ok()
                        .and_then(|v| usage::parse_usage_info(&v, self.api_type))
                })
            }
            LlmApiType::Gemini | LlmApiType::Ollama | LlmApiType::Responses => {
                self.session.original_events().iter().rev().find_map(|e| {
                    serde_json::from_str::<Value>(&e.data)
                        .ok()
                        .and_then(|v| usage::parse_usage_info(&v, self.api_type))
                })
            }
            LlmApiType::Anthropic => self
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
                        .and_then(|v| usage::parse_usage_info(&v, self.api_type))
                }),
        };

        if parsed.is_none() {
            let decision = PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                level: TransformLossLevel::LossyMinor,
                action: TransformAction::Drop,
                reason: "Usage cache miss forced a best-effort diagnostic fallback, but no recoverable usage payload was found.",
            };
            self.session.record_diagnostic(build_transform_diagnostic(
                TransformDiagnosticKind::CapabilityDowngrade,
                TransformProtocol::Api(self.api_type),
                TransformProtocol::Api(self.target_api_type),
                TransformValueKind::StreamError,
                decision,
                self.session.stream_id_clone(),
                Some("parse_usage_info"),
                Some("Unable to recover usage from cached stream diagnostics."),
                Some(format!(
                    "recent_original_events={}",
                    self.session.original_events_len()
                )),
                Some("Inspect upstream provider SSE usage frames or preserve a wider diagnostic window.".to_string()),
            ));
            warn!(
                "[transform][usage] stream_id={:?} provider={:?} usage cache miss and diagnostic fallback failed; recent_original_events={}",
                self.session.stream_id_clone(),
                self.api_type,
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

    pub fn diagnostics_snapshot(&self) -> Vec<UnifiedTransformDiagnostic> {
        self.session.diagnostics_snapshot()
    }

    pub(crate) fn get_or_generate_stream_id(&mut self) -> String {
        self.session.get_or_generate_stream_id(self.api_type)
    }

    pub(crate) fn get_or_default_stream_model(&self) -> String {
        self.session.get_or_default_stream_model(self.api_type)
    }

    pub(in crate::service::transform) fn normalize_unified_chunk_session_state(
        &mut self,
        unified_chunk: &mut UnifiedChunkResponse,
    ) {
        let chunk_core = unified_chunk.core();
        self.session.set_stream_model_if_present(chunk_core.model);
        if self.api_type == LlmApiType::Gemini {
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

    fn controlled_error_sse(
        &mut self,
        stage: &'static str,
        message: String,
        raw_data: &str,
    ) -> Vec<SseEvent> {
        error::controlled_error_sse(self, stage, message, raw_data)
    }

    pub(in crate::service::transform) fn bridge_stream_events_to_legacy_chunks(
        &mut self,
        events: Vec<UnifiedStreamEvent>,
    ) -> Vec<UnifiedChunkResponse> {
        super::bridge::bridge_stream_events_to_legacy_chunks(self, events)
    }

    fn stream_events_to_target_events(
        &mut self,
        stream_events: Vec<UnifiedStreamEvent>,
    ) -> Option<Vec<SseEvent>> {
        let mut transformed = Vec::new();
        let mut passthrough_events = Vec::new();

        for event in stream_events {
            match event {
                UnifiedStreamEvent::Error { error } => {
                    self.session.set_last_error(error.clone());
                    transformed.push(SseEvent {
                        event: Some("error".to_string()),
                        data: serde_json::to_string(&error).unwrap_or_else(|_| {
                            "{\"type\":\"transform_error\",\"message\":\"serialization failure\"}"
                                .to_string()
                        }),
                        ..Default::default()
                    });
                }
                other => passthrough_events.push(other),
            }
        }

        let target_adapter = self.target_adapter();
        let native_events = if target_adapter.stream.requires_legacy_bridge_for_events {
            let unified_chunks = self.bridge_stream_events_to_legacy_chunks(passthrough_events);
            let mut events = Vec::new();
            for unified_chunk in unified_chunks {
                let chunk_events = {
                    let mut context = self.stream_context();
                    (target_adapter.stream.encode_legacy_chunk)(unified_chunk, &mut context)
                };
                if let Some(chunk_events) = chunk_events {
                    events.extend(chunk_events);
                }
            }
            (!events.is_empty()).then_some(events)
        } else {
            let mut context = self.stream_context();
            (target_adapter.stream.encode_events)(passthrough_events, &mut context)
        };

        if let Some(events) = native_events {
            transformed.extend(events);
        }

        (!transformed.is_empty()).then_some(transformed)
    }

    pub fn transform_event(&mut self, event: SseEvent) -> Option<Vec<SseEvent>> {
        if event.data.is_empty() {
            return None;
        }

        self.session.push_original_event(event.clone());

        if self.api_type == self.target_api_type {
            // Best effort to update session state from passthrough events (e.g. usage info)
            let source_adapter = self.source_adapter();
            let decoded_frame = {
                let mut context = self.stream_context();
                (source_adapter.stream.decode_source)(&event.data, &mut context)
            };
            if let Ok(frame) = decoded_frame {
                match frame {
                    DecodedSourceStreamFrame::Events(stream_events) => {
                        self.update_session_from_stream_events(&stream_events);
                    }
                    DecodedSourceStreamFrame::LegacyChunk(mut unified_chunk) => {
                        self.normalize_unified_chunk_session_state(&mut unified_chunk);
                    }
                }
            }
            return Some(vec![event]);
        }

        // Handle OpenAI-compatible stream termination marker.
        if (self.api_type == LlmApiType::Openai || self.api_type == LlmApiType::GeminiOpenai)
            && event.data == "[DONE]"
        {
            return match self.target_api_type {
                LlmApiType::Anthropic => {
                    let transformed =
                        self.stream_events_to_target_events(vec![UnifiedStreamEvent::MessageStop]);
                    if let Some(events) = &transformed {
                        self.record_transformed_events(events);
                    }
                    transformed
                }
                LlmApiType::Gemini | LlmApiType::Ollama => None,
                _ => Some(vec![event]),
            };
        }

        if self.api_type == LlmApiType::Responses && self.target_api_type == LlmApiType::Openai {
            let transformed =
                match serde_json::from_str::<responses::ResponsesChunkResponse>(&event.data) {
                    Ok(chunk) => {
                        let mut context = self.stream_context();
                        responses::transform_responses_chunk_to_openai_events(chunk, &mut context)
                    }
                    Err(e) => {
                        let events = self.controlled_error_sse(
                            "deserialize_source_chunk",
                            format!(
                                "failed to deserialize {:?} chunk: {}",
                                LlmApiType::Responses,
                                e
                            ),
                            &event.data,
                        );
                        self.record_transformed_events(&events);
                        return Some(events);
                    }
                };

            if let Some(events) = &transformed {
                self.record_transformed_events(events);
            }

            return transformed;
        }

        let source_adapter = self.source_adapter();
        let target_adapter = self.target_adapter();

        let decoded_frame = {
            let mut context = self.stream_context();
            (source_adapter.stream.decode_source)(&event.data, &mut context)
        };

        let transformed = match decoded_frame {
            Ok(DecodedSourceStreamFrame::Events(stream_events)) => {
                self.update_session_from_stream_events(&stream_events);
                self.stream_events_to_target_events(stream_events)
            }
            Ok(DecodedSourceStreamFrame::LegacyChunk(mut unified_chunk)) => {
                let consistent_id = self.get_or_generate_stream_id();
                unified_chunk.id = consistent_id;
                self.normalize_unified_chunk_session_state(&mut unified_chunk);
                let mut context = self.stream_context();
                (target_adapter.stream.encode_legacy_chunk)(unified_chunk, &mut context)
            }
            Err(e) => {
                let events = self.controlled_error_sse(
                    "deserialize_source_chunk",
                    format!(
                        "failed to deserialize {:?} chunk: {}",
                        source_adapter.api_type, e
                    ),
                    &event.data,
                );
                self.record_transformed_events(&events);
                return Some(events);
            }
        };

        if let Some(events) = &transformed {
            self.record_transformed_events(events);
        }

        transformed
    }
}
