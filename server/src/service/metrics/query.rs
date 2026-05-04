use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, TimeZone, Utc};

use crate::controller::BaseError;
use crate::database::api_key::ApiKey;
use crate::database::metrics::{
    MetricAttemptWindowAggregate, MetricCostAggregate, MetricCostRollupMinute,
    MetricHttpStatusCount, MetricRequestRollupMinute, MetricRequestWindowAggregate,
    list_cost_rollup_minutes, list_request_rollup_minutes, query_attempt_window_aggregates,
    query_cost_window_aggregates, query_http_status_breakdown, query_request_window_aggregates,
};
use crate::database::model::{Model, ModelSummaryItem};
use crate::database::provider::{Provider, ProviderSummaryItem};
use crate::database::stat::{
    DashboardTopModelItem, UsageStatsGroupBy, UsageStatsQueryItem, get_dashboard_cost_alert_models,
    get_dashboard_top_models, get_usage_stats_aggregates, start_of_today_timestamp_ms,
};

use super::service::MetricsService;
use super::types::MetricsTimeseriesPoint;

impl MetricsService {
    pub fn query_request_window_metrics(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        scope_type_filter: Option<&str>,
        scope_id_filter: Option<&str>,
    ) -> Result<Vec<MetricRequestWindowAggregate>, BaseError> {
        query_request_window_aggregates(
            start_time_ms,
            end_time_ms,
            scope_type_filter,
            scope_id_filter,
        )
    }

    pub fn query_attempt_window_metrics(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        scope_type_filter: Option<&str>,
        scope_id_filter: Option<&str>,
    ) -> Result<Vec<MetricAttemptWindowAggregate>, BaseError> {
        query_attempt_window_aggregates(
            start_time_ms,
            end_time_ms,
            scope_type_filter,
            scope_id_filter,
        )
    }

    pub fn query_http_status_breakdown(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        scope_type_filter: &str,
        scope_id_filter: &str,
    ) -> Result<Vec<MetricHttpStatusCount>, BaseError> {
        query_http_status_breakdown(
            start_time_ms,
            end_time_ms,
            scope_type_filter,
            scope_id_filter,
        )
    }

    pub fn query_cost_window_metrics(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        metric_kind_filter: &str,
        scope_type_filter: &str,
        scope_id_filter: &str,
    ) -> Result<Vec<MetricCostAggregate>, BaseError> {
        query_cost_window_aggregates(
            start_time_ms,
            end_time_ms,
            metric_kind_filter,
            scope_type_filter,
            scope_id_filter,
        )
    }

    pub fn query_timeseries(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        interval: &str,
        scope_type_filter: Option<&str>,
        scope_id_filter: Option<&str>,
    ) -> Result<Vec<MetricsTimeseriesPoint>, BaseError> {
        let rows = list_request_rollup_minutes(
            start_time_ms,
            end_time_ms,
            scope_type_filter,
            scope_id_filter,
        )?;
        let mut points = BTreeMap::<(i64, String, String), MetricsTimeseriesPoint>::new();
        for row in rows {
            let bucket_start_ms = interval_bucket_start(row.bucket_start_ms, interval)?;
            let key = (
                bucket_start_ms,
                row.scope_type.clone(),
                row.scope_id.clone(),
            );
            let entry = points.entry(key).or_insert_with(|| MetricsTimeseriesPoint {
                bucket_start_ms,
                scope_type: row.scope_type.clone(),
                scope_id: row.scope_id.clone(),
                request_count: 0,
                success_count: 0,
                error_count: 0,
                total_tokens: 0,
            });
            entry.request_count += row.request_count;
            entry.success_count += row.success_count;
            entry.error_count += row.error_count + row.cancelled_count;
            entry.total_tokens += row.total_tokens;
        }
        Ok(points.into_values().collect())
    }

