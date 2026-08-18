use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::service::metrics::dashboard::MetricsDashboardTodayStats;
use crate::service::metrics::provider_runtime::{
    ProviderRuntimeItem, ProviderRuntimeLevel, ProviderRuntimeSummary, ProviderRuntimeWindow,
};
use crate::service::metrics::runtime_overview::{
    DashboardCostProviderReadItem, DashboardOperationalSignalsReadModel,
    DashboardProviderSignalReadItem, DashboardTopProviderReadItem,
    operational_signals_from_runtime_items, top_providers_from_runtime_items,
};
use crate::service::runtime::RuntimeStateBackendOperatorStatus;
use crate::{
    controller::error::BaseError,
    database::stat::{
        DashboardOverviewStats as DbDashboardOverviewStats, DashboardTopModelItem,
        SystemOverviewStats, TodayRequestLogStats, UsageStatsGroupBy as DbUsageStatsGroupBy,
        UsageStatsQueryItem, get_system_overview_stats, get_today_request_log_stats,
    },
    utils::HttpResult,
};
use axum::{
    extract::{Query, State},
    routing::get,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(test)]
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Serialize, Debug)]
pub struct SystemOverviewResponse {
    #[serde(flatten)]
    stats: SystemOverviewStats,
    runtime_state_backend: RuntimeStateBackendOperatorStatus,
}

#[derive(Deserialize, Debug)]
pub struct UsageStatsParams {
    #[serde(default = "default_start_time")]
    start_time: i64, // Milliseconds
    #[serde(default = "default_end_time")]
    end_time: i64, // Milliseconds
    #[serde(default = "default_interval")]
    interval: Interval, // "month", "day", "hour"
    provider_id: Option<i64>,
    model_id: Option<i64>,
    api_key_id: Option<i64>,
    provider_api_key_id: Option<i64>,
    #[serde(default = "default_group_by")]
    group_by: UsageGroupBy,
    #[serde(default = "default_metric")]
    metric: UsageMetric,
    #[serde(default = "default_top_n")]
    top_n: usize,
    #[serde(default = "default_include_others")]
    include_others: bool,
}

fn default_start_time() -> i64 {
    0
}

fn default_end_time() -> i64 {
    Utc::now().timestamp_millis()
}

fn default_interval() -> Interval {
    Interval::Day
}

fn default_group_by() -> UsageGroupBy {
    UsageGroupBy::Model
}

fn default_metric() -> UsageMetric {
    UsageMetric::TotalTokens
}

fn default_top_n() -> usize {
    6
}

fn default_include_others() -> bool {
    true
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Interval {
    Minute,
    Hour,
    Day,
    Month,
}

impl Interval {
    #[allow(dead_code)]
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "minute" => Ok(Interval::Minute),
            "hour" => Ok(Interval::Hour),
            "day" => Ok(Interval::Day),
            "month" => Ok(Interval::Month),
            _ => Err(format!(
                "Invalid interval: {}. Supported intervals are 'minute', 'hour', 'day', 'month'.",
                s
            )),
        }
    }
}

