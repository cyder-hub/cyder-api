use std::collections::BTreeMap;

use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel::{QueryableByName, sql_query};
use serde::{Deserialize, Serialize};

use super::{
    DbResult,
    runtime::{
        DatabaseRuntime, DatabaseWorkload, RuntimeConnection, db_execute as async_db_execute,
    },
};
use crate::controller::BaseError;
use crate::db_object;

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
        pub time_to_first_response_body_sum_ms: i64,
        pub time_to_first_response_body_count: i64,
        pub ttft_sum_ms: i64,
        pub ttft_count: i64,
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
    #[diesel(primary_key(bucket_start_ms, scope_type, scope_id, currency))]
    pub struct MetricCostRollupMinute {
        pub bucket_start_ms: i64,
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
    pub time_to_first_response_body_sum_ms: i64,
    pub time_to_first_response_body_count: i64,
    pub ttft_sum_ms: i64,
    pub ttft_count: i64,
    pub total_latency_sum_ms: i64,
    pub total_latency_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub last_request_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error_at: Option<i64>,
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

macro_rules! upsert_request_rollup_delta_async_in_tx {
    ($conn:expr, $delta:expr) => {{
        let query = diesel::insert_into(metric_request_rollup_minute::table)
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
                metric_request_rollup_minute::dsl::time_to_first_response_body_sum_ms.eq(
                    metric_request_rollup_minute::dsl::time_to_first_response_body_sum_ms
                        + excluded(
                            metric_request_rollup_minute::dsl::time_to_first_response_body_sum_ms,
                        ),
                ),
                metric_request_rollup_minute::dsl::time_to_first_response_body_count.eq(
                    metric_request_rollup_minute::dsl::time_to_first_response_body_count
                        + excluded(
                            metric_request_rollup_minute::dsl::time_to_first_response_body_count,
                        ),
                ),
                metric_request_rollup_minute::dsl::ttft_sum_ms
                    .eq(metric_request_rollup_minute::dsl::ttft_sum_ms
                        + excluded(metric_request_rollup_minute::dsl::ttft_sum_ms)),
                metric_request_rollup_minute::dsl::ttft_count
                    .eq(metric_request_rollup_minute::dsl::ttft_count
                        + excluded(metric_request_rollup_minute::dsl::ttft_count)),
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
                metric_request_rollup_minute::dsl::updated_at
                    .eq(excluded(metric_request_rollup_minute::dsl::updated_at)),
            ));
        diesel_async::RunQueryDsl::execute(query, &mut *$conn)
            .await
            .map(|_| ())
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add request metrics delta for {}:{} bucket {}: {}",
                    $delta.scope_type, $delta.scope_id, $delta.bucket_start_ms, error
                )))
            })
    }};
}

macro_rules! upsert_http_status_rollup_delta_async_in_tx {
    ($conn:expr, $delta:expr) => {{
        let query = diesel::insert_into(metric_http_status_rollup_minute::table)
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
            ));
        diesel_async::RunQueryDsl::execute(query, &mut *$conn)
            .await
            .map(|_| ())
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add HTTP status metrics delta for {}:{} status {} bucket {}: {}",
                    $delta.scope_type,
                    $delta.scope_id,
                    $delta.http_status,
                    $delta.bucket_start_ms,
                    error
                )))
            })
    }};
}

macro_rules! upsert_cost_rollup_delta_async_in_tx {
    ($conn:expr, $delta:expr) => {{
        let query = diesel::insert_into(metric_cost_rollup_minute::table)
            .values(MetricCostRollupMinuteDb::to_db($delta))
            .on_conflict((
                metric_cost_rollup_minute::dsl::bucket_start_ms,
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
            ));
        diesel_async::RunQueryDsl::execute(query, &mut *$conn)
            .await
            .map(|_| ())
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to add cost metrics delta for {}:{} {} bucket {}: {}",
                    $delta.scope_type,
                    $delta.scope_id,
                    $delta.currency,
                    $delta.bucket_start_ms,
                    error
                )))
            })
    }};
}

