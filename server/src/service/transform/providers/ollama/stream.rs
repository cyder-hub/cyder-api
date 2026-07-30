use chrono::Utc;

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

        // Map Ollama's done_reason to unified finish_reason
        let finish_reason = finish_reason.map(|reason| {
            match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(), // Default to stop for other reasons
            }
        });

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
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata: None,
            synthetic_metadata: None,
        }
    }
}
