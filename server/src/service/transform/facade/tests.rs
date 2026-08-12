use super::*;
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol};
use crate::service::transform::{
    TransformAction, TransformFailureOrigin, TransformOutcomeKind, TransformPhase,
    TransformReasonCode, TransformSemanticUnit,
};
use crate::utils::usage::UsageInfo;
use serde_json::{Value, json};

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

    assert_eq!(failure.origin, TransformFailureOrigin::DownstreamInput);
    assert_eq!(failure.phase, TransformPhase::RequestDecode);
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
fn qualitative_reasoning_controls_from_all_public_wires_reach_responses_with_expected_loss() {
    let openai = transform_request_data(
        json!({
            "model":"gpt-5","messages":[{"role":"user","content":"hello"}],
            "reasoning_effort":"medium"
        }),
        DownstreamProtocol::Openai,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("OpenAI qualitative effort must reach Responses");
    assert_eq!(openai.value["reasoning"]["effort"], "medium");
    assert!(openai.summary.facts.iter().any(|fact| {
        fact.semantic_unit == TransformSemanticUnit::ReasoningContent
            && fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Synthesize
            && fact.reason_code == TransformReasonCode::UnsupportedReasoning
            && fact.safe_summary.is_none()
    }));

    for (thinking, output_config, expected) in [
        (json!({"type":"adaptive"}), json!({"effort":"low"}), "low"),
        (
            json!({"type":"enabled","budget_tokens":1024}),
            json!(null),
            "high",
        ),
        (json!({"type":"disabled"}), json!(null), "none"),
        (json!({"type":"adaptive"}), json!({"effort":"max"}), "xhigh"),
    ] {
        let anthropic = transform_request_data(
            json!({
                "model":"claude","max_tokens":4096,
                "messages":[{"role":"user","content":"hello"}],
                "thinking":thinking,"output_config":output_config
            }),
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Responses,
            false,
        )
        .expect("Anthropic qualitative reasoning must reach Responses");
        assert_eq!(anthropic.value["reasoning"]["effort"], expected);
        assert!(anthropic.summary.facts.iter().any(|fact| {
            fact.semantic_unit == TransformSemanticUnit::ReasoningContent
                && fact.outcome == TransformOutcomeKind::ControlledLossMinor
                && fact.safe_summary.is_none()
        }));
    }

    for (thinking_config, expected) in [
        (json!({"thinkingLevel":"minimal"}), Some("minimal")),
        (
            json!({"thinkingLevel":"high","includeThoughts":true}),
            Some("high"),
        ),
        (json!({"thinkingBudget":0}), Some("none")),
        (json!({"thinkingBudget":-1}), None),
    ] {
        let gemini = transform_request_data(
            json!({
                "contents":[{"role":"user","parts":[{"text":"hello"}]}],
                "generationConfig":{"thinkingConfig":thinking_config}
            }),
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Responses,
            false,
        )
        .expect("Gemini qualitative reasoning must reach Responses");
        assert_eq!(
            gemini
                .value
                .pointer("/reasoning/effort")
                .and_then(Value::as_str),
            expected
        );
        assert!(gemini.summary.facts.iter().any(|fact| {
            fact.semantic_unit == TransformSemanticUnit::ReasoningContent
                && fact.outcome == TransformOutcomeKind::ControlledLossMinor
                && fact.safe_summary.is_none()
        }));
    }

    let native = json!({
        "model":"gpt-5","input":"hello","reasoning":{"effort":"xhigh","summary":"detailed"}
    });
    let responses = transform_request_data(
        native.clone(),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("Responses native reasoning must remain same-wire");
    assert_eq!(responses.value["reasoning"], native["reasoning"]);
    assert!(!responses.summary.facts.iter().any(|fact| matches!(
        fact.outcome,
        TransformOutcomeKind::ControlledLossMinor | TransformOutcomeKind::ControlledLossMajor
    )));
}

#[test]
fn reasoning_conflicts_and_invalid_controls_fail_closed_for_the_responses_target() {
    let cases = [
        (
            DownstreamProtocol::Openai,
            json!({"model":"gpt-5","messages":[],"reasoning_effort":"extreme"}),
            TransformReasonCode::InvalidProtocolShape,
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({
                "model":"claude","max_tokens":64,"messages":[],
                "thinking":{"type":"disabled"},"output_config":{"effort":"high"}
            }),
            TransformReasonCode::UnsupportedReasoning,
        ),
        (
            DownstreamProtocol::Gemini,
            json!({
                "contents":[],"generationConfig":{"thinkingConfig":{"thinkingBudget":128}}
            }),
            TransformReasonCode::UnsupportedReasoning,
        ),
        (
            DownstreamProtocol::Gemini,
            json!({
                "contents":[],"generationConfig":{"thinkingConfig":{"thinkingLevel":"low","thinkingBudget":0}}
            }),
            TransformReasonCode::InvalidProtocolShape,
        ),
        (
            DownstreamProtocol::Gemini,
            json!({
                "contents":[],"generationConfig":{"thinkingConfig":{"includeThoughts":"yes"}}
            }),
            TransformReasonCode::InvalidProtocolShape,
        ),
    ];
    for (protocol, request, expected_reason) in cases {
        let failure = transform_request_data(request, protocol, UpstreamProtocol::Responses, false)
            .expect_err("invalid or contradictory reasoning control must reject");
        assert_eq!(
            failure.origin,
            TransformFailureOrigin::DownstreamInput,
            "{protocol:?}"
        );
        assert_eq!(
            failure.semantic_unit,
            TransformSemanticUnit::ReasoningContent,
            "{protocol:?}"
        );
        assert_eq!(failure.reason_code, expected_reason, "{protocol:?}");
    }

    let native = transform_request_data(
        json!({"model":"gpt-5","input":"hello","reasoning":{"effort":"extreme"}}),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("same-wire remains transparent until final validation");
    let final_error = validate_final_generation_request(
        &native.value,
        UpstreamProtocol::Responses,
        &UpstreamProfileType::Responses,
    )
    .expect_err("invalid native Responses effort must fail final validation");
    assert_eq!(final_error.path, "/reasoning/effort");
}

#[test]
fn responses_visible_reasoning_maps_without_leaking_encrypted_content() {
    const ENCRYPTED_MARKER: &str = "encrypted-private-reasoning-marker";
    let source = json!({
        "id":"resp_reasoning","object":"response","created_at":1,
        "status":"completed","model":"gpt-5",
        "output":[
            {"type":"reasoning","id":"rs_1","content":[],
             "summary":[{"type":"summary_text","text":"visible summary"}],
             "encrypted_content":ENCRYPTED_MARKER},
            {"type":"message","id":"msg_1","status":"completed","role":"assistant",
             "content":[{"type":"output_text","text":"answer","annotations":[],"logprobs":[]}]}
        ],
        "usage":{"input_tokens":1,"output_tokens":2,"total_tokens":3,
                 "input_tokens_details":{"cached_tokens":0},
                 "output_tokens_details":{"reasoning_tokens":1}}
    });

    let native = transform_result(
        source.clone(),
        UpstreamProtocol::Responses,
        DownstreamProtocol::Responses,
    )
    .expect("native Responses reasoning must pass through");
    assert_eq!(native.value.0, source);

    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini,
    ] {
        let transformed = transform_result(source.clone(), UpstreamProtocol::Responses, downstream)
            .expect("portable visible reasoning must transform");
        let serialized = serde_json::to_string(&transformed.value.0).expect("JSON");
        assert!(!serialized.contains(ENCRYPTED_MARKER), "{downstream:?}");
        assert!(transformed.summary.facts.iter().any(|fact| {
            fact.outcome == TransformOutcomeKind::ControlledLossMinor && fact.safe_summary.is_none()
        }));
        if downstream == DownstreamProtocol::Openai {
            assert!(!serialized.contains("visible summary"));
            assert!(serialized.contains("answer"));
        } else {
            assert!(serialized.contains("visible summary"), "{downstream:?}");
        }
    }
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
fn portable_multimodal_inputs_from_all_public_wires_reach_responses_natively() {
    let cases = [
        (
            DownstreamProtocol::Openai,
            json!({
                "model":"gpt-5","messages":[{"role":"user","content":[
                    {"type":"image_url","image_url":{"url":"https://example.com/a.png","detail":"high"}},
                    {"type":"image_url","image_url":{"url":"data:image/png;base64,ZmFrZQ=="}},
                    {"type":"input_audio","input_audio":{"data":"UklGRg==","format":"wav"}},
                    {"type":"file","file":{"filename":"report.pdf","file_data":"JVBERi0="}},
                    {"type":"file","file":{"file_id":"file_same_target"}}
                ]}]
            }),
            false,
        ),
        (
            DownstreamProtocol::Responses,
            json!({
                "model":"gpt-5","input":[{"type":"message","role":"user","content":[
                    {"type":"input_image","image_url":"data:image/webp;base64,ZmFrZQ==","detail":"low"},
                    {"type":"input_audio","input_audio":{"data":"SUQz","format":"mp3"}},
                    {"type":"input_file","filename":"notes.md","file_data":"IyBub3Rlcw=="},
                    {"type":"input_file","file_id":"file_same_target"}
                ]}]
            }),
            false,
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({
                "model":"claude","max_tokens":64,"messages":[{"role":"user","content":[
                    {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"ZmFrZQ=="}},
                    {"type":"image","source":{"type":"url","url":"https://example.com/a.webp"}},
                    {"type":"document","source":{"type":"base64","media_type":"text/csv","data":"YSxi"},"title":"table.csv"}
                ]}]
            }),
            false,
        ),
        (
            DownstreamProtocol::Gemini,
            json!({
                "contents":[{"role":"user","parts":[
                    {"inlineData":{"mimeType":"image/png","data":"ZmFrZQ==","displayName":"preview.png"}},
                    {"inlineData":{"mimeType":"audio/mpeg","data":"SUQz","displayName":"voice.mp3"}},
                    {"inlineData":{"mimeType":"application/json","data":"e30=","displayName":"data.json"}},
                    {"fileData":{"mimeType":"image/jpeg","fileUri":"https://example.com/a.jpg","displayName":"preview.jpg"}}
                ]}]
            }),
            true,
        ),
    ];

    for (protocol, request, expects_loss) in cases {
        let mut transformed =
            transform_request_data(request, protocol, UpstreamProtocol::Responses, false)
                .expect("portable multimodal input must reach Responses");
        if transformed.value["model"] == "" {
            transformed.value["model"] = json!("materialized-target-model");
        }
        validate_final_generation_request(
            &transformed.value,
            UpstreamProtocol::Responses,
            &UpstreamProfileType::Responses,
        )
        .expect("encoded Responses media must pass the final target boundary");

        let parts = transformed.value["input"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find_map(|item| item.get("content").and_then(Value::as_array))
            })
            .expect("Responses target must receive typed message content");
        assert!(
            parts.iter().any(|part| part["type"] == "input_image"),
            "{protocol:?}"
        );
        assert!(
            parts.iter().any(|part| part["type"] == "input_file"),
            "{protocol:?}"
        );
        if protocol != DownstreamProtocol::Anthropic {
            assert!(
                parts.iter().any(|part| part["type"] == "input_audio"),
                "{protocol:?}"
            );
        }
        if protocol == DownstreamProtocol::Openai {
            assert!(
                parts
                    .iter()
                    .any(|part| { part["type"] == "input_image" && part["detail"] == "high" })
            );
        }
        let has_loss = transformed.summary.facts.iter().any(|fact| {
            matches!(
                fact.outcome,
                TransformOutcomeKind::ControlledLossMinor
                    | TransformOutcomeKind::ControlledLossMajor
            )
        });
        assert_eq!(has_loss, expects_loss, "{protocol:?}");
        if protocol == DownstreamProtocol::Gemini {
            assert!(transformed.summary.facts.iter().any(|fact| {
                fact.semantic_unit == TransformSemanticUnit::Metadata
                    && fact.outcome == TransformOutcomeKind::ControlledLossMinor
                    && fact.action == TransformAction::Drop
                    && fact.safe_summary.is_none()
            }));
            assert!(!transformed.value.to_string().contains("displayName"));
        }
        assert!(
            transformed
                .summary
                .facts
                .iter()
                .all(|fact| fact.safe_summary.is_none())
        );
    }
}

#[test]
fn anthropic_media_cache_hint_is_dropped_with_payload_free_diagnostics_for_responses() {
    let transformed = transform_request_data(
        json!({
            "model":"claude","max_tokens":64,"messages":[{"role":"user","content":[{
                "type":"image",
                "source":{"type":"base64","media_type":"image/png","data":"ZmFrZQ=="},
                "cache_control":{"type":"ephemeral"}
            }]}]
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("cache metadata loss must remain sendable");
    assert!(transformed.summary.facts.iter().any(|fact| {
        fact.semantic_unit == TransformSemanticUnit::Metadata
            && fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Drop
            && fact.safe_summary.is_none()
    }));
    assert!(!transformed.value.to_string().contains("cache_control"));
    assert_eq!(
        transformed.value["input"][0]["content"][0]["type"],
        "input_image"
    );
}

#[test]
fn responses_target_media_contract_rejects_unportable_shapes_without_payloads() {
    const PRIVATE_MARKER: &str = "responses-media-private-marker";
    let cross_wire_cases = [
        (
            DownstreamProtocol::Openai,
            json!({"model":"gpt-5","messages":[{"role":"user","content":[
                {"type":"image_url","image_url":{"url":format!("data:image/png;base64,{PRIVATE_MARKER}")}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Openai,
            json!({"model":"gpt-5","messages":[{"role":"user","content":[
                {"type":"file","file":{"filename":"payload.exe","file_data":"AA=="}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Openai,
            json!({"model":"gpt-5","messages":[{"role":"assistant","content":[
                {"type":"image_url","image_url":{"url":"https://example.com/a.png"}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({"model":"claude","max_tokens":64,"messages":[{"role":"user","content":[
                {"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"JVBERi0="}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({"model":"claude","max_tokens":64,"messages":[{"role":"user","content":[
                {"type":"document","source":{"type":"file","file_id":PRIVATE_MARKER},"title":"remote.pdf"}
            ]}]}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"user","parts":[
                {"inlineData":{"mimeType":"video/mp4","data":"ZmFrZQ==","displayName":"clip.mp4"}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"user","parts":[
                {"inlineData":{"mimeType":"application/octet-stream","data":"AA==","displayName":"blob.bin"}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"user","parts":[
                {"inlineData":{"mimeType":"application/pdf","data":"JVBERi0="}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"user","parts":[
                {"fileData":{"mimeType":"application/pdf","fileUri":format!("https://generativelanguage.googleapis.com/v1beta/files/{PRIVATE_MARKER}")}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"model","parts":[
                {"inlineData":{"mimeType":"image/png","data":"ZmFrZQ=="}}
            ]}]}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"user","parts":[
                {"executableCode":{"language":"python","code":PRIVATE_MARKER}}
            ]}]}),
        ),
    ];
    for (protocol, request) in cross_wire_cases {
        let failure = transform_request_data(request, protocol, UpstreamProtocol::Responses, false)
            .expect_err("unportable media must fail before target send");
        assert!(
            !format!("{failure:?}").contains(PRIVATE_MARKER),
            "{protocol:?}"
        );
    }

    let native_cases = [
        json!({"model":"gpt-5","input":[{"role":"user","content":[
            {"type":"input_image","image_url":"https://example.com/a.png","file_id":"file_same_target"}
        ]}]}),
        json!({"model":"gpt-5","input":[{"role":"assistant","content":[
            {"type":"input_audio","input_audio":{"data":"AA==","format":"wav"}}
        ]}]}),
        json!({"model":"gpt-5","input":[{"role":"user","content":[
            {"type":"input_file","filename":"","file_data":"JVBERi0="}
        ]}]}),
        json!({"model":"gpt-5","input":[{"role":"user","content":[
            {"type":"input_file","filename":"remote.pdf","file_url":format!("https://example.com/{PRIVATE_MARKER}.pdf")}
        ]}]}),
        json!({"model":"gpt-5","input":[{"role":"user","content":[
            {"type":"input_file","filename":"payload.bin","file_data":"data:application/octet-stream;base64,AA=="}
        ]}]}),
    ];
    for request in native_cases {
        let transformed = transform_request_data(
            request,
            DownstreamProtocol::Responses,
            UpstreamProtocol::Responses,
            false,
        )
        .expect("same-wire remains transparent until final validation");
        let error = validate_final_generation_request(
            &transformed.value,
            UpstreamProtocol::Responses,
            &UpstreamProfileType::Responses,
        )
        .expect_err("unportable native media must fail the final boundary");
        assert!(!format!("{error:?}").contains(PRIVATE_MARKER));
    }
}

#[test]
fn responses_native_media_output_item_is_transparent_same_wire_and_rejected_cross_wire() {
    const PRIVATE_MARKER: &str = "responses-image-output-private-marker";
    let source = json!({
        "id":"resp_media","object":"response","created_at":1,"status":"completed",
        "model":"gpt-5","output":[{
            "type":"image_generation_call","id":"ig_1","status":"completed",
            "result":PRIVATE_MARKER
        }],
        "usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2,
                 "input_tokens_details":{"cached_tokens":0},
                 "output_tokens_details":{"reasoning_tokens":0}}
    });
    let native = transform_result(
        source.clone(),
        UpstreamProtocol::Responses,
        DownstreamProtocol::Responses,
    )
    .expect("native media output remains same-wire transparent");
    assert_eq!(native.value.0, source);

    for downstream in [
        DownstreamProtocol::Openai,
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini,
    ] {
        let failure = transform_result(source.clone(), UpstreamProtocol::Responses, downstream)
            .expect_err("native media output must fail closed cross-wire");
        assert_eq!(failure.origin, TransformFailureOrigin::UpstreamPayload);
        assert_eq!(
            failure.semantic_unit,
            TransformSemanticUnit::ResponsesUnknownItem
        );
        assert_eq!(
            failure.reason_code,
            TransformReasonCode::UnknownSemanticUnit
        );
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
fn structured_outputs_from_all_public_wires_reach_responses_with_stable_names_and_loss() {
    let schema = json!({
        "type":"object",
        "properties":{"answer":{"type":"string","minLength":2}},
        "required":["answer"],
        "additionalProperties":false
    });
    let cases = [
        (
            DownstreamProtocol::Openai,
            json!({
                "model":"gpt-5","messages":[{"role":"user","content":"answer as JSON"}],
                "response_format":{"type":"json_schema","json_schema":{
                    "name":"answer_contract","description":"A stable answer",
                    "schema":schema,"strict":true
                }}
            }),
            Some("answer_contract"),
            false,
        ),
        (
            DownstreamProtocol::Responses,
            json!({
                "model":"gpt-5","input":"answer as JSON","text":{"format":{
                    "type":"json_schema","name":"answer_contract",
                    "description":"A stable answer","schema":schema,"strict":true
                }}
            }),
            Some("answer_contract"),
            false,
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({
                "model":"claude","max_tokens":128,
                "messages":[{"role":"user","content":"answer as JSON"}],
                "output_config":{"format":{"type":"json_schema","schema":schema}}
            }),
            None,
            true,
        ),
        (
            DownstreamProtocol::Gemini,
            json!({
                "contents":[{"role":"user","parts":[{"text":"answer as JSON"}]}],
                "generationConfig":{"responseFormat":{"text":{
                    "mimeType":"application/json","schema":{
                        "type":"object","propertyOrdering":["answer"],
                        "properties":{"answer":{
                            "type":"string","minLength":2,"propertyOrdering":[]
                        }},
                        "required":["answer"],"additionalProperties":false
                    }
                }}}
            }),
            None,
            true,
        ),
    ];

    for (protocol, request, expected_name, expects_loss) in cases {
        let first = transform_request_data(
            request.clone(),
            protocol,
            UpstreamProtocol::Responses,
            false,
        )
        .expect("portable structured output must reach Responses");
        let second = transform_request_data(request, protocol, UpstreamProtocol::Responses, false)
            .expect("stable structured output must repeat");
        let format = &first.value["text"]["format"];
        assert_eq!(format["type"], "json_schema", "{protocol:?}");
        assert_eq!(format["strict"], true, "{protocol:?}");
        assert_eq!(
            format["schema"]["properties"]["answer"]["minLength"], 2,
            "{protocol:?}"
        );
        assert_eq!(format["name"], second.value["text"]["format"]["name"]);
        match expected_name {
            Some(expected_name) => {
                assert_eq!(format["name"], expected_name, "{protocol:?}");
                assert_eq!(format["description"], "A stable answer", "{protocol:?}");
            }
            None => assert!(
                format["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("cyder_schema_")),
                "{protocol:?}"
            ),
        }
        if protocol == DownstreamProtocol::Gemini {
            assert!(!format["schema"].to_string().contains("propertyOrdering"));
        }
        let has_loss = first.summary.facts.iter().any(|fact| {
            matches!(
                fact.outcome,
                TransformOutcomeKind::ControlledLossMinor
                    | TransformOutcomeKind::ControlledLossMajor
            )
        });
        assert_eq!(has_loss, expects_loss, "{protocol:?}");
        assert!(
            first
                .summary
                .facts
                .iter()
                .all(|fact| fact.safe_summary.is_none())
        );
    }

    for (protocol, request) in [
        (
            DownstreamProtocol::Openai,
            json!({"model":"gpt-5","messages":[{"role":"user","content":"json"}],
                   "response_format":{"type":"json_object"}}),
        ),
        (
            DownstreamProtocol::Responses,
            json!({"model":"gpt-5","input":"json","text":{"format":{"type":"json_object"}}}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[{"role":"user","parts":[{"text":"json"}]}],
                   "generationConfig":{"responseMimeType":"application/json"}}),
        ),
    ] {
        let transformed =
            transform_request_data(request, protocol, UpstreamProtocol::Responses, false)
                .expect("json_object mode must reach Responses");
        assert_eq!(transformed.value["text"]["format"]["type"], "json_object");
    }
}

#[test]
fn responses_target_structured_output_rejects_conflicts_and_invalid_core_shapes() {
    const PRIVATE_MARKER: &str = "responses-structured-private-marker";
    let cross_wire_cases = [
        (
            DownstreamProtocol::Openai,
            json!({"model":"gpt-5","messages":[],
                   "response_format":{"type":"grammar","grammar":PRIVATE_MARKER}}),
        ),
        (
            DownstreamProtocol::Openai,
            json!({"model":"gpt-5","messages":[],"response_format":{"type":"json_schema",
                   "json_schema":{"name":"","schema":{"private":PRIVATE_MARKER},"strict":true}}}),
        ),
        (
            DownstreamProtocol::Openai,
            json!({"model":"gpt-5","messages":[],"response_format":{"type":"json_schema",
                   "json_schema":{"name":"valid_name","schema":[PRIVATE_MARKER],"strict":true}}}),
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({"model":"claude","max_tokens":64,"messages":[],
                   "output_config":{"format":{"type":"grammar","grammar":PRIVATE_MARKER}}}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[],"generationConfig":{"responseMimeType":"text/x.enum",
                   "responseSchema":{"private":PRIVATE_MARKER}}}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[],"generationConfig":{"responseMimeType":"application/json",
                   "responseSchema":{"private":PRIVATE_MARKER},
                   "responseJsonSchema":{"private":PRIVATE_MARKER}}}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[],"generationConfig":{"responseFormat":{"text":{
                   "mimeType":"application/json","schema":[PRIVATE_MARKER]}},
                   "responseMimeType":"application/json"}}),
        ),
    ];
    for (protocol, request) in cross_wire_cases {
        let failure = transform_request_data(request, protocol, UpstreamProtocol::Responses, false)
            .expect_err("invalid structured output must fail before target send");
        assert_eq!(
            failure.semantic_unit,
            TransformSemanticUnit::StructuredOutput,
            "{protocol:?}"
        );
        assert!(!format!("{failure:?}").contains(PRIVATE_MARKER));
    }

    let native_cases = [
        json!({"model":"gpt-5","input":"json","text":{"format":{
            "type":"grammar","grammar":PRIVATE_MARKER}}}),
        json!({"model":"gpt-5","input":"json","text":{"format":{
            "type":"json_schema","name":"","schema":{"private":PRIVATE_MARKER}}}}),
        json!({"model":"gpt-5","input":"json","text":{"format":{
            "type":"json_schema","name":"valid_name","schema":[PRIVATE_MARKER]}}}),
        json!({"model":"gpt-5","input":"json","text":{"format":{
            "type":"json_object","schema":{"private":PRIVATE_MARKER}}}}),
        json!({"model":"gpt-5","input":"json","response_format":{
            "type":"json_schema","private":PRIVATE_MARKER}}),
    ];
    for request in native_cases {
        let transformed = transform_request_data(
            request,
            DownstreamProtocol::Responses,
            UpstreamProtocol::Responses,
            false,
        )
        .expect("same-wire remains transparent until final validation");
        let error = validate_final_generation_request(
            &transformed.value,
            UpstreamProtocol::Responses,
            &UpstreamProfileType::Responses,
        )
        .expect_err("invalid native structured output must fail final validation");
        assert!(!format!("{error:?}").contains(PRIVATE_MARKER));
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
fn responses_target_enforces_store_false_and_preserves_same_wire_extensions() {
    let transformed = transform_request_data(
        json!({
            "model": "gpt",
            "input": "hello",
            "vendor_extension": {"preserved": true}
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("stateless same-wire Responses request should pass");

    assert_eq!(transformed.value["store"], false);
    assert_eq!(transformed.value["vendor_extension"]["preserved"], true);
}

#[test]
fn responses_target_enforces_store_false_for_all_downstream_protocols() {
    for (protocol, request) in [
        (
            DownstreamProtocol::Openai,
            json!({
                "model": "gpt",
                "messages": [{"role": "user", "content": "hello"}]
            }),
        ),
        (
            DownstreamProtocol::Responses,
            json!({"model": "gpt", "input": "hello"}),
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({
                "model": "claude",
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "hello"}]
            }),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents": [{"role": "user", "parts": [{"text": "hello"}]}]}),
        ),
    ] {
        let transformed =
            transform_request_data(request, protocol, UpstreamProtocol::Responses, false)
                .unwrap_or_else(|failure| panic!("{protocol:?}: {failure:?}"));
        assert_eq!(transformed.value["store"], false, "{protocol:?}");
    }
}

#[test]
fn responses_target_rejects_stateful_controls_without_payload_diagnostics() {
    for (field, value) in [
        ("store", json!(true)),
        (
            "previous_response_id",
            json!("response-state-private-marker"),
        ),
        (
            "conversation",
            json!({"id": "conversation-state-private-marker"}),
        ),
        ("background", json!(true)),
    ] {
        let mut request = json!({
            "model": "gpt",
            "input": "prompt-private-marker"
        });
        request[field] = value;
        let failure = transform_request_data(
            request,
            DownstreamProtocol::Responses,
            UpstreamProtocol::Responses,
            false,
        )
        .expect_err("stateful Responses request must be rejected");

        assert_eq!(failure.origin, TransformFailureOrigin::DownstreamInput);
        assert_eq!(failure.reason_code, TransformReasonCode::PolicyOverride);
        assert_eq!(
            failure.semantic_unit,
            TransformSemanticUnit::RequestEnvelope
        );
        let diagnostic = format!("{failure:?}");
        for marker in [
            "prompt-private-marker",
            "response-state-private-marker",
            "conversation-state-private-marker",
        ] {
            assert!(!diagnostic.contains(marker), "{field}: {marker}");
        }
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
fn portable_tool_lifecycles_from_all_public_wires_reach_responses_with_stable_pairing() {
    for choice in ["none", "auto", "required"] {
        let transformed = transform_request_data(
            json!({
                "model":"gpt-5","messages":[{"role":"user","content":"lookup"}],
                "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}],
                "tool_choice":choice
            }),
            DownstreamProtocol::Openai,
            UpstreamProtocol::Responses,
            false,
        )
        .expect("portable scalar tool choice must reach Responses");
        assert_eq!(transformed.value["tool_choice"], choice);
    }

    let openai = transform_request_data(
        json!({
            "model": "gpt-5",
            "messages": [
                {"role":"assistant","content":null,"tool_calls":[
                    {"id":"call-weather","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}},
                    {"id":"call-time","type":"function","function":{"name":"time","arguments":"{\"zone\":\"UTC\"}"}}
                ]},
                {"role":"tool","tool_call_id":"call-weather","content":"{\"temp\":21}"},
                {"role":"tool","tool_call_id":"call-time","content":"12:00"}
            ],
            "tools": [
                {"type":"function","function":{"name":"weather","description":"lookup weather","parameters":{"type":"object"},"strict":true}},
                {"type":"function","function":{"name":"time","parameters":{"type":"object"},"strict":false}}
            ],
            "tool_choice":{"type":"allowed_tools","allowed_tools":{"mode":"required","tools":[
                {"type":"function","function":{"name":"weather"}},
                {"type":"function","function":{"name":"time"}}
            ]}},
            "parallel_tool_calls": false
        }),
        DownstreamProtocol::Openai,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("OpenAI portable tool lifecycle must reach Responses");
    assert!(!openai.summary.facts.iter().any(|fact| matches!(
        fact.outcome,
        TransformOutcomeKind::ControlledLossMinor | TransformOutcomeKind::ControlledLossMajor
    )));
    assert_eq!(openai.value["tools"][0]["name"], "weather");
    assert_eq!(openai.value["tools"][0]["description"], "lookup weather");
    assert_eq!(
        openai.value["tools"][0]["parameters"],
        json!({"type":"object"})
    );
    assert_eq!(openai.value["tools"][0]["strict"], true);
    assert_eq!(openai.value["tool_choice"]["type"], "allowed_tools");
    assert_eq!(openai.value["tool_choice"]["mode"], "required");
    assert_eq!(openai.value["parallel_tool_calls"], false);
    assert_eq!(openai.value["input"][0]["call_id"], "call-weather");
    assert_eq!(openai.value["input"][1]["call_id"], "call-time");
    assert_eq!(openai.value["input"][2]["call_id"], "call-weather");
    assert_eq!(openai.value["input"][3]["call_id"], "call-time");

    let anthropic = transform_request_data(
        json!({
            "model":"claude-sonnet","max_tokens":64,
            "messages":[
                {"role":"assistant","content":[
                    {"type":"tool_use","id":"tool-weather","name":"weather","input":{"city":"Paris"}},
                    {"type":"tool_use","id":"tool-time","name":"time","input":{"zone":"UTC"}}
                ]},
                {"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"tool-weather","content":{"temp":21}},
                    {"type":"tool_result","tool_use_id":"tool-time","content":"12:00"}
                ]}
            ],
            "tools":[
                {"name":"weather","description":"lookup weather","input_schema":{"type":"object"},"strict":true},
                {"name":"time","input_schema":{"type":"object"},"strict":false}
            ],
            "tool_choice":{"type":"tool","name":"weather","disable_parallel_tool_use":true}
        }),
        DownstreamProtocol::Anthropic,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("Anthropic portable tool lifecycle must reach Responses");
    assert!(!anthropic.summary.facts.iter().any(|fact| matches!(
        fact.outcome,
        TransformOutcomeKind::ControlledLossMinor | TransformOutcomeKind::ControlledLossMajor
    )));
    assert_eq!(
        anthropic.value["tool_choice"],
        json!({"type":"function","name":"weather"})
    );
    assert_eq!(anthropic.value["parallel_tool_calls"], false);
    assert_eq!(anthropic.value["input"][0]["call_id"], "tool-weather");
    assert_eq!(anthropic.value["input"][1]["call_id"], "tool-time");
    assert_eq!(anthropic.value["input"][2]["call_id"], "tool-weather");
    assert_eq!(anthropic.value["input"][3]["call_id"], "tool-time");

    let gemini_request = json!({
        "contents":[
            {"role":"model","parts":[
                {"functionCall":{"name":"weather","args":{"city":"Paris"}}},
                {"functionCall":{"name":"time","args":{"zone":"UTC"}}}
            ]},
            {"role":"user","parts":[
                {"functionResponse":{"name":"weather","response":{"temp":21}}},
                {"functionResponse":{"name":"time","response":{"result":"12:00"}}}
            ]}
        ],
        "tools":[{"functionDeclarations":[
            {"name":"weather","description":"lookup weather","parameters":{"type":"object"}},
            {"name":"time","parameters":{"type":"object"}}
        ]}],
        "toolConfig":{"functionCallingConfig":{"mode":"ANY","allowedFunctionNames":["weather","time"]}}
    });
    let gemini = transform_request_data(
        gemini_request.clone(),
        DownstreamProtocol::Gemini,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("Gemini portable tool lifecycle must reach Responses");
    let repeated_gemini = transform_request_data(
        gemini_request,
        DownstreamProtocol::Gemini,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("repeated Gemini portable tool lifecycle must reach Responses");
    assert!(gemini.summary.facts.iter().any(|fact| {
        fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.reason_code == TransformReasonCode::SyntheticCorrelationId
            && matches!(
                fact.semantic_unit,
                TransformSemanticUnit::ToolCall | TransformSemanticUnit::ToolResult
            )
            && fact.safe_summary.is_none()
    }));
    assert_eq!(gemini.value["tool_choice"]["type"], "allowed_tools");
    assert_eq!(gemini.value["tool_choice"]["mode"], "required");
    assert_eq!(
        gemini.value["input"][0]["call_id"],
        repeated_gemini.value["input"][0]["call_id"]
    );
    assert_eq!(
        gemini.value["input"][1]["call_id"],
        repeated_gemini.value["input"][1]["call_id"]
    );
    assert_eq!(
        gemini.value["input"][0]["call_id"],
        gemini.value["input"][2]["call_id"]
    );
    assert_eq!(
        gemini.value["input"][1]["call_id"],
        gemini.value["input"][3]["call_id"]
    );

    let responses = transform_request_data(
        json!({
            "model":"gpt-5","input":[
                {"type":"function_call","id":"fc-weather","call_id":"response-weather","name":"weather","arguments":"{\"city\":\"Paris\"}"},
                {"type":"function_call_output","id":"fco-weather","call_id":"response-weather","output":{"temp":21}}
            ],
            "tools":[{"type":"function","name":"weather","description":"lookup weather","parameters":{"type":"object"},"strict":true}],
            "tool_choice":"required","parallel_tool_calls":false
        }),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("Responses portable tool lifecycle must remain same-wire");
    assert_eq!(responses.value["input"][0]["call_id"], "response-weather");
    assert_eq!(responses.value["input"][1]["call_id"], "response-weather");
    assert!(!responses.summary.facts.iter().any(|fact| matches!(
        fact.outcome,
        TransformOutcomeKind::ControlledLossMinor | TransformOutcomeKind::ControlledLossMajor
    )));
}

#[test]
fn portable_function_definitions_reject_invalid_names_parameters_descriptions_and_parallel_controls()
 {
    let cases = [
        (
            DownstreamProtocol::Openai,
            json!({"model":"gpt-5","messages":[],"tools":[{"type":"function","function":{"name":"","parameters":{}}}]}),
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({"model":"claude","max_tokens":1,"messages":[],"tools":[{"name":"lookup","description":7,"input_schema":{}}]}),
        ),
        (
            DownstreamProtocol::Gemini,
            json!({"contents":[],"tools":[{"functionDeclarations":[{"name":"lookup","parameters":"object"}]}]}),
        ),
    ];
    for (protocol, request) in cases {
        let failure = transform_request_data(request, protocol, UpstreamProtocol::Responses, false)
            .expect_err("invalid portable function definition must reject");
        assert_eq!(
            failure.origin,
            TransformFailureOrigin::DownstreamInput,
            "{protocol:?}"
        );
        assert_eq!(
            failure.semantic_unit,
            TransformSemanticUnit::ToolDefinitions,
            "{protocol:?}"
        );
        assert_eq!(
            failure.reason_code,
            TransformReasonCode::InvalidProtocolShape,
            "{protocol:?}"
        );
    }

    let invalid_same_wire = transform_request_data(
        json!({"model":"gpt-5","input":"hi","tools":[{"type":"function","name":"lookup","parameters":[]}]}),
        DownstreamProtocol::Responses,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("same-wire transform stays transparent until final target validation");
    let final_error = validate_final_generation_request(
        &invalid_same_wire.value,
        UpstreamProtocol::Responses,
        &UpstreamProfileType::Responses,
    )
    .expect_err("invalid same-wire function parameters must fail final validation");
    assert_eq!(final_error.path, "/tools");

    let invalid_parallel = transform_request_data(
        json!({"model":"gpt-5","messages":[],"parallel_tool_calls":"yes"}),
        DownstreamProtocol::Openai,
        UpstreamProtocol::Responses,
        false,
    )
    .expect_err("invalid parallel control must reject");
    assert_eq!(
        invalid_parallel.semantic_unit,
        TransformSemanticUnit::ToolDefinitions
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

    let optional_to_responses = transform_request_data(
        json!({
            "model":"gpt-5","messages":[{"role":"user","content":"lookup"}],
            "tools":[
                {"type":"custom","name":"private_tool"},
                {"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}
            ],
            "tool_choice":"auto"
        }),
        DownstreamProtocol::Openai,
        UpstreamProtocol::Responses,
        false,
    )
    .expect("optional nonportable OpenAI tool may be removed before Responses");
    assert_eq!(
        optional_to_responses.value["tools"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(optional_to_responses.value["tools"][0]["name"], "lookup");
    assert!(optional_to_responses.summary.facts.iter().any(|fact| {
        fact.semantic_unit == TransformSemanticUnit::ToolDefinitions
            && fact.outcome == TransformOutcomeKind::ControlledLossMinor
            && fact.action == TransformAction::Drop
            && fact.safe_summary.is_none()
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
fn explicit_then_idless_results_consume_the_remaining_call_in_multi_call_order() {
    let cases = [
        (
            DownstreamProtocol::Openai,
            json!({
                "model":"gpt-5","messages":[
                    {"role":"assistant","content":null,"tool_calls":[
                        {"id":"call-a","type":"function","function":{"name":"a","arguments":"{}"}},
                        {"id":"call-b","type":"function","function":{"name":"b","arguments":"{}"}}
                    ]},
                    {"role":"tool","tool_call_id":"call-a","content":"first"},
                    {"role":"tool","content":"second"}
                ]
            }),
        ),
        (
            DownstreamProtocol::Responses,
            json!({
                "model":"gpt-5","input":[
                    {"type":"function_call","id":"fc-a","call_id":"call-a","name":"a","arguments":"{}"},
                    {"type":"function_call","id":"fc-b","call_id":"call-b","name":"b","arguments":"{}"},
                    {"type":"function_call_output","id":"fco-a","call_id":"call-a","output":"first"},
                    {"type":"function_call_output","id":"fco-b","output":"second"}
                ]
            }),
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({
                "model":"claude","max_tokens":16,"messages":[
                    {"role":"assistant","content":[
                        {"type":"tool_use","id":"call-a","name":"a","input":{}},
                        {"type":"tool_use","id":"call-b","name":"b","input":{}}
                    ]},
                    {"role":"user","content":[
                        {"type":"tool_result","tool_use_id":"call-a","content":"first"},
                        {"type":"tool_result","content":"second"}
                    ]}
                ]
            }),
        ),
    ];

    for (protocol, request) in cases {
        let transformed =
            transform_request_data(request, protocol, UpstreamProtocol::Responses, false)
                .expect("mixed explicit/idless multi-call lifecycle must transform");
        assert_eq!(
            transformed.value["input"][2]["call_id"], "call-a",
            "{protocol:?}"
        );
        assert_eq!(
            transformed.value["input"][3]["call_id"], "call-b",
            "{protocol:?}"
        );
    }
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
