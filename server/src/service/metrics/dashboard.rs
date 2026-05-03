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
use crate::service::metrics::provider_runtime::{
    ProviderRuntimeItem, ProviderRuntimeLevel, ProviderRuntimeSummary, ProviderRuntimeWindow,
    runtime_backend_status_for_provider_items,
};

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
    pub avg_first_byte_ms: Option<f64>,
    pub avg_total_latency_ms: Option<f64>,
    pub active_provider_count: i64,
    pub active_model_count: i64,
    pub active_api_key_count: i64,
}

#[derive(Debug)]
pub struct MetricsDashboardKpiReadModel {
    pub today: MetricsDashboardTodayStats,
    pub runtime: ProviderRuntimeSummary,
}

#[derive(Debug)]
pub struct MetricsDashboardResourcesReadModel {
    pub overview: DbDashboardOverviewStats,
    pub today: MetricsDashboardTodayStats,
    pub runtime: ProviderRuntimeSummary,
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
            avg_first_byte_ms: value.avg_first_byte_ms,
            avg_total_latency_ms: value.avg_total_latency_ms,
            active_provider_count: value.active_provider_count,
            active_model_count: value.active_model_count,
            active_api_key_count: value.active_api_key_count,
        }
    }
}

impl MetricsService {
    pub async fn build_dashboard_kpi(
        &self,
        app_state: &Arc<AppState>,
        timezone: Option<&str>,
    ) -> Result<MetricsDashboardKpiReadModel, BaseError> {
        let today = self.dashboard_today_stats(timezone)?;
        let window = self.default_provider_runtime_window();
        let runtime_items = self
            .build_provider_runtime_items(app_state, window, true)
            .await?;
        let runtime = self
            .dashboard_runtime_summary_from_items(app_state, window, &runtime_items)
            .await;
        Ok(MetricsDashboardKpiReadModel { today, runtime })
    }

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
            .dashboard_runtime_summary_from_items(app_state, window, &runtime_items)
            .await;
        Ok(MetricsDashboardResourcesReadModel {
            overview,
            today,
            runtime,
        })
    }

    pub async fn dashboard_runtime_summary_from_items(
        &self,
        app_state: &Arc<AppState>,
        window: ProviderRuntimeWindow,
        items: &[ProviderRuntimeItem],
    ) -> ProviderRuntimeSummary {
        let runtime_state_backend =
            runtime_backend_status_for_provider_items(app_state, items).await;
        let mut summary = ProviderRuntimeSummary {
            total_provider_count: items.len() as i64,
            healthy_count: 0,
            degraded_count: 0,
            half_open_count: 0,
            open_count: 0,
            no_traffic_count: 0,
            window,
            generated_at: Utc::now().timestamp_millis(),
            runtime_state_backend,
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

        let total_cost = query_cost_window_aggregates(
            start_time_ms,
            end_time_ms,
            "request",
            "global",
            "global",
        )?
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
            avg_first_byte_ms: average_or_none(
                global.first_byte_latency_sum_ms,
                global.first_byte_latency_count,
            ),
            avg_total_latency_ms: average_or_none(
                global.total_latency_sum_ms,
                global.total_latency_count,
            ),
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

#[cfg(test)]
mod tests {
    use super::MetricsDashboardTodayStats;
    use crate::config::MetricsConfig;
    use crate::database::TestDbContext;
    use crate::database::metrics::{
        MetricCostRollupMinute, MetricRequestRollupMinute, add_cost_rollup_delta,
        add_request_rollup_delta,
    };
    use crate::service::app_state::{AppState, create_test_app_state};
    use crate::service::metrics::MetricsService;
    use crate::service::metrics::provider_runtime::ProviderRuntimeWindow;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn with_metrics_config(
        app_state: Arc<AppState>,
        metrics_config: MetricsConfig,
    ) -> Arc<AppState> {
        Arc::new(AppState {
            metrics: Arc::new(MetricsService::new(metrics_config)),
            ..(*app_state).clone()
        })
    }

    #[test]
    fn dashboard_today_stats_use_global_rollup_and_active_scopes() {
        let context = TestDbContext::new_sqlite("metrics-dashboard-today.sqlite");
        context.run_sync(|| {
            let bucket_start_ms =
                (chrono::Utc::now().timestamp_millis() - 60_000).div_euclid(60_000) * 60_000;
            add_request_rollup_delta(&request_rollup(
                bucket_start_ms,
                "global",
                "global",
                3,
                1,
                1,
                1,
                120,
                2,
                1_200,
                3,
                10,
                20,
                5,
                35,
            ))
            .unwrap();
            add_request_rollup_delta(&request_rollup(
                bucket_start_ms,
                "provider",
                "7",
                2,
                1,
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ))
            .unwrap();
            add_request_rollup_delta(&request_rollup(
                bucket_start_ms,
                "provider",
                "8",
                1,
                0,
                0,
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ))
            .unwrap();
            add_request_rollup_delta(&request_rollup(
                bucket_start_ms,
                "model",
                "11",
                3,
                1,
                1,
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ))
            .unwrap();
            add_request_rollup_delta(&request_rollup(
                bucket_start_ms,
                "api_key",
                "21",
                2,
                1,
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ))
            .unwrap();
            add_request_rollup_delta(&request_rollup(
                bucket_start_ms,
                "api_key",
                "22",
                1,
                0,
                0,
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ))
            .unwrap();
            add_cost_rollup_delta(&cost_rollup(bucket_start_ms, "USD", 1_500)).unwrap();
            add_cost_rollup_delta(&cost_rollup(bucket_start_ms, "CNY", 2_500)).unwrap();

            let service = MetricsService::new(MetricsConfig {
                request_log_query_fallback_enabled: false,
                ..MetricsConfig::default()
            });
            let stats = service.dashboard_today_stats(Some("UTC")).unwrap();

            let expected_cost =
                HashMap::from([("CNY".to_string(), 2_500), ("USD".to_string(), 1_500)]);
            assert_eq!(
                stats,
                MetricsDashboardTodayStats {
                    request_count: 3,
                    success_count: 1,
                    error_count: 2,
                    success_rate: Some(1.0 / 3.0),
                    total_input_tokens: 10,
                    total_output_tokens: 20,
                    total_reasoning_tokens: 5,
                    total_tokens: 35,
                    total_cost: expected_cost,
                    avg_first_byte_ms: Some(60.0),
                    avg_total_latency_ms: Some(400.0),
                    active_provider_count: 2,
                    active_model_count: 1,
                    active_api_key_count: 2,
                }
            );
        });
    }

    #[tokio::test]
    async fn dashboard_kpi_and_resources_use_configured_provider_runtime_window() {
        let context = TestDbContext::new_sqlite("metrics-dashboard-default-window.sqlite");
        context
            .run_async(async {
                let app_state = create_test_app_state(context.clone()).await;
                let app_state = with_metrics_config(
                    app_state,
                    MetricsConfig {
                        provider_runtime_default_window_seconds: 900,
                        ..MetricsConfig::default()
                    },
                );

                let kpi = app_state
                    .metrics
                    .build_dashboard_kpi(&app_state, Some("UTC"))
                    .await
                    .expect("dashboard kpi should build");
                assert_eq!(kpi.runtime.window, ProviderRuntimeWindow::FifteenMinutes);

                let resources = app_state
                    .metrics
                    .build_dashboard_resources(&app_state, Some("UTC"))
                    .await
                    .expect("dashboard resources should build");
                assert_eq!(
                    resources.runtime.window,
                    ProviderRuntimeWindow::FifteenMinutes
                );
            })
            .await;
    }

    fn request_rollup(
        bucket_start_ms: i64,
        scope_type: &str,
        scope_id: &str,
        request_count: i64,
        success_count: i64,
        error_count: i64,
        cancelled_count: i64,
        first_byte_latency_sum_ms: i64,
        first_byte_latency_count: i64,
        total_latency_sum_ms: i64,
        total_latency_count: i64,
        input_tokens: i64,
        output_tokens: i64,
        reasoning_tokens: i64,
        total_tokens: i64,
    ) -> MetricRequestRollupMinute {
        MetricRequestRollupMinute {
            bucket_start_ms,
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            scope_label: None,
            request_count,
            success_count,
            error_count,
            cancelled_count,
            retry_count: 0,
            fallback_count: 0,
            first_byte_latency_sum_ms,
            first_byte_latency_count,
            total_latency_sum_ms,
            total_latency_count,
            input_tokens,
            output_tokens,
            reasoning_tokens,
            total_tokens,
            transform_diagnostic_count: 0,
            transform_diagnostic_lossy_major_count: 0,
            transform_diagnostic_reject_count: 0,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn cost_rollup(
        bucket_start_ms: i64,
        currency: &str,
        amount_nanos: i64,
    ) -> MetricCostRollupMinute {
        MetricCostRollupMinute {
            bucket_start_ms,
            metric_kind: "request".to_string(),
            scope_type: "global".to_string(),
            scope_id: "global".to_string(),
            currency: currency.to_string(),
            amount_nanos,
            created_at: 1,
            updated_at: 1,
        }
    }
}
