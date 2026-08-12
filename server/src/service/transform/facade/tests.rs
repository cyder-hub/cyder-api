use super::*;
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::service::transform::{
    TransformAction, TransformFailureOrigin, TransformOutcomeKind, TransformPhase,
    TransformReasonCode, TransformSemanticUnit,
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
fn ordinary_unknown_object_fields_are_silently_ignored_across_wires() {
    let cases = [
        (
            DownstreamProtocol::Openai,
            UpstreamProtocol::Gemini,
            json!({
                "model": "gpt-4",
                "messages": [{
                    "role": "user",
                    "content": "hello",
                    "vendor_message_extension": {"enabled": true}
                }],
                "vendor_request_extension": {"opaque": [1, 2, 3]}
            }),
        ),
        (
            DownstreamProtocol::Responses,
            UpstreamProtocol::Openai,
            json!({
                "model": "gpt-4",
                "input": "hello",
                "vendor_request_extension": {"enabled": true}
            }),
        ),
        (
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Openai,
            json!({
                "model": "claude",
                "messages": [{
                    "role": "user",
                    "content": "hello",
                    "vendor_message_extension": {"enabled": true}
                }],
                "max_tokens": 64,
                "vendor_request_extension": {"enabled": true}
            }),
        ),
        (
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Openai,
            json!({
                "contents": [{
                    "role": "user",
                    "parts": [{
                        "text": "hello",
                        "vendor_part_extension": {"enabled": true}
                    }],
                    "vendor_content_extension": true
                }],
                "vendor_request_extension": {"enabled": true}
            }),
        ),
    ];

    for (source, target, request) in cases {
        transform_request_data(request, source, target, false)
            .unwrap_or_else(|failure| panic!("{source:?}->{target:?}: {failure:?}"));
    }
}

#[test]
fn unknown_tagged_request_blocks_are_explicitly_rejected() {
    const PRIVATE_MARKER: &str = "unknown-tag-private-marker";
    let cases = [
        (
            DownstreamProtocol::Openai,
            UpstreamProtocol::Gemini,
            json!({
                "model": "gpt-4",
                "messages": [{
                    "role": "user",
                    "content": [{"type": "future_blob", "data": PRIVATE_MARKER}]
                }]
            }),
        ),
        (
            DownstreamProtocol::Responses,
            UpstreamProtocol::Openai,
            json!({
                "model": "gpt-4",
                "input": [{
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "future_blob", "data": PRIVATE_MARKER}]
                }]
            }),
        ),
        (
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Openai,
            json!({
                "model": "claude",
                "messages": [{
                    "role": "user",
                    "content": [{"type": "future_blob", "data": PRIVATE_MARKER}]
                }],
                "max_tokens": 64
            }),
        ),
        (
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Openai,
            json!({
                "contents": [{
                    "role": "user",
                    "parts": [{"futurePart": {"data": PRIVATE_MARKER}}]
                }]
            }),
        ),
    ];

    for (source, target, request) in cases {
        let failure = transform_request_data(request, source, target, false)
            .expect_err("unknown tagged content must fail closed");
        assert_eq!(failure.origin, TransformFailureOrigin::DownstreamInput);
        assert_eq!(
            failure.reason_code,
            TransformReasonCode::UnknownSemanticUnit
        );
        assert!(!format!("{failure:?}").contains(PRIVATE_MARKER));
    }
}

#[test]
fn registered_unrepresentable_gemini_reasoning_conflict_is_rejected_without_payloads() {
    const PRIVATE_MARKER: &str = "conflict-private-marker";
    let failure = transform_request_data(
        json!({
            "contents": [{"role": "user", "parts": [{"text": PRIVATE_MARKER}]}],
            "generationConfig": {
                "thinkingConfig": {"thinkingBudget": 1024}
            }
        }),
        DownstreamProtocol::Gemini,
        UpstreamProtocol::Openai,
        false,
    )
    .expect_err("registered strong conflicts must fail closed");
    assert_eq!(failure.origin, TransformFailureOrigin::DownstreamInput);
    assert_eq!(failure.phase, TransformPhase::RequestDecode);
    assert_eq!(
        failure.semantic_unit,
        TransformSemanticUnit::ReasoningContent
    );
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::UnsupportedReasoning
    );
    assert!(!format!("{failure:?}").contains(PRIVATE_MARKER));
}

