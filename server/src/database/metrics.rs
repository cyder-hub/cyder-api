use std::collections::BTreeMap;

use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel::{QueryableByName, sql_query};
use serde::{Deserialize, Serialize};

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::{db_execute, db_object};

db_object! {
    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = metric_ingested_request_log)]
    pub struct MetricIngestedRequestLog {
        pub request_log_id: i64,
        pub request_received_at: i64,
        pub completed_at: Option<i64>,
        pub ingested_at: i64,
    }

    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = metric_request_rollup_minute)]
    #[diesel(primary_key(bucket_start_ms, scope_type, scope_id))]
    pub struct MetricRequestRollupMinute {
        pub bucket_start_ms: i64,
        pub scope_type: String,
        pub scope_id: String,
        pub scope_label: Option<String>,
        pub request_count: i64,
        pub success_count: i64,
        pub error_count: i64,
        pub cancelled_count: i64,
        pub retry_count: i64,
        pub fallback_count: i64,
        pub first_byte_latency_sum_ms: i64,
        pub first_byte_latency_count: i64,
        pub total_latency_sum_ms: i64,
        pub total_latency_count: i64,
        pub input_tokens: i64,
        pub output_tokens: i64,
        pub reasoning_tokens: i64,
        pub total_tokens: i64,
        pub transform_diagnostic_count: i64,
        pub transform_diagnostic_lossy_major_count: i64,
        pub transform_diagnostic_reject_count: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = metric_attempt_rollup_minute)]
    #[diesel(primary_key(bucket_start_ms, scope_type, scope_id))]
    pub struct MetricAttemptRollupMinute {
        pub bucket_start_ms: i64,
        pub scope_type: String,
        pub scope_id: String,
        pub scope_label: Option<String>,
        pub attempt_count: i64,
        pub success_count: i64,
        pub error_count: i64,
        pub skipped_count: i64,
        pub retry_same_candidate_count: i64,
        pub fallback_next_candidate_count: i64,
        pub fail_fast_count: i64,
        pub first_byte_latency_sum_ms: i64,
        pub first_byte_latency_count: i64,
        pub total_latency_sum_ms: i64,
        pub total_latency_count: i64,
        pub input_tokens: i64,
        pub output_tokens: i64,
        pub reasoning_tokens: i64,
        pub total_tokens: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = metric_http_status_rollup_minute)]
    #[diesel(primary_key(bucket_start_ms, scope_type, scope_id, http_status))]
    pub struct MetricHttpStatusRollupMinute {
        pub bucket_start_ms: i64,
        pub scope_type: String,
        pub scope_id: String,
        pub http_status: i32,
        pub count: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = metric_cost_rollup_minute)]
    #[diesel(primary_key(bucket_start_ms, metric_kind, scope_type, scope_id, currency))]
    pub struct MetricCostRollupMinute {
        pub bucket_start_ms: i64,
        pub metric_kind: String,
        pub scope_type: String,
        pub scope_id: String,
        pub currency: String,
        pub amount_nanos: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetricRequestWindowAggregate {
    pub scope_type: String,
    pub scope_id: String,
    pub scope_label: Option<String>,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub cancelled_count: i64,
    pub retry_count: i64,
    pub fallback_count: i64,
    pub first_byte_latency_sum_ms: i64,
    pub first_byte_latency_count: i64,
    pub total_latency_sum_ms: i64,
    pub total_latency_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub transform_diagnostic_count: i64,
    pub transform_diagnostic_lossy_major_count: i64,
    pub transform_diagnostic_reject_count: i64,
    pub last_request_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetricAttemptWindowAggregate {
    pub scope_type: String,
    pub scope_id: String,
    pub scope_label: Option<String>,
    pub attempt_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub skipped_count: i64,
    pub retry_same_candidate_count: i64,
    pub fallback_next_candidate_count: i64,
    pub fail_fast_count: i64,
    pub first_byte_latency_sum_ms: i64,
    pub first_byte_latency_count: i64,
    pub total_latency_sum_ms: i64,
    pub total_latency_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricHttpStatusCount {
    pub status_code: i32,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricCostAggregate {
    pub currency: String,
    pub amount_nanos: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsRepairDeleteSummary {
    pub deleted_ingest_markers: usize,
    pub deleted_request_rollups: usize,
    pub deleted_attempt_rollups: usize,
    pub deleted_http_status_rollups: usize,
    pub deleted_cost_rollups: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct ReconciliationRequestLogRef {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub id: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub request_received_at: i64,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

macro_rules! upsert_request_rollup_delta_in_tx {
    ($conn:expr, $delta:expr) => {{
        diesel::insert_into(metric_request_rollup_minute::table)
            .values(MetricRequestRollupMinuteDb::to_db($delta))
            .on_conflict((
                metric_request_rollup_minute::dsl::bucket_start_ms,
                metric_request_rollup_minute::dsl::scope_type,
                metric_request_rollup_minute::dsl::scope_id,
            ))
            .do_update()
            .set((
                metric_request_rollup_minute::dsl::scope_label
                    .eq(excluded(metric_request_rollup_minute::dsl::scope_label)),
                metric_request_rollup_minute::dsl::request_count
                    .eq(metric_request_rollup_minute::dsl::request_count
                        + excluded(metric_request_rollup_minute::dsl::request_count)),
                metric_request_rollup_minute::dsl::success_count
                    .eq(metric_request_rollup_minute::dsl::success_count
                        + excluded(metric_request_rollup_minute::dsl::success_count)),
                metric_request_rollup_minute::dsl::error_count
                    .eq(metric_request_rollup_minute::dsl::error_count
                        + excluded(metric_request_rollup_minute::dsl::error_count)),
                metric_request_rollup_minute::dsl::cancelled_count
                    .eq(metric_request_rollup_minute::dsl::cancelled_count
                        + excluded(metric_request_rollup_minute::dsl::cancelled_count)),
                metric_request_rollup_minute::dsl::retry_count
                    .eq(metric_request_rollup_minute::dsl::retry_count
                        + excluded(metric_request_rollup_minute::dsl::retry_count)),
                metric_request_rollup_minute::dsl::fallback_count
                    .eq(metric_request_rollup_minute::dsl::fallback_count
                        + excluded(metric_request_rollup_minute::dsl::fallback_count)),
                metric_request_rollup_minute::dsl::first_byte_latency_sum_ms.eq(
                    metric_request_rollup_minute::dsl::first_byte_latency_sum_ms
                        + excluded(metric_request_rollup_minute::dsl::first_byte_latency_sum_ms),
                ),
                metric_request_rollup_minute::dsl::first_byte_latency_count.eq(
                    metric_request_rollup_minute::dsl::first_byte_latency_count
                        + excluded(metric_request_rollup_minute::dsl::first_byte_latency_count),
                ),
                metric_request_rollup_minute::dsl::total_latency_sum_ms.eq(
                    metric_request_rollup_minute::dsl::total_latency_sum_ms
                        + excluded(metric_request_rollup_minute::dsl::total_latency_sum_ms),
                ),
                metric_request_rollup_minute::dsl::total_latency_count.eq(
                    metric_request_rollup_minute::dsl::total_latency_count
                        + excluded(metric_request_rollup_minute::dsl::total_latency_count),
                ),
                metric_request_rollup_minute::dsl::input_tokens
                    .eq(metric_request_rollup_minute::dsl::input_tokens
                        + excluded(metric_request_rollup_minute::dsl::input_tokens)),
                metric_request_rollup_minute::dsl::output_tokens
                    .eq(metric_request_rollup_minute::dsl::output_tokens
                        + excluded(metric_request_rollup_minute::dsl::output_tokens)),
                metric_request_rollup_minute::dsl::reasoning_tokens
                    .eq(metric_request_rollup_minute::dsl::reasoning_tokens
                        + excluded(metric_request_rollup_minute::dsl::reasoning_tokens)),
                metric_request_rollup_minute::dsl::total_tokens
                    .eq(metric_request_rollup_minute::dsl::total_tokens
                        + excluded(metric_request_rollup_minute::dsl::total_tokens)),
                metric_request_rollup_minute::dsl::transform_diagnostic_count.eq(
                    metric_request_rollup_minute::dsl::transform_diagnostic_count
                        + excluded(metric_request_rollup_minute::dsl::transform_diagnostic_count),
                ),
                metric_request_rollup_minute::dsl::transform_diagnostic_lossy_major_count.eq(
                    metric_request_rollup_minute::dsl::transform_diagnostic_lossy_major_count
                        + excluded(
                            metric_request_rollup_minute::dsl::transform_diagnostic_lossy_major_count,
                        ),
                ),
                metric_request_rollup_minute::dsl::transform_diagnostic_reject_count.eq(
                    metric_request_rollup_minute::dsl::transform_diagnostic_reject_count
                        + excluded(
                            metric_request_rollup_minute::dsl::transform_diagnostic_reject_count,
                        ),
                ),
                metric_request_rollup_minute::dsl::updated_at
                    .eq(excluded(metric_request_rollup_minute::dsl::updated_at)),
            ))
            .execute($conn)
            .map(|_| ())
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add request metrics delta for {}:{} bucket {}: {}",
                    $delta.scope_type, $delta.scope_id, $delta.bucket_start_ms, err
                )))
            })
    }};
}

macro_rules! upsert_attempt_rollup_delta_in_tx {
    ($conn:expr, $delta:expr) => {{
        diesel::insert_into(metric_attempt_rollup_minute::table)
            .values(MetricAttemptRollupMinuteDb::to_db($delta))
            .on_conflict((
                metric_attempt_rollup_minute::dsl::bucket_start_ms,
                metric_attempt_rollup_minute::dsl::scope_type,
                metric_attempt_rollup_minute::dsl::scope_id,
            ))
            .do_update()
            .set((
                metric_attempt_rollup_minute::dsl::scope_label
                    .eq(excluded(metric_attempt_rollup_minute::dsl::scope_label)),
                metric_attempt_rollup_minute::dsl::attempt_count
                    .eq(metric_attempt_rollup_minute::dsl::attempt_count
                        + excluded(metric_attempt_rollup_minute::dsl::attempt_count)),
                metric_attempt_rollup_minute::dsl::success_count
                    .eq(metric_attempt_rollup_minute::dsl::success_count
                        + excluded(metric_attempt_rollup_minute::dsl::success_count)),
                metric_attempt_rollup_minute::dsl::error_count
                    .eq(metric_attempt_rollup_minute::dsl::error_count
                        + excluded(metric_attempt_rollup_minute::dsl::error_count)),
                metric_attempt_rollup_minute::dsl::skipped_count
                    .eq(metric_attempt_rollup_minute::dsl::skipped_count
                        + excluded(metric_attempt_rollup_minute::dsl::skipped_count)),
                metric_attempt_rollup_minute::dsl::retry_same_candidate_count.eq(
                    metric_attempt_rollup_minute::dsl::retry_same_candidate_count
                        + excluded(metric_attempt_rollup_minute::dsl::retry_same_candidate_count),
                ),
                metric_attempt_rollup_minute::dsl::fallback_next_candidate_count.eq(
                    metric_attempt_rollup_minute::dsl::fallback_next_candidate_count
                        + excluded(
                            metric_attempt_rollup_minute::dsl::fallback_next_candidate_count,
                        ),
                ),
                metric_attempt_rollup_minute::dsl::fail_fast_count
                    .eq(metric_attempt_rollup_minute::dsl::fail_fast_count
                        + excluded(metric_attempt_rollup_minute::dsl::fail_fast_count)),
                metric_attempt_rollup_minute::dsl::first_byte_latency_sum_ms
                    .eq(metric_attempt_rollup_minute::dsl::first_byte_latency_sum_ms
                        + excluded(metric_attempt_rollup_minute::dsl::first_byte_latency_sum_ms)),
                metric_attempt_rollup_minute::dsl::first_byte_latency_count
                    .eq(metric_attempt_rollup_minute::dsl::first_byte_latency_count
                        + excluded(metric_attempt_rollup_minute::dsl::first_byte_latency_count)),
                metric_attempt_rollup_minute::dsl::total_latency_sum_ms
                    .eq(metric_attempt_rollup_minute::dsl::total_latency_sum_ms
                        + excluded(metric_attempt_rollup_minute::dsl::total_latency_sum_ms)),
                metric_attempt_rollup_minute::dsl::total_latency_count
                    .eq(metric_attempt_rollup_minute::dsl::total_latency_count
                        + excluded(metric_attempt_rollup_minute::dsl::total_latency_count)),
                metric_attempt_rollup_minute::dsl::input_tokens
                    .eq(metric_attempt_rollup_minute::dsl::input_tokens
                        + excluded(metric_attempt_rollup_minute::dsl::input_tokens)),
                metric_attempt_rollup_minute::dsl::output_tokens
                    .eq(metric_attempt_rollup_minute::dsl::output_tokens
                        + excluded(metric_attempt_rollup_minute::dsl::output_tokens)),
                metric_attempt_rollup_minute::dsl::reasoning_tokens
                    .eq(metric_attempt_rollup_minute::dsl::reasoning_tokens
                        + excluded(metric_attempt_rollup_minute::dsl::reasoning_tokens)),
                metric_attempt_rollup_minute::dsl::total_tokens
                    .eq(metric_attempt_rollup_minute::dsl::total_tokens
                        + excluded(metric_attempt_rollup_minute::dsl::total_tokens)),
                metric_attempt_rollup_minute::dsl::updated_at
                    .eq(excluded(metric_attempt_rollup_minute::dsl::updated_at)),
            ))
            .execute($conn)
            .map(|_| ())
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add attempt metrics delta for {}:{} bucket {}: {}",
                    $delta.scope_type, $delta.scope_id, $delta.bucket_start_ms, err
                )))
            })
    }};
}

