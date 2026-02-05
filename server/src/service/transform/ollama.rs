use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::unified::*;
use crate::utils::ID_GENERATOR;

// --- Ollama to Unified ---

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaRequestPayload {
    pub model: String,
    pub messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<OllamaOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>, // Base64 encoded images
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(rename = "num_predict")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
}

impl From<OllamaRequestPayload> for UnifiedRequest {
    fn from(ollama_req: OllamaRequestPayload) -> Self {
        let messages = ollama_req
            .messages
            .into_iter()
            .map(|msg| {
                let role = match msg.role.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    _ => UnifiedRole::User, // Default to user
                };
                // NOTE: Ollama's `images` field is ignored for now.
                // UnifiedRequest would need to be updated to handle multimodal content.
                let content = vec![UnifiedContentPart::Text { text: msg.content }];
                UnifiedMessage {
                    role,
                    content,
                }
            })
            .collect();

        let (temperature, max_tokens, top_p, stop, seed, presence_penalty, frequency_penalty) =
            if let Some(options) = ollama_req.options {
                (
                    options.temperature,
                    options.max_tokens,
                    options.top_p,
                    options.stop,
                    options.seed,
                    options.presence_penalty,
                    options.frequency_penalty,
                )
            } else {
                (None, None, None, None, None, None, None)
            };

        UnifiedRequest {
            model: Some(ollama_req.model),
            messages,
            // Ollama doesn't support tools/function calling - always set to None
            tools: None,
            stream: ollama_req.stream.unwrap_or(false),
            temperature,
            max_tokens,
            top_p,
            stop,
            seed,
            presence_penalty,
            frequency_penalty,
            format: ollama_req.format,
            keep_alive: ollama_req.keep_alive,
            ..Default::default()
        }
        .filter_empty() // Filter out empty content and messages
    }
}

impl From<UnifiedRequest> for OllamaRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let messages = unified_req
            .messages
            .into_iter()
            .filter_map(|msg| {
                let role = match msg.role {
                    UnifiedRole::System => "system",
                    UnifiedRole::User => "user",
                    UnifiedRole::Assistant => "assistant",
                    // Ollama doesn't support tool role or function calling - drop tool messages
                    // Warning: This means tool results will be lost when targeting Ollama
                    UnifiedRole::Tool => return None,
                }
                .to_string();

                let mut final_content = String::new();
                
                for part in msg.content {
                    match part {
                        UnifiedContentPart::Text { text } => {
                             final_content.push_str(&text);
                        }
                        _ => {}
                    }
                }

                if final_content.is_empty() {
                    // Don't send empty messages? Or send empty string.
                    // Assuming we send what we have.
                }

                Some(OllamaMessage {
                    role,
                    content: final_content,
                    images: None,
                })
            })
            .collect();

        let options = if unified_req.temperature.is_some()
            || unified_req.max_tokens.is_some()
            || unified_req.top_p.is_some()
            || unified_req.stop.is_some()
            || unified_req.seed.is_some()
            || unified_req.presence_penalty.is_some()
            || unified_req.frequency_penalty.is_some()
        {
            Some(OllamaOptions {
                temperature: unified_req.temperature,
                max_tokens: unified_req.max_tokens,
                top_p: unified_req.top_p,
                stop: unified_req.stop,
                seed: unified_req.seed,
                presence_penalty: unified_req.presence_penalty,
                frequency_penalty: unified_req.frequency_penalty,
            })
        } else {
            None
        };

        OllamaRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            messages,
            stream: Some(unified_req.stream),
            options,
            format: None,
            keep_alive: None,
        }
    }
}

// --- Ollama Response to Unified ---

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaResponse {
    pub model: String,
    pub created_at: String,
    pub message: OllamaMessage,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_reason: Option<String>,
    #[serde(rename = "prompt_eval_count")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(rename = "eval_count")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_duration: Option<u64>,
}

impl From<OllamaResponse> for UnifiedResponse {
    fn from(ollama_res: OllamaResponse) -> Self {
        let message = UnifiedMessage {
            role: UnifiedRole::Assistant, // Ollama response is always assistant
            content: vec![UnifiedContentPart::Text { text: ollama_res.message.content }],
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
            model: ollama_res.model,
            choices: vec![choice],
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
        }
    }
}

