use super::payload::OllamaResponse;

use crate::service::transform::unified::*;
use crate::utils::ID_GENERATOR;

impl From<OllamaResponse> for UnifiedResponse {
    fn from(ollama_res: OllamaResponse) -> Self {
        let message = UnifiedMessage {
            role: UnifiedRole::Assistant, // Ollama response is always assistant
            content: vec![UnifiedContentPart::Text {
                text: ollama_res.message.content,
            }],
        };

        let finish_reason = if ollama_res.done {
            ollama_res.done_reason.or_else(|| Some("stop".to_string()))
        } else {
            None
        };

        // Map Ollama's done_reason to unified finish_reason
        let finish_reason = finish_reason.map(|reason| match reason.as_str() {
            "stop" => "stop".to_string(),
            "length" => "length".to_string(),
            _ => reason,
        });

        let choice = UnifiedChoice {
            index: 0,
            message,
            items: Vec::new(),
            finish_reason,
            logprobs: None,
        };

        let usage = if let (Some(prompt_tokens), Some(completion_tokens)) =
            (ollama_res.prompt_tokens, ollama_res.completion_tokens)
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

        UnifiedResponse {
            id: format!("chatcmpl-{}", ID_GENERATOR.generate_id()),
            model: Some(ollama_res.model),
            choices: vec![choice],
            usage,
            created: Some(
                chrono::DateTime::parse_from_rfc3339(&ollama_res.created_at)
                    .expect("Ollama created_at is validated by the adapter")
                    .timestamp(),
            ),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        }
    }
}