    pub fn dashboard_top_models(
        &self,
        limit: usize,
        timezone: Option<&str>,
    ) -> Result<Vec<DashboardTopModelItem>, BaseError> {
        if !self.config().enabled {
            return self.dashboard_top_models_fallback(limit, timezone, "metrics_disabled");
        }

        let start_time_ms = start_of_today_timestamp_ms(timezone)?;
        let end_time_ms = Utc::now().timestamp_millis();
        let request_aggregates = query_request_window_aggregates(
            start_time_ms,
            end_time_ms,
            Some("provider_model"),
            None,
        )?;
        if request_aggregates.is_empty() {
            return self.dashboard_top_models_fallback(limit, timezone, "rollup_empty");
        }

        let model_map = model_summary_map()?;
        let provider_map = provider_summary_map()?;
        let mut items = request_aggregates
            .into_iter()
            .filter(|item| item.request_count > 0)
            .filter_map(|item| {
                let (provider_id, model_id) = parse_provider_model_scope(&item.scope_id)?;
                let model = model_map.get(&model_id);
                let provider = provider_map.get(&provider_id);
                let total_cost = query_cost_window_aggregates(
                    start_time_ms,
                    end_time_ms,
                    "request",
                    "provider_model",
                    &item.scope_id,
                )
                .ok()?
                .into_iter()
                .map(|cost| (cost.currency, cost.amount_nanos))
                .collect::<HashMap<_, _>>();
                Some(DashboardTopModelItem {
                    provider_id,
                    provider_key: model
                        .map(|item| item.provider_key.clone())
                        .or_else(|| provider.map(|item| item.provider_key.clone()))
                        .unwrap_or_else(|| provider_id.to_string()),
                    model_id,
                    model_name: model
                        .map(|item| item.model_name.clone())
                        .or_else(|| item.scope_label.clone())
                        .unwrap_or_else(|| model_id.to_string()),
                    real_model_name: model.and_then(|item| item.real_model_name.clone()),
                    request_count: item.request_count,
                    total_tokens: item.total_tokens,
                    total_cost,
                })
            })
            .collect::<Vec<_>>();

        items.sort_by(|left, right| {
            right
                .request_count
                .cmp(&left.request_count)
                .then_with(|| left.provider_id.cmp(&right.provider_id))
                .then_with(|| left.model_id.cmp(&right.model_id))
        });
        items.truncate(limit);
        Ok(items)
    }

