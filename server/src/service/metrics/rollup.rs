use crate::{
    database::{
        metrics::{
            MetricCostRollupMinute, MetricHttpStatusRollupMinute, MetricRequestRollupMinute,
        },
        request_log::RequestLog,
    },
    schema::enum_def::{RequestStatus, UpstreamProfileType},
};

use super::types::{MetricsScope, MetricsScopeType};

#[derive(Debug, Clone, Default)]
pub struct MetricsRollupDeltas {
    pub request_rollups: Vec<MetricRequestRollupMinute>,
    pub http_status_rollups: Vec<MetricHttpStatusRollupMinute>,
    pub cost_rollups: Vec<MetricCostRollupMinute>,
}

pub fn build_rollup_deltas(
    request_log: &RequestLog,
    bucket_seconds: u64,
    now_ms: i64,
) -> MetricsRollupDeltas {
    let bucket_ms = (bucket_seconds.max(1) as i64).saturating_mul(1_000);
    let bucket_start_ms = request_log.request_received_at.div_euclid(bucket_ms) * bucket_ms;
    let scopes = request_scopes(request_log);
    let request_rollups = scopes
        .iter()
        .map(|scope| request_rollup_delta(request_log, scope, bucket_start_ms, now_ms))
        .collect();
    let http_status_rollups = request_log
        .upstream_http_status
        .into_iter()
        .flat_map(|http_status| {
            scopes
                .iter()
                .map(move |scope| MetricHttpStatusRollupMinute {
                    bucket_start_ms,
                    scope_type: scope.scope_type.as_str().to_string(),
                    scope_id: scope.scope_id.clone(),
                    http_status,
                    count: 1,
                    created_at: now_ms,
                    updated_at: now_ms,
                })
        })
        .collect();
    let mut cost_rollups = Vec::new();
    if let (Some(amount), Some(currency)) = (
        request_log
            .estimated_cost_nanos
            .filter(|amount| *amount > 0),
        request_log
            .estimated_cost_currency
            .as_deref()
            .map(str::trim)
            .filter(|currency| !currency.is_empty()),
    ) {
        for scope in &scopes {
            cost_rollups.push(MetricCostRollupMinute {
                bucket_start_ms,
                scope_type: scope.scope_type.as_str().to_string(),
                scope_id: scope.scope_id.clone(),
                currency: currency.to_string(),
                amount_nanos: amount,
                created_at: now_ms,
                updated_at: now_ms,
            });
        }
    }
    MetricsRollupDeltas {
        request_rollups,
        http_status_rollups,
        cost_rollups,
    }
}

fn id_scope(scope_type: MetricsScopeType, id: i64, label: Option<String>) -> MetricsScope {
    MetricsScope {
        scope_type,
        scope_id: id.to_string(),
        scope_label: label,
    }
}

fn request_scopes(request_log: &RequestLog) -> Vec<MetricsScope> {
    let mut scopes = vec![MetricsScope::global()];
    if let Some(provider_id) = request_log.provider_id {
        scopes.push(id_scope(
            MetricsScopeType::Provider,
            provider_id,
            request_log.provider_name_snapshot.clone(),
        ));
    }
    if let Some(source_id) = request_log.source_id {
        scopes.push(id_scope(
            MetricsScopeType::Source,
            source_id,
            request_log
                .source_profile_type_snapshot
                .map(upstream_profile_wire_name),
        ));
    }
    if let Some(model_id) = request_log.model_id {
        scopes.push(id_scope(
            MetricsScopeType::Model,
            model_id,
            request_log.model_name_snapshot.clone(),
        ));
    }
    scopes.push(id_scope(
        MetricsScopeType::ApiKey,
        request_log.api_key_id,
        None,
    ));
    if let Some(provider_api_key_id) = request_log.provider_api_key_id {
        scopes.push(id_scope(
            MetricsScopeType::ProviderApiKey,
            provider_api_key_id,
            request_log.provider_key_snapshot.clone(),
        ));
    }
    if let (Some(provider_id), Some(model_id)) = (request_log.provider_id, request_log.model_id) {
        scopes.push(MetricsScope {
            scope_type: MetricsScopeType::ProviderModel,
            scope_id: format!("{provider_id}:{model_id}"),
            scope_label: match (
                request_log.provider_name_snapshot.as_deref(),
                request_log.model_name_snapshot.as_deref(),
            ) {
                (Some(provider), Some(model)) => Some(format!("{provider} / {model}")),
                _ => None,
            },
        });
    }
    scopes
}

