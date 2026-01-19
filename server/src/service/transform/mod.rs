use cyder_tools::log::{debug, error};
use serde_json::Value;

use crate::controller::llm_types::LlmApiType;
use crate::utils::sse::SseEvent;

pub mod openai;
pub mod gemini;
pub mod ollama;
pub mod anthropic;
pub mod unified;
use unified::*;

pub fn transform_request_data(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    is_stream: bool,
) -> Value {
    if api_type == target_api_type {
        return data;
    }

    debug!(
        "[transform] API type mismatch. Incoming: {:?}, Target: {:?}. Transforming request body.",
        api_type, target_api_type
    );

    macro_rules! deserialize {
        ($type:ty, $name:expr) => {
            match serde_json::from_value::<$type>(data.clone()) {
                Ok(payload) => payload.into(),
                Err(e) => {
                    error!(
                        "[transform] Failed to deserialize {} request: {}. Returning original data.",
                        $name, e
                    );
                    return data;
                }
            }
        };
    }

    // Step 1: Deserialize to UnifiedRequest
    let mut unified_request: UnifiedRequest = match api_type {
        LlmApiType::OpenAI => deserialize!(openai::OpenAiRequestPayload, "OpenAI"),
        LlmApiType::Gemini => deserialize!(gemini::GeminiRequestPayload, "Gemini"),
        LlmApiType::Ollama => deserialize!(ollama::OllamaRequestPayload, "Ollama"),
        LlmApiType::Anthropic => deserialize!(anthropic::AnthropicRequestPayload, "Anthropic"),
    };

    // The `is_stream` from the request URL is the source of truth.
    unified_request.stream = is_stream;
    
    // Warn if top_k is used with non-Anthropic targets
    if unified_request.top_k.is_some() && target_api_type != LlmApiType::Anthropic {
        debug!(
            "[transform] Warning: top_k parameter is set but target API ({:?}) may not support it. This is primarily an Anthropic feature.",
            target_api_type
        );
    }
    
    // Warn if tools are used with Ollama
    if unified_request.tools.is_some() && target_api_type == LlmApiType::Ollama {
        debug!(
            "[transform] Warning: tools/function calling is not supported by Ollama. Tool definitions will be dropped."
        );
    }

    debug!("[transform] unified request: {:?}", unified_request);

    macro_rules! serialize {
        ($type:ty) => {{
            let payload: $type = unified_request.into();
            serde_json::to_value(payload)
        }};
    }

    // Step 2: Serialize from UnifiedRequest to target format
    let target_payload_result = match target_api_type {
        LlmApiType::OpenAI => serialize!(openai::OpenAiRequestPayload),
        LlmApiType::Gemini => serialize!(gemini::GeminiRequestPayload),
        LlmApiType::Ollama => serialize!(ollama::OllamaRequestPayload),
        LlmApiType::Anthropic => serialize!(anthropic::AnthropicRequestPayload),
    };

    match target_payload_result {
        Ok(value) => {
            debug!(
                "[transform] Transformation complete. Result: {}",
                serde_json::to_string(&value).unwrap_or_default()
            );
            value
        }
        Err(e) => {
            error!(
                "[transform] Failed to serialize to target request format: {}. Returning original data.",
                e
            );
            data
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn test_transform_request_data_openai_to_gemini_basic() {
        let openai_request = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "What is the weather in Boston?"}
            ],
            "temperature": 0.5,
            "max_tokens": 100,
            "top_p": 0.9,
            "stop": "stop_word"
        });

        let transformed = transform_request_data(
            openai_request,
            LlmApiType::OpenAI,
            LlmApiType::Gemini,
            false,
        );

        let expected_gemini_request = json!({
            "system_instruction": {
                "parts": [{"text": "You are a helpful assistant."}]
            },
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What is the weather in Boston?"}]
                }
            ],
            "generationConfig": {
                "temperature": 0.5,
                "maxOutputTokens": 100,
                "topP": 0.9,
                "stopSequences": ["stop_word"]
            }
        });

        assert_eq!(transformed, expected_gemini_request);
    }

    #[test]
    fn test_transform_request_data_openai_to_gemini_with_tools() {
        let openai_request = json!({
            "model": "gpt-4-turbo",
            "messages": [
                {"role": "user", "content": "What is the weather in Boston?"},
                {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "get_current_weather",
                                "arguments": "{\"location\": \"Boston, MA\"}"
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_123",
                    "name": "get_current_weather",
                    "content": "{\"temperature\": 22, \"unit\": \"celsius\"}"
                }
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_current_weather",
                        "description": "Get the current weather in a given location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {
                                    "type": "string",
                                    "description": "The city and state, e.g. San Francisco, CA"
                                }
                            },
                            "required": ["location"]
                        }
                    }
                }
            ]
        });

        let transformed = transform_request_data(
            openai_request,
            LlmApiType::OpenAI,
            LlmApiType::Gemini,
            false,
        );

        let expected_gemini_request = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What is the weather in Boston?"}]
                },
                {
                    "role": "model",
                    "parts": [
                        {
                            "functionCall": {
                                "name": "get_current_weather",
                                "args": {
                                    "location": "Boston, MA"
                                }
                            }
                        }
                    ]
                },
                {
                    "role": "user", // Gemini uses 'user' role for function responses
                    "parts": [
                        {
                            "functionResponse": {
                                "name": "get_current_weather",
                                "response": {
                                    "temperature": 22,
                                    "unit": "celsius"
                                }
                            }
                        }
                    ]
                }
            ],
            "tools": [
                {
                    "functionDeclarations": [
                        {
                            "name": "get_current_weather",
                            "description": "Get the current weather in a given location",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "location": {
                                        "type": "string",
                                        "description": "The city and state, e.g. San Francisco, CA"
                                    }
                                },
                                "required": ["location"]
                            }
                        }
                    ]
                }
            ]
        });

        assert_eq!(transformed, expected_gemini_request);
    }

    #[test]
    fn test_transform_request_data_gemini_to_openai_basic() {
        let gemini_request = json!({
            "system_instruction": {
                "parts": [{"text": "You are a helpful assistant."}]
            },
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What is the weather in Boston?"}]
                }
            ],
            "generationConfig": {
                "temperature": 0.5,
                "maxOutputTokens": 100,
                "topP": 0.9,
                "stopSequences": ["stop_word"]
            }
        });

        let transformed = transform_request_data(
            gemini_request,
            LlmApiType::Gemini,
            LlmApiType::OpenAI,
            true, // is_stream
        );

        let expected_openai_request = json!({
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "What is the weather in Boston?"}
            ],
            "temperature": 0.5,
            "max_tokens": 100,
            "top_p": 0.9,
            "stop": "stop_word",
            "stream": true
        });

        assert_eq!(transformed, expected_openai_request);
    }

    #[test]
    fn test_transform_request_data_gemini_to_openai_with_tools() {
        let gemini_request = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What is the weather in Boston?"}]
                },
                {
                    "role": "model",
                    "parts": [
                        {
                            "functionCall": {
                                "name": "get_current_weather",
                                "args": { "location": "Boston, MA" }
                            }
                        }
                    ]
                },
                {
                    "role": "user", // Gemini expects tool responses to have 'user' role
                    "parts": [
                        {
                            "functionResponse": {
                                "name": "get_current_weather",
                                "response": {
                                    "result": "{\"temperature\": 22, \"unit\": \"celsius\"}"
                                }
                            }
                        }
                    ]
                }
            ],
            "tools": [
                {
                    "functionDeclarations": [
                        {
                            "name": "get_current_weather",
                            "description": "Get the current weather in a given location",
                            "parameters": {
                                "type": "OBJECT",
                                "properties": {
                                    "location": {
                                        "type": "STRING"
                                    }
                                }
                            }
                        }
                    ]
                }
            ]
        });

        let transformed = transform_request_data(
            gemini_request,
            LlmApiType::Gemini,
            LlmApiType::OpenAI,
            false,
        );

        let mut transformed_obj = transformed.as_object().unwrap().clone();
        let messages = transformed_obj
            .get_mut("messages")
            .unwrap()
            .as_array_mut()
            .unwrap();

        let generated_id;

        // Scope the first mutable borrow to find the assistant message, check the generated ID,
        // and replace it with a fixed value for the final assertion.
        {
            let assistant_message = messages
                .iter_mut()
                .find(|m| m["role"] == "assistant")
                .unwrap();
            let tool_calls = assistant_message
                .get_mut("tool_calls")
                .unwrap()
                .as_array_mut()
                .unwrap();
            let tool_call = tool_calls.get_mut(0).unwrap().as_object_mut().unwrap();
            generated_id = tool_call.get("id").unwrap().as_str().unwrap().to_string();
            assert!(generated_id.starts_with("call_"));
            tool_call.insert("id".to_string(), json!("FIXED_ID_FOR_TEST"));
        }

        // Scope the second mutable borrow to find the tool message, check its ID,
        // and replace it with a fixed value.
        {
            let tool_message = messages
                .iter_mut()
                .find(|m| m["role"] == "tool")
                .unwrap()
                .as_object_mut()
                .unwrap();
            let tool_message_id = tool_message.get("tool_call_id").unwrap().as_str().unwrap();
            assert_eq!(generated_id, tool_message_id);
            tool_message.insert("tool_call_id".to_string(), json!("FIXED_ID_FOR_TEST"));
        }

        let transformed_back_to_value = serde_json::to_value(transformed_obj).unwrap();

        let expected_openai_request = json!({
            "messages": [
                {"role": "user", "content": "What is the weather in Boston?"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "FIXED_ID_FOR_TEST",
                            "type": "function",
                            "function": {
                                "name": "get_current_weather",
                                "arguments": "{\"location\":\"Boston, MA\"}"
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "FIXED_ID_FOR_TEST",
                    "name": "get_current_weather",
                    "content": "{\"temperature\": 22, \"unit\": \"celsius\"}"
                }
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_current_weather",
                        "description": "Get the current weather in a given location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {
                                    "type": "string"
                                }
                            }
                        }
                    }
                }
            ],
            "stream": false
        });

        assert_eq!(transformed_back_to_value, expected_openai_request);
    }

    #[test]
    fn test_transform_result_chunk_openai_to_gemini() {
        let mut transformer = StreamTransformer::new(LlmApiType::OpenAI, LlmApiType::Gemini);

        // Test case 1: Content chunk
        let openai_data_content = "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}";
        let event = SseEvent {
            data: openai_data_content.to_string(),
            ..Default::default()
        };
        let transformed_events = transformer.transform_event(event).unwrap();
        assert_eq!(transformed_events.len(), 1);
        let transformed_json: Value = serde_json::from_str(&transformed_events[0].data).unwrap();

        let expected_json = json!({
            "candidates": [{
                "index": 0,
                "content": {
                    "parts": [{"text": "Hello"}],
                    "role": "model"
                }
            }]
        });
        assert_eq!(transformed_json, expected_json);

        // Test case 2: Finish reason chunk
        let openai_data_finish = "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}";
        let event_finish = SseEvent {
            data: openai_data_finish.to_string(),
            ..Default::default()
        };
        let transformed_events_finish = transformer.transform_event(event_finish).unwrap();
        assert_eq!(transformed_events_finish.len(), 1);
        let transformed_json_finish: Value =
            serde_json::from_str(&transformed_events_finish[0].data).unwrap();

        assert_eq!(
            transformed_json_finish["candidates"][0]["finishReason"],
            "STOP"
        );
        assert!(transformed_json_finish["candidates"][0]["safetyRatings"].is_array());

        // Test case 3: DONE chunk
        let openai_data_done = "[DONE]";
        let event_done = SseEvent {
            data: openai_data_done.to_string(),
            ..Default::default()
        };
        let transformed_done = transformer.transform_event(event_done);
        assert!(transformed_done.is_none());

        // Test case 4: Tool call chunk
        let openai_data_tool = "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_123\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"location\\\": \\\"Boston\\\"}\"}}]}}]}";
        let event_tool = SseEvent {
            data: openai_data_tool.to_string(),
            ..Default::default()
        };
        let transformed_events_tool = transformer.transform_event(event_tool).unwrap();
        assert_eq!(transformed_events_tool.len(), 1);
        let transformed_json_tool: Value =
            serde_json::from_str(&transformed_events_tool[0].data).unwrap();

        let expected_tool_json = json!({
            "candidates": [{
                "index": 0,
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "get_weather",
                            "args": {
                                "location": "Boston"
                            }
                        }
                    }]
                }
            }]
        });
        assert_eq!(transformed_json_tool, expected_tool_json);

        // Test case 5: Empty content chunk should be filtered out
        let openai_data_empty_content = "{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"}}]}";
        let event_empty = SseEvent {
            data: openai_data_empty_content.to_string(),
            ..Default::default()
        };
        let transformed_empty_content = transformer.transform_event(event_empty);
        assert!(transformed_empty_content.is_none());
    }

    #[test]
    fn test_transform_result_chunk_gemini_to_openai() {
        let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::OpenAI);

        // Test case 1: Content chunk
        let gemini_data_content = "{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" World\"}],\"role\":\"model\"},\"index\":0}]}";
        let event = SseEvent {
            data: gemini_data_content.to_string(),
            ..Default::default()
        };
        let transformed_events = transformer.transform_event(event).unwrap();
        assert_eq!(transformed_events.len(), 1);
        let transformed_json: Value = serde_json::from_str(&transformed_events[0].data).unwrap();

        assert_eq!(
            transformed_json["choices"][0]["delta"]["content"],
            " World"
        );
        assert_eq!(transformed_json["choices"][0]["index"], 0);
        assert_eq!(transformed_json["object"], "chat.completion.chunk");

        // Test case 2: Finish reason chunk
        let gemini_data_finish = "{\"candidates\":[{\"finishReason\":\"STOP\",\"index\":0}]}";
        let event_finish = SseEvent {
            data: gemini_data_finish.to_string(),
            ..Default::default()
        };
        let transformed_events_finish = transformer.transform_event(event_finish).unwrap();
        assert_eq!(transformed_events_finish.len(), 1);
        let transformed_json_finish: Value =
            serde_json::from_str(&transformed_events_finish[0].data).unwrap();

        assert_eq!(
            transformed_json_finish["choices"][0]["finish_reason"],
            "stop"
        );
        assert!(transformed_json_finish["choices"][0]["delta"]
            .as_object()
            .unwrap()
            .is_empty());

        // Test case 3: Function call chunk
        let gemini_data_tool = "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"location\":\"Boston\"}}}]},\"index\":0}]}";
        let event_tool = SseEvent {
            data: gemini_data_tool.to_string(),
            ..Default::default()
        };
        let transformed_events_tool = transformer.transform_event(event_tool).unwrap();
        assert_eq!(transformed_events_tool.len(), 1);
        let mut transformed_json_tool: Value =
            serde_json::from_str(&transformed_events_tool[0].data).unwrap();

        // The ID is generated, so we need to extract it and then compare
        let tool_call = transformed_json_tool["choices"][0]["delta"]["tool_calls"][0]
            .as_object_mut()
            .unwrap();
        let id = tool_call.get("id").unwrap().as_str().unwrap().to_string();
        tool_call.insert("id".to_string(), json!("FIXED_ID_FOR_TEST"));

        assert!(id.starts_with("call_"));
        let mut tool_call_delta = serde_json::Map::new();
        if let Some(role) = transformed_json_tool["choices"][0]["delta"].get("role") {
            tool_call_delta.insert("role".to_string(), role.clone());
        }
        if let Some(tcs) = transformed_json_tool["choices"][0]["delta"].get("tool_calls") {
            tool_call_delta.insert("tool_calls".to_string(), tcs.clone());
        }

        let expected_tool_json = json!({
            "id": transformed_json_tool["id"].clone(),
            "object": "chat.completion.chunk",
            "created": transformed_json_tool["created"].clone(),
            "model": "gemini-transformed-model",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": "FIXED_ID_FOR_TEST",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\":\"Boston\"}"
                        }
                    }]
                }
            }]
        });
        assert_eq!(transformed_json_tool, expected_tool_json);
    }

    #[test]
    fn test_transform_result_openai_to_gemini_basic() {
        let openai_result = json!({
          "id": "chatcmpl-123",
          "object": "chat.completion",
          "created": 1677652288,
          "model": "gpt-3.5-turbo-0125",
          "choices": [{
            "index": 0,
            "message": {
              "role": "assistant",
              "content": "Hello there! How can I help you today?"
            },
            "finish_reason": "stop"
          }],
          "usage": {
            "prompt_tokens": 9,
            "completion_tokens": 12,
            "total_tokens": 21
          }
        });

        let transformed = transform_result(
            openai_result,
            LlmApiType::OpenAI,
            LlmApiType::Gemini,
        );

        let expected_gemini_result = json!({
          "candidates": [
            {
              "index": 0,
              "content": {
                "parts": [
                  {
                    "text": "Hello there! How can I help you today?"
                  }
                ],
                "role": "model"
              },
              "finishReason": "STOP",
              "safetyRatings": [
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE" },
                { "category": "HARM_CATEGORY_HATE_SPEECH", "probability": "NEGLIGIBLE" },
                { "category": "HARM_CATEGORY_HARASSMENT", "probability": "NEGLIGIBLE" },
                { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "probability": "NEGLIGIBLE" }
              ]
            }
          ],
          "usageMetadata": {
            "promptTokenCount": 9,
            "candidatesTokenCount": 12,
            "totalTokenCount": 21
          }
        });

        assert_eq!(transformed, expected_gemini_result);
    }

    #[test]
    fn test_transform_result_gemini_to_openai_basic() {
        let gemini_result = json!({
          "candidates": [
            {
              "index": 0,
              "content": {
                "parts": [
                  {
                    "text": "This is a test response from Gemini."
                  }
                ],
                "role": "model"
              },
              "finishReason": "STOP",
              "safetyRatings": [
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE" }
              ]
            }
          ],
          "usageMetadata": {
            "promptTokenCount": 10,
            "candidatesTokenCount": 8,
            "totalTokenCount": 18
          }
        });

        let transformed = transform_result(
            gemini_result,
            LlmApiType::Gemini,
            LlmApiType::OpenAI,
        );

        let mut transformed_obj = transformed.as_object().unwrap().clone();
        assert!(transformed_obj.get("id").unwrap().as_str().unwrap().starts_with("chatcmpl-"));
        assert!(transformed_obj.get("created").unwrap().is_number());
        transformed_obj.remove("id");
        transformed_obj.remove("created");

        let expected_openai_result = json!({
          "object": "chat.completion",
          "model": "gemini-transformed-model",
          "choices": [
            {
              "index": 0,
              "message": {
                "role": "assistant",
                "content": "This is a test response from Gemini."
              },
              "finish_reason": "stop"
            }
          ],
          "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 8,
            "total_tokens": 18
          }
        });

        assert_eq!(serde_json::to_value(transformed_obj).unwrap(), expected_openai_result);
    }

    #[test]
    fn test_transform_result_openai_to_gemini_with_tools() {
        let openai_result = json!({
          "id": "chatcmpl-123",
          "object": "chat.completion",
          "created": 1677652288,
          "model": "gpt-3.5-turbo-0125",
          "choices": [{
            "index": 0,
            "message": {
              "role": "assistant",
              "content": null,
              "tool_calls": [
                {
                  "id": "call_abc",
                  "type": "function",
                  "function": {
                    "name": "get_current_weather",
                    "arguments": "{\"location\":\"Boston, MA\"}"
                  }
                }
              ]
            },
            "finish_reason": "tool_calls"
          }],
          "usage": {
            "prompt_tokens": 9,
            "completion_tokens": 12,
            "total_tokens": 21
          }
        });

        let transformed = transform_result(
            openai_result,
            LlmApiType::OpenAI,
            LlmApiType::Gemini,
        );

        let expected_gemini_result = json!({
          "candidates": [
            {
              "index": 0,
              "content": {
                "parts": [
                  {
                    "functionCall": {
                      "name": "get_current_weather",
                      "args": {
                        "location": "Boston, MA"
                      }
                    }
                  }
                ],
                "role": "model"
              },
              "finishReason": "TOOL_USE",
              "safetyRatings": [
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE" },
                { "category": "HARM_CATEGORY_HATE_SPEECH", "probability": "NEGLIGIBLE" },
                { "category": "HARM_CATEGORY_HARASSMENT", "probability": "NEGLIGIBLE" },
                { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "probability": "NEGLIGIBLE" }
              ]
            }
          ],
          "usageMetadata": {
            "promptTokenCount": 9,
            "candidatesTokenCount": 12,
            "totalTokenCount": 21
          }
        });

        assert_eq!(transformed, expected_gemini_result);
    }

    #[test]
    fn test_transform_result_gemini_to_openai_with_tools() {
        let gemini_result = json!({
          "candidates": [
            {
              "content": {
                "parts": [
                  {
                    "functionCall": {
                      "name": "get_current_weather",
                      "args": {
                        "location": "Boston, MA"
                      }
                    }
                  }
                ],
                "role": "model"
              },
              "finishReason": "TOOL_USE",
              "index": 0
            }
          ]
        });

        let transformed = transform_result(
            gemini_result,
            LlmApiType::Gemini,
            LlmApiType::OpenAI,
        );

        let mut transformed_obj = transformed.as_object().unwrap().clone();
        transformed_obj.remove("id");
        transformed_obj.remove("created");

        let choices = transformed_obj.get_mut("choices").unwrap().as_array_mut().unwrap();
        let message = choices[0].get_mut("message").unwrap().as_object_mut().unwrap();
        let tool_calls = message.get_mut("tool_calls").unwrap().as_array_mut().unwrap();
        let tool_call = tool_calls[0].as_object_mut().unwrap();
        assert!(tool_call.get("id").unwrap().as_str().unwrap().starts_with("call_"));
        tool_call.insert("id".to_string(), json!("FIXED_ID_FOR_TEST"));

        let expected_openai_result = json!({
          "object": "chat.completion",
          "model": "gemini-transformed-model",
          "choices": [
            {
              "index": 0,
              "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                  {
                    "id": "FIXED_ID_FOR_TEST",
                    "type": "function",
                    "function": {
                      "name": "get_current_weather",
                      "arguments": "{\"location\":\"Boston, MA\"}"
                    }
                  }
                ]
              },
              "finish_reason": "tool_calls"
            }
          ]
        });

        assert_eq!(serde_json::to_value(transformed_obj).unwrap(), expected_openai_result);
    }

    #[test]
    fn test_transform_result_gemini_to_openai_with_tools_and_stop_reason() {
        let gemini_result = json!({
          "candidates": [
            {
              "content": {
                "parts": [
                  {
                    "functionCall": {
                      "name": "get_current_weather",
                      "args": {
                        "location": "Boston, MA"
                      }
                    }
                  }
                ],
                "role": "model"
              },
              "finishReason": "STOP", // Key difference: STOP instead of TOOL_USE
              "index": 0
            }
          ]
        });

        let transformed = transform_result(
            gemini_result,
            LlmApiType::Gemini,
            LlmApiType::OpenAI,
        );

        let mut transformed_obj = transformed.as_object().unwrap().clone();
        transformed_obj.remove("id");
        transformed_obj.remove("created");

        let choices = transformed_obj.get_mut("choices").unwrap().as_array_mut().unwrap();
        let message = choices[0].get_mut("message").unwrap().as_object_mut().unwrap();
        let tool_calls = message.get_mut("tool_calls").unwrap().as_array_mut().unwrap();
        let tool_call = tool_calls[0].as_object_mut().unwrap();
        assert!(tool_call.get("id").unwrap().as_str().unwrap().starts_with("call_"));
        tool_call.insert("id".to_string(), json!("FIXED_ID_FOR_TEST"));

        let expected_openai_result = json!({
          "object": "chat.completion",
          "model": "gemini-transformed-model",
          "choices": [
            {
              "index": 0,
              "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                  {
                    "id": "FIXED_ID_FOR_TEST",
                    "type": "function",
                    "function": {
                      "name": "get_current_weather",
                      "arguments": "{\"location\":\"Boston, MA\"}"
                    }
                  }
                ]
              },
              "finish_reason": "tool_calls" // Should be tool_calls because a tool was called
            }
          ]
        });

        assert_eq!(serde_json::to_value(transformed_obj).unwrap(), expected_openai_result);
    }

    #[test]
    fn test_transform_request_data_no_op() {
        let openai_request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let transformed = transform_request_data(
            openai_request.clone(),
            LlmApiType::OpenAI,
            LlmApiType::OpenAI,
            false,
        );

        assert_eq!(openai_request, transformed);
    }

    #[test]
    fn test_transform_result_on_deserialization_error() {
        let malformed_openai_result = json!({
            "id": "chatcmpl-123",
            "choices": "this should be an array"
        });

        let transformed = transform_result(
            malformed_openai_result.clone(),
            LlmApiType::OpenAI,
            LlmApiType::Gemini,
        );

        // On error, the original data should be returned
        assert_eq!(transformed, malformed_openai_result);
    }

}