    pub fn dashboard_cost_alert_models(
        &self,
        limit_per_currency: usize,
        timezone: Option<&str>,
    ) -> Result<Vec<DashboardTopModelItem>, BaseError> {
        if !self.config().enabled {
            return self.dashboard_cost_alert_models_fallback(
                limit_per_currency,
                timezone,
                "metrics_disabled",
            );
        }

        let start_time_ms = start_of_today_timestamp_ms(timezone)?;
        let end_time_ms = Utc::now().timestamp_millis();
        let request_aggregates = query_request_window_aggregates(
            start_time_ms,
            end_time_ms,
            Some("provider_model"),
            None,
        )?;
        if request_aggregates.is_empty() {
            return self.dashboard_cost_alert_models_fallback(
                limit_per_currency,
                timezone,
                "rollup_empty",
            );
        }

        let model_map = model_summary_map()?;
        let provider_map = provider_summary_map()?;
        let requests_by_scope = request_aggregates
            .into_iter()
            .map(|item| (item.scope_id.clone(), item))
            .collect::<HashMap<_, _>>();
        let mut cost_by_scope_currency = BTreeMap::<(String, String), i64>::new();
        for cost in list_cost_rollup_minutes(
            start_time_ms,
            end_time_ms,
            "request",
            Some("provider_model"),
            None,
        )? {
            if parse_provider_model_scope(&cost.scope_id).is_none() {
                continue;
            }
            *cost_by_scope_currency
                .entry((cost.scope_id, cost.currency))
                .or_insert(0) += cost.amount_nanos;
        }

        let mut by_currency = BTreeMap::<String, Vec<DashboardTopModelItem>>::new();
        for ((scope_id, currency), amount_nanos) in cost_by_scope_currency {
            let Some((provider_id, model_id)) = parse_provider_model_scope(&scope_id) else {
                continue;
            };
            let model = model_map.get(&model_id);
            let provider = provider_map.get(&provider_id);
            let request = requests_by_scope.get(&scope_id);
            let mut total_cost = HashMap::new();
            total_cost.insert(currency.clone(), amount_nanos);
            by_currency
                .entry(currency)
                .or_default()
                .push(DashboardTopModelItem {
                    provider_id,
                    provider_key: model
                        .map(|item| item.provider_key.clone())
                        .or_else(|| provider.map(|item| item.provider_key.clone()))
                        .unwrap_or_else(|| provider_id.to_string()),
                    model_id,
                    model_name: model
                        .map(|item| item.model_name.clone())
                        .unwrap_or_else(|| model_id.to_string()),
                    real_model_name: model.and_then(|item| item.real_model_name.clone()),
                    request_count: request.map_or(0, |item| item.request_count),
                    total_tokens: request.map_or(0, |item| item.total_tokens),
                    total_cost,
                });
        }

        let mut result = Vec::new();
        for (_currency, mut items) in by_currency {
            items.sort_by(|left, right| {
                let left_cost = left.total_cost.values().copied().sum::<i64>();
                let right_cost = right.total_cost.values().copied().sum::<i64>();
                right_cost
                    .cmp(&left_cost)
                    .then_with(|| right.request_count.cmp(&left.request_count))
                    .then_with(|| left.provider_id.cmp(&right.provider_id))
                    .then_with(|| left.model_id.cmp(&right.model_id))
            });
            items.truncate(limit_per_currency);
            result.extend(items);
        }
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn usage_stats_aggregates(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        interval: &str,
        group_by: UsageStatsGroupBy,
        provider_id_filter: Option<i64>,
        model_id_filter: Option<i64>,
        api_key_id_filter: Option<i64>,
        provider_api_key_id_filter: Option<i64>,
    ) -> Result<Vec<UsageStatsQueryItem>, BaseError> {
        if !self.config().enabled {
            return self.usage_stats_fallback(
                start_time_ms,
                end_time_ms,
                interval,
                group_by,
                provider_id_filter,
                model_id_filter,
                api_key_id_filter,
                provider_api_key_id_filter,
                "metrics_disabled",
            );
        }

        let Some(source) = UsageRollupSource::from_filters(
            group_by,
            provider_id_filter,
            model_id_filter,
            api_key_id_filter,
            provider_api_key_id_filter,
        ) else {
            return self.usage_stats_fallback(
                start_time_ms,
                end_time_ms,
                interval,
                group_by,
                provider_id_filter,
                model_id_filter,
                api_key_id_filter,
                provider_api_key_id_filter,
                "unsupported_filter_combination",
            );
        };

        let rows = list_request_rollup_minutes(
            start_time_ms,
            end_time_ms,
            Some(source.scope_type),
            source.scope_id_filter.as_deref(),
        )?;
        if rows.is_empty() {
            return self.usage_stats_fallback(
                start_time_ms,
                end_time_ms,
                interval,
                group_by,
                provider_id_filter,
                model_id_filter,
                api_key_id_filter,
                provider_api_key_id_filter,
                "rollup_empty",
            );
        }

        let metadata = UsageMetadata::load()?;
        let mut items = HashMap::<(i64, i64), UsageStatsQueryItem>::new();
        let mut seen_scope_ids = HashSet::<String>::new();
        for row in rows {
            if !source.matches_row(&row) {
                continue;
            }
            let Some(identity) = source.identity_for_row(&row, &metadata) else {
                continue;
            };
            let time_bucket = interval_bucket_start(row.bucket_start_ms, interval)?;
            let key = (time_bucket, identity.group_id);
            seen_scope_ids.insert(row.scope_id.clone());
            let entry = items.entry(key).or_insert_with(|| UsageStatsQueryItem {
                time: time_bucket,
                group_id: identity.group_id,
                provider_id: identity.provider_id,
                model_id: identity.model_id,
                api_key_id: identity.api_key_id,
                provider_key: identity.provider_key.clone(),
                model_name: identity.model_name.clone(),
                real_model_name: identity.real_model_name.clone(),
                api_key_name: identity.api_key_name.clone(),
                group_label: identity.group_label.clone(),
                group_detail: identity.group_detail.clone(),
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_reasoning_tokens: 0,
                total_tokens: 0,
                request_count: 0,
                success_count: 0,
                error_count: 0,
                success_rate: None,
                avg_total_latency_ms: None,
                latency_sample_count: 0,
                total_cost: HashMap::new(),
            });
            entry.total_input_tokens += row.input_tokens;
            entry.total_output_tokens += row.output_tokens;
            entry.total_reasoning_tokens += row.reasoning_tokens;
            entry.total_tokens += row.total_tokens;
            entry.request_count += row.request_count;
            entry.success_count += row.success_count;
            entry.error_count += row.error_count + row.cancelled_count;
            let previous_latency_sum =
                entry.avg_total_latency_ms.unwrap_or(0.0) * entry.latency_sample_count as f64;
            entry.latency_sample_count += row.total_latency_count;
            entry.avg_total_latency_ms = if entry.latency_sample_count > 0 {
                Some(
                    (previous_latency_sum + row.total_latency_sum_ms as f64)
                        / entry.latency_sample_count as f64,
                )
            } else {
                None
            };
            entry.success_rate = if entry.request_count > 0 {
                Some(entry.success_count as f64 / entry.request_count as f64)
            } else {
                None
            };
        }

        for cost in list_cost_rollup_minutes(
            start_time_ms,
            end_time_ms,
            "request",
            Some(source.scope_type),
            source.scope_id_filter.as_deref(),
        )? {
            if !seen_scope_ids.contains(&cost.scope_id) || !source.matches_cost_row(&cost) {
                continue;
            }
            let Some(identity) = source.identity_for_cost_row(&cost, &metadata) else {
                continue;
            };
            let time_bucket = interval_bucket_start(cost.bucket_start_ms, interval)?;
            if let Some(item) = items.get_mut(&(time_bucket, identity.group_id)) {
                *item.total_cost.entry(cost.currency).or_insert(0) += cost.amount_nanos;
            }
        }

        let mut result = items.into_values().collect::<Vec<_>>();
        result.sort_by(|left, right| {
            left.time
                .cmp(&right.time)
                .then_with(|| left.group_label.cmp(&right.group_label))
                .then_with(|| left.group_id.cmp(&right.group_id))
        });
        Ok(result)
    }

    fn dashboard_top_models_fallback(
        &self,
        limit: usize,
        timezone: Option<&str>,
        reason: &'static str,
    ) -> Result<Vec<DashboardTopModelItem>, BaseError> {
        if !self.config().request_log_query_fallback_enabled {
            return Ok(Vec::new());
        }
        let rows = get_dashboard_top_models(limit, timezone)?;
        if !rows.is_empty() {
            crate::warn_event!(
                "metrics.dashboard_top_models_request_log_fallback",
                reason = reason,
                row_count = rows.len()
            );
        }
        Ok(rows)
    }

    fn dashboard_cost_alert_models_fallback(
        &self,
        limit: usize,
        timezone: Option<&str>,
        reason: &'static str,
    ) -> Result<Vec<DashboardTopModelItem>, BaseError> {
        if !self.config().request_log_query_fallback_enabled {
            return Ok(Vec::new());
        }
        let rows = get_dashboard_cost_alert_models(limit, timezone)?;
        if !rows.is_empty() {
            crate::warn_event!(
                "metrics.dashboard_cost_alert_models_request_log_fallback",
                reason = reason,
                row_count = rows.len()
            );
        }
        Ok(rows)
    }

    #[allow(clippy::too_many_arguments)]
    fn usage_stats_fallback(
        &self,
        start_time_ms: i64,
        end_time_ms: i64,
        interval: &str,
        group_by: UsageStatsGroupBy,
        provider_id_filter: Option<i64>,
        model_id_filter: Option<i64>,
        api_key_id_filter: Option<i64>,
        provider_api_key_id_filter: Option<i64>,
        reason: &'static str,
    ) -> Result<Vec<UsageStatsQueryItem>, BaseError> {
        if !self.config().request_log_query_fallback_enabled {
            return Ok(Vec::new());
        }
        let rows = get_usage_stats_aggregates(
            start_time_ms,
            end_time_ms,
            interval,
            group_by,
            provider_id_filter,
            model_id_filter,
            api_key_id_filter,
            provider_api_key_id_filter,
        )?;
        if !rows.is_empty() {
            crate::warn_event!(
                "metrics.usage_stats_request_log_fallback",
                reason = reason,
                row_count = rows.len()
            );
        }
        Ok(rows)
    }
}

#[derive(Debug, Clone)]
struct UsageRollupSource {
    scope_type: &'static str,
    scope_id_filter: Option<String>,
    group_by: UsageStatsGroupBy,
    provider_id_filter: Option<i64>,
    model_id_filter: Option<i64>,
    api_key_id_filter: Option<i64>,
}

impl UsageRollupSource {
    fn from_filters(
        group_by: UsageStatsGroupBy,
        provider_id_filter: Option<i64>,
        model_id_filter: Option<i64>,
        api_key_id_filter: Option<i64>,
        provider_api_key_id_filter: Option<i64>,
    ) -> Option<Self> {
        if provider_api_key_id_filter.is_some() {
            return None;
        }
        match group_by {
            UsageStatsGroupBy::Provider => {
                if api_key_id_filter.is_some() {
                    return None;
                }
                if let Some(model_id) = model_id_filter {
                    Some(Self {
                        scope_type: "provider_model",
                        scope_id_filter: None,
                        group_by,
                        provider_id_filter,
                        model_id_filter: Some(model_id),
                        api_key_id_filter: None,
                    })
                } else {
                    Some(Self {
                        scope_type: "provider",
                        scope_id_filter: provider_id_filter.map(|id| id.to_string()),
                        group_by,
                        provider_id_filter,
                        model_id_filter: None,
                        api_key_id_filter: None,
                    })
                }
            }
            UsageStatsGroupBy::Model => {
                if api_key_id_filter.is_some() {
                    return None;
                }
                Some(Self {
                    scope_type: "provider_model",
                    scope_id_filter: match (provider_id_filter, model_id_filter) {
                        (Some(provider_id), Some(model_id)) => {
                            Some(format!("{provider_id}:{model_id}"))
                        }
                        _ => None,
                    },
                    group_by,
                    provider_id_filter,
                    model_id_filter,
                    api_key_id_filter: None,
                })
            }
            UsageStatsGroupBy::ApiKey => {
                if provider_id_filter.is_some() || model_id_filter.is_some() {
                    return None;
                }
                Some(Self {
                    scope_type: "api_key",
                    scope_id_filter: api_key_id_filter.map(|id| id.to_string()),
                    group_by,
                    provider_id_filter: None,
                    model_id_filter: None,
                    api_key_id_filter,
                })
            }
        }
    }

