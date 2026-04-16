use crate::controller::provider_runtime::{
    ProviderRuntimeItem, ProviderRuntimeLevel, ProviderRuntimeWindow, build_provider_runtime_items,
};
use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::{
    controller::error::BaseError,
    database::stat::{
        DashboardOverviewStats, DashboardTodayStats, DashboardTopModelItem, SystemOverviewStats,
        TodayRequestLogStats, UsageStatsGroupBy, UsageStatsQueryItem,
        get_dashboard_cost_alert_models, get_dashboard_overview_stats, get_dashboard_today_stats,
        get_dashboard_top_models, get_system_overview_stats, get_today_request_log_stats,
        get_usage_stats_aggregates,
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
use std::sync::Arc;

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
    system_api_key_id: Option<i64>,
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
    SystemApiKey,
}

impl UsageGroupBy {
    fn to_database_group_by(self) -> UsageStatsGroupBy {
        match self {
            UsageGroupBy::Provider => UsageStatsGroupBy::Provider,
            UsageGroupBy::Model => UsageStatsGroupBy::Model,
            UsageGroupBy::SystemApiKey => UsageStatsGroupBy::SystemApiKey,
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
    ErrorCount,
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct UsageStatItem {
    provider_id: Option<i64>,
    model_id: Option<i64>,
    system_api_key_id: Option<i64>,
    provider_key: Option<String>,
    model_name: Option<String>,
    real_model_name: Option<String>,
    system_api_key_name: Option<String>,
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
    avg_total_latency_ms: Option<f64>,
    latency_sample_count: i64,
    total_cost: HashMap<String, i64>,
    is_other: bool,
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
    alerts: DashboardAlerts,
    top_providers: Vec<DashboardTopProviderItem>,
    top_models: Vec<DashboardTopModelItem>,
}

#[derive(Serialize, Debug)]
pub struct DashboardKpiSection {
    today: DashboardTodayStats,
    runtime: DashboardRuntimeSummary,
}

#[derive(Serialize, Debug)]
pub struct DashboardResourcesSection {
    overview: DashboardOverviewStats,
    today: DashboardTodayStats,
    runtime: DashboardRuntimeSummary,
}

#[derive(Serialize, Debug)]
pub struct DashboardAlertsSection {
    alerts: DashboardAlerts,
    top_providers: Vec<DashboardTopProviderItem>,
    top_models: Vec<DashboardTopModelItem>,
}

#[derive(Serialize, Debug)]
pub struct DashboardRuntimeSummary {
    window: ProviderRuntimeWindow,
    healthy_count: i64,
    degraded_count: i64,
    half_open_count: i64,
    open_count: i64,
    no_traffic_count: i64,
}

#[derive(Serialize, Debug, Default)]
pub struct DashboardAlerts {
    open_providers: Vec<DashboardProviderAlertItem>,
    half_open_providers: Vec<DashboardProviderAlertItem>,
    degraded_providers: Vec<DashboardProviderAlertItem>,
    top_error_providers: Vec<DashboardProviderAlertItem>,
    top_cost_providers: Vec<DashboardCostProviderAlertItem>,
    top_cost_models: Vec<DashboardCostModelAlertItem>,
}

#[derive(Serialize, Debug)]
pub struct DashboardProviderAlertItem {
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
pub struct DashboardCostProviderAlertItem {
    provider_id: i64,
    provider_key: String,
    provider_name: String,
    request_count: i64,
    success_rate: Option<f64>,
    avg_total_latency_ms: Option<f64>,
    total_cost: HashMap<String, i64>,
}

#[derive(Serialize, Debug)]
pub struct DashboardCostModelAlertItem {
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
    avg_total_latency_ms: Option<f64>,
}

fn usage_stat_item_from_query_item(item: UsageStatsQueryItem) -> UsageStatItem {
    UsageStatItem {
        provider_id: item.provider_id,
        model_id: item.model_id,
        system_api_key_id: item.system_api_key_id,
        provider_key: item.provider_key,
        model_name: item.model_name,
        real_model_name: item.real_model_name,
        system_api_key_name: item.system_api_key_name,
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
        avg_total_latency_ms: item.avg_total_latency_ms,
        latency_sample_count: item.latency_sample_count,
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
    target.latency_sample_count += item.latency_sample_count;

    let weighted_latency_sum = target.avg_total_latency_ms.unwrap_or(0.0)
        * (target.latency_sample_count - item.latency_sample_count) as f64
        + item.avg_total_latency_ms.unwrap_or(0.0) * item.latency_sample_count as f64;
    target.avg_total_latency_ms = if target.latency_sample_count > 0 {
        Some(weighted_latency_sum / target.latency_sample_count as f64)
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
        latency_sample_count: i64,
        latency_weighted_sum: f64,
    }

    let mut scores = HashMap::<String, GroupMetricAccumulator>::new();
    for item in items {
        let entry = scores.entry(item.group_key.clone()).or_default();
        entry.score_sum += metric_rank_value(item, metric);
        entry.success_count += item.success_count;
        entry.request_count += item.request_count;
        entry.latency_sample_count += item.latency_sample_count;
        entry.latency_weighted_sum +=
            item.avg_total_latency_ms.unwrap_or(0.0) * item.latency_sample_count as f64;
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
                if left.1.latency_sample_count > 0 {
                    left.1.latency_weighted_sum / left.1.latency_sample_count as f64
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
                if right.1.latency_sample_count > 0 {
                    right.1.latency_weighted_sum / right.1.latency_sample_count as f64
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

async fn system_overview_stats() -> Result<HttpResult<SystemOverviewStats>, BaseError> {
    let stats = get_system_overview_stats()?;
    Ok(HttpResult::new(stats))
}

async fn today_request_log_stats() -> Result<HttpResult<TodayRequestLogStats>, BaseError> {
    let stats = get_today_request_log_stats()?;
    Ok(HttpResult::new(stats))
}

fn runtime_summary_from_items(items: &[ProviderRuntimeItem]) -> DashboardRuntimeSummary {
    let mut summary = DashboardRuntimeSummary {
        window: ProviderRuntimeWindow::OneHour,
        healthy_count: 0,
        degraded_count: 0,
        half_open_count: 0,
        open_count: 0,
        no_traffic_count: 0,
    };

    for item in items {
        match item.runtime_level {
            ProviderRuntimeLevel::Healthy => summary.healthy_count += 1,
            ProviderRuntimeLevel::Degraded => summary.degraded_count += 1,
            ProviderRuntimeLevel::HalfOpen => summary.half_open_count += 1,
            ProviderRuntimeLevel::Open => summary.open_count += 1,
            ProviderRuntimeLevel::NoTraffic => summary.no_traffic_count += 1,
        }
    }

    summary
}

fn alert_item_from_runtime_item(item: &ProviderRuntimeItem) -> DashboardProviderAlertItem {
    DashboardProviderAlertItem {
        provider_id: item.provider_id,
        provider_key: item.provider_key.clone(),
        provider_name: item.provider_name.clone(),
        runtime_level: item.runtime_level,
        request_count: item.request_count,
        error_count: item.error_count,
        success_rate: item.success_rate,
        avg_total_latency_ms: item.avg_total_latency_ms,
        last_error_at: item.last_error_at,
        last_error_summary: item.last_error_summary.clone(),
    }
}

fn top_provider_item_from_runtime_item(item: &ProviderRuntimeItem) -> DashboardTopProviderItem {
    DashboardTopProviderItem {
        provider_id: item.provider_id,
        provider_key: item.provider_key.clone(),
        provider_name: item.provider_name.clone(),
        request_count: item.request_count,
        success_count: item.success_count,
        error_count: item.error_count,
        success_rate: item.success_rate,
        total_cost: item
            .total_cost
            .iter()
            .map(|cost| (cost.currency.clone(), cost.amount_nanos))
            .collect(),
        avg_total_latency_ms: item.avg_total_latency_ms,
    }
}

fn total_cost_rank_value(cost: &HashMap<String, i64>) -> i64 {
    cost.values().copied().sum()
}

fn cost_provider_alert_item_from_runtime_item(
    item: &ProviderRuntimeItem,
) -> DashboardCostProviderAlertItem {
    DashboardCostProviderAlertItem {
        provider_id: item.provider_id,
        provider_key: item.provider_key.clone(),
        provider_name: item.provider_name.clone(),
        request_count: item.request_count,
        success_rate: item.success_rate,
        avg_total_latency_ms: item.avg_total_latency_ms,
        total_cost: item
            .total_cost
            .iter()
            .map(|cost| (cost.currency.clone(), cost.amount_nanos))
            .collect(),
    }
}

fn cost_model_alert_item_from_top_model_item(
    item: DashboardTopModelItem,
) -> DashboardCostModelAlertItem {
    DashboardCostModelAlertItem {
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

fn dashboard_alerts_from_runtime_items(items: &[ProviderRuntimeItem]) -> DashboardAlerts {
    let mut open_providers = items
        .iter()
        .filter(|item| item.runtime_level == ProviderRuntimeLevel::Open)
        .map(alert_item_from_runtime_item)
        .collect::<Vec<_>>();
    open_providers.sort_by(|left, right| {
        right
            .error_count
            .cmp(&left.error_count)
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });

    let mut half_open_providers = items
        .iter()
        .filter(|item| item.runtime_level == ProviderRuntimeLevel::HalfOpen)
        .map(alert_item_from_runtime_item)
        .collect::<Vec<_>>();
    half_open_providers.sort_by(|left, right| {
        right
            .error_count
            .cmp(&left.error_count)
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });

    let mut degraded_providers = items
        .iter()
        .filter(|item| item.runtime_level == ProviderRuntimeLevel::Degraded)
        .map(alert_item_from_runtime_item)
        .collect::<Vec<_>>();
    degraded_providers.sort_by(|left, right| {
        right
            .error_count
            .cmp(&left.error_count)
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });

    let mut top_error_providers = items
        .iter()
        .filter(|item| item.error_count > 0)
        .map(alert_item_from_runtime_item)
        .collect::<Vec<_>>();
    top_error_providers.sort_by(|left, right| {
        right
            .error_count
            .cmp(&left.error_count)
            .then_with(|| right.last_error_at.cmp(&left.last_error_at))
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });
    top_error_providers.truncate(5);

    let mut top_cost_providers = items
        .iter()
        .filter(|item| item.total_cost.iter().any(|cost| cost.amount_nanos > 0))
        .map(cost_provider_alert_item_from_runtime_item)
        .collect::<Vec<_>>();
    top_cost_providers.sort_by(|left, right| {
        total_cost_rank_value(&right.total_cost)
            .cmp(&total_cost_rank_value(&left.total_cost))
            .then_with(|| right.request_count.cmp(&left.request_count))
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });
    top_cost_providers.truncate(5);

    DashboardAlerts {
        open_providers,
        half_open_providers,
        degraded_providers,
        top_error_providers,
        top_cost_providers,
        top_cost_models: Vec::new(),
    }
}

fn top_providers_from_runtime_items(
    items: &[ProviderRuntimeItem],
) -> Vec<DashboardTopProviderItem> {
    let mut top_providers = items
        .iter()
        .map(top_provider_item_from_runtime_item)
        .collect::<Vec<_>>();
    top_providers.sort_by(|left, right| {
        right
            .request_count
            .cmp(&left.request_count)
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });
    top_providers.truncate(5);
    top_providers
}

async fn build_dashboard_runtime_items(
    app_state: &Arc<AppState>,
) -> Result<Vec<ProviderRuntimeItem>, BaseError> {
    build_provider_runtime_items(app_state, ProviderRuntimeWindow::OneHour, true).await
}

fn build_dashboard_alerts_section(
    runtime_items: &[ProviderRuntimeItem],
) -> Result<DashboardAlertsSection, BaseError> {
    let mut alerts = dashboard_alerts_from_runtime_items(runtime_items);
    alerts.top_cost_models = get_dashboard_cost_alert_models(5)?
        .into_iter()
        .map(cost_model_alert_item_from_top_model_item)
        .collect();

    Ok(DashboardAlertsSection {
        alerts,
        top_providers: top_providers_from_runtime_items(runtime_items),
        top_models: get_dashboard_top_models(5)?,
    })
}

async fn system_dashboard(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<DashboardResponse>, BaseError> {
    let overview = get_dashboard_overview_stats()?;
    let today = get_dashboard_today_stats()?;
    let runtime_items = build_dashboard_runtime_items(&app_state).await?;
    let runtime = runtime_summary_from_items(&runtime_items);
    let alerts_section = build_dashboard_alerts_section(&runtime_items)?;

    Ok(HttpResult::new(DashboardResponse {
        overview,
        today,
        runtime,
        alerts: alerts_section.alerts,
        top_providers: alerts_section.top_providers,
        top_models: alerts_section.top_models,
    }))
}

async fn system_dashboard_kpi(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<DashboardKpiSection>, BaseError> {
    let today = get_dashboard_today_stats()?;
    let runtime_items = build_dashboard_runtime_items(&app_state).await?;
    let runtime = runtime_summary_from_items(&runtime_items);

    Ok(HttpResult::new(DashboardKpiSection { today, runtime }))
}

async fn system_dashboard_resources(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<DashboardResourcesSection>, BaseError> {
    let overview = get_dashboard_overview_stats()?;
    let today = get_dashboard_today_stats()?;
    let runtime_items = build_dashboard_runtime_items(&app_state).await?;
    let runtime = runtime_summary_from_items(&runtime_items);

    Ok(HttpResult::new(DashboardResourcesSection {
        overview,
        today,
        runtime,
    }))
}

async fn system_dashboard_alerts(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<DashboardAlertsSection>, BaseError> {
    let runtime_items = build_dashboard_runtime_items(&app_state).await?;
    Ok(HttpResult::new(build_dashboard_alerts_section(
        &runtime_items,
    )?))
}

async fn system_usage_stats(
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

    let usage_rows = get_usage_stats_aggregates(
        params.start_time,
        params.end_time,
        interval.as_str(),
        params.group_by.to_database_group_by(),
        params.provider_id,
        params.model_id,
        params.system_api_key_id,
        params.provider_api_key_id,
    )?;

    let usage_items = usage_rows
        .iter()
        .map(|item| {
            usage_stat_item_from_query_item(UsageStatsQueryItem {
                time: item.time,
                group_id: item.group_id,
                provider_id: item.provider_id,
                model_id: item.model_id,
                system_api_key_id: item.system_api_key_id,
                provider_key: item.provider_key.clone(),
                model_name: item.model_name.clone(),
                real_model_name: item.real_model_name.clone(),
                system_api_key_name: item.system_api_key_name.clone(),
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
                avg_total_latency_ms: item.avg_total_latency_ms,
                latency_sample_count: item.latency_sample_count,
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
        .route("/system/dashboard/kpi", get(system_dashboard_kpi))
        .route(
            "/system/dashboard/resources",
            get(system_dashboard_resources),
        )
        .route("/system/dashboard/alerts", get(system_dashboard_alerts))
        .route("/system/overview", get(system_overview_stats))
        .route("/system/today_log_stats", get(today_request_log_stats))
        .route("/system/usage_stats", get(system_usage_stats))
}

#[cfg(test)]
mod tests {
    use super::{
        DashboardRuntimeSummary, UsageMetric, UsageStatItem, dashboard_alerts_from_runtime_items,
        runtime_summary_from_items, top_group_keys, top_providers_from_runtime_items,
    };
    use crate::controller::provider_runtime::{
        ProviderRuntimeCostStat, ProviderRuntimeHealthStatus, ProviderRuntimeItem,
        ProviderRuntimeLevel, ProviderRuntimeStatusCodeStat, ProviderRuntimeWindow,
    };
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
            provider_type: "OPENAI".to_string(),
            is_enabled: true,
            use_proxy: false,
            enabled_model_count: 1,
            enabled_provider_key_count: 1,
            health_status: ProviderRuntimeHealthStatus::Healthy,
            runtime_level,
            consecutive_failures: 0,
            half_open_probe_in_flight: false,
            opened_at: None,
            last_failure_at: None,
            last_recovered_at: None,
            last_error: None,
            request_count,
            success_count: request_count.saturating_sub(error_count),
            error_count,
            success_rate: if request_count > 0 {
                Some((request_count - error_count) as f64 / request_count as f64)
            } else {
                None
            },
            avg_first_byte_ms: Some(100.0),
            avg_total_latency_ms: Some(300.0),
            last_request_at: None,
            last_success_at: None,
            last_error_at: None,
            last_error_summary: None,
            status_code_breakdown: Vec::<ProviderRuntimeStatusCodeStat>::new(),
            total_cost: vec![ProviderRuntimeCostStat {
                currency: "USD".to_string(),
                amount_nanos: provider_id * 10,
            }],
            sort_score: 0.0,
        }
    }

    fn sample_usage_item(
        group_key: &str,
        request_count: i64,
        success_count: i64,
        avg_total_latency_ms: Option<f64>,
        latency_sample_count: i64,
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
            latency_sample_count,
            total_cost: HashMap::new(),
            ..Default::default()
        }
    }

    #[test]
    fn dashboard_runtime_summary_counts_each_level() {
        let items = vec![
            sample_runtime_item(1, ProviderRuntimeLevel::Healthy, 10, 0),
            sample_runtime_item(2, ProviderRuntimeLevel::Degraded, 10, 3),
            sample_runtime_item(3, ProviderRuntimeLevel::HalfOpen, 1, 1),
            sample_runtime_item(4, ProviderRuntimeLevel::Open, 1, 1),
            sample_runtime_item(5, ProviderRuntimeLevel::NoTraffic, 0, 0),
        ];

        let summary: DashboardRuntimeSummary = runtime_summary_from_items(&items);

        assert_eq!(summary.window, ProviderRuntimeWindow::OneHour);
        assert_eq!(summary.healthy_count, 1);
        assert_eq!(summary.degraded_count, 1);
        assert_eq!(summary.half_open_count, 1);
        assert_eq!(summary.open_count, 1);
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
    fn dashboard_alerts_include_half_open_and_cost_hotspots() {
        let mut expensive = sample_runtime_item(1, ProviderRuntimeLevel::Open, 20, 5);
        expensive.total_cost[0].amount_nanos = 500;

        let mut recovering = sample_runtime_item(2, ProviderRuntimeLevel::HalfOpen, 8, 2);
        recovering.total_cost[0].amount_nanos = 200;

        let mut steady = sample_runtime_item(3, ProviderRuntimeLevel::Healthy, 30, 1);
        steady.total_cost[0].amount_nanos = 900;

        let alerts = dashboard_alerts_from_runtime_items(&[expensive, recovering, steady]);

        assert_eq!(
            alerts
                .open_providers
                .iter()
                .map(|item| item.provider_id)
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(
            alerts
                .half_open_providers
                .iter()
                .map(|item| item.provider_id)
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(
            alerts
                .top_cost_providers
                .iter()
                .map(|item| item.provider_id)
                .collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
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
}
