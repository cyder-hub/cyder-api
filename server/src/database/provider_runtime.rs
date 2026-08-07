use diesel::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

use crate::database::{DbConnection, DbResult, get_connection};
use crate::schema::enum_def::RequestStatus;

#[derive(Queryable, Debug, Clone)]
pub struct RequestLogEntryForProviderRuntime {
    pub provider_id: i64,
    pub source_id: i64,
    pub request_received_at: i64,
    pub upstream_request_sent_at: Option<i64>,
    pub first_response_body_at: Option<i64>,
    pub first_token_at: Option<i64>,
    pub is_stream: bool,
    pub completed_at: Option<i64>,
    pub status: RequestStatus,
    pub estimated_cost_nanos: Option<i64>,
    pub estimated_cost_currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProviderRuntimeStatusCodeCount {
    pub status_code: i32,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProviderRuntimeCostAggregate {
    pub currency: String,
    pub amount_nanos: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProviderRuntimeAggregate {
    pub provider_id: i64,
    pub source_id: i64,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub avg_time_to_first_response_body_ms: Option<f64>,
    pub time_to_first_response_body_sample_count: i64,
    pub avg_ttft_ms: Option<f64>,
    pub ttft_sample_count: i64,
    pub avg_total_latency_ms: Option<f64>,
    pub total_latency_sample_count: i64,
    pub last_request_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error_at: Option<i64>,
    pub status_code_breakdown: Vec<ProviderRuntimeStatusCodeCount>,
    pub total_cost: Vec<ProviderRuntimeCostAggregate>,
}

#[derive(Debug, Default)]
struct ProviderRuntimeAccumulator {
    request_count: i64,
    success_count: i64,
    error_count: i64,
    time_to_first_response_body_sum_ms: i64,
    time_to_first_response_body_count: i64,
    ttft_sum_ms: i64,
    ttft_count: i64,
    total_latency_sum_ms: i64,
    total_latency_count: i64,
    last_request_at: Option<i64>,
    last_success_at: Option<i64>,
    last_error_at: Option<i64>,
    status_code_breakdown: HashMap<i32, i64>,
    total_cost: HashMap<String, i64>,
}

impl ProviderRuntimeAccumulator {
    fn into_aggregate(self, provider_id: i64, source_id: i64) -> ProviderRuntimeAggregate {
        let mut status_code_breakdown = self
            .status_code_breakdown
            .into_iter()
            .map(|(status_code, count)| ProviderRuntimeStatusCodeCount { status_code, count })
            .collect::<Vec<_>>();
        status_code_breakdown.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.status_code.cmp(&b.status_code))
        });

        let mut total_cost = self
            .total_cost
            .into_iter()
            .map(|(currency, amount_nanos)| ProviderRuntimeCostAggregate {
                currency,
                amount_nanos,
            })
            .collect::<Vec<_>>();
        total_cost.sort_by(|a, b| a.currency.cmp(&b.currency));

        ProviderRuntimeAggregate {
            provider_id,
            source_id,
            request_count: self.request_count,
            success_count: self.success_count,
            error_count: self.error_count,
            avg_time_to_first_response_body_ms: average_or_none(
                self.time_to_first_response_body_sum_ms,
                self.time_to_first_response_body_count,
            ),
            time_to_first_response_body_sample_count: self.time_to_first_response_body_count,
            avg_ttft_ms: average_or_none(self.ttft_sum_ms, self.ttft_count),
            ttft_sample_count: self.ttft_count,
            avg_total_latency_ms: average_or_none(
                self.total_latency_sum_ms,
                self.total_latency_count,
            ),
            total_latency_sample_count: self.total_latency_count,
            last_request_at: self.last_request_at,
            last_success_at: self.last_success_at,
            last_error_at: self.last_error_at,
            status_code_breakdown,
            total_cost,
        }
    }
}

fn average_or_none(sum: i64, count: i64) -> Option<f64> {
    if count > 0 {
        Some(sum as f64 / count as f64)
    } else {
        None
    }
}

fn update_latest(target: &mut Option<i64>, candidate: i64) {
    *target = Some(target.map_or(candidate, |current| current.max(candidate)));
}

fn is_success_status(status: &RequestStatus) -> bool {
    matches!(status, RequestStatus::Success)
}

fn is_error_status(status: &RequestStatus) -> bool {
    matches!(status, RequestStatus::Error | RequestStatus::Cancelled)
}

fn positive_duration_ms(start_ms: Option<i64>, end_ms: Option<i64>) -> Option<i64> {
    let (start_ms, end_ms) = (start_ms?, end_ms?);
    let duration = end_ms - start_ms;
    (duration >= 0).then_some(duration)
}

