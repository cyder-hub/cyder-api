use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::StreamTransformer;
use crate::service::transform::providers::{anthropic, gemini, ollama, openai, responses};
use crate::service::transform::unified::{
    UnifiedChunkResponse, UnifiedContentPartDelta, UnifiedStreamEvent,
};
use crate::utils::sse::SseEvent;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticToolCall {
    pub name: Option<String>,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticReplaySnapshot {
    pub stream_id: Option<String>,
    pub model: Option<String>,
    pub text: String,
    pub reasoning: String,
    pub finish_reason: Option<String>,
    pub usage: Option<(u32, u32, u32)>,
    pub tool_calls: Vec<SemanticToolCall>,
    pub binary_payload_count: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayRegressionReport {
    pub fixture_name: String,
    pub source_api: LlmApiType,
    pub target_api: LlmApiType,
    pub source: SemanticReplaySnapshot,
    pub target: SemanticReplaySnapshot,
    pub source_frame_count: usize,
    pub expected_min_transformed_frame_count: usize,
    pub transformed_frame_count: usize,
    pub schema_conformant: bool,
    pub schema_errors: Vec<String>,
    pub preserved_text: bool,
    pub preserved_reasoning: bool,
    pub preserved_tool_calls: bool,
    pub preserved_finish_reason: bool,
    pub preserved_usage: bool,
    pub preserved_binary_payloads: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayRegressionSummary {
    pub fixture_count: usize,
    pub transformed_fixture_count: usize,
    pub schema_conformance_required: bool,
    pub schema_conformant_fixture_count: usize,
    pub provider_schema_conformant: bool,
    pub preserved_text_count: usize,
    pub preserved_reasoning_count: usize,
    pub preserved_tool_call_count: usize,
    pub preserved_finish_reason_count: usize,
    pub preserved_usage_count: usize,
    pub preserved_binary_payload_count: usize,
    pub all_frames_emitted: bool,
    pub passed: bool,
    pub reports: Vec<ReplayRegressionReport>,
}
#[derive(Debug, Clone, Copy)]
pub(super) struct ReplayFixtureCase {
    pub(super) fixture_name: &'static str,
    pub(super) source_api: LlmApiType,
    pub(super) target_api: LlmApiType,
    pub(super) expected_min_transformed_frame_count: usize,
    pub(super) expect_reasoning_preserved: bool,
    pub(super) fixture_json: &'static str,
}
pub(super) fn stage2_replay_fixture_cases() -> Vec<ReplayFixtureCase> {
    vec![
        ReplayFixtureCase {
            fixture_name: "anthropic_tool_use_json_delta",
            source_api: LlmApiType::Anthropic,
            target_api: LlmApiType::Responses,
            expected_min_transformed_frame_count: 11,
            expect_reasoning_preserved: true,
            fixture_json: include_str!("../testdata/anthropic_tool_use_json_delta.json"),
        },
        ReplayFixtureCase {
            fixture_name: "anthropic_text_stream",
            source_api: LlmApiType::Anthropic,
            target_api: LlmApiType::Openai,
            expected_min_transformed_frame_count: 3,
            expect_reasoning_preserved: true,
            fixture_json: include_str!("../testdata/anthropic_text_stream.json"),
        },
        ReplayFixtureCase {
            fixture_name: "responses_reasoning_function_call",
            source_api: LlmApiType::Responses,
            target_api: LlmApiType::Openai,
            expected_min_transformed_frame_count: 9,
            expect_reasoning_preserved: false,
            fixture_json: include_str!("../testdata/responses_reasoning_function_call.json"),
        },
        ReplayFixtureCase {
            fixture_name: "responses_formal_item_lifecycle",
            source_api: LlmApiType::Responses,
            target_api: LlmApiType::Openai,
            expected_min_transformed_frame_count: 4,
            expect_reasoning_preserved: true,
            fixture_json: include_str!("../testdata/responses_formal_item_lifecycle.json"),
        },
        ReplayFixtureCase {
            fixture_name: "gemini_function_call_stream",
            source_api: LlmApiType::Gemini,
            target_api: LlmApiType::Openai,
            expected_min_transformed_frame_count: 1,
            expect_reasoning_preserved: true,
            fixture_json: include_str!("../testdata/gemini_function_call_stream.json"),
        },
        ReplayFixtureCase {
            fixture_name: "gemini_text_tool_multiframe_stream",
            source_api: LlmApiType::Gemini,
            target_api: LlmApiType::Responses,
            expected_min_transformed_frame_count: 7,
            expect_reasoning_preserved: true,
            fixture_json: include_str!("../testdata/gemini_text_tool_multiframe_stream.json"),
        },
        ReplayFixtureCase {
            fixture_name: "openai_tool_stream",
            source_api: LlmApiType::Openai,
            target_api: LlmApiType::Responses,
            expected_min_transformed_frame_count: 7,
            expect_reasoning_preserved: true,
            fixture_json: include_str!("../testdata/openai_tool_stream.json"),
        },
        ReplayFixtureCase {
            fixture_name: "openai_compatible_deepseek_tool_stream",
            source_api: LlmApiType::Openai,
            target_api: LlmApiType::Responses,
            expected_min_transformed_frame_count: 7,
            expect_reasoning_preserved: true,
            fixture_json: include_str!("../testdata/openai_compatible_deepseek_tool_stream.json"),
        },
        ReplayFixtureCase {
            fixture_name: "gemini_multimodal_tool_stream",
            source_api: LlmApiType::Gemini,
            target_api: LlmApiType::Responses,
            expected_min_transformed_frame_count: 7,
            expect_reasoning_preserved: true,
            fixture_json: include_str!("../testdata/gemini_multimodal_tool_stream.json"),
        },
    ]
}

pub(super) fn load_sse_fixture(raw: &str) -> Vec<SseEvent> {
    serde_json::from_str(raw).expect("valid SSE fixture")
}

pub(super) fn semantic_snapshot_from_stream_events(
    events: impl IntoIterator<Item = UnifiedStreamEvent>,
) -> SemanticReplaySnapshot {
    let mut snapshot = SemanticReplaySnapshot::default();
    let mut tool_calls: BTreeMap<u32, SemanticToolCall> = BTreeMap::new();

    for event in events {
        match event {
            UnifiedStreamEvent::MessageStart { id, model, .. } => {
                if snapshot.stream_id.is_none() {
                    snapshot.stream_id = id;
                }
                if snapshot.model.is_none() {
                    snapshot.model = model.filter(|value| !value.is_empty());
                }
            }
            UnifiedStreamEvent::ContentBlockDelta { text, .. } => {
                snapshot.text.push_str(&text);
            }
            UnifiedStreamEvent::ReasoningDelta { text, .. } => {
                snapshot.reasoning.push_str(&text);
            }
            UnifiedStreamEvent::ToolCallStart { index, name, .. } => {
                tool_calls.entry(index).or_default().name = Some(name);
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                name,
                arguments,
                ..
            } => {
                let entry = tool_calls.entry(index).or_default();
                if entry.name.is_none() {
                    entry.name = name;
                }
                entry.arguments.push_str(&arguments);
            }
            UnifiedStreamEvent::MessageDelta { finish_reason } => {
                if finish_reason.is_some() {
                    snapshot.finish_reason = finish_reason;
                }
            }
            UnifiedStreamEvent::Usage { usage } => {
                snapshot.usage =
                    Some((usage.input_tokens, usage.output_tokens, usage.total_tokens));
            }
            UnifiedStreamEvent::BlobDelta { .. } => {
                snapshot.binary_payload_count += 1;
            }
            UnifiedStreamEvent::Error { .. } => {
                snapshot.error_count += 1;
            }
            UnifiedStreamEvent::ItemAdded { .. }
            | UnifiedStreamEvent::ItemDone { .. }
            | UnifiedStreamEvent::MessageStop
            | UnifiedStreamEvent::ContentPartAdded { .. }
            | UnifiedStreamEvent::ContentPartDone { .. }
            | UnifiedStreamEvent::ContentBlockStart { .. }
            | UnifiedStreamEvent::ContentBlockStop { .. }
            | UnifiedStreamEvent::ToolCallStop { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
            | UnifiedStreamEvent::ReasoningStart { .. }
            | UnifiedStreamEvent::ReasoningStop { .. } => {}
        }
    }

    snapshot.tool_calls = tool_calls.into_values().collect();
    snapshot
}

pub(super) fn semantic_snapshot_from_unified_chunks(
    chunks: impl IntoIterator<Item = UnifiedChunkResponse>,
) -> SemanticReplaySnapshot {
    let mut snapshot = SemanticReplaySnapshot::default();
    let mut tool_calls: BTreeMap<u32, SemanticToolCall> = BTreeMap::new();

    for chunk in chunks {
        if snapshot.stream_id.is_none() && !chunk.id.is_empty() {
            snapshot.stream_id = Some(chunk.id.clone());
        }
        if snapshot.model.is_none() {
            snapshot.model = chunk.model.clone().filter(|value| !value.is_empty());
        }
        if let Some(usage) = chunk.usage {
            snapshot.usage = Some((usage.input_tokens, usage.output_tokens, usage.total_tokens));
        }

        for choice in chunk.choices {
            if choice.finish_reason.is_some() {
                snapshot.finish_reason = choice.finish_reason;
            }

            for part in choice.delta.content {
                match part {
                    UnifiedContentPartDelta::TextDelta { text, .. } => {
                        snapshot.text.push_str(&text);
                    }
                    UnifiedContentPartDelta::ToolCallDelta(tool_call) => {
                        let entry = tool_calls.entry(tool_call.index).or_default();
                        if entry.name.is_none() {
                            entry.name = tool_call.name;
                        }
                        if let Some(arguments) = tool_call.arguments {
                            entry.arguments.push_str(&arguments);
                        }
                    }
                    UnifiedContentPartDelta::ImageDelta { .. } => {
                        snapshot.binary_payload_count += 1;
                    }
                }
            }
        }
    }

    snapshot.tool_calls = tool_calls.into_values().collect();
    snapshot
}

pub(super) fn source_fixture_to_semantics(
    source_api: LlmApiType,
    fixture: &[SseEvent],
) -> SemanticReplaySnapshot {
    match source_api {
        LlmApiType::Anthropic => {
            semantic_snapshot_from_stream_events(fixture.iter().flat_map(|event| {
                let parsed: anthropic::AnthropicEvent =
                    serde_json::from_str(&event.data).expect("valid anthropic fixture");
                anthropic::anthropic_event_to_unified_stream_events(parsed)
            }))
        }
        LlmApiType::Responses => {
            semantic_snapshot_from_stream_events(fixture.iter().flat_map(|event| {
                let parsed: responses::ResponsesChunkResponse =
                    serde_json::from_str(&event.data).expect("valid responses fixture");
                responses::responses_chunk_to_unified_stream_events(parsed)
            }))
        }
        LlmApiType::Gemini => semantic_snapshot_from_unified_chunks(
            fixture
                .iter()
                .filter(|event| event.event.is_none())
                .map(|event| {
                    let parsed: gemini::GeminiChunkResponse =
                        serde_json::from_str(&event.data).expect("valid gemini fixture");
                    UnifiedChunkResponse::from(parsed)
                }),
        ),
        LlmApiType::Openai | LlmApiType::GeminiOpenai => semantic_snapshot_from_unified_chunks(
            fixture
                .iter()
                .filter(|event| event.event.is_none())
                .map(|event| {
                    let parsed: openai::OpenAiChunkResponse =
                        serde_json::from_str(&event.data).expect("valid openai fixture");
                    UnifiedChunkResponse::from(parsed)
                }),
        ),
        LlmApiType::Ollama => semantic_snapshot_from_unified_chunks(
            fixture
                .iter()
                .filter(|event| event.event.is_none())
                .map(|event| {
                    let parsed: ollama::OllamaChunkResponse =
                        serde_json::from_str(&event.data).expect("valid ollama fixture");
                    UnifiedChunkResponse::from(parsed)
                }),
        ),
    }
}

pub(super) fn replay_fixture_through_transformer(
    source_api: LlmApiType,
    target_api: LlmApiType,
    fixture: &[SseEvent],
) -> Vec<SseEvent> {
    let mut transformer = StreamTransformer::new(source_api, target_api);
    fixture
        .iter()
        .flat_map(|event| {
            transformer
                .transform_event(event.clone())
                .unwrap_or_default()
        })
        .collect()
}

pub(super) fn validate_provider_native_schema(
    target_api: LlmApiType,
    frames: &[SseEvent],
) -> Vec<String> {
    frames
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            let result: Result<(), String> = match target_api {
                LlmApiType::Anthropic => {
                    serde_json::from_str::<anthropic::AnthropicEvent>(&event.data)
                        .map(|_| ())
                        .map_err(|err| err.to_string())
                }
                LlmApiType::Responses => {
                    match serde_json::from_str::<Value>(&event.data).map_err(|err| err.to_string())
                    {
                        Ok(value) => {
                            let is_legacy_wrapped_delta = value.get("delta").is_some()
                                && value.get("id").is_some()
                                && value.get("model").is_some()
                                && value.get("type").is_none();
                            if is_legacy_wrapped_delta {
                                Err(
                                    "legacy wrapped delta envelope is not provider-native schema"
                                        .to_string(),
                                )
                            } else if value.get("type").is_none() {
                                Err("responses frame missing top-level type".to_string())
                            } else {
                                serde_json::from_value::<responses::ResponsesChunkResponse>(value)
                                    .map(|_| ())
                                    .map_err(|err| err.to_string())
                            }
                        }
                        Err(err) => Err(err),
                    }
                }
                LlmApiType::Gemini => {
                    if event.event.is_some() {
                        Ok(())
                    } else {
                        serde_json::from_str::<gemini::GeminiChunkResponse>(&event.data)
                            .map(|_| ())
                            .map_err(|err| err.to_string())
                    }
                }
                LlmApiType::Openai | LlmApiType::GeminiOpenai => {
                    if event.event.is_some() || event.data == "[DONE]" {
                        Ok(())
                    } else {
                        serde_json::from_str::<openai::OpenAiChunkResponse>(&event.data)
                            .map(|_| ())
                            .map_err(|err| err.to_string())
                    }
                }
                LlmApiType::Ollama => {
                    if event.event.is_some() {
                        Ok(())
                    } else {
                        serde_json::from_str::<ollama::OllamaChunkResponse>(&event.data)
                            .map(|_| ())
                            .map_err(|err| err.to_string())
                    }
                }
            };

            result.err().map(|err| format!("frame {index}: {err}"))
        })
        .collect()
}

pub(super) fn build_replay_regression_report(case: ReplayFixtureCase) -> ReplayRegressionReport {
    let fixture = load_sse_fixture(case.fixture_json);
    let source = source_fixture_to_semantics(case.source_api, &fixture);
    let transformed =
        replay_fixture_through_transformer(case.source_api, case.target_api, &fixture);
    let target = source_fixture_to_semantics(case.target_api, &transformed);
    let schema_errors = validate_provider_native_schema(case.target_api, &transformed);

    ReplayRegressionReport {
        fixture_name: case.fixture_name.to_string(),
        source_api: case.source_api,
        target_api: case.target_api,
        schema_conformant: schema_errors.is_empty(),
        schema_errors,
        preserved_text: source.text == target.text,
        preserved_reasoning: source.reasoning == target.reasoning,
        preserved_tool_calls: source.tool_calls == target.tool_calls,
        preserved_finish_reason: source.finish_reason == target.finish_reason,
        preserved_usage: source.usage == target.usage,
        preserved_binary_payloads: source.binary_payload_count == target.binary_payload_count,
        source_frame_count: fixture.len(),
        expected_min_transformed_frame_count: case.expected_min_transformed_frame_count,
        transformed_frame_count: transformed.len(),
        source,
        target,
    }
}

pub(super) fn replay_report_emitted_all_frames(report: &ReplayRegressionReport) -> bool {
    report.transformed_frame_count >= report.expected_min_transformed_frame_count
}

pub(super) fn replay_summary_passed(
    fixture_count: usize,
    expected_reasoning_preserved_count: usize,
    schema_conformance_required: bool,
    provider_schema_conformant: bool,
    all_frames_emitted: bool,
    preserved_text_count: usize,
    preserved_reasoning_count: usize,
    preserved_tool_call_count: usize,
    preserved_finish_reason_count: usize,
    preserved_usage_count: usize,
    preserved_binary_payload_count: usize,
) -> bool {
    fixture_count > 0
        && (!schema_conformance_required || provider_schema_conformant)
        && all_frames_emitted
        && preserved_text_count == fixture_count
        && preserved_tool_call_count == fixture_count
        && preserved_finish_reason_count == fixture_count
        && preserved_usage_count == fixture_count
        && preserved_reasoning_count == expected_reasoning_preserved_count
        && preserved_binary_payload_count == fixture_count
}

pub(super) fn build_stage2_replay_regression_summary_with_options(
    require_native_schema_conformance: bool,
) -> ReplayRegressionSummary {
    let cases = stage2_replay_fixture_cases();
    let expected_reasoning_preserved_count = cases
        .iter()
        .filter(|case| case.expect_reasoning_preserved)
        .count();
    let reports: Vec<_> = cases
        .into_iter()
        .map(build_replay_regression_report)
        .collect();

    let fixture_count = reports.len();
    let transformed_fixture_count = reports
        .iter()
        .filter(|report| replay_report_emitted_all_frames(report))
        .count();
    let schema_conformant_fixture_count = reports
        .iter()
        .filter(|report| report.schema_conformant)
        .count();
    let preserved_text_count = reports
        .iter()
        .filter(|report| report.preserved_text)
        .count();
    let preserved_reasoning_count = reports
        .iter()
        .filter(|report| report.preserved_reasoning)
        .count();
    let preserved_tool_call_count = reports
        .iter()
        .filter(|report| report.preserved_tool_calls)
        .count();
    let preserved_finish_reason_count = reports
        .iter()
        .filter(|report| report.preserved_finish_reason)
        .count();
    let preserved_usage_count = reports
        .iter()
        .filter(|report| report.preserved_usage)
        .count();
    let preserved_binary_payload_count = reports
        .iter()
        .filter(|report| report.preserved_binary_payloads)
        .count();
    let all_frames_emitted = reports.iter().all(replay_report_emitted_all_frames);
    let provider_schema_conformant = schema_conformant_fixture_count == fixture_count;
    let passed = replay_summary_passed(
        fixture_count,
        expected_reasoning_preserved_count,
        require_native_schema_conformance,
        provider_schema_conformant,
        all_frames_emitted,
        preserved_text_count,
        preserved_reasoning_count,
        preserved_tool_call_count,
        preserved_finish_reason_count,
        preserved_usage_count,
        preserved_binary_payload_count,
    );

    ReplayRegressionSummary {
        fixture_count,
        transformed_fixture_count,
        schema_conformance_required: require_native_schema_conformance,
        schema_conformant_fixture_count,
        provider_schema_conformant,
        preserved_text_count,
        preserved_reasoning_count,
        preserved_tool_call_count,
        preserved_finish_reason_count,
        preserved_usage_count,
        preserved_binary_payload_count,
        all_frames_emitted,
        passed,
        reports,
    }
}

pub fn build_stage2_replay_regression_summary() -> ReplayRegressionSummary {
    build_stage2_replay_regression_summary_with_options(true)
}
