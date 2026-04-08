use cyder_tools::log::{debug, error, warn};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::schema::enum_def::{LlmApiType, ProviderType};
use crate::utils::billing::{self, UsageInfo};
use crate::utils::sse::SseEvent;

pub mod anthropic;
pub mod gemini;
pub mod ollama;
pub mod openai;
pub mod quality;
pub mod responses;
pub mod unified;
use unified::*;

const STREAM_DIAGNOSTIC_WINDOW: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TransformProtocol {
    Unified,
    Api(LlmApiType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformValueKind {
    TopKParameter,
    ToolDefinitions,
    ToolRoleMessage,
    ResponsesUnknownItem,
    Text,
    Refusal,
    ImageUrl,
    ImageData,
    FileUrl,
    FileData,
    ExecutableCode,
    ToolCall,
    ToolResult,
    ReasoningContent,
    ImageDelta,
    ToolCallDelta,
    ReasoningDelta,
    BlobDelta,
    StreamError,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformAction {
    Send,
    Drop,
    Reject,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformLossLevel {
    Lossless,
    LossyMinor,
    LossyMajor,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformDiagnosticKind {
    FatalTransformError,
    LossyTransform,
    CapabilityDowngrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicyDecision {
    pub diagnostic_kind: TransformDiagnosticKind,
    pub level: TransformLossLevel,
    pub action: TransformAction,
    pub reason: &'static str,
}

pub(crate) struct PolicyEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProtocolCapabilityMatrix {
    pub request: RequestCapabilityMatrix,
    pub response: ResponseCapabilityMatrix,
    pub stream: StreamCapabilityMatrix,
    pub structured_content: StructuredContentCapabilityMatrix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestCapabilityMatrix {
    pub tool_definitions: bool,
    pub tool_role_messages: bool,
    pub top_k_parameter: bool,
    pub image_url_input: bool,
    pub image_inline_input: bool,
    pub file_url_input: bool,
    pub file_inline_input: bool,
    pub executable_code_input: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResponseCapabilityMatrix {
    pub reasoning_content: bool,
    pub refusal: bool,
    pub citations: bool,
    pub file_output: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StreamCapabilityMatrix {
    pub tool_call_deltas: bool,
    pub reasoning_deltas: bool,
    pub reasoning_summary_parts: bool,
    pub image_deltas: bool,
    pub blob_deltas: bool,
    pub structured_errors: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StructuredContentCapabilityMatrix {
    pub images: bool,
    pub tool_results: bool,
    pub refusal: bool,
    pub citations: bool,
    pub file_references: bool,
    pub json_schema_strict: bool,
}

impl ProtocolCapabilityMatrix {
    pub(crate) const fn for_api(api: LlmApiType) -> Self {
        match api {
            LlmApiType::Openai | LlmApiType::GeminiOpenai => Self {
                request: RequestCapabilityMatrix {
                    tool_definitions: true,
                    tool_role_messages: true,
                    top_k_parameter: false,
                    image_url_input: true,
                    image_inline_input: false,
                    file_url_input: false,
                    file_inline_input: false,
                    executable_code_input: false,
                },
                response: ResponseCapabilityMatrix {
                    reasoning_content: false,
                    refusal: true,
                    citations: false,
                    file_output: false,
                },
                stream: StreamCapabilityMatrix {
                    tool_call_deltas: true,
                    reasoning_deltas: false,
                    reasoning_summary_parts: false,
                    image_deltas: false,
                    blob_deltas: false,
                    structured_errors: false,
                },
                structured_content: StructuredContentCapabilityMatrix {
                    images: true,
                    tool_results: true,
                    refusal: true,
                    citations: false,
                    file_references: false,
                    json_schema_strict: true,
                },
            },
            LlmApiType::Gemini => Self {
                request: RequestCapabilityMatrix {
                    tool_definitions: true,
                    tool_role_messages: true,
                    top_k_parameter: false,
                    image_url_input: true,
                    image_inline_input: true,
                    file_url_input: false,
                    file_inline_input: false,
                    executable_code_input: false,
                },
                response: ResponseCapabilityMatrix {
                    reasoning_content: false,
                    refusal: false,
                    citations: true,
                    file_output: false,
                },
                stream: StreamCapabilityMatrix {
                    tool_call_deltas: true,
                    reasoning_deltas: false,
                    reasoning_summary_parts: false,
                    image_deltas: false,
                    blob_deltas: false,
                    structured_errors: false,
                },
                structured_content: StructuredContentCapabilityMatrix {
                    images: true,
                    tool_results: true,
                    refusal: false,
                    citations: true,
                    file_references: false,
                    json_schema_strict: true,
                },
            },
            LlmApiType::Anthropic => Self {
                request: RequestCapabilityMatrix {
                    tool_definitions: true,
                    tool_role_messages: true,
                    top_k_parameter: true,
                    image_url_input: true,
                    image_inline_input: true,
                    file_url_input: true,
                    file_inline_input: true,
                    executable_code_input: true,
                },
                response: ResponseCapabilityMatrix {
                    reasoning_content: true,
                    refusal: true,
                    citations: false,
                    file_output: false,
                },
                stream: StreamCapabilityMatrix {
                    tool_call_deltas: true,
                    reasoning_deltas: true,
                    reasoning_summary_parts: false,
                    image_deltas: false,
                    blob_deltas: true,
                    structured_errors: true,
                },
                structured_content: StructuredContentCapabilityMatrix {
                    images: true,
                    tool_results: true,
                    refusal: true,
                    citations: false,
                    file_references: true,
                    json_schema_strict: true,
                },
            },
            LlmApiType::Responses => Self {
                request: RequestCapabilityMatrix {
                    tool_definitions: true,
                    tool_role_messages: true,
                    top_k_parameter: false,
                    image_url_input: true,
                    image_inline_input: true,
                    file_url_input: true,
                    file_inline_input: true,
                    executable_code_input: true,
                },
                response: ResponseCapabilityMatrix {
                    reasoning_content: true,
                    refusal: true,
                    citations: true,
                    file_output: true,
                },
                stream: StreamCapabilityMatrix {
                    tool_call_deltas: true,
                    reasoning_deltas: true,
                    reasoning_summary_parts: true,
                    image_deltas: false,
                    blob_deltas: true,
                    structured_errors: true,
                },
                structured_content: StructuredContentCapabilityMatrix {
                    images: true,
                    tool_results: true,
                    refusal: true,
                    citations: true,
                    file_references: true,
                    json_schema_strict: true,
                },
            },
            LlmApiType::Ollama => Self {
                request: RequestCapabilityMatrix {
                    tool_definitions: false,
                    tool_role_messages: false,
                    top_k_parameter: false,
                    image_url_input: false,
                    image_inline_input: false,
                    file_url_input: false,
                    file_inline_input: false,
                    executable_code_input: false,
                },
                response: ResponseCapabilityMatrix {
                    reasoning_content: false,
                    refusal: false,
                    citations: false,
                    file_output: false,
                },
                stream: StreamCapabilityMatrix {
                    tool_call_deltas: false,
                    reasoning_deltas: false,
                    reasoning_summary_parts: false,
                    image_deltas: false,
                    blob_deltas: false,
                    structured_errors: false,
                },
                structured_content: StructuredContentCapabilityMatrix {
                    images: false,
                    tool_results: false,
                    refusal: false,
                    citations: false,
                    file_references: false,
                    json_schema_strict: false,
                },
            },
        }
    }
}

type RequestDecodeFn = fn(Value) -> Result<UnifiedRequest, serde_json::Error>;
type RequestEncodeFn = fn(UnifiedRequest) -> Result<Value, serde_json::Error>;
type ResponseDecodeFn = fn(Value) -> Result<UnifiedResponse, serde_json::Error>;
type ResponseEncodeFn = fn(UnifiedResponse) -> Result<Value, serde_json::Error>;
type SourceStreamDecodeFn =
    fn(&str, &mut StreamTransformer) -> Result<DecodedSourceStreamFrame, serde_json::Error>;
type TargetStreamEventsEncodeFn =
    fn(Vec<UnifiedStreamEvent>, &mut StreamTransformer) -> Option<Vec<SseEvent>>;
type TargetLegacyChunkEncodeFn =
    fn(UnifiedChunkResponse, &mut StreamTransformer) -> Option<Vec<SseEvent>>;
type RequestFinalizeFn = fn(Value, &ProviderType, &str) -> Value;

#[derive(Clone, Copy)]
struct RequestCodec {
    decode: RequestDecodeFn,
    encode: RequestEncodeFn,
    finalize: Option<RequestFinalizeFn>,
}

#[derive(Clone, Copy)]
struct ResponseCodec {
    decode: ResponseDecodeFn,
    encode: ResponseEncodeFn,
}

#[derive(Clone, Copy)]
struct StreamCodec {
    decode_source: SourceStreamDecodeFn,
    encode_events: TargetStreamEventsEncodeFn,
    encode_legacy_chunk: TargetLegacyChunkEncodeFn,
    requires_legacy_bridge_for_events: bool,
}

#[derive(Clone, Copy)]
struct TransformAdapter {
    api_type: LlmApiType,
    name: &'static str,
    capabilities: ProtocolCapabilityMatrix,
    request: RequestCodec,
    response: ResponseCodec,
    stream: StreamCodec,
}

enum DecodedSourceStreamFrame {
    Events(Vec<UnifiedStreamEvent>),
    LegacyChunk(UnifiedChunkResponse),
}

fn noop_finalize_request(
    data: Value,
    _provider_type: &ProviderType,
    _downstream_path: &str,
) -> Value {
    data
}

fn finalize_openai_request(
    mut data: Value,
    provider_type: &ProviderType,
    downstream_path: &str,
) -> Value {
    apply_stream_options(&mut data);

    let (openai_variant, sanitize_report) = openai::finalize_openai_compatible_request_payload(
        &mut data,
        provider_type,
        downstream_path,
    );
    if !sanitize_report.removed_fields.is_empty() || !sanitize_report.injected_defaults.is_empty() {
        warn!(
            "[transform] Sanitized OpenAI-compatible payload for variant {:?}. removed={:?}, injected_defaults={:?}",
            openai_variant, sanitize_report.removed_fields, sanitize_report.injected_defaults
        );
    }

    data
}

fn decode_openai_request(data: Value) -> Result<UnifiedRequest, serde_json::Error> {
    serde_json::from_value::<openai::OpenAiRequestPayload>(data).map(Into::into)
}

fn encode_openai_request(unified: UnifiedRequest) -> Result<Value, serde_json::Error> {
    serde_json::to_value(openai::OpenAiRequestPayload::from(unified))
}

fn decode_gemini_request(data: Value) -> Result<UnifiedRequest, serde_json::Error> {
    serde_json::from_value::<gemini::GeminiRequestPayload>(data).map(Into::into)
}

fn encode_gemini_request(unified: UnifiedRequest) -> Result<Value, serde_json::Error> {
    serde_json::to_value(gemini::GeminiRequestPayload::from(unified))
}

fn decode_ollama_request(data: Value) -> Result<UnifiedRequest, serde_json::Error> {
    serde_json::from_value::<ollama::OllamaRequestPayload>(data).map(Into::into)
}

fn encode_ollama_request(unified: UnifiedRequest) -> Result<Value, serde_json::Error> {
    serde_json::to_value(ollama::OllamaRequestPayload::from(unified))
}

fn decode_anthropic_request(data: Value) -> Result<UnifiedRequest, serde_json::Error> {
    serde_json::from_value::<anthropic::AnthropicRequestPayload>(data).map(Into::into)
}

fn encode_anthropic_request(unified: UnifiedRequest) -> Result<Value, serde_json::Error> {
    serde_json::to_value(anthropic::AnthropicRequestPayload::from(unified))
}

fn decode_responses_request(data: Value) -> Result<UnifiedRequest, serde_json::Error> {
    serde_json::from_value::<responses::ResponsesRequestPayload>(data).map(Into::into)
}

fn encode_responses_request(unified: UnifiedRequest) -> Result<Value, serde_json::Error> {
    serde_json::to_value(responses::ResponsesRequestPayload::from(unified))
}

fn decode_openai_response(data: Value) -> Result<UnifiedResponse, serde_json::Error> {
    serde_json::from_value::<openai::OpenAiResponse>(data).map(Into::into)
}

fn encode_openai_response(unified: UnifiedResponse) -> Result<Value, serde_json::Error> {
    serde_json::to_value(openai::OpenAiResponse::from(unified))
}

fn decode_gemini_response(data: Value) -> Result<UnifiedResponse, serde_json::Error> {
    serde_json::from_value::<gemini::GeminiResponse>(data).map(Into::into)
}

fn encode_gemini_response(unified: UnifiedResponse) -> Result<Value, serde_json::Error> {
    serde_json::to_value(gemini::GeminiResponse::from(unified))
}

fn decode_ollama_response(data: Value) -> Result<UnifiedResponse, serde_json::Error> {
    serde_json::from_value::<ollama::OllamaResponse>(data).map(Into::into)
}

fn encode_ollama_response(unified: UnifiedResponse) -> Result<Value, serde_json::Error> {
    serde_json::to_value(ollama::OllamaResponse::from(unified))
}

fn decode_anthropic_response(data: Value) -> Result<UnifiedResponse, serde_json::Error> {
    serde_json::from_value::<anthropic::AnthropicResponse>(data).map(Into::into)
}

fn encode_anthropic_response(unified: UnifiedResponse) -> Result<Value, serde_json::Error> {
    serde_json::to_value(anthropic::AnthropicResponse::from(unified))
}

fn decode_responses_response(data: Value) -> Result<UnifiedResponse, serde_json::Error> {
    serde_json::from_value::<responses::ResponsesResponse>(data).map(Into::into)
}

fn encode_responses_response(unified: UnifiedResponse) -> Result<Value, serde_json::Error> {
    serde_json::to_value(responses::ResponsesResponse::from(unified))
}

fn decode_openai_stream_frame(
    raw: &str,
    _transformer: &mut StreamTransformer,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<openai::OpenAiChunkResponse>(raw)
        .map(openai::openai_chunk_to_unified_stream_events)
        .map(DecodedSourceStreamFrame::Events)
}

fn decode_gemini_stream_frame(
    raw: &str,
    _transformer: &mut StreamTransformer,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<gemini::GeminiChunkResponse>(raw)
        .map(Into::into)
        .map(DecodedSourceStreamFrame::LegacyChunk)
}

fn decode_ollama_stream_frame(
    raw: &str,
    _transformer: &mut StreamTransformer,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<ollama::OllamaChunkResponse>(raw)
        .map(Into::into)
        .map(DecodedSourceStreamFrame::LegacyChunk)
}

fn decode_anthropic_stream_frame(
    raw: &str,
    transformer: &mut StreamTransformer,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<anthropic::AnthropicEvent>(raw)
        .map(|event| {
            anthropic::anthropic_event_to_unified_stream_events_with_state(
                event,
                &mut transformer.session.anthropic,
            )
        })
        .map(DecodedSourceStreamFrame::Events)
}

fn decode_responses_stream_frame(
    raw: &str,
    _transformer: &mut StreamTransformer,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<responses::ResponsesChunkResponse>(raw)
        .map(responses::responses_chunk_to_unified_stream_events)
        .map(DecodedSourceStreamFrame::Events)
}

fn encode_anthropic_stream_events(
    stream_events: Vec<UnifiedStreamEvent>,
    transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    anthropic::transform_unified_stream_events_to_anthropic_events(stream_events, transformer)
}

fn encode_anthropic_legacy_chunk(
    unified_chunk: UnifiedChunkResponse,
    transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    anthropic::transform_unified_chunk_to_anthropic_events(unified_chunk, transformer)
}

const OPENAI_ADAPTER: TransformAdapter = TransformAdapter {
    api_type: LlmApiType::Openai,
    name: "openai",
    capabilities: ProtocolCapabilityMatrix::for_api(LlmApiType::Openai),
    request: RequestCodec {
        decode: decode_openai_request,
        encode: encode_openai_request,
        finalize: Some(finalize_openai_request),
    },
    response: ResponseCodec {
        decode: decode_openai_response,
        encode: encode_openai_response,
    },
    stream: StreamCodec {
        decode_source: decode_openai_stream_frame,
        encode_events: openai::transform_unified_stream_events_to_openai_events,
        encode_legacy_chunk: openai::transform_unified_chunk_to_openai_events,
        requires_legacy_bridge_for_events: false,
    },
};

const GEMINI_ADAPTER: TransformAdapter = TransformAdapter {
    api_type: LlmApiType::Gemini,
    name: "gemini",
    capabilities: ProtocolCapabilityMatrix::for_api(LlmApiType::Gemini),
    request: RequestCodec {
        decode: decode_gemini_request,
        encode: encode_gemini_request,
        finalize: Some(noop_finalize_request),
    },
    response: ResponseCodec {
        decode: decode_gemini_response,
        encode: encode_gemini_response,
    },
    stream: StreamCodec {
        decode_source: decode_gemini_stream_frame,
        encode_events: gemini::transform_unified_stream_events_to_gemini_events,
        encode_legacy_chunk: gemini::transform_unified_chunk_to_gemini_events,
        requires_legacy_bridge_for_events: false,
    },
};

const OLLAMA_ADAPTER: TransformAdapter = TransformAdapter {
    api_type: LlmApiType::Ollama,
    name: "ollama",
    capabilities: ProtocolCapabilityMatrix::for_api(LlmApiType::Ollama),
    request: RequestCodec {
        decode: decode_ollama_request,
        encode: encode_ollama_request,
        finalize: Some(noop_finalize_request),
    },
    response: ResponseCodec {
        decode: decode_ollama_response,
        encode: encode_ollama_response,
    },
    stream: StreamCodec {
        decode_source: decode_ollama_stream_frame,
        encode_events: ollama::transform_unified_stream_events_to_ollama_events,
        encode_legacy_chunk: ollama::transform_unified_chunk_to_ollama_events,
        requires_legacy_bridge_for_events: false,
    },
};

const ANTHROPIC_ADAPTER: TransformAdapter = TransformAdapter {
    api_type: LlmApiType::Anthropic,
    name: "anthropic",
    capabilities: ProtocolCapabilityMatrix::for_api(LlmApiType::Anthropic),
    request: RequestCodec {
        decode: decode_anthropic_request,
        encode: encode_anthropic_request,
        finalize: Some(noop_finalize_request),
    },
    response: ResponseCodec {
        decode: decode_anthropic_response,
        encode: encode_anthropic_response,
    },
    stream: StreamCodec {
        decode_source: decode_anthropic_stream_frame,
        encode_events: encode_anthropic_stream_events,
        encode_legacy_chunk: encode_anthropic_legacy_chunk,
        requires_legacy_bridge_for_events: false,
    },
};

const RESPONSES_ADAPTER: TransformAdapter = TransformAdapter {
    api_type: LlmApiType::Responses,
    name: "responses",
    capabilities: ProtocolCapabilityMatrix::for_api(LlmApiType::Responses),
    request: RequestCodec {
        decode: decode_responses_request,
        encode: encode_responses_request,
        finalize: Some(noop_finalize_request),
    },
    response: ResponseCodec {
        decode: decode_responses_response,
        encode: encode_responses_response,
    },
    stream: StreamCodec {
        decode_source: decode_responses_stream_frame,
        encode_events: responses::transform_unified_stream_events_to_responses_events,
        encode_legacy_chunk: responses::transform_unified_chunk_to_responses_events,
        requires_legacy_bridge_for_events: false,
    },
};

fn adapter_for(api_type: LlmApiType) -> &'static TransformAdapter {
    match api_type {
        LlmApiType::Openai => &OPENAI_ADAPTER,
        LlmApiType::Gemini => &GEMINI_ADAPTER,
        LlmApiType::Ollama => &OLLAMA_ADAPTER,
        LlmApiType::Anthropic => &ANTHROPIC_ADAPTER,
        LlmApiType::Responses => &RESPONSES_ADAPTER,
        LlmApiType::GeminiOpenai => &OPENAI_ADAPTER,
    }
}

#[derive(Debug, Default, Clone)]
pub struct AnthropicSessionState {
    pub message_started: bool,
    pub active_blocks: HashMap<u32, AnthropicActiveBlockState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnthropicActiveBlockKind {
    Text,
    ToolUse,
    Thinking,
}

#[derive(Debug, Clone)]
pub struct AnthropicActiveBlockState {
    pub kind: AnthropicActiveBlockKind,
    pub text: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
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

#[allow(dead_code)]
#[derive(Debug, Default, Clone)]
pub struct GeminiSessionState {
    pub tool_call_id_map: HashMap<String, String>,
    pub next_message_index_by_choice: HashMap<u32, u32>,
}

#[allow(dead_code)]
#[derive(Debug, Default, Clone)]
pub struct ResponsesSessionState {
    pub created_sent: bool,
    pub completion_pending: bool,
    pub next_output_index: u32,
    pub current_output_index: u32,
    pub current_item_id: Option<String>,
    pub current_item_role: Option<crate::service::transform::unified::UnifiedRole>,
    pub output_item_ids: HashMap<u32, String>,
    pub output_text: String,
    pub reasoning_item_ids: HashMap<u32, String>,
    pub reasoning_summaries: HashMap<u32, String>,
    pub active_tool_calls: HashMap<u32, crate::service::transform::responses::FunctionCall>,
    pub completed_output: BTreeMap<u32, crate::service::transform::responses::ItemField>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageMergeStrategy {
    Replace,
    FinalOnly,
}

#[allow(dead_code)]
#[derive(Debug, Default, Clone)]
pub struct SessionContext {
    pub stream_id: Option<String>,
    pub stream_model: Option<String>,
    pub tool_call_id_map: HashMap<String, String>,
    pub current_item_index: Option<u32>,
    pub current_content_block_index: Option<u32>,
    pub current_content_part_index: Option<u32>,
    pub current_reasoning_block_index: Option<u32>,
    pub current_reasoning_part_index: Option<u32>,
    pub usage_cache: Option<UsageInfo>,
    pub finish_reason_cache: Option<String>,
    pub last_error: Option<Value>,
    pub diagnostics: VecDeque<UnifiedTransformDiagnostic>,
    pub original_events: VecDeque<SseEvent>,
    pub transformed_events: VecDeque<SseEvent>,
    pub anthropic: AnthropicSessionState,
    pub gemini: GeminiSessionState,
    pub responses: ResponsesSessionState,
}

impl SessionContext {
    fn push_original_event(&mut self, event: SseEvent) {
        Self::push_bounded(&mut self.original_events, event);
    }

    fn push_transformed_event(&mut self, event: SseEvent) {
        Self::push_bounded(&mut self.transformed_events, event);
    }

    fn record_diagnostic(&mut self, diagnostic: UnifiedTransformDiagnostic) {
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

    fn merge_usage(&mut self, usage: UsageInfo, strategy: UsageMergeStrategy) {
        let _ = strategy;
        self.usage_cache = Some(usage);
    }

    fn gemini_message_index(&self, provider_order: u32) -> u32 {
        self.gemini
            .next_message_index_by_choice
            .get(&provider_order)
            .copied()
            .unwrap_or(0)
    }

    fn get_or_create_gemini_tool_call_id(
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

    fn advance_gemini_message_index(&mut self, provider_order: u32) {
        self.gemini
            .next_message_index_by_choice
            .entry(provider_order)
            .and_modify(|index| *index += 1)
            .or_insert(1);
    }
}

impl From<&UnifiedContentPart> for TransformValueKind {
    fn from(part: &UnifiedContentPart) -> Self {
        match part {
            UnifiedContentPart::Text { .. } => Self::Text,
            UnifiedContentPart::Refusal { .. } => Self::Refusal,
            UnifiedContentPart::Reasoning { .. } => Self::ReasoningContent,
            UnifiedContentPart::ImageUrl { .. } => Self::ImageUrl,
            UnifiedContentPart::ImageData { .. } => Self::ImageData,
            UnifiedContentPart::FileUrl { .. } => Self::FileUrl,
            UnifiedContentPart::FileData { .. } => Self::FileData,
            UnifiedContentPart::ExecutableCode { .. } => Self::ExecutableCode,
            UnifiedContentPart::ToolCall(_) => Self::ToolCall,
            UnifiedContentPart::ToolResult(_) => Self::ToolResult,
        }
    }
}

impl From<&UnifiedContentPartDelta> for TransformValueKind {
    fn from(part: &UnifiedContentPartDelta) -> Self {
        match part {
            UnifiedContentPartDelta::TextDelta { .. } => Self::Text,
            UnifiedContentPartDelta::ImageDelta { .. } => Self::ImageDelta,
            UnifiedContentPartDelta::ToolCallDelta(_) => Self::ToolCallDelta,
        }
    }
}

impl PolicyEngine {
    fn target_capabilities(target: TransformProtocol) -> Option<ProtocolCapabilityMatrix> {
        match target {
            TransformProtocol::Api(api) => Some(adapter_for(api).capabilities),
            TransformProtocol::Unified => None,
        }
    }

    fn evaluate_capability_matrix(
        target: TransformProtocol,
        kind: TransformValueKind,
    ) -> Option<PolicyDecision> {
        let capabilities = Self::target_capabilities(target)?;

        match kind {
            TransformValueKind::TopKParameter if !capabilities.request.top_k_parameter => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMinor,
                    action: TransformAction::Drop,
                    reason: "The target request capability matrix marks top_k as unsupported.",
                })
            }
            TransformValueKind::ToolDefinitions if !capabilities.request.tool_definitions => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target request capability matrix marks tool definitions as unsupported.",
                })
            }
            TransformValueKind::ToolRoleMessage if !capabilities.request.tool_role_messages => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target request capability matrix marks tool role messages as unsupported.",
                })
            }
            TransformValueKind::ToolCallDelta if !capabilities.stream.tool_call_deltas => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target stream capability matrix marks tool call deltas as unsupported.",
                })
            }
            TransformValueKind::ReasoningDelta if !capabilities.stream.reasoning_deltas => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target stream capability matrix marks reasoning deltas as unsupported.",
                })
            }
            TransformValueKind::BlobDelta if !capabilities.stream.blob_deltas => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target stream capability matrix marks blob deltas as unsupported.",
                })
            }
            TransformValueKind::StreamError if !capabilities.stream.structured_errors => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target stream capability matrix marks structured stream errors as unsupported.",
                })
            }
            TransformValueKind::ReasoningContent if !capabilities.response.reasoning_content => {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target response capability matrix marks reasoning content as unsupported.",
                })
            }
            TransformValueKind::Refusal
                if !capabilities.response.refusal || !capabilities.structured_content.refusal =>
            {
                Some(PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target capability matrix marks refusal content as unsupported.",
                })
            }
            _ => None,
        }
    }

    pub(crate) fn evaluate(
        source: TransformProtocol,
        target: TransformProtocol,
        kind: TransformValueKind,
    ) -> PolicyDecision {
        if let Some(decision) = Self::evaluate_capability_matrix(target, kind) {
            return decision;
        }

        match (source, target, kind) {
            (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::ImageUrl)
            | (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::FileUrl)
            | (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::FileData)
            | (
                _,
                TransformProtocol::Api(LlmApiType::Anthropic),
                TransformValueKind::ExecutableCode,
            ) => PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                level: TransformLossLevel::LossyMajor,
                action: TransformAction::Send,
                reason: "Anthropic adapter preserves this request content with native image blocks or recoverable text downgrade.",
            },
            (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::ImageData) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                    level: TransformLossLevel::Lossless,
                    action: TransformAction::Send,
                    reason: "Anthropic adapter can preserve inline image data natively in request blocks.",
                }
            }
            (_, TransformProtocol::Api(LlmApiType::Gemini), TransformValueKind::ImageUrl) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Send,
                    reason: "Gemini adapter preserves remote image URLs as recoverable text when inline bytes are unavailable.",
                }
            }
            (_, TransformProtocol::Api(LlmApiType::Responses), TransformValueKind::ImageData)
            | (_, TransformProtocol::Api(LlmApiType::Responses), TransformValueKind::FileUrl)
            | (_, TransformProtocol::Api(LlmApiType::Responses), TransformValueKind::FileData)
            | (
                _,
                TransformProtocol::Api(LlmApiType::Responses),
                TransformValueKind::ExecutableCode,
            ) => PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                level: TransformLossLevel::LossyMajor,
                action: TransformAction::Send,
                reason: "Responses adapter preserves this content in item inputs or recoverable instruction text.",
            },
            (
                TransformProtocol::Api(LlmApiType::Responses),
                TransformProtocol::Unified,
                TransformValueKind::ResponsesUnknownItem,
            ) => PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                level: TransformLossLevel::LossyMajor,
                action: TransformAction::Drop,
                reason: "Responses item is structured and not formally supported by the current unified response adapter.",
            },
            (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::ImageData)
            | (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::FileUrl)
            | (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::FileData)
            | (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::ExecutableCode) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "OpenAI chat adapter cannot encode this unified content type in this path.",
                }
            }
            (
                _,
                TransformProtocol::Api(LlmApiType::Ollama),
                TransformValueKind::ToolRoleMessage,
            )
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ToolCall)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ToolResult)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ImageUrl)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ImageData)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::FileUrl)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::FileData)
            | (_, TransformProtocol::Api(LlmApiType::Ollama), TransformValueKind::ExecutableCode) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Send,
                    reason: "Ollama adapter preserves structured request content as base64 images plus recoverable plain text.",
                }
            }
            (_, TransformProtocol::Api(LlmApiType::Openai), TransformValueKind::ImageDelta)
            | (_, TransformProtocol::Api(LlmApiType::Gemini), TransformValueKind::ImageDelta)
            | (_, TransformProtocol::Api(LlmApiType::Anthropic), TransformValueKind::ImageDelta) => {
                PolicyDecision {
                    diagnostic_kind: TransformDiagnosticKind::CapabilityDowngrade,
                    level: TransformLossLevel::LossyMajor,
                    action: TransformAction::Drop,
                    reason: "The target streaming adapter cannot express image deltas yet.",
                }
            }
            _ => PolicyDecision {
                diagnostic_kind: TransformDiagnosticKind::LossyTransform,
                level: TransformLossLevel::Lossless,
                action: TransformAction::Send,
                reason: "lossless",
            },
        }
    }
}