macro_rules! upsert_http_status_rollup_delta_in_tx {
    ($conn:expr, $delta:expr) => {{
        diesel::insert_into(metric_http_status_rollup_minute::table)
            .values(MetricHttpStatusRollupMinuteDb::to_db($delta))
            .on_conflict((
                metric_http_status_rollup_minute::dsl::bucket_start_ms,
                metric_http_status_rollup_minute::dsl::scope_type,
                metric_http_status_rollup_minute::dsl::scope_id,
                metric_http_status_rollup_minute::dsl::http_status,
            ))
            .do_update()
            .set((
                metric_http_status_rollup_minute::dsl::count
                    .eq(metric_http_status_rollup_minute::dsl::count
                        + excluded(metric_http_status_rollup_minute::dsl::count)),
                metric_http_status_rollup_minute::dsl::updated_at
                    .eq(excluded(metric_http_status_rollup_minute::dsl::updated_at)),
            ))
            .execute($conn)
            .map(|_| ())
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add HTTP status metrics delta for {}:{} status {} bucket {}: {}",
                    $delta.scope_type,
                    $delta.scope_id,
                    $delta.http_status,
                    $delta.bucket_start_ms,
                    err
                )))
            })
    }};
}

macro_rules! upsert_cost_rollup_delta_in_tx {
    ($conn:expr, $delta:expr) => {{
        diesel::insert_into(metric_cost_rollup_minute::table)
            .values(MetricCostRollupMinuteDb::to_db($delta))
            .on_conflict((
                metric_cost_rollup_minute::dsl::bucket_start_ms,
                metric_cost_rollup_minute::dsl::metric_kind,
                metric_cost_rollup_minute::dsl::scope_type,
                metric_cost_rollup_minute::dsl::scope_id,
                metric_cost_rollup_minute::dsl::currency,
            ))
            .do_update()
            .set((
                metric_cost_rollup_minute::dsl::amount_nanos
                    .eq(metric_cost_rollup_minute::dsl::amount_nanos
                        + excluded(metric_cost_rollup_minute::dsl::amount_nanos)),
                metric_cost_rollup_minute::dsl::updated_at
                    .eq(excluded(metric_cost_rollup_minute::dsl::updated_at)),
            ))
            .execute($conn)
            .map(|_| ())
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add cost metrics delta for {} {}:{} {} bucket {}: {}",
                    $delta.metric_kind,
                    $delta.scope_type,
                    $delta.scope_id,
                    $delta.currency,
                    $delta.bucket_start_ms,
                    err
                )))
            })
    }};
}