pub fn transform_result(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Value {
    if api_type == target_api_type {
        return data;
    }

    debug!(
        "[transform_result] API type mismatch. Incoming: {:?}, Target: {:?}. Transforming response body.",
        api_type, target_api_type
    );

    // Step 1: Deserialize to UnifiedResponse
    let unified_response_result: Result<UnifiedResponse, serde_json::Error> = match api_type {
        LlmApiType::OpenAI => serde_json::from_value::<openai::OpenAiResponse>(data.clone()).map(Into::into),
        LlmApiType::Gemini => serde_json::from_value::<gemini::GeminiResponse>(data.clone()).map(Into::into),
        LlmApiType::Ollama => serde_json::from_value::<ollama::OllamaResponse>(data.clone()).map(Into::into),
        LlmApiType::Anthropic => serde_json::from_value::<anthropic::AnthropicResponse>(data.clone()).map(Into::into),
    };

    let unified_response = match unified_response_result {
        Ok(ur) => ur,
        Err(e) => {
            error!(
                "[transform_result] Failed to deserialize to UnifiedResponse from {:?}: {}. Returning original data.",
                api_type, e
            );
            return data;
        }
    };

    // Step 2: Serialize from UnifiedResponse to target format
    let target_payload_result = match target_api_type {
        LlmApiType::OpenAI => serde_json::to_value(openai::OpenAiResponse::from(unified_response)),
        LlmApiType::Gemini => serde_json::to_value(gemini::GeminiResponse::from(unified_response)),
        LlmApiType::Ollama => serde_json::to_value(ollama::OllamaResponse::from(unified_response)),
        LlmApiType::Anthropic => serde_json::to_value(anthropic::AnthropicResponse::from(unified_response)),
    };

    match target_payload_result {
        Ok(value) => {
            debug!(
                "[transform_result] Transformation complete. Result: {}",
                serde_json::to_string(&value).unwrap_or_default()
            );
            value
        }
        Err(e) => {
            error!(
                "[transform_result] Failed to serialize to target response format: {}. Returning original data.",
                e
            );
            data
        }
    }
}

pub struct StreamTransformer {
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    pub is_first_chunk: bool,
    // Anthropic-specific state
    pub is_first_content_chunk: bool,
    // Cached stream ID for consistency across chunks
    stream_id: Option<String>,
}

impl StreamTransformer {
    pub fn new(api_type: LlmApiType, target_api_type: LlmApiType) -> Self {
        Self {
            api_type,
            target_api_type,
            is_first_chunk: true,
            is_first_content_chunk: true,
            stream_id: None,
        }
    }
    
    fn get_or_generate_stream_id(&mut self) -> String {
        if let Some(ref id) = self.stream_id {
            id.clone()
        } else {
            use crate::utils::ID_GENERATOR;
            let new_id = format!("chatcmpl-{}", ID_GENERATOR.generate_id());
            self.stream_id = Some(new_id.clone());
            new_id
        }
    }

    pub fn transform_event(&mut self, event: SseEvent) -> Option<Vec<SseEvent>> {
        if self.api_type == self.target_api_type {
            return Some(vec![event]);
        }

        // Handle OpenAI's stream termination marker
        if self.api_type == LlmApiType::OpenAI && event.data == "[DONE]" {
            // Gemini, Ollama, and Anthropic streams just end, so we return None to not send anything.
            return if self.target_api_type == LlmApiType::Gemini
                || self.target_api_type == LlmApiType::Ollama
                || self.target_api_type == LlmApiType::Anthropic
            {
                None
            } else {
                // Pass through for other potential targets
                Some(vec![event])
            };
        }

        if event.data.is_empty() {
            return None;
        }

        // Step 1: Deserialize to UnifiedChunkResponse
        let unified_chunk_result: Result<UnifiedChunkResponse, _> = match self.api_type {
            LlmApiType::OpenAI => serde_json::from_str::<openai::OpenAiChunkResponse>(&event.data)
                .map(|p| p.into()),
            LlmApiType::Gemini => serde_json::from_str::<gemini::GeminiChunkResponse>(&event.data)
                .map(|p| p.into()),
            LlmApiType::Ollama => serde_json::from_str::<ollama::OllamaChunkResponse>(&event.data)
                .map(|p| p.into()),
            LlmApiType::Anthropic => {
                let event_result: Result<anthropic::AnthropicEvent, _> =
                    serde_json::from_str(&event.data);
                event_result.map(|event| event.into())
            }
        };

        let mut unified_chunk = match unified_chunk_result {
            Ok(uc) => uc,
            Err(e) => {
                error!(
                    "[StreamTransformer::transform_event] Failed to deserialize chunk from {:?}: {}. Data: '{}'",
                    self.api_type, e, event.data
                );
                // Return an empty data chunk to avoid breaking client-side parsers.
                return Some(vec![SseEvent {
                    data: "{}".to_string(),
                    ..Default::default()
                }]);
            }
        };

        // Ensure consistent stream ID across all chunks
        let consistent_id = self.get_or_generate_stream_id();
        unified_chunk.id = consistent_id;

        // Step 2: Serialize from UnifiedChunkResponse to target format
        match self.target_api_type {
            LlmApiType::OpenAI => {
                let value =
                    serde_json::to_value(openai::OpenAiChunkResponse::from(unified_chunk)).ok()?;
                Some(vec![SseEvent {
                    data: serde_json::to_string(&value).unwrap_or_default(),
                    ..Default::default()
                }])
            }
            LlmApiType::Gemini => {
                let value =
                    serde_json::to_value(gemini::GeminiChunkResponse::from(unified_chunk)).ok()?;
                if let Some(candidates) = value.get("candidates").and_then(|c| c.as_array()) {
                    if candidates.is_empty() {
                        return None;
                    }
                }
                Some(vec![SseEvent {
                    data: serde_json::to_string(&value).unwrap_or_default(),
                    ..Default::default()
                }])
            }
            LlmApiType::Ollama => {
                let value =
                    serde_json::to_value(ollama::OllamaChunkResponse::from(unified_chunk)).ok()?;
                Some(vec![SseEvent {
                    data: serde_json::to_string(&value).unwrap_or_default(),
                    ..Default::default()
                }])
            }
            LlmApiType::Anthropic => {
                anthropic::transform_unified_chunk_to_anthropic_events(unified_chunk, self)
            }
        }
    }

}
