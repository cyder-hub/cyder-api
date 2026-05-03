use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::controller::BaseError;
use crate::database::alert::{
    ALERT_STATUS_ACTIVE, AlertEvent, AlertListFilter, AlertRuleState, get_rule_state, list_alerts,
};
use crate::service::app_state::AppState;
use crate::service::metrics::MetricsService;
use crate::service::metrics::provider_runtime::{
    ProviderRuntimeHealthStatus, ProviderRuntimeWindow,
};
use crate::service::notification::NotificationService;
use crate::service::notification::types::NotificationEventType;

use super::rules::{
    AlertEvaluationWindow, MANAGED_RULE_KEYS, RULE_COST_HOTSPOT, RULE_HIGH_ERROR_RATE,
    RULE_HIGH_LATENCY, RULE_LOGGING_PIPELINE_DEGRADED, RULE_METRICS_UNAVAILABLE,
    RULE_PROVIDER_OPEN, RULE_RUNTIME_STATE_BACKEND_DEGRADED, attach_evaluation_windows,
    cost_hotspot_rule_candidates, cost_scope_key, logging_pipeline_rule_candidate,
    metrics_rollup_rule_candidates, metrics_unavailable_rule_candidate,
    provider_runtime_rule_candidates, runtime_state_backend_rule_candidate,
};
use super::service::AlertsService;
use super::types::{AlertEvaluationTickResult, AlertFireInput};

const METRICS_UNAVAILABLE_MANAGED_RULE_KEYS: &[&str] = &[
    RULE_METRICS_UNAVAILABLE,
    RULE_LOGGING_PIPELINE_DEGRADED,
    RULE_RUNTIME_STATE_BACKEND_DEGRADED,
];

impl AlertsService {
    pub async fn evaluate_all(
        &self,
        app_state: &Arc<AppState>,
        now_ms: i64,
    ) -> Result<AlertEvaluationTickResult, BaseError> {
        if !self.config().enabled {
            return Ok(AlertEvaluationTickResult::default());
        }

        let evaluation_window = AlertEvaluationWindow::ending_at(now_ms);
        let cost_hotspot_window = AlertEvaluationWindow::cost_hotspot_ending_at(now_ms);
        let start_time_ms = evaluation_window.start_time_ms;
        let mut candidates = Vec::new();
        let mut metrics_available = app_state.metrics.config().enabled;
        let mut recovery_window = None;
        let mut provider_open_healthy_scopes = None;

        if !metrics_available {
            candidates.push(metrics_unavailable_rule_candidate(
                app_state.metrics.config(),
                "metrics_disabled",
                None,
            ));
        } else {
            match app_state
                .metrics
                .query_request_window_metrics(start_time_ms, now_ms, None, None)
            {
                Ok(request_aggregates) => match app_state.metrics.query_attempt_window_metrics(
                    start_time_ms,
                    now_ms,
                    None,
                    None,
                ) {
                    Ok(attempt_aggregates) => {
                        recovery_window = Some((start_time_ms, now_ms));
                        let runtime_items = app_state
                            .metrics
                            .build_provider_runtime_items(
                                app_state,
                                ProviderRuntimeWindow::FifteenMinutes,
                                true,
                            )
                            .await?;
                        candidates.extend(provider_runtime_rule_candidates(
                            &runtime_items,
                            &self.config().rules,
                        ));
                        let cost_request_aggregates = self.cost_request_aggregates_for_rules(
                            app_state.metrics.as_ref(),
                            cost_hotspot_window.start_time_ms,
                            now_ms,
                        )?;
                        let provider_costs = self.provider_costs_for_rules(
                            app_state.metrics.as_ref(),
                            cost_hotspot_window.start_time_ms,
                            now_ms,
                            &cost_request_aggregates,
                        )?;
                        candidates.extend(metrics_rollup_rule_candidates(
                            &request_aggregates,
                            &attempt_aggregates,
                            &self.config().rules,
                        ));
                        candidates.extend(cost_hotspot_rule_candidates(
                            &cost_request_aggregates,
                            &provider_costs,
                            &self.config().rules,
                        ));
                        provider_open_healthy_scopes = Some(
                            runtime_items
                                .iter()
                                .filter(|item| {
                                    item.health_status == ProviderRuntimeHealthStatus::Healthy
                                        && !item.runtime_state_backend_degraded
                                        && item.runtime_state_backend_error.is_none()
                                })
                                .map(|item| item.provider_id.to_string())
                                .collect::<HashSet<_>>(),
                        );
                    }
                    Err(err) => {
                        metrics_available = false;
                        candidates.push(metrics_unavailable_rule_candidate(
                            app_state.metrics.config(),
                            "attempt_rollup_query_failed",
                            Some(format!("{err:?}")),
                        ));
                    }
                },
                Err(err) => {
                    metrics_available = false;
                    candidates.push(metrics_unavailable_rule_candidate(
                        app_state.metrics.config(),
                        "request_rollup_query_failed",
                        Some(format!("{err:?}")),
                    ));
                }
            }
        }

        let current_log_metrics = app_state.infra.log_manager().metrics();
        let log_delta = self.consume_log_manager_metrics_delta(current_log_metrics.clone());
        if let Some(candidate) =
            logging_pipeline_rule_candidate(&current_log_metrics, &log_delta, &self.config().rules)
        {
            candidates.push(candidate);
        }

        let runtime_state_backend = app_state.runtime_state_backend_operator_status().await;
        if let Some(candidate) =
            runtime_state_backend_rule_candidate(&runtime_state_backend, &self.config().rules)
        {
            candidates.push(candidate);
        }
        let candidates =
            attach_evaluation_windows(candidates, evaluation_window, cost_hotspot_window);
        let managed_rule_keys = if metrics_available {
            MANAGED_RULE_KEYS
        } else {
            METRICS_UNAVAILABLE_MANAGED_RULE_KEYS
        };

        self.apply_rule_candidates_with_notification(
            candidates,
            managed_rule_keys,
            now_ms,
            Some(app_state.notification.as_ref()),
            self.config().default_cooldown_seconds,
            app_state.metrics.as_ref(),
            recovery_window,
            provider_open_healthy_scopes.as_ref(),
        )
    }