pub fn insert_ingested_request_log_marker(marker: &MetricIngestedRequestLog) -> DbResult<bool> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::insert_into(metric_ingested_request_log::table)
            .values(MetricIngestedRequestLogDb::to_db(marker))
            .on_conflict(metric_ingested_request_log::dsl::request_log_id)
            .do_nothing()
            .execute(conn)
            .map(|affected| affected > 0)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to insert metrics ingest marker for request_log {}: {}",
                    marker.request_log_id, err
                )))
            })
    })
}

pub fn ingested_request_log_marker_exists(request_log_id: i64) -> DbResult<bool> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        metric_ingested_request_log::table
            .find(request_log_id)
            .select(MetricIngestedRequestLogDb::as_select())
            .first::<MetricIngestedRequestLogDb>(conn)
            .optional()
            .map(|row| row.is_some())
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to check metrics ingest marker for request_log {}: {}",
                    request_log_id, err
                )))
            })
    })
}

pub fn ingest_metric_rollups(
    marker: &MetricIngestedRequestLog,
    request_rollups: &[MetricRequestRollupMinute],
    attempt_rollups: &[MetricAttemptRollupMinute],
    http_status_rollups: &[MetricHttpStatusRollupMinute],
    cost_rollups: &[MetricCostRollupMinute],
) -> DbResult<bool> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        conn.transaction::<bool, BaseError, _>(|conn| {
            let inserted = diesel::insert_into(metric_ingested_request_log::table)
                .values(MetricIngestedRequestLogDb::to_db(marker))
                .on_conflict(metric_ingested_request_log::dsl::request_log_id)
                .do_nothing()
                .execute(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert metrics ingest marker for request_log {}: {}",
                        marker.request_log_id, err
                    )))
                })?;

            if inserted == 0 {
                return Ok(false);
            }

            for delta in request_rollups {
                upsert_request_rollup_delta_in_tx!(conn, delta)?;
            }
            for delta in attempt_rollups {
                upsert_attempt_rollup_delta_in_tx!(conn, delta)?;
            }
            for delta in http_status_rollups {
                upsert_http_status_rollup_delta_in_tx!(conn, delta)?;
            }
            for delta in cost_rollups {
                upsert_cost_rollup_delta_in_tx!(conn, delta)?;
            }

            Ok(true)
        })
    })
}

pub fn delete_metrics_data_in_range(
    marker_start_time_ms: i64,
    marker_end_time_ms: i64,
    rollup_start_time_ms: i64,
    rollup_end_time_ms: i64,
) -> DbResult<MetricsRepairDeleteSummary> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        conn.transaction::<MetricsRepairDeleteSummary, BaseError, _>(|conn| {
            let deleted_cost_rollups = diesel::delete(
                metric_cost_rollup_minute::table
                    .filter(
                        metric_cost_rollup_minute::dsl::bucket_start_ms.ge(rollup_start_time_ms),
                    )
                    .filter(metric_cost_rollup_minute::dsl::bucket_start_ms.lt(rollup_end_time_ms)),
            )
            .execute(conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete cost metrics rollups {}..{}: {}",
                    rollup_start_time_ms, rollup_end_time_ms, err
                )))
            })?;

            let deleted_http_status_rollups = diesel::delete(
                metric_http_status_rollup_minute::table
                    .filter(
                        metric_http_status_rollup_minute::dsl::bucket_start_ms
                            .ge(rollup_start_time_ms),
                    )
                    .filter(
                        metric_http_status_rollup_minute::dsl::bucket_start_ms
                            .lt(rollup_end_time_ms),
                    ),
            )
            .execute(conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete HTTP status metrics rollups {}..{}: {}",
                    rollup_start_time_ms, rollup_end_time_ms, err
                )))
            })?;

            let deleted_attempt_rollups = diesel::delete(
                metric_attempt_rollup_minute::table
                    .filter(
                        metric_attempt_rollup_minute::dsl::bucket_start_ms.ge(rollup_start_time_ms),
                    )
                    .filter(
                        metric_attempt_rollup_minute::dsl::bucket_start_ms.lt(rollup_end_time_ms),
                    ),
            )
            .execute(conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete attempt metrics rollups {}..{}: {}",
                    rollup_start_time_ms, rollup_end_time_ms, err
                )))
            })?;

            let deleted_request_rollups = diesel::delete(
                metric_request_rollup_minute::table
                    .filter(
                        metric_request_rollup_minute::dsl::bucket_start_ms.ge(rollup_start_time_ms),
                    )
                    .filter(
                        metric_request_rollup_minute::dsl::bucket_start_ms.lt(rollup_end_time_ms),
                    ),
            )
            .execute(conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete request metrics rollups {}..{}: {}",
                    rollup_start_time_ms, rollup_end_time_ms, err
                )))
            })?;

            let deleted_ingest_markers = diesel::delete(
                metric_ingested_request_log::table
                    .filter(
                        metric_ingested_request_log::dsl::request_received_at
                            .ge(marker_start_time_ms),
                    )
                    .filter(
                        metric_ingested_request_log::dsl::request_received_at
                            .lt(marker_end_time_ms),
                    ),
            )
            .execute(conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete metrics ingest markers {}..{}: {}",
                    marker_start_time_ms, marker_end_time_ms, err
                )))
            })?;

            Ok(MetricsRepairDeleteSummary {
                deleted_ingest_markers,
                deleted_request_rollups,
                deleted_attempt_rollups,
                deleted_http_status_rollups,
                deleted_cost_rollups,
            })
        })
    })
}