impl Interval {
    fn as_str(self) -> &'static str {
        match self {
            Interval::Minute => "minute",
            Interval::Hour => "hour",
            Interval::Day => "day",
            Interval::Month => "month",
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum UsageGroupBy {
    Provider,
    Model,
    ApiKey,
}

impl UsageGroupBy {
    fn to_database_group_by(self) -> DbUsageStatsGroupBy {
        match self {
            UsageGroupBy::Provider => DbUsageStatsGroupBy::Provider,
            UsageGroupBy::Model => DbUsageStatsGroupBy::Model,
            UsageGroupBy::ApiKey => DbUsageStatsGroupBy::ApiKey,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum UsageMetric {
    TotalInputTokens,
    TotalOutputTokens,
    TotalReasoningTokens,
    TotalTokens,
    RequestCount,
    TotalCost,
    SuccessRate,
    AvgLatency,
    AvgTimeToFirstResponseBody,
    AvgTtft,
    ErrorCount,
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct UsageStatItem {
    provider_id: Option<i64>,
    model_id: Option<i64>,
    api_key_id: Option<i64>,
    provider_key: Option<String>,
    model_name: Option<String>,
    real_model_name: Option<String>,
    api_key_name: Option<String>,
    group_key: String,
    group_label: String,
    group_detail: Option<String>,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_reasoning_tokens: i64,
    total_tokens: i64,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    success_rate: Option<f64>,
    avg_time_to_first_response_body_ms: Option<f64>,
    time_to_first_response_body_sample_count: i64,
    avg_ttft_ms: Option<f64>,
    ttft_sample_count: i64,
    avg_total_latency_ms: Option<f64>,
    total_latency_sample_count: i64,
    total_cost: HashMap<String, i64>,
    is_other: bool,
}

#[derive(Serialize, Debug, Default)]
pub struct DashboardOverviewStats {
    provider_count: i64,
    enabled_provider_count: i64,
    model_count: i64,
    enabled_model_count: i64,
    provider_key_count: i64,
    enabled_provider_key_count: i64,
    api_key_count: i64,
    enabled_api_key_count: i64,
}

impl From<DbDashboardOverviewStats> for DashboardOverviewStats {
    fn from(value: DbDashboardOverviewStats) -> Self {
        Self {
            provider_count: value.provider_count,
            enabled_provider_count: value.enabled_provider_count,
            model_count: value.model_count,
            enabled_model_count: value.enabled_model_count,
            provider_key_count: value.provider_key_count,
            enabled_provider_key_count: value.enabled_provider_key_count,
            api_key_count: value.api_key_count,
            enabled_api_key_count: value.enabled_api_key_count,
        }
    }
}

#[derive(Serialize, Debug, Default)]
pub struct DashboardTodayStats {
    request_count: i64,
    success_count: i64,
    error_count: i64,
    success_rate: Option<f64>,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_reasoning_tokens: i64,
    total_tokens: i64,
    total_cost: HashMap<String, i64>,
    avg_time_to_first_response_body_ms: Option<f64>,
    time_to_first_response_body_sample_count: i64,
    avg_ttft_ms: Option<f64>,
    ttft_sample_count: i64,
    avg_total_latency_ms: Option<f64>,
    total_latency_sample_count: i64,
    active_provider_count: i64,
    active_model_count: i64,
    active_api_key_count: i64,
}

impl From<MetricsDashboardTodayStats> for DashboardTodayStats {
    fn from(value: MetricsDashboardTodayStats) -> Self {
        Self {
            request_count: value.request_count,
            success_count: value.success_count,
            error_count: value.error_count,
            success_rate: value.success_rate,
            total_input_tokens: value.total_input_tokens,
            total_output_tokens: value.total_output_tokens,
            total_reasoning_tokens: value.total_reasoning_tokens,
            total_tokens: value.total_tokens,
            total_cost: value.total_cost,
            avg_time_to_first_response_body_ms: value.avg_time_to_first_response_body_ms,
            time_to_first_response_body_sample_count: value
                .time_to_first_response_body_sample_count,
            avg_ttft_ms: value.avg_ttft_ms,
            ttft_sample_count: value.ttft_sample_count,
            avg_total_latency_ms: value.avg_total_latency_ms,
            total_latency_sample_count: value.total_latency_sample_count,
            active_provider_count: value.active_provider_count,
            active_model_count: value.active_model_count,
            active_api_key_count: value.active_api_key_count,
        }
    }
}

#[derive(Serialize, Debug)]
pub struct UsageStatsPeriod {
    time: i64, // Timestamp for the beginning of the period (milliseconds)
    data: Vec<UsageStatItem>,
}

#[derive(Serialize, Debug)]
pub struct DashboardResponse {
    overview: DashboardOverviewStats,
    today: DashboardTodayStats,
    runtime: DashboardRuntimeSummary,
    runtime_state_backend: RuntimeStateBackendOperatorStatus,
    operational_signals: DashboardOperationalSignals,
    top_providers: Vec<DashboardTopProviderItem>,
    top_models: Vec<DashboardTopModelItem>,
}

#[derive(Serialize, Debug)]
pub struct DashboardOperationsSection {
    operational_signals: DashboardOperationalSignals,
    top_providers: Vec<DashboardTopProviderItem>,
    top_models: Vec<DashboardTopModelItem>,
}

#[derive(Serialize, Debug)]
pub struct DashboardRuntimeSummary {
    window: ProviderRuntimeWindow,
    total_provider_count: i64,
    enabled_provider_count: i64,
    total_source_count: i64,
    enabled_source_count: i64,
    healthy_count: i64,
    degraded_count: i64,
    no_traffic_count: i64,
}

impl From<ProviderRuntimeSummary> for DashboardRuntimeSummary {
    fn from(value: ProviderRuntimeSummary) -> Self {
        Self {
            window: value.window,
            total_provider_count: value.total_provider_count,
            enabled_provider_count: value.enabled_provider_count,
            total_source_count: value.total_source_count,
            enabled_source_count: value.enabled_source_count,
            healthy_count: value.healthy_count,
            degraded_count: value.degraded_count,
            no_traffic_count: value.no_traffic_count,
        }
    }
}

#[derive(Serialize, Debug, Default)]
pub struct DashboardOperationalSignals {
    degraded_providers: Vec<DashboardProviderSignalItem>,
    top_error_providers: Vec<DashboardProviderSignalItem>,
    top_cost_providers: Vec<DashboardCostProviderItem>,
    top_cost_models: Vec<DashboardCostModelItem>,
}

#[derive(Serialize, Debug)]
pub struct DashboardProviderSignalItem {
    provider_id: i64,
    provider_key: String,
    provider_name: String,
    runtime_level: ProviderRuntimeLevel,
    request_count: i64,
    error_count: i64,
    success_rate: Option<f64>,
    avg_total_latency_ms: Option<f64>,
    last_error_at: Option<i64>,
    last_error_summary: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct DashboardCostProviderItem {
    provider_id: i64,
    provider_key: String,
    provider_name: String,
    request_count: i64,
    success_rate: Option<f64>,
    avg_total_latency_ms: Option<f64>,
    total_cost: HashMap<String, i64>,
}

#[derive(Serialize, Debug)]
pub struct DashboardCostModelItem {
    provider_id: i64,
    provider_key: String,
    model_id: i64,
    model_name: String,
    real_model_name: Option<String>,
    request_count: i64,
    total_tokens: i64,
    total_cost: HashMap<String, i64>,
}

#[derive(Serialize, Debug)]
pub struct DashboardTopProviderItem {
    provider_id: i64,
    provider_key: String,
    provider_name: String,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    success_rate: Option<f64>,
    total_cost: HashMap<String, i64>,
    avg_time_to_first_response_body_ms: Option<f64>,
    time_to_first_response_body_sample_count: i64,
    avg_ttft_ms: Option<f64>,
    ttft_sample_count: i64,
    avg_total_latency_ms: Option<f64>,
    total_latency_sample_count: i64,
}

impl From<DashboardProviderSignalReadItem> for DashboardProviderSignalItem {
    fn from(value: DashboardProviderSignalReadItem) -> Self {
        Self {
            provider_id: value.provider_id,
            provider_key: value.provider_key,
            provider_name: value.provider_name,
            runtime_level: value.runtime_level,
            request_count: value.request_count,
            error_count: value.error_count,
            success_rate: value.success_rate,
            avg_total_latency_ms: value.avg_total_latency_ms,
            last_error_at: value.last_error_at,
            last_error_summary: value.last_error_summary,
        }
    }
}

impl From<DashboardCostProviderReadItem> for DashboardCostProviderItem {
    fn from(value: DashboardCostProviderReadItem) -> Self {
        Self {
            provider_id: value.provider_id,
            provider_key: value.provider_key,
            provider_name: value.provider_name,
            request_count: value.request_count,
            success_rate: value.success_rate,
            avg_total_latency_ms: value.avg_total_latency_ms,
            total_cost: value.total_cost,
        }
    }
}

impl From<DashboardTopProviderReadItem> for DashboardTopProviderItem {
    fn from(value: DashboardTopProviderReadItem) -> Self {
        Self {
            provider_id: value.provider_id,
            provider_key: value.provider_key,
            provider_name: value.provider_name,
            request_count: value.request_count,
            success_count: value.success_count,
            error_count: value.error_count,
            success_rate: value.success_rate,
            total_cost: value.total_cost,
            avg_time_to_first_response_body_ms: value.avg_time_to_first_response_body_ms,
            time_to_first_response_body_sample_count: value
                .time_to_first_response_body_sample_count,
            avg_ttft_ms: value.avg_ttft_ms,
            ttft_sample_count: value.ttft_sample_count,
            avg_total_latency_ms: value.avg_total_latency_ms,
            total_latency_sample_count: value.total_latency_sample_count,
        }
    }
}

impl From<DashboardOperationalSignalsReadModel> for DashboardOperationalSignals {
    fn from(value: DashboardOperationalSignalsReadModel) -> Self {
        Self {
            degraded_providers: value
                .degraded_providers
                .into_iter()
                .map(Into::into)
                .collect(),
            top_error_providers: value
                .top_error_providers
                .into_iter()
                .map(Into::into)
                .collect(),
            top_cost_providers: value
                .top_cost_providers
                .into_iter()
                .map(Into::into)
                .collect(),
            top_cost_models: Vec::new(),
        }
    }
}

fn usage_stat_item_from_query_item(item: UsageStatsQueryItem) -> UsageStatItem {
    UsageStatItem {
        provider_id: item.provider_id,
        model_id: item.model_id,
        api_key_id: item.api_key_id,
        provider_key: item.provider_key,
        model_name: item.model_name,
        real_model_name: item.real_model_name,
        api_key_name: item.api_key_name,
        group_key: item.group_id.to_string(),
        group_label: item.group_label,
        group_detail: item.group_detail,
        total_input_tokens: item.total_input_tokens,
        total_output_tokens: item.total_output_tokens,
        total_reasoning_tokens: item.total_reasoning_tokens,
        total_tokens: item.total_tokens,
        request_count: item.request_count,
        success_count: item.success_count,
        error_count: item.error_count,
        success_rate: item.success_rate,
        avg_time_to_first_response_body_ms: item.avg_time_to_first_response_body_ms,
        time_to_first_response_body_sample_count: item.time_to_first_response_body_sample_count,
        avg_ttft_ms: item.avg_ttft_ms,
        ttft_sample_count: item.ttft_sample_count,
        avg_total_latency_ms: item.avg_total_latency_ms,
        total_latency_sample_count: item.total_latency_sample_count,
        total_cost: item.total_cost,
        is_other: false,
    }
}

fn metric_rank_value(item: &UsageStatItem, metric: UsageMetric) -> f64 {
    match metric {
        UsageMetric::TotalInputTokens => item.total_input_tokens as f64,
        UsageMetric::TotalOutputTokens => item.total_output_tokens as f64,
        UsageMetric::TotalReasoningTokens => item.total_reasoning_tokens as f64,
        UsageMetric::TotalTokens => item.total_tokens as f64,
        UsageMetric::RequestCount => item.request_count as f64,
        UsageMetric::TotalCost => item.total_cost.values().sum::<i64>() as f64,
        UsageMetric::SuccessRate => item.success_rate.unwrap_or(0.0),
        UsageMetric::AvgLatency => item.avg_total_latency_ms.unwrap_or(0.0),
        UsageMetric::AvgTimeToFirstResponseBody => {
            item.avg_time_to_first_response_body_ms.unwrap_or(0.0)
        }
        UsageMetric::AvgTtft => item.avg_ttft_ms.unwrap_or(0.0),
        UsageMetric::ErrorCount => item.error_count as f64,
    }
}

fn merge_usage_stat_item(target: &mut UsageStatItem, item: &UsageStatItem) {
    target.total_input_tokens += item.total_input_tokens;
    target.total_output_tokens += item.total_output_tokens;
    target.total_reasoning_tokens += item.total_reasoning_tokens;
    target.total_tokens += item.total_tokens;
    target.request_count += item.request_count;
    target.success_count += item.success_count;
    target.error_count += item.error_count;
    let previous_body_count = target.time_to_first_response_body_sample_count;
    let previous_body_sum =
        target.avg_time_to_first_response_body_ms.unwrap_or(0.0) * previous_body_count as f64;
    target.time_to_first_response_body_sample_count +=
        item.time_to_first_response_body_sample_count;
    target.avg_time_to_first_response_body_ms =
        if target.time_to_first_response_body_sample_count > 0 {
            Some(
                (previous_body_sum
                    + item.avg_time_to_first_response_body_ms.unwrap_or(0.0)
                        * item.time_to_first_response_body_sample_count as f64)
                    / target.time_to_first_response_body_sample_count as f64,
            )
        } else {
            None
        };

    let previous_ttft_count = target.ttft_sample_count;
    let previous_ttft_sum = target.avg_ttft_ms.unwrap_or(0.0) * previous_ttft_count as f64;
    target.ttft_sample_count += item.ttft_sample_count;
    target.avg_ttft_ms = if target.ttft_sample_count > 0 {
        Some(
            (previous_ttft_sum + item.avg_ttft_ms.unwrap_or(0.0) * item.ttft_sample_count as f64)
                / target.ttft_sample_count as f64,
        )
    } else {
        None
    };

    let previous_total_count = target.total_latency_sample_count;
    let previous_total_sum =
        target.avg_total_latency_ms.unwrap_or(0.0) * previous_total_count as f64;
    target.total_latency_sample_count += item.total_latency_sample_count;
    target.avg_total_latency_ms = if target.total_latency_sample_count > 0 {
        let weighted_total_sum = previous_total_sum
            + item.avg_total_latency_ms.unwrap_or(0.0) * item.total_latency_sample_count as f64;
        Some(weighted_total_sum / target.total_latency_sample_count as f64)
    } else {
        None
    };
    target.success_rate = if target.request_count > 0 {
        Some(target.success_count as f64 / target.request_count as f64)
    } else {
        None
    };

    for (currency, amount) in &item.total_cost {
        *target.total_cost.entry(currency.clone()).or_insert(0) += amount;
    }
}

fn top_group_keys(items: &[UsageStatItem], metric: UsageMetric, top_n: usize) -> Vec<String> {
    #[derive(Default)]
    struct GroupMetricAccumulator {
        score_sum: f64,
        success_count: i64,
        request_count: i64,
        total_latency_sample_count: i64,
        latency_weighted_sum: f64,
        time_to_first_response_body_sample_count: i64,
        time_to_first_response_body_weighted_sum: f64,
        ttft_sample_count: i64,
        ttft_weighted_sum: f64,
    }

    let mut scores = HashMap::<String, GroupMetricAccumulator>::new();
    for item in items {
        let entry = scores.entry(item.group_key.clone()).or_default();
        entry.score_sum += metric_rank_value(item, metric);
        entry.success_count += item.success_count;
        entry.request_count += item.request_count;
        entry.total_latency_sample_count += item.total_latency_sample_count;
        entry.latency_weighted_sum +=
            item.avg_total_latency_ms.unwrap_or(0.0) * item.total_latency_sample_count as f64;
        entry.time_to_first_response_body_sample_count +=
            item.time_to_first_response_body_sample_count;
        entry.time_to_first_response_body_weighted_sum +=
            item.avg_time_to_first_response_body_ms.unwrap_or(0.0)
                * item.time_to_first_response_body_sample_count as f64;
        entry.ttft_sample_count += item.ttft_sample_count;
        entry.ttft_weighted_sum += item.avg_ttft_ms.unwrap_or(0.0) * item.ttft_sample_count as f64;
    }

    let mut entries = scores.into_iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        let left_score = match metric {
            UsageMetric::SuccessRate => {
                if left.1.request_count > 0 {
                    left.1.success_count as f64 / left.1.request_count as f64
                } else {
                    0.0
                }
            }
            UsageMetric::AvgLatency => {
                if left.1.total_latency_sample_count > 0 {
                    left.1.latency_weighted_sum / left.1.total_latency_sample_count as f64
                } else {
                    0.0
                }
            }
            UsageMetric::AvgTimeToFirstResponseBody => {
                if left.1.time_to_first_response_body_sample_count > 0 {
                    left.1.time_to_first_response_body_weighted_sum
                        / left.1.time_to_first_response_body_sample_count as f64
                } else {
                    0.0
                }
            }
            UsageMetric::AvgTtft => {
                if left.1.ttft_sample_count > 0 {
                    left.1.ttft_weighted_sum / left.1.ttft_sample_count as f64
                } else {
                    0.0
                }
            }
            _ => left.1.score_sum,
        };
        let right_score = match metric {
            UsageMetric::SuccessRate => {
                if right.1.request_count > 0 {
                    right.1.success_count as f64 / right.1.request_count as f64
                } else {
                    0.0
                }
            }
            UsageMetric::AvgLatency => {
                if right.1.total_latency_sample_count > 0 {
                    right.1.latency_weighted_sum / right.1.total_latency_sample_count as f64
                } else {
                    0.0
                }
            }
            UsageMetric::AvgTimeToFirstResponseBody => {
                if right.1.time_to_first_response_body_sample_count > 0 {
                    right.1.time_to_first_response_body_weighted_sum
                        / right.1.time_to_first_response_body_sample_count as f64
                } else {
                    0.0
                }
            }
            UsageMetric::AvgTtft => {
                if right.1.ttft_sample_count > 0 {
                    right.1.ttft_weighted_sum / right.1.ttft_sample_count as f64
                } else {
                    0.0
                }
            }
            _ => right.1.score_sum,
        };
        right_score
            .partial_cmp(&left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    entries
        .into_iter()
        .take(top_n)
        .map(|(key, _)| key)
        .collect()
}

async fn system_overview_stats(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<SystemOverviewResponse>, BaseError> {
    let stats = get_system_overview_stats(&app_state.database).await?;
    let runtime_state_backend = app_state.runtime_state_backend_operator_status().await;
    Ok(HttpResult::new(SystemOverviewResponse {
        stats,
        runtime_state_backend,
    }))
}

async fn today_request_log_stats(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<TodayRequestLogStats>, BaseError> {
    let timezone = configured_timezone(&app_state);
    let stats = get_today_request_log_stats(&app_state.database, timezone.as_deref()).await?;
    Ok(HttpResult::new(stats))
}

fn configured_timezone(app_state: &Arc<AppState>) -> Option<String> {
    app_state.timezone.clone()
}

#[cfg(test)]
fn runtime_summary_from_items(items: &[ProviderRuntimeItem]) -> DashboardRuntimeSummary {
    let provider_ids = items
        .iter()
        .map(|item| item.provider_id)
        .collect::<HashSet<_>>();
    let mut summary = DashboardRuntimeSummary {
        window: ProviderRuntimeWindow::OneHour,
        total_provider_count: provider_ids.len() as i64,
        enabled_provider_count: items
            .iter()
            .filter(|item| item.provider_is_enabled)
            .map(|item| item.provider_id)
            .collect::<HashSet<_>>()
            .len() as i64,
        total_source_count: items.len() as i64,
        enabled_source_count: items.iter().filter(|item| item.source_is_enabled).count() as i64,
        healthy_count: 0,
        degraded_count: 0,
        no_traffic_count: 0,
    };

    for item in items {
        match item.runtime_level {
            ProviderRuntimeLevel::Healthy => summary.healthy_count += 1,
            ProviderRuntimeLevel::Degraded => summary.degraded_count += 1,
            ProviderRuntimeLevel::NoTraffic => summary.no_traffic_count += 1,
        }
    }

    summary
}

fn cost_model_item_from_top_model_item(item: DashboardTopModelItem) -> DashboardCostModelItem {
    DashboardCostModelItem {
        provider_id: item.provider_id,
        provider_key: item.provider_key,
        model_id: item.model_id,
        model_name: item.model_name,
        real_model_name: item.real_model_name,
        request_count: item.request_count,
        total_tokens: item.total_tokens,
        total_cost: item.total_cost,
    }
}

async fn build_dashboard_operations_section(
    app_state: &Arc<AppState>,
    runtime_items: &[ProviderRuntimeItem],
    timezone: Option<&str>,
) -> Result<DashboardOperationsSection, BaseError> {
    let mut operational_signals =
        DashboardOperationalSignals::from(operational_signals_from_runtime_items(runtime_items));
    operational_signals.top_cost_models = app_state
        .metrics
        .dashboard_top_cost_models(5, timezone)
        .await?
        .into_iter()
        .map(cost_model_item_from_top_model_item)
        .collect();

    Ok(DashboardOperationsSection {
        operational_signals,
        top_providers: top_providers_from_runtime_items(runtime_items)
            .into_iter()
            .map(Into::into)
            .collect(),
        top_models: app_state.metrics.dashboard_top_models(5, timezone).await?,
    })
}

async fn system_dashboard(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<DashboardResponse>, BaseError> {
    let timezone = configured_timezone(&app_state);
    let timezone = timezone.as_deref();
    let resources = app_state
        .metrics
        .build_dashboard_resources(&app_state, timezone)
        .await?;
    let operations_section =
        build_dashboard_operations_section(&app_state, &resources.runtime_items, timezone).await?;
    let runtime_state_backend = resources.runtime.runtime_state_backend.clone();

    Ok(HttpResult::new(DashboardResponse {
        overview: DashboardOverviewStats::from(resources.overview),
        today: DashboardTodayStats::from(resources.today),
        runtime: DashboardRuntimeSummary::from(resources.runtime),
        runtime_state_backend,
        operational_signals: operations_section.operational_signals,
        top_providers: operations_section.top_providers,
        top_models: operations_section.top_models,
    }))
}

async fn system_usage_stats(
    State(app_state): State<Arc<AppState>>,
    Query(params): Query<UsageStatsParams>,
) -> Result<HttpResult<Vec<UsageStatsPeriod>>, BaseError> {
    let interval = params.interval;

    let time_range_ms = params.end_time - params.start_time;
    match interval {
        Interval::Minute => {
            if time_range_ms > 180 * 60 * 1000 {
                return Err(BaseError::ParamInvalid(Some(
                    "For minute interval, the time range cannot exceed 180 minutes.".to_string(),
                )));
            }
        }
        Interval::Hour => {
            if time_range_ms > 168 * 60 * 60 * 1000 {
                return Err(BaseError::ParamInvalid(Some(
                    "For hour interval, the time range cannot exceed 168 hours.".to_string(),
                )));
            }
        }
        Interval::Day => {
            let one_eighty_days_in_ms: i64 = 180 * 24 * 60 * 60 * 1000;
            if time_range_ms > one_eighty_days_in_ms {
                return Err(BaseError::ParamInvalid(Some(
                    "For day interval, the time range cannot exceed 180 days.".to_string(),
                )));
            }
        }
        Interval::Month => {}
    }

    if params.start_time >= params.end_time {
        return Err(BaseError::ParamInvalid(Some(
            "startTime must be before endTime".to_string(),
        )));
    }

    if params.top_n == 0 || params.top_n > 20 {
        return Err(BaseError::ParamInvalid(Some(
            "top_n must be between 1 and 20.".to_string(),
        )));
    }

    let usage_rows = app_state
        .metrics
        .usage_stats_aggregates(
            params.start_time,
            params.end_time,
            interval.as_str(),
            params.group_by.to_database_group_by(),
            params.provider_id,
            params.model_id,
            params.api_key_id,
            params.provider_api_key_id,
        )
        .await?;

    let usage_items = usage_rows
        .iter()
        .map(|item| {
            usage_stat_item_from_query_item(UsageStatsQueryItem {
                time: item.time,
                group_id: item.group_id,
                provider_id: item.provider_id,
                model_id: item.model_id,
                api_key_id: item.api_key_id,
                provider_key: item.provider_key.clone(),
                model_name: item.model_name.clone(),
                real_model_name: item.real_model_name.clone(),
                api_key_name: item.api_key_name.clone(),
                group_label: item.group_label.clone(),
                group_detail: item.group_detail.clone(),
                total_input_tokens: item.total_input_tokens,
                total_output_tokens: item.total_output_tokens,
                total_reasoning_tokens: item.total_reasoning_tokens,
                total_tokens: item.total_tokens,
                request_count: item.request_count,
                success_count: item.success_count,
                error_count: item.error_count,
                success_rate: item.success_rate,
                avg_time_to_first_response_body_ms: item.avg_time_to_first_response_body_ms,
                time_to_first_response_body_sample_count: item
                    .time_to_first_response_body_sample_count,
                avg_ttft_ms: item.avg_ttft_ms,
                ttft_sample_count: item.ttft_sample_count,
                avg_total_latency_ms: item.avg_total_latency_ms,
                total_latency_sample_count: item.total_latency_sample_count,
                total_cost: item.total_cost.clone(),
            })
        })
        .collect::<Vec<_>>();

    let top_group_key_set = top_group_keys(&usage_items, params.metric, params.top_n)
        .into_iter()
        .collect::<std::collections::HashSet<_>>();

    let mut periods = HashMap::<i64, Vec<UsageStatItem>>::new();
    let mut others_by_time = HashMap::<i64, UsageStatItem>::new();

    for row in usage_rows {
        let time_bucket = row.time;
        let item = usage_stat_item_from_query_item(row);
        if top_group_key_set.contains(&item.group_key) {
            periods.entry(time_bucket).or_default().push(item);
            continue;
        }

        if params.include_others {
            let entry = others_by_time
                .entry(time_bucket)
                .or_insert_with(|| UsageStatItem {
                    group_key: "__others__".to_string(),
                    group_label: "Others".to_string(),
                    is_other: true,
                    ..Default::default()
                });
            merge_usage_stat_item(entry, &item);
        }
    }

    for (time_bucket, other_item) in others_by_time {
        periods.entry(time_bucket).or_default().push(other_item);
    }

    let mut result = periods
        .into_iter()
        .map(|(time_bucket, mut data)| {
            data.sort_by(|left, right| left.group_label.cmp(&right.group_label));
            UsageStatsPeriod {
                time: time_bucket,
                data,
            }
        })
        .collect::<Vec<_>>();

    result.sort_by_key(|period| period.time);

    Ok(HttpResult::new(result))
}

pub fn routes() -> StateRouter {
    create_state_router()
        .route("/system/dashboard", get(system_dashboard))
        .route("/system/overview", get(system_overview_stats))
        .route("/system/today_log_stats", get(today_request_log_stats))
        .route("/system/usage_stats", get(system_usage_stats))
}

#[cfg(test)]
mod tests {
    use super::{
        DashboardOverviewStats, DashboardRuntimeSummary, DbDashboardOverviewStats, UsageGroupBy,
        UsageMetric, UsageStatItem, runtime_summary_from_items, top_group_keys,
    };
    use crate::schema::enum_def::UpstreamProfileType;
    use crate::service::metrics::provider_runtime::{
        ProviderRuntimeCostStat, ProviderRuntimeItem, ProviderRuntimeLevel,
        ProviderRuntimeStatusCodeStat, ProviderRuntimeWindow, first_runtime_backend_read_error,
    };
    use crate::service::metrics::runtime_overview::{
        operational_signals_from_runtime_items, top_providers_from_runtime_items,
    };
    use serde_json::to_value;
    use std::collections::HashMap;

    fn sample_runtime_item(
        provider_id: i64,
        runtime_level: ProviderRuntimeLevel,
        request_count: i64,
        error_count: i64,
    ) -> ProviderRuntimeItem {
        ProviderRuntimeItem {
            provider_id,
            provider_key: format!("p{}", provider_id),
            provider_name: format!("Provider {}", provider_id),
            provider_is_enabled: true,
            source_id: provider_id * 10 + 1,
            source_profile_type: UpstreamProfileType::Openai,
            source_base_url: "https://api.example.com/v1".to_string(),
            source_use_proxy: false,
            source_is_enabled: true,
            source_is_default: true,
            enabled_model_count: 1,
            enabled_provider_key_count: 1,
            runtime_level,
            runtime_state_backend_degraded: false,
            runtime_state_backend_error: None,
            request_count,
            success_count: request_count.saturating_sub(error_count),
            error_count,
            success_rate: if request_count > 0 {
                Some((request_count - error_count) as f64 / request_count as f64)
            } else {
                None
            },
            avg_time_to_first_response_body_ms: Some(100.0),
            time_to_first_response_body_sample_count: 1,
            avg_ttft_ms: Some(80.0),
            ttft_sample_count: 1,
            avg_total_latency_ms: Some(300.0),
            total_latency_sample_count: 1,
            last_request_at: None,
            last_success_at: None,
            last_error_at: None,
            last_error_summary: None,
            status_code_breakdown: Vec::<ProviderRuntimeStatusCodeStat>::new(),
            total_cost: vec![ProviderRuntimeCostStat {
                currency: "USD".to_string(),
                amount_nanos: provider_id * 10,
            }],
        }
    }

    fn sample_usage_item(
        group_key: &str,
        request_count: i64,
        success_count: i64,
        avg_total_latency_ms: Option<f64>,
        total_latency_sample_count: i64,
    ) -> UsageStatItem {
        UsageStatItem {
            group_key: group_key.to_string(),
            request_count,
            success_count,
            success_rate: if request_count > 0 {
                Some(success_count as f64 / request_count as f64)
            } else {
                None
            },
            avg_total_latency_ms,
            total_latency_sample_count,
            total_cost: HashMap::new(),
            ..Default::default()
        }
    }

    #[test]
    fn dashboard_runtime_summary_counts_each_level() {
        let items = vec![
            sample_runtime_item(1, ProviderRuntimeLevel::Healthy, 10, 0),
            sample_runtime_item(2, ProviderRuntimeLevel::Degraded, 10, 3),
            sample_runtime_item(3, ProviderRuntimeLevel::Degraded, 1, 1),
            sample_runtime_item(4, ProviderRuntimeLevel::Healthy, 1, 0),
            sample_runtime_item(5, ProviderRuntimeLevel::NoTraffic, 0, 0),
        ];

        let summary: DashboardRuntimeSummary = runtime_summary_from_items(&items);

        assert_eq!(summary.window, ProviderRuntimeWindow::OneHour);
        assert_eq!(summary.healthy_count, 2);
        assert_eq!(summary.degraded_count, 2);
        assert_eq!(summary.no_traffic_count, 1);
    }

    #[test]
    fn dashboard_top_providers_are_sorted_by_request_count() {
        let items = vec![
            sample_runtime_item(1, ProviderRuntimeLevel::Healthy, 10, 0),
            sample_runtime_item(2, ProviderRuntimeLevel::Healthy, 30, 1),
            sample_runtime_item(3, ProviderRuntimeLevel::Healthy, 20, 2),
        ];

        let top = top_providers_from_runtime_items(&items);

        let ids = top.iter().map(|item| item.provider_id).collect::<Vec<_>>();
        assert_eq!(ids, vec![2, 3, 1]);
    }

    #[test]
    fn dashboard_operational_signals_include_degraded_and_cost_hotspots() {
        let mut expensive = sample_runtime_item(1, ProviderRuntimeLevel::Degraded, 20, 5);
        expensive.total_cost[0].amount_nanos = 500;

        let mut recovering = sample_runtime_item(2, ProviderRuntimeLevel::Degraded, 8, 2);
        recovering.total_cost[0].amount_nanos = 200;

        let mut steady = sample_runtime_item(3, ProviderRuntimeLevel::Healthy, 30, 1);
        steady.total_cost[0].amount_nanos = 900;

        let signals = operational_signals_from_runtime_items(&[expensive, recovering, steady]);

        assert_eq!(
            signals
                .degraded_providers
                .iter()
                .map(|item| item.provider_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            signals
                .top_cost_providers
                .iter()
                .map(|item| item.provider_id)
                .collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
    }

    #[test]
    fn dashboard_runtime_backend_read_error_uses_item_error() {
        let mut healthy = sample_runtime_item(1, ProviderRuntimeLevel::Healthy, 10, 0);
        let mut degraded = sample_runtime_item(2, ProviderRuntimeLevel::Degraded, 0, 0);
        degraded.runtime_state_backend_degraded = true;
        degraded.runtime_state_backend_error = Some("redis snapshot failed".to_string());

        assert_eq!(
            first_runtime_backend_read_error(&[healthy.clone(), degraded]).as_deref(),
            Some("redis snapshot failed")
        );

        healthy.runtime_state_backend_error = None;
        assert!(first_runtime_backend_read_error(&[healthy]).is_none());
    }

    #[test]
    fn top_group_keys_ranks_success_rate_by_actual_ratio() {
        let keys = top_group_keys(
            &[
                sample_usage_item("high-volume-mid-success", 100, 80, None, 0),
                sample_usage_item("low-volume-high-success", 10, 10, None, 0),
            ],
            UsageMetric::SuccessRate,
            1,
        );

        assert_eq!(keys, vec!["low-volume-high-success".to_string()]);
    }

    #[test]
    fn top_group_keys_ranks_avg_latency_by_weighted_latency() {
        let keys = top_group_keys(
            &[
                sample_usage_item("slow", 20, 20, Some(800.0), 20),
                sample_usage_item("fast", 200, 200, Some(120.0), 200),
            ],
            UsageMetric::AvgLatency,
            1,
        );

        assert_eq!(keys, vec!["slow".to_string()]);
    }

    #[test]
    fn top_group_keys_ranks_response_body_and_ttft_by_sample_weight() {
        let mut slow_body = sample_usage_item("slow-body", 10, 10, None, 0);
        slow_body.avg_time_to_first_response_body_ms = Some(800.0);
        slow_body.time_to_first_response_body_sample_count = 10;

        let mut fast_body = sample_usage_item("fast-body", 100, 100, None, 0);
        fast_body.avg_time_to_first_response_body_ms = Some(120.0);
        fast_body.time_to_first_response_body_sample_count = 100;

        let mut slow_ttft = sample_usage_item("slow-ttft", 10, 10, None, 0);
        slow_ttft.avg_ttft_ms = Some(700.0);
        slow_ttft.ttft_sample_count = 10;

        let mut fast_ttft = sample_usage_item("fast-ttft", 100, 100, None, 0);
        fast_ttft.avg_ttft_ms = Some(90.0);
        fast_ttft.ttft_sample_count = 100;

        assert_eq!(
            top_group_keys(
                &[slow_body, fast_body],
                UsageMetric::AvgTimeToFirstResponseBody,
                1,
            ),
            vec!["slow-body".to_string()]
        );
        assert_eq!(
            top_group_keys(&[slow_ttft, fast_ttft], UsageMetric::AvgTtft, 1),
            vec!["slow-ttft".to_string()]
        );
    }

    #[test]
    fn usage_group_by_serializes_api_key() {
        let value = to_value(UsageGroupBy::ApiKey).expect("enum should serialize");
        assert_eq!(value.as_str(), Some("api_key"));
    }

    #[test]
    fn usage_latency_metrics_serialize_for_query_contract() {
        assert_eq!(
            to_value(UsageMetric::AvgTimeToFirstResponseBody)
                .expect("body metric should serialize")
                .as_str(),
            Some("avg_time_to_first_response_body")
        );
        assert_eq!(
            to_value(UsageMetric::AvgTtft)
                .expect("ttft metric should serialize")
                .as_str(),
            Some("avg_ttft")
        );
    }

    #[test]
    fn usage_stat_item_serializes_api_key_fields() {
        let value = to_value(UsageStatItem {
            api_key_id: Some(7),
            api_key_name: Some("gateway-key".to_string()),
            ..Default::default()
        })
        .expect("usage stat item should serialize");

        assert_eq!(value.get("api_key_id").and_then(|v| v.as_i64()), Some(7));
        assert_eq!(
            value.get("api_key_name").and_then(|v| v.as_str()),
            Some("gateway-key")
        );
        let legacy_api_key_id_field = ["system", "api", "key", "id"].join("_");
        let legacy_api_key_name_field = ["system", "api", "key", "name"].join("_");
        assert!(value.get(&legacy_api_key_id_field).is_none());
        assert!(value.get(&legacy_api_key_name_field).is_none());
    }

    #[test]
    fn dashboard_overview_stats_serializes_api_key_counts() {
        let value = to_value(DashboardOverviewStats::from(DbDashboardOverviewStats {
            provider_count: 1,
            enabled_provider_count: 1,
            model_count: 2,
            enabled_model_count: 2,
            provider_key_count: 3,
            enabled_provider_key_count: 2,
            api_key_count: 4,
            enabled_api_key_count: 3,
        }))
        .expect("dashboard overview stats should serialize");

        assert_eq!(value.get("api_key_count").and_then(|v| v.as_i64()), Some(4));
        assert_eq!(
            value.get("enabled_api_key_count").and_then(|v| v.as_i64()),
            Some(3)
        );
        let legacy_api_key_count_field = ["system", "api", "key", "count"].join("_");
        let legacy_enabled_api_key_count_field =
            ["enabled", "system", "api", "key", "count"].join("_");
        assert!(value.get(&legacy_api_key_count_field).is_none());
        assert!(value.get(&legacy_enabled_api_key_count_field).is_none());
    }
}
