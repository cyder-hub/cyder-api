use crate::database::{
    metrics::{
        MetricAttemptRollupMinute, MetricCostRollupMinute, MetricHttpStatusRollupMinute,
        MetricRequestRollupMinute,
    },
    request_attempt::RequestAttempt,
    request_log::RequestLog,
};
use crate::schema::enum_def::{RequestAttemptStatus, RequestStatus, SchedulerAction};

use super::types::{MetricsScope, MetricsScopeType};

#[derive(Debug, Clone, Default)]
pub struct MetricsRollupDeltas {
    pub request_rollups: Vec<MetricRequestRollupMinute>,
    pub attempt_rollups: Vec<MetricAttemptRollupMinute>,
    pub http_status_rollups: Vec<MetricHttpStatusRollupMinute>,
    pub cost_rollups: Vec<MetricCostRollupMinute>,
}

pub fn build_rollup_deltas(
    request_log: &RequestLog,
    attempts: &[RequestAttempt],
    bucket_seconds: u64,
    now_ms: i64,
) -> MetricsRollupDeltas {
    let bucket_ms = (bucket_seconds.max(1) as i64).saturating_mul(1_000);
    let request_bucket = bucket_start(request_log.request_received_at, bucket_ms);
    let request_scopes = request_scopes(request_log);
    let request_rollups = request_scopes
        .iter()
        .map(|scope| request_rollup_delta(request_log, scope, request_bucket, now_ms))
        .collect::<Vec<_>>();

    let mut cost_rollups = Vec::new();
    if let Some(cost) = request_cost_delta(request_log, request_bucket, now_ms) {
        for scope in &request_scopes {
            cost_rollups.push(cost_rollup_delta(
                "request",
                scope,
                &cost.0,
                cost.1,
                request_bucket,
                now_ms,
            ));
        }
    }

    let mut attempt_rollups = Vec::new();
    let mut http_status_rollups = Vec::new();
    for attempt in attempts {
        let attempt_bucket = bucket_start(
            attempt
                .started_at
                .unwrap_or(request_log.request_received_at),
            bucket_ms,
        );
        let attempt_scopes = attempt_scopes(request_log, attempt);
        for scope in &attempt_scopes {
            attempt_rollups.push(attempt_rollup_delta(attempt, scope, attempt_bucket, now_ms));
            if let Some(http_status) = attempt.http_status {
                http_status_rollups.push(MetricHttpStatusRollupMinute {
                    bucket_start_ms: attempt_bucket,
                    scope_type: scope.scope_type.as_str().to_string(),
                    scope_id: scope.scope_id.clone(),
                    http_status,
                    count: 1,
                    created_at: now_ms,
                    updated_at: now_ms,
                });
            }
        }
        if let Some(cost) = attempt_cost_delta(attempt, attempt_bucket, now_ms) {
            for scope in &attempt_scopes {
                cost_rollups.push(cost_rollup_delta(
                    "attempt",
                    scope,
                    &cost.0,
                    cost.1,
                    attempt_bucket,
                    now_ms,
                ));
            }
        }
    }

    MetricsRollupDeltas {
        request_rollups,
        attempt_rollups,
        http_status_rollups,
        cost_rollups,
    }
}

fn bucket_start(timestamp_ms: i64, bucket_ms: i64) -> i64 {
    timestamp_ms.div_euclid(bucket_ms) * bucket_ms
}

fn id_scope(scope_type: MetricsScopeType, id: i64, label: Option<String>) -> MetricsScope {
    MetricsScope {
        scope_type,
        scope_id: id.to_string(),
        scope_label: label,
    }
}

fn provider_model_scope(
    provider_id: i64,
    model_id: i64,
    provider_label: Option<&str>,
    model_label: Option<&str>,
) -> MetricsScope {
    let label = match (provider_label, model_label) {
        (Some(provider), Some(model)) => Some(format!("{provider} / {model}")),
        (Some(provider), None) => Some(provider.to_string()),
        (None, Some(model)) => Some(model.to_string()),
        (None, None) => None,
    };
    MetricsScope {
        scope_type: MetricsScopeType::ProviderModel,
        scope_id: format!("{provider_id}:{model_id}"),
        scope_label: label,
    }
}