    fn matches_row(&self, row: &MetricRequestRollupMinute) -> bool {
        self.matches_scope_id(&row.scope_id)
    }

    fn matches_cost_row(&self, row: &MetricCostRollupMinute) -> bool {
        self.matches_scope_id(&row.scope_id)
    }

    fn matches_scope_id(&self, scope_id: &str) -> bool {
        match self.group_by {
            UsageStatsGroupBy::Provider if self.scope_type == "provider_model" => {
                let Some((provider_id, model_id)) = parse_provider_model_scope(scope_id) else {
                    return false;
                };
                self.provider_id_filter.is_none_or(|id| id == provider_id)
                    && self.model_id_filter.is_none_or(|id| id == model_id)
            }
            UsageStatsGroupBy::Model => {
                let Some((provider_id, model_id)) = parse_provider_model_scope(scope_id) else {
                    return false;
                };
                self.provider_id_filter.is_none_or(|id| id == provider_id)
                    && self.model_id_filter.is_none_or(|id| id == model_id)
            }
            UsageStatsGroupBy::ApiKey => self
                .api_key_id_filter
                .is_none_or(|id| scope_id == id.to_string()),
            UsageStatsGroupBy::Provider => self
                .provider_id_filter
                .is_none_or(|id| scope_id == id.to_string()),
        }
    }

