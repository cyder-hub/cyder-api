use cyder_tools::log::warn;
use serde_json::Value;

use super::providers::{anthropic, gemini, ollama, openai, responses};
use super::request::apply_stream_options;
use super::stream::StreamTransformContext;
use super::unified::{UnifiedChunkResponse, UnifiedRequest, UnifiedResponse, UnifiedStreamEvent};
use crate::schema::enum_def::{LlmApiType, ProviderType};
use crate::utils::sse::SseEvent;

pub(in crate::service::transform) type RequestDecodeFn =
    fn(Value) -> Result<UnifiedRequest, serde_json::Error>;
pub(in crate::service::transform) type RequestEncodeFn =
    fn(UnifiedRequest) -> Result<Value, serde_json::Error>;
pub(in crate::service::transform) type ResponseDecodeFn =
    fn(Value) -> Result<UnifiedResponse, serde_json::Error>;
pub(in crate::service::transform) type ResponseEncodeFn =
    fn(UnifiedResponse) -> Result<Value, serde_json::Error>;
pub(in crate::service::transform) type SourceStreamDecodeFn =
    fn(
        &str,
        &mut StreamTransformContext<'_>,
    ) -> Result<DecodedSourceStreamFrame, serde_json::Error>;
pub(in crate::service::transform) type TargetStreamEventsEncodeFn =
    fn(Vec<UnifiedStreamEvent>, &mut StreamTransformContext<'_>) -> Option<Vec<SseEvent>>;
pub(in crate::service::transform) type TargetLegacyChunkEncodeFn =
    fn(UnifiedChunkResponse, &mut StreamTransformContext<'_>) -> Option<Vec<SseEvent>>;
pub(in crate::service::transform) type RequestFinalizeFn = fn(Value, &ProviderType, &str) -> Value;

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct RequestCodec {
    pub(in crate::service::transform) decode: RequestDecodeFn,
    pub(in crate::service::transform) encode: RequestEncodeFn,
    pub(in crate::service::transform) finalize: Option<RequestFinalizeFn>,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct ResponseCodec {
    pub(in crate::service::transform) decode: ResponseDecodeFn,
    pub(in crate::service::transform) encode: ResponseEncodeFn,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct StreamCodec {
    pub(in crate::service::transform) decode_source: SourceStreamDecodeFn,
    pub(in crate::service::transform) encode_events: TargetStreamEventsEncodeFn,
    pub(in crate::service::transform) encode_legacy_chunk: TargetLegacyChunkEncodeFn,
    pub(in crate::service::transform) requires_legacy_bridge_for_events: bool,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct TransformAdapter {
    pub(in crate::service::transform) api_type: LlmApiType,
    pub(in crate::service::transform) name: &'static str,
    pub(in crate::service::transform) request: RequestCodec,
    pub(in crate::service::transform) response: ResponseCodec,
    pub(in crate::service::transform) stream: StreamCodec,
}

pub(in crate::service::transform) enum DecodedSourceStreamFrame {
    Events(Vec<UnifiedStreamEvent>),
    LegacyChunk(UnifiedChunkResponse),
}

pub(in crate::service::transform) fn noop_finalize_request(
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
    context: &mut StreamTransformContext<'_>,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<openai::OpenAiChunkResponse>(raw)
        .map(|chunk| openai::openai_chunk_to_unified_stream_events_with_state(chunk, context))
        .map(DecodedSourceStreamFrame::Events)
}

fn decode_gemini_stream_frame(
    raw: &str,
    _context: &mut StreamTransformContext<'_>,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<gemini::GeminiChunkResponse>(raw)
        .map(Into::into)
        .map(DecodedSourceStreamFrame::LegacyChunk)
}

fn decode_ollama_stream_frame(
    raw: &str,
    _context: &mut StreamTransformContext<'_>,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<ollama::OllamaChunkResponse>(raw)
        .map(Into::into)
        .map(DecodedSourceStreamFrame::LegacyChunk)
}

fn decode_anthropic_stream_frame(
    raw: &str,
    context: &mut StreamTransformContext<'_>,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<anthropic::AnthropicEvent>(raw)
        .map(|event| {
            anthropic::anthropic_event_to_unified_stream_events_with_state(
                event,
                context.anthropic_session_mut(),
            )
        })
        .map(DecodedSourceStreamFrame::Events)
}

fn decode_responses_stream_frame(
    raw: &str,
    _context: &mut StreamTransformContext<'_>,
) -> Result<DecodedSourceStreamFrame, serde_json::Error> {
    serde_json::from_str::<responses::ResponsesChunkResponse>(raw)
        .map(responses::responses_chunk_to_unified_stream_events)
        .map(DecodedSourceStreamFrame::Events)
}

fn encode_anthropic_stream_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    anthropic::transform_unified_stream_events_to_anthropic_events(stream_events, context)
}

fn encode_anthropic_legacy_chunk(
    unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Option<Vec<SseEvent>> {
    anthropic::transform_unified_chunk_to_anthropic_events(unified_chunk, context)
}

const OPENAI_ADAPTER: TransformAdapter = TransformAdapter {
    api_type: LlmApiType::Openai,
    name: "openai",
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

pub(in crate::service::transform) fn adapter_for(
    api_type: LlmApiType,
) -> &'static TransformAdapter {
    match api_type {
        LlmApiType::Openai => &OPENAI_ADAPTER,
        LlmApiType::Gemini => &GEMINI_ADAPTER,
        LlmApiType::Ollama => &OLLAMA_ADAPTER,
        LlmApiType::Anthropic => &ANTHROPIC_ADAPTER,
        LlmApiType::Responses => &RESPONSES_ADAPTER,
        LlmApiType::GeminiOpenai => &OPENAI_ADAPTER,
    }
}

#[cfg(test)]
mod tests {
    use super::super::capability::ProtocolCapabilityMatrix;
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
        }
    }

    #[test]
    fn test_adapter_contract_gemini_openai_alias_uses_openai_adapter() {
        let openai = adapter_for(LlmApiType::Openai);
        let gemini_openai = adapter_for(LlmApiType::GeminiOpenai);

        assert_eq!(gemini_openai.name, "openai");
        assert_eq!(gemini_openai.api_type, LlmApiType::Openai);
        assert_eq!(
            ProtocolCapabilityMatrix::for_api(LlmApiType::GeminiOpenai),
            ProtocolCapabilityMatrix::for_api(openai.api_type)
        );
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
}