fn request_scopes(request_log: &RequestLog) -> Vec<MetricsScope> {
    let mut scopes = vec![MetricsScope::global()];
    if let Some(provider_id) = request_log.final_provider_id {
        scopes.push(id_scope(
            MetricsScopeType::Provider,
            provider_id,
            request_log.final_provider_name_snapshot.clone(),
        ));
    }
    if let Some(model_id) = request_log.final_model_id {
        scopes.push(id_scope(
            MetricsScopeType::Model,
            model_id,
            request_log.final_model_name_snapshot.clone(),
        ));
    }
    scopes.push(id_scope(
        MetricsScopeType::ApiKey,
        request_log.api_key_id,
        None,
    ));
    if let Some(provider_api_key_id) = request_log.final_provider_api_key_id {
        scopes.push(id_scope(
            MetricsScopeType::ProviderApiKey,
            provider_api_key_id,
            request_log.final_provider_key_snapshot.clone(),
        ));
    }
    if let (Some(provider_id), Some(model_id)) =
        (request_log.final_provider_id, request_log.final_model_id)
    {
        scopes.push(provider_model_scope(
            provider_id,
            model_id,
            request_log.final_provider_name_snapshot.as_deref(),
            request_log.final_model_name_snapshot.as_deref(),
        ));
    }
    scopes
}

fn attempt_scopes(request_log: &RequestLog, attempt: &RequestAttempt) -> Vec<MetricsScope> {
    let mut scopes = vec![MetricsScope::global()];
    if let Some(provider_id) = attempt.provider_id {
        scopes.push(id_scope(
            MetricsScopeType::Provider,
            provider_id,
            attempt.provider_name_snapshot.clone(),
        ));
    }
    if let Some(model_id) = attempt.model_id {
        scopes.push(id_scope(
            MetricsScopeType::Model,
            model_id,
            attempt.model_name_snapshot.clone(),
        ));
    }
    scopes.push(id_scope(
        MetricsScopeType::ApiKey,
        request_log.api_key_id,
        None,
    ));
    if let Some(provider_api_key_id) = attempt.provider_api_key_id {
        scopes.push(id_scope(
            MetricsScopeType::ProviderApiKey,
            provider_api_key_id,
            attempt.provider_key_snapshot.clone(),
        ));
    }
    if let (Some(provider_id), Some(model_id)) = (attempt.provider_id, attempt.model_id) {
        scopes.push(provider_model_scope(
            provider_id,
            model_id,
            attempt.provider_name_snapshot.as_deref(),
            attempt.model_name_snapshot.as_deref(),
        ));
    }
    scopes
}

fn request_rollup_delta(
    request_log: &RequestLog,
    scope: &MetricsScope,
    bucket_start_ms: i64,
    now_ms: i64,
) -> MetricRequestRollupMinute {
    let first_byte_latency = positive_duration_ms(
        request_log.first_attempt_started_at,
        request_log.response_started_to_client_at,
    );
    let total_latency = positive_duration_ms(
        request_log.first_attempt_started_at,
        request_log.completed_at,
    );

    MetricRequestRollupMinute {
        bucket_start_ms,
        scope_type: scope.scope_type.as_str().to_string(),
        scope_id: scope.scope_id.clone(),
        scope_label: scope.scope_label.clone(),
        request_count: 1,
        success_count: i64::from(matches!(request_log.overall_status, RequestStatus::Success)),
        error_count: i64::from(matches!(request_log.overall_status, RequestStatus::Error)),
        cancelled_count: i64::from(matches!(
            request_log.overall_status,
            RequestStatus::Cancelled
        )),
        retry_count: i64::from(request_log.retry_count.max(0)),
        fallback_count: i64::from(request_log.fallback_count.max(0)),
        first_byte_latency_sum_ms: first_byte_latency.unwrap_or_default(),
        first_byte_latency_count: i64::from(first_byte_latency.is_some()),
        total_latency_sum_ms: total_latency.unwrap_or_default(),
        total_latency_count: i64::from(total_latency.is_some()),
        input_tokens: i64::from(request_log.total_input_tokens.unwrap_or_default().max(0)),
        output_tokens: i64::from(request_log.total_output_tokens.unwrap_or_default().max(0)),
        reasoning_tokens: i64::from(request_log.reasoning_tokens.unwrap_or_default().max(0)),
        total_tokens: i64::from(request_log.total_tokens.unwrap_or_default().max(0)),
        transform_diagnostic_count: i64::from(request_log.transform_diagnostic_count.max(0)),
        transform_diagnostic_lossy_major_count: i64::from(
            request_log
                .transform_diagnostic_max_loss_level
                .as_deref()
                .is_some_and(|level| level == "lossy_major"),
        ),
        transform_diagnostic_reject_count: i64::from(
            request_log
                .transform_diagnostic_max_loss_level
                .as_deref()
                .is_some_and(|level| level == "reject"),
        ),
        created_at: now_ms,
        updated_at: now_ms,
    }
}