    #[cfg(test)]
    pub(crate) fn apply_rule_candidates(
        &self,
        candidates: Vec<AlertFireInput>,
        managed_rule_keys: &[&str],
        now_ms: i64,
    ) -> Result<AlertEvaluationTickResult, BaseError> {
        let metrics = MetricsService::new(crate::config::MetricsConfig::default());
        self.apply_rule_candidates_with_notification(
            candidates,
            managed_rule_keys,
            now_ms,
            None,
            0,
            &metrics,
            None,
            None,
        )
    }

    fn apply_rule_candidates_with_notification(
        &self,
        candidates: Vec<AlertFireInput>,
        managed_rule_keys: &[&str],
        now_ms: i64,
        notification: Option<&NotificationService>,
        notification_cooldown_seconds: u64,
        metrics: &MetricsService,
        recovery_window: Option<(i64, i64)>,
        provider_open_healthy_scopes: Option<&HashSet<String>>,
    ) -> Result<AlertEvaluationTickResult, BaseError> {
        let mut summary = AlertEvaluationTickResult {
            evaluated: managed_rule_keys.len() as u64,
            ..AlertEvaluationTickResult::default()
        };
        let mut fired_fingerprints = HashSet::new();
        let managed_rule_key_set = managed_rule_keys.iter().copied().collect::<HashSet<_>>();

        for candidate in candidates {
            if !managed_rule_key_set.contains(candidate.rule_key.as_str()) {
                continue;
            }
            if !fired_fingerprints.insert(candidate.fingerprint.clone()) {
                continue;
            }
            let alert = self.fire_alert(candidate, now_ms)?;
            if let Some(notification) = notification {
                notification.enqueue_alert_event(
                    &alert,
                    NotificationEventType::AlertFired,
                    now_ms,
                    notification_cooldown_seconds,
                )?;
            }
            self.upsert_rule_state(&AlertRuleState {
                rule_key: alert.rule_key.clone(),
                scope_type: alert.scope_type.clone(),
                scope_id: alert.scope_id.clone(),
                last_evaluated_at: now_ms,
                last_fired_at: Some(now_ms),
                last_resolved_at: None,
                cooldown_until: None,
            })?;
            summary.fired += 1;
        }

        for alert in list_alerts(AlertListFilter {
            status: Some(ALERT_STATUS_ACTIVE.to_string()),
            ..AlertListFilter::default()
        })? {
            if !managed_rule_key_set.contains(alert.rule_key.as_str()) {
                continue;
            }
            if fired_fingerprints.contains(&alert.fingerprint) {
                continue;
            }

            if !self.should_resolve_missing_alert(
                &alert,
                now_ms,
                metrics,
                recovery_window,
                provider_open_healthy_scopes,
            )? {
                summary.evaluated += 1;
                continue;
            }

            let resolved = self.resolve_alert(alert.id, now_ms)?;
            if let Some(notification) = notification {
                notification.enqueue_alert_event(
                    &resolved,
                    NotificationEventType::AlertRecovered,
                    now_ms,
                    notification_cooldown_seconds,
                )?;
            }
            self.upsert_rule_state(&AlertRuleState {
                rule_key: resolved.rule_key.clone(),
                scope_type: resolved.scope_type.clone(),
                scope_id: resolved.scope_id.clone(),
                last_evaluated_at: now_ms,
                last_fired_at: None,
                last_resolved_at: Some(now_ms),
                cooldown_until: None,
            })?;
            summary.resolved += 1;
        }

        Ok(summary)
    }

    fn cost_request_aggregates_for_rules(
        &self,
        metrics: &MetricsService,
        start_time_ms: i64,
        end_time_ms: i64,
    ) -> Result<Vec<crate::database::metrics::MetricRequestWindowAggregate>, BaseError> {
        if self.config().rules.cost_hotspot_amount_nanos.is_none() {
            return Ok(Vec::new());
        }

        metrics.query_request_window_metrics(start_time_ms, end_time_ms, None, None)
    }