pub fn count_ingested_request_log_markers() -> DbResult<i64> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        metric_ingested_request_log::table
            .count()
            .get_result::<i64>(conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to count metrics ingest markers: {}",
                    err
                )))
            })
    })
}

pub fn count_uningested_request_logs_in_range(
    start_time_ms: i64,
    end_time_ms: i64,
) -> DbResult<i64> {
    match &mut get_connection()? {
        super::DbConnection::Postgres(conn) => sql_query(
            "SELECT COUNT(*) AS count
             FROM request_log rl
             LEFT JOIN metric_ingested_request_log marker
               ON marker.request_log_id = rl.id
             WHERE rl.request_received_at >= $1
               AND rl.request_received_at < $2
               AND marker.request_log_id IS NULL",
        )
        .bind::<diesel::sql_types::BigInt, _>(start_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(end_time_ms)
        .get_result::<CountRow>(conn)
        .map(|row| row.count)
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to count uningested request logs {}..{}: {}",
                start_time_ms, end_time_ms, err
            )))
        }),
        super::DbConnection::Sqlite(conn) => sql_query(
            "SELECT COUNT(*) AS count
             FROM request_log rl
             LEFT JOIN metric_ingested_request_log marker
               ON marker.request_log_id = rl.id
             WHERE rl.request_received_at >= ?
               AND rl.request_received_at < ?
               AND marker.request_log_id IS NULL",
        )
        .bind::<diesel::sql_types::BigInt, _>(start_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(end_time_ms)
        .get_result::<CountRow>(conn)
        .map(|row| row.count)
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to count uningested request logs {}..{}: {}",
                start_time_ms, end_time_ms, err
            )))
        }),
    }
}

pub fn list_uningested_request_log_ids(
    start_time_ms: i64,
    end_time_ms: i64,
    limit: i64,
) -> DbResult<Vec<ReconciliationRequestLogRef>> {
    match &mut get_connection()? {
        super::DbConnection::Postgres(conn) => sql_query(
            "SELECT rl.id AS id, rl.request_received_at AS request_received_at
             FROM request_log rl
             LEFT JOIN metric_ingested_request_log marker
               ON marker.request_log_id = rl.id
             WHERE rl.request_received_at >= $1
               AND rl.request_received_at < $2
               AND marker.request_log_id IS NULL
             ORDER BY rl.request_received_at ASC, rl.id ASC
             LIMIT $3",
        )
        .bind::<diesel::sql_types::BigInt, _>(start_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(end_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .load::<ReconciliationRequestLogRef>(conn)
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to list uningested request logs {}..{}: {}",
                start_time_ms, end_time_ms, err
            )))
        }),
        super::DbConnection::Sqlite(conn) => sql_query(
            "SELECT rl.id AS id, rl.request_received_at AS request_received_at
             FROM request_log rl
             LEFT JOIN metric_ingested_request_log marker
               ON marker.request_log_id = rl.id
             WHERE rl.request_received_at >= ?
               AND rl.request_received_at < ?
               AND marker.request_log_id IS NULL
             ORDER BY rl.request_received_at ASC, rl.id ASC
             LIMIT ?",
        )
        .bind::<diesel::sql_types::BigInt, _>(start_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(end_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .load::<ReconciliationRequestLogRef>(conn)
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to list uningested request logs {}..{}: {}",
                start_time_ms, end_time_ms, err
            )))
        }),
    }
}

pub fn list_request_log_ids_in_range_after(
    start_time_ms: i64,
    end_time_ms: i64,
    after_request_received_at: i64,
    after_request_log_id: i64,
    limit: i64,
) -> DbResult<Vec<ReconciliationRequestLogRef>> {
    match &mut get_connection()? {
        super::DbConnection::Postgres(conn) => sql_query(
            "SELECT rl.id AS id, rl.request_received_at AS request_received_at
             FROM request_log rl
             WHERE rl.request_received_at >= $1
               AND rl.request_received_at < $2
               AND (
                   rl.request_received_at > $3
                   OR (rl.request_received_at = $3 AND rl.id > $4)
               )
             ORDER BY rl.request_received_at ASC, rl.id ASC
             LIMIT $5",
        )
        .bind::<diesel::sql_types::BigInt, _>(start_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(end_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(after_request_received_at)
        .bind::<diesel::sql_types::BigInt, _>(after_request_log_id)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .load::<ReconciliationRequestLogRef>(conn)
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to list request logs {}..{} after {}:{}: {}",
                start_time_ms, end_time_ms, after_request_received_at, after_request_log_id, err
            )))
        }),
        super::DbConnection::Sqlite(conn) => sql_query(
            "SELECT rl.id AS id, rl.request_received_at AS request_received_at
             FROM request_log rl
             WHERE rl.request_received_at >= ?
               AND rl.request_received_at < ?
               AND (
                   rl.request_received_at > ?
                   OR (rl.request_received_at = ? AND rl.id > ?)
               )
             ORDER BY rl.request_received_at ASC, rl.id ASC
             LIMIT ?",
        )
        .bind::<diesel::sql_types::BigInt, _>(start_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(end_time_ms)
        .bind::<diesel::sql_types::BigInt, _>(after_request_received_at)
        .bind::<diesel::sql_types::BigInt, _>(after_request_received_at)
        .bind::<diesel::sql_types::BigInt, _>(after_request_log_id)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .load::<ReconciliationRequestLogRef>(conn)
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to list request logs {}..{} after {}:{}: {}",
                start_time_ms, end_time_ms, after_request_received_at, after_request_log_id, err
            )))
        }),
    }
}

