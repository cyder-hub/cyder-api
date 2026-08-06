use std::collections::HashMap;

use super::provider_runtime::{ProviderRuntimeItem, ProviderRuntimeLevel};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DashboardOperationalSignalsReadModel {
    pub open_providers: Vec<DashboardProviderSignalReadItem>,
    pub half_open_providers: Vec<DashboardProviderSignalReadItem>,
    pub degraded_providers: Vec<DashboardProviderSignalReadItem>,
    pub top_error_providers: Vec<DashboardProviderSignalReadItem>,
    pub top_cost_providers: Vec<DashboardCostProviderReadItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardProviderSignalReadItem {
    pub provider_id: i64,
    pub provider_key: String,
    pub provider_name: String,
    pub runtime_level: ProviderRuntimeLevel,
    pub request_count: i64,
    pub error_count: i64,
    pub success_rate: Option<f64>,
    pub avg_total_latency_ms: Option<f64>,
    pub last_error_at: Option<i64>,
    pub last_error_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardCostProviderReadItem {
    pub provider_id: i64,
    pub provider_key: String,
    pub provider_name: String,
    pub request_count: i64,
    pub success_rate: Option<f64>,
    pub avg_total_latency_ms: Option<f64>,
    pub total_cost: HashMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardTopProviderReadItem {
    pub provider_id: i64,
    pub provider_key: String,
    pub provider_name: String,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub success_rate: Option<f64>,
    pub total_cost: HashMap<String, i64>,
    pub avg_time_to_first_response_body_ms: Option<f64>,
    pub time_to_first_response_body_sample_count: i64,
    pub avg_ttft_ms: Option<f64>,
    pub ttft_sample_count: i64,
    pub avg_total_latency_ms: Option<f64>,
    pub total_latency_sample_count: i64,
}

pub fn operational_signals_from_runtime_items(
    items: &[ProviderRuntimeItem],
) -> DashboardOperationalSignalsReadModel {
    let mut open_providers = items
        .iter()
        .filter(|item| item.runtime_level == ProviderRuntimeLevel::Open)
        .map(signal_item_from_runtime_item)
        .collect::<Vec<_>>();
    sort_provider_signals(&mut open_providers);

    let mut half_open_providers = items
        .iter()
        .filter(|item| item.runtime_level == ProviderRuntimeLevel::HalfOpen)
        .map(signal_item_from_runtime_item)
        .collect::<Vec<_>>();
    sort_provider_signals(&mut half_open_providers);

    let mut degraded_providers = items
        .iter()
        .filter(|item| item.runtime_level == ProviderRuntimeLevel::Degraded)
        .map(signal_item_from_runtime_item)
        .collect::<Vec<_>>();
    sort_provider_signals(&mut degraded_providers);

    let mut top_error_providers = items
        .iter()
        .filter(|item| item.error_count > 0)
        .map(signal_item_from_runtime_item)
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
        .map(cost_provider_item_from_runtime_item)
        .collect::<Vec<_>>();
    top_cost_providers.sort_by(|left, right| {
        total_cost_rank_value(&right.total_cost)
            .cmp(&total_cost_rank_value(&left.total_cost))
            .then_with(|| right.request_count.cmp(&left.request_count))
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });
    top_cost_providers.truncate(5);

    DashboardOperationalSignalsReadModel {
        open_providers,
        half_open_providers,
        degraded_providers,
        top_error_providers,
        top_cost_providers,
    }
}

pub fn top_providers_from_runtime_items(
    items: &[ProviderRuntimeItem],
) -> Vec<DashboardTopProviderReadItem> {
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

fn sort_provider_signals(items: &mut [DashboardProviderSignalReadItem]) {
    items.sort_by(|left, right| {
        right
            .error_count
            .cmp(&left.error_count)
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });
}

fn signal_item_from_runtime_item(item: &ProviderRuntimeItem) -> DashboardProviderSignalReadItem {
    DashboardProviderSignalReadItem {
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

fn cost_provider_item_from_runtime_item(
    item: &ProviderRuntimeItem,
) -> DashboardCostProviderReadItem {
    DashboardCostProviderReadItem {
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

fn top_provider_item_from_runtime_item(item: &ProviderRuntimeItem) -> DashboardTopProviderReadItem {
    DashboardTopProviderReadItem {
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
        avg_time_to_first_response_body_ms: item.avg_time_to_first_response_body_ms,
        time_to_first_response_body_sample_count: item.time_to_first_response_body_sample_count,
        avg_ttft_ms: item.avg_ttft_ms,
        ttft_sample_count: item.ttft_sample_count,
        avg_total_latency_ms: item.avg_total_latency_ms,
        total_latency_sample_count: item.total_latency_sample_count,
    }
}

fn total_cost_rank_value(cost: &HashMap<String, i64>) -> i64 {
    cost.values().copied().sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::enum_def::UpstreamProfileType;
    use crate::service::metrics::provider_runtime::{
        ProviderRuntimeCostStat, ProviderRuntimeHealthStatus, ProviderRuntimeStatusCodeStat,
    };

    #[test]
    fn dashboard_runtime_overview_is_stateless_and_sorted() {
        let items = vec![
            runtime_item(1, ProviderRuntimeLevel::Open, 10, 5, 500),
            runtime_item(2, ProviderRuntimeLevel::HalfOpen, 8, 2, 200),
            runtime_item(3, ProviderRuntimeLevel::Healthy, 30, 1, 900),
        ];

        let read_model = operational_signals_from_runtime_items(&items);

        assert_eq!(read_model.open_providers[0].provider_id, 1);
        assert_eq!(read_model.half_open_providers[0].provider_id, 2);
        assert_eq!(
            read_model
                .top_cost_providers
                .iter()
                .map(|item| item.provider_id)
                .collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
        assert_eq!(
            top_providers_from_runtime_items(&items)
                .iter()
                .map(|item| item.provider_id)
                .collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
    }

    fn runtime_item(
        provider_id: i64,
        runtime_level: ProviderRuntimeLevel,
        request_count: i64,
        error_count: i64,
        cost: i64,
    ) -> ProviderRuntimeItem {
        ProviderRuntimeItem {
            provider_id,
            provider_key: format!("p{provider_id}"),
            provider_name: format!("Provider {provider_id}"),
            is_enabled: true,
            source_id: provider_id * 10 + 1,
            source_key: "primary".to_string(),
            source_profile_type: UpstreamProfileType::Openai,
            source_endpoint: "https://api.example.com/v1".to_string(),
            source_use_proxy: false,
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
            runtime_state_backend_degraded: false,
            runtime_state_backend_error: None,
            request_count,
            success_count: request_count.saturating_sub(error_count),
            error_count,
            success_rate: (request_count > 0)
                .then_some((request_count - error_count) as f64 / request_count as f64),
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
                amount_nanos: cost,
            }],
        }
    }
}