    fn identity_for_row(
        &self,
        row: &MetricRequestRollupMinute,
        metadata: &UsageMetadata,
    ) -> Option<UsageIdentity> {
        self.identity_for_scope_id(&row.scope_id, metadata)
    }

    fn identity_for_cost_row(
        &self,
        row: &MetricCostRollupMinute,
        metadata: &UsageMetadata,
    ) -> Option<UsageIdentity> {
        self.identity_for_scope_id(&row.scope_id, metadata)
    }

    fn identity_for_scope_id(
        &self,
        scope_id: &str,
        metadata: &UsageMetadata,
    ) -> Option<UsageIdentity> {
        match self.group_by {
            UsageStatsGroupBy::Provider => {
                let provider_id = if self.scope_type == "provider_model" {
                    parse_provider_model_scope(scope_id)?.0
                } else {
                    scope_id.parse::<i64>().ok()?
                };
                let provider = metadata.providers.get(&provider_id);
                Some(UsageIdentity {
                    group_id: provider_id,
                    provider_id: Some(provider_id),
                    model_id: None,
                    api_key_id: None,
                    provider_key: provider.map(|item| item.provider_key.clone()),
                    model_name: None,
                    real_model_name: None,
                    api_key_name: None,
                    group_label: provider
                        .map(|item| item.provider_key.clone())
                        .unwrap_or_else(|| provider_id.to_string()),
                    group_detail: provider.map(|item| item.name.clone()),
                })
            }
            UsageStatsGroupBy::Model => {
                let (provider_id, model_id) = parse_provider_model_scope(scope_id)?;
                let model = metadata.models.get(&model_id);
                let provider = metadata.providers.get(&provider_id);
                let provider_key = model
                    .map(|item| item.provider_key.clone())
                    .or_else(|| provider.map(|item| item.provider_key.clone()))
                    .unwrap_or_else(|| provider_id.to_string());
                let model_name = model
                    .map(|item| item.model_name.clone())
                    .unwrap_or_else(|| model_id.to_string());
                Some(UsageIdentity {
                    group_id: model_id,
                    provider_id: Some(provider_id),
                    model_id: Some(model_id),
                    api_key_id: None,
                    provider_key: Some(provider_key.clone()),
                    model_name: Some(model_name.clone()),
                    real_model_name: model.and_then(|item| item.real_model_name.clone()),
                    api_key_name: None,
                    group_label: format!("{provider_key}/{model_name}"),
                    group_detail: model.and_then(|item| item.real_model_name.clone()),
                })
            }
            UsageStatsGroupBy::ApiKey => {
                let api_key_id = scope_id.parse::<i64>().ok()?;
                let api_key = metadata.api_keys.get(&api_key_id);
                Some(UsageIdentity {
                    group_id: api_key_id,
                    provider_id: None,
                    model_id: None,
                    api_key_id: Some(api_key_id),
                    provider_key: None,
                    model_name: None,
                    real_model_name: None,
                    api_key_name: api_key.map(|item| item.name.clone()),
                    group_label: api_key
                        .map(|item| item.name.clone())
                        .unwrap_or_else(|| api_key_id.to_string()),
                    group_detail: api_key
                        .map(|item| format!("{}***{}", item.key_prefix, item.key_last4)),
                })
            }
        }
    }
}

struct UsageMetadata {
    providers: HashMap<i64, ProviderSummaryItem>,
    models: HashMap<i64, ModelSummaryItem>,
    api_keys: HashMap<i64, crate::database::api_key::ApiKeySummary>,
}

impl UsageMetadata {
    fn load() -> Result<Self, BaseError> {
        Ok(Self {
            providers: Provider::list_summary()?
                .into_iter()
                .map(|item| (item.id, item))
                .collect(),
            models: model_summary_map()?,
            api_keys: ApiKey::list_summary()?
                .into_iter()
                .map(|item| (item.id, item))
                .collect(),
        })
    }
}

struct UsageIdentity {
    group_id: i64,
    provider_id: Option<i64>,
    model_id: Option<i64>,
    api_key_id: Option<i64>,
    provider_key: Option<String>,
    model_name: Option<String>,
    real_model_name: Option<String>,
    api_key_name: Option<String>,
    group_label: String,
    group_detail: Option<String>,
}

fn model_summary_map() -> Result<HashMap<i64, ModelSummaryItem>, BaseError> {
    Ok(Model::list_summary()?
        .into_iter()
        .map(|item| (item.id, item))
        .collect())
}

fn provider_summary_map() -> Result<HashMap<i64, ProviderSummaryItem>, BaseError> {
    Ok(Provider::list_summary()?
        .into_iter()
        .map(|item| (item.id, item))
        .collect())
}

fn parse_provider_model_scope(scope_id: &str) -> Option<(i64, i64)> {
    let (provider, model) = scope_id.split_once(':')?;
    Some((provider.parse().ok()?, model.parse().ok()?))
}

fn interval_bucket_start(timestamp_ms: i64, interval: &str) -> Result<i64, BaseError> {
    let bucket = match interval {
        "minute" => timestamp_ms.div_euclid(60_000) * 60_000,
        "hour" => timestamp_ms.div_euclid(3_600_000) * 3_600_000,
        "day" => timestamp_ms.div_euclid(86_400_000) * 86_400_000,
        "month" => {
            let dt = Utc
                .timestamp_millis_opt(timestamp_ms)
                .single()
                .ok_or_else(|| {
                    BaseError::ParamInvalid(Some("invalid usage stats timestamp".to_string()))
                })?;
            Utc.with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
                .single()
                .ok_or_else(|| {
                    BaseError::ParamInvalid(Some("invalid usage stats month bucket".to_string()))
                })?
                .timestamp_millis()
        }
        _ => {
            return Err(BaseError::ParamInvalid(Some(format!(
                "invalid metrics interval '{interval}'"
            ))));
        }
    };
    Ok(bucket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MetricsConfig;
    use crate::database::TestDbContext;
    use crate::database::metrics::{
        MetricCostRollupMinute, MetricRequestRollupMinute, add_cost_rollup_delta,
        add_request_rollup_delta,
    };

    #[test]
    fn usage_stats_return_empty_when_fallback_disabled_and_rollup_empty() {
        let context = TestDbContext::new_sqlite("metrics-query-empty.sqlite");
        context.run_sync(|| {
            let service = MetricsService::new(MetricsConfig {
                request_log_query_fallback_enabled: false,
                ..MetricsConfig::default()
            });
            let rows = service
                .usage_stats_aggregates(
                    0,
                    60_000,
                    "minute",
                    UsageStatsGroupBy::Provider,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
            assert!(rows.is_empty());
        });
    }

    #[test]
    fn usage_stats_group_model_from_provider_model_rollup() {
        let context = TestDbContext::new_sqlite("metrics-query-model.sqlite");
        context.run_sync(|| {
            add_request_rollup_delta(&request_rollup(0, "provider_model", "7:11", 3, 2, 1, 42))
                .unwrap();
            add_cost_rollup_delta(&cost_rollup(0, "provider_model", "7:11", "USD", 500)).unwrap();

            let service = MetricsService::new(MetricsConfig {
                request_log_query_fallback_enabled: false,
                ..MetricsConfig::default()
            });
            let rows = service
                .usage_stats_aggregates(
                    0,
                    60_000,
                    "minute",
                    UsageStatsGroupBy::Model,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].time, 0);
            assert_eq!(rows[0].provider_id, Some(7));
            assert_eq!(rows[0].model_id, Some(11));
            assert_eq!(rows[0].request_count, 3);
            assert_eq!(rows[0].success_count, 2);
            assert_eq!(rows[0].error_count, 1);
            assert_eq!(rows[0].total_tokens, 42);
            assert_eq!(rows[0].total_cost.get("USD"), Some(&500));
        });
    }

    #[test]
    fn dashboard_top_models_use_rollup_without_request_log_fallback() {
        let context = TestDbContext::new_sqlite("metrics-query-dashboard-top.sqlite");
        context.run_sync(|| {
            let bucket = Utc::now().timestamp_millis().div_euclid(60_000) * 60_000;
            add_request_rollup_delta(&request_rollup(
                bucket,
                "provider_model",
                "7:11",
                4,
                4,
                0,
                100,
            ))
            .unwrap();
            add_cost_rollup_delta(&cost_rollup(bucket, "provider_model", "7:11", "USD", 900))
                .unwrap();

            let service = MetricsService::new(MetricsConfig {
                request_log_query_fallback_enabled: false,
                ..MetricsConfig::default()
            });
            let rows = service.dashboard_top_models(5, None).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].provider_id, 7);
            assert_eq!(rows[0].model_id, 11);
            assert_eq!(rows[0].request_count, 4);
            assert_eq!(rows[0].total_cost.get("USD"), Some(&900));
        });
    }