pub fn add_request_rollup_delta(
    delta: &MetricRequestRollupMinute,
) -> DbResult<MetricRequestRollupMinute> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::insert_into(metric_request_rollup_minute::table)
            .values(MetricRequestRollupMinuteDb::to_db(delta))
            .on_conflict((
                metric_request_rollup_minute::dsl::bucket_start_ms,
                metric_request_rollup_minute::dsl::scope_type,
                metric_request_rollup_minute::dsl::scope_id,
            ))
            .do_update()
            .set((
                metric_request_rollup_minute::dsl::scope_label
                    .eq(excluded(metric_request_rollup_minute::dsl::scope_label)),
                metric_request_rollup_minute::dsl::request_count
                    .eq(metric_request_rollup_minute::dsl::request_count
                        + excluded(metric_request_rollup_minute::dsl::request_count)),
                metric_request_rollup_minute::dsl::success_count
                    .eq(metric_request_rollup_minute::dsl::success_count
                        + excluded(metric_request_rollup_minute::dsl::success_count)),
                metric_request_rollup_minute::dsl::error_count
                    .eq(metric_request_rollup_minute::dsl::error_count
                        + excluded(metric_request_rollup_minute::dsl::error_count)),
                metric_request_rollup_minute::dsl::cancelled_count
                    .eq(metric_request_rollup_minute::dsl::cancelled_count
                        + excluded(metric_request_rollup_minute::dsl::cancelled_count)),
                metric_request_rollup_minute::dsl::retry_count
                    .eq(metric_request_rollup_minute::dsl::retry_count
                        + excluded(metric_request_rollup_minute::dsl::retry_count)),
                metric_request_rollup_minute::dsl::fallback_count
                    .eq(metric_request_rollup_minute::dsl::fallback_count
                        + excluded(metric_request_rollup_minute::dsl::fallback_count)),
                metric_request_rollup_minute::dsl::first_byte_latency_sum_ms
                    .eq(metric_request_rollup_minute::dsl::first_byte_latency_sum_ms
                        + excluded(metric_request_rollup_minute::dsl::first_byte_latency_sum_ms)),
                metric_request_rollup_minute::dsl::first_byte_latency_count
                    .eq(metric_request_rollup_minute::dsl::first_byte_latency_count
                        + excluded(metric_request_rollup_minute::dsl::first_byte_latency_count)),
                metric_request_rollup_minute::dsl::total_latency_sum_ms
                    .eq(metric_request_rollup_minute::dsl::total_latency_sum_ms
                        + excluded(metric_request_rollup_minute::dsl::total_latency_sum_ms)),
                metric_request_rollup_minute::dsl::total_latency_count
                    .eq(metric_request_rollup_minute::dsl::total_latency_count
                        + excluded(metric_request_rollup_minute::dsl::total_latency_count)),
                metric_request_rollup_minute::dsl::input_tokens
                    .eq(metric_request_rollup_minute::dsl::input_tokens
                        + excluded(metric_request_rollup_minute::dsl::input_tokens)),
                metric_request_rollup_minute::dsl::output_tokens
                    .eq(metric_request_rollup_minute::dsl::output_tokens
                        + excluded(metric_request_rollup_minute::dsl::output_tokens)),
                metric_request_rollup_minute::dsl::reasoning_tokens
                    .eq(metric_request_rollup_minute::dsl::reasoning_tokens
                        + excluded(metric_request_rollup_minute::dsl::reasoning_tokens)),
                metric_request_rollup_minute::dsl::total_tokens
                    .eq(metric_request_rollup_minute::dsl::total_tokens
                        + excluded(metric_request_rollup_minute::dsl::total_tokens)),
                metric_request_rollup_minute::dsl::transform_diagnostic_count.eq(
                    metric_request_rollup_minute::dsl::transform_diagnostic_count
                        + excluded(
                            metric_request_rollup_minute::dsl::transform_diagnostic_count,
                        ),
                ),
                metric_request_rollup_minute::dsl::transform_diagnostic_lossy_major_count.eq(
                    metric_request_rollup_minute::dsl::transform_diagnostic_lossy_major_count
                        + excluded(
                            metric_request_rollup_minute::dsl::transform_diagnostic_lossy_major_count,
                        ),
                ),
                metric_request_rollup_minute::dsl::transform_diagnostic_reject_count.eq(
                    metric_request_rollup_minute::dsl::transform_diagnostic_reject_count
                        + excluded(
                            metric_request_rollup_minute::dsl::transform_diagnostic_reject_count,
                        ),
                ),
                metric_request_rollup_minute::dsl::updated_at
                    .eq(excluded(metric_request_rollup_minute::dsl::updated_at)),
            ))
            .returning(MetricRequestRollupMinuteDb::as_returning())
            .get_result::<MetricRequestRollupMinuteDb>(conn)
            .map(MetricRequestRollupMinuteDb::from_db)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add request metrics delta for {}:{} bucket {}: {}",
                    delta.scope_type, delta.scope_id, delta.bucket_start_ms, err
                )))
            })
    })
}

pub fn add_attempt_rollup_delta(
    delta: &MetricAttemptRollupMinute,
) -> DbResult<MetricAttemptRollupMinute> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::insert_into(metric_attempt_rollup_minute::table)
            .values(MetricAttemptRollupMinuteDb::to_db(delta))
            .on_conflict((
                metric_attempt_rollup_minute::dsl::bucket_start_ms,
                metric_attempt_rollup_minute::dsl::scope_type,
                metric_attempt_rollup_minute::dsl::scope_id,
            ))
            .do_update()
            .set((
                metric_attempt_rollup_minute::dsl::scope_label
                    .eq(excluded(metric_attempt_rollup_minute::dsl::scope_label)),
                metric_attempt_rollup_minute::dsl::attempt_count
                    .eq(metric_attempt_rollup_minute::dsl::attempt_count
                        + excluded(metric_attempt_rollup_minute::dsl::attempt_count)),
                metric_attempt_rollup_minute::dsl::success_count
                    .eq(metric_attempt_rollup_minute::dsl::success_count
                        + excluded(metric_attempt_rollup_minute::dsl::success_count)),
                metric_attempt_rollup_minute::dsl::error_count
                    .eq(metric_attempt_rollup_minute::dsl::error_count
                        + excluded(metric_attempt_rollup_minute::dsl::error_count)),
                metric_attempt_rollup_minute::dsl::skipped_count
                    .eq(metric_attempt_rollup_minute::dsl::skipped_count
                        + excluded(metric_attempt_rollup_minute::dsl::skipped_count)),
                metric_attempt_rollup_minute::dsl::retry_same_candidate_count.eq(
                    metric_attempt_rollup_minute::dsl::retry_same_candidate_count
                        + excluded(metric_attempt_rollup_minute::dsl::retry_same_candidate_count),
                ),
                metric_attempt_rollup_minute::dsl::fallback_next_candidate_count.eq(
                    metric_attempt_rollup_minute::dsl::fallback_next_candidate_count
                        + excluded(
                            metric_attempt_rollup_minute::dsl::fallback_next_candidate_count,
                        ),
                ),
                metric_attempt_rollup_minute::dsl::fail_fast_count
                    .eq(metric_attempt_rollup_minute::dsl::fail_fast_count
                        + excluded(metric_attempt_rollup_minute::dsl::fail_fast_count)),
                metric_attempt_rollup_minute::dsl::first_byte_latency_sum_ms
                    .eq(metric_attempt_rollup_minute::dsl::first_byte_latency_sum_ms
                        + excluded(metric_attempt_rollup_minute::dsl::first_byte_latency_sum_ms)),
                metric_attempt_rollup_minute::dsl::first_byte_latency_count
                    .eq(metric_attempt_rollup_minute::dsl::first_byte_latency_count
                        + excluded(metric_attempt_rollup_minute::dsl::first_byte_latency_count)),
                metric_attempt_rollup_minute::dsl::total_latency_sum_ms
                    .eq(metric_attempt_rollup_minute::dsl::total_latency_sum_ms
                        + excluded(metric_attempt_rollup_minute::dsl::total_latency_sum_ms)),
                metric_attempt_rollup_minute::dsl::total_latency_count
                    .eq(metric_attempt_rollup_minute::dsl::total_latency_count
                        + excluded(metric_attempt_rollup_minute::dsl::total_latency_count)),
                metric_attempt_rollup_minute::dsl::input_tokens
                    .eq(metric_attempt_rollup_minute::dsl::input_tokens
                        + excluded(metric_attempt_rollup_minute::dsl::input_tokens)),
                metric_attempt_rollup_minute::dsl::output_tokens
                    .eq(metric_attempt_rollup_minute::dsl::output_tokens
                        + excluded(metric_attempt_rollup_minute::dsl::output_tokens)),
                metric_attempt_rollup_minute::dsl::reasoning_tokens
                    .eq(metric_attempt_rollup_minute::dsl::reasoning_tokens
                        + excluded(metric_attempt_rollup_minute::dsl::reasoning_tokens)),
                metric_attempt_rollup_minute::dsl::total_tokens
                    .eq(metric_attempt_rollup_minute::dsl::total_tokens
                        + excluded(metric_attempt_rollup_minute::dsl::total_tokens)),
                metric_attempt_rollup_minute::dsl::updated_at
                    .eq(excluded(metric_attempt_rollup_minute::dsl::updated_at)),
            ))
            .returning(MetricAttemptRollupMinuteDb::as_returning())
            .get_result::<MetricAttemptRollupMinuteDb>(conn)
            .map(MetricAttemptRollupMinuteDb::from_db)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add attempt metrics delta for {}:{} bucket {}: {}",
                    delta.scope_type, delta.scope_id, delta.bucket_start_ms, err
                )))
            })
    })
}