#[test]
fn known_safe_field_loss_remains_sendable_with_controlled_loss_diagnostic() {
    let transformed = transform_request_data(
        json!({
            "model": "claude",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": "hello",
                    "cache_control": {"type": "ephemeral"}
                }]
            }],
            "max_tokens": 64
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("safe cache metadata loss should remain sendable");

    assert!(transformed.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Drop
            && fact.semantic_unit == TransformSemanticUnit::Metadata
    }));
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
fn ollama_rejects_audio_and_file_id_inputs_before_encoding() {
    let cases = [
        (
            DownstreamProtocol::Openai,
            json!({
                "model": "gpt",
                "messages": [{
                    "role": "user",
                    "content": [{
                        "type": "input_audio",
                        "input_audio": {"data": "AA==", "format": "wav"}
                    }]
                }]
            }),
            TransformSemanticUnit::AudioData,
        ),
        (
            DownstreamProtocol::Responses,
            json!({
                "model": "gpt",
                "input": [{
                    "role": "user",
                    "content": [{
                        "type": "input_file",
                        "filename": "report.pdf",
                        "file_id": "file_123"
                    }]
                }]
            }),
            TransformSemanticUnit::FileId,
        ),
    ];

    for (source, request, semantic_unit) in cases {
        let failure = transform_request_data(request, source, UpstreamProtocol::Ollama, false)
            .expect_err("Ollama must reject media that its encoder cannot represent");
        assert_eq!(failure.origin, TransformFailureOrigin::TargetCapability);
        assert_eq!(failure.phase, TransformPhase::RequestEncode);
        assert_eq!(failure.semantic_unit, semantic_unit);
        assert_eq!(failure.reason_code, TransformReasonCode::UnsupportedContent);
    }
}

