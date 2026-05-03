use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricsScopeType {
    Global,
    Provider,
    Model,
    ApiKey,
    ProviderApiKey,
    ProviderModel,
}

impl MetricsScopeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Provider => "provider",
            Self::Model => "model",
            Self::ApiKey => "api_key",
            Self::ProviderApiKey => "provider_api_key",
            Self::ProviderModel => "provider_model",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsScope {
    pub scope_type: MetricsScopeType,
    pub scope_id: String,
    pub scope_label: Option<String>,
}

impl MetricsScope {
    pub fn global() -> Self {
        Self {
            scope_type: MetricsScopeType::Global,
            scope_id: "global".to_string(),
            scope_label: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsWorkerTickResult {
    pub processed: u64,
    pub skipped: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsIngestOutcome {
    pub request_log_id: i64,
    pub ingested: bool,
    pub skipped_existing: bool,
    pub request_rollup_deltas: usize,
    pub attempt_rollup_deltas: usize,
    pub http_status_deltas: usize,
    pub cost_rollup_deltas: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsReconciliationParams {
    pub start_time: i64,
    pub end_time: i64,
    pub limit: usize,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsReconciliationSummary {
    pub scanned: usize,
    pub ingested: usize,
    pub skipped: usize,
    pub failed: usize,
    pub oldest_uningested: Option<i64>,
    pub newest_uningested: Option<i64>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsRepairParams {
    pub start_time: i64,
    pub end_time: i64,
    pub limit: usize,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsRepairSummary {
    pub requested_start_time: i64,
    pub requested_end_time: i64,
    pub expanded_replay_start_time: i64,
    pub expanded_replay_end_time: i64,
    pub deleted_ingest_markers: usize,
    pub deleted_request_rollups: usize,
    pub deleted_attempt_rollups: usize,
    pub deleted_http_status_rollups: usize,
    pub deleted_cost_rollups: usize,
    pub reconciliation: MetricsReconciliationSummary,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsIngestStatus {
    pub ingested_request_log_count: i64,
    pub pending_reconciliation_count: Option<i64>,
    pub generated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsTimeseriesPoint {
    pub bucket_start_ms: i64,
    pub scope_type: String,
    pub scope_id: String,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub total_tokens: i64,
}
