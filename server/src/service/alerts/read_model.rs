use std::collections::{HashMap, HashSet};

use crate::controller::BaseError;
use crate::database::alert::{ALERT_STATUS_ACTIVE, AlertListFilter, list_alerts};
use crate::service::metrics::provider_runtime::{ProviderRuntimeItem, ProviderRuntimeLevel};

use super::lifecycle::is_alert_suppressed;
use super::rules::{
    RULE_COST_HOTSPOT, RULE_HIGH_ERROR_RATE, RULE_PROVIDER_DEGRADED, RULE_PROVIDER_OPEN,
};
use super::service::AlertsService;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DashboardAlertsReadModel {
    pub open_providers: Vec<DashboardProviderAlertReadItem>,
    pub half_open_providers: Vec<DashboardProviderAlertReadItem>,
    pub degraded_providers: Vec<DashboardProviderAlertReadItem>,
    pub top_error_providers: Vec<DashboardProviderAlertReadItem>,
    pub top_cost_providers: Vec<DashboardCostProviderAlertReadItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardProviderAlertReadItem {
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
pub struct DashboardCostProviderAlertReadItem {
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
    pub avg_total_latency_ms: Option<f64>,
}

impl AlertsService {
    pub fn build_dashboard_alerts_from_runtime_items(
        &self,
        items: &[ProviderRuntimeItem],
        now_ms: i64,
    ) -> Result<DashboardAlertsReadModel, BaseError> {
        let suppressed = suppressed_provider_alerts(now_ms)?;
        Ok(dashboard_alerts_from_runtime_items(items, &suppressed))
    }

    pub fn top_providers_from_runtime_items(
        &self,
        items: &[ProviderRuntimeItem],
    ) -> Vec<DashboardTopProviderReadItem> {
        top_providers_from_runtime_items(items)
    }
}

fn suppressed_provider_alerts(now_ms: i64) -> Result<HashSet<(String, i64)>, BaseError> {
    let mut suppressed = HashSet::new();
    for alert in list_alerts(AlertListFilter {
        status: Some(ALERT_STATUS_ACTIVE.to_string()),
        ..AlertListFilter::default()
    })? {
        if alert.scope_type != "provider" || !is_alert_suppressed(&alert, now_ms) {
            continue;
        }
        if let Ok(provider_id) = alert.scope_id.parse::<i64>() {
            suppressed.insert((alert.rule_key, provider_id));
        }
    }
    Ok(suppressed)
}

fn dashboard_alerts_from_runtime_items(
    items: &[ProviderRuntimeItem],
    suppressed: &HashSet<(String, i64)>,
) -> DashboardAlertsReadModel {
    let mut open_providers = items
        .iter()
        .filter(|item| item.runtime_level == ProviderRuntimeLevel::Open)
        .filter(|item| !is_suppressed(suppressed, RULE_PROVIDER_OPEN, item.provider_id))
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
        .filter(|item| !is_suppressed(suppressed, RULE_PROVIDER_DEGRADED, item.provider_id))
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
        .filter(|item| !is_suppressed(suppressed, RULE_HIGH_ERROR_RATE, item.provider_id))
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
        .filter(|item| !is_suppressed(suppressed, RULE_COST_HOTSPOT, item.provider_id))
        .map(cost_provider_alert_item_from_runtime_item)
        .collect::<Vec<_>>();
    top_cost_providers.sort_by(|left, right| {
        total_cost_rank_value(&right.total_cost)
            .cmp(&total_cost_rank_value(&left.total_cost))
            .then_with(|| right.request_count.cmp(&left.request_count))
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });
    top_cost_providers.truncate(5);

    DashboardAlertsReadModel {
        open_providers,
        half_open_providers,
        degraded_providers,
        top_error_providers,
        top_cost_providers,
    }
}

fn is_suppressed(suppressed: &HashSet<(String, i64)>, rule_key: &str, provider_id: i64) -> bool {
    suppressed.contains(&(rule_key.to_string(), provider_id))
}

fn alert_item_from_runtime_item(item: &ProviderRuntimeItem) -> DashboardProviderAlertReadItem {
    DashboardProviderAlertReadItem {
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

fn cost_provider_alert_item_from_runtime_item(
    item: &ProviderRuntimeItem,
) -> DashboardCostProviderAlertReadItem {
    DashboardCostProviderAlertReadItem {
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
        avg_total_latency_ms: item.avg_total_latency_ms,
    }
}

fn top_providers_from_runtime_items(
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

fn total_cost_rank_value(cost: &HashMap<String, i64>) -> i64 {
    cost.values().copied().sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AlertsConfig;
    use crate::database::TestDbContext;
    use crate::database::alert::{AlertFireRecord, fire_alert, suppress_alert};
    use crate::service::alerts::AlertsService;
    use crate::service::metrics::provider_runtime::{
        ProviderRuntimeCostStat, ProviderRuntimeHealthStatus, ProviderRuntimeStatusCodeStat,
    };

    #[test]
    fn read_model_keeps_legacy_groups_and_hides_suppressed_alerts() {
        let context = TestDbContext::new_sqlite("alert-read-model.sqlite");
        context.run_sync(|| {
            let alert = fire_alert(&fire_record(RULE_PROVIDER_OPEN, 1), 1_000).unwrap();
            suppress_alert(alert.id, 3_000, Some("maintenance".to_string()), 1_100).unwrap();

            let service = AlertsService::new(AlertsConfig::default());
            let items = vec![
                runtime_item(1, ProviderRuntimeLevel::Open, 10, 5, 500),
                runtime_item(2, ProviderRuntimeLevel::HalfOpen, 8, 2, 200),
                runtime_item(3, ProviderRuntimeLevel::Healthy, 30, 1, 900),
            ];
            let read_model = service
                .build_dashboard_alerts_from_runtime_items(&items, 2_000)
                .unwrap();

            assert!(read_model.open_providers.is_empty());
            assert_eq!(read_model.half_open_providers[0].provider_id, 2);
            assert_eq!(
                read_model
                    .top_cost_providers
                    .iter()
                    .map(|item| item.provider_id)
                    .collect::<Vec<_>>(),
                vec![3, 1, 2]
            );
        });
    }

    fn fire_record(rule_key: &str, provider_id: i64) -> AlertFireRecord {
        AlertFireRecord {
            fingerprint: format!("{rule_key}:provider:{provider_id}"),
            rule_key: rule_key.to_string(),
            severity: "critical".to_string(),
            scope_type: "provider".to_string(),
            scope_id: provider_id.to_string(),
            title: "Provider alert".to_string(),
            summary: "Provider alert".to_string(),
            details_json: "{}".to_string(),
            metrics_snapshot_json: None,
        }
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
            avg_first_byte_ms: Some(100.0),
            avg_total_latency_ms: Some(300.0),
            last_request_at: None,
            last_success_at: None,
            last_error_at: None,
            last_error_summary: None,
            status_code_breakdown: Vec::<ProviderRuntimeStatusCodeStat>::new(),
            total_cost: vec![ProviderRuntimeCostStat {
                currency: "USD".to_string(),
                amount_nanos: cost,
            }],
            sort_score: 0.0,
        }
    }
}