    fn provider_costs_for_rules(
        &self,
        metrics: &MetricsService,
        start_time_ms: i64,
        end_time_ms: i64,
        request_aggregates: &[crate::database::metrics::MetricRequestWindowAggregate],
    ) -> Result<HashMap<String, Vec<crate::database::metrics::MetricCostAggregate>>, BaseError>
    {
        let mut costs = HashMap::new();
        if self.config().rules.cost_hotspot_amount_nanos.is_none() {
            return Ok(costs);
        }

        for request in request_aggregates.iter().filter(|item| {
            matches!(
                item.scope_type.as_str(),
                "provider" | "model" | "api_key" | "provider_api_key"
            )
        }) {
            costs.insert(
                cost_scope_key(&request.scope_type, &request.scope_id),
                metrics.query_cost_window_metrics(
                    start_time_ms,
                    end_time_ms,
                    "request",
                    &request.scope_type,
                    &request.scope_id,
                )?,
            );
        }
        Ok(costs)
    }

    fn should_resolve_missing_alert(
        &self,
        alert: &AlertEvent,
        now_ms: i64,
        metrics: &MetricsService,
        recovery_window: Option<(i64, i64)>,
        provider_open_healthy_scopes: Option<&HashSet<String>>,
    ) -> Result<bool, BaseError> {
        match alert.rule_key.as_str() {
            RULE_METRICS_UNAVAILABLE => Ok(recovery_window.is_some()),
            RULE_PROVIDER_OPEN => {
                Ok(provider_open_healthy_scopes
                    .is_some_and(|scopes| scopes.contains(&alert.scope_id)))
            }
            RULE_HIGH_ERROR_RATE => self.high_error_recovered(metrics, alert, recovery_window),
            RULE_HIGH_LATENCY => self.high_latency_recovered(metrics, alert, recovery_window),
            RULE_COST_HOTSPOT => self.cost_hotspot_recovered(metrics, alert, now_ms),
            _ => self.consecutive_missing_window_recovered(alert, now_ms),
        }
    }

    fn high_error_recovered(
        &self,
        metrics: &MetricsService,
        alert: &AlertEvent,
        recovery_window: Option<(i64, i64)>,
    ) -> Result<bool, BaseError> {
        let Some((start_time, end_time)) = recovery_window else {
            return Ok(true);
        };
        let request = metrics
            .query_request_window_metrics(
                start_time,
                end_time,
                Some(&alert.scope_type),
                Some(&alert.scope_id),
            )?
            .into_iter()
            .next();
        let attempt = metrics
            .query_attempt_window_metrics(
                start_time,
                end_time,
                Some(&alert.scope_type),
                Some(&alert.scope_id),
            )?
            .into_iter()
            .next();
        let request_rate = request
            .as_ref()
            .and_then(|item| {
                error_rate(item.request_count, item.error_count + item.cancelled_count)
            })
            .unwrap_or(0.0);
        let attempt_rate = attempt
            .as_ref()
            .and_then(|item| error_rate(item.attempt_count, item.error_count))
            .unwrap_or(0.0);
        Ok(request_rate < 0.10 && attempt_rate < 0.10)
    }

    fn high_latency_recovered(
        &self,
        metrics: &MetricsService,
        alert: &AlertEvent,
        recovery_window: Option<(i64, i64)>,
    ) -> Result<bool, BaseError> {
        let Some((start_time, end_time)) = recovery_window else {
            return Ok(true);
        };
        let request = metrics
            .query_request_window_metrics(
                start_time,
                end_time,
                Some(&alert.scope_type),
                Some(&alert.scope_id),
            )?
            .into_iter()
            .next();
        let attempt = metrics
            .query_attempt_window_metrics(
                start_time,
                end_time,
                Some(&alert.scope_type),
                Some(&alert.scope_id),
            )?
            .into_iter()
            .next();
        let request_avg = request
            .as_ref()
            .and_then(|item| average_or_none(item.total_latency_sum_ms, item.total_latency_count))
            .unwrap_or(0.0);
        let attempt_avg = attempt
            .as_ref()
            .and_then(|item| average_or_none(item.total_latency_sum_ms, item.total_latency_count))
            .unwrap_or(0.0);
        Ok(request_avg < 5_000.0 && attempt_avg < 5_000.0)
    }

    fn cost_hotspot_recovered(
        &self,
        metrics: &MetricsService,
        alert: &AlertEvent,
        now_ms: i64,
    ) -> Result<bool, BaseError> {
        let Some(threshold) = self.config().rules.cost_hotspot_amount_nanos else {
            return Ok(true);
        };
        let recovery_window = AlertEvaluationWindow::cost_hotspot_ending_at(now_ms);
        let currency = cost_hotspot_currency(&alert.fingerprint);
        let costs = metrics.query_cost_window_metrics(
            recovery_window.start_time_ms,
            recovery_window.end_time_ms,
            "request",
            &alert.scope_type,
            &alert.scope_id,
        )?;
        let recovery_threshold = threshold / 2;
        Ok(costs
            .into_iter()
            .filter(|item| {
                currency
                    .as_deref()
                    .is_none_or(|expected| item.currency == expected)
            })
            .all(|item| item.amount_nanos < recovery_threshold))
    }