fn diagnostic_action(action: TransformAction) -> UnifiedTransformDiagnosticAction {
    match action {
        TransformAction::Send => UnifiedTransformDiagnosticAction::Send,
        TransformAction::Drop => UnifiedTransformDiagnosticAction::Drop,
        TransformAction::Reject => UnifiedTransformDiagnosticAction::Reject,
    }
}

fn diagnostic_loss_level(level: TransformLossLevel) -> UnifiedTransformDiagnosticLossLevel {
    match level {
        TransformLossLevel::Lossless => UnifiedTransformDiagnosticLossLevel::Lossless,
        TransformLossLevel::LossyMinor => UnifiedTransformDiagnosticLossLevel::LossyMinor,
        TransformLossLevel::LossyMajor => UnifiedTransformDiagnosticLossLevel::LossyMajor,
        TransformLossLevel::Reject => UnifiedTransformDiagnosticLossLevel::Reject,
    }
}

fn diagnostic_kind(kind: TransformDiagnosticKind) -> UnifiedTransformDiagnosticKind {
    match kind {
        TransformDiagnosticKind::FatalTransformError => {
            UnifiedTransformDiagnosticKind::FatalTransformError
        }
        TransformDiagnosticKind::LossyTransform => UnifiedTransformDiagnosticKind::LossyTransform,
        TransformDiagnosticKind::CapabilityDowngrade => {
            UnifiedTransformDiagnosticKind::CapabilityDowngrade
        }
    }
}

