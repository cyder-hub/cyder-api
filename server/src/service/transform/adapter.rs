use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::audit::{
    audit_target_request, audit_target_response, normalize_portable_tool_request,
    validate_downstream_request, validate_upstream_response,
};
use super::diagnostics::{transform_failure, transform_success};
use super::providers::{anthropic, gemini, ollama, openai, responses};
use super::stream::StreamTransformContext;
use super::stream_audit::{
    SourceStreamSemanticError, audit_target_legacy_chunk, audit_target_stream_events,
    validate_anthropic_stream_event, validate_openai_stream_chunk, validate_responses_stream_chunk,
    validate_upstream_stream_frame,
};
use super::unified::{
    UnifiedChunkResponse, UnifiedRequest, UnifiedResponse, UnifiedStreamEvent,
    meaningful_output_from_legacy_chunk, meaningful_output_from_stream_events,
};
use super::{
    TransformAction, TransformFailureOrigin, TransformOutcomeKind, TransformPhase,
    TransformReasonCode, TransformResult, TransformSafeSummary, TransformSemanticUnit,
};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol};
use crate::utils::sse::SseEvent;

pub(in crate::service::transform) type RequestDecodeFn =
    fn(Value) -> TransformResult<UnifiedRequest>;
pub(in crate::service::transform) type RequestEncodeFn =
    fn(UnifiedRequest) -> TransformResult<Value>;
pub(in crate::service::transform) type ResponseDecodeFn =
    fn(Value) -> TransformResult<UnifiedResponse>;
pub(in crate::service::transform) type ResponseEncodeFn =
    fn(UnifiedResponse) -> TransformResult<Value>;
