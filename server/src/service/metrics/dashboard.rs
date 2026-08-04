use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;

use crate::controller::BaseError;
use crate::database::metrics::{query_cost_window_aggregates, query_request_window_aggregates};
use crate::database::stat::{
    DashboardOverviewStats as DbDashboardOverviewStats,
    DashboardTodayStats as DbDashboardTodayStats, get_dashboard_overview_stats,
    get_dashboard_today_stats, start_of_today_timestamp_ms,
};
use crate::service::app_state::AppState;
use crate::service::metrics::provider_runtime::{ProviderRuntimeItem, ProviderRuntimeSummary};

use super::service::MetricsService;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetricsDashboardTodayStats {
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub success_rate: Option<f64>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_reasoning_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: HashMap<String, i64>,
    pub avg_time_to_first_response_body_ms: Option<f64>,
    pub time_to_first_response_body_sample_count: i64,
    pub avg_ttft_ms: Option<f64>,
    pub ttft_sample_count: i64,
    pub avg_total_latency_ms: Option<f64>,
    pub total_latency_sample_count: i64,
    pub active_provider_count: i64,
    pub active_model_count: i64,
    pub active_api_key_count: i64,
}

#[derive(Debug)]
pub struct MetricsDashboardResourcesReadModel {
    pub overview: DbDashboardOverviewStats,
    pub today: MetricsDashboardTodayStats,
    pub runtime: ProviderRuntimeSummary,
    pub runtime_items: Vec<ProviderRuntimeItem>,
}

impl From<DbDashboardTodayStats> for MetricsDashboardTodayStats {
    fn from(value: DbDashboardTodayStats) -> Self {
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

impl MetricsService {
    pub async fn build_dashboard_resources(
        &self,
        app_state: &Arc<AppState>,
        timezone: Option<&str>,
    ) -> Result<MetricsDashboardResourcesReadModel, BaseError> {
        let overview = get_dashboard_overview_stats()?;
        let today = self.dashboard_today_stats(timezone)?;
        let window = self.default_provider_runtime_window();
        let runtime_items = self
            .build_provider_runtime_items(app_state, window, true)
            .await?;
        let runtime = self
            .provider_runtime_summary_from_items(app_state, window, &runtime_items)
            .await;
        Ok(MetricsDashboardResourcesReadModel {
            overview,
            today,
            runtime,
            runtime_items,
        })
    }

    pub fn dashboard_today_stats(
        &self,
        timezone: Option<&str>,
    ) -> Result<MetricsDashboardTodayStats, BaseError> {
        if !self.config().enabled {
            return self.dashboard_today_request_log_fallback(timezone, "metrics_disabled");
        }

        let start_time_ms = start_of_today_timestamp_ms(timezone)?;
        let end_time_ms = Utc::now().timestamp_millis();
        let global_aggregates = query_request_window_aggregates(
            start_time_ms,
            end_time_ms,
            Some("global"),
            Some("global"),
        )?;

        let Some(global) = global_aggregates.into_iter().next() else {
            if self.config().request_log_query_fallback_enabled {
                return self.dashboard_today_request_log_fallback(timezone, "rollup_empty");
            }
            return Ok(MetricsDashboardTodayStats::default());
        };

        let total_cost =
            query_cost_window_aggregates(start_time_ms, end_time_ms, "global", "global")?
                .into_iter()
                .map(|item| (item.currency, item.amount_nanos))
                .collect::<HashMap<_, _>>();

        Ok(MetricsDashboardTodayStats {
            request_count: global.request_count,
            success_count: global.success_count,
            error_count: global.error_count + global.cancelled_count,
            success_rate: calculate_success_rate(global.request_count, global.success_count),
            total_input_tokens: global.input_tokens,
            total_output_tokens: global.output_tokens,
            total_reasoning_tokens: global.reasoning_tokens,
            total_tokens: global.total_tokens,
            total_cost,
            avg_time_to_first_response_body_ms: average_or_none(
                global.time_to_first_response_body_sum_ms,
                global.time_to_first_response_body_count,
            ),
            time_to_first_response_body_sample_count: global.time_to_first_response_body_count,
            avg_ttft_ms: average_or_none(global.ttft_sum_ms, global.ttft_count),
            ttft_sample_count: global.ttft_count,
            avg_total_latency_ms: average_or_none(
                global.total_latency_sum_ms,
                global.total_latency_count,
            ),
            total_latency_sample_count: global.total_latency_count,
            active_provider_count: active_scope_count(start_time_ms, end_time_ms, "provider")?,
            active_model_count: active_scope_count(start_time_ms, end_time_ms, "model")?,
            active_api_key_count: active_scope_count(start_time_ms, end_time_ms, "api_key")?,
        })
    }

    fn dashboard_today_request_log_fallback(
        &self,
        timezone: Option<&str>,
        reason: &'static str,
    ) -> Result<MetricsDashboardTodayStats, BaseError> {
        if !self.config().request_log_query_fallback_enabled {
            return Ok(MetricsDashboardTodayStats::default());
        }

        let fallback = get_dashboard_today_stats(timezone)?;
        if fallback.request_count > 0 {
            crate::warn_event!(
                "metrics.dashboard_today_request_log_fallback",
                reason = reason,
                request_count = fallback.request_count
            );
        }
        Ok(fallback.into())
    }
}

fn active_scope_count(
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type: &str,
) -> Result<i64, BaseError> {
    Ok(
        query_request_window_aggregates(start_time_ms, end_time_ms, Some(scope_type), None)?
            .into_iter()
            .filter(|item| item.request_count > 0)
            .count() as i64,
    )
}

fn calculate_success_rate(request_count: i64, success_count: i64) -> Option<f64> {
    if request_count > 0 {
        Some(success_count as f64 / request_count as f64)
    } else {
        None
    }
}

fn average_or_none(sum: i64, count: i64) -> Option<f64> {
    if count > 0 {
        Some(sum as f64 / count as f64)
    } else {
        None
    }
}