fn protocol_name(protocol: TransformProtocol) -> String {
    match protocol {
        TransformProtocol::Unified => "unified".to_string(),
        TransformProtocol::Api(api) => format!("{api:?}"),
    }
}

fn build_transform_diagnostic(
    diagnostic_kind_value: TransformDiagnosticKind,
    source: TransformProtocol,
    target: TransformProtocol,
    kind: TransformValueKind,
    decision: PolicyDecision,
    stream_id: Option<String>,
    stage: Option<&str>,
    context: Option<&str>,
    raw_data_summary: Option<String>,
    recovery_hint: Option<String>,
) -> UnifiedTransformDiagnostic {
    UnifiedTransformDiagnostic {
        type_: match diagnostic_kind_value {
            TransformDiagnosticKind::FatalTransformError => "transform_error".to_string(),
            TransformDiagnosticKind::LossyTransform
            | TransformDiagnosticKind::CapabilityDowngrade => "transform_diagnostic".to_string(),
        },
        diagnostic_kind: diagnostic_kind(diagnostic_kind_value),
        provider: protocol_name(source),
        target_provider: protocol_name(target),
        source: protocol_name(source),
        target: protocol_name(target),
        stream_id,
        stage: stage.map(ToString::to_string),
        loss_level: diagnostic_loss_level(decision.level),
        action: diagnostic_action(decision.action),
        semantic_unit: format!("{kind:?}"),
        reason: decision.reason.to_string(),
        context: context.map(ToString::to_string),
        raw_data_summary,
        recovery_hint,
    }
}

pub(crate) fn apply_transform_policy(
    source: TransformProtocol,
    target: TransformProtocol,
    kind: TransformValueKind,
    context: &'static str,
) -> bool {
    let decision = PolicyEngine::evaluate(source, target, kind);
    if decision.level != TransformLossLevel::Lossless {
        let diagnostic = build_transform_diagnostic(
            decision.diagnostic_kind,
            source,
            target,
            kind,
            decision,
            None,
            None,
            Some(context),
            None,
            Some(decision.reason.to_string()),
        );
        let diagnostic_json =
            serde_json::to_string(&diagnostic).unwrap_or_else(|_| "{}".to_string());
        match decision.level {
            TransformLossLevel::LossyMinor | TransformLossLevel::LossyMajor => {
                warn!("[transform][diagnostic] {}", diagnostic_json)
            }
            TransformLossLevel::Reject => error!("[transform][diagnostic] {}", diagnostic_json),
            TransformLossLevel::Lossless => {}
        }
    }

    matches!(decision.action, TransformAction::Send)
}

pub(super) fn build_stream_diagnostic_sse(
    transformer: &mut StreamTransformer,
    source: TransformProtocol,
    target: TransformProtocol,
    kind: TransformValueKind,
    stage: &'static str,
    context: String,
    raw_data_summary: Option<String>,
    recovery_hint: Option<String>,
) -> SseEvent {
    let decision = PolicyEngine::evaluate(source, target, kind);
    let diagnostic = build_transform_diagnostic(
        decision.diagnostic_kind,
        source,
        target,
        kind,
        decision,
        transformer.session.stream_id.clone(),
        Some(stage),
        Some(&context),
        raw_data_summary,
        recovery_hint.or_else(|| Some(decision.reason.to_string())),
    );
    let diagnostic_json = serde_json::to_string(&diagnostic).unwrap_or_else(|_| {
        "{\"type\":\"transform_diagnostic\",\"message\":\"serialization failure\"}".to_string()
    });
    transformer.session.record_diagnostic(diagnostic);
    match decision.level {
        TransformLossLevel::LossyMinor | TransformLossLevel::LossyMajor => {
            warn!("[transform][diagnostic] {}", diagnostic_json)
        }
        TransformLossLevel::Reject => error!("[transform][diagnostic] {}", diagnostic_json),
        TransformLossLevel::Lossless => debug!("[transform][diagnostic] {}", diagnostic_json),
    }

    SseEvent {
        event: Some("transform_diagnostic".to_string()),
        data: diagnostic_json,
        ..Default::default()
    }
}

