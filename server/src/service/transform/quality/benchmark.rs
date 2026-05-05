use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkScenarioMetrics {
    pub kind: String,
    pub name: String,
    pub throughput_ops_per_sec: f64,
    pub throughput_mib_per_sec: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub avg_allocations: f64,
    pub avg_reallocations: f64,
    pub avg_allocated_bytes: f64,
    pub avg_peak_bytes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkSummary {
    pub format_version: u32,
    pub quick: bool,
    pub warmup_rounds: usize,
    pub sample_rounds: usize,
    pub scenarios: Vec<BenchmarkScenarioMetrics>,
}
