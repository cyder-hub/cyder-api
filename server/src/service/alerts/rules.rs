use std::collections::{BTreeSet, HashMap};

use serde_json::{Value, json};

use crate::config::{AlertRulesConfig, MetricsConfig};
use crate::database::metrics::{
    MetricAttemptWindowAggregate, MetricCostAggregate, MetricRequestWindowAggregate,
};
use crate::proxy::logging::LogManagerMetricsSnapshot;
use crate::service::metrics::provider_runtime::{ProviderRuntimeItem, ProviderRuntimeLevel};
use crate::service::runtime::RuntimeStateBackendOperatorStatus;

use super::types::{AlertFireInput, AlertScopeType, AlertSeverity};

pub const RULE_PROVIDER_OPEN: &str = "provider_open";
pub const RULE_PROVIDER_DEGRADED: &str = "provider_degraded";
pub const RULE_HIGH_ERROR_RATE: &str = "high_error_rate";
pub const RULE_HIGH_LATENCY: &str = "high_latency";
pub const RULE_COST_HOTSPOT: &str = "cost_hotspot";
pub const RULE_TRANSFORM_DIAGNOSTIC_SPIKE: &str = "transform_diagnostic_spike";
pub const RULE_LOGGING_PIPELINE_DEGRADED: &str = "logging_pipeline_degraded";
pub const RULE_RUNTIME_STATE_BACKEND_DEGRADED: &str = "runtime_state_backend_degraded";
pub const RULE_METRICS_UNAVAILABLE: &str = "metrics_unavailable";
pub const ALERT_METRICS_EVALUATION_WINDOW_SECONDS: i64 = 15 * 60;
pub const COST_HOTSPOT_EVALUATION_WINDOW_SECONDS: i64 = 60 * 60;