fn apply_stream_options(data: &mut Value) {
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

pub fn finalize_request_data(
    data: Value,
    target_api_type: LlmApiType,
    provider_type: &ProviderType,
    downstream_path: &str,
) -> Value {
    let adapter = adapter_for(target_api_type);
    let finalize = adapter.request.finalize.unwrap_or(noop_finalize_request);
    finalize(data, provider_type, downstream_path)
}

pub fn transform_request_data(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    is_stream: bool,
) -> Value {
    if api_type == target_api_type {
        return data;
    }

    debug!(
        "[transform] API type mismatch. Incoming: {:?}, Target: {:?}. Transforming request body.",
        api_type, target_api_type
    );

    let source_adapter = adapter_for(api_type);
    let target_adapter = adapter_for(target_api_type);

    let mut unified_request: UnifiedRequest = match (source_adapter.request.decode)(data.clone()) {
        Ok(payload) => payload,
        Err(e) => {
            error!(
                "[transform] Failed to deserialize {} request: {}. Returning original data.",
                source_adapter.name, e
            );
            return data;
        }
    };

    // The `is_stream` from the request URL is the source of truth.
    unified_request.stream = is_stream;

    // Warn if top_k is used with non-Anthropic targets
    if unified_request.top_k().is_some() && target_api_type != LlmApiType::Anthropic {
        apply_transform_policy(
            TransformProtocol::Api(api_type),
            TransformProtocol::Api(target_api_type),
            TransformValueKind::TopKParameter,
            "Dropping unsupported request field during UnifiedRequest serialization.",
        );
    }

    // Warn if tools are used with Ollama
    if unified_request.tools.is_some() && target_api_type == LlmApiType::Ollama {
        apply_transform_policy(
            TransformProtocol::Api(api_type),
            TransformProtocol::Api(target_api_type),
            TransformValueKind::ToolDefinitions,
            "Dropping unsupported tool definitions during UnifiedRequest serialization.",
        );
    }

    debug!("[transform] unified request: {:?}", unified_request);

    let target_payload_result = (target_adapter.request.encode)(unified_request);

    match target_payload_result {
        Ok(value) => {
            debug!(
                "[transform] Transformation complete. Result: {}",
                serde_json::to_string(&value).unwrap_or_default()
            );
            value
        }
        Err(e) => {
            error!(
                "[transform] Failed to serialize to target request format: {}. Returning original data.",
                e
            );
            data
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::sse::SseEvent;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct SemanticToolCall {
        name: Option<String>,
        arguments: String,
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct SemanticReplaySnapshot {
        stream_id: Option<String>,
        model: Option<String>,
        text: String,
        reasoning: String,
        finish_reason: Option<String>,
        usage: Option<(u32, u32, u32)>,
        tool_calls: Vec<SemanticToolCall>,
        binary_payload_count: usize,
        error_count: usize,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct ReplayRegressionReport {
        fixture_name: &'static str,
        source_api: LlmApiType,
        target_api: LlmApiType,
        source: SemanticReplaySnapshot,
        target: SemanticReplaySnapshot,
        source_frame_count: usize,
        transformed_frame_count: usize,
        preserved_text: bool,
        preserved_reasoning: bool,
        preserved_tool_calls: bool,
        preserved_finish_reason: bool,
        preserved_usage: bool,
        preserved_binary_payloads: bool,
    }

    #[derive(Debug, Clone, Copy)]
    struct ReplayFixtureCase {
        fixture_name: &'static str,
        source_api: LlmApiType,
        target_api: LlmApiType,
        fixture_json: &'static str,
    }

    fn load_sse_fixture(path: &str) -> Vec<SseEvent> {
        serde_json::from_str(path).expect("valid SSE fixture")
    }

    fn semantic_snapshot_from_stream_events(
        events: impl IntoIterator<Item = UnifiedStreamEvent>,
    ) -> SemanticReplaySnapshot {
        let mut snapshot = SemanticReplaySnapshot::default();
        let mut tool_calls: BTreeMap<u32, SemanticToolCall> = BTreeMap::new();

        for event in events {
            match event {
                UnifiedStreamEvent::MessageStart { id, model, .. } => {
                    if snapshot.stream_id.is_none() {
                        snapshot.stream_id = id;
                    }
                    if snapshot.model.is_none() {
                        snapshot.model = model.filter(|value| !value.is_empty());
                    }
                }
                UnifiedStreamEvent::ItemAdded { .. } | UnifiedStreamEvent::ItemDone { .. } => {}
                UnifiedStreamEvent::ContentBlockDelta { text, .. } => {
                    snapshot.text.push_str(&text);
                }
                UnifiedStreamEvent::ReasoningDelta { text, .. } => {
                    snapshot.reasoning.push_str(&text);
                }
                UnifiedStreamEvent::ToolCallStart { index, name, .. } => {
                    tool_calls.entry(index).or_default().name = Some(name);
                }
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index,
                    name,
                    arguments,
                    ..
                } => {
                    let entry = tool_calls.entry(index).or_default();
                    if entry.name.is_none() {
                        entry.name = name;
                    }
                    entry.arguments.push_str(&arguments);
                }
                UnifiedStreamEvent::MessageDelta { finish_reason } => {
                    if finish_reason.is_some() {
                        snapshot.finish_reason = finish_reason;
                    }
                }
                UnifiedStreamEvent::Usage { usage } => {
                    snapshot.usage =
                        Some((usage.input_tokens, usage.output_tokens, usage.total_tokens));
                }
                UnifiedStreamEvent::BlobDelta { .. } => {
                    snapshot.binary_payload_count += 1;
                }
                UnifiedStreamEvent::Error { .. } => {
                    snapshot.error_count += 1;
                }
                UnifiedStreamEvent::MessageStop
                | UnifiedStreamEvent::ContentPartAdded { .. }
                | UnifiedStreamEvent::ContentPartDone { .. }
                | UnifiedStreamEvent::ContentBlockStart { .. }
                | UnifiedStreamEvent::ContentBlockStop { .. }
                | UnifiedStreamEvent::ToolCallStop { .. }
                | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
                | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
                | UnifiedStreamEvent::ReasoningStart { .. }
                | UnifiedStreamEvent::ReasoningStop { .. } => {}
            }
        }

        snapshot.tool_calls = tool_calls.into_values().collect();
        snapshot
    }

    fn semantic_snapshot_from_unified_chunks(
        chunks: impl IntoIterator<Item = UnifiedChunkResponse>,
    ) -> SemanticReplaySnapshot {
        let mut snapshot = SemanticReplaySnapshot::default();
        let mut tool_calls: BTreeMap<u32, SemanticToolCall> = BTreeMap::new();

        for chunk in chunks {
            if snapshot.stream_id.is_none() && !chunk.id.is_empty() {
                snapshot.stream_id = Some(chunk.id.clone());
            }
            if snapshot.model.is_none() {
                snapshot.model = chunk.model.clone().filter(|value| !value.is_empty());
            }
            if let Some(usage) = chunk.usage {
                snapshot.usage =
                    Some((usage.input_tokens, usage.output_tokens, usage.total_tokens));
            }

            for choice in chunk.choices {
                if choice.finish_reason.is_some() {
                    snapshot.finish_reason = choice.finish_reason;
                }

                for part in choice.delta.content {
                    match part {
                        UnifiedContentPartDelta::TextDelta { text, .. } => {
                            snapshot.text.push_str(&text);
                        }
                        UnifiedContentPartDelta::ToolCallDelta(tool_call) => {
                            let entry = tool_calls.entry(tool_call.index).or_default();
                            if entry.name.is_none() {
                                entry.name = tool_call.name;
                            }
                            if let Some(arguments) = tool_call.arguments {
                                entry.arguments.push_str(&arguments);
                            }
                        }
                        UnifiedContentPartDelta::ImageDelta { .. } => {
                            snapshot.binary_payload_count += 1;
                        }
                    }
                }
            }
        }

        snapshot.tool_calls = tool_calls.into_values().collect();
        snapshot
    }

    fn source_fixture_to_semantics(
        source_api: LlmApiType,
        fixture: &[SseEvent],
    ) -> SemanticReplaySnapshot {
        match source_api {
            LlmApiType::Anthropic => semantic_snapshot_from_stream_events(
                fixture
                    .iter()
                    .flat_map(|event| {
                        let parsed: anthropic::AnthropicEvent =
                            serde_json::from_str(&event.data).expect("valid anthropic fixture");
                        anthropic::anthropic_event_to_unified_stream_events(parsed)
                    })
                    .collect::<Vec<_>>(),
            ),
            LlmApiType::Responses => semantic_snapshot_from_stream_events(
                fixture
                    .iter()
                    .flat_map(|event| {
                        let parsed: responses::ResponsesChunkResponse =
                            serde_json::from_str(&event.data).expect("valid responses fixture");
                        responses::responses_chunk_to_unified_stream_events(parsed)
                    })
                    .collect::<Vec<_>>(),
            ),
            LlmApiType::Gemini => semantic_snapshot_from_unified_chunks(
                fixture
                    .iter()
                    .filter(|event| event.event.is_none())
                    .map(|event| {
                        let parsed: gemini::GeminiChunkResponse =
                            serde_json::from_str(&event.data).expect("valid gemini fixture");
                        UnifiedChunkResponse::from(parsed)
                    })
                    .collect::<Vec<_>>(),
            ),
            LlmApiType::Openai | LlmApiType::GeminiOpenai => semantic_snapshot_from_unified_chunks(
                fixture
                    .iter()
                    .filter(|event| event.event.is_none())
                    .map(|event| {
                        let parsed: openai::OpenAiChunkResponse =
                            serde_json::from_str(&event.data).expect("valid openai fixture");
                        UnifiedChunkResponse::from(parsed)
                    })
                    .collect::<Vec<_>>(),
            ),
            LlmApiType::Ollama => semantic_snapshot_from_unified_chunks(
                fixture
                    .iter()
                    .filter(|event| event.event.is_none())
                    .map(|event| {
                        let parsed: ollama::OllamaChunkResponse =
                            serde_json::from_str(&event.data).expect("valid ollama fixture");
                        UnifiedChunkResponse::from(parsed)
                    })
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn target_fixture_to_semantics(
        target_api: LlmApiType,
        fixture: &[SseEvent],
    ) -> SemanticReplaySnapshot {
        source_fixture_to_semantics(target_api, fixture)
    }

    fn replay_fixture_through_transformer(
        source_api: LlmApiType,
        target_api: LlmApiType,
        fixture: &[SseEvent],
    ) -> Vec<SseEvent> {
        let mut transformer = StreamTransformer::new(source_api, target_api);
        fixture
            .iter()
            .flat_map(|event| {
                transformer
                    .transform_event(event.clone())
                    .unwrap_or_default()
            })
            .collect()
    }

    fn build_replay_regression_report(case: ReplayFixtureCase) -> ReplayRegressionReport {
        let fixture = load_sse_fixture(case.fixture_json);
        let source = source_fixture_to_semantics(case.source_api, &fixture);
        let transformed =
            replay_fixture_through_transformer(case.source_api, case.target_api, &fixture);
        let target = target_fixture_to_semantics(case.target_api, &transformed);

        ReplayRegressionReport {
            fixture_name: case.fixture_name,
            source_api: case.source_api,
            target_api: case.target_api,
            preserved_text: source.text == target.text,
            preserved_reasoning: source.reasoning == target.reasoning,
            preserved_tool_calls: source.tool_calls == target.tool_calls,
            preserved_finish_reason: source.finish_reason == target.finish_reason,
            preserved_usage: source.usage == target.usage,
            preserved_binary_payloads: source.binary_payload_count == target.binary_payload_count,
            source_frame_count: fixture.len(),
            transformed_frame_count: transformed.len(),
            source,
            target,
        }
    }

    #[test]
    fn test_policy_engine_marks_top_k_as_lossy_minor_for_non_anthropic_targets() {
        let decision = PolicyEngine::evaluate(
            TransformProtocol::Api(LlmApiType::Anthropic),
            TransformProtocol::Api(LlmApiType::Openai),
            TransformValueKind::TopKParameter,
        );

        assert_eq!(
            decision.diagnostic_kind,
            TransformDiagnosticKind::CapabilityDowngrade
        );
        assert_eq!(decision.level, TransformLossLevel::LossyMinor);
        assert_eq!(decision.action, TransformAction::Drop);
    }

    #[test]
    fn test_semantic_replay_anthropic_tool_use_json_delta_to_responses() {
        let fixture = load_sse_fixture(include_str!("testdata/anthropic_tool_use_json_delta.json"));

        let source_semantics = source_fixture_to_semantics(LlmApiType::Anthropic, &fixture);
        let transformed = replay_fixture_through_transformer(
            LlmApiType::Anthropic,
            LlmApiType::Responses,
            &fixture,
        );
        let target_semantics = target_fixture_to_semantics(LlmApiType::Responses, &transformed);

        assert_eq!(source_semantics.text, target_semantics.text);
        assert_eq!(source_semantics.tool_calls, target_semantics.tool_calls);
        assert_eq!(
            source_semantics.finish_reason,
            target_semantics.finish_reason
        );
        assert_eq!(source_semantics.usage, target_semantics.usage);
    }

    #[test]
    fn test_semantic_replay_responses_reasoning_and_function_call_round_trip() {
        let fixture = load_sse_fixture(include_str!(
            "testdata/responses_reasoning_function_call.json"
        ));

        let source_semantics = source_fixture_to_semantics(LlmApiType::Responses, &fixture);
        let unified_events: Vec<UnifiedStreamEvent> = fixture
            .iter()
            .flat_map(|event| {
                let parsed: responses::ResponsesChunkResponse =
                    serde_json::from_str(&event.data).expect("valid responses fixture");
                responses::responses_chunk_to_unified_stream_events(parsed)
            })
            .collect();
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
        let replayed = responses::transform_unified_stream_events_to_responses_events(
            unified_events,
            &mut transformer,
        )
        .expect("responses replay");
        let target_semantics = target_fixture_to_semantics(LlmApiType::Responses, &replayed);

        assert_eq!(source_semantics, target_semantics);
        assert_eq!(
            target_semantics.reasoning,
            "Considering the weather source."
        );
        assert_eq!(target_semantics.text, "Let me check.");
    }

    #[test]
    fn test_semantic_replay_gemini_function_call_stream_to_openai() {
        let fixture = load_sse_fixture(include_str!("testdata/gemini_function_call_stream.json"));

        let source_semantics = source_fixture_to_semantics(LlmApiType::Gemini, &fixture);
        let transformed =
            replay_fixture_through_transformer(LlmApiType::Gemini, LlmApiType::Openai, &fixture);
        let target_semantics = target_fixture_to_semantics(LlmApiType::Openai, &transformed);

        assert_eq!(source_semantics.text, target_semantics.text);
        assert_eq!(source_semantics.tool_calls, target_semantics.tool_calls);
        assert_eq!(
            source_semantics.finish_reason,
            target_semantics.finish_reason
        );
        assert_eq!(source_semantics.usage, target_semantics.usage);
    }

    #[test]
    fn test_semantic_replay_responses_to_openai_marks_reasoning_as_lossy_but_keeps_tool_call() {
        let fixture = load_sse_fixture(include_str!(
            "testdata/responses_reasoning_function_call.json"
        ));

        let source_semantics = source_fixture_to_semantics(LlmApiType::Responses, &fixture);
        let transformed =
            replay_fixture_through_transformer(LlmApiType::Responses, LlmApiType::Openai, &fixture);
        let target_semantics = target_fixture_to_semantics(LlmApiType::Openai, &transformed);

        assert_eq!(source_semantics.text, target_semantics.text);
        assert_eq!(source_semantics.tool_calls, target_semantics.tool_calls);
        assert_eq!(
            source_semantics.finish_reason,
            target_semantics.finish_reason
        );
        assert_eq!(source_semantics.usage, target_semantics.usage);
        assert_eq!(target_semantics.reasoning, "");
        assert!(!source_semantics.reasoning.is_empty());
    }

    #[test]
    fn test_semantic_replay_openai_tool_stream_to_responses() {
        let fixture = load_sse_fixture(include_str!("testdata/openai_tool_stream.json"));

        let source_semantics = source_fixture_to_semantics(LlmApiType::Openai, &fixture);
        let transformed =
            replay_fixture_through_transformer(LlmApiType::Openai, LlmApiType::Responses, &fixture);
        let target_semantics = target_fixture_to_semantics(LlmApiType::Responses, &transformed);

        assert_eq!(source_semantics.text, target_semantics.text);
        assert_eq!(source_semantics.tool_calls, target_semantics.tool_calls);
        assert_eq!(
            source_semantics.finish_reason,
            target_semantics.finish_reason
        );
        assert_eq!(source_semantics.usage, target_semantics.usage);
        assert!(target_semantics.stream_id.is_some());
        assert_eq!(target_semantics.model.as_deref(), Some("gpt-4.1"));
    }

    #[test]
    fn test_semantic_replay_gemini_multimodal_tool_stream_to_responses_preserves_binary_payloads() {
        let fixture = load_sse_fixture(include_str!("testdata/gemini_multimodal_tool_stream.json"));

        let source_semantics = source_fixture_to_semantics(LlmApiType::Gemini, &fixture);
        let transformed =
            replay_fixture_through_transformer(LlmApiType::Gemini, LlmApiType::Responses, &fixture);
        let target_semantics = target_fixture_to_semantics(LlmApiType::Responses, &transformed);

        assert_eq!(source_semantics.text, target_semantics.text);
        assert_eq!(source_semantics.tool_calls, target_semantics.tool_calls);
        assert_eq!(
            source_semantics.binary_payload_count,
            target_semantics.binary_payload_count
        );
        assert_eq!(source_semantics.usage, target_semantics.usage);
    }

    #[test]
    fn test_stream_failure_fixture_anthropic_unsupported_thinking_yields_controlled_error() {
        let fixture = load_sse_fixture(include_str!(
            "testdata/anthropic_unsupported_thinking_stream.json"
        ));

        let transformed = replay_fixture_through_transformer(
            LlmApiType::Anthropic,
            LlmApiType::Responses,
            &fixture,
        );

        assert_eq!(transformed.len(), 1);
        assert_eq!(transformed[0].event.as_deref(), Some("error"));
        let payload: Value = serde_json::from_str(&transformed[0].data).expect("error payload");
        assert_eq!(
            payload.get("type").and_then(Value::as_str),
            Some("transform_error")
        );
        assert_eq!(
            payload.get("stage").and_then(Value::as_str),
            Some("deserialize_source_chunk")
        );
        assert!(payload.get("raw_data_summary").is_some());
    }

    #[test]
    fn test_replay_regression_suite_summary_covers_stage2_samples() {
        let cases = [
            ReplayFixtureCase {
                fixture_name: "anthropic_tool_use_json_delta",
                source_api: LlmApiType::Anthropic,
                target_api: LlmApiType::Responses,
                fixture_json: include_str!("testdata/anthropic_tool_use_json_delta.json"),
            },
            ReplayFixtureCase {
                fixture_name: "responses_reasoning_function_call",
                source_api: LlmApiType::Responses,
                target_api: LlmApiType::Openai,
                fixture_json: include_str!("testdata/responses_reasoning_function_call.json"),
            },
            ReplayFixtureCase {
                fixture_name: "gemini_function_call_stream",
                source_api: LlmApiType::Gemini,
                target_api: LlmApiType::Openai,
                fixture_json: include_str!("testdata/gemini_function_call_stream.json"),
            },
            ReplayFixtureCase {
                fixture_name: "openai_tool_stream",
                source_api: LlmApiType::Openai,
                target_api: LlmApiType::Responses,
                fixture_json: include_str!("testdata/openai_tool_stream.json"),
            },
            ReplayFixtureCase {
                fixture_name: "gemini_multimodal_tool_stream",
                source_api: LlmApiType::Gemini,
                target_api: LlmApiType::Responses,
                fixture_json: include_str!("testdata/gemini_multimodal_tool_stream.json"),
            },
        ];

        let reports: Vec<_> = cases
            .into_iter()
            .map(build_replay_regression_report)
            .collect();

        assert_eq!(reports.len(), 5);
        assert!(
            reports
                .iter()
                .all(|report| report.transformed_frame_count > 0)
        );
        assert_eq!(
            reports
                .iter()
                .filter(|report| report.preserved_text)
                .count(),
            5
        );
        assert_eq!(
            reports
                .iter()
                .filter(|report| report.preserved_tool_calls)
                .count(),
            5
        );
        assert_eq!(
            reports
                .iter()
                .filter(|report| report.preserved_usage)
                .count(),
            5
        );
        assert_eq!(
            reports
                .iter()
                .filter(|report| report.preserved_reasoning)
                .count(),
            4
        );
        assert_eq!(
            reports
                .iter()
                .filter(|report| report.preserved_binary_payloads)
                .count(),
            5
        );
        assert!(reports.iter().any(|report| report.fixture_name
            == "gemini_multimodal_tool_stream"
            && report.target.binary_payload_count == 1));
    }

    #[test]
    fn test_capability_matrix_reports_expected_responses_capabilities() {
        let caps = ProtocolCapabilityMatrix::for_api(LlmApiType::Responses);

        assert!(caps.request.tool_definitions);
        assert!(caps.request.image_inline_input);
        assert!(caps.request.file_inline_input);
        assert!(caps.response.reasoning_content);
        assert!(caps.response.refusal);
        assert!(caps.response.citations);
        assert!(caps.response.file_output);
        assert!(caps.stream.tool_call_deltas);
        assert!(caps.stream.reasoning_deltas);
        assert!(caps.stream.reasoning_summary_parts);
        assert!(caps.stream.blob_deltas);
        assert!(caps.stream.structured_errors);
        assert!(caps.structured_content.images);
        assert!(caps.structured_content.tool_results);
        assert!(caps.structured_content.refusal);
        assert!(caps.structured_content.citations);
        assert!(caps.structured_content.file_references);
        assert!(caps.structured_content.json_schema_strict);
    }

    #[test]
    fn test_capability_matrix_reports_expected_gemini_and_ollama_boundaries() {
        let gemini = ProtocolCapabilityMatrix::for_api(LlmApiType::Gemini);
        let ollama = ProtocolCapabilityMatrix::for_api(LlmApiType::Ollama);

        assert!(gemini.request.image_url_input);
        assert!(gemini.request.image_inline_input);
        assert!(!gemini.request.file_inline_input);
        assert!(!gemini.response.refusal);
        assert!(gemini.response.citations);
        assert!(!gemini.stream.reasoning_summary_parts);
        assert!(gemini.structured_content.citations);

        assert!(!ollama.request.tool_definitions);
        assert!(!ollama.request.image_inline_input);
        assert!(!ollama.response.refusal);
        assert!(!ollama.stream.tool_call_deltas);
        assert!(!ollama.stream.reasoning_deltas);
        assert!(!ollama.structured_content.json_schema_strict);
    }

    #[test]
    fn test_policy_engine_uses_capability_matrix_for_ollama_tool_streaming() {
        let decision = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Ollama),
            TransformValueKind::ToolCallDelta,
        );

        assert_eq!(decision.level, TransformLossLevel::LossyMajor);
        assert_eq!(decision.action, TransformAction::Drop);
        assert_eq!(
            decision.reason,
            "The target stream capability matrix marks tool call deltas as unsupported."
        );
    }

    #[test]
    fn test_policy_engine_uses_capability_matrix_for_responses_reasoning_stream() {
        let decision = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Responses),
            TransformValueKind::ReasoningDelta,
        );

        assert_eq!(decision.level, TransformLossLevel::Lossless);
        assert_eq!(decision.action, TransformAction::Send);
    }

    #[test]
    fn test_policy_engine_uses_capability_matrix_for_response_refusal_and_request_top_k() {
        let refusal = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Gemini),
            TransformValueKind::Refusal,
        );
        assert_eq!(refusal.level, TransformLossLevel::LossyMajor);
        assert_eq!(refusal.action, TransformAction::Drop);
        assert_eq!(
            refusal.reason,
            "The target capability matrix marks refusal content as unsupported."
        );

        let top_k = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Responses),
            TransformValueKind::TopKParameter,
        );
        assert_eq!(top_k.level, TransformLossLevel::LossyMinor);
        assert_eq!(top_k.action, TransformAction::Drop);
        assert_eq!(
            top_k.reason,
            "The target request capability matrix marks top_k as unsupported."
        );
    }

    #[test]
    fn test_policy_engine_uses_capability_matrix_for_reasoning_and_structured_stream_errors() {
        let reasoning = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Openai),
            TransformValueKind::ReasoningContent,
        );
        assert_eq!(reasoning.level, TransformLossLevel::LossyMajor);
        assert_eq!(reasoning.action, TransformAction::Drop);
        assert_eq!(
            reasoning.reason,
            "The target response capability matrix marks reasoning content as unsupported."
        );

        let stream_error = PolicyEngine::evaluate(
            TransformProtocol::Unified,
            TransformProtocol::Api(LlmApiType::Gemini),
            TransformValueKind::StreamError,
        );
        assert_eq!(stream_error.level, TransformLossLevel::LossyMajor);
        assert_eq!(stream_error.action, TransformAction::Drop);
        assert_eq!(
            stream_error.reason,
            "The target stream capability matrix marks structured stream errors as unsupported."
        );
    }

    #[test]
    fn test_transform_request_data_openai_to_gemini_basic() {
        let openai_request = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "What is the weather in Boston?"}
            ],
            "temperature": 0.5,
            "max_tokens": 100,
            "top_p": 0.9,
            "stop": "stop_word"
        });

        let transformed = transform_request_data(
            openai_request,
            LlmApiType::Openai,
            LlmApiType::Gemini,
            false,
        );

        let expected_gemini_request = json!({
            "system_instruction": {
                "parts": [{"text": "You are a helpful assistant."}]
            },
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What is the weather in Boston?"}]
                }
            ],
            "generationConfig": {
                "temperature": 0.5,
                "maxOutputTokens": 100,
                "topP": 0.9,
                "stopSequences": ["stop_word"]
            }
        });

        assert_eq!(transformed, expected_gemini_request);
    }

    #[test]
    fn test_transform_request_data_openai_to_gemini_with_tools() {
        let openai_request = json!({
            "model": "gpt-4-turbo",
            "messages": [
                {"role": "user", "content": "What is the weather in Boston?"},
                {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "get_current_weather",
                                "arguments": "{\"location\": \"Boston, MA\"}"
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_123",
                    "name": "get_current_weather",
                    "content": "{\"temperature\": 22, \"unit\": \"celsius\"}"
                }
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_current_weather",
                        "description": "Get the current weather in a given location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {
                                    "type": "string",
                                    "description": "The city and state, e.g. San Francisco, CA"
                                }
                            },
                            "required": ["location"]
                        }
                    }
                }
            ]
        });

        let transformed = transform_request_data(
            openai_request,
            LlmApiType::Openai,
            LlmApiType::Gemini,
            false,
        );

        let expected_gemini_request = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What is the weather in Boston?"}]
                },
                {
                    "role": "model",
                    "parts": [
                        {
                            "functionCall": {
                                "name": "get_current_weather",
                                "args": {
                                    "location": "Boston, MA"
                                }
                            }
                        }
                    ]
                },
                {
                    "role": "user", // Gemini uses 'user' role for function responses
                    "parts": [
                        {
                            "functionResponse": {
                                "name": "get_current_weather",
                                "response": {
                                    "temperature": 22,
                                    "unit": "celsius"
                                }
                            }
                        }
                    ]
                }
            ],
            "tools": [
                {
                    "functionDeclarations": [
                        {
                            "name": "get_current_weather",
                            "description": "Get the current weather in a given location",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "location": {
                                        "type": "string",
                                        "description": "The city and state, e.g. San Francisco, CA"
                                    }
                                },
                                "required": ["location"]
                            }
                        }
                    ]
                }
            ]
        });

        assert_eq!(transformed, expected_gemini_request);
    }

    #[test]
    fn test_transform_request_data_gemini_to_openai_basic() {
        let gemini_request = json!({
            "system_instruction": {
                "parts": [{"text": "You are a helpful assistant."}]
            },
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What is the weather in Boston?"}]
                }
            ],
            "generationConfig": {
                "temperature": 0.5,
                "maxOutputTokens": 100,
                "topP": 0.9,
                "stopSequences": ["stop_word"]
            }
        });

        let transformed = transform_request_data(
            gemini_request,
            LlmApiType::Gemini,
            LlmApiType::Openai,
            true, // is_stream
        );

        let expected_openai_request = json!({
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "What is the weather in Boston?"}
            ],
            "temperature": 0.5,
            "max_tokens": 100,
            "top_p": 0.9,
            "stop": "stop_word",
            "stream": true
        });

        assert_eq!(transformed, expected_openai_request);
    }

    #[test]
    fn test_transform_request_data_gemini_to_openai_with_tools() {
        let gemini_request = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What is the weather in Boston?"}]
                },
                {
                    "role": "model",
                    "parts": [
                        {
                            "functionCall": {
                                "name": "get_current_weather",
                                "args": { "location": "Boston, MA" }
                            }
                        }
                    ]
                },
                {
                    "role": "user", // Gemini expects tool responses to have 'user' role
                    "parts": [
                        {
                            "functionResponse": {
                                "name": "get_current_weather",
                                "response": {
                                    "result": "{\"temperature\": 22, \"unit\": \"celsius\"}"
                                }
                            }
                        }
                    ]
                }
            ],
            "tools": [
                {
                    "functionDeclarations": [
                        {
                            "name": "get_current_weather",
                            "description": "Get the current weather in a given location",
                            "parameters": {
                                "type": "OBJECT",
                                "properties": {
                                    "location": {
                                        "type": "STRING"
                                    }
                                }
                            }
                        }
                    ]
                }
            ]
        });

        let transformed = transform_request_data(
            gemini_request,
            LlmApiType::Gemini,
            LlmApiType::Openai,
            false,
        );

        let mut transformed_obj = transformed.as_object().unwrap().clone();
        let messages = transformed_obj
            .get_mut("messages")
            .unwrap()
            .as_array_mut()
            .unwrap();

        let generated_id;

        // Scope the first mutable borrow to find the assistant message, check the generated ID,
        // and replace it with a fixed value for the final assertion.
        {
            let assistant_message = messages
                .iter_mut()
                .find(|m| m["role"] == "assistant")
                .unwrap();
            let tool_calls = assistant_message
                .get_mut("tool_calls")
                .unwrap()
                .as_array_mut()
                .unwrap();
            let tool_call = tool_calls.get_mut(0).unwrap().as_object_mut().unwrap();
            generated_id = tool_call.get("id").unwrap().as_str().unwrap().to_string();
            assert!(generated_id.starts_with("call_"));
            tool_call.insert("id".to_string(), json!("FIXED_ID_FOR_TEST"));
        }

        // Scope the second mutable borrow to find the tool message, check its ID,
        // and replace it with a fixed value.
        {
            let tool_message = messages
                .iter_mut()
                .find(|m| m["role"] == "tool")
                .unwrap()
                .as_object_mut()
                .unwrap();
            let tool_message_id = tool_message.get("tool_call_id").unwrap().as_str().unwrap();
            assert_eq!(generated_id, tool_message_id);
            tool_message.insert("tool_call_id".to_string(), json!("FIXED_ID_FOR_TEST"));
        }

        let transformed_back_to_value = serde_json::to_value(transformed_obj).unwrap();

        let expected_openai_request = json!({
            "messages": [
                {"role": "user", "content": "What is the weather in Boston?"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "FIXED_ID_FOR_TEST",
                            "type": "function",
                            "function": {
                                "name": "get_current_weather",
                                "arguments": "{\"location\":\"Boston, MA\"}"
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "FIXED_ID_FOR_TEST",
                    "name": "get_current_weather",
                    "content": "{\"temperature\": 22, \"unit\": \"celsius\"}"
                }
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_current_weather",
                        "description": "Get the current weather in a given location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {
                                    "type": "string"
                                }
                            }
                        }
                    }
                }
            ],
            "stream": false
        });

        assert_eq!(transformed_back_to_value, expected_openai_request);
    }

    #[test]
    fn test_transform_request_data_openai_to_gemini_preserves_image_url_as_recoverable_text() {
        let openai_request = json!({
            "model": "gpt-4",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "describe this"},
                        {"type": "image_url", "image_url": {"url": "https://example.com/cat.png"}}
                    ]
                }
            ]
        });

        let transformed = transform_request_data(
            openai_request,
            LlmApiType::Openai,
            LlmApiType::Gemini,
            false,
        );

        assert_eq!(
            transformed,
            json!({
                "contents": [
                    {
                        "role": "user",
                        "parts": [
                            {"text": "describe this"},
                            {"text": "image_url: https://example.com/cat.png"}
                        ]
                    }
                ]
            })
        );
        assert!(!transformed.to_string().contains("[Image:"));
    }

    #[test]
    fn test_transform_result_chunk_openai_to_gemini() {
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);

        // Test case 1: Content chunk
        let openai_data_content = "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}";
        let event = SseEvent {
            data: openai_data_content.to_string(),
            ..Default::default()
        };
        let transformed_events = transformer.transform_event(event).unwrap();
        assert_eq!(transformed_events.len(), 1);
        let transformed_json: Value = serde_json::from_str(&transformed_events[0].data).unwrap();

        let expected_json = json!({
            "candidates": [{
                "index": 0,
                "content": {
                    "parts": [{"text": "Hello"}],
                    "role": "model"
                }
            }]
        });
        assert_eq!(transformed_json, expected_json);

        // Test case 2: Finish reason chunk
        let openai_data_finish = "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}";
        let event_finish = SseEvent {
            data: openai_data_finish.to_string(),
            ..Default::default()
        };
        let transformed_events_finish = transformer.transform_event(event_finish).unwrap();
        assert_eq!(transformed_events_finish.len(), 1);
        let transformed_json_finish: Value =
            serde_json::from_str(&transformed_events_finish[0].data).unwrap();

        assert_eq!(
            transformed_json_finish["candidates"][0]["finishReason"],
            "STOP"
        );
        assert!(transformed_json_finish["candidates"][0]["safetyRatings"].is_null());

        // Test case 3: DONE chunk
        let openai_data_done = "[DONE]";
        let event_done = SseEvent {
            data: openai_data_done.to_string(),
            ..Default::default()
        };
        let transformed_done = transformer.transform_event(event_done);
        assert!(transformed_done.is_none());

        // Test case 4: Tool call chunk
        let openai_data_tool = "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_123\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"location\\\": \\\"Boston\\\"}\"}}]}}]}";
        let event_tool = SseEvent {
            data: openai_data_tool.to_string(),
            ..Default::default()
        };
        let transformed_events_tool = transformer.transform_event(event_tool).unwrap();
        assert_eq!(transformed_events_tool.len(), 1);
        let transformed_json_tool: Value =
            serde_json::from_str(&transformed_events_tool[0].data).unwrap();

        let expected_tool_json = json!({
            "candidates": [{
                "index": 0,
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "get_weather",
                            "args": {
                                "location": "Boston"
                            }
                        }
                    }]
                }
            }]
        });
        assert_eq!(transformed_json_tool, expected_tool_json);

        // Test case 5: Empty content chunk should be filtered out
        let openai_data_empty_content = "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"}}]}";
        let event_empty = SseEvent {
            data: openai_data_empty_content.to_string(),
            ..Default::default()
        };
        let transformed_empty_content = transformer.transform_event(event_empty);
        assert!(transformed_empty_content.is_none());
    }

    #[test]
    fn test_transform_result_chunk_gemini_to_openai() {
        let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);

        // Test case 1: Content chunk
        let gemini_data_content = "{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" World\"}],\"role\":\"model\"},\"index\":0}]}";
        let event = SseEvent {
            data: gemini_data_content.to_string(),
            ..Default::default()
        };
        let transformed_events = transformer.transform_event(event).unwrap();
        assert_eq!(transformed_events.len(), 1);
        let transformed_json: Value = serde_json::from_str(&transformed_events[0].data).unwrap();

        assert_eq!(transformed_json["choices"][0]["delta"]["content"], " World");
        assert_eq!(transformed_json["choices"][0]["index"], 0);
        assert_eq!(transformed_json["object"], "chat.completion.chunk");

        // Test case 2: Finish reason chunk
        let gemini_data_finish = "{\"candidates\":[{\"finishReason\":\"STOP\",\"index\":0}]}";
        let event_finish = SseEvent {
            data: gemini_data_finish.to_string(),
            ..Default::default()
        };
        let transformed_events_finish = transformer.transform_event(event_finish).unwrap();
        assert_eq!(transformed_events_finish.len(), 1);
        let transformed_json_finish: Value =
            serde_json::from_str(&transformed_events_finish[0].data).unwrap();

        assert_eq!(
            transformed_json_finish["choices"][0]["finish_reason"],
            "stop"
        );
        assert!(
            transformed_json_finish["choices"][0]["delta"]
                .as_object()
                .unwrap()
                .is_empty()
        );

        // Test case 3: Function call chunk
        let gemini_data_tool = "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"location\":\"Boston\"}}}]},\"index\":0}]}";
        let event_tool = SseEvent {
            data: gemini_data_tool.to_string(),
            ..Default::default()
        };
        let transformed_events_tool = transformer.transform_event(event_tool).unwrap();
        assert_eq!(transformed_events_tool.len(), 1);
        let mut transformed_json_tool: Value =
            serde_json::from_str(&transformed_events_tool[0].data).unwrap();

        // The ID is generated, so we need to extract it and then compare
        let tool_call = transformed_json_tool["choices"][0]["delta"]["tool_calls"][0]
            .as_object_mut()
            .unwrap();
        let id = tool_call.get("id").unwrap().as_str().unwrap().to_string();
        tool_call.insert("id".to_string(), json!("FIXED_ID_FOR_TEST"));

        assert!(id.starts_with("gemini-call-"));
        let mut tool_call_delta = serde_json::Map::new();
        if let Some(role) = transformed_json_tool["choices"][0]["delta"].get("role") {
            tool_call_delta.insert("role".to_string(), role.clone());
        }
        if let Some(tcs) = transformed_json_tool["choices"][0]["delta"].get("tool_calls") {
            tool_call_delta.insert("tool_calls".to_string(), tcs.clone());
        }

        let expected_tool_json = json!({
            "id": transformed_json_tool["id"].clone(),
            "object": "chat.completion.chunk",
            "created": transformed_json_tool["created"].clone(),
            "model": "",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": "FIXED_ID_FOR_TEST",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\":\"Boston\"}"
                        }
                    }]
                }
            }]
        });
        assert_eq!(transformed_json_tool, expected_tool_json);
    }

    #[test]
    fn test_stream_transformer_session_keeps_gemini_tool_call_ids_stable() {
        let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);
        let gemini_data_tool = "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"location\":\"Boston\"}}}]},\"index\":0}]}";

        let first = transformer
            .transform_event(SseEvent {
                data: gemini_data_tool.to_string(),
                ..Default::default()
            })
            .unwrap();
        let second = transformer
            .transform_event(SseEvent {
                data: gemini_data_tool.to_string(),
                ..Default::default()
            })
            .unwrap();

        let first_json: Value = serde_json::from_str(&first[0].data).unwrap();
        let second_json: Value = serde_json::from_str(&second[0].data).unwrap();

        assert_eq!(
            first_json["choices"][0]["delta"]["tool_calls"][0]["id"],
            second_json["choices"][0]["delta"]["tool_calls"][0]["id"]
        );
    }

    #[test]
    fn test_stream_transformer_session_advances_gemini_tool_call_ids_after_finish() {
        let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);
        let gemini_data_tool = "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"location\":\"Boston\"}}}]},\"index\":0}]}";
        let gemini_finish = "{\"candidates\":[{\"index\":0,\"finishReason\":\"STOP\"}]}";

        let first = transformer
            .transform_event(SseEvent {
                data: gemini_data_tool.to_string(),
                ..Default::default()
            })
            .unwrap();
        transformer
            .transform_event(SseEvent {
                data: gemini_finish.to_string(),
                ..Default::default()
            })
            .unwrap();
        let second = transformer
            .transform_event(SseEvent {
                data: gemini_data_tool.to_string(),
                ..Default::default()
            })
            .unwrap();

        let first_json: Value = serde_json::from_str(&first[0].data).unwrap();
        let second_json: Value = serde_json::from_str(&second[0].data).unwrap();

        assert_ne!(
            first_json["choices"][0]["delta"]["tool_calls"][0]["id"],
            second_json["choices"][0]["delta"]["tool_calls"][0]["id"]
        );
    }

    #[test]
    fn test_transform_result_openai_to_gemini_basic() {
        let openai_result = json!({
          "id": "chatcmpl-123",
          "object": "chat.completion",
          "created": 1677652288,
          "model": "gpt-3.5-turbo-0125",
          "choices": [{
            "index": 0,
            "message": {
              "role": "assistant",
              "content": "Hello there! How can I help you today?"
            },
            "finish_reason": "stop"
          }],
          "usage": {
            "prompt_tokens": 9,
            "completion_tokens": 12,
            "total_tokens": 21
          }
        });

        let (transformed, usage_info) =
            transform_result(openai_result, LlmApiType::Openai, LlmApiType::Gemini);

        let expected_gemini_result = json!({
          "candidates": [
            {
              "index": 0,
              "content": {
                "parts": [
                  {
                    "text": "Hello there! How can I help you today?"
                  }
                ],
                "role": "model"
              },
              "finishReason": "STOP"
            }
          ],
          "usageMetadata": {
            "promptTokenCount": 9,
            "candidatesTokenCount": 12,
            "totalTokenCount": 21,
            "promptTokensDetails": [{"modality": "TEXT", "tokenCount": 9}],
            "candidatesTokensDetails": [{"modality": "TEXT", "tokenCount": 12}]
          }
        });

        assert_eq!(transformed, expected_gemini_result);
        assert_eq!(
            usage_info,
            Some(UsageInfo {
                input_tokens: 9,
                output_tokens: 12,
                total_tokens: 21,
                ..Default::default()
            })
        );
    }

    #[test]
    fn test_transform_result_gemini_to_openai_basic() {
        let gemini_result = json!({
          "candidates": [
            {
              "index": 0,
              "content": {
                "parts": [
                  {
                    "text": "This is a test response from Gemini."
                  }
                ],
                "role": "model"
              },
              "finishReason": "STOP",
              "safetyRatings": [
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE" }
              ]
            }
          ],
          "usageMetadata": {
            "promptTokenCount": 10,
            "candidatesTokenCount": 8,
            "totalTokenCount": 18
          }
        });

        let (transformed, usage_info) =
            transform_result(gemini_result, LlmApiType::Gemini, LlmApiType::Openai);

        let mut transformed_obj = transformed.as_object().unwrap().clone();
        assert!(
            transformed_obj
                .get("id")
                .unwrap()
                .as_str()
                .unwrap()
                .starts_with("gemini-response-")
        );
        assert!(transformed_obj.get("created").unwrap().is_number());
        transformed_obj.remove("id");
        transformed_obj.remove("created");

        let expected_openai_result = json!({
          "object": "chat.completion",
          "model": "",
          "choices": [
            {
              "index": 0,
              "message": {
                "role": "assistant",
                "content": "This is a test response from Gemini."
              },
              "finish_reason": "stop"
            }
          ],
          "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 8,
            "total_tokens": 18
          }
        });

        assert_eq!(
            serde_json::to_value(transformed_obj).unwrap(),
            expected_openai_result
        );
        assert_eq!(
            usage_info,
            Some(UsageInfo {
                input_tokens: 10,
                output_tokens: 8,
                total_tokens: 18,
                ..Default::default()
            })
        );
    }

    #[test]
    fn test_transform_result_openai_to_gemini_with_tools() {
        let openai_result = json!({
          "id": "chatcmpl-123",
          "object": "chat.completion",
          "created": 1677652288,
          "model": "gpt-3.5-turbo-0125",
          "choices": [{
            "index": 0,
            "message": {
              "role": "assistant",
              "content": null,
              "tool_calls": [
                {
                  "id": "call_abc",
                  "type": "function",
                  "function": {
                    "name": "get_current_weather",
                    "arguments": "{\"location\":\"Boston, MA\"}"
                  }
                }
              ]
            },
            "finish_reason": "tool_calls"
          }],
          "usage": {
            "prompt_tokens": 9,
            "completion_tokens": 12,
            "total_tokens": 21
          }
        });

        let (transformed, usage_info) =
            transform_result(openai_result, LlmApiType::Openai, LlmApiType::Gemini);

        let expected_gemini_result = json!({
          "candidates": [
            {
              "index": 0,
              "content": {
                "parts": [
                  {
                    "functionCall": {
                      "name": "get_current_weather",
                      "args": {
                        "location": "Boston, MA"
                      }
                    }
                  }
                ],
                "role": "model"
              },
              "finishReason": "TOOL_USE"
            }
          ],
          "usageMetadata": {
            "promptTokenCount": 9,
            "candidatesTokenCount": 12,
            "totalTokenCount": 21,
            "promptTokensDetails": [{"modality": "TEXT", "tokenCount": 9}],
            "candidatesTokensDetails": [{"modality": "TEXT", "tokenCount": 12}]
          }
        });

        assert_eq!(transformed, expected_gemini_result);
        assert_eq!(
            usage_info,
            Some(UsageInfo {
                input_tokens: 9,
                output_tokens: 12,
                total_tokens: 21,
                ..Default::default()
            })
        );
    }

    #[test]
    fn test_transform_result_gemini_to_openai_with_tools() {
        let gemini_result = json!({
          "candidates": [
            {
              "content": {
                "parts": [
                  {
                    "functionCall": {
                      "name": "get_current_weather",
                      "args": {
                        "location": "Boston, MA"
                      }
                    }
                  }
                ],
                "role": "model"
              },
              "finishReason": "TOOL_USE",
              "index": 0
            }
          ]
        });

        let (transformed, usage_info) =
            transform_result(gemini_result, LlmApiType::Gemini, LlmApiType::Openai);

        let mut transformed_obj = transformed.as_object().unwrap().clone();
        transformed_obj.remove("id");
        transformed_obj.remove("created");

        let choices = transformed_obj
            .get_mut("choices")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let message = choices[0]
            .get_mut("message")
            .unwrap()
            .as_object_mut()
            .unwrap();
        let tool_calls = message
            .get_mut("tool_calls")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let tool_call = tool_calls[0].as_object_mut().unwrap();
        assert!(
            tool_call
                .get("id")
                .unwrap()
                .as_str()
                .unwrap()
                .starts_with("gemini-call-")
        );
        tool_call.insert("id".to_string(), json!("FIXED_ID_FOR_TEST"));

        let expected_openai_result = json!({
          "object": "chat.completion",
          "model": "",
          "choices": [
            {
              "index": 0,
              "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                  {
                    "id": "FIXED_ID_FOR_TEST",
                    "type": "function",
                    "function": {
                      "name": "get_current_weather",
                      "arguments": "{\"location\":\"Boston, MA\"}"
                    }
                  }
                ]
              },
              "finish_reason": "tool_calls"
            }
          ]
        });

        assert_eq!(
            serde_json::to_value(transformed_obj).unwrap(),
            expected_openai_result
        );
        assert!(usage_info.is_none());
    }

    #[test]
    fn test_transform_result_gemini_to_openai_with_tools_and_stop_reason() {
        let gemini_result = json!({
          "candidates": [
            {
              "content": {
                "parts": [
                  {
                    "functionCall": {
                      "name": "get_current_weather",
                      "args": {
                        "location": "Boston, MA"
                      }
                    }
                  }
                ],
                "role": "model"
              },
              "finishReason": "STOP", // Key difference: STOP instead of TOOL_USE
              "index": 0
            }
          ]
        });

        let (transformed, usage_info) =
            transform_result(gemini_result, LlmApiType::Gemini, LlmApiType::Openai);

        let mut transformed_obj = transformed.as_object().unwrap().clone();
        transformed_obj.remove("id");
        transformed_obj.remove("created");

        let choices = transformed_obj
            .get_mut("choices")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let message = choices[0]
            .get_mut("message")
            .unwrap()
            .as_object_mut()
            .unwrap();
        let tool_calls = message
            .get_mut("tool_calls")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let tool_call = tool_calls[0].as_object_mut().unwrap();
        assert!(
            tool_call
                .get("id")
                .unwrap()
                .as_str()
                .unwrap()
                .starts_with("gemini-call-")
        );
        tool_call.insert("id".to_string(), json!("FIXED_ID_FOR_TEST"));

        let expected_openai_result = json!({
          "object": "chat.completion",
          "model": "",
          "choices": [
            {
              "index": 0,
              "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                  {
                    "id": "FIXED_ID_FOR_TEST",
                    "type": "function",
                    "function": {
                      "name": "get_current_weather",
                      "arguments": "{\"location\":\"Boston, MA\"}"
                    }
                  }
                ]
              },
              "finish_reason": "tool_calls" // Should be tool_calls because a tool was called
            }
          ]
        });

        assert_eq!(
            serde_json::to_value(transformed_obj).unwrap(),
            expected_openai_result
        );
        assert!(usage_info.is_none());
    }

    #[test]
    fn test_transform_request_data_no_op() {
        let openai_request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let transformed = transform_request_data(
            openai_request.clone(),
            LlmApiType::Openai,
            LlmApiType::Openai,
            false,
        );

        assert_eq!(openai_request, transformed);
    }

    #[test]
    fn test_transform_request_data_responses_to_openai_preserves_shorthand_input_message() {
        let responses_request = json!({
            "model": "gemini/gemini-2.5-flash-lite",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "你好"
                        }
                    ]
                }
            ],
            "stream": true
        });

        let transformed = transform_request_data(
            responses_request,
            LlmApiType::Responses,
            LlmApiType::Openai,
            true,
        );

        assert_eq!(transformed["messages"][0]["role"], json!("user"));
        assert_eq!(transformed["messages"][0]["content"], json!("你好"));
        assert_eq!(transformed["stream"], json!(true));
    }

    #[test]
    fn test_transform_request_data_responses_to_gemini_preserves_shorthand_input_message() {
        let responses_request = json!({
            "model": "gemini/gemini-2.5-flash-lite",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "你好"
                        }
                    ]
                }
            ],
            "include": ["reasoning.encrypted_content"]
        });

        let transformed = transform_request_data(
            responses_request,
            LlmApiType::Responses,
            LlmApiType::Gemini,
            false,
        );

        assert_eq!(transformed["contents"][0]["role"], json!("user"));
        assert_eq!(
            transformed["contents"][0]["parts"][0]["text"],
            json!("你好")
        );
    }

    #[test]
    fn test_finalize_request_data_for_vertex_openai_applies_gemini_variant_policy() {
        let data = json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true,
            "stream_options": {"include_usage": false},
            "parallel_tool_calls": true,
            "user": "user-123"
        });

        let finalized = finalize_request_data(
            data,
            LlmApiType::Openai,
            &ProviderType::VertexOpenai,
            "chat/completions",
        );

        assert_eq!(
            finalized,
            json!({
                "model": "gemini-2.5-pro",
                "messages": [{"role": "user", "content": "hello"}],
                "stream": true
            })
        );
    }

    #[test]
    fn test_finalize_request_data_for_standard_openai_keeps_stream_options() {
        let data = json!({
            "model": "gpt-4.1",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true
        });

        let finalized = finalize_request_data(
            data,
            LlmApiType::Openai,
            &ProviderType::Openai,
            "chat/completions",
        );

        assert_eq!(
            finalized,
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "user", "content": "hello"}],
                "stream": true,
                "stream_options": {"include_usage": true}
            })
        );
    }

    #[test]
    fn test_transform_result_on_deserialization_error() {
        let malformed_openai_result = json!({
            "id": "chatcmpl-123",
            "choices": "this should be an array"
        });

        let (transformed, usage_info) = transform_result(
            malformed_openai_result.clone(),
            LlmApiType::Openai,
            LlmApiType::Gemini,
        );

        // On error, the original data should be returned
        assert_eq!(transformed, malformed_openai_result);
        assert!(usage_info.is_none());
    }

    #[test]
    fn test_stream_transformer_session_keeps_bounded_diagnostic_windows() {
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);

        for index in 0..40 {
            let event = SseEvent {
                data: format!(
                    "{{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{}\"}}}}]}}",
                    index
                ),
                ..Default::default()
            };
            let _ = transformer.transform_event(event);
        }

        assert_eq!(
            transformer.session.original_events.len(),
            STREAM_DIAGNOSTIC_WINDOW
        );
        assert_eq!(
            transformer.session.transformed_events.len(),
            STREAM_DIAGNOSTIC_WINDOW
        );
        assert!(
            transformer
                .session
                .original_events
                .front()
                .unwrap()
                .data
                .contains("\"8\"")
        );
    }

    #[test]
    fn test_stream_transformer_session_caches_usage_and_finish_reason() {
        let mut transformer = StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Openai);
        let event = SseEvent {
            data: json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": "end_turn",
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": 7,
                        "output_tokens": 11
                    }
                }
            })
            .to_string(),
            ..Default::default()
        };

        let transformed = transformer.transform_event(event).unwrap();

        assert_eq!(transformed.len(), 2);
        assert_eq!(
            transformer.session.finish_reason_cache,
            Some("stop".to_string())
        );
        assert_eq!(
            transformer.session.usage_cache,
            Some(UsageInfo {
                input_tokens: 7,
                output_tokens: 11,
                total_tokens: 18,
                ..Default::default()
            })
        );
        assert_eq!(
            transformer.parse_usage_info(),
            transformer.session.usage_cache.clone()
        );
    }

    #[test]
    fn test_anthropic_stream_event_bridge_matches_legacy_text_delta_output() {
        let raw_event = anthropic::AnthropicEvent::ContentBlockDelta {
            index: 0,
            delta: anthropic::AnthropicContentDelta::TextDelta {
                text: "Hello".to_string(),
            },
        };
        let legacy_chunk: UnifiedChunkResponse = raw_event.into();
        let legacy_openai =
            serde_json::to_value(openai::OpenAiChunkResponse::from(legacy_chunk)).unwrap();

        let mut transformer = StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Openai);
        let transformed = transformer
            .transform_event(SseEvent {
                data: json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "text_delta", "text": "Hello"}
                })
                .to_string(),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(transformed.len(), 1);
        let bridged_openai: Value = serde_json::from_str(&transformed[0].data).unwrap();
        assert_eq!(bridged_openai["choices"], legacy_openai["choices"]);
    }

    #[test]
    fn test_openai_native_stream_encoder_matches_legacy_bridge_for_supported_events() {
        let events = vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("chatcmpl-native".to_string()),
                model: Some("gpt-test".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "Hello".to_string(),
            },
            UnifiedStreamEvent::ToolCallStart {
                index: 0,
                id: "call_123".to_string(),
                name: "lookup".to_string(),
            },
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: None,
                item_id: None,
                id: Some("call_123".to_string()),
                name: Some("lookup".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("tool_calls".to_string()),
            },
            UnifiedStreamEvent::Usage {
                usage: UnifiedUsage {
                    input_tokens: 7,
                    output_tokens: 11,
                    total_tokens: 18,
                    ..Default::default()
                },
            },
        ];

        let mut native_transformer =
            StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Openai);
        native_transformer.update_session_from_stream_events(&events);
        let native = openai::transform_unified_stream_events_to_openai_events(
            events.clone(),
            &mut native_transformer,
        )
        .unwrap();

        let mut legacy_transformer =
            StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Openai);
        legacy_transformer.update_session_from_stream_events(&events);
        let legacy = legacy_transformer
            .bridge_stream_events_to_legacy_chunks(events)
            .into_iter()
            .map(|chunk| serde_json::to_value(openai::OpenAiChunkResponse::from(chunk)).unwrap())
            .collect::<Vec<_>>();

        let native_values = native
            .into_iter()
            .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(native_values.len(), legacy.len());
        for (native_value, legacy_value) in native_values.iter().zip(legacy.iter()) {
            assert_eq!(native_value["choices"], legacy_value["choices"]);
            assert_eq!(native_value["usage"], legacy_value["usage"]);
        }
    }

    #[test]
    fn test_gemini_native_stream_encoder_matches_legacy_bridge_for_supported_events() {
        let events = vec![
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "Hello".to_string(),
            },
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: None,
                item_id: None,
                id: Some("call_123".to_string()),
                name: Some("lookup".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("tool_calls".to_string()),
            },
        ];
        let usage_event = UnifiedStreamEvent::Usage {
            usage: UnifiedUsage {
                input_tokens: 7,
                output_tokens: 11,
                total_tokens: 18,
                ..Default::default()
            },
        };
        let mut native_transformer =
            StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Gemini);
        native_transformer.update_session_from_stream_events(&events);
        let native = gemini::transform_unified_stream_events_to_gemini_events(
            events.clone(),
            &mut native_transformer,
        )
        .unwrap();

        let usage_only = gemini::transform_unified_stream_events_to_gemini_events(
            vec![usage_event],
            &mut native_transformer,
        )
        .unwrap();

        let mut legacy_transformer =
            StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Gemini);
        legacy_transformer.update_session_from_stream_events(&events);
        let legacy = legacy_transformer
            .bridge_stream_events_to_legacy_chunks(events)
            .into_iter()
            .filter_map(|chunk| {
                let value = serde_json::to_value(gemini::GeminiChunkResponse::from(chunk)).unwrap();
                let has_candidates = value
                    .get("candidates")
                    .and_then(|c| c.as_array())
                    .is_some_and(|c| !c.is_empty());
                has_candidates.then_some(value)
            })
            .collect::<Vec<_>>();

        let native_values = native
            .into_iter()
            .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(native_values, legacy);
        assert_eq!(usage_only.len(), 1);
        let usage_value: Value = serde_json::from_str(&usage_only[0].data).unwrap();
        assert_eq!(usage_value["usageMetadata"]["promptTokenCount"], json!(7));
        assert_eq!(
            usage_value["usageMetadata"]["candidatesTokenCount"],
            json!(11)
        );
        assert_eq!(usage_value["usageMetadata"]["totalTokenCount"], json!(18));
    }

    #[test]
    fn test_ollama_native_stream_encoder_matches_legacy_bridge_for_supported_events() {
        let events = vec![
            UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "Hello".to_string(),
            },
            UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("stop".to_string()),
            },
            UnifiedStreamEvent::Usage {
                usage: UnifiedUsage {
                    input_tokens: 7,
                    output_tokens: 11,
                    total_tokens: 18,
                    ..Default::default()
                },
            },
        ];

        let mut native_transformer =
            StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Ollama);
        native_transformer.session.stream_model = Some("llama3".to_string());
        let native = ollama::transform_unified_stream_events_to_ollama_events(
            events.clone(),
            &mut native_transformer,
        )
        .unwrap();

        let mut legacy_transformer =
            StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Ollama);
        legacy_transformer.session.stream_model = Some("llama3".to_string());
        let legacy = legacy_transformer
            .bridge_stream_events_to_legacy_chunks(events)
            .into_iter()
            .map(|chunk| serde_json::to_value(ollama::OllamaChunkResponse::from(chunk)).unwrap())
            .collect::<Vec<_>>();

        let native_values = native
            .into_iter()
            .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(native_values.len(), legacy.len());
        for (native_value, legacy_value) in native_values.iter().zip(legacy.iter()) {
            assert_eq!(native_value["message"], legacy_value["message"]);
            assert_eq!(native_value["done"], legacy_value["done"]);
            assert_eq!(native_value["done_reason"], legacy_value["done_reason"]);
            assert_eq!(
                native_value["prompt_eval_count"],
                legacy_value["prompt_eval_count"]
            );
            assert_eq!(native_value["eval_count"], legacy_value["eval_count"]);
        }
    }

    #[test]
    fn test_openai_native_stream_encoder_emits_structured_diagnostics_for_reasoning_and_blob() {
        let events = vec![
            UnifiedStreamEvent::ReasoningDelta {
                index: 1,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "hidden chain".to_string(),
            },
            UnifiedStreamEvent::BlobDelta {
                index: Some(2),
                data: json!({"mime_type":"image/png","data":"aGVsbG8="}),
            },
        ];

        let mut transformer = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Openai);
        transformer.session.stream_id = Some("stream-openai".to_string());
        let encoded =
            openai::transform_unified_stream_events_to_openai_events(events, &mut transformer)
                .unwrap();

        assert_eq!(encoded.len(), 2);
        assert!(
            encoded
                .iter()
                .all(|event| event.event.as_deref() == Some("transform_diagnostic"))
        );
        let payloads = encoded
            .iter()
            .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(payloads[0]["target_provider"], json!("Openai"));
        assert_eq!(payloads[0]["semantic_unit"], json!("ReasoningDelta"));
        assert_eq!(payloads[0]["type"], json!("transform_diagnostic"));
        assert_eq!(payloads[0]["stream_id"], json!("stream-openai"));
        assert_eq!(payloads[1]["semantic_unit"], json!("BlobDelta"));
        assert_eq!(transformer.session.diagnostics.len(), 2);
    }

    #[test]
    fn test_gemini_native_stream_encoder_preserves_inline_blob_and_diagnostics_reasoning() {
        let events = vec![
            UnifiedStreamEvent::BlobDelta {
                index: Some(3),
                data: json!({"mime_type":"image/png","data":"aGVsbG8="}),
            },
            UnifiedStreamEvent::ReasoningDelta {
                index: 4,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "thinking".to_string(),
            },
        ];

        let mut transformer = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Gemini);
        let encoded =
            gemini::transform_unified_stream_events_to_gemini_events(events, &mut transformer)
                .unwrap();

        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[1].event.as_deref(), Some("transform_diagnostic"));

        let blob_payload: Value = serde_json::from_str(&encoded[0].data).unwrap();
        assert_eq!(
            blob_payload["candidates"][0]["content"]["parts"][0]["inlineData"]["mimeType"],
            json!("image/png")
        );
        assert_eq!(
            blob_payload["candidates"][0]["content"]["parts"][0]["inlineData"]["data"],
            json!("aGVsbG8=")
        );

        let diagnostic_payload: Value = serde_json::from_str(&encoded[1].data).unwrap();
        assert_eq!(diagnostic_payload["semantic_unit"], json!("ReasoningDelta"));
        assert_eq!(diagnostic_payload["target_provider"], json!("Gemini"));
        assert_eq!(transformer.session.diagnostics.len(), 1);
    }

    #[test]
    fn test_ollama_native_stream_encoder_emits_structured_diagnostics_for_tool_reasoning_and_blob()
    {
        let events = vec![
            UnifiedStreamEvent::ToolCallStart {
                index: 0,
                id: "call_1".to_string(),
                name: "lookup".to_string(),
            },
            UnifiedStreamEvent::ReasoningDelta {
                index: 1,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "thinking".to_string(),
            },
            UnifiedStreamEvent::BlobDelta {
                index: None,
                data: json!({"kind":"artifact"}),
            },
        ];

        let mut transformer = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Ollama);
        let encoded =
            ollama::transform_unified_stream_events_to_ollama_events(events, &mut transformer)
                .unwrap();

        assert_eq!(encoded.len(), 3);
        let payloads = encoded
            .iter()
            .map(|event| {
                assert_eq!(event.event.as_deref(), Some("transform_diagnostic"));
                serde_json::from_str::<Value>(&event.data).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(payloads[0]["semantic_unit"], json!("ToolCallDelta"));
        assert_eq!(payloads[1]["semantic_unit"], json!("ReasoningDelta"));
        assert_eq!(payloads[2]["semantic_unit"], json!("BlobDelta"));
        assert_eq!(payloads[2]["target_provider"], json!("Ollama"));
        assert_eq!(transformer.session.diagnostics.len(), 3);
    }

    #[test]
    fn test_openai_source_stream_native_path_matches_legacy_chunk_path_for_gemini_target() {
        let raw = json!({
            "id": "chatcmpl-src",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4.1",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": "Hello"
                }
            }]
        });

        let event = SseEvent {
            data: raw.to_string(),
            ..Default::default()
        };
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);
        let optimized = transformer.transform_event(event).unwrap();

        let mut legacy_transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);
        let mut legacy_chunk: UnifiedChunkResponse =
            serde_json::from_value::<openai::OpenAiChunkResponse>(raw)
                .unwrap()
                .into();
        legacy_chunk.id = legacy_transformer.get_or_generate_stream_id();
        legacy_transformer.normalize_unified_chunk_session_state(&mut legacy_chunk);
        let legacy = vec![SseEvent {
            data: serde_json::to_string(&gemini::GeminiChunkResponse::from(legacy_chunk)).unwrap(),
            ..Default::default()
        }];

        let optimized_value: Value = serde_json::from_str(&optimized[0].data).unwrap();
        let legacy_value: Value = serde_json::from_str(&legacy[0].data).unwrap();
        assert_eq!(optimized_value, legacy_value);
    }

    #[test]
    fn test_responses_source_stream_fast_path_matches_unified_openai_path() {
        let raw = json!({
            "id": "resp_123",
            "model": "gpt-4.1",
            "delta": {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_123",
                "name": "lookup_weather",
                "arguments": "{\"city\":\"Boston\"}",
                "status": "completed"
            }
        });

        let event = SseEvent {
            data: raw.to_string(),
            ..Default::default()
        };

        let mut optimized = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Openai);
        let optimized_events = optimized.transform_event(event.clone()).unwrap();

        let parsed: responses::ResponsesChunkResponse = serde_json::from_value(raw).unwrap();
        let stream_events = responses::responses_chunk_to_unified_stream_events(parsed);
        let mut legacy = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Openai);
        legacy.update_session_from_stream_events(&stream_events);
        let legacy_events =
            openai::transform_unified_stream_events_to_openai_events(stream_events, &mut legacy)
                .unwrap();

        let optimized_values = optimized_events
            .into_iter()
            .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
            .collect::<Vec<_>>();
        let legacy_values = legacy_events
            .into_iter()
            .map(|event| serde_json::from_str::<Value>(&event.data).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(optimized_values, legacy_values);
    }

    #[test]
    fn test_stream_transformer_deserialize_failure_returns_controlled_error_event() {
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);

        let transformed = transformer
            .transform_event(SseEvent {
                data: "{not-json}".to_string(),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(transformed.len(), 1);
        assert_eq!(transformed[0].event.as_deref(), Some("error"));
        let payload: Value = serde_json::from_str(&transformed[0].data).unwrap();
        assert_eq!(payload["type"], "transform_error");
        assert_eq!(payload["diagnostic_kind"], "fatal_transform_error");
        assert_eq!(payload["stage"], "deserialize_source_chunk");
        assert_eq!(payload["provider"], "Openai");
        assert_eq!(payload["target_provider"], "Gemini");
        assert_eq!(payload["loss_level"], "reject");
        assert_eq!(payload["semantic_unit"], "StreamError");
        assert!(payload["recovery_hint"].is_string());
        assert!(transformer.session.last_error.is_some());
        assert_eq!(transformer.session.diagnostics.len(), 1);
    }

    #[test]
    fn test_parse_usage_info_falls_back_to_diagnostic_window_when_cache_is_empty() {
        let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);
        transformer.session.push_original_event(SseEvent {
            data: json!({
                "candidates": [],
                "usageMetadata": {
                    "promptTokenCount": 3,
                    "candidatesTokenCount": 5,
                    "totalTokenCount": 8
                }
            })
            .to_string(),
            ..Default::default()
        });

        assert_eq!(
            transformer.parse_usage_info(),
            Some(UsageInfo {
                input_tokens: 3,
                output_tokens: 5,
                total_tokens: 8,
                ..Default::default()
            })
        );
    }

    #[test]
    fn test_parse_usage_info_records_diagnostic_on_cache_miss() {
        let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);

        assert!(transformer.parse_usage_info().is_none());
        assert_eq!(transformer.session.diagnostics.len(), 1);
        let diagnostic = transformer.session.diagnostics.back().unwrap();
        assert_eq!(diagnostic.type_, "transform_diagnostic");
        assert_eq!(
            diagnostic.diagnostic_kind,
            UnifiedTransformDiagnosticKind::CapabilityDowngrade
        );
        assert_eq!(diagnostic.stage.as_deref(), Some("parse_usage_info"));
        assert_eq!(
            diagnostic.loss_level,
            UnifiedTransformDiagnosticLossLevel::LossyMinor
        );
    }
}

