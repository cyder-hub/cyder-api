use cyder_tools::log::warn;
use serde_json::Value;

use super::providers::{anthropic, gemini, ollama, openai, responses};
use super::request::apply_stream_options;
use super::stream::StreamTransformContext;
use super::unified::{
    UnifiedChunkResponse, UnifiedRequest, UnifiedResponse, UnifiedStreamEvent,
    meaningful_output_from_legacy_chunk, meaningful_output_from_stream_events,
};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol};
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
pub(in crate::service::transform) type RequestFinalizeFn =
    fn(Value, &UpstreamProfileType, &str) -> Value;

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct DownstreamRequestCodec {
    pub(in crate::service::transform) decode: RequestDecodeFn,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct UpstreamRequestCodec {
    pub(in crate::service::transform) encode: RequestEncodeFn,
    pub(in crate::service::transform) finalize: Option<RequestFinalizeFn>,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct DownstreamResponseCodec {
    pub(in crate::service::transform) encode: ResponseEncodeFn,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct UpstreamResponseCodec {
    pub(in crate::service::transform) decode: ResponseDecodeFn,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct DownstreamStreamCodec {
    pub(in crate::service::transform) encode_events: TargetStreamEventsEncodeFn,
    pub(in crate::service::transform) encode_legacy_chunk: TargetLegacyChunkEncodeFn,
    pub(in crate::service::transform) requires_legacy_bridge_for_events: bool,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct UpstreamStreamCodec {
    pub(in crate::service::transform) decode_source: SourceStreamDecodeFn,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct DownstreamAdapter {
    pub(in crate::service::transform) protocol: DownstreamProtocol,
    pub(in crate::service::transform) name: &'static str,
    pub(in crate::service::transform) request: DownstreamRequestCodec,
    pub(in crate::service::transform) response: DownstreamResponseCodec,
    pub(in crate::service::transform) stream: DownstreamStreamCodec,
}

#[derive(Clone, Copy)]
pub(in crate::service::transform) struct UpstreamAdapter {
    pub(in crate::service::transform) protocol: UpstreamProtocol,
    pub(in crate::service::transform) name: &'static str,
    pub(in crate::service::transform) request: UpstreamRequestCodec,
    pub(in crate::service::transform) response: UpstreamResponseCodec,
    pub(in crate::service::transform) stream: UpstreamStreamCodec,
}

pub(in crate::service::transform) enum DecodedSourceStreamFrame {
    Events(Vec<UnifiedStreamEvent>),
    LegacyChunk(UnifiedChunkResponse),
}

impl DecodedSourceStreamFrame {
    pub(in crate::service::transform) fn meaningful_output_observed(&self) -> bool {
        match self {
            Self::Events(events) => meaningful_output_from_stream_events(events),
            Self::LegacyChunk(chunk) => meaningful_output_from_legacy_chunk(chunk),
        }
    }
}

pub(in crate::service::transform) fn noop_finalize_request(
    data: Value,
    _profile_type: &UpstreamProfileType,
    _downstream_path: &str,
) -> Value {
    data
}

fn finalize_openai_request(
    mut data: Value,
    profile_type: &UpstreamProfileType,
    downstream_path: &str,
) -> Value {
    apply_stream_options(&mut data);

    let (openai_variant, sanitize_report) = openai::finalize_openai_compatible_request_payload(
        &mut data,
        profile_type,
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

const OPENAI_DOWNSTREAM_ADAPTER: DownstreamAdapter = DownstreamAdapter {
    protocol: DownstreamProtocol::Openai,
    name: "openai",
    request: DownstreamRequestCodec {
        decode: decode_openai_request,
    },
    response: DownstreamResponseCodec {
        encode: encode_openai_response,
    },
    stream: DownstreamStreamCodec {
        encode_events: openai::transform_unified_stream_events_to_openai_events,
        encode_legacy_chunk: openai::transform_unified_chunk_to_openai_events,
        requires_legacy_bridge_for_events: false,
    },
};

const GEMINI_DOWNSTREAM_ADAPTER: DownstreamAdapter = DownstreamAdapter {
    protocol: DownstreamProtocol::Gemini,
    name: "gemini",
    request: DownstreamRequestCodec {
        decode: decode_gemini_request,
    },
    response: DownstreamResponseCodec {
        encode: encode_gemini_response,
    },
    stream: DownstreamStreamCodec {
        encode_events: gemini::transform_unified_stream_events_to_gemini_events,
        encode_legacy_chunk: gemini::transform_unified_chunk_to_gemini_events,
        requires_legacy_bridge_for_events: false,
    },
};

const ANTHROPIC_DOWNSTREAM_ADAPTER: DownstreamAdapter = DownstreamAdapter {
    protocol: DownstreamProtocol::Anthropic,
    name: "anthropic",
    request: DownstreamRequestCodec {
        decode: decode_anthropic_request,
    },
    response: DownstreamResponseCodec {
        encode: encode_anthropic_response,
    },
    stream: DownstreamStreamCodec {
        encode_events: encode_anthropic_stream_events,
        encode_legacy_chunk: encode_anthropic_legacy_chunk,
        requires_legacy_bridge_for_events: false,
    },
};

const RESPONSES_DOWNSTREAM_ADAPTER: DownstreamAdapter = DownstreamAdapter {
    protocol: DownstreamProtocol::Responses,
    name: "responses",
    request: DownstreamRequestCodec {
        decode: decode_responses_request,
    },
    response: DownstreamResponseCodec {
        encode: encode_responses_response,
    },
    stream: DownstreamStreamCodec {
        encode_events: responses::transform_unified_stream_events_to_responses_events,
        encode_legacy_chunk: responses::transform_unified_chunk_to_responses_events,
        requires_legacy_bridge_for_events: false,
    },
};

const OPENAI_UPSTREAM_ADAPTER: UpstreamAdapter = UpstreamAdapter {
    protocol: UpstreamProtocol::Openai,
    name: "openai",
    request: UpstreamRequestCodec {
        encode: encode_openai_request,
        finalize: Some(finalize_openai_request),
    },
    response: UpstreamResponseCodec {
        decode: decode_openai_response,
    },
    stream: UpstreamStreamCodec {
        decode_source: decode_openai_stream_frame,
    },
};

const GEMINI_UPSTREAM_ADAPTER: UpstreamAdapter = UpstreamAdapter {
    protocol: UpstreamProtocol::Gemini,
    name: "gemini",
    request: UpstreamRequestCodec {
        encode: encode_gemini_request,
        finalize: Some(noop_finalize_request),
    },
    response: UpstreamResponseCodec {
        decode: decode_gemini_response,
    },
    stream: UpstreamStreamCodec {
        decode_source: decode_gemini_stream_frame,
    },
};

const OLLAMA_UPSTREAM_ADAPTER: UpstreamAdapter = UpstreamAdapter {
    protocol: UpstreamProtocol::Ollama,
    name: "ollama",
    request: UpstreamRequestCodec {
        encode: encode_ollama_request,
        finalize: Some(noop_finalize_request),
    },
    response: UpstreamResponseCodec {
        decode: decode_ollama_response,
    },
    stream: UpstreamStreamCodec {
        decode_source: decode_ollama_stream_frame,
    },
};

const ANTHROPIC_UPSTREAM_ADAPTER: UpstreamAdapter = UpstreamAdapter {
    protocol: UpstreamProtocol::Anthropic,
    name: "anthropic",
    request: UpstreamRequestCodec {
        encode: encode_anthropic_request,
        finalize: Some(noop_finalize_request),
    },
    response: UpstreamResponseCodec {
        decode: decode_anthropic_response,
    },
    stream: UpstreamStreamCodec {
        decode_source: decode_anthropic_stream_frame,
    },
};

const RESPONSES_UPSTREAM_ADAPTER: UpstreamAdapter = UpstreamAdapter {
    protocol: UpstreamProtocol::Responses,
    name: "responses",
    request: UpstreamRequestCodec {
        encode: encode_responses_request,
        finalize: Some(noop_finalize_request),
    },
    response: UpstreamResponseCodec {
        decode: decode_responses_response,
    },
    stream: UpstreamStreamCodec {
        decode_source: decode_responses_stream_frame,
    },
};

pub(in crate::service::transform) fn downstream_adapter_for(
    protocol: DownstreamProtocol,
) -> &'static DownstreamAdapter {
    match protocol {
        DownstreamProtocol::Openai => &OPENAI_DOWNSTREAM_ADAPTER,
        DownstreamProtocol::Gemini => &GEMINI_DOWNSTREAM_ADAPTER,
        DownstreamProtocol::Anthropic => &ANTHROPIC_DOWNSTREAM_ADAPTER,
        DownstreamProtocol::Responses => &RESPONSES_DOWNSTREAM_ADAPTER,
    }
}

pub(in crate::service::transform) fn upstream_adapter_for(
    protocol: UpstreamProtocol,
) -> &'static UpstreamAdapter {
    match protocol {
        UpstreamProtocol::Openai => &OPENAI_UPSTREAM_ADAPTER,
        UpstreamProtocol::Gemini => &GEMINI_UPSTREAM_ADAPTER,
        UpstreamProtocol::Ollama => &OLLAMA_UPSTREAM_ADAPTER,
        UpstreamProtocol::Anthropic => &ANTHROPIC_UPSTREAM_ADAPTER,
        UpstreamProtocol::Responses => &RESPONSES_UPSTREAM_ADAPTER,
    }
}

#[cfg(test)]
mod tests {
    use super::super::capability::ProtocolCapabilityMatrix;
    use super::*;

    #[test]
    fn downstream_registry_contains_exactly_the_four_public_protocols() {
        for (protocol, expected_name) in [
            (DownstreamProtocol::Openai, "openai"),
            (DownstreamProtocol::Gemini, "gemini"),
            (DownstreamProtocol::Anthropic, "anthropic"),
            (DownstreamProtocol::Responses, "responses"),
        ] {
            let adapter = downstream_adapter_for(protocol);
            assert_eq!(adapter.protocol, protocol);
            assert_eq!(adapter.name, expected_name);
        }
    }

    #[test]
    fn upstream_registry_contains_exactly_the_five_wire_protocols() {
        for (protocol, expected_name) in [
            (UpstreamProtocol::Openai, "openai"),
            (UpstreamProtocol::Gemini, "gemini"),
            (UpstreamProtocol::Ollama, "ollama"),
            (UpstreamProtocol::Anthropic, "anthropic"),
            (UpstreamProtocol::Responses, "responses"),
        ] {
            let adapter = upstream_adapter_for(protocol);
            assert_eq!(adapter.protocol, protocol);
            assert_eq!(adapter.name, expected_name);
        }
    }

    #[test]
    fn all_downstream_stream_encoders_are_event_native() {
        for protocol in [
            DownstreamProtocol::Openai,
            DownstreamProtocol::Gemini,
            DownstreamProtocol::Anthropic,
            DownstreamProtocol::Responses,
        ] {
            assert!(
                !downstream_adapter_for(protocol)
                    .stream
                    .requires_legacy_bridge_for_events
            );
        }
    }

    #[test]
    fn capability_registry_keeps_upstream_ollama_without_downstream_ollama() {
        assert!(
            !ProtocolCapabilityMatrix::for_upstream(UpstreamProtocol::Ollama)
                .request
                .tool_definitions
        );
        assert_eq!(
            downstream_adapter_for(DownstreamProtocol::Openai).name,
            "openai"
        );
    }
}
