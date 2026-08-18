//! Provider transform owner module.
//!
//! Provider modules are migrated under this namespace from task 9 onward.

pub(crate) mod anthropic;
pub(crate) mod gemini;
pub(crate) mod openai;
pub(crate) mod responses;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::transform::stream::StreamTransformContext;
    use crate::service::transform::unified::{
        UnifiedChunkResponse, UnifiedRequest, UnifiedResponse, UnifiedStreamEvent,
    };
    use crate::utils::sse::SseEvent;

    type FallibleEventEncoder = fn(
        Vec<UnifiedStreamEvent>,
        &mut StreamTransformContext<'_>,
    ) -> Result<Option<Vec<SseEvent>>, serde_json::Error>;
    type FallibleChunkEncoder = fn(
        UnifiedChunkResponse,
        &mut StreamTransformContext<'_>,
    ) -> Result<Option<Vec<SseEvent>>, serde_json::Error>;

    fn assert_downstream_request_decoder<T>()
    where
        T: Into<UnifiedRequest>,
    {
    }

    fn assert_upstream_request_encoder<T>()
    where
        T: From<UnifiedRequest>,
    {
    }

    fn assert_downstream_response_encoder<T>()
    where
        T: From<UnifiedResponse>,
    {
    }

    fn assert_upstream_response_decoder<T>()
    where
        T: Into<UnifiedResponse>,
    {
    }

    fn assert_downstream_legacy_chunk_encoder<T>()
    where
        T: From<UnifiedChunkResponse>,
    {
    }

    fn assert_upstream_legacy_chunk_decoder<T>()
    where
        UnifiedChunkResponse: From<T>,
    {
    }

    #[test]
    fn test_provider_modules_expose_required_codec_contracts() {
        assert_downstream_request_decoder::<openai::OpenAiRequestPayload>();
        assert_upstream_request_encoder::<openai::OpenAiRequestPayload>();
        assert_downstream_response_encoder::<openai::OpenAiResponse>();
        assert_upstream_response_decoder::<openai::OpenAiResponse>();
        assert_downstream_legacy_chunk_encoder::<openai::OpenAiChunkResponse>();
        assert_upstream_legacy_chunk_decoder::<openai::OpenAiChunkResponse>();

        assert_downstream_request_decoder::<gemini::GeminiRequestPayload>();
        assert_upstream_request_encoder::<gemini::GeminiRequestPayload>();
        assert_downstream_response_encoder::<gemini::GeminiResponse>();
        assert_upstream_response_decoder::<gemini::GeminiResponse>();
        assert_downstream_legacy_chunk_encoder::<gemini::GeminiChunkResponse>();
        assert_upstream_legacy_chunk_decoder::<gemini::GeminiChunkResponse>();

        assert_downstream_request_decoder::<anthropic::AnthropicRequestPayload>();
        assert_upstream_request_encoder::<anthropic::AnthropicRequestPayload>();
        assert_downstream_response_encoder::<anthropic::AnthropicResponse>();
        assert_upstream_response_decoder::<anthropic::AnthropicResponse>();
        let _: fn(anthropic::AnthropicEvent) -> Vec<UnifiedStreamEvent> =
            anthropic::anthropic_event_to_unified_stream_events;

        assert_downstream_request_decoder::<responses::ResponsesRequestPayload>();
        assert_upstream_request_encoder::<responses::ResponsesRequestPayload>();
        assert_downstream_response_encoder::<responses::ResponsesResponse>();
        assert_upstream_response_decoder::<responses::ResponsesResponse>();
        let _: fn(responses::ResponsesChunkResponse) -> Vec<UnifiedStreamEvent> =
            responses::responses_chunk_to_unified_stream_events;
    }

    #[test]
    fn test_provider_modules_expose_required_stream_encoders() {
        let _: FallibleEventEncoder = openai::try_transform_unified_stream_events_to_openai_events;
        let _: FallibleChunkEncoder = openai::try_transform_unified_chunk_to_openai_events;

        let _: FallibleEventEncoder = gemini::try_transform_unified_stream_events_to_gemini_events;
        let _: FallibleChunkEncoder = gemini::try_transform_unified_chunk_to_gemini_events;

        let _: FallibleEventEncoder =
            anthropic::try_transform_unified_stream_events_to_anthropic_events;
        let _: FallibleChunkEncoder = anthropic::try_transform_unified_chunk_to_anthropic_events;

        let _: FallibleEventEncoder =
            responses::try_transform_unified_stream_events_to_responses_events;
        let _: FallibleChunkEncoder = responses::try_transform_unified_chunk_to_responses_events;
    }
}