pub(in crate::service::transform) type SourceStreamDecodeFn =
    fn(&str, &mut StreamTransformContext<'_>) -> TransformResult<DecodedSourceStreamFrame>;
pub(in crate::service::transform) type TargetStreamEventsEncodeFn =
    fn(Vec<UnifiedStreamEvent>, &mut StreamTransformContext<'_>) -> TransformResult<Vec<SseEvent>>;
pub(in crate::service::transform) type TargetLegacyChunkEncodeFn =
    fn(UnifiedChunkResponse, &mut StreamTransformContext<'_>) -> TransformResult<Vec<SseEvent>>;
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
    data: Value,
    _profile_type: &UpstreamProfileType,
    _downstream_path: &str,
) -> Value {
    data
}

fn decode_json<T, U>(
    data: Value,
    origin: TransformFailureOrigin,
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
) -> TransformResult<U>
where
    T: DeserializeOwned,
    U: From<T>,
{
    let safe_summary = TransformSafeSummary::from_json(&data);
    match serde_json::from_value::<T>(data) {
        Ok(value) => Ok(transform_success(
            value.into(),
            phase,
            semantic_unit,
            TransformOutcomeKind::Lossless,
            TransformAction::Send,
            TransformReasonCode::LosslessConversion,
        )),
        Err(_) => Err(transform_failure(
            origin,
            phase,
            semantic_unit,
            if matches!(origin, TransformFailureOrigin::DownstreamInput) {
                TransformReasonCode::InvalidProtocolShape
            } else {
                TransformReasonCode::SourceDecodeFailed
            },
            Some(safe_summary),
        )),
    }
}

fn encode_json<T: Serialize>(
    value: T,
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
) -> TransformResult<Value> {
    match serde_json::to_value(value) {
        Ok(value) => Ok(transform_success(
            value,
            phase,
            semantic_unit,
            TransformOutcomeKind::Lossless,
            TransformAction::Send,
            TransformReasonCode::LosslessConversion,
        )),
        Err(_) => Err(transform_failure(
            TransformFailureOrigin::TargetEncoding,
            phase,
            semantic_unit,
            TransformReasonCode::TargetEncodeFailed,
            None,
        )),
    }
}

fn decode_openai_request(mut data: Value) -> TransformResult<UnifiedRequest> {
    normalize_portable_tool_request(DownstreamProtocol::Openai, &mut data)
        .map_err(|error| source_request_failure(error, &data))?;
    validate_request_source(DownstreamProtocol::Openai, &data)?;
    decode_json::<openai::OpenAiRequestPayload, _>(
        data,
        TransformFailureOrigin::DownstreamInput,
        TransformPhase::RequestDecode,
        TransformSemanticUnit::RequestEnvelope,
    )
}

fn encode_openai_request(unified: UnifiedRequest) -> TransformResult<Value> {
    audit_target_request(UpstreamProtocol::Openai, &unified);
    encode_json(
        openai::OpenAiRequestPayload::from(unified),
        TransformPhase::RequestEncode,
        TransformSemanticUnit::RequestEnvelope,
    )
}

fn decode_gemini_request(mut data: Value) -> TransformResult<UnifiedRequest> {
    normalize_portable_tool_request(DownstreamProtocol::Gemini, &mut data)
        .map_err(|error| source_request_failure(error, &data))?;
    validate_request_source(DownstreamProtocol::Gemini, &data)?;
    decode_json::<gemini::GeminiRequestPayload, _>(
        data,
        TransformFailureOrigin::DownstreamInput,
        TransformPhase::RequestDecode,
        TransformSemanticUnit::RequestEnvelope,
    )
}

fn encode_gemini_request(unified: UnifiedRequest) -> TransformResult<Value> {
    audit_target_request(UpstreamProtocol::Gemini, &unified);
    encode_json(
        gemini::GeminiRequestPayload::from(unified),
        TransformPhase::RequestEncode,
        TransformSemanticUnit::RequestEnvelope,
    )
}

fn encode_ollama_request(unified: UnifiedRequest) -> TransformResult<Value> {
    audit_target_request(UpstreamProtocol::Ollama, &unified);
    encode_json(
        ollama::OllamaRequestPayload::from(unified),
        TransformPhase::RequestEncode,
        TransformSemanticUnit::RequestEnvelope,
    )
}

fn decode_anthropic_request(mut data: Value) -> TransformResult<UnifiedRequest> {
    normalize_portable_tool_request(DownstreamProtocol::Anthropic, &mut data)
        .map_err(|error| source_request_failure(error, &data))?;
    validate_request_source(DownstreamProtocol::Anthropic, &data)?;
    decode_json::<anthropic::AnthropicRequestPayload, _>(
        data,
        TransformFailureOrigin::DownstreamInput,
        TransformPhase::RequestDecode,
        TransformSemanticUnit::RequestEnvelope,
    )
}

fn encode_anthropic_request(unified: UnifiedRequest) -> TransformResult<Value> {
    audit_target_request(UpstreamProtocol::Anthropic, &unified);
    encode_json(
        anthropic::AnthropicRequestPayload::from(unified),
        TransformPhase::RequestEncode,
        TransformSemanticUnit::RequestEnvelope,
    )
}

fn decode_responses_request(mut data: Value) -> TransformResult<UnifiedRequest> {
    normalize_portable_tool_request(DownstreamProtocol::Responses, &mut data)
        .map_err(|error| source_request_failure(error, &data))?;
    validate_request_source(DownstreamProtocol::Responses, &data)?;
    decode_json::<responses::ResponsesRequestPayload, _>(
        data,
        TransformFailureOrigin::DownstreamInput,
        TransformPhase::RequestDecode,
        TransformSemanticUnit::RequestEnvelope,
    )
}

fn encode_responses_request(unified: UnifiedRequest) -> TransformResult<Value> {
    audit_target_request(UpstreamProtocol::Responses, &unified);
    encode_json(
        responses::ResponsesRequestPayload::from(unified),
        TransformPhase::RequestEncode,
        TransformSemanticUnit::RequestEnvelope,
    )
}

fn decode_openai_response(data: Value) -> TransformResult<UnifiedResponse> {
    validate_response_source(UpstreamProtocol::Openai, &data)?;
    decode_json::<openai::OpenAiResponse, _>(
        data,
        TransformFailureOrigin::UpstreamPayload,
        TransformPhase::ResponseDecode,
        TransformSemanticUnit::ResponseEnvelope,
    )
}

fn encode_openai_response(unified: UnifiedResponse) -> TransformResult<Value> {
    audit_target_response(DownstreamProtocol::Openai, &unified);
    if unified.choices.iter().any(|choice| {
        choice.logprobs.as_ref().is_some_and(|logprobs| {
            serde_json::from_value::<openai::OpenAiLogProbs>(logprobs.clone()).is_err()
        })
    }) {
        return Err(transform_failure(
            TransformFailureOrigin::TargetEncoding,
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::Metadata,
            TransformReasonCode::TargetEncodeFailed,
            None,
        ));
    }
    encode_json(
        openai::OpenAiResponse::from(unified),
        TransformPhase::ResponseEncode,
        TransformSemanticUnit::ResponseEnvelope,
    )
}

fn decode_gemini_response(data: Value) -> TransformResult<UnifiedResponse> {
    validate_response_source(UpstreamProtocol::Gemini, &data)?;
    decode_json::<gemini::GeminiResponse, _>(
        data,
        TransformFailureOrigin::UpstreamPayload,
        TransformPhase::ResponseDecode,
        TransformSemanticUnit::ResponseEnvelope,
    )
}

fn encode_gemini_response(unified: UnifiedResponse) -> TransformResult<Value> {
    audit_target_response(DownstreamProtocol::Gemini, &unified);
    encode_json(
        gemini::GeminiResponse::from(unified),
        TransformPhase::ResponseEncode,
        TransformSemanticUnit::ResponseEnvelope,
    )
}

fn decode_ollama_response(data: Value) -> TransformResult<UnifiedResponse> {
    validate_response_source(UpstreamProtocol::Ollama, &data)?;
    decode_json::<ollama::OllamaResponse, _>(
        data,
        TransformFailureOrigin::UpstreamPayload,
        TransformPhase::ResponseDecode,
        TransformSemanticUnit::ResponseEnvelope,
    )
}

fn decode_anthropic_response(data: Value) -> TransformResult<UnifiedResponse> {
    validate_response_source(UpstreamProtocol::Anthropic, &data)?;
    decode_json::<anthropic::AnthropicResponse, _>(
        data,
        TransformFailureOrigin::UpstreamPayload,
        TransformPhase::ResponseDecode,
        TransformSemanticUnit::ResponseEnvelope,
    )
}

fn encode_anthropic_response(unified: UnifiedResponse) -> TransformResult<Value> {
    audit_target_response(DownstreamProtocol::Anthropic, &unified);
    encode_json(
        anthropic::AnthropicResponse::from(unified),
        TransformPhase::ResponseEncode,
        TransformSemanticUnit::ResponseEnvelope,
    )
}

fn decode_responses_response(data: Value) -> TransformResult<UnifiedResponse> {
    validate_response_source(UpstreamProtocol::Responses, &data)?;
    decode_json::<responses::ResponsesResponse, _>(
        data,
        TransformFailureOrigin::UpstreamPayload,
        TransformPhase::ResponseDecode,
        TransformSemanticUnit::ResponseEnvelope,
    )
}

fn encode_responses_response(unified: UnifiedResponse) -> TransformResult<Value> {
    audit_target_response(DownstreamProtocol::Responses, &unified);
    encode_json(
        responses::ResponsesResponse::from(unified),
        TransformPhase::ResponseEncode,
        TransformSemanticUnit::ResponseEnvelope,
    )
}

fn validate_request_source(protocol: DownstreamProtocol, data: &Value) -> TransformResult<()> {
    match validate_downstream_request(protocol, data) {
        Ok(()) => Ok(transform_success(
            (),
            TransformPhase::RequestDecode,
            TransformSemanticUnit::RequestEnvelope,
            TransformOutcomeKind::Lossless,
            TransformAction::Send,
            TransformReasonCode::LosslessConversion,
        )),
        Err(error) => Err(transform_failure(
            TransformFailureOrigin::DownstreamInput,
            TransformPhase::RequestDecode,
            error.semantic_unit,
            error.reason_code,
            Some(TransformSafeSummary::from_json(data)),
        )),
    }
}

fn source_request_failure(
    error: super::audit::SourceSemanticError,
    data: &Value,
) -> super::TransformFailure {
    transform_failure(
        TransformFailureOrigin::DownstreamInput,
        TransformPhase::RequestDecode,
        error.semantic_unit,
        error.reason_code,
        Some(TransformSafeSummary::from_json(data)),
    )
}

fn validate_response_source(protocol: UpstreamProtocol, data: &Value) -> TransformResult<()> {
    match validate_upstream_response(protocol, data) {
        Ok(()) => Ok(transform_success(
            (),
            TransformPhase::ResponseDecode,
            TransformSemanticUnit::ResponseEnvelope,
            TransformOutcomeKind::Lossless,
            TransformAction::Send,
            TransformReasonCode::LosslessConversion,
        )),
        Err(error) => Err(transform_failure(
            TransformFailureOrigin::UpstreamPayload,
            TransformPhase::ResponseDecode,
            error.semantic_unit,
            if error.reason_code == TransformReasonCode::InvalidProtocolShape {
                TransformReasonCode::SourceDecodeFailed
            } else {
                error.reason_code
            },
            Some(TransformSafeSummary::from_json(data)),
        )),
    }
}

fn decode_openai_stream_frame(
    raw: &str,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<DecodedSourceStreamFrame> {
    let chunk = match decode_stream_payload::<openai::OpenAiChunkResponse>(raw) {
        Ok(chunk) => chunk,
        Err(decode_failure) => {
            if let Err(semantic_failure) =
                validate_stream_source(UpstreamProtocol::Openai, raw, context)
            {
                return Err(semantic_failure);
            }
            return Err(decode_failure);
        }
    };
    validate_typed_stream_source(raw, validate_openai_stream_chunk(&chunk, context))?;
    Ok(stream_decode_success(DecodedSourceStreamFrame::Events(
        openai::openai_chunk_to_unified_stream_events_with_state(chunk, context),
    )))
}

fn decode_gemini_stream_frame(
    raw: &str,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<DecodedSourceStreamFrame> {
    validate_stream_source(UpstreamProtocol::Gemini, raw, context)?;
    decode_stream_result(
        raw,
        serde_json::from_str::<gemini::GeminiChunkResponse>(raw)
            .map(Into::into)
            .map(DecodedSourceStreamFrame::LegacyChunk),
    )
}

fn decode_ollama_stream_frame(
    raw: &str,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<DecodedSourceStreamFrame> {
    validate_stream_source(UpstreamProtocol::Ollama, raw, context)?;
    decode_stream_result(
        raw,
        serde_json::from_str::<ollama::OllamaChunkResponse>(raw)
            .map(Into::into)
            .map(DecodedSourceStreamFrame::LegacyChunk),
    )
}

fn decode_anthropic_stream_frame(
    raw: &str,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<DecodedSourceStreamFrame> {
    let event = match decode_stream_payload::<anthropic::AnthropicEvent>(raw) {
        Ok(event) => event,
        Err(decode_failure) => {
            if let Err(semantic_failure) =
                validate_stream_source(UpstreamProtocol::Anthropic, raw, context)
            {
                return Err(semantic_failure);
            }
            return Err(decode_failure);
        }
    };
    validate_typed_stream_source(raw, validate_anthropic_stream_event(&event, context))?;
    Ok(stream_decode_success(DecodedSourceStreamFrame::Events(
        anthropic::anthropic_event_to_unified_stream_events_with_state(
            event,
            context.anthropic_session_mut(),
        ),
    )))
}

fn decode_responses_stream_frame(
    raw: &str,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<DecodedSourceStreamFrame> {
    let chunk = decode_stream_payload::<responses::ResponsesChunkResponse>(raw)?;
    validate_typed_stream_source(raw, validate_responses_stream_chunk(&chunk, context))?;
    Ok(stream_decode_success(DecodedSourceStreamFrame::Events(
        responses::responses_chunk_to_unified_stream_events(chunk),
    )))
}

fn validate_typed_stream_source(
    raw: &str,
    result: Result<(), SourceStreamSemanticError>,
) -> Result<(), super::TransformFailure> {
    match result {
        Ok(()) => Ok(()),
        Err(error) => Err(transform_failure(
            TransformFailureOrigin::UpstreamPayload,
            TransformPhase::StreamDecode,
            error.semantic_unit,
            error.reason_code,
            Some(TransformSafeSummary::from_bytes(raw.as_bytes())),
        )),
    }
}

fn validate_stream_source(
    protocol: UpstreamProtocol,
    raw: &str,
    context: &mut StreamTransformContext<'_>,
) -> Result<(), super::TransformFailure> {
    match validate_upstream_stream_frame(protocol, raw, context) {
        Ok(()) => Ok(()),
        Err(error) => Err(transform_failure(
            TransformFailureOrigin::UpstreamPayload,
            TransformPhase::StreamDecode,
            error.semantic_unit,
            error.reason_code,
            Some(TransformSafeSummary::from_bytes(raw.as_bytes())),
        )),
    }
}

fn decode_stream_result(
    raw: &str,
    result: Result<DecodedSourceStreamFrame, serde_json::Error>,
) -> TransformResult<DecodedSourceStreamFrame> {
    match result {
        Ok(value) => Ok(transform_success(
            value,
            TransformPhase::StreamDecode,
            TransformSemanticUnit::StreamFrame,
            TransformOutcomeKind::Lossless,
            TransformAction::Send,
            TransformReasonCode::LosslessConversion,
        )),
        Err(_) => Err(transform_failure(
            TransformFailureOrigin::UpstreamPayload,
            TransformPhase::StreamDecode,
            TransformSemanticUnit::StreamFrame,
            TransformReasonCode::SourceDecodeFailed,
            Some(TransformSafeSummary::from_bytes(raw.as_bytes())),
        )),
    }
}

fn decode_stream_payload<T: DeserializeOwned>(raw: &str) -> Result<T, super::TransformFailure> {
    serde_json::from_str(raw).map_err(|_| {
        transform_failure(
            TransformFailureOrigin::UpstreamPayload,
            TransformPhase::StreamDecode,
            TransformSemanticUnit::StreamFrame,
            TransformReasonCode::SourceDecodeFailed,
            Some(TransformSafeSummary::from_bytes(raw.as_bytes())),
        )
    })
}

fn stream_decode_success(
    value: DecodedSourceStreamFrame,
) -> super::TransformSuccess<DecodedSourceStreamFrame> {
    transform_success(
        value,
        TransformPhase::StreamDecode,
        TransformSemanticUnit::StreamFrame,
        TransformOutcomeKind::Lossless,
        TransformAction::Send,
        TransformReasonCode::LosslessConversion,
    )
}

fn encode_stream_result(
    result: Result<Option<Vec<SseEvent>>, serde_json::Error>,
) -> TransformResult<Vec<SseEvent>> {
    let result = result.map_err(|_| {
        transform_failure(
            TransformFailureOrigin::TargetEncoding,
            TransformPhase::StreamEncode,
            TransformSemanticUnit::StreamFrame,
            TransformReasonCode::TargetSerializeFailed,
            None,
        )
    })?;
    let (events, action, reason_code) = match result {
        Some(events) => (
            events,
            TransformAction::Send,
            TransformReasonCode::LosslessConversion,
        ),
        None => (
            Vec::new(),
            TransformAction::Drop,
            TransformReasonCode::NoSemanticOutput,
        ),
    };
    Ok(transform_success(
        events,
        TransformPhase::StreamEncode,
        TransformSemanticUnit::StreamFrame,
        TransformOutcomeKind::Lossless,
        action,
        reason_code,
    ))
}

fn encode_openai_stream_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<Vec<SseEvent>> {
    audit_target_stream_events(DownstreamProtocol::Openai, &stream_events, context);
    encode_stream_result(
        openai::try_transform_unified_stream_events_to_openai_events(stream_events, context),
    )
}

fn encode_openai_legacy_chunk(
    mut unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<Vec<SseEvent>> {
    audit_target_legacy_chunk(DownstreamProtocol::Openai, &unified_chunk);
    if unified_chunk.model.as_deref().is_none_or(str::is_empty) {
        unified_chunk.model = Some(context.get_or_default_stream_model());
    }
    encode_stream_result(openai::try_transform_unified_chunk_to_openai_events(
        unified_chunk,
        context,
    ))
}

fn encode_gemini_stream_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<Vec<SseEvent>> {
    audit_target_stream_events(DownstreamProtocol::Gemini, &stream_events, context);
    encode_stream_result(
        gemini::try_transform_unified_stream_events_to_gemini_events(stream_events, context),
    )
}

fn encode_gemini_legacy_chunk(
    unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<Vec<SseEvent>> {
    audit_target_legacy_chunk(DownstreamProtocol::Gemini, &unified_chunk);
    encode_stream_result(gemini::try_transform_unified_chunk_to_gemini_events(
        unified_chunk,
        context,
    ))
}

fn encode_anthropic_stream_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<Vec<SseEvent>> {
    audit_target_stream_events(DownstreamProtocol::Anthropic, &stream_events, context);
    encode_stream_result(
        anthropic::try_transform_unified_stream_events_to_anthropic_events(stream_events, context),
    )
}

fn encode_anthropic_legacy_chunk(
    unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<Vec<SseEvent>> {
    audit_target_legacy_chunk(DownstreamProtocol::Anthropic, &unified_chunk);
    encode_stream_result(anthropic::try_transform_unified_chunk_to_anthropic_events(
        unified_chunk,
        context,
    ))
}

fn encode_responses_stream_events(
    stream_events: Vec<UnifiedStreamEvent>,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<Vec<SseEvent>> {
    audit_target_stream_events(DownstreamProtocol::Responses, &stream_events, context);
    encode_stream_result(
        responses::try_transform_unified_stream_events_to_responses_events(stream_events, context),
    )
}

fn encode_responses_legacy_chunk(
    unified_chunk: UnifiedChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> TransformResult<Vec<SseEvent>> {
    audit_target_legacy_chunk(DownstreamProtocol::Responses, &unified_chunk);
    encode_stream_result(responses::try_transform_unified_chunk_to_responses_events(
        unified_chunk,
        context,
    ))
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
        encode_events: encode_openai_stream_events,
        encode_legacy_chunk: encode_openai_legacy_chunk,
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
        encode_events: encode_gemini_stream_events,
        encode_legacy_chunk: encode_gemini_legacy_chunk,
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
        encode_events: encode_responses_stream_events,
        encode_legacy_chunk: encode_responses_legacy_chunk,
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
