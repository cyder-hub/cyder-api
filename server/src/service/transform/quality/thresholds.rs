use serde::{Deserialize, Serialize};

use super::benchmark::BenchmarkSummary;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkThresholds {
    pub format_version: u32,
    pub quick: bool,
    #[serde(default = "default_require_native_schema_conformance")]
    pub require_native_schema_conformance: bool,
    pub checks: Vec<BenchmarkThresholdRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkThresholdRule {
    pub kind: String,
    pub scenario: String,
    pub min_ops_per_sec: Option<f64>,
    pub max_p95_ms: Option<f64>,
    pub max_allocs_per_op: Option<f64>,
    pub max_reallocs_per_op: Option<f64>,
    pub max_alloc_mib_per_op: Option<f64>,
    pub max_peak_kib_per_op: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkThresholdCheck {
    pub kind: String,
    pub scenario: String,
    pub passed: bool,
    pub failures: Vec<String>,
}

fn default_require_native_schema_conformance() -> bool {
    true
}
pub fn evaluate_benchmark_thresholds(
    summary: &BenchmarkSummary,
    thresholds: &BenchmarkThresholds,
) -> Vec<BenchmarkThresholdCheck> {
    thresholds
        .checks
        .iter()
        .map(|rule| {
            let mut failures = Vec::new();
            let Some(scenario) = summary
                .scenarios
                .iter()
                .find(|candidate| candidate.kind == rule.kind && candidate.name == rule.scenario)
            else {
                failures.push("missing benchmark scenario".to_string());
                return BenchmarkThresholdCheck {
                    kind: rule.kind.clone(),
                    scenario: rule.scenario.clone(),
                    passed: false,
                    failures,
                };
            };

            if let Some(min_ops) = rule.min_ops_per_sec
                && scenario.throughput_ops_per_sec < min_ops
            {
                failures.push(format!(
                    "ops/s {:.0} < minimum {:.0}",
                    scenario.throughput_ops_per_sec, min_ops
                ));
            }
            if let Some(max_p95) = rule.max_p95_ms
                && scenario.p95_ms > max_p95
            {
                failures.push(format!(
                    "p95 {:.3} ms > maximum {:.3} ms",
                    scenario.p95_ms, max_p95
                ));
            }
            if let Some(max_allocs) = rule.max_allocs_per_op
                && scenario.avg_allocations > max_allocs
            {
                failures.push(format!(
                    "allocs/op {:.1} > maximum {:.1}",
                    scenario.avg_allocations, max_allocs
                ));
            }
            if let Some(max_reallocs) = rule.max_reallocs_per_op
                && scenario.avg_reallocations > max_reallocs
            {
                failures.push(format!(
                    "reallocs/op {:.1} > maximum {:.1}",
                    scenario.avg_reallocations, max_reallocs
                ));
            }
            if let Some(max_alloc_mib) = rule.max_alloc_mib_per_op {
                let alloc_mib = scenario.avg_allocated_bytes / (1024.0 * 1024.0);
                if alloc_mib > max_alloc_mib {
                    failures.push(format!(
                        "alloc MiB/op {:.3} > maximum {:.3}",
                        alloc_mib, max_alloc_mib
                    ));
                }
            }
            if let Some(max_peak_kib) = rule.max_peak_kib_per_op {
                let peak_kib = scenario.avg_peak_bytes / 1024.0;
                if peak_kib > max_peak_kib {
                    failures.push(format!(
                        "peak KiB/op {:.1} > maximum {:.1}",
                        peak_kib, max_peak_kib
                    ));
                }
            }

            BenchmarkThresholdCheck {
                kind: rule.kind.clone(),
                scenario: rule.scenario.clone(),
                passed: failures.is_empty(),
                failures,
            }
        })
        .collect()
}
