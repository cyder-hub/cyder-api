pub mod diagnostic;
pub mod extensions;
pub mod request;
pub mod response;
pub mod stream;
pub mod usage;

pub use diagnostic::*;
pub use extensions::*;
pub use request::*;
pub use response::*;
pub use stream::*;
pub use usage::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_unified_request_core_and_extensions_round_trip() {
        let request = UnifiedRequest {
            model: Some("gpt-4.1".to_string()),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text {
                    text: "hello".to_string(),
                }],
            }],
            items: vec![UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                id: "call_1".to_string(),
                name: "lookup".to_string(),
                arguments: json!({"city": "Boston"}),
            })],
            tools: Some(vec![UnifiedTool {
                type_: "function".to_string(),
                function: UnifiedFunctionDefinition {
                    name: "lookup".to_string(),
                    description: Some("Finds weather".to_string()),
                    parameters: json!({"type": "object"}),
                },
            }]),
            stream: true,
            temperature: Some(0.2),
            max_tokens: Some(128),
            top_p: Some(0.9),
            stop: Some(vec!["DONE".to_string()]),
            seed: Some(7),
            presence_penalty: Some(0.1),
            frequency_penalty: Some(0.2),
            extensions: Some(UnifiedRequestExtensions {
                openai: Some(UnifiedOpenAiRequestExtension {
                    tool_choice: Some(json!("auto")),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        };

        let core = request.core();
        let (owned_core, extensions) = request.clone().into_core_and_extensions();

        assert_eq!(core.model, owned_core.model);
        assert_eq!(core.messages.len(), owned_core.messages.len());
        assert_eq!(core.messages[0].role, owned_core.messages[0].role);
        assert_eq!(core.messages[0].content, owned_core.messages[0].content);
        assert_eq!(core.items, owned_core.items);
        assert_eq!(core.stream, owned_core.stream);
        assert!(
            extensions
                .as_ref()
                .and_then(|ext| ext.openai.as_ref())
                .is_some()
        );

        let rebuilt = UnifiedRequest::from_core_and_extensions(owned_core, extensions);
        assert_eq!(rebuilt.model, request.model);
        assert_eq!(rebuilt.messages.len(), request.messages.len());
        assert_eq!(rebuilt.messages[0].role, request.messages[0].role);
        assert_eq!(rebuilt.messages[0].content, request.messages[0].content);
        assert_eq!(rebuilt.items, request.items);
        assert_eq!(rebuilt.extensions.is_some(), request.extensions.is_some());
    }

    #[test]
    fn test_unified_response_and_chunk_layering_round_trip() {
        let response = UnifiedResponse {
            id: "resp_1".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "done".to_string(),
                    }],
                },
                items: vec![],
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            }),
            created: Some(1),
            object: Some("response".to_string()),
            system_fingerprint: Some("fp_123".to_string()),
            provider_response_metadata: Some(UnifiedProviderResponseMetadata {
                responses: Some(UnifiedResponsesResponseMetadata {
                    safety_identifier: Some("safe".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            synthetic_metadata: Some(UnifiedSyntheticMetadata {
                id: true,
                model: false,
                gemini_safety_ratings: false,
            }),
        };

        let response_core = response.core();
        let response_context = response.context();
        assert_eq!(response_core.id, "resp_1");
        assert_eq!(
            response_context
                .extensions
                .as_ref()
                .and_then(|ext| ext.openai.as_ref())
                .and_then(|openai| openai.system_fingerprint.as_deref()),
            Some("fp_123")
        );
        assert!(response_context.provider_metadata.is_some());
        assert!(response_context.synthetic_metadata.is_some());

        let rebuilt_response =
            UnifiedResponse::from_core_and_context(response_core, response_context);
        assert_eq!(rebuilt_response.system_fingerprint(), Some("fp_123"));
        assert!(rebuilt_response.provider_response_metadata().is_some());
        assert!(rebuilt_response.synthetic_metadata().is_some());

        let chunk = UnifiedChunkResponse {
            id: "chunk_1".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![UnifiedContentPartDelta::TextDelta {
                        index: 0,
                        text: "hi".to_string(),
                    }],
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                ..Default::default()
            }),
            created: Some(2),
            object: Some("response.chunk".to_string()),
            provider_session_metadata: Some(UnifiedProviderSessionMetadata {
                anthropic: Some(UnifiedAnthropicResponseMetadata {
                    role: Some("assistant".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            synthetic_metadata: Some(UnifiedSyntheticMetadata {
                id: false,
                model: true,
                gemini_safety_ratings: false,
            }),
        };

        let (chunk_core, chunk_context) = chunk.clone().into_core_and_context();
        assert_eq!(chunk_core.id, "chunk_1");
        assert!(chunk_context.provider_session_metadata.is_some());
        assert!(chunk_context.synthetic_metadata.is_some());

        let rebuilt_chunk = UnifiedChunkResponse::from_core_and_context(chunk_core, chunk_context);
        assert_eq!(rebuilt_chunk.model, chunk.model);
        assert!(rebuilt_chunk.provider_session_metadata().is_some());
        assert!(rebuilt_chunk.synthetic_metadata().is_some());
    }
}