fn upstream_profile_wire_name(profile: UpstreamProfileType) -> String {
    serde_json::to_value(profile)
        .expect("upstream profile type serialization is infallible")
        .as_str()
        .expect("upstream profile type serializes as a string")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::request_scopes;
    use crate::{
        database::request_log::RequestLog, schema::enum_def::UpstreamProfileType,
        service::metrics::types::MetricsScopeType,
    };

    #[test]
    fn request_scopes_preserve_provider_and_add_selected_source() {
        let request_log = RequestLog {
            api_key_id: 1,
            provider_id: Some(2),
            provider_name_snapshot: Some("Logical Provider".to_string()),
            source_id: Some(3),
            source_profile_type_snapshot: Some(
                crate::schema::enum_def::UpstreamProfileType::Openai,
            ),
            ..RequestLog::default()
        };

        let scopes = request_scopes(&request_log);
        assert!(scopes.iter().any(|scope| {
            scope.scope_type == MetricsScopeType::Provider
                && scope.scope_id == "2"
                && scope.scope_label.as_deref() == Some("Logical Provider")
        }));
        assert!(scopes.iter().any(|scope| {
            scope.scope_type == MetricsScopeType::Source
                && scope.scope_id == "3"
                && scope.scope_label.as_deref() == Some("OPENAI")
        }));
    }

    #[test]
    fn request_scopes_use_wire_names_for_compound_profiles() {
        for (profile, expected) in [
            (UpstreamProfileType::VertexOpenai, "VERTEX_OPENAI"),
            (UpstreamProfileType::GeminiOpenai, "GEMINI_OPENAI"),
        ] {
            let request_log = RequestLog {
                api_key_id: 1,
                source_id: Some(3),
                source_profile_type_snapshot: Some(profile),
                ..RequestLog::default()
            };

            let source_scope = request_scopes(&request_log)
                .into_iter()
                .find(|scope| scope.scope_type == MetricsScopeType::Source)
                .expect("source scope should be present");
            assert_eq!(source_scope.scope_label.as_deref(), Some(expected));
        }
    }
}

fn request_rollup_delta(
    request_log: &RequestLog,
    scope: &MetricsScope,
    bucket_start_ms: i64,
    now_ms: i64,
) -> MetricRequestRollupMinute {
    let time_to_first_response_body = positive_duration_ms(
        request_log.upstream_request_sent_at,
        request_log.first_response_body_at,
    );
    let ttft = request_log.is_stream.then(|| {
        positive_duration_ms(
            request_log.upstream_request_sent_at,
            request_log.first_token_at,
        )
    });
    let ttft = ttft.flatten();
    let total_latency = positive_duration_ms(
        request_log.upstream_request_sent_at,
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
        time_to_first_response_body_sum_ms: time_to_first_response_body.unwrap_or_default(),
        time_to_first_response_body_count: i64::from(time_to_first_response_body.is_some()),
        ttft_sum_ms: ttft.unwrap_or_default(),
        ttft_count: i64::from(ttft.is_some()),
        total_latency_sum_ms: total_latency.unwrap_or_default(),
        total_latency_count: i64::from(total_latency.is_some()),
        input_tokens: i64::from(request_log.total_input_tokens.unwrap_or_default().max(0)),
        output_tokens: i64::from(request_log.total_output_tokens.unwrap_or_default().max(0)),
        reasoning_tokens: i64::from(request_log.reasoning_tokens.unwrap_or_default().max(0)),
        total_tokens: i64::from(request_log.total_tokens.unwrap_or_default().max(0)),
        created_at: now_ms,
        updated_at: now_ms,
    }
}

fn positive_duration_ms(start_ms: Option<i64>, end_ms: Option<i64>) -> Option<i64> {
    let duration = end_ms? - start_ms?;
    (duration >= 0).then_some(duration)
}
