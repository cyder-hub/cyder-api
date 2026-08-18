mod benchmark;
mod contracts;
mod replay;
mod report;
mod thresholds;

#[cfg(test)]
pub(crate) mod tests;

pub use benchmark::{BenchmarkScenarioMetrics, BenchmarkSummary};
pub use contracts::{
    TransformContractCaseReport, TransformContractSummary, build_transform_contract_summary,
};
pub use replay::{
    ReplayRegressionReport, ReplayRegressionSummary, SemanticReplaySnapshot, SemanticToolCall,
    build_stage2_replay_regression_summary,
};
pub use report::{
    TransformQualityReport, build_transform_quality_report, load_benchmark_thresholds,
    write_transform_quality_report,
};
pub use thresholds::{
    BenchmarkThresholdCheck, BenchmarkThresholdRule, BenchmarkThresholds,
    evaluate_benchmark_thresholds,
};
