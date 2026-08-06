use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::controller::BaseError;
use crate::database::metrics::{
    MetricRequestWindowAggregate, list_cost_rollup_minutes, list_http_status_rollup_minutes,
};
use crate::database::model::Model;
use crate::database::provider::{Provider, ProviderApiKeyRepository};
use crate::database::provider_runtime::{
    ProviderRuntimeAggregate, ProviderRuntimeCostAggregate, ProviderRuntimeStatusCodeCount,
    get_provider_runtime_aggregates_in_range,
};
use crate::schema::enum_def::UpstreamProfileType;
use crate::service::app_state::AppState;
use crate::service::runtime::{
    RuntimeStateBackendOperatorStatus, SourceHealthSnapshot, SourceHealthStatus,
};

use super::service::MetricsService;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderRuntimeWindow {
    #[serde(rename = "15m")]
    FifteenMinutes,
    #[serde(rename = "1h")]
    OneHour,
    #[serde(rename = "6h")]
    SixHours,
    #[serde(rename = "24h")]
    TwentyFourHours,
}

impl Default for ProviderRuntimeWindow {
    fn default() -> Self {
        Self::OneHour
    }
}

impl ProviderRuntimeWindow {
    pub fn from_duration_seconds(seconds: u64) -> Option<Self> {
        match seconds {
            900 => Some(ProviderRuntimeWindow::FifteenMinutes),
            3_600 => Some(ProviderRuntimeWindow::OneHour),
            21_600 => Some(ProviderRuntimeWindow::SixHours),
            86_400 => Some(ProviderRuntimeWindow::TwentyFourHours),
            _ => None,
        }
    }

    pub fn duration_seconds(self) -> u64 {
        match self {
            ProviderRuntimeWindow::FifteenMinutes => 900,
            ProviderRuntimeWindow::OneHour => 3_600,
            ProviderRuntimeWindow::SixHours => 21_600,
            ProviderRuntimeWindow::TwentyFourHours => 86_400,
        }
    }

