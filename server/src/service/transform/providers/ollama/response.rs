use chrono::Utc;

use super::payload::{OllamaMessage, OllamaResponse};

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
        let finish_reason = finish_reason.map(|reason| {
            match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(), // Default to stop for other reasons
            }
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
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        }
    }
}

impl From<UnifiedResponse> for OllamaResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let choice = unified_res
            .choices
            .into_iter()
            .next()
            .unwrap_or_else(|| UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: None,
                logprobs: None,
            });

        let mut content = String::new();
        for part in choice.message.content {
            if let UnifiedContentPart::Text { text } = part {
                content.push_str(&text);
            }
        }

        let message = OllamaMessage {
            role: "assistant".to_string(),
            content,
            images: None,
        };

        let (prompt_tokens, completion_tokens) = if let Some(usage) = unified_res.usage {
            (Some(usage.input_tokens), Some(usage.output_tokens))
        } else {
            (None, None)
        };

        let done_reason = choice
            .finish_reason
            .as_ref()
            .map(|reason| match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(),
            });

        OllamaResponse {
            model: unified_res.model.unwrap_or_default(),
            created_at: Utc::now().to_rfc3339(),
            message,
            done: choice.finish_reason.is_some(),
            done_reason,
            prompt_tokens,
            completion_tokens,
            total_duration: None,
            load_duration: None,
            prompt_eval_duration: None,
            eval_duration: None,
        }
    }
}