pub async fn ingest_metric_rollups(
    database: &DatabaseRuntime,
    marker: &MetricIngestedRequestLog,
    request_rollups: &[MetricRequestRollupMinute],
    http_status_rollups: &[MetricHttpStatusRollupMinute],
    cost_rollups: &[MetricCostRollupMinute],
) -> DbResult<bool> {
    let marker = marker.clone();
    let request_rollups = request_rollups.to_vec();
    let http_status_rollups = http_status_rollups.to_vec();
    let cost_rollups = cost_rollups.to_vec();
    database
        .run_db(DatabaseWorkload::Background, move |connection| {
            Box::pin(async move {
                async_db_execute!(connection as conn, {
                    use diesel_async::AsyncConnection;
                    conn.transaction(async move |conn| {
                        let query = diesel::insert_into(metric_ingested_request_log::table)
                            .values(MetricIngestedRequestLogDb::to_db(&marker))
                            .on_conflict(metric_ingested_request_log::dsl::request_log_id)
                            .do_nothing();
                        let inserted = diesel_async::RunQueryDsl::execute(query, &mut *conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to insert metrics ingest marker for request_log {}: {}",
                                    marker.request_log_id, error
                                )))
                            })?;
                        if inserted == 0 {
                            return Ok(false);
                        }
                        for delta in &request_rollups {
                            upsert_request_rollup_delta_async_in_tx!(conn, delta)?;
                        }
                        for delta in &http_status_rollups {
                            upsert_http_status_rollup_delta_async_in_tx!(conn, delta)?;
                        }
                        for delta in &cost_rollups {
                            upsert_cost_rollup_delta_async_in_tx!(conn, delta)?;
                        }
                        Ok(true)
                    })
                    .await
                })
            })
        })
        .await
}

pub async fn delete_metrics_data_in_range(
    database: &DatabaseRuntime,
    marker_start_time_ms: i64,
    marker_end_time_ms: i64,
    rollup_start_time_ms: i64,
    rollup_end_time_ms: i64,
) -> DbResult<MetricsRepairDeleteSummary> {
    database
        .run_db(DatabaseWorkload::Background, move |connection| {
            Box::pin(async move {
                async_db_execute!(connection as conn, {
                    use diesel_async::AsyncConnection;
                    conn.transaction(async move |conn| {
                        let cost_query = diesel::delete(
                            metric_cost_rollup_minute::table
                                .filter(
                                    metric_cost_rollup_minute::dsl::bucket_start_ms
                                        .ge(rollup_start_time_ms),
                                )
                                .filter(
                                    metric_cost_rollup_minute::dsl::bucket_start_ms
                                        .lt(rollup_end_time_ms),
                                ),
                        );
                        let deleted_cost_rollups =
                            diesel_async::RunQueryDsl::execute(cost_query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to delete cost metrics rollups {}..{}: {}",
                                        rollup_start_time_ms, rollup_end_time_ms, error
                                    )))
                                })?;
                        let status_query = diesel::delete(
                            metric_http_status_rollup_minute::table
                                .filter(
                                    metric_http_status_rollup_minute::dsl::bucket_start_ms
                                        .ge(rollup_start_time_ms),
                                )
                                .filter(
                                    metric_http_status_rollup_minute::dsl::bucket_start_ms
                                        .lt(rollup_end_time_ms),
                                ),
                        );
                        let deleted_http_status_rollups =
                            diesel_async::RunQueryDsl::execute(status_query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to delete HTTP status metrics rollups {}..{}: {}",
                                        rollup_start_time_ms, rollup_end_time_ms, error
                                    )))
                                })?;
                        let request_query = diesel::delete(
                            metric_request_rollup_minute::table
                                .filter(
                                    metric_request_rollup_minute::dsl::bucket_start_ms
                                        .ge(rollup_start_time_ms),
                                )
                                .filter(
                                    metric_request_rollup_minute::dsl::bucket_start_ms
                                        .lt(rollup_end_time_ms),
                                ),
                        );
                        let deleted_request_rollups =
                            diesel_async::RunQueryDsl::execute(request_query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to delete request metrics rollups {}..{}: {}",
                                        rollup_start_time_ms, rollup_end_time_ms, error
                                    )))
                                })?;
                        let marker_query = diesel::delete(
                            metric_ingested_request_log::table
                                .filter(
                                    metric_ingested_request_log::dsl::request_received_at
                                        .ge(marker_start_time_ms),
                                )
                                .filter(
                                    metric_ingested_request_log::dsl::request_received_at
                                        .lt(marker_end_time_ms),
                                ),
                        );
                        let deleted_ingest_markers =
                            diesel_async::RunQueryDsl::execute(marker_query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to delete metrics ingest markers {}..{}: {}",
                                        marker_start_time_ms, marker_end_time_ms, error
                                    )))
                                })?;
                        Ok(MetricsRepairDeleteSummary {
                            deleted_ingest_markers,
                            deleted_request_rollups,
                            deleted_http_status_rollups,
                            deleted_cost_rollups,
                        })
                    })
                    .await
                })
            })
        })
        .await
}