    #[test]
    fn dashboard_cost_alert_models_aggregate_window_by_model_and_currency() {
        let context = TestDbContext::new_sqlite("metrics-query-dashboard-cost.sqlite");
        context.run_sync(|| {
            let bucket = Utc::now().timestamp_millis().div_euclid(60_000) * 60_000;
            add_request_rollup_delta(&request_rollup(
                bucket - 60_000,
                "provider_model",
                "7:11",
                1,
                1,
                0,
                10,
            ))
            .unwrap();
            add_request_rollup_delta(&request_rollup(
                bucket,
                "provider_model",
                "7:11",
                2,
                2,
                0,
                20,
            ))
            .unwrap();
            add_request_rollup_delta(&request_rollup(
                bucket,
                "provider_model",
                "7:12",
                5,
                5,
                0,
                50,
            ))
            .unwrap();
            add_cost_rollup_delta(&cost_rollup(
                bucket - 60_000,
                "provider_model",
                "7:11",
                "USD",
                100,
            ))
            .unwrap();
            add_cost_rollup_delta(&cost_rollup(bucket, "provider_model", "7:11", "USD", 200))
                .unwrap();
            add_cost_rollup_delta(&cost_rollup(bucket, "provider_model", "7:11", "CNY", 400))
                .unwrap();
            add_cost_rollup_delta(&cost_rollup(bucket, "provider_model", "7:12", "USD", 250))
                .unwrap();

            let service = MetricsService::new(MetricsConfig {
                request_log_query_fallback_enabled: false,
                ..MetricsConfig::default()
            });
            let rows = service.dashboard_cost_alert_models(1, None).unwrap();

            assert_eq!(rows.len(), 2);
            let usd = rows
                .iter()
                .find(|item| item.total_cost.contains_key("USD"))
                .expect("USD row should exist");
            assert_eq!(usd.provider_id, 7);
            assert_eq!(usd.model_id, 11);
            assert_eq!(usd.request_count, 3);
            assert_eq!(usd.total_tokens, 30);
            assert_eq!(usd.total_cost.get("USD"), Some(&300));

            let cny = rows
                .iter()
                .find(|item| item.total_cost.contains_key("CNY"))
                .expect("CNY row should exist");
            assert_eq!(cny.provider_id, 7);
            assert_eq!(cny.model_id, 11);
            assert_eq!(cny.total_cost.get("CNY"), Some(&400));
        });
    }