pub fn add_http_status_rollup_delta(
    delta: &MetricHttpStatusRollupMinute,
) -> DbResult<MetricHttpStatusRollupMinute> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::insert_into(metric_http_status_rollup_minute::table)
            .values(MetricHttpStatusRollupMinuteDb::to_db(delta))
            .on_conflict((
                metric_http_status_rollup_minute::dsl::bucket_start_ms,
                metric_http_status_rollup_minute::dsl::scope_type,
                metric_http_status_rollup_minute::dsl::scope_id,
                metric_http_status_rollup_minute::dsl::http_status,
            ))
            .do_update()
            .set((
                metric_http_status_rollup_minute::dsl::count
                    .eq(metric_http_status_rollup_minute::dsl::count
                        + excluded(metric_http_status_rollup_minute::dsl::count)),
                metric_http_status_rollup_minute::dsl::updated_at
                    .eq(excluded(metric_http_status_rollup_minute::dsl::updated_at)),
            ))
            .returning(MetricHttpStatusRollupMinuteDb::as_returning())
            .get_result::<MetricHttpStatusRollupMinuteDb>(conn)
            .map(MetricHttpStatusRollupMinuteDb::from_db)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add HTTP status metrics delta for {}:{} status {} bucket {}: {}",
                    delta.scope_type, delta.scope_id, delta.http_status, delta.bucket_start_ms, err
                )))
            })
    })
}

pub fn add_cost_rollup_delta(delta: &MetricCostRollupMinute) -> DbResult<MetricCostRollupMinute> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::insert_into(metric_cost_rollup_minute::table)
            .values(MetricCostRollupMinuteDb::to_db(delta))
            .on_conflict((
                metric_cost_rollup_minute::dsl::bucket_start_ms,
                metric_cost_rollup_minute::dsl::metric_kind,
                metric_cost_rollup_minute::dsl::scope_type,
                metric_cost_rollup_minute::dsl::scope_id,
                metric_cost_rollup_minute::dsl::currency,
            ))
            .do_update()
            .set((
                metric_cost_rollup_minute::dsl::amount_nanos
                    .eq(metric_cost_rollup_minute::dsl::amount_nanos
                        + excluded(metric_cost_rollup_minute::dsl::amount_nanos)),
                metric_cost_rollup_minute::dsl::updated_at
                    .eq(excluded(metric_cost_rollup_minute::dsl::updated_at)),
            ))
            .returning(MetricCostRollupMinuteDb::as_returning())
            .get_result::<MetricCostRollupMinuteDb>(conn)
            .map(MetricCostRollupMinuteDb::from_db)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add cost metrics delta for {} {}:{} {} bucket {}: {}",
                    delta.metric_kind,
                    delta.scope_type,
                    delta.scope_id,
                    delta.currency,
                    delta.bucket_start_ms,
                    err
                )))
            })
    })
}

pub fn query_request_window_aggregates(
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: Option<&str>,
    scope_id_filter: Option<&str>,
) -> DbResult<Vec<MetricRequestWindowAggregate>> {
    let conn = &mut get_connection()?;
    let rows = db_execute!(conn, {
        let mut query = metric_request_rollup_minute::table
            .filter(metric_request_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms))
            .filter(metric_request_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
            .into_boxed();

        if let Some(scope_type_filter) = scope_type_filter {
            query =
                query.filter(metric_request_rollup_minute::dsl::scope_type.eq(scope_type_filter));
        }
        if let Some(scope_id_filter) = scope_id_filter {
            query = query.filter(metric_request_rollup_minute::dsl::scope_id.eq(scope_id_filter));
        }

        query
            .select(MetricRequestRollupMinuteDb::as_select())
            .load::<MetricRequestRollupMinuteDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(MetricRequestRollupMinuteDb::from_db)
                    .collect::<Vec<_>>()
            })
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to query request metrics window {}..{}: {}",
                    start_time_ms, end_time_ms, err
                )))
            })
    })?;
    Ok(aggregate_request_rows(rows))
}

pub fn list_request_rollup_minutes(
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: Option<&str>,
    scope_id_filter: Option<&str>,
) -> DbResult<Vec<MetricRequestRollupMinute>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        let mut query = metric_request_rollup_minute::table
            .filter(metric_request_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms))
            .filter(metric_request_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
            .into_boxed();

        if let Some(scope_type_filter) = scope_type_filter {
            query =
                query.filter(metric_request_rollup_minute::dsl::scope_type.eq(scope_type_filter));
        }
        if let Some(scope_id_filter) = scope_id_filter {
            query = query.filter(metric_request_rollup_minute::dsl::scope_id.eq(scope_id_filter));
        }

        query
            .order(metric_request_rollup_minute::dsl::bucket_start_ms.asc())
            .select(MetricRequestRollupMinuteDb::as_select())
            .load::<MetricRequestRollupMinuteDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(MetricRequestRollupMinuteDb::from_db)
                    .collect::<Vec<_>>()
            })
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to list request metrics rollup minutes {}..{}: {}",
                    start_time_ms, end_time_ms, err
                )))
            })
    })
}

pub fn query_attempt_window_aggregates(
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: Option<&str>,
    scope_id_filter: Option<&str>,
) -> DbResult<Vec<MetricAttemptWindowAggregate>> {
    let conn = &mut get_connection()?;
    let rows = db_execute!(conn, {
        let mut query = metric_attempt_rollup_minute::table
            .filter(metric_attempt_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms))
            .filter(metric_attempt_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
            .into_boxed();

        if let Some(scope_type_filter) = scope_type_filter {
            query =
                query.filter(metric_attempt_rollup_minute::dsl::scope_type.eq(scope_type_filter));
        }
        if let Some(scope_id_filter) = scope_id_filter {
            query = query.filter(metric_attempt_rollup_minute::dsl::scope_id.eq(scope_id_filter));
        }

        query
            .select(MetricAttemptRollupMinuteDb::as_select())
            .load::<MetricAttemptRollupMinuteDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(MetricAttemptRollupMinuteDb::from_db)
                    .collect::<Vec<_>>()
            })
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to query attempt metrics window {}..{}: {}",
                    start_time_ms, end_time_ms, err
                )))
            })
    })?;
    Ok(aggregate_attempt_rows(rows))
}

