//! Provider transform owner module.
//!
//! Provider modules are migrated under this namespace from task 9 onward.

pub(crate) mod anthropic;
pub(crate) mod gemini;
pub(crate) mod ollama;
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

    type EventEncoder =
        fn(Vec<UnifiedStreamEvent>, &mut StreamTransformContext<'_>) -> Option<Vec<SseEvent>>;
    type ChunkEncoder =
        fn(UnifiedChunkResponse, &mut StreamTransformContext<'_>) -> Option<Vec<SseEvent>>;

    fn assert_request_codec<T>()
    where
        T: From<UnifiedRequest> + Into<UnifiedRequest>,
    {
    }

    fn assert_response_codec<T>()
    where
        T: From<UnifiedResponse> + Into<UnifiedResponse>,
    {
    }

    fn assert_bidirectional_legacy_chunk<T>()
    where
        T: From<UnifiedChunkResponse>,
        UnifiedChunkResponse: From<T>,
    {
    }

    #[test]
    fn test_provider_modules_expose_required_codec_contracts() {
        assert_request_codec::<openai::OpenAiRequestPayload>();
        assert_response_codec::<openai::OpenAiResponse>();
        assert_bidirectional_legacy_chunk::<openai::OpenAiChunkResponse>();

        assert_request_codec::<gemini::GeminiRequestPayload>();
        assert_response_codec::<gemini::GeminiResponse>();
        assert_bidirectional_legacy_chunk::<gemini::GeminiChunkResponse>();

        assert_request_codec::<ollama::OllamaRequestPayload>();
        assert_response_codec::<ollama::OllamaResponse>();
        assert_bidirectional_legacy_chunk::<ollama::OllamaChunkResponse>();

        assert_request_codec::<anthropic::AnthropicRequestPayload>();
        assert_response_codec::<anthropic::AnthropicResponse>();
        let _: fn(anthropic::AnthropicEvent) -> Vec<UnifiedStreamEvent> =
            anthropic::anthropic_event_to_unified_stream_events;

        assert_request_codec::<responses::ResponsesRequestPayload>();
        assert_response_codec::<responses::ResponsesResponse>();
        let _: fn(responses::ResponsesChunkResponse) -> Vec<UnifiedStreamEvent> =
            responses::responses_chunk_to_unified_stream_events;
    }

    #[test]
    fn test_provider_modules_expose_required_stream_encoders() {
        let _: EventEncoder = openai::transform_unified_stream_events_to_openai_events;
        let _: ChunkEncoder = openai::transform_unified_chunk_to_openai_events;

        let _: EventEncoder = gemini::transform_unified_stream_events_to_gemini_events;
        let _: ChunkEncoder = gemini::transform_unified_chunk_to_gemini_events;

        let _: EventEncoder = ollama::transform_unified_stream_events_to_ollama_events;
        let _: ChunkEncoder = ollama::transform_unified_chunk_to_ollama_events;

        let _: EventEncoder = anthropic::transform_unified_stream_events_to_anthropic_events;
        let _: ChunkEncoder = anthropic::transform_unified_chunk_to_anthropic_events;

        let _: EventEncoder = responses::transform_unified_stream_events_to_responses_events;
        let _: ChunkEncoder = responses::transform_unified_chunk_to_responses_events;
        let _: fn(
            responses::ResponsesChunkResponse,
            &mut StreamTransformContext<'_>,
        ) -> Option<Vec<SseEvent>> = responses::transform_responses_chunk_to_openai_events;
    }
}