    fn consecutive_missing_window_recovered(
        &self,
        alert: &AlertEvent,
        now_ms: i64,
    ) -> Result<bool, BaseError> {
        let state = get_rule_state(&alert.rule_key, &alert.scope_type, &alert.scope_id)?;
        if state
            .as_ref()
            .and_then(|item| item.cooldown_until)
            .is_some_and(|count| count >= 1)
        {
            return Ok(true);
        }

        self.upsert_rule_state(&AlertRuleState {
            rule_key: alert.rule_key.clone(),
            scope_type: alert.scope_type.clone(),
            scope_id: alert.scope_id.clone(),
            last_evaluated_at: now_ms,
            last_fired_at: state.as_ref().and_then(|item| item.last_fired_at),
            last_resolved_at: state.as_ref().and_then(|item| item.last_resolved_at),
            cooldown_until: Some(1),
        })?;
        Ok(false)
    }
}

fn error_rate(count: i64, errors: i64) -> Option<f64> {
    (count > 0).then_some(errors as f64 / count as f64)
}

fn average_or_none(sum: i64, count: i64) -> Option<f64> {
    (count > 0).then_some(sum as f64 / count as f64)
}

fn cost_hotspot_currency(fingerprint: &str) -> Option<String> {
    let mut parts = fingerprint.split(':');
    match (parts.next(), parts.next()) {
        (Some(RULE_COST_HOTSPOT), Some(currency)) => Some(currency.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AlertsConfig, MetricsConfig, ProviderGovernanceConfig};
    use crate::database::TestDbContext;
    use crate::database::alert::{ALERT_STATUS_RESOLVED, get_alert_by_fingerprint};
    use crate::database::metrics::{
        MetricAttemptRollupMinute, MetricCostRollupMinute, MetricRequestRollupMinute,
        add_attempt_rollup_delta, add_cost_rollup_delta, add_request_rollup_delta,
    };
    use crate::database::notification::{
        NOTIFICATION_CHANNEL_TYPE_WEBHOOK, NewNotificationChannel, NotificationDeliveryListFilter,
        create_channel, list_deliveries,
    };
    use crate::database::provider::{NewProvider, Provider};
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::alerts::types::{AlertFireInput, AlertScopeType, AlertSeverity};
    use crate::service::app_state::{AppState, create_test_app_state};
    use crate::service::metrics::MetricsService;
    use crate::service::notification::NotificationService;
    use std::{collections::HashSet, sync::Arc};

    fn with_metrics_config(
        app_state: Arc<AppState>,
        metrics_config: MetricsConfig,
    ) -> Arc<AppState> {
        Arc::new(AppState {
            metrics: Arc::new(MetricsService::new(metrics_config)),
            ..(*app_state).clone()
        })
    }

    fn with_alerts_config(app_state: Arc<AppState>, alerts_config: AlertsConfig) -> Arc<AppState> {
        Arc::new(AppState {
            alerts: Arc::new(AlertsService::new(alerts_config)),
            ..(*app_state).clone()
        })
    }

    #[test]
    fn engine_resolves_provider_open_only_when_provider_is_healthy() {
        let context = TestDbContext::new_sqlite("alert-engine-resolve.sqlite");
        context.run_sync(|| {
            let service = AlertsService::new(AlertsConfig::default());
            let metrics = MetricsService::new(crate::config::MetricsConfig::default());
            let first = service
                .apply_rule_candidates(vec![provider_open_candidate()], &["provider_open"], 1_000)
                .unwrap();
            assert_eq!(first.fired, 1);
            assert_eq!(first.resolved, 0);

            let second = service
                .apply_rule_candidates_with_notification(
                    Vec::new(),
                    &["provider_open"],
                    2_000,
                    None,
                    0,
                    &metrics,
                    None,
                    None,
                )
                .unwrap();
            assert_eq!(second.fired, 0);
            assert_eq!(second.resolved, 0);

            let healthy_scopes = HashSet::from(["7".to_string()]);
            let third = service
                .apply_rule_candidates_with_notification(
                    Vec::new(),
                    &["provider_open"],
                    3_000,
                    None,
                    0,
                    &metrics,
                    None,
                    Some(&healthy_scopes),
                )
                .unwrap();
            assert_eq!(third.fired, 0);
            assert_eq!(third.resolved, 1);

            let alert = get_alert_by_fingerprint("provider_open:provider:7")
                .unwrap()
                .expect("alert should exist");
            assert_eq!(alert.status, ALERT_STATUS_RESOLVED);
            assert_eq!(alert.resolved_at, Some(3_000));
        });
    }

    #[test]
    fn engine_updates_existing_active_alert_on_repeated_candidate() {
        let context = TestDbContext::new_sqlite("alert-engine-repeat.sqlite");
        context.run_sync(|| {
            let service = AlertsService::new(AlertsConfig::default());
            service
                .apply_rule_candidates(vec![provider_open_candidate()], &["provider_open"], 1_000)
                .unwrap();
            service
                .apply_rule_candidates(vec![provider_open_candidate()], &["provider_open"], 2_000)
                .unwrap();

            let alert = get_alert_by_fingerprint("provider_open:provider:7")
                .unwrap()
                .expect("alert should exist");
            assert_eq!(alert.occurrence_count, 2);
            assert_eq!(alert.last_seen_at, 2_000);
        });
    }

    #[test]
    fn engine_enqueues_fire_once_per_cooldown_and_recovery_delivery() {
        let context = TestDbContext::new_sqlite("alert-engine-notification.sqlite");
        context.run_sync(|| {
            let service = AlertsService::new(AlertsConfig::default());
            let metrics = MetricsService::new(crate::config::MetricsConfig::default());
            let notification =
                NotificationService::new(crate::config::NotificationConfig::default());
            create_channel(
                &NewNotificationChannel {
                    channel_key: "ops".to_string(),
                    channel_type: NOTIFICATION_CHANNEL_TYPE_WEBHOOK.to_string(),
                    name: "Ops".to_string(),
                    endpoint_url: "https://example.com/webhook".to_string(),
                    signing_secret: None,
                    headers_json: None,
                    cooldown_seconds: 900,
                    is_enabled: true,
                },
                900,
            )
            .unwrap();

            service
                .apply_rule_candidates_with_notification(
                    vec![provider_open_candidate()],
                    &["provider_open"],
                    1_000,
                    Some(&notification),
                    60,
                    &metrics,
                    None,
                    None,
                )
                .unwrap();
            service
                .apply_rule_candidates_with_notification(
                    vec![provider_open_candidate()],
                    &["provider_open"],
                    2_000,
                    Some(&notification),
                    60,
                    &metrics,
                    None,
                    None,
                )
                .unwrap();
            let healthy_scopes = HashSet::from(["7".to_string()]);
            service
                .apply_rule_candidates_with_notification(
                    Vec::new(),
                    &["provider_open"],
                    3_000,
                    Some(&notification),
                    60,
                    &metrics,
                    None,
                    Some(&healthy_scopes),
                )
                .unwrap();

            let alert_id = get_alert_by_fingerprint("provider_open:provider:7")
                .unwrap()
                .expect("alert should exist")
                .id;
            let deliveries = list_deliveries(NotificationDeliveryListFilter {
                alert_id: Some(alert_id),
                ..NotificationDeliveryListFilter::default()
            })
            .unwrap();
            assert_eq!(deliveries.len(), 2);
            assert!(
                deliveries
                    .iter()
                    .any(|item| item.event_type == "alert_fired")
            );
            assert!(
                deliveries
                    .iter()
                    .any(|item| item.event_type == "alert_recovered")
            );
        });
    }

    #[test]
    fn engine_uses_high_error_recovery_threshold_before_resolving() {
        let context = TestDbContext::new_sqlite("alert-engine-high-error-recovery.sqlite");
        context.run_sync(|| {
            let service = AlertsService::new(AlertsConfig::default());
            let metrics = MetricsService::new(crate::config::MetricsConfig::default());
            service
                .apply_rule_candidates(
                    vec![AlertFireInput {
                        fingerprint: "high_error_rate:provider:7".to_string(),
                        rule_key: "high_error_rate".to_string(),
                        severity: AlertSeverity::Critical,
                        scope_type: AlertScopeType::Provider,
                        scope_id: "7".to_string(),
                        title: "High provider error rate".to_string(),
                        summary: "Provider is above threshold".to_string(),
                        details_json: "{}".to_string(),
                        metrics_snapshot_json: None,
                    }],
                    &["high_error_rate"],
                    1_000,
                )
                .unwrap();
            add_request_rollup_delta(&request_rollup(0, 20, 17, 3)).unwrap();

            let still_active = service
                .apply_rule_candidates_with_notification(
                    Vec::new(),
                    &["high_error_rate"],
                    2_000,
                    None,
                    0,
                    &metrics,
                    Some((0, 60_000)),
                    None,
                )
                .unwrap();
            assert_eq!(still_active.resolved, 0);

            let recovered = service
                .apply_rule_candidates_with_notification(
                    Vec::new(),
                    &["high_error_rate"],
                    61_000,
                    None,
                    0,
                    &metrics,
                    Some((60_000, 120_000)),
                    None,
                )
                .unwrap();
            assert_eq!(recovered.resolved, 1);

            let alert = get_alert_by_fingerprint("high_error_rate:provider:7")
                .unwrap()
                .expect("alert should exist");
            assert_eq!(alert.status, ALERT_STATUS_RESOLVED);
        });
    }

    #[tokio::test]
    async fn evaluate_all_uses_fixed_15m_metrics_window_and_details() {
        let context = TestDbContext::new_sqlite("alert-engine-fixed-window.sqlite");
        context
            .run_async(async {
                let app_state = create_test_app_state(context.clone()).await;
                let now_ms = 2_000_000;
                add_request_rollup_delta(&request_rollup(now_ms - 20 * 60 * 1_000, 100, 10, 90))
                    .unwrap();
                add_request_rollup_delta(&request_rollup(now_ms - 10 * 60 * 1_000, 100, 100, 0))
                    .unwrap();

                let healthy = app_state
                    .alerts
                    .evaluate_all(&app_state, now_ms)
                    .await
                    .unwrap();
                assert_eq!(healthy.fired, 0);

                add_request_rollup_delta(&request_rollup(now_ms - 5 * 60 * 1_000, 100, 10, 90))
                    .unwrap();
                let fired = app_state
                    .alerts
                    .evaluate_all(&app_state, now_ms + 1_000)
                    .await
                    .unwrap();
                assert_eq!(fired.fired, 1);

                let alert = get_alert_by_fingerprint("high_error_rate:provider:7")
                    .unwrap()
                    .expect("provider high error alert should fire");
                let details: serde_json::Value =
                    serde_json::from_str(&alert.details_json).expect("details should be JSON");
                assert_eq!(
                    details
                        .get("evaluation_window_seconds")
                        .and_then(|value| value.as_i64()),
                    Some(15 * 60)
                );
                assert_eq!(
                    details
                        .get("window_end_ms")
                        .and_then(|value| value.as_i64()),
                    Some(now_ms + 1_000)
                );
                assert!(details.get("threshold").is_some());
            })
            .await;
    }

    #[tokio::test]
    async fn evaluate_all_uses_one_hour_cost_hotspot_window() {
        let context = TestDbContext::new_sqlite("alert-engine-cost-hotspot-window.sqlite");
        context
            .run_async(async {
                let app_state = create_test_app_state(context.clone()).await;
                let app_state = with_alerts_config(
                    app_state,
                    AlertsConfig {
                        rules: crate::config::AlertRulesConfig {
                            cost_hotspot_amount_nanos: Some(100),
                            ..crate::config::AlertRulesConfig::default()
                        },
                        ..AlertsConfig::default()
                    },
                );
                let now_ms = 3_600_000;
                let bucket_start_ms = now_ms - 50 * 60 * 1_000;
                add_request_rollup_delta(&request_rollup(bucket_start_ms, 1, 1, 0)).unwrap();
                add_cost_rollup_delta(&cost_rollup(bucket_start_ms, 150)).unwrap();

                let result = app_state
                    .alerts
                    .evaluate_all(&app_state, now_ms)
                    .await
                    .unwrap();

                assert_eq!(result.fired, 1);
                let alert = get_alert_by_fingerprint("cost_hotspot:USD:provider:7")
                    .unwrap()
                    .expect("one hour provider cost hotspot should fire");
                let details: serde_json::Value =
                    serde_json::from_str(&alert.details_json).expect("details should be JSON");
                assert_eq!(
                    details
                        .get("evaluation_window_seconds")
                        .and_then(serde_json::Value::as_i64),
                    Some(60 * 60)
                );
                assert_eq!(
                    details
                        .get("window_start_ms")
                        .and_then(serde_json::Value::as_i64),
                    Some(0)
                );
            })
            .await;
    }

    #[test]
    fn engine_uses_one_hour_cost_recovery_window_before_resolving() {
        let context = TestDbContext::new_sqlite("alert-engine-cost-hotspot-recovery.sqlite");
        context.run_sync(|| {
            let service = AlertsService::new(AlertsConfig {
                rules: crate::config::AlertRulesConfig {
                    cost_hotspot_amount_nanos: Some(100),
                    ..crate::config::AlertRulesConfig::default()
                },
                ..AlertsConfig::default()
            });
            let metrics = MetricsService::new(crate::config::MetricsConfig::default());
            service
                .apply_rule_candidates(
                    vec![AlertFireInput {
                        fingerprint: "cost_hotspot:USD:provider:7".to_string(),
                        rule_key: "cost_hotspot".to_string(),
                        severity: AlertSeverity::Warning,
                        scope_type: AlertScopeType::Provider,
                        scope_id: "7".to_string(),
                        title: "Cost hotspot".to_string(),
                        summary: "Provider is above cost threshold".to_string(),
                        details_json: "{}".to_string(),
                        metrics_snapshot_json: None,
                    }],
                    &["cost_hotspot"],
                    1_000,
                )
                .unwrap();

            let now_ms = 3_600_000;
            add_cost_rollup_delta(&cost_rollup(now_ms - 50 * 60 * 1_000, 75)).unwrap();
            let still_active = service
                .apply_rule_candidates_with_notification(
                    Vec::new(),
                    &["cost_hotspot"],
                    now_ms,
                    None,
                    0,
                    &metrics,
                    Some((now_ms - 15 * 60 * 1_000, now_ms)),
                    None,
                )
                .unwrap();
            assert_eq!(still_active.resolved, 0);

            let recovered_at = now_ms + 61 * 60 * 1_000;
            let recovered = service
                .apply_rule_candidates_with_notification(
                    Vec::new(),
                    &["cost_hotspot"],
                    recovered_at,
                    None,
                    0,
                    &metrics,
                    Some((recovered_at - 15 * 60 * 1_000, recovered_at)),
                    None,
                )
                .unwrap();
            assert_eq!(recovered.resolved, 1);

            let alert = get_alert_by_fingerprint("cost_hotspot:USD:provider:7")
                .unwrap()
                .expect("cost hotspot alert should exist");
            assert_eq!(alert.status, ALERT_STATUS_RESOLVED);
            assert_eq!(alert.resolved_at, Some(recovered_at));
        });
    }

    #[tokio::test]
    async fn evaluate_all_resolves_provider_open_after_circuit_is_healthy() {
        let context = TestDbContext::new_sqlite("alert-engine-provider-open-healthy.sqlite");
        context
            .run_async(async {
                let now_ms = 3_000_000;
                Provider::create(&NewProvider {
                    id: 7,
                    provider_key: "provider-7".to_string(),
                    name: "Provider 7".to_string(),
                    endpoint: "https://example.com".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    created_at: now_ms,
                    updated_at: now_ms,
                    provider_type: ProviderType::Openai,
                    provider_api_key_mode: ProviderApiKeyMode::Queue,
                })
                .expect("provider should insert");
                let app_state = create_test_app_state(context.clone()).await;
                app_state
                    .provider_circuit
                    .update_config(ProviderGovernanceConfig {
                        enabled: true,
                        consecutive_failure_threshold: 1,
                        open_cooldown_seconds: 0,
                    })
                    .await;
                app_state
                    .provider_circuit
                    .record_provider_failure(7, "timeout".to_string(), None)
                    .await
                    .expect("failure should open provider circuit");

                let fired = app_state
                    .alerts
                    .evaluate_all(&app_state, now_ms)
                    .await
                    .unwrap();
                assert_eq!(fired.fired, 1);

                let decision = app_state
                    .provider_circuit
                    .allow_provider_request(7)
                    .await
                    .expect("half-open probe should be allowed");
                let half_open = app_state
                    .alerts
                    .evaluate_all(&app_state, now_ms + 60_000)
                    .await
                    .unwrap();
                assert_eq!(half_open.resolved, 0);

                app_state
                    .provider_circuit
                    .record_provider_success(7, decision.probe_permit.as_ref())
                    .await
                    .expect("matching half-open success should close provider circuit");
                let resolved = app_state
                    .alerts
                    .evaluate_all(&app_state, now_ms + 120_000)
                    .await
                    .unwrap();
                assert_eq!(resolved.resolved, 1);

                let alert = get_alert_by_fingerprint("provider_open:provider:7")
                    .unwrap()
                    .expect("provider open alert should exist");
                assert_eq!(alert.status, ALERT_STATUS_RESOLVED);
            })
            .await;
    }

    #[tokio::test]
    async fn evaluate_all_fires_provider_high_error_from_attempt_only_rollup() {
        let context = TestDbContext::new_sqlite("alert-engine-attempt-only.sqlite");
        context
            .run_async(async {
                let app_state = create_test_app_state(context.clone()).await;
                let now_ms = 4_000_000;
                add_attempt_rollup_delta(&attempt_rollup(
                    now_ms - 60_000,
                    "provider",
                    "7",
                    20,
                    0,
                    20,
                ))
                .unwrap();

                let result = app_state
                    .alerts
                    .evaluate_all(&app_state, now_ms)
                    .await
                    .unwrap();
                assert_eq!(result.fired, 1);
                let alert = get_alert_by_fingerprint("high_error_rate:provider:7")
                    .unwrap()
                    .expect("attempt-only provider high error alert should fire");
                assert_eq!(alert.severity, "critical");
            })
            .await;
    }

    #[tokio::test]
    async fn evaluate_all_fires_dedupes_and_resolves_metrics_unavailable() {
        let context = TestDbContext::new_sqlite("alert-engine-metrics-unavailable.sqlite");
        context
            .run_async(async {
                create_channel(
                    &NewNotificationChannel {
                        channel_key: "ops".to_string(),
                        channel_type: NOTIFICATION_CHANNEL_TYPE_WEBHOOK.to_string(),
                        name: "Ops".to_string(),
                        endpoint_url: "https://example.com/webhook".to_string(),
                        signing_secret: None,
                        headers_json: None,
                        cooldown_seconds: 900,
                        is_enabled: true,
                    },
                    900,
                )
                .unwrap();

                let app_state = create_test_app_state(context.clone()).await;
                let disabled_app_state = with_metrics_config(
                    Arc::clone(&app_state),
                    MetricsConfig {
                        enabled: false,
                        request_log_query_fallback_enabled: true,
                        ..MetricsConfig::default()
                    },
                );
                let now_ms = 5_000_000;

                let fired = disabled_app_state
                    .alerts
                    .evaluate_all(&disabled_app_state, now_ms)
                    .await
                    .unwrap();
                assert_eq!(fired.fired, 1);
                let first = get_alert_by_fingerprint("metrics_unavailable:system:system")
                    .unwrap()
                    .expect("metrics unavailable alert should fire");
                assert_eq!(first.status, ALERT_STATUS_ACTIVE);
                let details: serde_json::Value =
                    serde_json::from_str(&first.details_json).expect("details should parse");
                assert_eq!(
                    details
                        .get("metrics_enabled")
                        .and_then(serde_json::Value::as_bool),
                    Some(false)
                );
                assert_eq!(
                    details
                        .get("request_log_query_fallback_enabled")
                        .and_then(serde_json::Value::as_bool),
                    Some(true)
                );
                assert_eq!(
                    details
                        .get("evaluation_window_seconds")
                        .and_then(serde_json::Value::as_i64),
                    Some(15 * 60)
                );

                disabled_app_state
                    .alerts
                    .evaluate_all(&disabled_app_state, now_ms + 1_000)
                    .await
                    .unwrap();
                let repeated = get_alert_by_fingerprint("metrics_unavailable:system:system")
                    .unwrap()
                    .expect("metrics unavailable alert should stay active");
                assert_eq!(repeated.id, first.id);
                assert_eq!(repeated.occurrence_count, first.occurrence_count + 1);
                assert_eq!(repeated.last_seen_at, now_ms + 1_000);

                let recovered_app_state =
                    with_metrics_config(Arc::clone(&app_state), MetricsConfig::default());
                let resolved = recovered_app_state
                    .alerts
                    .evaluate_all(&recovered_app_state, now_ms + 2_000)
                    .await
                    .unwrap();
                assert_eq!(resolved.resolved, 1);
                let alert = get_alert_by_fingerprint("metrics_unavailable:system:system")
                    .unwrap()
                    .expect("metrics unavailable alert should exist");
                assert_eq!(alert.status, ALERT_STATUS_RESOLVED);
                assert_eq!(alert.resolved_at, Some(now_ms + 2_000));

                let deliveries = list_deliveries(NotificationDeliveryListFilter {
                    alert_id: Some(first.id),
                    ..NotificationDeliveryListFilter::default()
                })
                .unwrap();
                assert_eq!(deliveries.len(), 2);
                assert!(
                    deliveries
                        .iter()
                        .any(|item| item.event_type == "alert_fired")
                );
                assert!(
                    deliveries
                        .iter()
                        .any(|item| item.event_type == "alert_recovered")
                );
            })
            .await;
    }

    fn provider_open_candidate() -> AlertFireInput {
        AlertFireInput {
            fingerprint: "provider_open:provider:7".to_string(),
            rule_key: "provider_open".to_string(),
            severity: AlertSeverity::Critical,
            scope_type: AlertScopeType::Provider,
            scope_id: "7".to_string(),
            title: "Provider open".to_string(),
            summary: "Provider is open".to_string(),
            details_json: "{}".to_string(),
            metrics_snapshot_json: None,
        }
    }

    fn request_rollup(
        bucket_start_ms: i64,
        request_count: i64,
        success_count: i64,
        error_count: i64,
    ) -> MetricRequestRollupMinute {
        MetricRequestRollupMinute {
            bucket_start_ms,
            scope_type: "provider".to_string(),
            scope_id: "7".to_string(),
            scope_label: Some("Provider 7".to_string()),
            request_count,
            success_count,
            error_count,
            cancelled_count: 0,
            retry_count: 0,
            fallback_count: 0,
            first_byte_latency_sum_ms: 0,
            first_byte_latency_count: 0,
            total_latency_sum_ms: 0,
            total_latency_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            transform_diagnostic_count: 0,
            transform_diagnostic_lossy_major_count: 0,
            transform_diagnostic_reject_count: 0,
            created_at: bucket_start_ms,
            updated_at: bucket_start_ms,
        }
    }

    fn attempt_rollup(
        bucket_start_ms: i64,
        scope_type: &str,
        scope_id: &str,
        attempt_count: i64,
        success_count: i64,
        error_count: i64,
    ) -> MetricAttemptRollupMinute {
        MetricAttemptRollupMinute {
            bucket_start_ms,
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            scope_label: Some(scope_id.to_string()),
            attempt_count,
            success_count,
            error_count,
            skipped_count: 0,
            retry_same_candidate_count: 0,
            fallback_next_candidate_count: 0,
            fail_fast_count: 0,
            first_byte_latency_sum_ms: 0,
            first_byte_latency_count: 0,
            total_latency_sum_ms: 0,
            total_latency_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            created_at: bucket_start_ms,
            updated_at: bucket_start_ms,
        }
    }

    fn cost_rollup(bucket_start_ms: i64, amount_nanos: i64) -> MetricCostRollupMinute {
        MetricCostRollupMinute {
            bucket_start_ms,
            metric_kind: "request".to_string(),
            scope_type: "provider".to_string(),
            scope_id: "7".to_string(),
            currency: "USD".to_string(),
            amount_nanos,
            created_at: bucket_start_ms,
            updated_at: bucket_start_ms,
        }
    }
}