pub async fn count_ingested_request_log_markers(database: &DatabaseRuntime) -> DbResult<i64> {
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                async_db_execute!(connection as conn, {
                    let query = metric_ingested_request_log::table.count();
                    diesel_async::RunQueryDsl::get_result::<i64>(query, &mut **conn)
                        .await
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to count metrics ingest markers: {}",
                                error
                            )))
                        })
                })
            })
        })
        .await
}

pub async fn count_uningested_request_logs_in_range(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
) -> DbResult<i64> {
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                let result = match connection {
                    RuntimeConnection::Postgres(conn) => {
                        let query = sql_query(
                            "SELECT COUNT(*) AS count
                             FROM request_log rl
                             LEFT JOIN metric_ingested_request_log marker
                               ON marker.request_log_id = rl.id
                             WHERE rl.request_received_at >= $1
                               AND rl.request_received_at < $2
                               AND marker.request_log_id IS NULL",
                        )
                        .bind::<diesel::sql_types::BigInt, _>(start_time_ms)
                        .bind::<diesel::sql_types::BigInt, _>(end_time_ms);
                        diesel_async::RunQueryDsl::get_result::<CountRow>(query, &mut **conn).await
                    }
                    RuntimeConnection::Sqlite(conn) => {
                        let query = sql_query(
                            "SELECT COUNT(*) AS count
                             FROM request_log rl
                             LEFT JOIN metric_ingested_request_log marker
                               ON marker.request_log_id = rl.id
                             WHERE rl.request_received_at >= ?
                               AND rl.request_received_at < ?
                               AND marker.request_log_id IS NULL",
                        )
                        .bind::<diesel::sql_types::BigInt, _>(start_time_ms)
                        .bind::<diesel::sql_types::BigInt, _>(end_time_ms);
                        diesel_async::RunQueryDsl::get_result::<CountRow>(query, &mut **conn).await
                    }
                };
                result.map(|row| row.count).map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to count uningested request logs {}..{}: {}",
                        start_time_ms, end_time_ms, error
                    )))
                })
            })
        })
        .await
}

pub async fn list_uningested_request_log_ids(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
    limit: i64,
) -> DbResult<Vec<ReconciliationRequestLogRef>> {
    database
        .run_db(DatabaseWorkload::Background, move |connection| {
            Box::pin(async move {
                let result = match connection {
                    RuntimeConnection::Postgres(conn) => {
                        let query = sql_query(
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
                        .bind::<diesel::sql_types::BigInt, _>(limit);
                        diesel_async::RunQueryDsl::load::<ReconciliationRequestLogRef>(
                            query,
                            &mut **conn,
                        )
                        .await
                    }
                    RuntimeConnection::Sqlite(conn) => {
                        let query = sql_query(
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
                        .bind::<diesel::sql_types::BigInt, _>(limit);
                        diesel_async::RunQueryDsl::load::<ReconciliationRequestLogRef>(
                            query,
                            &mut **conn,
                        )
                        .await
                    }
                };
                result.map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list uningested request logs {}..{}: {}",
                        start_time_ms, end_time_ms, error
                    )))
                })
            })
        })
        .await
}

pub async fn list_request_log_ids_in_range_after(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
    after_request_received_at: i64,
    after_request_log_id: i64,
    limit: i64,
) -> DbResult<Vec<ReconciliationRequestLogRef>> {
    database
        .run_db(DatabaseWorkload::Background, move |connection| {
            Box::pin(async move {
                let result = match connection {
                    RuntimeConnection::Postgres(conn) => {
                        let query = sql_query(
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
                        .bind::<diesel::sql_types::BigInt, _>(limit);
                        diesel_async::RunQueryDsl::load::<ReconciliationRequestLogRef>(
                            query,
                            &mut **conn,
                        )
                        .await
                    }
                    RuntimeConnection::Sqlite(conn) => {
                        let query = sql_query(
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
                        .bind::<diesel::sql_types::BigInt, _>(limit);
                        diesel_async::RunQueryDsl::load::<ReconciliationRequestLogRef>(
                            query,
                            &mut **conn,
                        )
                        .await
                    }
                };
                result.map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list request logs {}..{} after {}:{}: {}",
                        start_time_ms,
                        end_time_ms,
                        after_request_received_at,
                        after_request_log_id,
                        error
                    )))
                })
            })
        })
        .await
}