impl From<UnifiedResponse> for OllamaResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let choice = unified_res.choices.into_iter().next().unwrap_or_else(|| {
            UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![],
                },
                finish_reason: None,
                logprobs: None,
            }
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

        let done_reason = choice.finish_reason.as_ref().map(|reason| {
            match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(),
            }
        });

        OllamaResponse {
            model: unified_res.model,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_request_to_ollama_request() {
        let unified_req = UnifiedRequest {
            model: Some("test-model".to_string()),
            messages: vec![
                UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![UnifiedContentPart::Text { text: "You are a bot.".to_string() }],
                },
                UnifiedMessage {
                    role: UnifiedRole::User,
                    content: vec![UnifiedContentPart::Text { text: "Hello".to_string() }],
                },
            ],
            stream: true,
            temperature: Some(0.8),
            max_tokens: Some(100),
            top_p: Some(0.9),
            stop: Some(vec!["\n".to_string()]),
            seed: Some(123),
            presence_penalty: Some(0.5),
            frequency_penalty: Some(0.6),
            tools: None,
            ..Default::default()
        };

        let ollama_req: OllamaRequestPayload = unified_req.into();

        assert_eq!(ollama_req.model, "test-model");
        assert_eq!(ollama_req.messages.len(), 2);
        assert_eq!(ollama_req.messages[0].role, "system");
        assert_eq!(ollama_req.messages[0].content, "You are a bot.");
        assert_eq!(ollama_req.messages[1].role, "user");
        assert_eq!(ollama_req.messages[1].content, "Hello");
        assert_eq!(ollama_req.stream, Some(true));
        let options = ollama_req.options.unwrap();
        assert_eq!(options.temperature, Some(0.8));
        assert_eq!(options.max_tokens, Some(100));
        assert_eq!(options.top_p, Some(0.9));
        assert_eq!(options.stop, Some(vec!["\n".to_string()]));
        assert_eq!(options.seed, Some(123));
        assert_eq!(options.presence_penalty, Some(0.5));
        assert_eq!(options.frequency_penalty, Some(0.6));
    }

    #[test]
    fn test_ollama_request_to_unified_request() {
        let ollama_req = OllamaRequestPayload {
            model: "test-model".to_string(),
            messages: vec![
                OllamaMessage {
                    role: "system".to_string(),
                    content: "You are a bot.".to_string(),
                    images: None,
                },
                OllamaMessage {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                    images: None,
                },
            ],
            stream: Some(true),
            options: Some(OllamaOptions {
                temperature: Some(0.8),
                max_tokens: Some(100),
                top_p: Some(0.9),
                stop: Some(vec!["\n".to_string()]),
                seed: Some(123),
                presence_penalty: Some(0.5),
                frequency_penalty: Some(0.6),
            }),
            format: None,
            keep_alive: None,
        };

        let unified_req: UnifiedRequest = ollama_req.into();

        assert_eq!(unified_req.model, Some("test-model".to_string()));
        assert_eq!(unified_req.messages.len(), 2);
        assert_eq!(unified_req.messages[0].role, UnifiedRole::System);
        assert_eq!(
            unified_req.messages[0].content,
            vec![UnifiedContentPart::Text { text: "You are a bot.".to_string() }]
        );
        assert_eq!(unified_req.messages[1].role, UnifiedRole::User);
        assert_eq!(
            unified_req.messages[1].content,
            vec![UnifiedContentPart::Text { text: "Hello".to_string() }]
        );
        assert_eq!(unified_req.stream, true);
        assert_eq!(unified_req.temperature, Some(0.8));
        assert_eq!(unified_req.max_tokens, Some(100));
        assert_eq!(unified_req.top_p, Some(0.9));
        assert_eq!(unified_req.stop, Some(vec!["\n".to_string()]));
        assert_eq!(unified_req.seed, Some(123));
        assert_eq!(unified_req.presence_penalty, Some(0.5));
        assert_eq!(unified_req.frequency_penalty, Some(0.6));
    }

    #[test]
    fn test_ollama_response_to_unified_response() {
        let ollama_res = OllamaResponse {
            model: "test-model".to_string(),
            created_at: "2023-12-12T18:34:13.014Z".to_string(),
            message: OllamaMessage {
                role: "assistant".to_string(),
                content: "Hello there!".to_string(),
                images: None,
            },
            done: true,
            done_reason: Some("stop".to_string()),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            total_duration: None,
            load_duration: None,
            prompt_eval_duration: None,
            eval_duration: None,
        };

        let unified_res: UnifiedResponse = ollama_res.into();

        assert_eq!(unified_res.model, "test-model");
        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.index, 0);
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(
            choice.message.content,
            vec![UnifiedContentPart::Text { text: "Hello there!".to_string() }]
        );
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
        let usage = unified_res.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn test_unified_response_to_ollama_response() {
        let unified_res = UnifiedResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text { text: "Hello there!".to_string() }],
                },
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            }),
            created: Some(12345),
            object: Some("chat.completion.chunk".to_string()),
            system_fingerprint: None,
        };

        let ollama_res: OllamaResponse = unified_res.into();

        assert_eq!(ollama_res.model, "test-model");
        assert_eq!(ollama_res.message.role, "assistant");
        assert_eq!(ollama_res.message.content, "Hello there!");
        assert!(ollama_res.done);
        assert_eq!(ollama_res.prompt_tokens, Some(10));
        assert_eq!(ollama_res.completion_tokens, Some(5));
    }

    #[test]
    fn test_ollama_chunk_to_unified_chunk() {
        // Content chunk
        let ollama_chunk = OllamaChunkResponse {
            model: "llama2".to_string(),
            created_at: "2023-12-12T18:34:13.014Z".to_string(),
            message: Some(OllamaMessage {
                role: "assistant".to_string(),
                content: "Hello".to_string(),
                images: None,
            }),
            done: false,
            done_reason: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_duration: None,
            load_duration: None,
            prompt_eval_duration: None,
            eval_duration: None,
        };

        let unified_chunk: UnifiedChunkResponse = ollama_chunk.into();

        assert_eq!(unified_chunk.model, "llama2");
        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.index, 0);
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
        assert_eq!(choice.delta.content, vec![UnifiedContentPartDelta::TextDelta { index: 0, text: "Hello".to_string() }]);
        assert!(choice.finish_reason.is_none());
        assert!(unified_chunk.usage.is_none());

        // Final chunk
        let ollama_final_chunk = OllamaChunkResponse {
            model: "llama2".to_string(),
            created_at: "2023-12-12T18:34:13.014Z".to_string(),
            message: None,
            done: true,
            done_reason: Some("stop".to_string()),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            total_duration: None,
            load_duration: None,
            prompt_eval_duration: None,
            eval_duration: None,
        };

        let unified_final_chunk: UnifiedChunkResponse = ollama_final_chunk.into();
        assert_eq!(unified_final_chunk.model, "llama2");
        assert_eq!(unified_final_chunk.choices.len(), 1);
        let final_choice = &unified_final_chunk.choices[0];
        assert!(final_choice.delta.role.is_none());
        assert!(final_choice.delta.content.is_empty());
        assert_eq!(final_choice.finish_reason, Some("stop".to_string()));
        let usage = unified_final_chunk.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn test_unified_chunk_to_ollama_chunk() {
        // Content chunk
        let unified_chunk = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![UnifiedContentPartDelta::TextDelta { index: 0, text: " World".to_string() }],
                },
                finish_reason: None,
            }],
            usage: None,
            created: Some(12345),
            object: Some("chat.completion.chunk".to_string()),
        };

        let ollama_chunk: OllamaChunkResponse = unified_chunk.into();

        assert_eq!(ollama_chunk.model, "test-model");
        assert!(!ollama_chunk.done);
        let message = ollama_chunk.message.unwrap();
        assert_eq!(message.role, "assistant");
        assert_eq!(message.content, " World");
        assert!(message.images.is_none());
        assert!(ollama_chunk.prompt_tokens.is_none());
        assert!(ollama_chunk.completion_tokens.is_none());

        // Final chunk
        let unified_final_chunk = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: "test-model".to_string(),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta::default(),
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            }),
            created: Some(12345),
            object: Some("chat.completion.chunk".to_string()),
        };

        let ollama_final_chunk: OllamaChunkResponse = unified_final_chunk.into();
        assert_eq!(ollama_final_chunk.model, "test-model");
        assert!(ollama_final_chunk.done);
        assert!(ollama_final_chunk.message.is_none());
        assert_eq!(ollama_final_chunk.prompt_tokens, Some(10));
        assert_eq!(ollama_final_chunk.completion_tokens, Some(5));
    }
}

