use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    routing::get,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::controller::BaseError;
use crate::database::model::Model;
use crate::database::provider::{Provider, ProviderApiKey};
use crate::database::provider_runtime::{
    ProviderRuntimeAggregate, get_provider_runtime_aggregates_in_range,
};
use crate::schema::enum_def::ProviderType;
use crate::service::app_state::{AppState, ProviderHealthSnapshot, ProviderHealthStatus};
use crate::service::app_state::{StateRouter, create_state_router};
use crate::utils::HttpResult;

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
    fn duration_ms(self) -> i64 {
        match self {
            ProviderRuntimeWindow::FifteenMinutes => 15 * 60 * 1000,
            ProviderRuntimeWindow::OneHour => 60 * 60 * 1000,
            ProviderRuntimeWindow::SixHours => 6 * 60 * 60 * 1000,
            ProviderRuntimeWindow::TwentyFourHours => 24 * 60 * 60 * 1000,
        }
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
    #[serde(default)]
    pub window: ProviderRuntimeWindow,
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
    #[serde(default)]
    pub window: ProviderRuntimeWindow,
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

fn compute_runtime_level(
    health_status: ProviderHealthStatus,
    request_count: i64,
    error_count: i64,
    avg_total_latency_ms: Option<f64>,
) -> ProviderRuntimeLevel {
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

fn build_last_error_summary(
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

fn matches_status_filter(
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

fn search_matches(provider_name: &str, provider_key: &str, search: &str) -> bool {
    let needle = search.to_ascii_lowercase();
    provider_name.to_ascii_lowercase().contains(&needle)
        || provider_key.to_ascii_lowercase().contains(&needle)
}

async fn build_provider_runtime_items(
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
    let runtime_aggregates = get_provider_runtime_aggregates_in_range(start_time_ms, now, None)?;
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
        let health_snapshot = app_state.get_provider_health_snapshot(provider.id).await;
        let runtime_aggregate = aggregate_map
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

fn sort_provider_runtime_items(
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
            ProviderRuntimeSortField::Latency => compare_f64_option(
                left.avg_total_latency_ms,
                right.avg_total_latency_ms,
            )
            .then_with(|| health_rank(left.runtime_level).cmp(&health_rank(right.runtime_level))),
            ProviderRuntimeSortField::LastErrorAt => {
                compare_i64_option(left.last_error_at, right.last_error_at).then_with(|| {
                    health_rank(left.runtime_level).cmp(&health_rank(right.runtime_level))
                })
            }
            ProviderRuntimeSortField::RequestCount => left
                .request_count
                .cmp(&right.request_count)
                .then_with(|| health_rank(left.runtime_level).cmp(&health_rank(right.runtime_level))),
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

async fn list_provider_runtime(
    State(app_state): State<Arc<AppState>>,
    Query(params): Query<ProviderRuntimeListParams>,
) -> Result<HttpResult<Vec<ProviderRuntimeItem>>, BaseError> {
    let mut items = build_provider_runtime_items(&app_state, params.window, params.only_enabled).await?;

    if let Some(search) = params.search.as_ref().map(|value| value.trim()) {
        if !search.is_empty() {
            items.retain(|item| search_matches(&item.provider_name, &item.provider_key, search));
        }
    }

    items.retain(|item| matches_status_filter(item.runtime_level, params.status));
    sort_provider_runtime_items(&mut items, params.sort, params.direction);

    Ok(HttpResult::new(items))
}

async fn summary_provider_runtime(
    State(app_state): State<Arc<AppState>>,
    Query(params): Query<ProviderRuntimeSummaryParams>,
) -> Result<HttpResult<ProviderRuntimeSummary>, BaseError> {
    let items = build_provider_runtime_items(&app_state, params.window, params.only_enabled).await?;
    let mut summary = ProviderRuntimeSummary {
        total_provider_count: items.len() as i64,
        healthy_count: 0,
        degraded_count: 0,
        half_open_count: 0,
        open_count: 0,
        no_traffic_count: 0,
        window: params.window,
        generated_at: Utc::now().timestamp_millis(),
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

    Ok(HttpResult::new(summary))
}

pub fn create_provider_runtime_router() -> StateRouter {
    create_state_router().nest(
        "/provider/runtime",
        create_state_router()
            .route("/list", get(list_provider_runtime))
            .route("/summary", get(summary_provider_runtime)),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ProviderRuntimeCostStat, ProviderRuntimeHealthStatus, ProviderRuntimeItem,
        ProviderRuntimeLevel, ProviderRuntimeSortField, ProviderRuntimeStatusCodeStat,
        ProviderRuntimeStatusFilter, ProviderRuntimeSummaryParams, ProviderRuntimeWindow,
        SortDirection, build_last_error_summary, compute_runtime_level, sort_provider_runtime_items,
    };
    use crate::database::provider_runtime::{
        ProviderRuntimeAggregate, ProviderRuntimeStatusCodeCount,
    };
    use crate::service::app_state::{ProviderHealthSnapshot, ProviderHealthStatus};

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
        assert_eq!(serde_json::to_string(&SortDirection::Desc).unwrap(), "\"desc\"");
    }

    #[test]
    fn compute_runtime_level_handles_open_half_open_and_no_traffic() {
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Open, 10, 10, Some(20_000.0)),
            ProviderRuntimeLevel::Open
        );
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::HalfOpen, 10, 0, Some(100.0)),
            ProviderRuntimeLevel::HalfOpen
        );
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Healthy, 0, 0, None),
            ProviderRuntimeLevel::NoTraffic
        );
    }

    #[test]
    fn compute_runtime_level_marks_high_error_rate_and_latency_as_degraded() {
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Healthy, 5, 1, Some(500.0)),
            ProviderRuntimeLevel::Degraded
        );
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Healthy, 3, 0, Some(10_000.0)),
            ProviderRuntimeLevel::Degraded
        );
        assert_eq!(
            compute_runtime_level(ProviderHealthStatus::Healthy, 10, 1, Some(500.0)),
            ProviderRuntimeLevel::Healthy
        );
    }

    #[test]
    fn health_sort_prioritizes_open_then_half_open() {
        let mut items = vec![
            sample_item(1, "healthy", ProviderRuntimeLevel::Healthy, 100, 0, Some(100.0), None),
            sample_item(2, "open", ProviderRuntimeLevel::Open, 10, 10, Some(500.0), Some(2_000)),
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

        let ordered_ids = items.iter().map(|item| item.provider_id).collect::<Vec<_>>();
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
    fn provider_runtime_summary_params_default_to_only_enabled() {
        let params: ProviderRuntimeSummaryParams = serde_json::from_str("{}").unwrap();
        assert!(params.only_enabled);
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