    pub(crate) fn duration_ms(self) -> i64 {
        (self.duration_seconds() as i64).saturating_mul(1_000)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRuntimeHealthStatus {
    Healthy,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRuntimeLevel {
    Healthy,
    Degraded,
    Open,
    HalfOpen,
    NoTraffic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRuntimeStatusFilter {
    All,
    Healthy,
    Degraded,
    Open,
    HalfOpen,
    NoTraffic,
}

impl Default for ProviderRuntimeStatusFilter {
    fn default() -> Self {
        Self::All
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRuntimeSortField {
    Health,
    ErrorRate,
    Latency,
    TimeToFirstResponseBody,
    Ttft,
    LastErrorAt,
    RequestCount,
}

impl Default for ProviderRuntimeSortField {
    fn default() -> Self {
        Self::Health
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    Desc,
}

impl Default for SortDirection {
    fn default() -> Self {
        Self::Desc
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProviderRuntimeListParams {
    pub window: Option<ProviderRuntimeWindow>,
    #[serde(default)]
    pub status: ProviderRuntimeStatusFilter,
    pub search: Option<String>,
    #[serde(default)]
    pub sort: ProviderRuntimeSortField,
    #[serde(default)]
    pub direction: SortDirection,
    #[serde(default = "default_only_enabled")]
    pub only_enabled: bool,
}

fn default_only_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRuntimeStatusCodeStat {
    pub status_code: i32,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRuntimeCostStat {
    pub currency: String,
    pub amount_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRuntimeItem {
    pub provider_id: i64,
    pub provider_key: String,
    pub provider_name: String,
    pub is_enabled: bool,
    pub source_id: i64,
    pub source_key: String,
    pub source_profile_type: UpstreamProfileType,
    pub source_endpoint: String,
    pub source_use_proxy: bool,
    pub enabled_model_count: i64,
    pub enabled_provider_key_count: i64,
    pub health_status: ProviderRuntimeHealthStatus,
    pub runtime_level: ProviderRuntimeLevel,
    pub consecutive_failures: u32,
    pub half_open_probe_in_flight: bool,
    pub opened_at: Option<i64>,
    pub last_failure_at: Option<i64>,
    pub last_recovered_at: Option<i64>,
    pub last_error: Option<String>,
    pub runtime_state_backend_degraded: bool,
    pub runtime_state_backend_error: Option<String>,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub success_rate: Option<f64>,
    pub avg_time_to_first_response_body_ms: Option<f64>,
    pub time_to_first_response_body_sample_count: i64,
    pub avg_ttft_ms: Option<f64>,
    pub ttft_sample_count: i64,
    pub avg_total_latency_ms: Option<f64>,
    pub total_latency_sample_count: i64,
    pub last_request_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error_at: Option<i64>,
    pub last_error_summary: Option<String>,
    pub status_code_breakdown: Vec<ProviderRuntimeStatusCodeStat>,
    pub total_cost: Vec<ProviderRuntimeCostStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRuntimeSummary {
    pub total_provider_count: i64,
    pub healthy_count: i64,
    pub degraded_count: i64,
    pub half_open_count: i64,
    pub open_count: i64,
    pub no_traffic_count: i64,
    pub window: ProviderRuntimeWindow,
    pub generated_at: i64,
    pub runtime_state_backend: RuntimeStateBackendOperatorStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRuntimeSnapshot {
    pub items: Vec<ProviderRuntimeItem>,
    pub summary: ProviderRuntimeSummary,
}

impl MetricsService {
    pub fn default_provider_runtime_window(&self) -> ProviderRuntimeWindow {
        ProviderRuntimeWindow::from_duration_seconds(
            self.config().provider_runtime_default_window_seconds,
        )
        .unwrap_or_else(|| {
            crate::warn_event!(
                "metrics.provider_runtime_default_window_invalid",
                configured_seconds = self.config().provider_runtime_default_window_seconds,
                fallback_seconds = ProviderRuntimeWindow::OneHour.duration_seconds()
            );
            ProviderRuntimeWindow::OneHour
        })
    }

    pub fn provider_runtime_aggregates_in_range(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        provider_id_filter: Option<i64>,
    ) -> Result<Vec<ProviderRuntimeAggregate>, BaseError> {
        if !self.config().enabled {
            return self.provider_runtime_request_log_fallback(
                start_time_ms,
                end_time_ms,
                provider_id_filter,
                "metrics_disabled",
            );
        }

        let rollup_aggregates = self.provider_runtime_rollup_aggregates(
            start_time_ms,
            end_time_ms,
            provider_id_filter,
        )?;
        if !rollup_aggregates.is_empty() {
            return Ok(rollup_aggregates);
        }

        if self.config().request_log_query_fallback_enabled {
            return self.provider_runtime_request_log_fallback(
                start_time_ms,
                end_time_ms,
                provider_id_filter,
                "rollup_empty",
            );
        }

        Ok(Vec::new())
    }

    fn provider_runtime_rollup_aggregates(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        provider_id_filter: Option<i64>,
    ) -> Result<Vec<ProviderRuntimeAggregate>, BaseError> {
        let provider_scope_id = provider_id_filter.map(|value| value.to_string());
        let request_aggregates = self.query_request_window_metrics(
            start_time_ms,
            end_time_ms,
            Some("provider"),
            provider_scope_id.as_deref(),
        )?;
        let request_by_scope = request_aggregates
            .into_iter()
            .map(|item| (item.scope_id.clone(), item))
            .collect::<HashMap<_, _>>();
        let mut status_by_scope = HashMap::<String, HashMap<i32, i64>>::new();
        for row in list_http_status_rollup_minutes(
            start_time_ms,
            end_time_ms,
            "provider",
            provider_scope_id.as_deref(),
        )? {
            *status_by_scope
                .entry(row.scope_id)
                .or_default()
                .entry(row.http_status)
                .or_default() += row.count;
        }
        let mut cost_by_scope = HashMap::<String, BTreeMap<String, i64>>::new();
        for row in list_cost_rollup_minutes(
            start_time_ms,
            end_time_ms,
            Some("provider"),
            provider_scope_id.as_deref(),
        )? {
            *cost_by_scope
                .entry(row.scope_id)
                .or_default()
                .entry(row.currency)
                .or_default() += row.amount_nanos;
        }
        let mut scope_ids = request_by_scope.keys().cloned().collect::<Vec<_>>();
        scope_ids.sort();
        let mut result = Vec::with_capacity(scope_ids.len());

        for scope_id in scope_ids {
            let provider_id = match scope_id.parse::<i64>() {
                Ok(provider_id) => provider_id,
                Err(err) => {
                    crate::warn_event!(
                        "metrics.provider_runtime_invalid_scope_id",
                        scope_id = &scope_id,
                        error = err.to_string()
                    );
                    continue;
                }
            };
            let request = request_by_scope.get(&scope_id);

            let mut status_code_breakdown = status_by_scope
                .remove(&scope_id)
                .unwrap_or_default()
                .into_iter()
                .map(|(status_code, count)| ProviderRuntimeStatusCodeCount { status_code, count })
                .collect::<Vec<_>>();
            status_code_breakdown.sort_by(|left, right| {
                right
                    .count
                    .cmp(&left.count)
                    .then_with(|| left.status_code.cmp(&right.status_code))
            });

            let total_cost = cost_by_scope
                .remove(&scope_id)
                .unwrap_or_default()
                .into_iter()
                .map(|(currency, amount_nanos)| ProviderRuntimeCostAggregate {
                    currency,
                    amount_nanos,
                })
                .collect::<Vec<_>>();

            result.push(ProviderRuntimeAggregate {
                provider_id,
                request_count: request.map_or(0, |item| item.request_count),
                success_count: request.map_or(0, |item| item.success_count),
                error_count: request.map_or(0, |item| item.error_count + item.cancelled_count),
                avg_time_to_first_response_body_ms: provider_runtime_time_to_first_response_body(
                    request,
                ),
                time_to_first_response_body_sample_count: request
                    .map_or(0, |item| item.time_to_first_response_body_count),
                avg_ttft_ms: provider_runtime_ttft(request),
                ttft_sample_count: request.map_or(0, |item| item.ttft_count),
                avg_total_latency_ms: provider_runtime_total_latency(request),
                total_latency_sample_count: request.map_or(0, |item| item.total_latency_count),
                last_request_at: request.and_then(|item| item.last_request_at),
                last_success_at: request.and_then(|item| item.last_success_at),
                last_error_at: request.and_then(|item| item.last_error_at),
                status_code_breakdown,
                total_cost,
            });
        }

        result.sort_by_key(|item| item.provider_id);
        Ok(result)
    }

    fn provider_runtime_request_log_fallback(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        provider_id_filter: Option<i64>,
        reason: &'static str,
    ) -> Result<Vec<ProviderRuntimeAggregate>, BaseError> {
        if !self.config().request_log_query_fallback_enabled {
            return Ok(Vec::new());
        }

        let fallback = get_provider_runtime_aggregates_in_range(
            start_time_ms,
            end_time_ms,
            provider_id_filter,
        )?;
        if !fallback.is_empty() {
            let provider_filter = provider_id_filter
                .map(|value| value.to_string())
                .unwrap_or_else(|| "all".to_string());
            crate::warn_event!(
                "metrics.provider_runtime_request_log_fallback",
                reason = reason,
                start_time_ms = start_time_ms,
                end_time_ms = end_time_ms,
                provider_id_filter = &provider_filter,
                provider_count = fallback.len()
            );
        }
        Ok(fallback)
    }

    pub async fn build_provider_runtime_items(
        &self,
        app_state: &Arc<AppState>,
        window: ProviderRuntimeWindow,
        only_enabled: bool,
    ) -> Result<Vec<ProviderRuntimeItem>, BaseError> {
        let providers = if only_enabled {
            Provider::list_all_active()?
        } else {
            Provider::list_all()?
        };
        let models = Model::list_all()?;
        let provider_api_keys = ProviderApiKeyRepository::list_all_summaries()?;

        let now = Utc::now().timestamp_millis();
        let start_time_ms = now - window.duration_ms();
        let runtime_aggregates =
            self.provider_runtime_aggregates_in_range(start_time_ms, now, None)?;
        let aggregate_map = runtime_aggregates
            .into_iter()
            .map(|item| (item.provider_id, item))
            .collect::<HashMap<_, _>>();

        let mut enabled_model_count_by_provider: HashMap<i64, i64> = HashMap::new();
        for model in models {
            if !model.is_enabled {
                continue;
            }
            *enabled_model_count_by_provider
                .entry(model.provider_id)
                .or_insert(0) += 1;
        }

        let mut enabled_provider_key_count_by_provider: HashMap<i64, i64> = HashMap::new();
        for key in provider_api_keys {
            if !key.is_enabled {
                continue;
            }
            *enabled_provider_key_count_by_provider
                .entry(key.provider_id)
                .or_insert(0) += 1;
        }

        let mut items = Vec::with_capacity(providers.len());
        for provider in providers {
            let source = &provider.upstream_source;
            let (health_snapshot, runtime_state_backend_degraded, runtime_state_backend_error) =
                app_state
                    .source_circuit
                    .get_source_health_snapshot(source.id)
                    .await
                    .map(|snapshot| (snapshot, false, None))
                    .unwrap_or_else(|err| {
                        let error = err.to_string();
                        crate::warn_event!(
                            "runtime_state.read_failed",
                            read_model = "provider_runtime",
                            component = "source_circuit",
                            provider_id = provider.id,
                            source_id = source.id,
                            error = &error,
                        );
                        (SourceHealthSnapshot::default(), true, Some(error))
                    });
            let runtime_aggregate =
                aggregate_map
                    .get(&provider.id)
                    .cloned()
                    .unwrap_or(ProviderRuntimeAggregate {
                        provider_id: provider.id,
                        request_count: 0,
                        success_count: 0,
                        error_count: 0,
                        avg_time_to_first_response_body_ms: None,
                        time_to_first_response_body_sample_count: 0,
                        avg_ttft_ms: None,
                        ttft_sample_count: 0,
                        avg_total_latency_ms: None,
                        total_latency_sample_count: 0,
                        last_request_at: None,
                        last_success_at: None,
                        last_error_at: None,
                        status_code_breakdown: Vec::new(),
                        total_cost: Vec::new(),
                    });

            let runtime_level = compute_runtime_level(
                health_snapshot.status,
                runtime_aggregate.request_count,
                runtime_aggregate.error_count,
                runtime_aggregate.avg_total_latency_ms,
            );

            let item = ProviderRuntimeItem {
                provider_id: provider.id,
                provider_key: provider.provider_key.clone(),
                provider_name: provider.name.clone(),
                is_enabled: provider.is_enabled,
                source_id: source.id,
                source_key: source.source_key.clone(),
                source_profile_type: source.profile_type.clone(),
                source_endpoint: source.endpoint.clone(),
                source_use_proxy: source.use_proxy,
                enabled_model_count: enabled_model_count_by_provider
                    .get(&provider.id)
                    .copied()
                    .unwrap_or(0),
                enabled_provider_key_count: enabled_provider_key_count_by_provider
                    .get(&provider.id)
                    .copied()
                    .unwrap_or(0),
                health_status: map_health_status(health_snapshot.status),
                runtime_level,
                consecutive_failures: health_snapshot.consecutive_failures,
                half_open_probe_in_flight: health_snapshot.half_open_probe_in_flight,
                opened_at: health_snapshot.opened_at,
                last_failure_at: health_snapshot.last_failure_at,
                last_recovered_at: health_snapshot.last_recovered_at,
                last_error: health_snapshot.last_error.clone(),
                runtime_state_backend_degraded,
                runtime_state_backend_error,
                request_count: runtime_aggregate.request_count,
                success_count: runtime_aggregate.success_count,
                error_count: runtime_aggregate.error_count,
                success_rate: calculate_success_rate(
                    runtime_aggregate.request_count,
                    runtime_aggregate.success_count,
                ),
                avg_time_to_first_response_body_ms: runtime_aggregate
                    .avg_time_to_first_response_body_ms,
                time_to_first_response_body_sample_count: runtime_aggregate
                    .time_to_first_response_body_sample_count,
                avg_ttft_ms: runtime_aggregate.avg_ttft_ms,
                ttft_sample_count: runtime_aggregate.ttft_sample_count,
                avg_total_latency_ms: runtime_aggregate.avg_total_latency_ms,
                total_latency_sample_count: runtime_aggregate.total_latency_sample_count,
                last_request_at: runtime_aggregate.last_request_at,
                last_success_at: runtime_aggregate.last_success_at,
                last_error_at: runtime_aggregate.last_error_at,
                last_error_summary: build_last_error_summary(&health_snapshot, &runtime_aggregate),
                status_code_breakdown: runtime_aggregate
                    .status_code_breakdown
                    .into_iter()
                    .map(|item| ProviderRuntimeStatusCodeStat {
                        status_code: item.status_code,
                        count: item.count,
                    })
                    .collect(),
                total_cost: runtime_aggregate
                    .total_cost
                    .into_iter()
                    .map(|item| ProviderRuntimeCostStat {
                        currency: item.currency,
                        amount_nanos: item.amount_nanos,
                    })
                    .collect(),
            };
            items.push(item);
        }

        Ok(items)
    }

    pub async fn provider_runtime_summary_from_items(
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
}

fn average_or_none(sum: i64, count: i64) -> Option<f64> {
    if count > 0 {
        Some(sum as f64 / count as f64)
    } else {
        None
    }
}

fn provider_runtime_time_to_first_response_body(
    request: Option<&MetricRequestWindowAggregate>,
) -> Option<f64> {
    request.and_then(|item| {
        average_or_none(
            item.time_to_first_response_body_sum_ms,
            item.time_to_first_response_body_count,
        )
    })
}

fn provider_runtime_ttft(request: Option<&MetricRequestWindowAggregate>) -> Option<f64> {
    request.and_then(|item| average_or_none(item.ttft_sum_ms, item.ttft_count))
}

fn provider_runtime_total_latency(request: Option<&MetricRequestWindowAggregate>) -> Option<f64> {
    request.and_then(|item| average_or_none(item.total_latency_sum_ms, item.total_latency_count))
}

fn map_health_status(status: SourceHealthStatus) -> ProviderRuntimeHealthStatus {
    match status {
        SourceHealthStatus::Healthy => ProviderRuntimeHealthStatus::Healthy,
        SourceHealthStatus::Open => ProviderRuntimeHealthStatus::Open,
        SourceHealthStatus::HalfOpen => ProviderRuntimeHealthStatus::HalfOpen,
    }
}

fn calculate_success_rate(request_count: i64, success_count: i64) -> Option<f64> {
    if request_count > 0 {
        Some(success_count as f64 / request_count as f64)
    } else {
        None
    }
}

fn calculate_error_rate(request_count: i64, error_count: i64) -> Option<f64> {
    if request_count > 0 {
        Some(error_count as f64 / request_count as f64)
    } else {
        None
    }
}

pub(crate) fn compute_runtime_level(
    health_status: SourceHealthStatus,
    request_count: i64,
    error_count: i64,
    avg_total_latency_ms: Option<f64>,
) -> ProviderRuntimeLevel {
    match health_status {
        SourceHealthStatus::Open => ProviderRuntimeLevel::Open,
        SourceHealthStatus::HalfOpen => ProviderRuntimeLevel::HalfOpen,
        SourceHealthStatus::Healthy => {
            if request_count == 0 {
                return ProviderRuntimeLevel::NoTraffic;
            }

            let error_rate = calculate_error_rate(request_count, error_count).unwrap_or(0.0);
            let degraded_by_error_rate = request_count >= 5 && error_rate >= 0.2;
            let degraded_by_latency = avg_total_latency_ms.is_some_and(|value| value >= 10_000.0);

            if degraded_by_error_rate || degraded_by_latency {
                ProviderRuntimeLevel::Degraded
            } else {
                ProviderRuntimeLevel::Healthy
            }
        }
    }
}

pub(crate) fn build_last_error_summary(
    health_snapshot: &SourceHealthSnapshot,
    runtime_aggregate: &ProviderRuntimeAggregate,
) -> Option<String> {
    if let Some(last_error) = health_snapshot.last_error.as_ref() {
        return Some(last_error.clone());
    }

    let status_code = runtime_aggregate
        .status_code_breakdown
        .iter()
        .find(|item| item.status_code >= 400)
        .map(|item| item.status_code)?;
    Some(format!("Upstream status {}", status_code))
}

fn health_rank(level: ProviderRuntimeLevel) -> i32 {
    match level {
        ProviderRuntimeLevel::Open => 5,
        ProviderRuntimeLevel::HalfOpen => 4,
        ProviderRuntimeLevel::Degraded => 3,
        ProviderRuntimeLevel::Healthy => 2,
        ProviderRuntimeLevel::NoTraffic => 1,
    }
}

fn compare_f64_option(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

fn compare_i64_option(a: Option<i64>, b: Option<i64>) -> Ordering {
    a.cmp(&b)
}

pub(crate) fn matches_status_filter(
    runtime_level: ProviderRuntimeLevel,
    filter: ProviderRuntimeStatusFilter,
) -> bool {
    match filter {
        ProviderRuntimeStatusFilter::All => true,
        ProviderRuntimeStatusFilter::Healthy => runtime_level == ProviderRuntimeLevel::Healthy,
        ProviderRuntimeStatusFilter::Degraded => runtime_level == ProviderRuntimeLevel::Degraded,
        ProviderRuntimeStatusFilter::Open => runtime_level == ProviderRuntimeLevel::Open,
        ProviderRuntimeStatusFilter::HalfOpen => runtime_level == ProviderRuntimeLevel::HalfOpen,
        ProviderRuntimeStatusFilter::NoTraffic => runtime_level == ProviderRuntimeLevel::NoTraffic,
    }
}

pub(crate) fn search_matches(item: &ProviderRuntimeItem, search: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .map(|character| character.to_ascii_lowercase())
            .collect::<String>()
    };
    let needle = normalize(search);
    [
        item.provider_name.as_str(),
        item.provider_key.as_str(),
        item.source_key.as_str(),
        item.source_endpoint.as_str(),
    ]
    .into_iter()
    .any(|value| normalize(value).contains(&needle))
        || normalize(&format!("{:?}", item.source_profile_type)).contains(&needle)
}

pub(crate) fn sort_provider_runtime_items(
    items: &mut [ProviderRuntimeItem],
    sort: ProviderRuntimeSortField,
    direction: SortDirection,
) {
    items.sort_by(|left, right| {
        let ordering = match sort {
            ProviderRuntimeSortField::Health => health_rank(left.runtime_level)
                .cmp(&health_rank(right.runtime_level))
                .then_with(|| compare_f64_option(left.success_rate, right.success_rate).reverse())
                .then_with(|| compare_i64_option(left.last_error_at, right.last_error_at)),
            ProviderRuntimeSortField::ErrorRate => compare_f64_option(
                calculate_error_rate(left.request_count, left.error_count),
                calculate_error_rate(right.request_count, right.error_count),
            )
            .then_with(|| health_rank(left.runtime_level).cmp(&health_rank(right.runtime_level))),
            ProviderRuntimeSortField::Latency => {
                compare_f64_option(left.avg_total_latency_ms, right.avg_total_latency_ms).then_with(
                    || health_rank(left.runtime_level).cmp(&health_rank(right.runtime_level)),
                )
            }
            ProviderRuntimeSortField::TimeToFirstResponseBody => compare_f64_option(
                left.avg_time_to_first_response_body_ms,
                right.avg_time_to_first_response_body_ms,
            )
            .then_with(|| health_rank(left.runtime_level).cmp(&health_rank(right.runtime_level))),
            ProviderRuntimeSortField::Ttft => {
                compare_f64_option(left.avg_ttft_ms, right.avg_ttft_ms).then_with(|| {
                    health_rank(left.runtime_level).cmp(&health_rank(right.runtime_level))
                })
            }
            ProviderRuntimeSortField::LastErrorAt => {
                compare_i64_option(left.last_error_at, right.last_error_at).then_with(|| {
                    health_rank(left.runtime_level).cmp(&health_rank(right.runtime_level))
                })
            }
            ProviderRuntimeSortField::RequestCount => {
                left.request_count.cmp(&right.request_count).then_with(|| {
                    health_rank(left.runtime_level).cmp(&health_rank(right.runtime_level))
                })
            }
        };

        let with_tiebreaker = ordering
            .then_with(|| left.provider_name.cmp(&right.provider_name))
            .then_with(|| left.provider_id.cmp(&right.provider_id));

        match direction {
            SortDirection::Asc => with_tiebreaker,
            SortDirection::Desc => with_tiebreaker.reverse(),
        }
    });
}

pub(crate) fn first_runtime_backend_read_error(
    runtime_items: &[ProviderRuntimeItem],
) -> Option<String> {
    runtime_items
        .iter()
        .find_map(|item| item.runtime_state_backend_error.clone())
}

pub(crate) fn merge_runtime_backend_item_read_errors(
    status: &mut RuntimeStateBackendOperatorStatus,
    runtime_items: &[ProviderRuntimeItem],
    checked_at: i64,
) {
    if let Some(error) = first_runtime_backend_read_error(runtime_items) {
        status.runtime_degraded = true;
        if status.last_error.is_none() {
            status.last_error = Some(error);
            status.last_checked_at = checked_at;
        }
    }
}

pub(crate) async fn runtime_backend_status_for_provider_items(
    app_state: &Arc<AppState>,
    runtime_items: &[ProviderRuntimeItem],
) -> RuntimeStateBackendOperatorStatus {
    let mut status = app_state.runtime_state_backend_operator_status().await;
    merge_runtime_backend_item_read_errors(
        &mut status,
        runtime_items,
        Utc::now().timestamp_millis(),
    );
    status
}