fn attempt_rollup_delta(
    attempt: &RequestAttempt,
    scope: &MetricsScope,
    bucket_start_ms: i64,
    now_ms: i64,
) -> MetricAttemptRollupMinute {
    let first_byte_latency = positive_duration_ms(attempt.started_at, attempt.first_byte_at);
    let total_latency = positive_duration_ms(attempt.started_at, attempt.completed_at);

    MetricAttemptRollupMinute {
        bucket_start_ms,
        scope_type: scope.scope_type.as_str().to_string(),
        scope_id: scope.scope_id.clone(),
        scope_label: scope.scope_label.clone(),
        attempt_count: 1,
        success_count: i64::from(matches!(
            attempt.attempt_status,
            RequestAttemptStatus::Success
        )),
        error_count: i64::from(matches!(
            attempt.attempt_status,
            RequestAttemptStatus::Error | RequestAttemptStatus::Cancelled
        )),
        skipped_count: i64::from(matches!(
            attempt.attempt_status,
            RequestAttemptStatus::Skipped
        )),
        retry_same_candidate_count: i64::from(matches!(
            attempt.scheduler_action,
            SchedulerAction::RetrySameCandidate
        )),
        fallback_next_candidate_count: i64::from(matches!(
            attempt.scheduler_action,
            SchedulerAction::FallbackNextCandidate
        )),
        fail_fast_count: i64::from(matches!(
            attempt.scheduler_action,
            SchedulerAction::FailFast
        )),
        first_byte_latency_sum_ms: first_byte_latency.unwrap_or_default(),
        first_byte_latency_count: i64::from(first_byte_latency.is_some()),
        total_latency_sum_ms: total_latency.unwrap_or_default(),
        total_latency_count: i64::from(total_latency.is_some()),
        input_tokens: i64::from(attempt.total_input_tokens.unwrap_or_default().max(0)),
        output_tokens: i64::from(attempt.total_output_tokens.unwrap_or_default().max(0)),
        reasoning_tokens: i64::from(attempt.reasoning_tokens.unwrap_or_default().max(0)),
        total_tokens: i64::from(attempt.total_tokens.unwrap_or_default().max(0)),
        created_at: now_ms,
        updated_at: now_ms,
    }
}

fn cost_rollup_delta(
    metric_kind: &str,
    scope: &MetricsScope,
    currency: &str,
    amount_nanos: i64,
    bucket_start_ms: i64,
    now_ms: i64,
) -> MetricCostRollupMinute {
    MetricCostRollupMinute {
        bucket_start_ms,
        metric_kind: metric_kind.to_string(),
        scope_type: scope.scope_type.as_str().to_string(),
        scope_id: scope.scope_id.clone(),
        currency: currency.to_string(),
        amount_nanos,
        created_at: now_ms,
        updated_at: now_ms,
    }
}

fn request_cost_delta(
    request_log: &RequestLog,
    _bucket_start_ms: i64,
    _now_ms: i64,
) -> Option<(String, i64)> {
    let amount = request_log.estimated_cost_nanos?;
    let currency = request_log.estimated_cost_currency.as_deref()?.trim();
    if amount > 0 && !currency.is_empty() {
        Some((currency.to_string(), amount))
    } else {
        None
    }
}

fn attempt_cost_delta(
    attempt: &RequestAttempt,
    _bucket_start_ms: i64,
    _now_ms: i64,
) -> Option<(String, i64)> {
    let amount = attempt.estimated_cost_nanos?;
    let currency = attempt.estimated_cost_currency.as_deref()?.trim();
    if amount > 0 && !currency.is_empty() {
        Some((currency.to_string(), amount))
    } else {
        None
    }
}

