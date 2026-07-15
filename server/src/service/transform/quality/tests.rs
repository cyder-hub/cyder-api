use crate::schema::enum_def::LlmApiType;
use crate::utils::sse::SseEvent;

use super::replay::{
    build_replay_regression_report, replay_report_emitted_all_frames, replay_summary_passed,
    stage2_replay_fixture_cases, validate_provider_native_schema,
};
use super::*;

#[test]
fn test_stage2_replay_regression_summary_matches_expected_counts() {
    let summary = build_stage2_replay_regression_summary();
    let schema_failures: Vec<_> = summary
        .reports
        .iter()
        .filter(|report| !report.schema_conformant)
        .map(|report| (report.fixture_name.clone(), report.schema_errors.clone()))
        .collect();
    let incomplete_fixtures: Vec<_> = summary
        .reports
        .iter()
        .filter(|report| !replay_report_emitted_all_frames(report))
        .map(|report| {
            (
                report.fixture_name.clone(),
                report.expected_min_transformed_frame_count,
                report.transformed_frame_count,
            )
        })
        .collect();

    assert_eq!(summary.fixture_count, 9);
    assert_eq!(
        summary.transformed_fixture_count, 9,
        "{incomplete_fixtures:?}"
    );
    assert!(summary.schema_conformance_required);
    assert_eq!(
        summary.schema_conformant_fixture_count, 9,
        "{schema_failures:?}"
    );
    assert!(summary.provider_schema_conformant);
    assert_eq!(summary.preserved_text_count, 9);
    assert_eq!(summary.preserved_tool_call_count, 9);
    assert_eq!(summary.preserved_finish_reason_count, 9);
    assert_eq!(summary.preserved_usage_count, 9);
    assert_eq!(summary.preserved_reasoning_count, 8);
    assert_eq!(summary.preserved_binary_payload_count, 9);
    assert!(summary.passed);
}

#[test]
fn test_replay_report_emitted_all_frames_rejects_partial_output() {
    let report = ReplayRegressionReport {
        fixture_name: "partial".to_string(),
        source_api: LlmApiType::Openai,
        target_api: LlmApiType::Responses,
        source: SemanticReplaySnapshot::default(),
        target: SemanticReplaySnapshot::default(),
        source_frame_count: 4,
        expected_min_transformed_frame_count: 3,
        transformed_frame_count: 1,
        schema_conformant: true,
        schema_errors: Vec::new(),
        preserved_text: true,
        preserved_reasoning: true,
        preserved_tool_calls: true,
        preserved_finish_reason: true,
        preserved_usage: true,
        preserved_binary_payloads: true,
    };

    assert!(!replay_report_emitted_all_frames(&report));
}

#[test]
fn test_replay_summary_gate_requires_finish_reason_preservation() {
    assert!(!replay_summary_passed(
        5, 4, true, true, true, 5, 4, 5, 4, 5, 5
    ));
    assert!(!replay_summary_passed(
        5, 4, true, false, true, 5, 4, 5, 5, 5, 5
    ));
    assert!(!replay_summary_passed(
        5, 4, true, true, false, 5, 4, 5, 5, 5, 5
    ));
    assert!(replay_summary_passed(
        5, 4, true, true, true, 5, 4, 5, 5, 5, 5
    ));
    assert!(replay_summary_passed(
        5, 4, false, false, true, 5, 4, 5, 5, 5, 5
    ));
}

#[test]
fn test_replay_report_responses_formal_lifecycle_preserves_semantics_and_schema() {
    let case = stage2_replay_fixture_cases()
        .into_iter()
        .find(|case| case.fixture_name == "responses_formal_item_lifecycle")
        .expect("responses formal lifecycle case");
    let report = build_replay_regression_report(case);

    assert!(report.schema_conformant, "{:?}", report.schema_errors);
    assert!(report.preserved_text);
    assert!(report.preserved_tool_calls);
    assert!(report.preserved_finish_reason);
    assert!(report.preserved_usage);
    assert_eq!(report.source.text, "Need weather.");
    assert_eq!(report.target.text, "Need weather.");
    assert_eq!(
        report.target.tool_calls,
        vec![SemanticToolCall {
            name: Some("lookup_weather".to_string()),
            arguments: "{\"city\":\"Boston\"}".to_string(),
        }]
    );
}