    #[test]
    fn metrics_facade_query_timeseries_aggregates_rollup_buckets() {
        let context = TestDbContext::new_sqlite("metrics-query-timeseries.sqlite");
        context.run_sync(|| {
            add_request_rollup_delta(&request_rollup(0, "provider", "7", 1, 1, 0, 10)).unwrap();
            add_request_rollup_delta(&request_rollup(60_000, "provider", "7", 2, 1, 1, 20))
                .unwrap();

            let service = MetricsService::new(MetricsConfig {
                request_log_query_fallback_enabled: false,
                ..MetricsConfig::default()
            });
            let points = service
                .query_timeseries(0, 120_000, "hour", Some("provider"), Some("7"))
                .unwrap();

            assert_eq!(points.len(), 1);
            assert_eq!(points[0].bucket_start_ms, 0);
            assert_eq!(points[0].scope_type, "provider");
            assert_eq!(points[0].scope_id, "7");
            assert_eq!(points[0].request_count, 3);
            assert_eq!(points[0].success_count, 2);
            assert_eq!(points[0].error_count, 1);
            assert_eq!(points[0].total_tokens, 30);
        });
    }

    fn request_rollup(
        bucket_start_ms: i64,
        scope_type: &str,
        scope_id: &str,
        request_count: i64,
        success_count: i64,
        error_count: i64,
        total_tokens: i64,
    ) -> MetricRequestRollupMinute {
        MetricRequestRollupMinute {
            bucket_start_ms,
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            scope_label: Some(scope_id.to_string()),
            request_count,
            success_count,
            error_count,
            cancelled_count: 0,
            retry_count: 0,
            fallback_count: 0,
            first_byte_latency_sum_ms: 0,
            first_byte_latency_count: 0,
            total_latency_sum_ms: 900,
            total_latency_count: request_count,
            input_tokens: total_tokens / 2,
            output_tokens: total_tokens / 2,
            reasoning_tokens: 0,
            total_tokens,
            transform_diagnostic_count: 0,
            transform_diagnostic_lossy_major_count: 0,
            transform_diagnostic_reject_count: 0,
            created_at: bucket_start_ms,
            updated_at: bucket_start_ms,
        }
    }

    fn cost_rollup(
        bucket_start_ms: i64,
        scope_type: &str,
        scope_id: &str,
        currency: &str,
        amount_nanos: i64,
    ) -> MetricCostRollupMinute {
        MetricCostRollupMinute {
            bucket_start_ms,
            metric_kind: "request".to_string(),
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            currency: currency.to_string(),
            amount_nanos,
            created_at: bucket_start_ms,
            updated_at: bucket_start_ms,
        }
    }
}