#[test]
fn rejection_after_diagnostic_detail_cap_still_fails_closed() {
    let mut input = (0..40)
        .map(|index| {
            json!({
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": format!("message {index}")}]
            })
        })
        .collect::<Vec<_>>();
    input.push(json!({
        "type": "message",
        "role": "user",
        "content": [{"type": "input_file", "file_url": "https://files.example.com/report.pdf"}]
    }));

    let failure = transform_request_data(
        json!({
            "model": "gpt-5",
            "input": input,
            "reasoning": {"effort": "medium", "summary": "auto"}
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        false,
    )
    .expect_err("unsupported file URL must reject even after diagnostic detail overflow");

    assert_eq!(failure.origin, TransformFailureOrigin::TargetCapability);
    assert_eq!(failure.phase, TransformPhase::RequestEncode);
    assert_eq!(failure.reason_code, TransformReasonCode::UnsupportedContent);
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
fn responses_reasoning_effort_maps_to_openai_and_summary_is_safely_ignored() {
    let transformed = transform_request_data(
        json!({
            "model": "gpt-5",
            "input": "hello",
            "reasoning": {"effort": "medium", "summary": "detailed"}
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("Responses qualitative reasoning must map to OpenAI");

    assert_eq!(transformed.value["reasoning_effort"], json!("medium"));
    assert!(transformed.value.get("reasoning").is_none());
    assert_eq!(transformed.value["messages"][0]["content"], json!("hello"));
    assert!(transformed.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Drop
            && fact.semantic_unit == TransformSemanticUnit::ReasoningContent
            && fact.reason_code == TransformReasonCode::UnsupportedReasoning
    }));
}

#[test]
fn anthropic_qualitative_reasoning_maps_to_openai_without_injecting_cot() {
    for (thinking, output_config, expected) in [
        (
            json!({"type": "adaptive", "display": "summarized"}),
            json!({"effort": "medium"}),
            "medium",
        ),
        (json!({"type": "adaptive"}), json!(null), "high"),
        (json!({"type": "disabled"}), json!(null), "none"),
        (
            json!({"type": "adaptive"}),
            json!({"effort": "max"}),
            "xhigh",
        ),
    ] {
        let transformed = transform_request_data(
            json!({
                "model": "claude",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 64,
                "thinking": thinking,
                "output_config": output_config
            }),
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Openai,
            false,
        )
        .expect("Anthropic qualitative reasoning must map to OpenAI");

        assert_eq!(transformed.value["reasoning_effort"], json!(expected));
        assert_eq!(transformed.value["messages"][0]["content"], json!("hello"));
    }

    let display = transform_request_data(
        json!({
            "model": "claude",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 64,
            "thinking": {"type": "adaptive", "display": "omitted"}
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("display preference must be safely ignored");
    assert!(display.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Drop
            && fact.reason_code == TransformReasonCode::UnsupportedReasoning
    }));

    let max = transform_request_data(
        json!({
            "model": "claude",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 64,
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": "max"}
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("max effort must use the closest OpenAI effort");
    assert!(max.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Synthesize
            && fact.reason_code == TransformReasonCode::UnsupportedReasoning
    }));
}

#[test]
fn anthropic_enabled_thinking_budget_maps_to_openai_with_audited_loss() {
    let transformed = transform_request_data(
        json!({
            "model": "claude",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 4096,
            "thinking": {"type": "enabled", "budget_tokens": 1024}
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("Anthropic numeric thinking budget must degrade to OpenAI effort");

    assert_eq!(transformed.value["reasoning_effort"], json!("high"));
    assert_eq!(transformed.value["max_tokens"], json!(4096));
    assert!(transformed.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Synthesize
            && fact.semantic_unit == TransformSemanticUnit::ReasoningContent
            && fact.reason_code == TransformReasonCode::UnsupportedReasoning
    }));
}

#[test]
fn gemini_reasoning_controls_map_to_openai_with_budget_sentinels() {
    for (thinking_config, expected) in [
        (
            json!({"thinkingLevel": "low", "includeThoughts": true}),
            Some("low"),
        ),
        (json!({"thinkingBudget": 0}), Some("none")),
        (json!({"thinkingBudget": -1}), None),
    ] {
        let expects_include_thoughts_loss =
            thinking_config.get("includeThoughts") == Some(&json!(true));
        let transformed = transform_request_data(
            json!({
                "contents": [{"role": "user", "parts": [{"text": "hello"}]}],
                "generationConfig": {"thinkingConfig": thinking_config}
            }),
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Openai,
            false,
        )
        .expect("Gemini reasoning control must map to OpenAI");

        assert_eq!(
            transformed
                .value
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
            expected
        );
        assert_eq!(transformed.value["messages"][0]["content"], json!("hello"));
        if expects_include_thoughts_loss {
            assert!(transformed.summary.facts.iter().any(|fact| {
                fact.outcome == TransformOutcomeKind::ControlledLossMinor
                    && fact.action == TransformAction::Drop
                    && fact.reason_code == TransformReasonCode::UnsupportedReasoning
            }));
        }
    }
}

#[test]
fn contradictory_anthropic_reasoning_controls_are_rejected() {
    let failure = transform_request_data(
        json!({
            "model": "claude",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 64,
            "thinking": {"type": "disabled"},
            "output_config": {"effort": "high"}
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect_err("disabled thinking and positive effort are contradictory");

    assert_eq!(failure.origin, TransformFailureOrigin::DownstreamInput);
    assert_eq!(
        failure.reason_code,
        TransformReasonCode::UnsupportedReasoning
    );
}

#[test]
fn responses_multimodal_input_maps_to_openai_image_audio_and_file_parts() {
    let transformed = transform_request_data(
        json!({
            "model": "gpt-5",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_image", "image_url": "https://example.com/image.png", "detail": "high"},
                    {"type": "input_image", "image_url": "data:image/png;base64,ZmFrZQ==", "detail": "auto"},
                    {"type": "input_audio", "input_audio": {"data": "UklGRg==", "format": "wav"}},
                    {"type": "input_file", "filename": "report.pdf", "file_data": "JVBERi0="},
                    {"type": "input_image", "file_id": "file_123", "detail": "auto"}
                ]
            }]
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("portable Responses multimodal input must map to OpenAI");

    let parts = transformed.value["messages"][0]["content"]
        .as_array()
        .expect("OpenAI multimodal content parts");
    assert_eq!(parts[0]["type"], json!("image_url"));
    assert_eq!(parts[0]["image_url"]["detail"], json!("high"));
    assert_eq!(
        parts[1]["image_url"]["url"],
        json!("data:image/png;base64,ZmFrZQ==")
    );
    assert_eq!(
        parts[2],
        json!({"type":"input_audio","input_audio":{"data":"UklGRg==","format":"wav"}})
    );
    assert_eq!(
        parts[3],
        json!({"type":"file","file":{"file_data":"JVBERi0=","filename":"report.pdf"}})
    );
    assert_eq!(
        parts[4],
        json!({"type":"file","file":{"file_id":"file_123"}})
    );
    assert!(transformed.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Synthesize
            && fact.semantic_unit == TransformSemanticUnit::FileId
    }));
}

#[test]
fn anthropic_multimodal_input_maps_to_openai_without_textualizing_payloads() {
    let transformed = transform_request_data(
        json!({
            "model": "claude",
            "max_tokens": 64,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "source": {"type": "base64", "media_type": "image/png", "data": "ZmFrZQ=="},
                        "cache_control": {"type": "ephemeral"}
                    },
                    {
                        "type": "image",
                        "source": {"type": "url", "url": "https://example.com/image.webp"}
                    },
                    {
                        "type": "document",
                        "source": {"type": "base64", "media_type": "application/pdf", "data": "JVBERi0="},
                        "title": "report.pdf"
                    }
                ]
            }]
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("portable Anthropic multimodal input must map to OpenAI");

    let parts = transformed.value["messages"][0]["content"]
        .as_array()
        .expect("OpenAI multimodal content parts");
    assert_eq!(
        parts[0]["image_url"]["url"],
        json!("data:image/png;base64,ZmFrZQ==")
    );
    assert_eq!(
        parts[1]["image_url"]["url"],
        json!("https://example.com/image.webp")
    );
    assert_eq!(
        parts[2],
        json!({"type":"file","file":{"file_data":"JVBERi0=","filename":"report.pdf"}})
    );
    assert!(!transformed.value.to_string().contains("file_data: "));
    assert!(transformed.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Drop
            && fact.semantic_unit == TransformSemanticUnit::Metadata
    }));
}

#[test]
fn gemini_multimodal_input_classifies_inline_media_for_openai() {
    let transformed = transform_request_data(
        json!({
            "contents": [{
                "role": "user",
                "parts": [
                    {"inlineData": {"mimeType": "image/jpeg", "data": "ZmFrZQ=="}},
                    {"inlineData": {"mimeType": "audio/mpeg", "data": "SUQz", "displayName": "voice.mp3"}},
                    {"inlineData": {"mimeType": "application/pdf", "data": "JVBERi0=", "displayName": "report.pdf"}},
                    {"fileData": {"mimeType": "image/png", "fileUri": "https://example.com/image.png", "displayName": "ignored.png"}}
                ]
            }]
        }),
        DownstreamProtocol::Gemini,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("portable Gemini multimodal input must map to OpenAI");

    let parts = transformed.value["messages"][0]["content"]
        .as_array()
        .expect("OpenAI multimodal content parts");
    assert_eq!(
        parts[0]["image_url"]["url"],
        json!("data:image/jpeg;base64,ZmFrZQ==")
    );
    assert_eq!(
        parts[1],
        json!({"type":"input_audio","input_audio":{"data":"SUQz","format":"mp3"}})
    );
    assert_eq!(
        parts[2],
        json!({"type":"file","file":{"file_data":"JVBERi0=","filename":"report.pdf"}})
    );
    assert_eq!(
        parts[3]["image_url"]["url"],
        json!("https://example.com/image.png")
    );
    assert!(transformed.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Drop
            && fact.semantic_unit == TransformSemanticUnit::Metadata
    }));
}

#[test]
fn unportable_multimodal_inputs_fail_closed_without_payload_disclosure() {
    const PRIVATE_MARKER: &str = "multimodal-private-marker";
    let cases = [
        (
            DownstreamProtocol::Responses,
            json!({
                "model": "gpt-5",
                "input": [{"type":"message","role":"user","content":[
                    {"type":"input_file","filename":"remote.pdf","file_url":"https://files.example.com/remote.pdf"}
                ]}]
            }),
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({
                "model": "claude",
                "max_tokens": 64,
                "messages": [{"role":"user","content":[
                    {"type":"document","source":{"type":"file","file_id":PRIVATE_MARKER}}
                ]}]
            }),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"user","parts":[
                {"inlineData":{"mimeType":"video/mp4","data":"ZmFrZQ=="}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"user","parts":[
                {"fileData":{"mimeType":"application/pdf","fileUri":"https://generativelanguage.googleapis.com/v1beta/files/abc"}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"user","parts":[
                {"inlineData":{"mimeType":"application/octet-stream","data":PRIVATE_MARKER}}
            ]}]}),
        ),
    ];

    for (source, request) in cases {
        let failure = transform_request_data(request, source, UpstreamProtocol::Openai, false)
            .expect_err("unportable multimodal input must fail before target send");
        assert!(matches!(
            failure.origin,
            TransformFailureOrigin::DownstreamInput | TransformFailureOrigin::TargetCapability
        ));
        assert!(!format!("{failure:?}").contains(PRIVATE_MARKER));
    }
}

#[test]
fn responses_structured_outputs_preserve_json_object_and_schema_contracts() {
    let json_object = transform_request_data(
        json!({
            "model":"gpt-5",
            "input":"answer as JSON",
            "text":{"format":{"type":"json_object"}}
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("Responses JSON object mode must map to OpenAI");
    assert_eq!(
        json_object.value["response_format"],
        json!({"type":"json_object"})
    );

    let schema = json!({
        "type":"object",
        "properties":{"answer":{"type":"string","minLength":2}},
        "required":["answer"],
        "additionalProperties":false
    });
    let json_schema = transform_request_data(
        json!({
            "model":"gpt-5",
            "input":"answer as JSON",
            "text":{"format":{
                "type":"json_schema",
                "name":"answer_contract",
                "description":"A stable answer",
                "schema":schema,
                "strict":true
            }}
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("Responses JSON schema must map to OpenAI");

    assert_eq!(
        json_schema.value["response_format"],
        json!({
            "type":"json_schema",
            "json_schema":{
                "name":"answer_contract",
                "description":"A stable answer",
                "schema":schema,
                "strict":true
            }
        })
    );
}

#[test]
fn anthropic_structured_output_synthesizes_a_stable_openai_name() {
    let request = json!({
        "model":"claude",
        "max_tokens":128,
        "messages":[{"role":"user","content":"answer as JSON"}],
        "output_config":{"format":{
            "type":"json_schema",
            "schema":{
                "type":"object",
                "properties":{"answer":{"type":"string","pattern":"^[A-Z]+$"}},
                "required":["answer"],
                "additionalProperties":false
            }
        }}
    });
    let first = transform_request_data(
        request.clone(),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("Anthropic structured output must map to OpenAI");
    let second = transform_request_data(
        request,
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("repeated Anthropic structured output must map identically");

    let first_definition = &first.value["response_format"]["json_schema"];
    let second_definition = &second.value["response_format"]["json_schema"];
    assert_eq!(first_definition["name"], second_definition["name"]);
    assert!(
        first_definition["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("cyder_schema_"))
    );
    assert_eq!(first_definition["strict"], json!(true));
    assert_eq!(
        first_definition["schema"]["properties"]["answer"]["pattern"],
        json!("^[A-Z]+$")
    );
    assert!(first.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Synthesize
            && fact.semantic_unit == TransformSemanticUnit::StructuredOutput
    }));
}

#[test]
fn gemini_structured_output_preserves_constraints_and_drops_only_property_ordering() {
    let request = json!({
        "contents":[{"role":"user","parts":[{"text":"answer as JSON"}]}],
        "generationConfig":{
            "responseMimeType":"application/json",
            "responseJsonSchema":{
                "type":"object",
                "propertyOrdering":["answer"],
                "properties":{"answer":{"type":"string","minLength":2}},
                "required":["answer"],
                "additionalProperties":false
            }
        }
    });
    let first = transform_request_data(
        request.clone(),
        DownstreamProtocol::Gemini,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("Gemini structured output must map to OpenAI");
    let second = transform_request_data(
        request,
        DownstreamProtocol::Gemini,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("repeated Gemini structured output must map identically");

    let definition = &first.value["response_format"]["json_schema"];
    assert_eq!(
        definition["name"],
        second.value["response_format"]["json_schema"]["name"]
    );
    assert!(definition["schema"].get("propertyOrdering").is_none());
    assert_eq!(
        definition["schema"]["properties"]["answer"]["minLength"],
        json!(2)
    );
    assert_eq!(definition["schema"]["additionalProperties"], json!(false));
    assert!(first.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Drop
            && fact.semantic_unit == TransformSemanticUnit::StructuredOutput
    }));

    let current_format = transform_request_data(
        json!({
            "contents":[{"role":"user","parts":[{"text":"answer as JSON"}]}],
            "generationConfig":{"responseFormat":{"text":{
                "mimeType":"application/json",
                "schema":{"type":"object","properties":{"ok":{"type":"boolean"}}}
            }}}
        }),
        DownstreamProtocol::Gemini,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("current Gemini responseFormat must map to OpenAI");
    assert_eq!(
        current_format.value["response_format"]["json_schema"]["schema"]["properties"]["ok"]["type"],
        json!("boolean")
    );
}

#[test]
fn grammar_non_json_and_conflicting_structured_formats_fail_closed() {
    const PRIVATE_MARKER: &str = "structured-private-marker";
    let cases = [
        (
            DownstreamProtocol::Responses,
            json!({
                "model":"gpt-5",
                "input":"hello",
                "text":{"format":{"type":"grammar","grammar":PRIVATE_MARKER}}
            }),
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({
                "model":"claude",
                "max_tokens":64,
                "messages":[{"role":"user","content":"hello"}],
                "output_config":{"format":{"type":"grammar","grammar":PRIVATE_MARKER}}
            }),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({
                "contents":[{"role":"user","parts":[{"text":"hello"}]}],
                "generationConfig":{
                    "responseMimeType":"text/x.enum",
                    "responseSchema":{"description":PRIVATE_MARKER}
                }
            }),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({
                "contents":[{"role":"user","parts":[{"text":"hello"}]}],
                "generationConfig":{
                    "responseMimeType":"application/json",
                    "responseSchema":{"description":PRIVATE_MARKER},
                    "responseJsonSchema":{"description":PRIVATE_MARKER}
                }
            }),
        ),
    ];

    for (source, request) in cases {
        let failure = transform_request_data(request, source, UpstreamProtocol::Openai, false)
            .expect_err("unrepresentable structured output must reject");
        assert!(matches!(
            failure.origin,
            TransformFailureOrigin::DownstreamInput | TransformFailureOrigin::TargetCapability
        ));
        assert_eq!(
            failure.semantic_unit,
            TransformSemanticUnit::StructuredOutput
        );
        assert!(!format!("{failure:?}").contains(PRIVATE_MARKER));
    }
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
fn openai_stream_usage_false_is_overridden_with_a_bounded_diagnostic() {
    let data = json!({
        "model": "gemini-2.5-pro",
        "messages": [{"role": "user", "content": "hello"}],
        "stream": true,
        "stream_options": {"include_usage": false},
        "vendor_extension": true
    });

    let transformed = transform_request_data(
        data,
        DownstreamProtocol::Openai,
        UpstreamProtocol::Openai,
        true,
    )
    .expect("a valid streaming request must be normalized");

    assert_eq!(
        transformed.value,
        json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true,
            "stream_options": {"include_usage": true},
            "vendor_extension": true
        })
    );
    assert!(transformed.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Synthesize
            && fact.semantic_unit == TransformSemanticUnit::Usage
            && fact.reason_code == TransformReasonCode::PolicyOverride
    }));
}

#[test]
fn openai_stream_usage_omission_is_inserted_without_override_diagnostic() {
    let transformed = transform_request_data(
        json!({
            "model": "gpt",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true
        }),
        DownstreamProtocol::Openai,
        UpstreamProtocol::Openai,
        true,
    )
    .expect("omitted usage option must be inserted");

    assert_eq!(transformed.value["stream_options"]["include_usage"], true);
    assert!(
        transformed
            .summary
            .facts
            .iter()
            .all(|fact| fact.reason_code != TransformReasonCode::PolicyOverride)
    );
}

#[test]
fn openai_stream_usage_type_errors_are_payload_safe_rejections() {
    for request in [
        json!({
            "model": "gpt",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true,
            "stream_options": "payload-secret-marker"
        }),
        json!({
            "model": "gpt",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true,
            "stream_options": {"include_usage": "payload-secret-marker"}
        }),
    ] {
        let failure = transform_request_data(
            request,
            DownstreamProtocol::Openai,
            UpstreamProtocol::Openai,
            true,
        )
        .expect_err("malformed usage options must be rejected");

        assert_eq!(failure.origin, TransformFailureOrigin::DownstreamInput);
        assert_eq!(
            failure.reason_code,
            TransformReasonCode::InvalidProtocolShape
        );
        assert_eq!(failure.semantic_unit, TransformSemanticUnit::Usage);
        assert!(!format!("{failure:?}").contains("payload-secret-marker"));
    }
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
fn openai_response_target_rejects_cross_wire_audio_and_file_data() {
    for (mime_type, display_name, expected_unit) in [
        ("audio/wav", None, TransformSemanticUnit::AudioData),
        (
            "application/pdf",
            Some("report.pdf"),
            TransformSemanticUnit::FileData,
        ),
    ] {
        let failure = transform_result(
            json!({
                "candidates": [{
                    "index": 0,
                    "content": {
                        "role": "model",
                        "parts": [{
                            "inlineData": {
                                "mimeType": mime_type,
                                "data": "AA==",
                                "displayName": display_name
                            }
                        }]
                    },
                    "finishReason": "STOP"
                }]
            }),
            UpstreamProtocol::Gemini,
            DownstreamProtocol::Openai,
        )
        .expect_err("OpenAI response target must reject unencodable media");

        assert_eq!(failure.origin, TransformFailureOrigin::TargetCapability);
        assert_eq!(failure.phase, TransformPhase::ResponseEncode);
        assert_eq!(failure.semantic_unit, expected_unit);
        assert_eq!(failure.reason_code, TransformReasonCode::UnsupportedContent);
    }
}

#[test]
fn successful_response_without_usage_has_a_payload_free_degraded_observation() {
    const PRIVATE_MARKER: &str = "missing-usage-private-payload-marker";
    let transformed = transform_result_with_cost(
        json!({
            "id": "chatcmpl-missing-usage",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": PRIVATE_MARKER},
                "finish_reason": "stop"
            }]
        }),
        UpstreamProtocol::Openai,
        DownstreamProtocol::Openai,
    )
    .expect("valid same-wire response without usage must remain successful");

    assert_eq!(transformed.value.usage_info, None);
    assert_eq!(transformed.value.usage_normalization, None);
    let usage_fact = transformed
        .summary
        .facts
        .iter()
        .find(|fact| fact.reason_code == TransformReasonCode::UpstreamUsageMissing)
        .expect("missing usage should produce a typed diagnostic fact");
    assert_eq!(usage_fact.phase, TransformPhase::ResponseObserve);
    assert_eq!(usage_fact.semantic_unit, TransformSemanticUnit::Usage);
    assert_eq!(
        usage_fact.outcome,
        TransformOutcomeKind::ObservationDegraded
    );
    assert_eq!(usage_fact.action, TransformAction::PassThrough);
    assert_eq!(usage_fact.safe_summary, None);
    assert!(!format!("{:?}", transformed.summary).contains(PRIVATE_MARKER));
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

#[test]
fn portable_tool_controls_from_each_cross_wire_protocol_reach_openai() {
    let responses = transform_request_data(
        json!({
            "model": "gpt-5",
            "input": "weather",
            "tools": [{
                "type": "function",
                "name": "weather",
                "description": "lookup",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
                "strict": true
            }],
            "tool_choice": {
                "type": "allowed_tools",
                "mode": "required",
                "tools": [{"type": "function", "name": "weather"}]
            },
            "parallel_tool_calls": false
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("Responses portable tools must transform")
    .value;
    assert_eq!(responses["tools"][0]["function"]["strict"], true);
    assert_eq!(responses["tool_choice"]["type"], "allowed_tools");
    assert_eq!(
        responses["tool_choice"]["allowed_tools"]["mode"],
        "required"
    );
    assert_eq!(responses["parallel_tool_calls"], false);

    let anthropic = transform_request_data(
        json!({
            "model": "claude-sonnet",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "weather"}],
            "tools": [{
                "name": "weather",
                "description": "lookup",
                "input_schema": {"type": "object"},
                "strict": true
            }],
            "tool_choice": {
                "type": "tool",
                "name": "weather",
                "disable_parallel_tool_use": true
            }
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("Anthropic portable tools must transform")
    .value;
    assert_eq!(anthropic["tools"][0]["function"]["strict"], true);
    assert_eq!(anthropic["tool_choice"]["function"]["name"], "weather");
    assert_eq!(anthropic["parallel_tool_calls"], false);

    let gemini = transform_request_data(
        json!({
            "contents": [{"role": "user", "parts": [{"text": "weather"}]}],
            "tools": [{"functionDeclarations": [{
                "name": "weather",
                "description": "lookup",
                "parameters": {"type": "object"}
            }]}],
            "toolConfig": {"functionCallingConfig": {
                "mode": "VALIDATED",
                "allowedFunctionNames": ["weather"]
            }}
        }),
        DownstreamProtocol::Gemini,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("Gemini portable tools must transform")
    .value;
    assert_eq!(gemini["tools"][0]["function"]["strict"], true);
    assert_eq!(gemini["tool_choice"]["type"], "allowed_tools");
    assert_eq!(gemini["tool_choice"]["allowed_tools"]["mode"], "auto");
    assert_eq!(
        gemini["tool_choice"]["allowed_tools"]["tools"][0]["function"]["name"],
        "weather"
    );
}

#[test]
fn optional_nonportable_tools_drop_but_forced_selection_rejects() {
    let optional = transform_request_data(
        json!({
            "model": "gpt-5",
            "input": "search",
            "tools": [{"type": "web_search_preview"}],
            "tool_choice": "auto"
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("optional built-in tool may be removed");
    assert!(optional.value.get("tools").is_none());
    assert!(optional.summary.facts.iter().any(|fact| {
        fact.semantic_unit == TransformSemanticUnit::ToolDefinitions
            && fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Drop
    }));

    let forced = transform_request_data(
        json!({
            "model": "gpt-5",
            "input": "search",
            "tools": [{"type": "web_search_preview"}],
            "tool_choice": "required"
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Openai,
        false,
    )
    .expect_err("forced built-in tool must reject");
    assert_eq!(forced.origin, TransformFailureOrigin::DownstreamInput);
    assert_eq!(forced.semantic_unit, TransformSemanticUnit::ToolDefinitions);
    assert_eq!(
        forced.reason_code,
        TransformReasonCode::UnsupportedToolDefinitions
    );
}

#[test]
fn missing_tool_call_ids_are_stable_and_structured_results_are_canonical_text() {
    let request = json!({
        "model": "claude-sonnet",
        "max_tokens": 64,
        "messages": [
            {"role": "assistant", "content": [{
                "type": "tool_use",
                "name": "weather",
                "input": {"city": "Paris"}
            }]},
            {"role": "user", "content": [{
                "type": "tool_result",
                "content": {"z": 1, "a": 2},
                "is_error": true
            }]}
        ]
    });
    let first = transform_request_data(
        request.clone(),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("missing Anthropic IDs must be synthesized");
    let second = transform_request_data(
        request,
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Openai,
        false,
    )
    .expect("repeated transform must succeed");

    let call_id = first.value["messages"][0]["tool_calls"][0]["id"]
        .as_str()
        .expect("synthesized call id");
    assert_eq!(first.value["messages"][1]["tool_call_id"], call_id);
    assert_eq!(first.value, second.value);
    assert_eq!(
        first.value["messages"][1]["content"],
        "{\"error\":{\"a\":2,\"z\":1}}"
    );
    assert!(first.summary.facts.iter().any(|fact| {
        fact.semantic_unit == TransformSemanticUnit::ToolResult
            && fact.outcome == TransformOutcomeKind::ControlledLossMajor
            && fact.reason_code == TransformReasonCode::DeterministicTextDowngrade
    }));
}

#[test]
fn every_idless_tool_protocol_synthesizes_stable_call_result_correlation() {
    let cases = [
        (
            DownstreamProtocol::Responses,
            json!({
                "model":"gpt-5",
                "input":[
                    {"type":"function_call","name":"lookup","arguments":"{}"},
                    {"type":"function_call_output","output":"ok"}
                ]
            }),
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({
                "model":"claude-sonnet","max_tokens":64,
                "messages":[
                    {"role":"assistant","content":[{"type":"tool_use","name":"lookup","input":{}}]},
                    {"role":"user","content":[{"type":"tool_result","content":"ok"}]}
                ]
            }),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({
                "contents":[
                    {"role":"model","parts":[{"functionCall":{"name":"lookup","args":{}}}]},
                    {"role":"user","parts":[{"functionResponse":{"name":"lookup","response":{"result":"ok"}}}]}
                ]
            }),
        ),
    ];
    for (protocol, request) in cases {
        let first =
            transform_request_data(request.clone(), protocol, UpstreamProtocol::Openai, false)
                .expect("idless tool lifecycle must transform")
                .value;
        let second = transform_request_data(request, protocol, UpstreamProtocol::Openai, false)
            .expect("repeated idless tool lifecycle must transform")
            .value;
        let first_call_id = first["messages"][0]["tool_calls"][0]["id"]
            .as_str()
            .expect("synthetic tool call ID");
        assert_eq!(first["messages"][1]["tool_call_id"], first_call_id);
        assert_eq!(
            second["messages"][0]["tool_calls"][0]["id"], first_call_id,
            "{protocol:?}"
        );
    }

    let openai = json!({
        "model":"gpt-5",
        "messages":[
            {"role":"assistant","content":null,"tool_calls":[{
                "type":"function","function":{"name":"lookup","arguments":"{}"}
            }]},
            {"role":"tool","content":"ok"}
        ]
    });
    let converted = transform_request_data(
        openai,
        DownstreamProtocol::Openai,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("idless OpenAI lifecycle must transform")
    .value;
    assert_eq!(
        converted["input"][0]["call_id"],
        converted["input"][1]["call_id"]
    );
    assert!(
        converted["input"][0]["call_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("call-openai-"))
    );
}

#[test]
fn tool_error_result_survives_a_responses_round_trip() {
    let responses = transform_request_data(
        json!({
            "model":"claude-sonnet","max_tokens":64,
            "messages":[{"role":"user","content":[{
                "type":"tool_result","tool_use_id":"call_1",
                "content":{"code":"not_found"},"is_error":true
            }]}]
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("Anthropic error result must map to Responses")
    .value;
    let anthropic = transform_request_data(
        responses,
        DownstreamProtocol::Responses,
        UpstreamProtocol::Anthropic,
        false,
    )
    .expect("Responses error envelope must map back to Anthropic")
    .value;

    assert_eq!(anthropic["messages"][0]["content"][0]["is_error"], true);
    assert_eq!(
        anthropic["messages"][0]["content"][0]["content"],
        json!({"code":"not_found"})
    );
}