pub fn query_http_status_breakdown(
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: &str,
    scope_id_filter: &str,
) -> DbResult<Vec<MetricHttpStatusCount>> {
    let conn = &mut get_connection()?;
    let rows = db_execute!(conn, {
        metric_http_status_rollup_minute::table
            .filter(metric_http_status_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms))
            .filter(metric_http_status_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
            .filter(metric_http_status_rollup_minute::dsl::scope_type.eq(scope_type_filter))
            .filter(metric_http_status_rollup_minute::dsl::scope_id.eq(scope_id_filter))
            .select(MetricHttpStatusRollupMinuteDb::as_select())
            .load::<MetricHttpStatusRollupMinuteDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(MetricHttpStatusRollupMinuteDb::from_db)
                    .collect::<Vec<_>>()
            })
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to query HTTP status metrics window {}..{} for {}:{}: {}",
                    start_time_ms, end_time_ms, scope_type_filter, scope_id_filter, err
                )))
            })
    })?;

    let mut by_status = BTreeMap::<i32, i64>::new();
    for row in rows {
        *by_status.entry(row.http_status).or_default() += row.count;
    }
    let mut result = by_status
        .into_iter()
        .map(|(status_code, count)| MetricHttpStatusCount { status_code, count })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.status_code.cmp(&right.status_code))
    });
    Ok(result)
}

pub fn query_cost_window_aggregates(
    start_time_ms: i64,
    end_time_ms: i64,
    metric_kind_filter: &str,
    scope_type_filter: &str,
    scope_id_filter: &str,
) -> DbResult<Vec<MetricCostAggregate>> {
    let conn = &mut get_connection()?;
    let rows = db_execute!(conn, {
        metric_cost_rollup_minute::table
            .filter(metric_cost_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms))
            .filter(metric_cost_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
            .filter(metric_cost_rollup_minute::dsl::metric_kind.eq(metric_kind_filter))
            .filter(metric_cost_rollup_minute::dsl::scope_type.eq(scope_type_filter))
            .filter(metric_cost_rollup_minute::dsl::scope_id.eq(scope_id_filter))
            .select(MetricCostRollupMinuteDb::as_select())
            .load::<MetricCostRollupMinuteDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(MetricCostRollupMinuteDb::from_db)
                    .collect::<Vec<_>>()
            })
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to query cost metrics window {}..{} for {} {}:{}: {}",
                    start_time_ms,
                    end_time_ms,
                    metric_kind_filter,
                    scope_type_filter,
                    scope_id_filter,
                    err
                )))
            })
    })?;

    let mut by_currency = BTreeMap::<String, i64>::new();
    for row in rows {
        *by_currency.entry(row.currency).or_default() += row.amount_nanos;
    }
    Ok(by_currency
        .into_iter()
        .map(|(currency, amount_nanos)| MetricCostAggregate {
            currency,
            amount_nanos,
        })
        .collect())
}

pub fn list_cost_rollup_minutes(
    start_time_ms: i64,
    end_time_ms: i64,
    metric_kind_filter: &str,
    scope_type_filter: Option<&str>,
    scope_id_filter: Option<&str>,
) -> DbResult<Vec<MetricCostRollupMinute>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        let mut query = metric_cost_rollup_minute::table
            .filter(metric_cost_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms))
            .filter(metric_cost_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
            .filter(metric_cost_rollup_minute::dsl::metric_kind.eq(metric_kind_filter))
            .into_boxed();

        if let Some(scope_type_filter) = scope_type_filter {
            query = query.filter(metric_cost_rollup_minute::dsl::scope_type.eq(scope_type_filter));
        }
        if let Some(scope_id_filter) = scope_id_filter {
            query = query.filter(metric_cost_rollup_minute::dsl::scope_id.eq(scope_id_filter));
        }

        query
            .order(metric_cost_rollup_minute::dsl::bucket_start_ms.asc())
            .select(MetricCostRollupMinuteDb::as_select())
            .load::<MetricCostRollupMinuteDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(MetricCostRollupMinuteDb::from_db)
                    .collect::<Vec<_>>()
            })
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to list cost metrics rollup minutes {}..{} for {}: {}",
                    start_time_ms, end_time_ms, metric_kind_filter, err
                )))
            })
    })
}

fn aggregate_request_rows(
    rows: Vec<MetricRequestRollupMinute>,
) -> Vec<MetricRequestWindowAggregate> {
    let mut by_scope = BTreeMap::<(String, String), MetricRequestWindowAggregate>::new();
    for row in rows {
        let entry = by_scope
            .entry((row.scope_type.clone(), row.scope_id.clone()))
            .or_insert_with(|| MetricRequestWindowAggregate {
                scope_type: row.scope_type.clone(),
                scope_id: row.scope_id.clone(),
                scope_label: row.scope_label.clone(),
                ..Default::default()
            });
        if row.scope_label.is_some() {
            entry.scope_label = row.scope_label;
        }
        entry.request_count += row.request_count;
        entry.success_count += row.success_count;
        entry.error_count += row.error_count;
        entry.cancelled_count += row.cancelled_count;
        entry.retry_count += row.retry_count;
        entry.fallback_count += row.fallback_count;
        entry.first_byte_latency_sum_ms += row.first_byte_latency_sum_ms;
        entry.first_byte_latency_count += row.first_byte_latency_count;
        entry.total_latency_sum_ms += row.total_latency_sum_ms;
        entry.total_latency_count += row.total_latency_count;
        entry.input_tokens += row.input_tokens;
        entry.output_tokens += row.output_tokens;
        entry.reasoning_tokens += row.reasoning_tokens;
        entry.total_tokens += row.total_tokens;
        entry.transform_diagnostic_count += row.transform_diagnostic_count;
        entry.transform_diagnostic_lossy_major_count += row.transform_diagnostic_lossy_major_count;
        entry.transform_diagnostic_reject_count += row.transform_diagnostic_reject_count;
        if row.request_count > 0 {
            update_latest_ms(&mut entry.last_request_at, row.bucket_start_ms);
        }
        if row.success_count > 0 {
            update_latest_ms(&mut entry.last_success_at, row.bucket_start_ms);
        }
        if row.error_count + row.cancelled_count > 0 {
            update_latest_ms(&mut entry.last_error_at, row.bucket_start_ms);
        }
    }

    by_scope.into_values().collect()
}

fn update_latest_ms(target: &mut Option<i64>, candidate: i64) {
    *target = Some(target.map_or(candidate, |current| current.max(candidate)));
}

