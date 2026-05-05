use super::*;
use crate::utils::usage::UsageInfo;
use serde_json::json;

#[test]
fn test_transform_request_data_no_op_returns_original_payload() {
    let openai_request = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let transformed = transform_request_data(
        openai_request.clone(),
        LlmApiType::Openai,
        LlmApiType::Openai,
        false,
    );

    assert_eq!(openai_request, transformed);
}

#[test]
fn test_transform_request_data_openai_to_gemini_facade_smoke() {
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
        LlmApiType::Openai,
        LlmApiType::Gemini,
        false,
    );

    assert_eq!(
        transformed,
        json!({
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
        })
    );
}

#[test]
fn test_transform_request_data_responses_to_openai_with_function_call_output() {
    let responses_request = json!({
        "model": "deepseek-ai/DeepSeek-V3.2",
        "input": [
            {"role": "user", "content": "Search for BoardMix"},
            {"role": "assistant", "content": "I will search for BoardMix.\n\n"},
            {
                "type": "function_call",
                "call_id": "call_123",
                "name": "search_web",
                "arguments": ""
            },
            {
                "type": "function_call_output",
                "call_id": "call_123",
                "output": "{\"error\":\"query is required\"}"
            }
        ],
        "stream": true
    });

    let transformed = transform_request_data(
        responses_request,
        LlmApiType::Responses,
        LlmApiType::Openai,
        true,
    );

    assert!(transformed.get("input").is_none());
    assert_eq!(transformed["messages"][0]["role"], json!("user"));
    assert_eq!(
        transformed["messages"][0]["content"],
        json!("Search for BoardMix")
    );
    assert_eq!(transformed["messages"][2]["role"], json!("assistant"));
    assert_eq!(
        transformed["messages"][2]["tool_calls"][0]["id"],
        json!("call_123")
    );
    assert_eq!(
        transformed["messages"][2]["tool_calls"][0]["function"]["name"],
        json!("search_web")
    );
    assert_eq!(transformed["messages"][3]["role"], json!("tool"));
    assert_eq!(
        transformed["messages"][3]["tool_call_id"],
        json!("call_123")
    );
    assert_eq!(
        transformed["messages"][3]["content"],
        json!("{\"error\":\"query is required\"}")
    );
}

#[test]
fn test_finalize_request_data_for_vertex_openai_applies_gemini_variant_policy() {
    let data = json!({
        "model": "gemini-2.5-pro",
        "messages": [{"role": "user", "content": "hello"}],
        "stream": true,
        "stream_options": {"include_usage": false},
        "parallel_tool_calls": true,
        "user": "user-123"
    });

    let finalized = finalize_request_data(
        data,
        LlmApiType::Openai,
        &ProviderType::VertexOpenai,
        "chat/completions",
    );

    assert_eq!(
        finalized,
        json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true,
            "stream_options": {"include_usage": true}
        })
    );
}

#[test]
fn test_transform_result_openai_to_gemini_facade_smoke() {
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

    let (transformed, usage_info) =
        transform_result(openai_result, LlmApiType::Openai, LlmApiType::Gemini);

    assert_eq!(
        transformed,
        json!({
          "candidates": [
            {
              "index": 0,
              "content": {
                "parts": [{"text": "Hello there! How can I help you today?"}],
                "role": "model"
              },
              "finishReason": "STOP"
            }
          ],
          "usageMetadata": {
            "promptTokenCount": 9,
            "candidatesTokenCount": 12,
            "totalTokenCount": 21,
            "promptTokensDetails": [{"modality": "TEXT", "tokenCount": 9}],
            "candidatesTokensDetails": [{"modality": "TEXT", "tokenCount": 12}]
          }
        })
    );
    assert_eq!(
        usage_info,
        Some(UsageInfo {
            input_tokens: 9,
            output_tokens: 12,
            total_tokens: 21,
            ..Default::default()
        })
    );
}

#[test]
fn test_transform_result_on_deserialization_error_returns_original_payload() {
    let malformed_openai_result = json!({
        "id": "chatcmpl-123",
        "choices": "this should be an array"
    });

    let (transformed, usage_info) = transform_result(
        malformed_openai_result.clone(),
        LlmApiType::Openai,
        LlmApiType::Gemini,
    );

    assert_eq!(transformed, malformed_openai_result);
    assert!(usage_info.is_none());
}