pub async fn query_request_window_aggregates(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: Option<&str>,
    scope_id_filter: Option<&str>,
) -> DbResult<Vec<MetricRequestWindowAggregate>> {
    let scope_type_filter = scope_type_filter.map(str::to_string);
    let scope_id_filter = scope_id_filter.map(str::to_string);
    let rows = database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                async_db_execute!(connection as conn, {
                    let mut query = metric_request_rollup_minute::table
                        .filter(
                            metric_request_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms),
                        )
                        .filter(metric_request_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
                        .into_boxed();
                    if let Some(scope_type_filter) = scope_type_filter.as_deref() {
                        query = query.filter(
                            metric_request_rollup_minute::dsl::scope_type.eq(scope_type_filter),
                        );
                    }
                    if let Some(scope_id_filter) = scope_id_filter.as_deref() {
                        query = query.filter(
                            metric_request_rollup_minute::dsl::scope_id.eq(scope_id_filter),
                        );
                    }
                    let query = query.select(MetricRequestRollupMinuteDb::as_select());
                    diesel_async::RunQueryDsl::load::<MetricRequestRollupMinuteDb>(
                        query,
                        &mut **conn,
                    )
                    .await
                    .map(|rows| {
                        rows.into_iter()
                            .map(MetricRequestRollupMinuteDb::from_db)
                            .collect::<Vec<_>>()
                    })
                    .map_err(|error| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to query request metrics window {}..{}: {}",
                            start_time_ms, end_time_ms, error
                        )))
                    })
                })
            })
        })
        .await?;
    Ok(aggregate_request_rows(rows))
}

pub async fn list_request_rollup_minutes(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: Option<&str>,
    scope_id_filter: Option<&str>,
) -> DbResult<Vec<MetricRequestRollupMinute>> {
    let scope_type_filter = scope_type_filter.map(str::to_string);
    let scope_id_filter = scope_id_filter.map(str::to_string);
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                async_db_execute!(connection as conn, {
                    let mut query = metric_request_rollup_minute::table
                        .filter(
                            metric_request_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms),
                        )
                        .filter(metric_request_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
                        .into_boxed();
                    if let Some(scope_type_filter) = scope_type_filter.as_deref() {
                        query = query.filter(
                            metric_request_rollup_minute::dsl::scope_type.eq(scope_type_filter),
                        );
                    }
                    if let Some(scope_id_filter) = scope_id_filter.as_deref() {
                        query = query.filter(
                            metric_request_rollup_minute::dsl::scope_id.eq(scope_id_filter),
                        );
                    }
                    let query = query
                        .order(metric_request_rollup_minute::dsl::bucket_start_ms.asc())
                        .select(MetricRequestRollupMinuteDb::as_select());
                    diesel_async::RunQueryDsl::load::<MetricRequestRollupMinuteDb>(
                        query,
                        &mut **conn,
                    )
                    .await
                    .map(|rows| {
                        rows.into_iter()
                            .map(MetricRequestRollupMinuteDb::from_db)
                            .collect()
                    })
                    .map_err(|error| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to list request metrics rollup minutes {}..{}: {}",
                            start_time_ms, end_time_ms, error
                        )))
                    })
                })
            })
        })
        .await
}