#[test]
fn test_replay_report_openai_compatible_fixture_preserves_model_and_schema() {
    let case = stage2_replay_fixture_cases()
        .into_iter()
        .find(|case| case.fixture_name == "openai_compatible_deepseek_tool_stream")
        .expect("openai compatible case");
    let report = build_replay_regression_report(case);

    assert!(report.schema_conformant, "{:?}", report.schema_errors);
    assert!(report.preserved_text);
    assert!(report.preserved_tool_calls);
    assert_eq!(report.source.model.as_deref(), Some("deepseek-chat"));
    assert_eq!(report.target.model.as_deref(), Some("deepseek-chat"));
    assert_eq!(report.target.text, "One moment.");
}

#[test]
fn test_provider_native_schema_validation_rejects_legacy_wrapped_responses_frame() {
    let failures = validate_provider_native_schema(
        LlmApiType::Responses,
        &[SseEvent {
            data: serde_json::to_string(&serde_json::json!({
                "id": "resp_legacy",
                "model": "gpt-4.1",
                "delta": {
                    "type": "response.output_text.delta",
                    "item_id": "msg_1",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "hello"
                }
            }))
            .unwrap(),
            ..Default::default()
        }],
    );

    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("legacy wrapped delta envelope"));
}

#[test]
fn test_benchmark_threshold_evaluation_reports_regressions() {
    let summary = BenchmarkSummary {
        format_version: 1,
        quick: true,
        warmup_rounds: 2,
        sample_rounds: 8,
        scenarios: vec![BenchmarkScenarioMetrics {
            kind: "stream".to_string(),
            name: "responses_to_openai_long_session".to_string(),
            throughput_ops_per_sec: 2400.0,
            throughput_mib_per_sec: 64.0,
            p50_ms: 0.40,
            p95_ms: 0.48,
            avg_allocations: 5000.0,
            avg_reallocations: 330.0,
            avg_allocated_bytes: 0.60 * 1024.0 * 1024.0,
            avg_peak_bytes: 15.5 * 1024.0,
        }],
    };
    let thresholds = BenchmarkThresholds {
        format_version: 1,
        quick: true,
        require_native_schema_conformance: true,
        checks: vec![BenchmarkThresholdRule {
            kind: "stream".to_string(),
            scenario: "responses_to_openai_long_session".to_string(),
            min_ops_per_sec: Some(2600.0),
            max_p95_ms: Some(0.40),
            max_allocs_per_op: Some(4600.0),
            max_reallocs_per_op: Some(320.0),
            max_alloc_mib_per_op: Some(0.52),
            max_peak_kib_per_op: Some(15.0),
        }],
    };

    let checks = evaluate_benchmark_thresholds(&summary, &thresholds);

    assert_eq!(checks.len(), 1);
    assert!(!checks[0].passed);
    assert_eq!(checks[0].failures.len(), 6);
}

#[test]
fn test_quality_report_file_helpers_round_trip_gate_artifacts() {
    let temp_dir = tempfile::tempdir().expect("quality gate temp dir");
    let thresholds_path = temp_dir.path().join("thresholds.json");
    let report_path = temp_dir.path().join("artifacts/quality-report.json");
    let thresholds = BenchmarkThresholds {
        format_version: 1,
        quick: true,
        require_native_schema_conformance: true,
        checks: Vec::new(),
    };
    std::fs::write(
        &thresholds_path,
        serde_json::to_vec_pretty(&thresholds).expect("serialize thresholds"),
    )
    .expect("write thresholds");

    let loaded = load_benchmark_thresholds(&thresholds_path).expect("load thresholds");
    assert_eq!(loaded, thresholds);

    let report = build_transform_quality_report(
        BenchmarkSummary {
            format_version: 1,
            quick: true,
            warmup_rounds: 2,
            sample_rounds: 8,
            scenarios: Vec::new(),
        },
        &loaded,
    );
    assert!(report.passed);

    write_transform_quality_report(&report_path, &report).expect("write quality report");
    let persisted: TransformQualityReport = serde_json::from_slice(
        &std::fs::read(&report_path).expect("read persisted quality report"),
    )
    .expect("parse persisted quality report");
    assert_eq!(persisted, report);
}
