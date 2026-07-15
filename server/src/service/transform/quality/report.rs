use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::benchmark::BenchmarkSummary;
use super::replay::{ReplayRegressionSummary, build_stage2_replay_regression_summary_with_options};
use super::thresholds::{
    BenchmarkThresholdCheck, BenchmarkThresholds, evaluate_benchmark_thresholds,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformQualityReport {
    pub format_version: u32,
    pub replay_summary: ReplayRegressionSummary,
    pub benchmark_summary: BenchmarkSummary,
    pub threshold_checks: Vec<BenchmarkThresholdCheck>,
    pub passed: bool,
}
pub fn build_transform_quality_report(
    benchmark_summary: BenchmarkSummary,
    thresholds: &BenchmarkThresholds,
) -> TransformQualityReport {
    let replay_summary = build_stage2_replay_regression_summary_with_options(
        thresholds.require_native_schema_conformance,
    );
    let threshold_checks = evaluate_benchmark_thresholds(&benchmark_summary, thresholds);
    let passed = replay_summary.passed && threshold_checks.iter().all(|check| check.passed);

    TransformQualityReport {
        format_version: 1,
        replay_summary,
        benchmark_summary,
        threshold_checks,
        passed,
    }
}

pub fn load_benchmark_thresholds(path: &Path) -> Result<BenchmarkThresholds, String> {
    let payload =
        fs::read(path).map_err(|err| format!("read thresholds {}: {err}", path.display()))?;
    serde_json::from_slice(&payload)
        .map_err(|err| format!("parse thresholds {}: {err}", path.display()))
}

pub fn write_transform_quality_report(
    path: &Path,
    report: &TransformQualityReport,
) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create report directory {}: {err}", parent.display()))?;
    }
    let payload = serde_json::to_vec_pretty(report)
        .map_err(|err| format!("serialize transform quality report: {err}"))?;
    fs::write(path, payload).map_err(|err| format!("write report {}: {err}", path.display()))
}
