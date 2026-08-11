use chrono::DateTime;

use super::payload::OllamaChunkResponse;

use crate::service::transform::unified::*;
use crate::utils::ID_GENERATOR;

impl From<OllamaChunkResponse> for UnifiedChunkResponse {
    fn from(ollama_chunk: OllamaChunkResponse) -> Self {
        let delta = if let Some(message) = ollama_chunk.message {
            UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![UnifiedContentPartDelta::TextDelta {
                    index: 0,
                    text: message.content,
                }],
            }
        } else {
            UnifiedMessageDelta::default()
        };

        let finish_reason = if ollama_chunk.done {
            ollama_chunk
                .done_reason
                .or_else(|| Some("stop".to_string()))
        } else {
            None
        };

        // Preserve the provider reason. The source audit rejects unknown values
        // before this conversion, so no semantic reason is rewritten to `stop`.

        let choice = UnifiedChunkChoice {
            index: 0,
            delta,
            finish_reason,
        };

        let usage = if let (Some(prompt_tokens), Some(completion_tokens)) =
            (ollama_chunk.prompt_tokens, ollama_chunk.completion_tokens)
        {
            Some(UnifiedUsage {
                input_tokens: prompt_tokens,
                output_tokens: completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                ..Default::default()
            })
        } else {
            None
        };

        UnifiedChunkResponse {
            id: format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
            model: Some(ollama_chunk.model),
            choices: vec![choice],
            usage,
            created: Some(
                DateTime::parse_from_rfc3339(&ollama_chunk.created_at)
                    .expect("Ollama stream created_at must be validated before conversion")
                    .timestamp(),
            ),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata: None,
            synthetic_metadata: None,
        }
    }
}
