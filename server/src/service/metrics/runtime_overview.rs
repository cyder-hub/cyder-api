use std::collections::HashMap;

use super::provider_runtime::{ProviderRuntimeItem, ProviderRuntimeLevel};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DashboardOperationalSignalsReadModel {
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
    let provider_items = aggregate_provider_runtime_items(items);

    let mut degraded_providers = provider_items
        .iter()
        .filter(|item| item.runtime_level == ProviderRuntimeLevel::Degraded)
        .map(signal_item_from_runtime_item)
        .collect::<Vec<_>>();
    sort_provider_signals(&mut degraded_providers);

    let mut top_error_providers = provider_items
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

    let mut top_cost_providers = provider_items
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
        degraded_providers,
        top_error_providers,
        top_cost_providers,
    }
}

pub fn top_providers_from_runtime_items(
    items: &[ProviderRuntimeItem],
) -> Vec<DashboardTopProviderReadItem> {
    let mut top_providers = aggregate_provider_runtime_items(items)
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

fn aggregate_provider_runtime_items(items: &[ProviderRuntimeItem]) -> Vec<ProviderRuntimeItem> {
    let mut providers = HashMap::<i64, ProviderRuntimeItem>::new();
    for item in items {
        if let Some(existing) = providers.get_mut(&item.provider_id) {
            merge_provider_runtime_item(existing, item);
        } else {
            providers.insert(item.provider_id, item.clone());
        }
    }
    providers.into_values().collect()
}

fn merge_provider_runtime_item(existing: &mut ProviderRuntimeItem, incoming: &ProviderRuntimeItem) {
    existing.request_count = existing
        .request_count
        .saturating_add(incoming.request_count);
    existing.success_count = existing
        .success_count
        .saturating_add(incoming.success_count);
    existing.error_count = existing.error_count.saturating_add(incoming.error_count);
    existing.success_rate = (existing.request_count > 0)
        .then_some(existing.success_count as f64 / existing.request_count as f64);

    merge_weighted_average(
        &mut existing.avg_time_to_first_response_body_ms,
        &mut existing.time_to_first_response_body_sample_count,
        incoming.avg_time_to_first_response_body_ms,
        incoming.time_to_first_response_body_sample_count,
    );
    merge_weighted_average(
        &mut existing.avg_ttft_ms,
        &mut existing.ttft_sample_count,
        incoming.avg_ttft_ms,
        incoming.ttft_sample_count,
    );
    merge_weighted_average(
        &mut existing.avg_total_latency_ms,
        &mut existing.total_latency_sample_count,
        incoming.avg_total_latency_ms,
        incoming.total_latency_sample_count,
    );

    existing.runtime_level = worse_runtime_level(existing.runtime_level, incoming.runtime_level);
    existing.runtime_state_backend_degraded |= incoming.runtime_state_backend_degraded;
    if existing.runtime_state_backend_error.is_none() {
        existing.runtime_state_backend_error = incoming.runtime_state_backend_error.clone();
    }

    max_optional(&mut existing.last_request_at, incoming.last_request_at);
    max_optional(&mut existing.last_success_at, incoming.last_success_at);

    let incoming_is_newer_error = incoming.last_error_at > existing.last_error_at;
    let incoming_fills_same_error = incoming.last_error_at == existing.last_error_at
        && existing.last_error_summary.is_none()
        && incoming.last_error_summary.is_some();
    if incoming_is_newer_error || incoming_fills_same_error {
        existing.last_error_at = incoming.last_error_at;
        existing.last_error_summary = incoming.last_error_summary.clone();
    }

    existing.enabled_model_count = existing
        .enabled_model_count
        .max(incoming.enabled_model_count);
    existing.enabled_provider_key_count = existing
        .enabled_provider_key_count
        .max(incoming.enabled_provider_key_count);
    merge_status_code_breakdown(
        &mut existing.status_code_breakdown,
        &incoming.status_code_breakdown,
    );
    merge_cost_stats(&mut existing.total_cost, &incoming.total_cost);
}

fn merge_weighted_average(
    current_average: &mut Option<f64>,
    current_sample_count: &mut i64,
    incoming_average: Option<f64>,
    incoming_sample_count: i64,
) {
    let current_count = (*current_sample_count).max(0);
    let incoming_count = incoming_sample_count.max(0);
    let total_count = current_count.saturating_add(incoming_count);
    if total_count == 0 {
        *current_average = None;
        *current_sample_count = 0;
        return;
    }

    let current_sum = current_average.unwrap_or_default() * current_count as f64;
    let incoming_sum = incoming_average.unwrap_or_default() * incoming_count as f64;
    *current_average = Some((current_sum + incoming_sum) / total_count as f64);
    *current_sample_count = total_count;
}

fn max_optional(current: &mut Option<i64>, incoming: Option<i64>) {
    if incoming > *current {
        *current = incoming;
    }
}

fn merge_status_code_breakdown(
    current: &mut Vec<crate::service::metrics::provider_runtime::ProviderRuntimeStatusCodeStat>,
    incoming: &[crate::service::metrics::provider_runtime::ProviderRuntimeStatusCodeStat],
) {
    for incoming_item in incoming {
        if let Some(current_item) = current
            .iter_mut()
            .find(|item| item.status_code == incoming_item.status_code)
        {
            current_item.count = current_item.count.saturating_add(incoming_item.count);
        } else {
            current.push(incoming_item.clone());
        }
    }
    current.sort_by_key(|item| item.status_code);
}

fn merge_cost_stats(
    current: &mut Vec<crate::service::metrics::provider_runtime::ProviderRuntimeCostStat>,
    incoming: &[crate::service::metrics::provider_runtime::ProviderRuntimeCostStat],
) {
    for incoming_item in incoming {
        if let Some(current_item) = current
            .iter_mut()
            .find(|item| item.currency == incoming_item.currency)
        {
            current_item.amount_nanos = current_item
                .amount_nanos
                .saturating_add(incoming_item.amount_nanos);
        } else {
            current.push(incoming_item.clone());
        }
    }
    current.sort_by(|left, right| left.currency.cmp(&right.currency));
}

fn worse_runtime_level(
    left: ProviderRuntimeLevel,
    right: ProviderRuntimeLevel,
) -> ProviderRuntimeLevel {
    if runtime_level_priority(left) >= runtime_level_priority(right) {
        left
    } else {
        right
    }
}

fn runtime_level_priority(level: ProviderRuntimeLevel) -> u8 {
    match level {
        ProviderRuntimeLevel::Degraded => 3,
        ProviderRuntimeLevel::Healthy => 2,
        ProviderRuntimeLevel::NoTraffic => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::enum_def::UpstreamProfileType;
    use crate::service::metrics::provider_runtime::{
        ProviderRuntimeCostStat, ProviderRuntimeStatusCodeStat,
    };

    #[test]
    fn dashboard_runtime_overview_is_stateless_and_sorted() {
        let items = vec![
            runtime_item(1, ProviderRuntimeLevel::Degraded, 10, 5, 500),
            runtime_item(2, ProviderRuntimeLevel::Degraded, 8, 2, 200),
            runtime_item(3, ProviderRuntimeLevel::Healthy, 30, 1, 900),
        ];

        let read_model = operational_signals_from_runtime_items(&items);

        assert_eq!(
            read_model
                .degraded_providers
                .iter()
                .map(|item| item.provider_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
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

    #[test]
    fn dashboard_provider_views_aggregate_source_rows_before_ranking() {
        let mut degraded_source = runtime_item(7, ProviderRuntimeLevel::Degraded, 10, 4, 500);
        degraded_source.source_id = 71;
        degraded_source.last_error_at = Some(100);
        degraded_source.last_error_summary = Some("source 71 failed".to_string());

        let mut healthy_source = runtime_item(7, ProviderRuntimeLevel::Healthy, 30, 1, 700);
        healthy_source.source_id = 72;
        healthy_source.source_is_default = false;
        healthy_source.avg_total_latency_ms = Some(500.0);
        healthy_source.last_error_at = Some(200);
        healthy_source.last_error_summary = Some("source 72 failed".to_string());

        let signals = operational_signals_from_runtime_items(&[degraded_source, healthy_source]);
        assert_eq!(signals.degraded_providers.len(), 1);
        assert_eq!(signals.degraded_providers[0].provider_id, 7);
        assert_eq!(signals.degraded_providers[0].request_count, 40);
        assert_eq!(signals.degraded_providers[0].error_count, 5);
        assert_eq!(signals.degraded_providers[0].last_error_at, Some(200));
        assert_eq!(
            signals.degraded_providers[0].last_error_summary.as_deref(),
            Some("source 72 failed")
        );
        assert_eq!(signals.top_cost_providers.len(), 1);
        assert_eq!(signals.top_cost_providers[0].request_count, 40);
        assert_eq!(signals.top_cost_providers[0].total_cost["USD"], 1_200);
        assert_eq!(
            signals.top_cost_providers[0].avg_total_latency_ms,
            Some(400.0)
        );

        let top = top_providers_from_runtime_items(&[
            runtime_item(7, ProviderRuntimeLevel::Degraded, 10, 4, 500),
            runtime_item(7, ProviderRuntimeLevel::Healthy, 30, 1, 700),
        ]);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].provider_id, 7);
        assert_eq!(top[0].request_count, 40);
        assert_eq!(top[0].success_count, 35);
        assert_eq!(top[0].error_count, 5);
        assert_eq!(top[0].total_cost["USD"], 1_200);
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