fn aggregate_attempt_rows(
    rows: Vec<MetricAttemptRollupMinute>,
) -> Vec<MetricAttemptWindowAggregate> {
    let mut by_scope = BTreeMap::<(String, String), MetricAttemptWindowAggregate>::new();
    for row in rows {
        let entry = by_scope
            .entry((row.scope_type.clone(), row.scope_id.clone()))
            .or_insert_with(|| MetricAttemptWindowAggregate {
                scope_type: row.scope_type.clone(),
                scope_id: row.scope_id.clone(),
                scope_label: row.scope_label.clone(),
                ..Default::default()
            });
        if row.scope_label.is_some() {
            entry.scope_label = row.scope_label;
        }
        entry.attempt_count += row.attempt_count;
        entry.success_count += row.success_count;
        entry.error_count += row.error_count;
        entry.skipped_count += row.skipped_count;
        entry.retry_same_candidate_count += row.retry_same_candidate_count;
        entry.fallback_next_candidate_count += row.fallback_next_candidate_count;
        entry.fail_fast_count += row.fail_fast_count;
        entry.first_byte_latency_sum_ms += row.first_byte_latency_sum_ms;
        entry.first_byte_latency_count += row.first_byte_latency_count;
        entry.total_latency_sum_ms += row.total_latency_sum_ms;
        entry.total_latency_count += row.total_latency_count;
        entry.input_tokens += row.input_tokens;
        entry.output_tokens += row.output_tokens;
        entry.reasoning_tokens += row.reasoning_tokens;
        entry.total_tokens += row.total_tokens;
    }

    by_scope.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDbContext;

    fn request_delta(
        bucket_start_ms: i64,
        scope_type: &str,
        scope_id: &str,
    ) -> MetricRequestRollupMinute {
        MetricRequestRollupMinute {
            bucket_start_ms,
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            scope_label: Some(format!("{scope_type}:{scope_id}")),
            request_count: 1,
            success_count: 1,
            error_count: 0,
            cancelled_count: 0,
            retry_count: 1,
            fallback_count: 2,
            first_byte_latency_sum_ms: 100,
            first_byte_latency_count: 1,
            total_latency_sum_ms: 250,
            total_latency_count: 1,
            input_tokens: 10,
            output_tokens: 20,
            reasoning_tokens: 3,
            total_tokens: 33,
            transform_diagnostic_count: 1,
            transform_diagnostic_lossy_major_count: 1,
            transform_diagnostic_reject_count: 0,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn attempt_delta(
        bucket_start_ms: i64,
        scope_type: &str,
        scope_id: &str,
    ) -> MetricAttemptRollupMinute {
        MetricAttemptRollupMinute {
            bucket_start_ms,
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            scope_label: Some(format!("{scope_type}:{scope_id}")),
            attempt_count: 1,
            success_count: 0,
            error_count: 1,
            skipped_count: 0,
            retry_same_candidate_count: 1,
            fallback_next_candidate_count: 1,
            fail_fast_count: 0,
            first_byte_latency_sum_ms: 0,
            first_byte_latency_count: 0,
            total_latency_sum_ms: 400,
            total_latency_count: 1,
            input_tokens: 4,
            output_tokens: 5,
            reasoning_tokens: 6,
            total_tokens: 15,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn ingest_marker_is_idempotent() {
        let context = TestDbContext::new_sqlite("metrics-marker.sqlite");
        context.run_sync(|| {
            let marker = MetricIngestedRequestLog {
                request_log_id: 42,
                request_received_at: 1_000,
                completed_at: Some(1_500),
                ingested_at: 2_000,
            };

            assert!(insert_ingested_request_log_marker(&marker).unwrap());
            assert!(!insert_ingested_request_log_marker(&marker).unwrap());
            assert!(ingested_request_log_marker_exists(42).unwrap());
            assert!(!ingested_request_log_marker_exists(43).unwrap());
        });
    }

    #[test]
    fn request_and_attempt_rollup_deltas_accumulate_by_scope() {
        let context = TestDbContext::new_sqlite("metrics-rollup.sqlite");
        context.run_sync(|| {
            let mut second = request_delta(60_000, "provider", "7");
            second.success_count = 0;
            second.error_count = 1;
            second.cancelled_count = 1;
            second.updated_at = 2;

            let first = request_delta(60_000, "provider", "7");
            add_request_rollup_delta(&first).unwrap();
            let updated = add_request_rollup_delta(&second).unwrap();
            assert_eq!(updated.request_count, 2);
            assert_eq!(updated.success_count, 1);
            assert_eq!(updated.error_count, 1);
            assert_eq!(updated.cancelled_count, 1);
            assert_eq!(updated.retry_count, 2);
            assert_eq!(updated.fallback_count, 4);
            assert_eq!(updated.updated_at, 2);

            let aggregates =
                query_request_window_aggregates(0, 120_000, Some("provider"), Some("7")).unwrap();
            assert_eq!(aggregates.len(), 1);
            assert_eq!(aggregates[0].request_count, 2);
            assert_eq!(aggregates[0].first_byte_latency_sum_ms, 200);
            assert_eq!(aggregates[0].total_latency_count, 2);
            assert_eq!(aggregates[0].total_tokens, 66);
            assert_eq!(aggregates[0].transform_diagnostic_lossy_major_count, 2);

            add_attempt_rollup_delta(&attempt_delta(60_000, "provider", "7")).unwrap();
            add_attempt_rollup_delta(&attempt_delta(120_000, "provider", "7")).unwrap();
            let attempts =
                query_attempt_window_aggregates(0, 180_000, Some("provider"), Some("7")).unwrap();
            assert_eq!(attempts.len(), 1);
            assert_eq!(attempts[0].attempt_count, 2);
            assert_eq!(attempts[0].error_count, 2);
            assert_eq!(attempts[0].retry_same_candidate_count, 2);
            assert_eq!(attempts[0].fallback_next_candidate_count, 2);
            assert_eq!(attempts[0].total_latency_sum_ms, 800);
        });
    }

    #[test]
    fn cost_and_http_status_queries_aggregate_and_sort() {
        let context = TestDbContext::new_sqlite("metrics-cost-status.sqlite");
        context.run_sync(|| {
            add_http_status_rollup_delta(&MetricHttpStatusRollupMinute {
                bucket_start_ms: 60_000,
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                http_status: 500,
                count: 1,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
            add_http_status_rollup_delta(&MetricHttpStatusRollupMinute {
                bucket_start_ms: 120_000,
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                http_status: 429,
                count: 3,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
            add_http_status_rollup_delta(&MetricHttpStatusRollupMinute {
                bucket_start_ms: 120_000,
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                http_status: 500,
                count: 2,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();

            let statuses = query_http_status_breakdown(0, 180_000, "provider", "7").unwrap();
            assert_eq!(
                statuses,
                vec![
                    MetricHttpStatusCount {
                        status_code: 429,
                        count: 3,
                    },
                    MetricHttpStatusCount {
                        status_code: 500,
                        count: 3,
                    },
                ]
            );

            add_cost_rollup_delta(&MetricCostRollupMinute {
                bucket_start_ms: 60_000,
                metric_kind: "request".to_string(),
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                currency: "USD".to_string(),
                amount_nanos: 100,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
            add_cost_rollup_delta(&MetricCostRollupMinute {
                bucket_start_ms: 120_000,
                metric_kind: "request".to_string(),
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                currency: "USD".to_string(),
                amount_nanos: 250,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
            add_cost_rollup_delta(&MetricCostRollupMinute {
                bucket_start_ms: 120_000,
                metric_kind: "attempt".to_string(),
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                currency: "USD".to_string(),
                amount_nanos: 999,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();

            let costs =
                query_cost_window_aggregates(0, 180_000, "request", "provider", "7").unwrap();
            assert_eq!(
                costs,
                vec![MetricCostAggregate {
                    currency: "USD".to_string(),
                    amount_nanos: 350,
                }]
            );
        });
    }
}
