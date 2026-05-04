use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::controller::BaseError;
use crate::database::metrics::{
    MetricAttemptWindowAggregate, MetricRequestWindowAggregate, query_cost_window_aggregates,
};
use crate::database::model::Model;
use crate::database::provider::{Provider, ProviderApiKey};
use crate::database::provider_runtime::{
    ProviderRuntimeAggregate, ProviderRuntimeCostAggregate, ProviderRuntimeStatusCodeCount,
    get_provider_runtime_aggregates_in_range,
};
use crate::schema::enum_def::ProviderType;
use crate::service::app_state::AppState;
use crate::service::runtime::{
    ProviderHealthSnapshot, ProviderHealthStatus, RuntimeStateBackendOperatorStatus,
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

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProviderRuntimeSummaryParams {
    pub window: Option<ProviderRuntimeWindow>,
    #[serde(default = "default_only_enabled")]
    pub only_enabled: bool,
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
    pub provider_type: String,
    pub is_enabled: bool,
    pub use_proxy: bool,
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
    pub avg_first_byte_ms: Option<f64>,
    pub avg_total_latency_ms: Option<f64>,
    pub last_request_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error_at: Option<i64>,
    pub last_error_summary: Option<String>,
    pub status_code_breakdown: Vec<ProviderRuntimeStatusCodeStat>,
    pub total_cost: Vec<ProviderRuntimeCostStat>,
    pub sort_score: f64,
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
        let attempt_aggregates = self.query_attempt_window_metrics(
            start_time_ms,
            end_time_ms,
            Some("provider"),
            provider_scope_id.as_deref(),
        )?;
        let request_by_scope = request_aggregates
            .into_iter()
            .map(|item| (item.scope_id.clone(), item))
            .collect::<HashMap<_, _>>();
        let attempt_by_scope = attempt_aggregates
            .into_iter()
            .map(|item| (item.scope_id.clone(), item))
            .collect::<HashMap<_, _>>();
        let mut scope_ids = request_by_scope.keys().cloned().collect::<Vec<_>>();
        for scope_id in attempt_by_scope.keys() {
            if !request_by_scope.contains_key(scope_id) {
                scope_ids.push(scope_id.clone());
            }
        }
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
            let attempt = attempt_by_scope.get(&scope_id);

            let status_code_breakdown = self
                .query_http_status_breakdown(start_time_ms, end_time_ms, "provider", &scope_id)?
                .into_iter()
                .map(|item| ProviderRuntimeStatusCodeCount {
                    status_code: item.status_code,
                    count: item.count,
                })
                .collect::<Vec<_>>();

            let total_cost = query_cost_window_aggregates(
                start_time_ms,
                end_time_ms,
                "request",
                "provider",
                &scope_id,
            )?
            .into_iter()
            .map(|item| ProviderRuntimeCostAggregate {
                currency: item.currency,
                amount_nanos: item.amount_nanos,
            })
            .collect::<Vec<_>>();

            result.push(ProviderRuntimeAggregate {
                provider_id,
                request_count: request.map_or(0, |item| item.request_count),
                success_count: request.map_or(0, |item| item.success_count),
                error_count: request.map_or(0, |item| item.error_count + item.cancelled_count),
                avg_first_byte_ms: provider_runtime_first_byte_latency(request, attempt),
                avg_total_latency_ms: provider_runtime_total_latency(request, attempt),
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
        let provider_api_keys = ProviderApiKey::list_all()?;

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
            let (health_snapshot, runtime_state_backend_degraded, runtime_state_backend_error) =
                app_state
                    .provider_circuit
                    .get_provider_health_snapshot(provider.id)
                    .await
                    .map(|snapshot| (snapshot, false, None))
                    .unwrap_or_else(|err| {
                        let error = err.to_string();
                        crate::warn_event!(
                            "runtime_state.read_failed",
                            read_model = "provider_runtime",
                            component = "provider_circuit",
                            provider_id = provider.id,
                            error = &error,
                        );
                        (ProviderHealthSnapshot::default(), true, Some(error))
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
                        avg_first_byte_ms: None,
                        avg_total_latency_ms: None,
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
                runtime_state_backend_degraded,
            );

            let mut item = ProviderRuntimeItem {
                provider_id: provider.id,
                provider_key: provider.provider_key.clone(),
                provider_name: provider.name.clone(),
                provider_type: map_provider_type(&provider.provider_type).to_string(),
                is_enabled: provider.is_enabled,
                use_proxy: provider.use_proxy,
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
                avg_first_byte_ms: runtime_aggregate.avg_first_byte_ms,
                avg_total_latency_ms: runtime_aggregate.avg_total_latency_ms,
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
                sort_score: 0.0,
            };
            item.sort_score = compute_sort_score(&item);
            items.push(item);
        }

        Ok(items)
    }
}

fn average_or_none(sum: i64, count: i64) -> Option<f64> {
    if count > 0 {
        Some(sum as f64 / count as f64)
    } else {
        None
    }
}

fn provider_runtime_first_byte_latency(
    request: Option<&MetricRequestWindowAggregate>,
    attempt: Option<&MetricAttemptWindowAggregate>,
) -> Option<f64> {
    request
        .and_then(|item| {
            average_or_none(
                item.first_byte_latency_sum_ms,
                item.first_byte_latency_count,
            )
        })
        .or_else(|| {
            attempt.and_then(|item| {
                average_or_none(
                    item.first_byte_latency_sum_ms,
                    item.first_byte_latency_count,
                )
            })
        })
}

fn provider_runtime_total_latency(
    request: Option<&MetricRequestWindowAggregate>,
    attempt: Option<&MetricAttemptWindowAggregate>,
) -> Option<f64> {
    request
        .and_then(|item| average_or_none(item.total_latency_sum_ms, item.total_latency_count))
        .or_else(|| {
            attempt.and_then(|item| {
                average_or_none(item.total_latency_sum_ms, item.total_latency_count)
            })
        })
}

fn map_health_status(status: ProviderHealthStatus) -> ProviderRuntimeHealthStatus {
    match status {
        ProviderHealthStatus::Healthy => ProviderRuntimeHealthStatus::Healthy,
        ProviderHealthStatus::Open => ProviderRuntimeHealthStatus::Open,
        ProviderHealthStatus::HalfOpen => ProviderRuntimeHealthStatus::HalfOpen,
    }
}

fn map_provider_type(provider_type: &ProviderType) -> &'static str {
    match provider_type {
        ProviderType::Openai => "OPENAI",
        ProviderType::Gemini => "GEMINI",
        ProviderType::Vertex => "VERTEX",
        ProviderType::VertexOpenai => "VERTEX_OPENAI",
        ProviderType::Ollama => "OLLAMA",
        ProviderType::Anthropic => "ANTHROPIC",
        ProviderType::Responses => "RESPONSES",
        ProviderType::GeminiOpenai => "GEMINI_OPENAI",
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
    health_status: ProviderHealthStatus,
    request_count: i64,
    error_count: i64,
    avg_total_latency_ms: Option<f64>,
    runtime_state_backend_degraded: bool,
) -> ProviderRuntimeLevel {
    if runtime_state_backend_degraded {
        return ProviderRuntimeLevel::Degraded;
    }

    match health_status {
        ProviderHealthStatus::Open => ProviderRuntimeLevel::Open,
        ProviderHealthStatus::HalfOpen => ProviderRuntimeLevel::HalfOpen,
        ProviderHealthStatus::Healthy => {
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
    health_snapshot: &ProviderHealthSnapshot,
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

fn compute_sort_score(item: &ProviderRuntimeItem) -> f64 {
    let error_rate = calculate_error_rate(item.request_count, item.error_count).unwrap_or(0.0);
    (health_rank(item.runtime_level) as f64 * 1_000_000.0)
        + error_rate * 10_000.0
        + item.last_error_at.unwrap_or(0) as f64 / 1000.0
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

pub(crate) fn search_matches(provider_name: &str, provider_key: &str, search: &str) -> bool {
    let needle = search.to_ascii_lowercase();
    provider_name.to_ascii_lowercase().contains(&needle)
        || provider_key.to_ascii_lowercase().contains(&needle)
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

#[cfg(test)]
mod tests {
    use super::{
        ProviderRuntimeCostStat, ProviderRuntimeHealthStatus, ProviderRuntimeItem,
        ProviderRuntimeLevel, ProviderRuntimeListParams, ProviderRuntimeSortField,
        ProviderRuntimeStatusCodeStat, ProviderRuntimeStatusFilter, ProviderRuntimeSummaryParams,
        ProviderRuntimeWindow, SortDirection, build_last_error_summary, compute_runtime_level,
        merge_runtime_backend_item_read_errors, sort_provider_runtime_items,
    };
    use crate::config::MetricsConfig;
    use crate::database::TestDbContext;
    use crate::database::metrics::{
        MetricAttemptRollupMinute, MetricCostRollupMinute, MetricHttpStatusRollupMinute,
        MetricRequestRollupMinute, add_attempt_rollup_delta, add_cost_rollup_delta,
        add_http_status_rollup_delta, add_request_rollup_delta,
    };
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::provider_runtime::{
        ProviderRuntimeAggregate, ProviderRuntimeStatusCodeCount,
    };
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::app_state::create_test_app_state;
    use crate::service::metrics::MetricsService;
    use crate::service::runtime::{ProviderHealthSnapshot, ProviderHealthStatus};

    #[test]
    fn provider_runtime_enums_serialize_to_stable_contract_values() {
        assert_eq!(
            serde_json::to_string(&ProviderRuntimeWindow::OneHour).unwrap(),
            "\"1h\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderRuntimeHealthStatus::HalfOpen).unwrap(),
            "\"half_open\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderRuntimeLevel::NoTraffic).unwrap(),
            "\"no_traffic\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderRuntimeStatusFilter::Degraded).unwrap(),
            "\"degraded\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderRuntimeSortField::LastErrorAt).unwrap(),
            "\"last_error_at\""
        );
        assert_eq!(
            serde_json::to_string(&SortDirection::Desc).unwrap(),
            "\"desc\""
        );
    }

    #[test]
    fn compute_runtime_level_handles_open_half_open_and_no_traffic() {
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Open, 10, 10, Some(20_000.0), false),
            ProviderRuntimeLevel::Open
        );
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::HalfOpen, 10, 0, Some(100.0), false),
            ProviderRuntimeLevel::HalfOpen
        );
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Healthy, 0, 0, None, false),
            ProviderRuntimeLevel::NoTraffic
        );
    }

    #[test]
    fn compute_runtime_level_marks_high_error_rate_and_latency_as_degraded() {
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Healthy, 5, 1, Some(500.0), false),
            ProviderRuntimeLevel::Degraded
        );
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Healthy, 3, 0, Some(10_000.0), false),
            ProviderRuntimeLevel::Degraded
        );
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Healthy, 10, 1, Some(500.0), false),
            ProviderRuntimeLevel::Healthy
        );
    }

    #[test]
    fn compute_runtime_level_marks_runtime_state_backend_error_as_degraded() {
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Healthy, 0, 0, None, true),
            ProviderRuntimeLevel::Degraded
        );
    }

    #[test]
    fn health_sort_prioritizes_open_then_half_open() {
        let mut items = vec![
            sample_item(
                1,
                "healthy",
                ProviderRuntimeLevel::Healthy,
                100,
                0,
                Some(100.0),
                None,
            ),
            sample_item(
                2,
                "open",
                ProviderRuntimeLevel::Open,
                10,
                10,
                Some(500.0),
                Some(2_000),
            ),
            sample_item(
                3,
                "half-open",
                ProviderRuntimeLevel::HalfOpen,
                5,
                2,
                Some(300.0),
                Some(1_000),
            ),
        ];

        sort_provider_runtime_items(
            &mut items,
            ProviderRuntimeSortField::Health,
            SortDirection::Desc,
        );

        let ordered_ids = items
            .iter()
            .map(|item| item.provider_id)
            .collect::<Vec<_>>();
        assert_eq!(ordered_ids, vec![2, 3, 1]);
    }

    #[test]
    fn build_last_error_summary_ignores_success_status_codes() {
        let health_snapshot = ProviderHealthSnapshot {
            status: ProviderHealthStatus::Healthy,
            consecutive_failures: 0,
            half_open_probe_in_flight: false,
            opened_at: None,
            last_failure_at: None,
            last_recovered_at: None,
            last_error: None,
        };
        let runtime_aggregate = ProviderRuntimeAggregate {
            provider_id: 1,
            request_count: 3,
            success_count: 3,
            error_count: 0,
            avg_first_byte_ms: None,
            avg_total_latency_ms: None,
            last_request_at: Some(3_000),
            last_success_at: Some(3_000),
            last_error_at: None,
            status_code_breakdown: vec![
                ProviderRuntimeStatusCodeCount {
                    status_code: 200,
                    count: 2,
                },
                ProviderRuntimeStatusCodeCount {
                    status_code: 302,
                    count: 1,
                },
            ],
            total_cost: Vec::new(),
        };

        assert_eq!(
            build_last_error_summary(&health_snapshot, &runtime_aggregate),
            None
        );
    }

    #[test]
    fn build_last_error_summary_uses_first_failing_status_code_when_no_error_message() {
        let health_snapshot = ProviderHealthSnapshot {
            status: ProviderHealthStatus::Healthy,
            consecutive_failures: 0,
            half_open_probe_in_flight: false,
            opened_at: None,
            last_failure_at: None,
            last_recovered_at: None,
            last_error: None,
        };
        let runtime_aggregate = ProviderRuntimeAggregate {
            provider_id: 1,
            request_count: 4,
            success_count: 2,
            error_count: 2,
            avg_first_byte_ms: None,
            avg_total_latency_ms: None,
            last_request_at: Some(4_000),
            last_success_at: Some(2_000),
            last_error_at: Some(4_000),
            status_code_breakdown: vec![
                ProviderRuntimeStatusCodeCount {
                    status_code: 200,
                    count: 2,
                },
                ProviderRuntimeStatusCodeCount {
                    status_code: 429,
                    count: 1,
                },
                ProviderRuntimeStatusCodeCount {
                    status_code: 500,
                    count: 1,
                },
            ],
            total_cost: Vec::new(),
        };

        assert_eq!(
            build_last_error_summary(&health_snapshot, &runtime_aggregate),
            Some("Upstream status 429".to_string())
        );
    }

    #[test]
    fn provider_runtime_params_distinguish_missing_and_explicit_window() {
        let params: ProviderRuntimeSummaryParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.window, None);
        assert!(params.only_enabled);

        let params: ProviderRuntimeSummaryParams =
            serde_json::from_str(r#"{"window":"6h"}"#).unwrap();
        assert_eq!(params.window, Some(ProviderRuntimeWindow::SixHours));

        let params: ProviderRuntimeListParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.window, None);
        assert!(params.only_enabled);
    }

    #[test]
    fn default_provider_runtime_window_uses_allowed_config_or_falls_back() {
        for (seconds, expected) in [
            (900, ProviderRuntimeWindow::FifteenMinutes),
            (3_600, ProviderRuntimeWindow::OneHour),
            (21_600, ProviderRuntimeWindow::SixHours),
            (86_400, ProviderRuntimeWindow::TwentyFourHours),
        ] {
            let service = MetricsService::new(MetricsConfig {
                provider_runtime_default_window_seconds: seconds,
                ..MetricsConfig::default()
            });
            assert_eq!(service.default_provider_runtime_window(), expected);
        }

        let service = MetricsService::new(MetricsConfig {
            provider_runtime_default_window_seconds: 42,
            ..MetricsConfig::default()
        });
        assert_eq!(
            service.default_provider_runtime_window(),
            ProviderRuntimeWindow::OneHour
        );
    }

    #[test]
    fn runtime_backend_status_merge_marks_item_read_errors_degraded() {
        let mut status = crate::service::runtime::RuntimeStateBackendOperatorStatus {
            deployment_mode: "single_instance".to_string(),
            catalog_cache_backend: "memory".to_string(),
            catalog_cache_configured_backend: "memory".to_string(),
            catalog_cache_effective_backend: "memory".to_string(),
            catalog_cache_fallback_reason: None,
            runtime_configured_backend: "redis".to_string(),
            runtime_effective_backend: "redis".to_string(),
            runtime_shared: true,
            runtime_degraded: false,
            fallback_reason: None,
            last_error: None,
            last_checked_at: 1,
        };
        let mut item = sample_item(
            1,
            "redis-backed",
            ProviderRuntimeLevel::Healthy,
            0,
            0,
            None,
            None,
        );
        item.runtime_state_backend_degraded = true;
        item.runtime_state_backend_error = Some("provider circuit snapshot failed".to_string());

        merge_runtime_backend_item_read_errors(&mut status, &[item], 2);

        assert!(status.runtime_degraded);
        assert_eq!(
            status.last_error.as_deref(),
            Some("provider circuit snapshot failed")
        );
        assert_eq!(status.last_checked_at, 2);
    }

    #[test]
    fn provider_runtime_aggregates_use_metrics_rollups_and_attempt_statuses() {
        let context = TestDbContext::new_sqlite("metrics-provider-runtime.sqlite");
        context.run_sync(|| {
            add_request_rollup_delta(&request_rollup(60_000, 2, 1, 1, 0, 180, 2, 1_200, 2))
                .unwrap();
            add_request_rollup_delta(&request_rollup(120_000, 1, 0, 0, 1, 0, 0, 1_000, 1)).unwrap();
            add_http_status_rollup_delta(&http_status_rollup(60_000, 500, 1)).unwrap();
            add_http_status_rollup_delta(&http_status_rollup(120_000, 429, 2)).unwrap();
            add_cost_rollup_delta(&cost_rollup(60_000, "USD", 1_000)).unwrap();
            add_cost_rollup_delta(&cost_rollup(120_000, "USD", 2_500)).unwrap();

            let service = MetricsService::new(MetricsConfig {
                request_log_query_fallback_enabled: false,
                ..MetricsConfig::default()
            });
            let aggregates = service
                .provider_runtime_aggregates_in_range(0, 180_000, None)
                .unwrap();

            assert_eq!(aggregates.len(), 1);
            let aggregate = &aggregates[0];
            assert_eq!(aggregate.provider_id, 7);
            assert_eq!(aggregate.request_count, 3);
            assert_eq!(aggregate.success_count, 1);
            assert_eq!(aggregate.error_count, 2);
            assert_eq!(aggregate.avg_first_byte_ms, Some(90.0));
            assert_eq!(aggregate.avg_total_latency_ms, Some(2200.0 / 3.0));
            assert_eq!(aggregate.last_request_at, Some(120_000));
            assert_eq!(aggregate.last_success_at, Some(60_000));
            assert_eq!(aggregate.last_error_at, Some(120_000));
            assert_eq!(
                aggregate.status_code_breakdown,
                vec![
                    ProviderRuntimeStatusCodeCount {
                        status_code: 429,
                        count: 2,
                    },
                    ProviderRuntimeStatusCodeCount {
                        status_code: 500,
                        count: 1,
                    },
                ]
            );
            assert_eq!(
                aggregate.total_cost,
                vec![
                    crate::database::provider_runtime::ProviderRuntimeCostAggregate {
                        currency: "USD".to_string(),
                        amount_nanos: 3_500,
                    }
                ]
            );
        });
    }

    #[test]
    fn provider_runtime_aggregates_include_attempt_only_provider_scope() {
        let context = TestDbContext::new_sqlite("metrics-provider-runtime-attempt-only.sqlite");
        context.run_sync(|| {
            add_request_rollup_delta(&request_rollup_for_provider(
                120_000, 8, 1, 1, 0, 0, 40, 1, 400, 1,
            ))
            .unwrap();
            add_attempt_rollup_delta(&attempt_rollup_for_provider(
                60_000, 7, 1, 0, 1, 0, 0, 900, 1,
            ))
            .unwrap();
            add_http_status_rollup_delta(&http_status_rollup_for_provider(60_000, 7, 500, 1))
                .unwrap();

            let service = MetricsService::new(MetricsConfig {
                request_log_query_fallback_enabled: false,
                ..MetricsConfig::default()
            });
            let aggregates = service
                .provider_runtime_aggregates_in_range(0, 180_000, None)
                .unwrap();

            assert_eq!(aggregates.len(), 2);
            let attempt_only = aggregates
                .iter()
                .find(|item| item.provider_id == 7)
                .expect("attempt-only provider should be visible");
            assert_eq!(attempt_only.request_count, 0);
            assert_eq!(attempt_only.success_count, 0);
            assert_eq!(attempt_only.error_count, 0);
            assert_eq!(attempt_only.avg_total_latency_ms, Some(900.0));
            assert_eq!(
                attempt_only.status_code_breakdown,
                vec![ProviderRuntimeStatusCodeCount {
                    status_code: 500,
                    count: 1,
                }]
            );

            let final_provider = aggregates
                .iter()
                .find(|item| item.provider_id == 8)
                .expect("final provider should remain separate");
            assert_eq!(final_provider.request_count, 1);
            assert_eq!(final_provider.success_count, 1);
            assert_eq!(final_provider.error_count, 0);
            assert!(final_provider.status_code_breakdown.is_empty());
        });
    }

    #[tokio::test]
    async fn provider_runtime_items_include_attempt_only_status_breakdown() {
        let context =
            TestDbContext::new_sqlite("metrics-provider-runtime-attempt-only-list.sqlite");
        context
            .run_async(async {
                Provider::create(&NewProvider {
                    id: 7,
                    provider_key: "provider-7".to_string(),
                    name: "Provider 7".to_string(),
                    endpoint: "https://example.com".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    created_at: 60_000,
                    updated_at: 60_000,
                    provider_type: ProviderType::Openai,
                    provider_api_key_mode: ProviderApiKeyMode::Queue,
                })
                .expect("provider should insert");
                let now_bucket = chrono::Utc::now().timestamp_millis().div_euclid(60_000) * 60_000;
                add_attempt_rollup_delta(&attempt_rollup_for_provider(
                    now_bucket - 60_000,
                    7,
                    1,
                    0,
                    1,
                    0,
                    0,
                    900,
                    1,
                ))
                .unwrap();
                add_http_status_rollup_delta(&http_status_rollup_for_provider(
                    now_bucket - 60_000,
                    7,
                    500,
                    1,
                ))
                .unwrap();
                let app_state = create_test_app_state(context.clone()).await;

                let items = app_state
                    .metrics
                    .build_provider_runtime_items(
                        &app_state,
                        ProviderRuntimeWindow::FifteenMinutes,
                        true,
                    )
                    .await
                    .expect("provider runtime items should build");

                assert_eq!(items.len(), 1);
                assert_eq!(items[0].provider_id, 7);
                assert_eq!(items[0].request_count, 0);
                assert_eq!(items[0].runtime_level, ProviderRuntimeLevel::NoTraffic);
                assert_eq!(items[0].status_code_breakdown.len(), 1);
                assert_eq!(items[0].status_code_breakdown[0].status_code, 500);
                assert_eq!(items[0].status_code_breakdown[0].count, 1);
                assert_eq!(
                    items[0].last_error_summary.as_deref(),
                    Some("Upstream status 500")
                );
            })
            .await;
    }

    fn request_rollup(
        bucket_start_ms: i64,
        request_count: i64,
        success_count: i64,
        error_count: i64,
        cancelled_count: i64,
        first_byte_latency_sum_ms: i64,
        first_byte_latency_count: i64,
        total_latency_sum_ms: i64,
        total_latency_count: i64,
    ) -> MetricRequestRollupMinute {
        MetricRequestRollupMinute {
            bucket_start_ms,
            scope_type: "provider".to_string(),
            scope_id: "7".to_string(),
            scope_label: Some("provider:7".to_string()),
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
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            transform_diagnostic_count: 0,
            transform_diagnostic_lossy_major_count: 0,
            transform_diagnostic_reject_count: 0,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn request_rollup_for_provider(
        bucket_start_ms: i64,
        provider_id: i64,
        request_count: i64,
        success_count: i64,
        error_count: i64,
        cancelled_count: i64,
        first_byte_latency_sum_ms: i64,
        first_byte_latency_count: i64,
        total_latency_sum_ms: i64,
        total_latency_count: i64,
    ) -> MetricRequestRollupMinute {
        let mut rollup = request_rollup(
            bucket_start_ms,
            request_count,
            success_count,
            error_count,
            cancelled_count,
            first_byte_latency_sum_ms,
            first_byte_latency_count,
            total_latency_sum_ms,
            total_latency_count,
        );
        rollup.scope_id = provider_id.to_string();
        rollup.scope_label = Some(format!("provider:{provider_id}"));
        rollup
    }

    #[allow(clippy::too_many_arguments)]
    fn attempt_rollup_for_provider(
        bucket_start_ms: i64,
        provider_id: i64,
        attempt_count: i64,
        success_count: i64,
        error_count: i64,
        first_byte_latency_sum_ms: i64,
        first_byte_latency_count: i64,
        total_latency_sum_ms: i64,
        total_latency_count: i64,
    ) -> MetricAttemptRollupMinute {
        MetricAttemptRollupMinute {
            bucket_start_ms,
            scope_type: "provider".to_string(),
            scope_id: provider_id.to_string(),
            scope_label: Some(format!("provider:{provider_id}")),
            attempt_count,
            success_count,
            error_count,
            skipped_count: 0,
            retry_same_candidate_count: 0,
            fallback_next_candidate_count: 1,
            fail_fast_count: 0,
            first_byte_latency_sum_ms,
            first_byte_latency_count,
            total_latency_sum_ms,
            total_latency_count,
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn http_status_rollup(
        bucket_start_ms: i64,
        status_code: i32,
        count: i64,
    ) -> MetricHttpStatusRollupMinute {
        MetricHttpStatusRollupMinute {
            bucket_start_ms,
            scope_type: "provider".to_string(),
            scope_id: "7".to_string(),
            http_status: status_code,
            count,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn http_status_rollup_for_provider(
        bucket_start_ms: i64,
        provider_id: i64,
        status_code: i32,
        count: i64,
    ) -> MetricHttpStatusRollupMinute {
        let mut rollup = http_status_rollup(bucket_start_ms, status_code, count);
        rollup.scope_id = provider_id.to_string();
        rollup
    }

    fn cost_rollup(
        bucket_start_ms: i64,
        currency: &str,
        amount_nanos: i64,
    ) -> MetricCostRollupMinute {
        MetricCostRollupMinute {
            bucket_start_ms,
            metric_kind: "request".to_string(),
            scope_type: "provider".to_string(),
            scope_id: "7".to_string(),
            currency: currency.to_string(),
            amount_nanos,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn sample_item(
        provider_id: i64,
        provider_name: &str,
        runtime_level: ProviderRuntimeLevel,
        request_count: i64,
        error_count: i64,
        avg_total_latency_ms: Option<f64>,
        last_error_at: Option<i64>,
    ) -> ProviderRuntimeItem {
        ProviderRuntimeItem {
            provider_id,
            provider_key: format!("provider-{}", provider_id),
            provider_name: provider_name.to_string(),
            provider_type: "OPENAI".to_string(),
            is_enabled: true,
            use_proxy: false,
            enabled_model_count: 1,
            enabled_provider_key_count: 1,
            health_status: ProviderRuntimeHealthStatus::Healthy,
            runtime_level,
            consecutive_failures: error_count as u32,
            half_open_probe_in_flight: false,
            opened_at: None,
            last_failure_at: last_error_at,
            last_recovered_at: None,
            last_error: None,
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
            avg_first_byte_ms: None,
            avg_total_latency_ms,
            last_request_at: None,
            last_success_at: None,
            last_error_at,
            last_error_summary: None,
            status_code_breakdown: vec![ProviderRuntimeStatusCodeStat {
                status_code: 500,
                count: error_count,
            }],
            total_cost: vec![ProviderRuntimeCostStat {
                currency: "USD".to_string(),
                amount_nanos: 0,
            }],
            sort_score: 0.0,
        }
    }
}