pub fn aggregate_provider_runtime_entries(
    entries: Vec<RequestLogEntryForProviderRuntime>,
) -> Vec<ProviderRuntimeAggregate> {
    let mut by_source: HashMap<(i64, i64), ProviderRuntimeAccumulator> = HashMap::new();

    for entry in entries {
        let RequestLogEntryForProviderRuntime {
            provider_id,
            source_id,
            request_received_at,
            upstream_request_sent_at,
            first_response_body_at,
            first_token_at,
            is_stream,
            completed_at,
            status,
            estimated_cost_nanos,
            estimated_cost_currency,
        } = entry;

        let item = by_source.entry((provider_id, source_id)).or_default();
        item.request_count += 1;
        update_latest(&mut item.last_request_at, request_received_at);

        if is_success_status(&status) {
            item.success_count += 1;
            update_latest(&mut item.last_success_at, request_received_at);
        } else if is_error_status(&status) {
            item.error_count += 1;
            update_latest(&mut item.last_error_at, request_received_at);
        }

        if let Some(time_to_first_response_body_ms) =
            positive_duration_ms(upstream_request_sent_at, first_response_body_at)
        {
            item.time_to_first_response_body_sum_ms += time_to_first_response_body_ms;
            item.time_to_first_response_body_count += 1;
        }

        if is_stream {
            if let Some(ttft_ms) = positive_duration_ms(upstream_request_sent_at, first_token_at) {
                item.ttft_sum_ms += ttft_ms;
                item.ttft_count += 1;
            }
        }

        if let Some(total_latency_ms) = positive_duration_ms(upstream_request_sent_at, completed_at)
        {
            item.total_latency_sum_ms += total_latency_ms;
            item.total_latency_count += 1;
        }

        if let (Some(cost_nanos), Some(currency)) = (estimated_cost_nanos, estimated_cost_currency)
        {
            if cost_nanos > 0 {
                *item.total_cost.entry(currency).or_insert(0) += cost_nanos;
            }
        }
    }

    let mut result = by_source
        .into_iter()
        .map(|((provider_id, source_id), item)| item.into_aggregate(provider_id, source_id))
        .collect::<Vec<_>>();
    result.sort_by_key(|item| (item.provider_id, item.source_id));
    result
}

pub fn get_provider_runtime_aggregates_in_range(
    start_time_ms: i64,
    end_time_ms: i64,
    provider_id_filter: Option<i64>,
) -> DbResult<Vec<ProviderRuntimeAggregate>> {
    let entries = match &mut get_connection()? {
        DbConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::request_log;

            let mut query = request_log::table
                .filter(request_log::dsl::request_received_at.ge(start_time_ms))
                .filter(request_log::dsl::request_received_at.lt(end_time_ms))
                .filter(request_log::dsl::provider_id.is_not_null())
                .filter(request_log::dsl::source_id.is_not_null())
                .into_boxed();

            if let Some(provider_id) = provider_id_filter {
                query = query.filter(request_log::dsl::provider_id.eq(Some(provider_id)));
            }

            query
                .select((
                    request_log::dsl::provider_id,
                    request_log::dsl::source_id,
                    request_log::dsl::request_received_at,
                    request_log::dsl::upstream_request_sent_at,
                    request_log::dsl::first_response_body_at,
                    request_log::dsl::first_token_at,
                    request_log::dsl::is_stream,
                    request_log::dsl::completed_at,
                    request_log::dsl::status,
                    request_log::dsl::estimated_cost_nanos,
                    request_log::dsl::estimated_cost_currency.nullable(),
                ))
                .order(request_log::dsl::request_received_at.asc())
                .load::<(
                    Option<i64>,
                    Option<i64>,
                    i64,
                    Option<i64>,
                    Option<i64>,
                    Option<i64>,
                    bool,
                    Option<i64>,
                    RequestStatus,
                    Option<i64>,
                    Option<String>,
                )>(conn)?
                .into_iter()
                .filter_map(
                    |(
                        provider_id,
                        source_id,
                        request_received_at,
                        upstream_request_sent_at,
                        first_response_body_at,
                        first_token_at,
                        is_stream,
                        completed_at,
                        status,
                        estimated_cost_nanos,
                        estimated_cost_currency,
                    )| {
                        provider_id.zip(source_id).map(|(provider_id, source_id)| {
                            RequestLogEntryForProviderRuntime {
                                provider_id,
                                source_id,
                                request_received_at,
                                upstream_request_sent_at,
                                first_response_body_at,
                                first_token_at,
                                is_stream,
                                completed_at,
                                status,
                                estimated_cost_nanos,
                                estimated_cost_currency,
                            }
                        })
                    },
                )
                .collect()
        }
        DbConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::request_log;

            let mut query = request_log::table
                .filter(request_log::dsl::request_received_at.ge(start_time_ms))
                .filter(request_log::dsl::request_received_at.lt(end_time_ms))
                .filter(request_log::dsl::provider_id.is_not_null())
                .filter(request_log::dsl::source_id.is_not_null())
                .into_boxed();

            if let Some(provider_id) = provider_id_filter {
                query = query.filter(request_log::dsl::provider_id.eq(Some(provider_id)));
            }

            query
                .select((
                    request_log::dsl::provider_id,
                    request_log::dsl::source_id,
                    request_log::dsl::request_received_at,
                    request_log::dsl::upstream_request_sent_at,
                    request_log::dsl::first_response_body_at,
                    request_log::dsl::first_token_at,
                    request_log::dsl::is_stream,
                    request_log::dsl::completed_at,
                    request_log::dsl::status,
                    request_log::dsl::estimated_cost_nanos,
                    request_log::dsl::estimated_cost_currency.nullable(),
                ))
                .order(request_log::dsl::request_received_at.asc())
                .load::<(
                    Option<i64>,
                    Option<i64>,
                    i64,
                    Option<i64>,
                    Option<i64>,
                    Option<i64>,
                    bool,
                    Option<i64>,
                    RequestStatus,
                    Option<i64>,
                    Option<String>,
                )>(conn)?
                .into_iter()
                .filter_map(
                    |(
                        provider_id,
                        source_id,
                        request_received_at,
                        upstream_request_sent_at,
                        first_response_body_at,
                        first_token_at,
                        is_stream,
                        completed_at,
                        status,
                        estimated_cost_nanos,
                        estimated_cost_currency,
                    )| {
                        provider_id.zip(source_id).map(|(provider_id, source_id)| {
                            RequestLogEntryForProviderRuntime {
                                provider_id,
                                source_id,
                                request_received_at,
                                upstream_request_sent_at,
                                first_response_body_at,
                                first_token_at,
                                is_stream,
                                completed_at,
                                status,
                                estimated_cost_nanos,
                                estimated_cost_currency,
                            }
                        })
                    },
                )
                .collect()
        }
    };

    Ok(aggregate_provider_runtime_entries(entries))
}