pub async fn query_http_status_breakdown(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: &str,
    scope_id_filter: &str,
) -> DbResult<Vec<MetricHttpStatusCount>> {
    let scope_type_filter = scope_type_filter.to_string();
    let scope_id_filter = scope_id_filter.to_string();
    let rows = database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                async_db_execute!(connection as conn, {
                    let query = metric_http_status_rollup_minute::table
                        .filter(
                            metric_http_status_rollup_minute::dsl::bucket_start_ms
                                .ge(start_time_ms),
                        )
                        .filter(
                            metric_http_status_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms),
                        )
                        .filter(
                            metric_http_status_rollup_minute::dsl::scope_type
                                .eq(&scope_type_filter),
                        )
                        .filter(
                            metric_http_status_rollup_minute::dsl::scope_id.eq(&scope_id_filter),
                        )
                        .select(MetricHttpStatusRollupMinuteDb::as_select());
                    diesel_async::RunQueryDsl::load::<MetricHttpStatusRollupMinuteDb>(
                        query,
                        &mut **conn,
                    )
                    .await
                    .map(|rows| {
                        rows.into_iter()
                            .map(MetricHttpStatusRollupMinuteDb::from_db)
                            .collect::<Vec<_>>()
                    })
                    .map_err(|error| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to query HTTP status metrics window {}..{} for {}:{}: {}",
                            start_time_ms, end_time_ms, scope_type_filter, scope_id_filter, error
                        )))
                    })
                })
            })
        })
        .await?;
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

pub async fn list_http_status_rollup_minutes(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: &str,
    scope_id_filter: Option<&str>,
) -> DbResult<Vec<MetricHttpStatusRollupMinute>> {
    let scope_type_filter = scope_type_filter.to_string();
    let scope_id_filter = scope_id_filter.map(str::to_string);
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                async_db_execute!(connection as conn, {
                    let mut query = metric_http_status_rollup_minute::table
                        .filter(
                            metric_http_status_rollup_minute::dsl::bucket_start_ms
                                .ge(start_time_ms),
                        )
                        .filter(
                            metric_http_status_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms),
                        )
                        .filter(
                            metric_http_status_rollup_minute::dsl::scope_type
                                .eq(&scope_type_filter),
                        )
                        .into_boxed();
                    if let Some(scope_id_filter) = scope_id_filter.as_deref() {
                        query = query.filter(
                            metric_http_status_rollup_minute::dsl::scope_id.eq(scope_id_filter),
                        );
                    }
                    let query = query.select(MetricHttpStatusRollupMinuteDb::as_select());
                    diesel_async::RunQueryDsl::load::<MetricHttpStatusRollupMinuteDb>(
                        query,
                        &mut **conn,
                    )
                    .await
                    .map(|rows| {
                        rows.into_iter()
                            .map(MetricHttpStatusRollupMinuteDb::from_db)
                            .collect()
                    })
                    .map_err(|error| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to list HTTP status metrics window {}..{} for {}: {}",
                            start_time_ms, end_time_ms, scope_type_filter, error
                        )))
                    })
                })
            })
        })
        .await
}

pub async fn query_cost_window_aggregates(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: &str,
    scope_id_filter: &str,
) -> DbResult<Vec<MetricCostAggregate>> {
    let scope_type_filter = scope_type_filter.to_string();
    let scope_id_filter = scope_id_filter.to_string();
    let rows = database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                async_db_execute!(connection as conn, {
                    let query = metric_cost_rollup_minute::table
                        .filter(metric_cost_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms))
                        .filter(metric_cost_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
                        .filter(metric_cost_rollup_minute::dsl::scope_type.eq(&scope_type_filter))
                        .filter(metric_cost_rollup_minute::dsl::scope_id.eq(&scope_id_filter))
                        .select(MetricCostRollupMinuteDb::as_select());
                    diesel_async::RunQueryDsl::load::<MetricCostRollupMinuteDb>(query, &mut **conn)
                        .await
                        .map(|rows| {
                            rows.into_iter()
                                .map(MetricCostRollupMinuteDb::from_db)
                                .collect::<Vec<_>>()
                        })
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to query cost metrics window {}..{} for {}:{}: {}",
                                start_time_ms,
                                end_time_ms,
                                scope_type_filter,
                                scope_id_filter,
                                error
                            )))
                        })
                })
            })
        })
        .await?;
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