fn positive_duration_ms(start_ms: Option<i64>, end_ms: Option<i64>) -> Option<i64> {
    let duration = end_ms? - start_ms?;
    (duration >= 0).then_some(duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::enum_def::{LlmApiType, StorageType};

    fn request_log() -> RequestLog {
        RequestLog {
            id: 100,
            api_key_id: 9,
            requested_model_name: Some("gpt".to_string()),
            base_requested_model_name: None,
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            resolved_name_scope: None,
            resolved_route_id: None,
            resolved_route_name: None,
            user_api_type: LlmApiType::Openai,
            overall_status: RequestStatus::Success,
            final_error_code: None,
            final_error_message: None,
            attempt_count: 2,
            retry_count: 1,
            fallback_count: 1,
            request_received_at: 65_000,
            first_attempt_started_at: Some(65_100),
            response_started_to_client_at: Some(65_300),
            completed_at: Some(65_900),
            is_stream: false,
            client_ip: None,
            final_attempt_id: Some(2),
            final_provider_id: Some(7),
            final_provider_api_key_id: Some(8),
            final_model_id: Some(11),
            final_provider_key_snapshot: Some("pk".to_string()),
            final_provider_name_snapshot: Some("Provider".to_string()),
            final_model_name_snapshot: Some("Model".to_string()),
            final_real_model_name_snapshot: None,
            final_llm_api_type: Some(LlmApiType::Openai),
            estimated_cost_nanos: Some(100),
            estimated_cost_currency: Some("USD".to_string()),
            cost_catalog_id: None,
            cost_catalog_version_id: None,
            cost_snapshot_json: None,
            total_input_tokens: Some(10),
            total_output_tokens: Some(20),
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: Some(3),
            total_tokens: Some(33),
            has_transform_diagnostics: true,
            transform_diagnostic_count: 2,
            transform_diagnostic_max_loss_level: Some("lossy_major".to_string()),
            bundle_version: None,
            bundle_storage_type: Some(StorageType::FileSystem),
            bundle_storage_key: None,
            created_at: 65_900,
            updated_at: 65_900,
        }
    }

    fn attempt() -> RequestAttempt {
        RequestAttempt {
            id: 2,
            request_log_id: 100,
            attempt_index: 1,
            candidate_position: 1,
            provider_id: Some(7),
            provider_api_key_id: Some(8),
            model_id: Some(11),
            provider_key_snapshot: Some("pk".to_string()),
            provider_name_snapshot: Some("Provider".to_string()),
            model_name_snapshot: Some("Model".to_string()),
            real_model_name_snapshot: None,
            llm_api_type: Some(LlmApiType::Openai),
            attempt_status: RequestAttemptStatus::Error,
            scheduler_action: SchedulerAction::FallbackNextCandidate,
            error_code: Some("upstream_error".to_string()),
            error_message: None,
            request_uri: None,
            request_headers_json: None,
            response_headers_json: None,
            http_status: Some(500),
            started_at: Some(65_200),
            first_byte_at: None,
            completed_at: Some(65_700),
            response_started_to_client: false,
            backoff_ms: None,
            applied_request_patch_ids_json: None,
            request_patch_summary_json: None,
            estimated_cost_nanos: Some(25),
            estimated_cost_currency: Some("USD".to_string()),
            cost_catalog_version_id: None,
            total_input_tokens: Some(4),
            total_output_tokens: Some(5),
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: Some(1),
            total_tokens: Some(10),
            llm_request_blob_id: None,
            llm_request_patch_id: None,
            llm_response_blob_id: None,
            llm_response_capture_state: None,
            created_at: 65_700,
            updated_at: 65_700,
        }
    }

    #[test]
    fn build_rollup_deltas_covers_request_and_attempt_scopes() {
        let deltas = build_rollup_deltas(&request_log(), &[attempt()], 60, 70_000);

        assert_eq!(deltas.request_rollups.len(), 6);
        assert_eq!(deltas.attempt_rollups.len(), 6);
        assert_eq!(deltas.http_status_rollups.len(), 6);
        assert_eq!(deltas.cost_rollups.len(), 12);
        assert!(
            deltas
                .request_rollups
                .iter()
                .any(|row| row.scope_type == "provider_model" && row.scope_id == "7:11")
        );
        let provider = deltas
            .request_rollups
            .iter()
            .find(|row| row.scope_type == "provider" && row.scope_id == "7")
            .expect("provider request row");
        assert_eq!(provider.bucket_start_ms, 60_000);
        assert_eq!(provider.request_count, 1);
        assert_eq!(provider.success_count, 1);
        assert_eq!(provider.retry_count, 1);
        assert_eq!(provider.fallback_count, 1);
        assert_eq!(provider.first_byte_latency_sum_ms, 200);
        assert_eq!(provider.total_latency_sum_ms, 800);
        assert_eq!(provider.transform_diagnostic_lossy_major_count, 1);

        let attempt = deltas
            .attempt_rollups
            .iter()
            .find(|row| row.scope_type == "provider" && row.scope_id == "7")
            .expect("provider attempt row");
        assert_eq!(attempt.error_count, 1);
        assert_eq!(attempt.fallback_next_candidate_count, 1);
        assert_eq!(attempt.total_latency_sum_ms, 500);
    }
}
