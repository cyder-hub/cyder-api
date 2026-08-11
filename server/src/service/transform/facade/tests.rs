use super::*;
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::service::transform::{
    TransformFailureOrigin, TransformOutcomeKind, TransformPhase, TransformReasonCode,
};
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
        DownstreamProtocol::Openai,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("same-wire request passthrough must succeed");

    assert_eq!(openai_request, transformed.value);
    assert_eq!(
        transformed.summary.facts[0].outcome,
        TransformOutcomeKind::Passthrough
    );
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
        DownstreamProtocol::Openai,
        UpstreamProtocol::Gemini,
        false,
    )
    .expect("OpenAI to Gemini request transform must succeed")
    .value;

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
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        true,
    )
    .expect("Responses to OpenAI request transform must succeed")
    .value;

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
fn malformed_cross_protocol_request_is_an_explicit_downstream_input_failure() {
    let failure = transform_request_data(
        json!({"model": "gpt-4", "messages": "not-an-array"}),
        DownstreamProtocol::Openai,
        UpstreamProtocol::Gemini,
        false,
    )
    .expect_err("invalid downstream request shape must fail");

    assert_eq!(failure.origin, TransformFailureOrigin::DownstreamInput);
    assert_eq!(failure.phase, TransformPhase::RequestDecode);
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::InvalidProtocolShape
    );
}

#[test]
fn unsupported_request_capability_is_rejected_before_target_send() {
    let failure = transform_request_data(
        json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}],
            "tools": [{
                "type": "function",
                "function": {"name": "lookup", "parameters": {"type": "object"}}
            }]
        }),
        DownstreamProtocol::Openai,
        UpstreamProtocol::Ollama,
        false,
    )
    .expect_err("unsupported tool definitions must be rejected");

    assert_eq!(failure.origin, TransformFailureOrigin::TargetCapability);
    assert_eq!(failure.phase, TransformPhase::RequestEncode);
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::UnsupportedToolDefinitions
    );
    assert_eq!(
        failure.summary.facts[0].outcome,
        TransformOutcomeKind::ExplicitReject
    );
}

#[test]
fn rejection_after_diagnostic_detail_cap_still_fails_closed() {
    let input = (0..40)
        .map(|index| {
            json!({
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": format!("message {index}")}]
            })
        })
        .collect::<Vec<_>>();

    let failure = transform_request_data(
        json!({
            "model": "gpt-5",
            "input": input,
            "reasoning": {"effort": "medium", "summary": null}
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        false,
    )
    .expect_err("unsupported reasoning must reject even after diagnostic detail overflow");

    assert_eq!(failure.origin, TransformFailureOrigin::TargetCapability);
    assert_eq!(failure.phase, TransformPhase::RequestEncode);
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::UnsupportedReasoning
    );
    assert!(failure.summary.dropped_diagnostic_count > 0);
    assert_eq!(
        failure
            .summary
            .action_counts
            .get(&crate::service::transform::TransformAction::Reject),
        Some(&1)
    );
}

#[test]
fn registered_deterministic_text_downgrade_remains_sendable() {
    let transformed = transform_request_data(
        json!({
            "model": "gpt-4",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image_url",
                    "image_url": {
                        "url": "https://images.example.com/chart.png",
                        "detail": "high"
                    }
                }]
            }]
        }),
        DownstreamProtocol::Openai,
        UpstreamProtocol::Anthropic,
        false,
    )
    .expect("registered image reference text downgrade must remain sendable");

    assert_eq!(
        transformed.value["messages"][0]["content"],
        "image_url: https://images.example.com/chart.png\ndetail: high"
    );
    assert!(transformed.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMajor
            && fact.reason_code == TransformReasonCode::DeterministicTextDowngrade
    }));
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
        UpstreamProtocol::Openai,
        &UpstreamProfileType::VertexOpenai,
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

    let transformed = transform_result(
        openai_result,
        UpstreamProtocol::Openai,
        DownstreamProtocol::Gemini,
    )
    .expect("OpenAI to Gemini response transform must succeed");
    let (transformed, usage_info) = transformed.value;

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
fn test_transform_result_on_deserialization_error_is_explicit_failure() {
    let malformed_openai_result = json!({
        "id": "chatcmpl-123",
        "choices": "this should be an array"
    });

    let failure = transform_result(
        malformed_openai_result.clone(),
        UpstreamProtocol::Openai,
        DownstreamProtocol::Gemini,
    )
    .expect_err("malformed cross-protocol response must fail");

    assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
    assert_eq!(failure.phase, TransformPhase::ResponseDecode);
    assert_eq!(failure.reason_code, TransformReasonCode::SourceDecodeFailed);
    assert_eq!(failure.summary.total_fact_count, 1);
    assert_ne!(
        failure.summary.facts[0]
            .safe_summary
            .as_ref()
            .unwrap()
            .sha256,
        malformed_openai_result.to_string()
    );
}