#[cfg(test)]
mod tests {
    use super::{
        RequestLogEntryForProviderRuntime, aggregate_provider_runtime_entries,
        get_provider_runtime_aggregates_in_range,
    };
    use crate::schema::enum_def::RequestStatus;

    fn entry(
        provider_id: i64,
        request_received_at: i64,
        upstream_request_sent_at: i64,
        first_chunk_at: Option<i64>,
        completed_at: Option<i64>,
        status: RequestStatus,
        estimated_cost_nanos: Option<i64>,
        estimated_cost_currency: Option<&str>,
    ) -> RequestLogEntryForProviderRuntime {
        RequestLogEntryForProviderRuntime {
            provider_id,
            source_id: provider_id + 100,
            request_received_at,
            upstream_request_sent_at: Some(upstream_request_sent_at),
            first_response_body_at: first_chunk_at,
            first_token_at: first_chunk_at,
            is_stream: true,
            completed_at,
            status,
            estimated_cost_nanos,
            estimated_cost_currency: estimated_cost_currency.map(str::to_string),
        }
    }

    #[test]
    fn provider_runtime_aggregate_returns_empty_for_empty_window() {
        let result = aggregate_provider_runtime_entries(Vec::new());
        assert!(result.is_empty());
    }

    #[test]
    fn provider_runtime_aggregate_computes_counts_latencies_and_costs() {
        let result = aggregate_provider_runtime_entries(vec![
            entry(
                1,
                1_000,
                1_000,
                Some(1_050),
                Some(1_200),
                RequestStatus::Success,
                Some(100),
                Some("USD"),
            ),
            entry(
                1,
                2_000,
                2_000,
                Some(2_100),
                Some(2_400),
                RequestStatus::Error,
                Some(50),
                Some("USD"),
            ),
            entry(
                1,
                3_000,
                3_000,
                None,
                None,
                RequestStatus::Cancelled,
                None,
                None,
            ),
        ]);

        let aggregate = result.into_iter().next().expect("provider aggregate");
        assert_eq!(aggregate.provider_id, 1);
        assert_eq!(aggregate.request_count, 3);
        assert_eq!(aggregate.success_count, 1);
        assert_eq!(aggregate.error_count, 2);
        assert_eq!(aggregate.avg_time_to_first_response_body_ms, Some(75.0));
        assert_eq!(aggregate.time_to_first_response_body_sample_count, 2);
        assert_eq!(aggregate.avg_ttft_ms, Some(75.0));
        assert_eq!(aggregate.ttft_sample_count, 2);
        assert_eq!(aggregate.avg_total_latency_ms, Some(300.0));
        assert_eq!(aggregate.total_latency_sample_count, 2);
        assert_eq!(aggregate.last_request_at, Some(3_000));
        assert_eq!(aggregate.last_success_at, Some(1_000));
        assert_eq!(aggregate.last_error_at, Some(3_000));
        assert!(aggregate.status_code_breakdown.is_empty());
        assert_eq!(aggregate.total_cost.len(), 1);
        assert_eq!(aggregate.total_cost[0].currency, "USD");
        assert_eq!(aggregate.total_cost[0].amount_nanos, 150);
    }