pub async fn list_cost_rollup_minutes(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
    scope_type_filter: Option<&str>,
    scope_id_filter: Option<&str>,
) -> DbResult<Vec<MetricCostRollupMinute>> {
    let scope_type_filter = scope_type_filter.map(str::to_string);
    let scope_id_filter = scope_id_filter.map(str::to_string);
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                async_db_execute!(connection as conn, {
                    let mut query = metric_cost_rollup_minute::table
                        .filter(metric_cost_rollup_minute::dsl::bucket_start_ms.ge(start_time_ms))
                        .filter(metric_cost_rollup_minute::dsl::bucket_start_ms.lt(end_time_ms))
                        .into_boxed();
                    if let Some(scope_type_filter) = scope_type_filter.as_deref() {
                        query = query.filter(
                            metric_cost_rollup_minute::dsl::scope_type.eq(scope_type_filter),
                        );
                    }
                    if let Some(scope_id_filter) = scope_id_filter.as_deref() {
                        query = query
                            .filter(metric_cost_rollup_minute::dsl::scope_id.eq(scope_id_filter));
                    }
                    let query = query
                        .order(metric_cost_rollup_minute::dsl::bucket_start_ms.asc())
                        .select(MetricCostRollupMinuteDb::as_select());
                    diesel_async::RunQueryDsl::load::<MetricCostRollupMinuteDb>(query, &mut **conn)
                        .await
                        .map(|rows| {
                            rows.into_iter()
                                .map(MetricCostRollupMinuteDb::from_db)
                                .collect()
                        })
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to list cost metrics rollup minutes {}..{}: {}",
                                start_time_ms, end_time_ms, error
                            )))
                        })
                })
            })
        })
        .await
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
        entry.time_to_first_response_body_sum_ms += row.time_to_first_response_body_sum_ms;
        entry.time_to_first_response_body_count += row.time_to_first_response_body_count;
        entry.ttft_sum_ms += row.ttft_sum_ms;
        entry.ttft_count += row.ttft_count;
        entry.total_latency_sum_ms += row.total_latency_sum_ms;
        entry.total_latency_count += row.total_latency_count;
        entry.input_tokens += row.input_tokens;
        entry.output_tokens += row.output_tokens;
        entry.reasoning_tokens += row.reasoning_tokens;
        entry.total_tokens += row.total_tokens;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDatabase;

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
            time_to_first_response_body_sum_ms: 100,
            time_to_first_response_body_count: 1,
            ttft_sum_ms: 80,
            ttft_count: 1,
            total_latency_sum_ms: 250,
            total_latency_count: 1,
            input_tokens: 10,
            output_tokens: 20,
            reasoning_tokens: 3,
            total_tokens: 33,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[tokio::test]
    async fn ingest_marker_is_idempotent() {
        let context = TestDatabase::new_sqlite_default("metrics-marker.sqlite").await;
        let database = context.runtime();
        let marker = MetricIngestedRequestLog {
            request_log_id: 42,
            request_received_at: 1_000,
            completed_at: Some(1_500),
            ingested_at: 2_000,
        };

        assert!(
            ingest_metric_rollups(&database, &marker, &[], &[], &[])
                .await
                .unwrap()
        );
        assert!(
            !ingest_metric_rollups(&database, &marker, &[], &[], &[])
                .await
                .unwrap()
        );
        assert_eq!(
            count_ingested_request_log_markers(&database).await.unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn request_rollup_deltas_accumulate_by_scope() {
        let context = TestDatabase::new_sqlite_default("metrics-rollup.sqlite").await;
        let database = context.runtime();
        let mut second = request_delta(60_000, "provider", "7");
        second.success_count = 0;
        second.error_count = 1;
        second.cancelled_count = 1;
        second.updated_at = 2;

        let first = request_delta(60_000, "provider", "7");
        for (request_log_id, delta) in [(1, first), (2, second)] {
            assert!(
                ingest_metric_rollups(
                    &database,
                    &MetricIngestedRequestLog {
                        request_log_id,
                        request_received_at: 60_000,
                        completed_at: Some(60_250),
                        ingested_at: 120_000,
                    },
                    &[delta],
                    &[],
                    &[],
                )
                .await
                .unwrap()
            );
        }

        let aggregates =
            query_request_window_aggregates(&database, 0, 120_000, Some("provider"), Some("7"))
                .await
                .unwrap();
        assert_eq!(aggregates.len(), 1);
        assert_eq!(aggregates[0].request_count, 2);
        assert_eq!(aggregates[0].success_count, 1);
        assert_eq!(aggregates[0].error_count, 1);
        assert_eq!(aggregates[0].cancelled_count, 1);
        assert_eq!(aggregates[0].time_to_first_response_body_sum_ms, 200);
        assert_eq!(aggregates[0].ttft_sum_ms, 160);
        assert_eq!(aggregates[0].ttft_count, 2);
        assert_eq!(aggregates[0].total_latency_count, 2);
        assert_eq!(aggregates[0].total_tokens, 66);
    }

    #[tokio::test]
    async fn request_rollup_window_preserves_independent_latency_sample_counts() {
        let context = TestDatabase::new_sqlite_default("metrics-weighted-window.sqlite").await;
        let database = context.runtime();
        let first = request_delta(60_000, "global", "global");
        let mut second = request_delta(120_000, "global", "global");
        second.time_to_first_response_body_sum_ms = 300;
        second.time_to_first_response_body_count = 3;
        second.ttft_sum_ms = 600;
        second.ttft_count = 3;
        second.total_latency_sum_ms = 900;
        second.total_latency_count = 3;

        for (request_log_id, delta) in [(1, first), (2, second)] {
            ingest_metric_rollups(
                &database,
                &MetricIngestedRequestLog {
                    request_log_id,
                    request_received_at: delta.bucket_start_ms,
                    completed_at: None,
                    ingested_at: 180_000,
                },
                &[delta],
                &[],
                &[],
            )
            .await
            .unwrap();
        }

        let aggregate =
            query_request_window_aggregates(&database, 0, 180_000, Some("global"), None)
                .await
                .unwrap()
                .into_iter()
                .next()
                .expect("global aggregate");
        assert_eq!(aggregate.time_to_first_response_body_sum_ms, 400);
        assert_eq!(aggregate.time_to_first_response_body_count, 4);
        assert_eq!(aggregate.ttft_sum_ms, 680);
        assert_eq!(aggregate.ttft_count, 4);
        assert_eq!(aggregate.total_latency_sum_ms, 1_150);
        assert_eq!(aggregate.total_latency_count, 4);
        assert_eq!(
            aggregate.time_to_first_response_body_sum_ms as f64
                / aggregate.time_to_first_response_body_count as f64,
            100.0
        );
        assert_eq!(
            aggregate.ttft_sum_ms as f64 / aggregate.ttft_count as f64,
            170.0
        );
        assert_eq!(
            aggregate.total_latency_sum_ms as f64 / aggregate.total_latency_count as f64,
            287.5
        );
    }

    #[tokio::test]
    async fn cost_and_http_status_queries_aggregate_and_sort() {
        let context = TestDatabase::new_sqlite_default("metrics-cost-status.sqlite").await;
        let database = context.runtime();
        let http_status_rollups = [
            MetricHttpStatusRollupMinute {
                bucket_start_ms: 60_000,
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                http_status: 500,
                count: 1,
                created_at: 1,
                updated_at: 1,
            },
            MetricHttpStatusRollupMinute {
                bucket_start_ms: 120_000,
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                http_status: 429,
                count: 3,
                created_at: 1,
                updated_at: 1,
            },
            MetricHttpStatusRollupMinute {
                bucket_start_ms: 120_000,
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                http_status: 500,
                count: 2,
                created_at: 1,
                updated_at: 1,
            },
        ];
        let cost_rollups = [
            MetricCostRollupMinute {
                bucket_start_ms: 60_000,
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                currency: "USD".to_string(),
                amount_nanos: 100,
                created_at: 1,
                updated_at: 1,
            },
            MetricCostRollupMinute {
                bucket_start_ms: 120_000,
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                currency: "USD".to_string(),
                amount_nanos: 250,
                created_at: 1,
                updated_at: 1,
            },
        ];
        ingest_metric_rollups(
            &database,
            &MetricIngestedRequestLog {
                request_log_id: 1,
                request_received_at: 60_000,
                completed_at: Some(60_250),
                ingested_at: 180_000,
            },
            &[],
            &http_status_rollups,
            &cost_rollups,
        )
        .await
        .unwrap();

        let statuses = query_http_status_breakdown(&database, 0, 180_000, "provider", "7")
            .await
            .unwrap();
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

        let costs = query_cost_window_aggregates(&database, 0, 180_000, "provider", "7")
            .await
            .unwrap();
        assert_eq!(
            costs,
            vec![MetricCostAggregate {
                currency: "USD".to_string(),
                amount_nanos: 350,
            }]
        );
    }
}