pub const MANAGED_RULE_KEYS: &[&str] = &[
    RULE_PROVIDER_OPEN,
    RULE_PROVIDER_DEGRADED,
    RULE_HIGH_ERROR_RATE,
    RULE_HIGH_LATENCY,
    RULE_COST_HOTSPOT,
    RULE_TRANSFORM_DIAGNOSTIC_SPIKE,
    RULE_LOGGING_PIPELINE_DEGRADED,
    RULE_RUNTIME_STATE_BACKEND_DEGRADED,
    RULE_METRICS_UNAVAILABLE,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlertEvaluationWindow {
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub evaluation_window_seconds: i64,
}

impl AlertEvaluationWindow {
    pub fn ending_at(end_time_ms: i64) -> Self {
        Self::ending_at_with_seconds(end_time_ms, ALERT_METRICS_EVALUATION_WINDOW_SECONDS)
    }

    pub fn cost_hotspot_ending_at(end_time_ms: i64) -> Self {
        Self::ending_at_with_seconds(end_time_ms, COST_HOTSPOT_EVALUATION_WINDOW_SECONDS)
    }

    fn ending_at_with_seconds(end_time_ms: i64, window_seconds: i64) -> Self {
        let window_seconds = window_seconds.max(1);
        let window_ms = window_seconds.saturating_mul(1_000);
        Self {
            start_time_ms: end_time_ms.saturating_sub(window_ms),
            end_time_ms,
            evaluation_window_seconds: window_seconds,
        }
    }
}

pub fn attach_evaluation_window(
    candidates: Vec<AlertFireInput>,
    window: AlertEvaluationWindow,
) -> Vec<AlertFireInput> {
    attach_evaluation_windows(candidates, window, window)
}

pub fn attach_evaluation_windows(
    mut candidates: Vec<AlertFireInput>,
    default_window: AlertEvaluationWindow,
    cost_hotspot_window: AlertEvaluationWindow,
) -> Vec<AlertFireInput> {
    for candidate in &mut candidates {
        let window = if candidate.rule_key == RULE_COST_HOTSPOT {
            cost_hotspot_window
        } else {
            default_window
        };
        candidate.details_json = details_with_evaluation_window(&candidate.details_json, window);
    }
    candidates
}

pub fn provider_runtime_rule_candidates(
    items: &[ProviderRuntimeItem],
    rules: &AlertRulesConfig,
) -> Vec<AlertFireInput> {
    let mut candidates = Vec::new();
    for item in items {
        match item.runtime_level {
            ProviderRuntimeLevel::Open => candidates.push(provider_alert(
                RULE_PROVIDER_OPEN,
                AlertSeverity::Critical,
                item,
                "Provider circuit is open",
                format!("Provider {} is currently open", item.provider_name),
                json!({
                    "health_status": "open",
                    "recovery_status": "healthy",
                }),
            )),
            ProviderRuntimeLevel::Degraded => {
                let error_rate = error_rate(item.request_count, item.error_count).unwrap_or(0.0);
                let latency = item.avg_total_latency_ms.unwrap_or(0.0);
                if item.request_count >= rules.provider_degraded_min_requests
                    && (latency >= rules.provider_degraded_latency_ms as f64
                        || error_rate >= rules.provider_degraded_error_rate)
                {
                    candidates.push(provider_alert(
                        RULE_PROVIDER_DEGRADED,
                        AlertSeverity::Warning,
                        item,
                        "Provider runtime is degraded",
                        format!(
                            "Provider {} is degraded with request_count={}, error_rate={:.2}, avg_latency_ms={:.0}",
                            item.provider_name, item.request_count, error_rate, latency
                        ),
                        json!({
                            "request_count": item.request_count,
                            "error_count": item.error_count,
                            "error_rate": error_rate,
                            "avg_total_latency_ms": latency,
                            "min_requests": rules.provider_degraded_min_requests,
                            "error_rate_threshold": rules.provider_degraded_error_rate,
                            "latency_threshold_ms": rules.provider_degraded_latency_ms,
                        }),
                    ));
                }
            }
            _ => {}
        }
    }
    candidates
}

pub fn metrics_rollup_rule_candidates(
    request_aggregates: &[MetricRequestWindowAggregate],
    attempt_aggregates: &[MetricAttemptWindowAggregate],
    rules: &AlertRulesConfig,
) -> Vec<AlertFireInput> {
    let attempt_by_scope = attempt_aggregates
        .iter()
        .map(|item| ((item.scope_type.clone(), item.scope_id.clone()), item))
        .collect::<HashMap<_, _>>();
    let request_by_scope = request_aggregates
        .iter()
        .map(|item| ((item.scope_type.clone(), item.scope_id.clone()), item))
        .collect::<HashMap<_, _>>();
    let mut candidates = Vec::new();

    let mut alert_scopes = BTreeSet::<(String, String)>::new();
    for request in request_aggregates
        .iter()
        .filter(|item| matches!(item.scope_type.as_str(), "global" | "provider"))
    {
        alert_scopes.insert((request.scope_type.clone(), request.scope_id.clone()));
    }
    for attempt in attempt_aggregates
        .iter()
        .filter(|item| matches!(item.scope_type.as_str(), "global" | "provider"))
    {
        alert_scopes.insert((attempt.scope_type.clone(), attempt.scope_id.clone()));
    }

    for scope_key in alert_scopes {
        let request = request_by_scope
            .get(&scope_key)
            .copied()
            .cloned()
            .unwrap_or_else(|| {
                synthetic_request_aggregate_from_attempt(attempt_by_scope[&scope_key])
            });
        let attempt = attempt_by_scope.get(&scope_key).copied();
        if let Some(candidate) = high_error_candidate(&request, attempt, rules) {
            candidates.push(candidate);
        }
        if let Some(candidate) = high_latency_candidate(&request, attempt, rules) {
            candidates.push(candidate);
        }
    }

    for request in request_aggregates {
        if matches!(request.scope_type.as_str(), "global" | "provider_model") {
            if let Some(candidate) = transform_diagnostic_candidate(request, rules) {
                candidates.push(candidate);
            }
        }
    }

    candidates
}

pub fn cost_hotspot_rule_candidates(
    request_aggregates: &[MetricRequestWindowAggregate],
    provider_costs: &HashMap<String, Vec<MetricCostAggregate>>,
    rules: &AlertRulesConfig,
) -> Vec<AlertFireInput> {
    let Some(cost_threshold) = rules.cost_hotspot_amount_nanos else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    for request in request_aggregates {
        if !matches!(
            request.scope_type.as_str(),
            "provider" | "model" | "api_key" | "provider_api_key"
        ) {
            continue;
        }

        for cost in provider_costs
            .get(&scope_key_to_string(&request.scope_type, &request.scope_id))
            .into_iter()
            .flatten()
        {
            if cost.amount_nanos >= cost_threshold {
                candidates.push(cost_hotspot_candidate(request, cost, cost_threshold));
            }
        }
    }

    candidates
}

fn synthetic_request_aggregate_from_attempt(
    attempt: &MetricAttemptWindowAggregate,
) -> MetricRequestWindowAggregate {
    MetricRequestWindowAggregate {
        scope_type: attempt.scope_type.clone(),
        scope_id: attempt.scope_id.clone(),
        scope_label: attempt.scope_label.clone(),
        ..MetricRequestWindowAggregate::default()
    }
}

fn scope_key_to_string(scope_type: &str, scope_id: &str) -> String {
    format!("{scope_type}:{scope_id}")
}

pub fn cost_scope_key(scope_type: &str, scope_id: &str) -> String {
    scope_key_to_string(scope_type, scope_id)
}

#[allow(dead_code)]
pub fn metric_alert_scope_type(scope_type: &str) -> Option<AlertScopeType> {
    match scope_type {
        "global" => Some(AlertScopeType::Global),
        "provider" => Some(AlertScopeType::Provider),
        "model" => Some(AlertScopeType::Model),
        "api_key" => Some(AlertScopeType::ApiKey),
        "provider_api_key" => Some(AlertScopeType::ProviderApiKey),
        "provider_model" => Some(AlertScopeType::ProviderModel),
        _ => None,
    }
}

pub fn logging_pipeline_rule_candidate(
    current: &LogManagerMetricsSnapshot,
    delta: &LogManagerMetricsSnapshot,
    rules: &AlertRulesConfig,
) -> Option<AlertFireInput> {
    let failure_delta = delta.enqueue_failures
        + delta.storage_failures
        + delta.db_failures
        + delta.cleanup_failures
        + delta.compensation_needed
        + delta.channel_full_events;
    let saturated = current.pending >= rules.logging_pending_threshold
        || current.in_flight >= rules.logging_in_flight_threshold;

    if failure_delta == 0 && !saturated {
        return None;
    }

    Some(system_alert(
        RULE_LOGGING_PIPELINE_DEGRADED,
        AlertSeverity::Critical,
        "Logging pipeline degraded",
        "Log manager metrics indicate queue saturation or persistence failures".to_string(),
        json!({
            "current": current,
            "delta": delta,
            "failure_delta": failure_delta,
            "pending_threshold": rules.logging_pending_threshold,
            "in_flight_threshold": rules.logging_in_flight_threshold,
        }),
    ))
}

pub fn runtime_state_backend_rule_candidate(
    status: &RuntimeStateBackendOperatorStatus,
    rules: &AlertRulesConfig,
) -> Option<AlertFireInput> {
    if !rules.runtime_state_backend_degraded_enabled || !status.runtime_degraded {
        return None;
    }

    Some(system_alert(
        RULE_RUNTIME_STATE_BACKEND_DEGRADED,
        AlertSeverity::Critical,
        "Runtime state backend degraded",
        status
            .last_error
            .clone()
            .unwrap_or_else(|| "Runtime state backend is degraded".to_string()),
        json!({
            "deployment_mode": &status.deployment_mode,
            "runtime_configured_backend": &status.runtime_configured_backend,
            "runtime_effective_backend": &status.runtime_effective_backend,
            "runtime_shared": status.runtime_shared,
            "fallback_reason": &status.fallback_reason,
            "last_error": &status.last_error,
            "last_checked_at": status.last_checked_at,
        }),
    ))
}

pub fn metrics_unavailable_rule_candidate(
    config: &MetricsConfig,
    reason: &'static str,
    error: Option<String>,
) -> AlertFireInput {
    let mut details = json!({
        "metrics_enabled": config.enabled,
        "request_log_query_fallback_enabled": config.request_log_query_fallback_enabled,
        "reason": reason,
    });
    if let (Some(object), Some(error)) = (details.as_object_mut(), error) {
        object.insert("error".to_string(), json!(error));
    }

    system_alert(
        RULE_METRICS_UNAVAILABLE,
        AlertSeverity::Critical,
        "Metrics unavailable",
        "Metrics rollups are unavailable for alert evaluation".to_string(),
        details,
    )
}

pub fn log_manager_metrics_delta(
    current: &LogManagerMetricsSnapshot,
    previous: &LogManagerMetricsSnapshot,
) -> LogManagerMetricsSnapshot {
    LogManagerMetricsSnapshot {
        enqueued: current.enqueued.saturating_sub(previous.enqueued),
        processed: current.processed.saturating_sub(previous.processed),
        pending: current.pending,
        in_flight: current.in_flight,
        retries: current.retries.saturating_sub(previous.retries),
        channel_full_events: current
            .channel_full_events
            .saturating_sub(previous.channel_full_events),
        enqueue_failures: current
            .enqueue_failures
            .saturating_sub(previous.enqueue_failures),
        storage_failures: current
            .storage_failures
            .saturating_sub(previous.storage_failures),
        db_failures: current.db_failures.saturating_sub(previous.db_failures),
        cleanup_failures: current
            .cleanup_failures
            .saturating_sub(previous.cleanup_failures),
        compensation_needed: current
            .compensation_needed
            .saturating_sub(previous.compensation_needed),
    }
}

fn high_error_candidate(
    request: &MetricRequestWindowAggregate,
    attempt: Option<&MetricAttemptWindowAggregate>,
    rules: &AlertRulesConfig,
) -> Option<AlertFireInput> {
    let request_errors = request.error_count + request.cancelled_count;
    let request_error_rate = error_rate(request.request_count, request_errors);
    let attempt_errors = attempt.map_or(0, |item| item.error_count);
    let attempt_count = attempt.map_or(0, |item| item.attempt_count);
    let attempt_error_rate = error_rate(attempt_count, attempt_errors);

    let request_triggered = request.request_count >= rules.high_error_min_requests
        && request_error_rate.unwrap_or(0.0) >= rules.high_error_rate;
    let attempt_triggered = attempt_count >= rules.high_error_min_requests
        && attempt_error_rate.unwrap_or(0.0) >= rules.high_error_rate;

    if !request_triggered && !attempt_triggered {
        return None;
    }

    Some(provider_scope_alert(
        RULE_HIGH_ERROR_RATE,
        AlertSeverity::Critical,
        request,
        "High provider error rate",
        format!(
            "Provider {} has request_error_rate={:.2} and attempt_error_rate={:.2}",
            provider_label(request),
            request_error_rate.unwrap_or(0.0),
            attempt_error_rate.unwrap_or(0.0)
        ),
        json!({
            "request_count": request.request_count,
            "request_error_count": request_errors,
            "request_error_rate": request_error_rate,
            "attempt_count": attempt_count,
            "attempt_error_count": attempt_errors,
            "attempt_error_rate": attempt_error_rate,
            "threshold": rules.high_error_rate,
            "min_requests": rules.high_error_min_requests,
        }),
    ))
}

fn high_latency_candidate(
    request: &MetricRequestWindowAggregate,
    attempt: Option<&MetricAttemptWindowAggregate>,
    rules: &AlertRulesConfig,
) -> Option<AlertFireInput> {
    let request_avg = average_or_none(request.total_latency_sum_ms, request.total_latency_count);
    let attempt_avg = attempt
        .and_then(|item| average_or_none(item.total_latency_sum_ms, item.total_latency_count));
    let request_triggered = request.total_latency_count >= rules.high_latency_min_samples
        && request_avg.unwrap_or(0.0) >= rules.high_latency_ms as f64;
    let attempt_triggered = attempt
        .is_some_and(|item| item.total_latency_count >= rules.high_latency_min_samples)
        && attempt_avg.unwrap_or(0.0) >= rules.high_latency_ms as f64;

    if !request_triggered && !attempt_triggered {
        return None;
    }

    Some(provider_scope_alert(
        RULE_HIGH_LATENCY,
        AlertSeverity::Warning,
        request,
        "High provider latency",
        format!(
            "Provider {} has request_avg_latency_ms={:.0} and attempt_avg_latency_ms={:.0}",
            provider_label(request),
            request_avg.unwrap_or(0.0),
            attempt_avg.unwrap_or(0.0)
        ),
        json!({
            "request_latency_sample_count": request.total_latency_count,
            "request_avg_latency_ms": request_avg,
            "attempt_latency_sample_count": attempt.map_or(0, |item| item.total_latency_count),
            "attempt_avg_latency_ms": attempt_avg,
            "threshold_ms": rules.high_latency_ms,
            "min_samples": rules.high_latency_min_samples,
        }),
    ))
}

fn transform_diagnostic_candidate(
    request: &MetricRequestWindowAggregate,
    rules: &AlertRulesConfig,
) -> Option<AlertFireInput> {
    let severe_count =
        request.transform_diagnostic_lossy_major_count + request.transform_diagnostic_reject_count;
    let triggered_by_total =
        request.transform_diagnostic_count >= rules.transform_diagnostic_count_threshold;
    let triggered_by_reject = request.transform_diagnostic_reject_count > 0;
    let triggered_by_lossy_major = request.transform_diagnostic_lossy_major_count
        >= rules.transform_diagnostic_lossy_major_threshold;
    if !triggered_by_total && !triggered_by_reject && !triggered_by_lossy_major {
        return None;
    }

    Some(provider_scope_alert(
        RULE_TRANSFORM_DIAGNOSTIC_SPIKE,
        AlertSeverity::Warning,
        request,
        "Transform diagnostics spike",
        format!(
            "Provider {} emitted {} lossy_major/reject transform diagnostics",
            provider_label(request),
            severe_count
        ),
        json!({
            "transform_diagnostic_count": request.transform_diagnostic_count,
            "lossy_major_count": request.transform_diagnostic_lossy_major_count,
            "reject_count": request.transform_diagnostic_reject_count,
            "severe_count": severe_count,
            "diagnostic_count_threshold": rules.transform_diagnostic_count_threshold,
            "severe_threshold": rules.transform_diagnostic_lossy_major_threshold,
        }),
    ))
}

fn cost_hotspot_candidate(
    request: &MetricRequestWindowAggregate,
    cost: &MetricCostAggregate,
    threshold: i64,
) -> AlertFireInput {
    provider_scope_alert(
        &format!("{}:{}", RULE_COST_HOTSPOT, cost.currency),
        AlertSeverity::Warning,
        request,
        "Provider cost hotspot",
        format!(
            "Provider {} spent {} {} nanos in the evaluation window",
            provider_label(request),
            cost.amount_nanos,
            cost.currency
        ),
        json!({
            "currency": &cost.currency,
            "amount_nanos": cost.amount_nanos,
            "threshold_nanos": threshold,
        }),
    )
}

fn provider_alert(
    rule_key: &'static str,
    severity: AlertSeverity,
    item: &ProviderRuntimeItem,
    title: &str,
    summary: String,
    extra_details: serde_json::Value,
) -> AlertFireInput {
    let mut details = json!({
        "provider_id": item.provider_id,
        "provider_key": &item.provider_key,
        "provider_name": &item.provider_name,
        "runtime_level": &item.runtime_level,
        "request_count": item.request_count,
        "success_count": item.success_count,
        "error_count": item.error_count,
        "success_rate": item.success_rate,
        "avg_total_latency_ms": item.avg_total_latency_ms,
        "last_error_at": item.last_error_at,
        "last_error_summary": &item.last_error_summary,
    });
    merge_json_object(&mut details, extra_details);

    AlertFireInput {
        fingerprint: fingerprint(
            rule_key,
            AlertScopeType::Provider,
            &item.provider_id.to_string(),
        ),
        rule_key: rule_key.to_string(),
        severity,
        scope_type: AlertScopeType::Provider,
        scope_id: item.provider_id.to_string(),
        title: title.to_string(),
        summary,
        details_json: json_string(details),
        metrics_snapshot_json: None,
    }
}

fn provider_scope_alert(
    rule_key: &str,
    severity: AlertSeverity,
    request: &MetricRequestWindowAggregate,
    title: &str,
    summary: String,
    details: serde_json::Value,
) -> AlertFireInput {
    let normalized_rule_key = rule_key.split(':').next().unwrap_or(rule_key);
    let scope_type =
        metric_alert_scope_type(&request.scope_type).unwrap_or(AlertScopeType::Provider);
    AlertFireInput {
        fingerprint: fingerprint(rule_key, scope_type, &request.scope_id),
        rule_key: normalized_rule_key.to_string(),
        severity,
        scope_type,
        scope_id: request.scope_id.clone(),
        title: title.to_string(),
        summary,
        details_json: json_string(details),
        metrics_snapshot_json: Some(json_string(json!({
            "request_count": request.request_count,
            "success_count": request.success_count,
            "error_count": request.error_count,
            "cancelled_count": request.cancelled_count,
            "total_tokens": request.total_tokens,
        }))),
    }
}

fn system_alert(
    rule_key: &'static str,
    severity: AlertSeverity,
    title: &str,
    summary: String,
    details: serde_json::Value,
) -> AlertFireInput {
    AlertFireInput {
        fingerprint: fingerprint(rule_key, AlertScopeType::System, "system"),
        rule_key: rule_key.to_string(),
        severity,
        scope_type: AlertScopeType::System,
        scope_id: "system".to_string(),
        title: title.to_string(),
        summary,
        details_json: json_string(details),
        metrics_snapshot_json: None,
    }
}

fn fingerprint(rule_key: &str, scope_type: AlertScopeType, scope_id: &str) -> String {
    format!("{}:{}:{}", rule_key, scope_type.as_str(), scope_id)
}

fn provider_label(request: &MetricRequestWindowAggregate) -> String {
    request
        .scope_label
        .clone()
        .unwrap_or_else(|| request.scope_id.clone())
}

fn error_rate(request_count: i64, error_count: i64) -> Option<f64> {
    if request_count > 0 {
        Some(error_count as f64 / request_count as f64)
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

fn json_string(value: serde_json::Value) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

fn merge_json_object(target: &mut Value, source: Value) {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
}

fn details_with_evaluation_window(details_json: &str, window: AlertEvaluationWindow) -> String {
    let mut details = serde_json::from_str::<Value>(details_json)
        .unwrap_or_else(|_| json!({ "raw_details": details_json }));
    if !details.is_object() {
        details = json!({ "value": details });
    }

    if let Some(object) = details.as_object_mut() {
        object.insert("window_start_ms".to_string(), json!(window.start_time_ms));
        object.insert("window_end_ms".to_string(), json!(window.end_time_ms));
        object.insert(
            "evaluation_window_seconds".to_string(),
            json!(window.evaluation_window_seconds),
        );
    }

    json_string(details)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::metrics::provider_runtime::{
        ProviderRuntimeHealthStatus, ProviderRuntimeStatusCodeStat,
    };

    #[test]
    fn provider_open_generates_critical_alert() {
        let item = provider_item(7, ProviderRuntimeLevel::Open, 3, 3, None);
        let candidates = provider_runtime_rule_candidates(&[item], &AlertRulesConfig::default());

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].rule_key, RULE_PROVIDER_OPEN);
        assert_eq!(candidates[0].severity, AlertSeverity::Critical);
        assert_eq!(candidates[0].fingerprint, "provider_open:provider:7");
    }

    #[test]
    fn high_error_and_latency_use_request_and_attempt_rollups() {
        let rules = AlertRulesConfig {
            high_error_min_requests: 10,
            high_error_rate: 0.3,
            high_latency_min_samples: 10,
            high_latency_ms: 1_000,
            ..AlertRulesConfig::default()
        };
        let request = request_aggregate(7, 20, 10, 10, 0, 20_000, 20);
        let attempt = attempt_aggregate(7, 30, 12, 45_000, 30);
        let candidates = metrics_rollup_rule_candidates(&[request], &[attempt], &rules);
        let rule_keys = candidates
            .iter()
            .map(|item| item.rule_key.as_str())
            .collect::<Vec<_>>();

        assert!(rule_keys.contains(&RULE_HIGH_ERROR_RATE));
        assert!(rule_keys.contains(&RULE_HIGH_LATENCY));
        let high_error = candidates
            .iter()
            .find(|item| item.rule_key == RULE_HIGH_ERROR_RATE)
            .expect("high error candidate");
        assert_eq!(high_error.severity, AlertSeverity::Critical);
    }

    #[test]
    fn attempt_only_provider_high_error_generates_provider_alert() {
        let rules = AlertRulesConfig {
            high_error_min_requests: 10,
            high_error_rate: 0.3,
            ..AlertRulesConfig::default()
        };
        let attempt = attempt_aggregate(7, 20, 20, 0, 0);

        let candidates = metrics_rollup_rule_candidates(&[], &[attempt], &rules);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].rule_key, RULE_HIGH_ERROR_RATE);
        assert_eq!(candidates[0].scope_type, AlertScopeType::Provider);
        assert_eq!(candidates[0].fingerprint, "high_error_rate:provider:7");
        let details: serde_json::Value =
            serde_json::from_str(&candidates[0].details_json).expect("details should parse");
        assert_eq!(
            details
                .get("attempt_error_count")
                .and_then(|value| value.as_i64()),
            Some(20)
        );
    }

    #[test]
    fn global_high_error_generates_global_critical_alert() {
        let rules = AlertRulesConfig {
            high_error_min_requests: 10,
            high_error_rate: 0.3,
            ..AlertRulesConfig::default()
        };
        let mut request = request_aggregate(0, 20, 10, 10, 0, 0, 0);
        request.scope_type = "global".to_string();
        request.scope_id = "global".to_string();
        request.scope_label = None;

        let candidates = metrics_rollup_rule_candidates(&[request], &[], &rules);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].rule_key, RULE_HIGH_ERROR_RATE);
        assert_eq!(candidates[0].severity, AlertSeverity::Critical);
        assert_eq!(candidates[0].scope_type, AlertScopeType::Global);
        assert_eq!(candidates[0].fingerprint, "high_error_rate:global:global");
    }

    #[test]
    fn provider_degraded_requires_min_requests_before_rate_or_latency_trigger() {
        let rules = AlertRulesConfig {
            provider_degraded_min_requests: 5,
            provider_degraded_error_rate: 0.2,
            provider_degraded_latency_ms: 10_000,
            ..AlertRulesConfig::default()
        };
        let too_few = provider_item(7, ProviderRuntimeLevel::Degraded, 4, 4, Some(20_000.0));
        let enough = provider_item(7, ProviderRuntimeLevel::Degraded, 5, 1, Some(20_000.0));

        assert!(provider_runtime_rule_candidates(&[too_few], &rules).is_empty());
        assert_eq!(
            provider_runtime_rule_candidates(&[enough], &rules)[0].rule_key,
            RULE_PROVIDER_DEGRADED
        );
    }

    #[test]
    fn transform_diagnostic_spike_uses_lossy_major_or_reject_or_total_counts() {
        let mut request = request_aggregate(7, 20, 20, 0, 0, 100, 1);
        request.scope_type = "provider_model".to_string();
        request.scope_id = "7:11".to_string();
        request.transform_diagnostic_count = 1;
        request.transform_diagnostic_reject_count = 1;

        let candidates =
            metrics_rollup_rule_candidates(&[request], &[], &AlertRulesConfig::default());

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].rule_key, RULE_TRANSFORM_DIAGNOSTIC_SPIKE);
    }

    #[test]
    fn cost_hotspot_covers_all_required_scopes_and_currency_fingerprint() {
        let rules = AlertRulesConfig {
            cost_hotspot_amount_nanos: Some(100),
            ..AlertRulesConfig::default()
        };
        let mut rows = Vec::new();
        for (scope_type, scope_id) in [
            ("provider", "7"),
            ("model", "11"),
            ("api_key", "13"),
            ("provider_api_key", "17"),
        ] {
            let mut row = request_aggregate(0, 1, 1, 0, 0, 0, 0);
            row.scope_type = scope_type.to_string();
            row.scope_id = scope_id.to_string();
            rows.push(row);
        }
        let costs = rows
            .iter()
            .map(|row| {
                (
                    cost_scope_key(&row.scope_type, &row.scope_id),
                    vec![MetricCostAggregate {
                        currency: "USD".to_string(),
                        amount_nanos: 150,
                    }],
                )
            })
            .collect::<HashMap<_, _>>();

        let candidates = cost_hotspot_rule_candidates(&rows, &costs, &rules);

        assert_eq!(candidates.len(), 4);
        assert!(
            candidates
                .iter()
                .any(|item| item.fingerprint == "cost_hotspot:USD:model:11")
        );
        assert!(
            candidates
                .iter()
                .any(|item| item.fingerprint == "cost_hotspot:USD:api_key:13")
        );
    }

    #[test]
    fn cost_hotspot_details_use_one_hour_evaluation_window() {
        let rules = AlertRulesConfig {
            cost_hotspot_amount_nanos: Some(100),
            ..AlertRulesConfig::default()
        };
        let request = request_aggregate(7, 1, 1, 0, 0, 0, 0);
        let costs = HashMap::from([(
            cost_scope_key(&request.scope_type, &request.scope_id),
            vec![MetricCostAggregate {
                currency: "USD".to_string(),
                amount_nanos: 150,
            }],
        )]);
        let candidates = cost_hotspot_rule_candidates(&[request], &costs, &rules);
        let candidates = attach_evaluation_windows(
            candidates,
            AlertEvaluationWindow::ending_at(3_600_000),
            AlertEvaluationWindow::cost_hotspot_ending_at(3_600_000),
        );

        let details: serde_json::Value =
            serde_json::from_str(&candidates[0].details_json).expect("details should parse");
        assert_eq!(
            details
                .get("evaluation_window_seconds")
                .and_then(serde_json::Value::as_i64),
            Some(COST_HOTSPOT_EVALUATION_WINDOW_SECONDS)
        );
        assert_eq!(
            details
                .get("window_start_ms")
                .and_then(serde_json::Value::as_i64),
            Some(0)
        );
    }

    #[test]
    fn logging_pipeline_delta_covers_enqueue_and_cleanup_failures() {
        let previous = LogManagerMetricsSnapshot {
            enqueue_failures: 1,
            cleanup_failures: 2,
            ..LogManagerMetricsSnapshot::default()
        };
        let current = LogManagerMetricsSnapshot {
            enqueue_failures: 3,
            cleanup_failures: 5,
            ..LogManagerMetricsSnapshot::default()
        };
        let delta = log_manager_metrics_delta(&current, &previous);
        let candidate =
            logging_pipeline_rule_candidate(&current, &delta, &AlertRulesConfig::default())
                .expect("failure delta should trigger alert");

        assert_eq!(delta.enqueue_failures, 2);
        assert_eq!(delta.cleanup_failures, 3);
        assert_eq!(candidate.rule_key, RULE_LOGGING_PIPELINE_DEGRADED);
    }

    #[test]
    fn metrics_unavailable_generates_system_critical_alert() {
        let candidate = metrics_unavailable_rule_candidate(
            &crate::config::MetricsConfig {
                enabled: false,
                request_log_query_fallback_enabled: true,
                ..crate::config::MetricsConfig::default()
            },
            "metrics_disabled",
            None,
        );

        assert_eq!(candidate.rule_key, RULE_METRICS_UNAVAILABLE);
        assert_eq!(candidate.severity, AlertSeverity::Critical);
        assert_eq!(candidate.scope_type, AlertScopeType::System);
        assert_eq!(candidate.fingerprint, "metrics_unavailable:system:system");
        let details: serde_json::Value =
            serde_json::from_str(&candidate.details_json).expect("details should parse");
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
            details.get("reason").and_then(serde_json::Value::as_str),
            Some("metrics_disabled")
        );
    }

    fn request_aggregate(
        provider_id: i64,
        request_count: i64,
        success_count: i64,
        error_count: i64,
        cancelled_count: i64,
        latency_sum: i64,
        latency_count: i64,
    ) -> MetricRequestWindowAggregate {
        MetricRequestWindowAggregate {
            scope_type: "provider".to_string(),
            scope_id: provider_id.to_string(),
            scope_label: Some(format!("provider-{provider_id}")),
            request_count,
            success_count,
            error_count,
            cancelled_count,
            total_latency_sum_ms: latency_sum,
            total_latency_count: latency_count,
            ..MetricRequestWindowAggregate::default()
        }
    }

    fn attempt_aggregate(
        provider_id: i64,
        attempt_count: i64,
        error_count: i64,
        latency_sum: i64,
        latency_count: i64,
    ) -> MetricAttemptWindowAggregate {
        MetricAttemptWindowAggregate {
            scope_type: "provider".to_string(),
            scope_id: provider_id.to_string(),
            attempt_count,
            error_count,
            total_latency_sum_ms: latency_sum,
            total_latency_count: latency_count,
            ..MetricAttemptWindowAggregate::default()
        }
    }

    fn provider_item(
        provider_id: i64,
        runtime_level: ProviderRuntimeLevel,
        request_count: i64,
        error_count: i64,
        avg_total_latency_ms: Option<f64>,
    ) -> ProviderRuntimeItem {
        ProviderRuntimeItem {
            provider_id,
            provider_key: format!("provider-{provider_id}"),
            provider_name: format!("Provider {provider_id}"),
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
            last_failure_at: None,
            last_recovered_at: None,
            last_error: None,
            runtime_state_backend_degraded: false,
            runtime_state_backend_error: None,
            request_count,
            success_count: request_count.saturating_sub(error_count),
            error_count,
            success_rate: error_rate(request_count, request_count - error_count),
            avg_first_byte_ms: None,
            avg_total_latency_ms,
            last_request_at: None,
            last_success_at: None,
            last_error_at: None,
            last_error_summary: None,
            status_code_breakdown: vec![ProviderRuntimeStatusCodeStat {
                status_code: 500,
                count: error_count,
            }],
            total_cost: Vec::new(),
            sort_score: 0.0,
        }
    }
}