// --- Ollama Chunk Response ---

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaChunkResponse {
    pub model: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<OllamaMessage>,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_reason: Option<String>,
    // Usage stats are only in the final chunk
    #[serde(rename = "prompt_eval_count")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(rename = "eval_count")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_duration: Option<u64>,
}

impl From<OllamaChunkResponse> for UnifiedChunkResponse {
    fn from(ollama_chunk: OllamaChunkResponse) -> Self {
        let delta = if let Some(message) = ollama_chunk.message {
            UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![UnifiedContentPartDelta::TextDelta { index: 0, text: message.content }],
            }
        } else {
            UnifiedMessageDelta::default()
        };

        let finish_reason = if ollama_chunk.done {
            ollama_chunk.done_reason.or_else(|| Some("stop".to_string()))
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
            model: ollama_chunk.model,
            choices: vec![choice],
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
        }
    }
}

impl From<UnifiedChunkResponse> for OllamaChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let choice = unified_chunk
            .choices
            .into_iter()
            .next()
            .unwrap_or_else(|| UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta::default(),
                finish_reason: None,
            });

        let mut final_content = String::new();
        for part in choice.delta.content {
             if let UnifiedContentPartDelta::TextDelta { text, .. } = part {
                final_content.push_str(&text);
            }
        }

        let message = if !final_content.is_empty() {
            Some(OllamaMessage {
                role: "assistant".to_string(),
                content: final_content,
                images: None,
            })
        } else {
            None
        };

        let (prompt_tokens, completion_tokens) = if let Some(usage) = unified_chunk.usage {
            (Some(usage.input_tokens), Some(usage.output_tokens))
        } else {
            (None, None)
        };

        let done_reason = choice.finish_reason.as_ref().map(|reason| {
            match reason.as_str() {
                "stop" => "stop".to_string(),
                "length" => "length".to_string(),
                _ => "stop".to_string(),
            }
        });

        OllamaChunkResponse {
            model: unified_chunk.model,
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