    #[test]
    fn provider_runtime_aggregate_keeps_same_provider_sources_isolated() {
        let first = entry(
            7,
            1_000,
            1_000,
            Some(1_050),
            Some(1_200),
            RequestStatus::Success,
            Some(100),
            Some("USD"),
        );
        let mut second = entry(
            7,
            2_000,
            2_000,
            Some(2_100),
            Some(2_400),
            RequestStatus::Error,
            Some(50),
            Some("EUR"),
        );
        second.source_id = 702;

        let mut result = aggregate_provider_runtime_entries(vec![first, second]);
        result.sort_by_key(|item| item.source_id);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].provider_id, 7);
        assert_eq!(result[0].source_id, 107);
        assert_eq!(result[0].request_count, 1);
        assert_eq!(result[0].success_count, 1);
        assert_eq!(result[0].error_count, 0);
        assert_eq!(result[0].total_cost[0].currency, "USD");
        assert_eq!(result[1].source_id, 702);
        assert_eq!(result[1].request_count, 1);
        assert_eq!(result[1].success_count, 0);
        assert_eq!(result[1].error_count, 1);
        assert_eq!(result[1].total_cost[0].currency, "EUR");
    }

    #[test]
    fn provider_runtime_aggregate_ignores_negative_latencies() {
        let result = aggregate_provider_runtime_entries(vec![entry(
            1,
            1_000,
            2_000,
            Some(1_000),
            Some(1_500),
            RequestStatus::Success,
            None,
            None,
        )]);

        let aggregate = result.into_iter().next().expect("provider aggregate");
        assert_eq!(aggregate.avg_time_to_first_response_body_ms, None);
        assert_eq!(aggregate.avg_ttft_ms, None);
        assert_eq!(aggregate.avg_total_latency_ms, None);
    }

    #[test]
    fn provider_runtime_aggregate_excludes_ttft_for_non_stream_requests() {
        let mut non_stream = entry(
            1,
            1_000,
            1_000,
            Some(1_050),
            Some(1_200),
            RequestStatus::Success,
            None,
            None,
        );
        non_stream.is_stream = false;

        let aggregate = aggregate_provider_runtime_entries(vec![non_stream])
            .into_iter()
            .next()
            .expect("provider aggregate");
        assert_eq!(aggregate.avg_time_to_first_response_body_ms, Some(50.0));
        assert_eq!(aggregate.avg_ttft_ms, None);
        assert_eq!(aggregate.ttft_sample_count, 0);
    }

    #[test]
    fn provider_runtime_aggregate_counts_zero_duration_samples() {
        let aggregate = aggregate_provider_runtime_entries(vec![entry(
            1,
            1_000,
            1_000,
            Some(1_000),
            Some(1_000),
            RequestStatus::Success,
            None,
            None,
        )])
        .into_iter()
        .next()
        .expect("provider aggregate");

        assert_eq!(aggregate.avg_time_to_first_response_body_ms, Some(0.0));
        assert_eq!(aggregate.time_to_first_response_body_sample_count, 1);
        assert_eq!(aggregate.avg_ttft_ms, Some(0.0));
        assert_eq!(aggregate.ttft_sample_count, 1);
        assert_eq!(aggregate.avg_total_latency_ms, Some(0.0));
        assert_eq!(aggregate.total_latency_sample_count, 1);
    }

    #[test]
    fn provider_runtime_aggregate_groups_multiple_providers() {
        let result = aggregate_provider_runtime_entries(vec![
            entry(
                2,
                2_000,
                2_000,
                Some(2_020),
                Some(2_100),
                RequestStatus::Success,
                None,
                None,
            ),
            entry(
                1,
                1_000,
                1_000,
                Some(1_050),
                Some(1_200),
                RequestStatus::Error,
                None,
                None,
            ),
        ]);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].provider_id, 1);
        assert_eq!(result[1].provider_id, 2);
    }

    #[test]
    fn provider_runtime_db_query_returns_empty_when_no_logs_match() {
        let result =
            get_provider_runtime_aggregates_in_range(9_000_000_000_000, 9_000_000_000_100, None)
                .expect("query should succeed");
        assert!(result.is_empty());
    }
}