pub fn transform_result(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> (Value, Option<UsageInfo>) {
    // Step 1: Deserialize to UnifiedResponse. This is now UNCONDITIONAL.
    // This allows us to get usage info from a typed struct.
    let source_adapter = adapter_for(api_type);
    let target_adapter = adapter_for(target_api_type);
    let unified_response_result = (source_adapter.response.decode)(data.clone());

    let unified_response = match unified_response_result {
        Ok(ur) => ur,
        Err(e) => {
            error!(
                "[transform_result] Failed to deserialize to UnifiedResponse from {:?}: {}. Returning original data.",
                source_adapter.api_type, e
            );
            return (data, None);
        }
    };

    let usage_info: Option<UsageInfo> = unified_response.usage.clone().map(Into::into);

    if api_type == target_api_type {
        // No transformation needed, return original data and parsed usage.
        return (data, usage_info);
    }

    debug!(
        "[transform_result] API type mismatch. Incoming: {:?}, Target: {:?}. Transforming response body.",
        api_type, target_api_type
    );

    // Step 2: Serialize from UnifiedResponse to target format
    let target_payload_result = (target_adapter.response.encode)(unified_response);

    match target_payload_result {
        Ok(value) => {
            debug!(
                "[transform_result] Transformation complete. Result: {}",
                serde_json::to_string(&value).unwrap_or_default()
            );
            (value, usage_info)
        }
        Err(e) => {
            error!(
                "[transform_result] Failed to serialize to target response format: {}. Returning original data.",
                e
            );
            (data, usage_info)
        }
    }
}

pub struct StreamTransformer {
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    pub session: SessionContext,
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

    fn record_transformed_events(&mut self, events: &[SseEvent]) {
        for event in events {
            self.session.push_transformed_event(event.clone());
        }
    }

    fn usage_merge_strategy(&self) -> UsageMergeStrategy {
        match self.api_type {
            LlmApiType::Gemini | LlmApiType::Responses => UsageMergeStrategy::Replace,
            LlmApiType::Openai
            | LlmApiType::Anthropic
            | LlmApiType::Ollama
            | LlmApiType::GeminiOpenai => UsageMergeStrategy::FinalOnly,
        }
    }

    pub fn parse_usage_info(&mut self) -> Option<UsageInfo> {
        if let Some(usage) = &self.session.usage_cache {
            return Some(usage.clone());
        }

        if self.session.original_events.is_empty() {
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
                self.session.stream_id.clone(),
                Some("parse_usage_info"),
                Some("Unable to recover usage because no original stream events were retained."),
                Some("recent_original_events=0".to_string()),
                Some("Preserve upstream usage frames or widen the diagnostic window for this stream.".to_string()),
            ));
            debug!(
                "[transform][usage] stream_id={:?} provider={:?} no cached usage and no diagnostic events available",
                self.session.stream_id, self.api_type
            );
            return None;
        }

        let parsed = match self.api_type {
            LlmApiType::Openai | LlmApiType::GeminiOpenai => {
                self.session.original_events.iter().rev().find_map(|e| {
                    if e.data == "[DONE]" || e.data.is_empty() {
                        return None;
                    }
                    serde_json::from_str::<Value>(&e.data)
                        .ok()
                        .and_then(|v| billing::parse_usage_info(&v, self.api_type))
                })
            }
            LlmApiType::Gemini | LlmApiType::Ollama | LlmApiType::Responses => {
                self.session.original_events.iter().rev().find_map(|e| {
                    serde_json::from_str::<Value>(&e.data)
                        .ok()
                        .and_then(|v| billing::parse_usage_info(&v, self.api_type))
                })
            }
            LlmApiType::Anthropic => self
                .session
                .original_events
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
                        .and_then(|v| billing::parse_usage_info(&v, self.api_type))
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
                self.session.stream_id.clone(),
                Some("parse_usage_info"),
                Some("Unable to recover usage from cached stream diagnostics."),
                Some(format!(
                    "recent_original_events={}",
                    self.session.original_events.len()
                )),
                Some("Inspect upstream provider SSE usage frames or preserve a wider diagnostic window.".to_string()),
            ));
            warn!(
                "[transform][usage] stream_id={:?} provider={:?} usage cache miss and diagnostic fallback failed; recent_original_events={}",
                self.session.stream_id,
                self.api_type,
                self.session.original_events.len()
            );
        }

        parsed
    }

    pub(crate) fn get_or_generate_stream_id(&mut self) -> String {
        if let Some(ref id) = self.session.stream_id {
            id.clone()
        } else {
            use crate::utils::ID_GENERATOR;
            let new_id = if self.api_type == LlmApiType::Gemini {
                format!("gemini-stream-{}", ID_GENERATOR.generate_id())
            } else {
                format!("chatcmpl-{}", ID_GENERATOR.generate_id())
            };
            self.session.stream_id = Some(new_id.clone());
            new_id
        }
    }

    pub(crate) fn get_or_default_stream_model(&self) -> String {
        self.session.stream_model.clone().unwrap_or_else(|| {
            if self.api_type == LlmApiType::Gemini {
                "".to_string()
            } else {
                "unified-stream-model".to_string()
            }
        })
    }

    fn normalize_unified_chunk_session_state(&mut self, unified_chunk: &mut UnifiedChunkResponse) {
        let chunk_core = unified_chunk.core();
        if let Some(model) = chunk_core.model.filter(|value| !value.is_empty()) {
            self.session.stream_model = Some(model);
        }
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
                        self.session
                            .tool_call_id_map
                            .insert(stable_id.clone(), stable_id);
                    }
                }
                if choice.finish_reason.is_some() {
                    self.session.advance_gemini_message_index(choice.index);
                }
            }
        }

        if let Some(usage) = chunk_core.usage {
            self.session
                .merge_usage(usage.into(), self.usage_merge_strategy());
        }
        if let Some(finish_reason) = unified_chunk
            .choices
            .iter()
            .find_map(|choice| choice.finish_reason.clone())
        {
            self.session.finish_reason_cache = Some(finish_reason);
        }
    }

    pub(crate) fn update_session_from_stream_events(&mut self, events: &[UnifiedStreamEvent]) {
        for event in events {
            self.update_session_from_stream_event(event);
        }
    }

    pub(crate) fn update_session_from_stream_event(&mut self, event: &UnifiedStreamEvent) {
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
                self.session.current_item_index = *item_index;
                if let Some(item_id) = item_id {
                    self.session
                        .tool_call_id_map
                        .entry(item_id.clone())
                        .or_insert_with(|| item_id.clone());
                }
            }
            UnifiedStreamEvent::MessageStart { id, model, .. } => {
                if let Some(id) = id {
                    self.session.stream_id = Some(id.clone());
                }
                if let Some(model) = model {
                    self.session.stream_model = Some(model.clone());
                }
            }
            UnifiedStreamEvent::ContentBlockStart { index, kind } => match kind {
                UnifiedBlockKind::Text | UnifiedBlockKind::ToolCall => {
                    self.session.current_content_block_index = Some(*index);
                }
                UnifiedBlockKind::Reasoning => {
                    self.session.current_reasoning_block_index = Some(*index);
                }
                UnifiedBlockKind::Blob => {}
            },
            UnifiedStreamEvent::ContentBlockStop { index } => {
                if self.session.current_content_block_index == Some(*index) {
                    self.session.current_content_block_index = None;
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
                self.session.current_item_index = *item_index;
                self.session.current_content_part_index = Some(*part_index);
            }
            UnifiedStreamEvent::ReasoningStart { index } => {
                self.session.current_reasoning_block_index = Some(*index);
            }
            UnifiedStreamEvent::ReasoningStop { index } => {
                if self.session.current_reasoning_block_index == Some(*index) {
                    self.session.current_reasoning_block_index = None;
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
                self.session.current_item_index = *item_index;
                self.session.current_reasoning_part_index = Some(*part_index);
            }
            UnifiedStreamEvent::Usage { usage } => {
                self.session
                    .merge_usage(usage.clone().into(), self.usage_merge_strategy());
            }
            UnifiedStreamEvent::MessageDelta { finish_reason } => {
                if let Some(finish_reason) = finish_reason {
                    self.session.finish_reason_cache = Some(finish_reason.clone());
                }
            }
            UnifiedStreamEvent::ToolCallStart { id, .. } => {
                self.session.tool_call_id_map.insert(id.clone(), id.clone());
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta { id: Some(id), .. }
            | UnifiedStreamEvent::ToolCallStop { id: Some(id), .. } => {
                self.session.tool_call_id_map.insert(id.clone(), id.clone());
            }
            UnifiedStreamEvent::Error { error } => {
                self.session.last_error = Some(error.clone());
                if let Ok(diagnostic) =
                    serde_json::from_value::<UnifiedTransformDiagnostic>(error.clone())
                {
                    self.session.record_diagnostic(diagnostic);
                }
            }
            UnifiedStreamEvent::MessageStop
            | UnifiedStreamEvent::ContentBlockDelta { .. }
            | UnifiedStreamEvent::ToolCallArgumentsDelta { .. }
            | UnifiedStreamEvent::ToolCallStop { .. }
            | UnifiedStreamEvent::ReasoningDelta { .. }
            | UnifiedStreamEvent::BlobDelta { .. } => {}
        }
    }

    fn build_stream_error_payload(
        &self,
        stage: &'static str,
        message: String,
        raw_data: &str,
    ) -> Value {
        let raw_summary = if raw_data.len() > 256 {
            format!("{}...", &raw_data[..256])
        } else {
            raw_data.to_string()
        };
        let decision = PolicyDecision {
            diagnostic_kind: TransformDiagnosticKind::FatalTransformError,
            level: TransformLossLevel::Reject,
            action: TransformAction::Reject,
            reason: "A fatal transform error interrupted this streaming conversion.",
        };
        serde_json::to_value(build_transform_diagnostic(
            TransformDiagnosticKind::FatalTransformError,
            TransformProtocol::Api(self.api_type),
            TransformProtocol::Api(self.target_api_type),
            TransformValueKind::StreamError,
            decision,
            self.session.stream_id.clone(),
            Some(stage),
            Some(&message),
            Some(raw_summary),
            Some(
                "Inspect the raw summary and recent stream diagnostics to recover context."
                    .to_string(),
            ),
        ))
        .unwrap_or_else(|_| {
            serde_json::json!({
                "type": "transform_error",
                "stage": stage,
                "provider": format!("{:?}", self.api_type),
                "target": format!("{:?}", self.target_api_type),
                "stream_id": self.session.stream_id,
                "message": message,
                "raw_data_summary": raw_data
            })
        })
    }

    fn controlled_error_sse(
        &mut self,
        stage: &'static str,
        message: String,
        raw_data: &str,
    ) -> Vec<SseEvent> {
        let payload = self.build_stream_error_payload(stage, message, raw_data);
        self.session.last_error = Some(payload.clone());
        if let Ok(diagnostic) =
            serde_json::from_value::<UnifiedTransformDiagnostic>(payload.clone())
        {
            self.session.record_diagnostic(diagnostic);
        }
        vec![SseEvent {
            event: Some("error".to_string()),
            data: serde_json::to_string(&payload).unwrap_or_else(|_| {
                "{\"type\":\"transform_error\",\"message\":\"serialization failure\"}".to_string()
            }),
            ..Default::default()
        }]
    }

    fn bridge_stream_events_to_legacy_chunks(
        &mut self,
        events: Vec<UnifiedStreamEvent>,
    ) -> Vec<UnifiedChunkResponse> {
        let mut chunks = Vec::new();

        for event in events {
            let id = self.get_or_generate_stream_id();
            let model = Some(self.get_or_default_stream_model());

            let maybe_chunk = match event {
                UnifiedStreamEvent::MessageStart { role, .. } => Some(UnifiedChunkResponse {
                    id,
                    model,
                    choices: vec![UnifiedChunkChoice {
                        index: 0,
                        delta: UnifiedMessageDelta {
                            role: Some(role),
                            content: vec![],
                        },
                        finish_reason: None,
                    }],
                    object: Some("chat.completion.chunk".to_string()),
                    ..Default::default()
                }),
                UnifiedStreamEvent::ItemAdded { .. } | UnifiedStreamEvent::ItemDone { .. } => None,
                UnifiedStreamEvent::ContentBlockDelta { index, text, .. } => {
                    Some(UnifiedChunkResponse {
                        id,
                        model,
                        choices: vec![UnifiedChunkChoice {
                            index: 0,
                            delta: UnifiedMessageDelta {
                                role: None,
                                content: vec![UnifiedContentPartDelta::TextDelta { index, text }],
                            },
                            finish_reason: None,
                        }],
                        object: Some("chat.completion.chunk".to_string()),
                        ..Default::default()
                    })
                }
                UnifiedStreamEvent::ToolCallStart {
                    index,
                    id: tool_id,
                    name,
                } => Some(UnifiedChunkResponse {
                    id,
                    model,
                    choices: vec![UnifiedChunkChoice {
                        index: 0,
                        delta: UnifiedMessageDelta {
                            role: None,
                            content: vec![UnifiedContentPartDelta::ToolCallDelta(
                                UnifiedToolCallDelta {
                                    index,
                                    id: Some(tool_id),
                                    name: Some(name),
                                    arguments: None,
                                },
                            )],
                        },
                        finish_reason: None,
                    }],
                    object: Some("chat.completion.chunk".to_string()),
                    ..Default::default()
                }),
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index,
                    item_index: _,
                    item_id: _,
                    id: tool_id,
                    name,
                    arguments,
                } => Some(UnifiedChunkResponse {
                    id,
                    model,
                    choices: vec![UnifiedChunkChoice {
                        index: 0,
                        delta: UnifiedMessageDelta {
                            role: None,
                            content: vec![UnifiedContentPartDelta::ToolCallDelta(
                                UnifiedToolCallDelta {
                                    index,
                                    id: tool_id,
                                    name,
                                    arguments: Some(arguments),
                                },
                            )],
                        },
                        finish_reason: None,
                    }],
                    object: Some("chat.completion.chunk".to_string()),
                    ..Default::default()
                }),
                UnifiedStreamEvent::MessageDelta { finish_reason } => Some(UnifiedChunkResponse {
                    id,
                    model,
                    choices: vec![UnifiedChunkChoice {
                        index: 0,
                        delta: UnifiedMessageDelta::default(),
                        finish_reason,
                    }],
                    object: Some("chat.completion.chunk".to_string()),
                    ..Default::default()
                }),
                UnifiedStreamEvent::Usage { usage } => Some(UnifiedChunkResponse {
                    id,
                    model,
                    choices: vec![UnifiedChunkChoice {
                        index: 0,
                        delta: UnifiedMessageDelta::default(),
                        finish_reason: None,
                    }],
                    usage: Some(usage),
                    object: Some("chat.completion.chunk".to_string()),
                    ..Default::default()
                }),
                UnifiedStreamEvent::ReasoningStart { .. }
                | UnifiedStreamEvent::ContentPartAdded { .. }
                | UnifiedStreamEvent::ContentPartDone { .. }
                | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
                | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
                | UnifiedStreamEvent::ReasoningDelta { .. }
                | UnifiedStreamEvent::ReasoningStop { .. } => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Api(self.target_api_type),
                        TransformValueKind::ReasoningDelta,
                        "Dropping reasoning stream event while bridging to legacy chunk model.",
                    );
                    None
                }
                UnifiedStreamEvent::BlobDelta { .. } => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Api(self.target_api_type),
                        TransformValueKind::BlobDelta,
                        "Dropping blob stream event while bridging to legacy chunk model.",
                    );
                    None
                }
                UnifiedStreamEvent::Error { .. } => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Api(self.target_api_type),
                        TransformValueKind::StreamError,
                        "Dropping structured error event while bridging to legacy chunk model.",
                    );
                    None
                }
                UnifiedStreamEvent::MessageStop
                | UnifiedStreamEvent::ContentBlockStart { .. }
                | UnifiedStreamEvent::ContentBlockStop { .. }
                | UnifiedStreamEvent::ToolCallStop { .. } => None,
            };

            if let Some(mut chunk) = maybe_chunk {
                self.normalize_unified_chunk_session_state(&mut chunk);
                chunks.push(chunk);
            }
        }

        chunks
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
                    self.session.last_error = Some(error.clone());
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
                if let Some(chunk_events) =
                    (target_adapter.stream.encode_legacy_chunk)(unified_chunk, self)
                {
                    events.extend(chunk_events);
                }
            }
            (!events.is_empty()).then_some(events)
        } else {
            (target_adapter.stream.encode_events)(passthrough_events, self)
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
            if let Ok(frame) = (source_adapter.stream.decode_source)(&event.data, self) {
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

        // Handle OpenAI's stream termination marker
        if self.api_type == LlmApiType::Openai && event.data == "[DONE]" {
            // Gemini, Ollama, and Anthropic streams just end, so we return None to not send anything.
            return if self.target_api_type == LlmApiType::Gemini
                || self.target_api_type == LlmApiType::Ollama
                || self.target_api_type == LlmApiType::Anthropic
            {
                None
            } else {
                // Pass through for other potential targets
                Some(vec![event])
            };
        }

        if self.api_type == LlmApiType::Responses && self.target_api_type == LlmApiType::Openai {
            let transformed = match serde_json::from_str::<responses::ResponsesChunkResponse>(
                &event.data,
            ) {
                Ok(chunk) => responses::transform_responses_chunk_to_openai_events(chunk, self),
                Err(e) => {
                    error!(
                        "[StreamTransformer::transform_event] Failed to deserialize chunk from {:?}: {}. Data: '{}'",
                        LlmApiType::Responses,
                        e,
                        event.data
                    );
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

        let transformed = match (source_adapter.stream.decode_source)(&event.data, self) {
            Ok(DecodedSourceStreamFrame::Events(stream_events)) => {
                self.update_session_from_stream_events(&stream_events);
                self.stream_events_to_target_events(stream_events)
            }
            Ok(DecodedSourceStreamFrame::LegacyChunk(mut unified_chunk)) => {
                let consistent_id = self.get_or_generate_stream_id();
                unified_chunk.id = consistent_id;
                self.normalize_unified_chunk_session_state(&mut unified_chunk);
                (target_adapter.stream.encode_legacy_chunk)(unified_chunk, self)
            }
            Err(e) => {
                error!(
                    "[StreamTransformer::transform_event] Failed to deserialize chunk from {:?}: {}. Data: '{}'",
                    source_adapter.api_type, e, event.data
                );
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

#[cfg(test)]
mod adapter_contract_tests {
    use super::*;

    #[test]
    fn test_adapter_contract_registry_covers_all_transform_providers() {
        let cases = [
            (LlmApiType::Openai, "openai"),
            (LlmApiType::Gemini, "gemini"),
            (LlmApiType::Ollama, "ollama"),
            (LlmApiType::Anthropic, "anthropic"),
            (LlmApiType::Responses, "responses"),
        ];

        for (api_type, expected_name) in cases {
            let adapter = adapter_for(api_type);
            assert_eq!(adapter.api_type, api_type);
            assert_eq!(adapter.name, expected_name);
            assert_eq!(
                adapter.capabilities,
                ProtocolCapabilityMatrix::for_api(api_type)
            );
        }
    }

    #[test]
    fn test_adapter_contract_all_stream_encoders_are_event_native() {
        assert!(
            !adapter_for(LlmApiType::Openai)
                .stream
                .requires_legacy_bridge_for_events
        );
        assert!(
            !adapter_for(LlmApiType::Gemini)
                .stream
                .requires_legacy_bridge_for_events
        );
        assert!(
            !adapter_for(LlmApiType::Ollama)
                .stream
                .requires_legacy_bridge_for_events
        );
        assert!(
            !adapter_for(LlmApiType::Anthropic)
                .stream
                .requires_legacy_bridge_for_events
        );
        assert!(
            !adapter_for(LlmApiType::Responses)
                .stream
                .requires_legacy_bridge_for_events
        );
    }

    #[test]
    fn test_update_session_from_item_lifecycle_events_tracks_item_and_part_indices() {
        let mut transformer = StreamTransformer::new(LlmApiType::Responses, LlmApiType::Openai);
        transformer.update_session_from_stream_events(&[
            UnifiedStreamEvent::ItemAdded {
                item_index: Some(3),
                item_id: Some("msg_1".to_string()),
                item: UnifiedItem::Message(UnifiedMessageItem {
                    role: UnifiedRole::Assistant,
                    content: Vec::new(),
                    annotations: Vec::new(),
                }),
            },
            UnifiedStreamEvent::ContentPartAdded {
                item_index: Some(3),
                item_id: Some("msg_1".to_string()),
                part_index: 2,
                part: None,
            },
            UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index: Some(4),
                item_id: Some("rs_1".to_string()),
                part_index: 1,
                part: None,
            },
        ]);

        assert_eq!(transformer.session.current_item_index, Some(4));
        assert_eq!(transformer.session.current_content_part_index, Some(2));
        assert_eq!(transformer.session.current_reasoning_part_index, Some(1));
        assert_eq!(
            transformer.session.tool_call_id_map.get("msg_1"),
            Some(&"msg_1".to_string())
        );
    }
}
