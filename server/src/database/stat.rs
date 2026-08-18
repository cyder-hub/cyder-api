use crate::controller::BaseError;
use crate::database::DbResult;
use crate::database::runtime::{DatabaseRuntime, DatabaseWorkload, RuntimeConnection};
use crate::db_object;
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use diesel::QueryableByName;
use diesel::dsl::{count_star, sum};
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Double, Nullable, Text};
use serde::Serialize;
use std::collections::HashMap;

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug)]
    #[diesel(table_name = api_key)]
    pub struct StatApiKeyRow {
        pub id: i64,
    }
}

#[derive(Queryable, QueryableByName, Debug)]
pub struct RequestLogEntryForStats {
    // from request_log
    #[diesel(sql_type = BigInt)]
    pub created_at: i64,
    #[diesel(sql_type = BigInt)]
    pub provider_id: i64,
    #[diesel(sql_type = BigInt)]
    pub model_id: i64,
    #[diesel(sql_type = Nullable<diesel::sql_types::Integer>)]
    pub total_input_tokens: Option<i32>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Integer>)]
    pub total_output_tokens: Option<i32>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Integer>)]
    pub reasoning_tokens: Option<i32>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Integer>)]
    pub total_tokens: Option<i32>,
    #[diesel(sql_type = Nullable<BigInt>)]
    pub estimated_cost_nanos: Option<i64>,
    #[diesel(sql_type = Nullable<Text>)]
    pub estimated_cost_currency: Option<String>,
    // from joined tables
    #[diesel(sql_type = Nullable<Text>)]
    pub provider_key: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub model_name: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub real_model_name: Option<String>,
}

#[derive(Serialize, Debug, Default)]
pub struct SystemOverviewStats {
    pub providers_count: i64,
    pub models_count: i64,
    pub provider_keys_count: i64,
}

#[derive(Serialize, Debug, Default)]
pub struct TodayRequestLogStats {
    pub requests_count: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_reasoning_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: HashMap<String, i64>,
}

#[derive(Serialize, Debug, Default)]
pub struct DashboardOverviewStats {
    pub provider_count: i64,
    pub enabled_provider_count: i64,
    pub model_count: i64,
    pub enabled_model_count: i64,
    pub provider_key_count: i64,
    pub enabled_provider_key_count: i64,
    pub api_key_count: i64,
    pub enabled_api_key_count: i64,
}

#[derive(Serialize, Debug, Default)]
pub struct DashboardTodayStats {
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub success_rate: Option<f64>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_reasoning_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: HashMap<String, i64>,
    pub avg_time_to_first_response_body_ms: Option<f64>,
    pub time_to_first_response_body_sample_count: i64,
    pub avg_ttft_ms: Option<f64>,
    pub ttft_sample_count: i64,
    pub avg_total_latency_ms: Option<f64>,
    pub total_latency_sample_count: i64,
    pub active_provider_count: i64,
    pub active_model_count: i64,
    pub active_api_key_count: i64,
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct DashboardTopModelItem {
    pub provider_id: i64,
    pub provider_key: String,
    pub model_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub request_count: i64,
    pub total_tokens: i64,
    pub total_cost: HashMap<String, i64>,
}

#[derive(QueryableByName, Debug)]
struct CostByCurrencyRow {
    #[diesel(sql_type = Text)]
    currency: String,
    #[diesel(sql_type = BigInt)]
    total_cost_nanos: i64,
}

#[derive(QueryableByName, Debug)]
struct DashboardTopModelBaseRow {
    #[diesel(sql_type = BigInt)]
    provider_id: i64,
    #[diesel(sql_type = Nullable<Text>)]
    provider_key: Option<String>,
    #[diesel(sql_type = BigInt)]
    model_id: i64,
    #[diesel(sql_type = Nullable<Text>)]
    model_name: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    real_model_name: Option<String>,
    #[diesel(sql_type = BigInt)]
    request_count: i64,
    #[diesel(sql_type = BigInt)]
    total_tokens: i64,
}

#[derive(QueryableByName, Debug)]
struct DashboardTopModelCostRow {
    #[diesel(sql_type = BigInt)]
    provider_id: i64,
    #[diesel(sql_type = BigInt)]
    model_id: i64,
    #[diesel(sql_type = Text)]
    currency: String,
    #[diesel(sql_type = BigInt)]
    total_cost_nanos: i64,
}

#[derive(QueryableByName, Debug)]
struct DashboardTodayAggregateRow {
    #[diesel(sql_type = BigInt)]
    request_count: i64,
    #[diesel(sql_type = BigInt)]
    success_count: i64,
    #[diesel(sql_type = BigInt)]
    error_count: i64,
    #[diesel(sql_type = BigInt)]
    total_input_tokens: i64,
    #[diesel(sql_type = BigInt)]
    total_output_tokens: i64,
    #[diesel(sql_type = BigInt)]
    total_reasoning_tokens: i64,
    #[diesel(sql_type = BigInt)]
    total_tokens: i64,
    #[diesel(sql_type = Nullable<Double>)]
    avg_time_to_first_response_body_ms: Option<f64>,
    #[diesel(sql_type = BigInt)]
    time_to_first_response_body_sample_count: i64,
    #[diesel(sql_type = Nullable<Double>)]
    avg_ttft_ms: Option<f64>,
    #[diesel(sql_type = BigInt)]
    ttft_sample_count: i64,
    #[diesel(sql_type = Nullable<Double>)]
    avg_total_latency_ms: Option<f64>,
    #[diesel(sql_type = BigInt)]
    total_latency_sample_count: i64,
    #[diesel(sql_type = BigInt)]
    active_provider_count: i64,
    #[diesel(sql_type = BigInt)]
    active_model_count: i64,
    #[diesel(sql_type = BigInt)]
    active_api_key_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageStatsGroupBy {
    Provider,
    Model,
    ApiKey,
}

#[derive(Debug)]
pub struct UsageStatsQueryItem {
    pub time: i64,
    pub group_id: i64,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub api_key_id: Option<i64>,
    pub provider_key: Option<String>,
    pub model_name: Option<String>,
    pub real_model_name: Option<String>,
    pub api_key_name: Option<String>,
    pub group_label: String,
    pub group_detail: Option<String>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_reasoning_tokens: i64,
    pub total_tokens: i64,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub success_rate: Option<f64>,
    pub avg_time_to_first_response_body_ms: Option<f64>,
    pub time_to_first_response_body_sample_count: i64,
    pub avg_ttft_ms: Option<f64>,
    pub ttft_sample_count: i64,
    pub avg_total_latency_ms: Option<f64>,
    pub total_latency_sample_count: i64,
    pub total_cost: HashMap<String, i64>,
}

#[derive(QueryableByName, Debug)]
struct UsageStatsBaseRow {
    #[diesel(sql_type = BigInt)]
    time_bucket: i64,
    #[diesel(sql_type = BigInt)]
    group_id: i64,
    #[diesel(sql_type = Nullable<BigInt>)]
    provider_id: Option<i64>,
    #[diesel(sql_type = Nullable<BigInt>)]
    model_id: Option<i64>,
    #[diesel(sql_type = Nullable<BigInt>)]
    api_key_id: Option<i64>,
    #[diesel(sql_type = Nullable<Text>)]
    provider_key: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    model_name: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    real_model_name: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    api_key_name: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    group_label: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    group_detail: Option<String>,
    #[diesel(sql_type = BigInt)]
    total_input_tokens: i64,
    #[diesel(sql_type = BigInt)]
    total_output_tokens: i64,
    #[diesel(sql_type = BigInt)]
    total_reasoning_tokens: i64,
    #[diesel(sql_type = BigInt)]
    total_tokens: i64,
    #[diesel(sql_type = BigInt)]
    request_count: i64,
    #[diesel(sql_type = BigInt)]
    success_count: i64,
    #[diesel(sql_type = BigInt)]
    error_count: i64,
    #[diesel(sql_type = Nullable<Double>)]
    time_to_first_response_body_sum_ms: Option<f64>,
    #[diesel(sql_type = BigInt)]
    time_to_first_response_body_sample_count: i64,
    #[diesel(sql_type = Nullable<Double>)]
    ttft_sum_ms: Option<f64>,
    #[diesel(sql_type = BigInt)]
    ttft_sample_count: i64,
    #[diesel(sql_type = Nullable<Double>)]
    total_latency_sum_ms: Option<f64>,
    #[diesel(sql_type = BigInt)]
    total_latency_sample_count: i64,
}

#[derive(QueryableByName, Debug)]
struct UsageStatsCostRow {
    #[diesel(sql_type = BigInt)]
    time_bucket: i64,
    #[diesel(sql_type = BigInt)]
    group_id: i64,
    #[diesel(sql_type = Text)]
    currency: String,
    #[diesel(sql_type = BigInt)]
    total_cost_nanos: i64,
}

pub async fn get_system_overview_stats(
    database: &DatabaseRuntime,
) -> DbResult<SystemOverviewStats> {
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                match connection {
                    RuntimeConnection::Postgres(conn) => {
                        use crate::database::_postgres_schema::*;
                        let providers_query = provider::table
                            .filter(provider::dsl::deleted_at.is_null())
                            .select(count_star());
                        let providers_count =
                            diesel_async::RunQueryDsl::first::<i64>(providers_query, &mut **conn)
                                .await?;
                        let models_query = model::table
                            .filter(model::dsl::deleted_at.is_null())
                            .select(count_star());
                        let models_count =
                            diesel_async::RunQueryDsl::first::<i64>(models_query, &mut **conn)
                                .await?;
                        let provider_keys_query = provider_api_key::table
                            .filter(provider_api_key::dsl::deleted_at.is_null())
                            .select(count_star());
                        let provider_keys_count = diesel_async::RunQueryDsl::first::<i64>(
                            provider_keys_query,
                            &mut **conn,
                        )
                        .await?;
                        Ok(SystemOverviewStats {
                            providers_count,
                            models_count,
                            provider_keys_count,
                        })
                    }
                    RuntimeConnection::Sqlite(conn) => {
                        use crate::database::_sqlite_schema::*;
                        let providers_query = provider::table
                            .filter(provider::dsl::deleted_at.is_null())
                            .select(count_star());
                        let providers_count =
                            diesel_async::RunQueryDsl::first::<i64>(providers_query, &mut **conn)
                                .await?;
                        let models_query = model::table
                            .filter(model::dsl::deleted_at.is_null())
                            .select(count_star());
                        let models_count =
                            diesel_async::RunQueryDsl::first::<i64>(models_query, &mut **conn)
                                .await?;
                        let provider_keys_query = provider_api_key::table
                            .filter(provider_api_key::dsl::deleted_at.is_null())
                            .select(count_star());
                        let provider_keys_count = diesel_async::RunQueryDsl::first::<i64>(
                            provider_keys_query,
                            &mut **conn,
                        )
                        .await?;
                        Ok(SystemOverviewStats {
                            providers_count,
                            models_count,
                            provider_keys_count,
                        })
                    }
                }
            })
        })
        .await
}

pub async fn get_dashboard_overview_stats(
    database: &DatabaseRuntime,
) -> DbResult<DashboardOverviewStats> {
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                macro_rules! load_counts {
                    ($conn:expr) => {{
                        let provider_count = diesel_async::RunQueryDsl::first::<i64>(
                            provider::table
                                .filter(provider::dsl::deleted_at.is_null())
                                .select(count_star()),
                            &mut *$conn,
                        )
                        .await?;
                        let enabled_provider_count = diesel_async::RunQueryDsl::first::<i64>(
                            provider::table
                                .filter(
                                    provider::dsl::deleted_at
                                        .is_null()
                                        .and(provider::dsl::is_enabled.eq(true)),
                                )
                                .select(count_star()),
                            &mut *$conn,
                        )
                        .await?;
                        let model_count = diesel_async::RunQueryDsl::first::<i64>(
                            model::table
                                .filter(model::dsl::deleted_at.is_null())
                                .select(count_star()),
                            &mut *$conn,
                        )
                        .await?;
                        let enabled_model_count = diesel_async::RunQueryDsl::first::<i64>(
                            model::table
                                .filter(
                                    model::dsl::deleted_at
                                        .is_null()
                                        .and(model::dsl::is_enabled.eq(true)),
                                )
                                .select(count_star()),
                            &mut *$conn,
                        )
                        .await?;
                        let provider_key_count = diesel_async::RunQueryDsl::first::<i64>(
                            provider_api_key::table
                                .filter(provider_api_key::dsl::deleted_at.is_null())
                                .select(count_star()),
                            &mut *$conn,
                        )
                        .await?;
                        let enabled_provider_key_count = diesel_async::RunQueryDsl::first::<i64>(
                            provider_api_key::table
                                .filter(
                                    provider_api_key::dsl::deleted_at
                                        .is_null()
                                        .and(provider_api_key::dsl::is_enabled.eq(true)),
                                )
                                .select(count_star()),
                            &mut *$conn,
                        )
                        .await?;
                        let api_key_count = diesel_async::RunQueryDsl::first::<i64>(
                            api_key::table
                                .filter(api_key::dsl::deleted_at.is_null())
                                .select(count_star()),
                            &mut *$conn,
                        )
                        .await?;
                        let enabled_api_key_count = diesel_async::RunQueryDsl::first::<i64>(
                            api_key::table
                                .filter(
                                    api_key::dsl::deleted_at
                                        .is_null()
                                        .and(api_key::dsl::is_enabled.eq(true)),
                                )
                                .select(count_star()),
                            &mut *$conn,
                        )
                        .await?;
                        Ok(DashboardOverviewStats {
                            provider_count,
                            enabled_provider_count,
                            model_count,
                            enabled_model_count,
                            provider_key_count,
                            enabled_provider_key_count,
                            api_key_count,
                            enabled_api_key_count,
                        })
                    }};
                }
                match connection {
                    RuntimeConnection::Postgres(conn) => {
                        use crate::database::_postgres_schema::*;
                        load_counts!(&mut **conn)
                    }
                    RuntimeConnection::Sqlite(conn) => {
                        use crate::database::_sqlite_schema::*;
                        load_counts!(&mut **conn)
                    }
                }
            })
        })
        .await
}

pub async fn get_today_request_log_stats(
    database: &DatabaseRuntime,
    timezone: Option<&str>,
) -> DbResult<TodayRequestLogStats> {
    let start_of_today = start_of_today_timestamp_ms(timezone)?;
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                match connection {
                    RuntimeConnection::Postgres(conn) => {
                        use crate::database::_postgres_schema::*;
                        let summary_query = request_log::table
                            .filter(request_log::dsl::request_received_at.ge(start_of_today))
                            .select((
                                count_star(),
                                sum(request_log::dsl::total_input_tokens),
                                sum(request_log::dsl::total_output_tokens),
                                sum(request_log::dsl::reasoning_tokens),
                                sum(request_log::dsl::total_tokens),
                            ));
                        let row: (i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>) =
                            diesel_async::RunQueryDsl::first(summary_query, &mut **conn).await?;
                        let cost_query = sql_query(
                            "SELECT estimated_cost_currency AS currency,
                                    CAST(SUM(estimated_cost_nanos) AS BIGINT) AS total_cost_nanos
                             FROM request_log
                             WHERE request_received_at >= $1
                               AND estimated_cost_nanos IS NOT NULL
                               AND estimated_cost_currency IS NOT NULL
                             GROUP BY estimated_cost_currency",
                        )
                        .bind::<BigInt, _>(start_of_today);
                        let costs = diesel_async::RunQueryDsl::load::<CostByCurrencyRow>(
                            cost_query,
                            &mut **conn,
                        )
                        .await?;
                        Ok(TodayRequestLogStats {
                            requests_count: row.0,
                            total_input_tokens: row.1.unwrap_or(0),
                            total_output_tokens: row.2.unwrap_or(0),
                            total_reasoning_tokens: row.3.unwrap_or(0),
                            total_tokens: row.4.unwrap_or(0),
                            total_cost: costs
                                .into_iter()
                                .map(|row| (row.currency, row.total_cost_nanos))
                                .collect(),
                        })
                    }
                    RuntimeConnection::Sqlite(conn) => {
                        use crate::database::_sqlite_schema::*;
                        let summary_query = request_log::table
                            .filter(request_log::dsl::request_received_at.ge(start_of_today))
                            .select((
                                count_star(),
                                sum(request_log::dsl::total_input_tokens),
                                sum(request_log::dsl::total_output_tokens),
                                sum(request_log::dsl::reasoning_tokens),
                                sum(request_log::dsl::total_tokens),
                            ));
                        let row: (i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>) =
                            diesel_async::RunQueryDsl::first(summary_query, &mut **conn).await?;
                        let cost_query = sql_query(
                            "SELECT estimated_cost_currency AS currency,
                                    CAST(SUM(estimated_cost_nanos) AS BIGINT) AS total_cost_nanos
                             FROM request_log
                             WHERE request_received_at >= ?
                               AND estimated_cost_nanos IS NOT NULL
                               AND estimated_cost_currency IS NOT NULL
                             GROUP BY estimated_cost_currency",
                        )
                        .bind::<BigInt, _>(start_of_today);
                        let costs = diesel_async::RunQueryDsl::load::<CostByCurrencyRow>(
                            cost_query,
                            &mut **conn,
                        )
                        .await?;
                        Ok(TodayRequestLogStats {
                            requests_count: row.0,
                            total_input_tokens: row.1.unwrap_or(0),
                            total_output_tokens: row.2.unwrap_or(0),
                            total_reasoning_tokens: row.3.unwrap_or(0),
                            total_tokens: row.4.unwrap_or(0),
                            total_cost: costs
                                .into_iter()
                                .map(|row| (row.currency, row.total_cost_nanos))
                                .collect(),
                        })
                    }
                }
            })
        })
        .await
}

pub async fn get_dashboard_today_stats(
    database: &DatabaseRuntime,
    timezone: Option<&str>,
) -> DbResult<DashboardTodayStats> {
    let start_of_today = start_of_today_timestamp_ms(timezone)?;
    let aggregate = load_dashboard_today_aggregate(database, start_of_today).await?;
    let today = get_today_request_log_stats(database, timezone).await?;
    Ok(DashboardTodayStats {
        request_count: aggregate.request_count,
        success_count: aggregate.success_count,
        error_count: aggregate.error_count,
        success_rate: calculate_success_rate(aggregate.request_count, aggregate.success_count),
        total_input_tokens: aggregate.total_input_tokens,
        total_output_tokens: aggregate.total_output_tokens,
        total_reasoning_tokens: aggregate.total_reasoning_tokens,
        total_tokens: aggregate.total_tokens,
        total_cost: today.total_cost,
        avg_time_to_first_response_body_ms: aggregate.avg_time_to_first_response_body_ms,
        time_to_first_response_body_sample_count: aggregate
            .time_to_first_response_body_sample_count,
        avg_ttft_ms: aggregate.avg_ttft_ms,
        ttft_sample_count: aggregate.ttft_sample_count,
        avg_total_latency_ms: aggregate.avg_total_latency_ms,
        total_latency_sample_count: aggregate.total_latency_sample_count,
        active_provider_count: aggregate.active_provider_count,
        active_model_count: aggregate.active_model_count,
        active_api_key_count: aggregate.active_api_key_count,
    })
}

async fn load_dashboard_today_aggregate(
    database: &DatabaseRuntime,
    start_of_today: i64,
) -> DbResult<DashboardTodayAggregateRow> {
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                let result = match connection {
                    RuntimeConnection::Postgres(conn) => {
                        let query = sql_query(
                            "SELECT
                                CAST(COUNT(*) AS BIGINT) AS request_count,
                                CAST(COALESCE(SUM(CASE WHEN CAST(overall_status AS TEXT) = 'SUCCESS' THEN 1 ELSE 0 END), 0) AS BIGINT) AS success_count,
                                CAST(COALESCE(SUM(CASE WHEN CAST(overall_status AS TEXT) IN ('ERROR', 'CANCELLED') THEN 1 ELSE 0 END), 0) AS BIGINT) AS error_count,
                                CAST(COALESCE(SUM(total_input_tokens), 0) AS BIGINT) AS total_input_tokens,
                                CAST(COALESCE(SUM(total_output_tokens), 0) AS BIGINT) AS total_output_tokens,
                                CAST(COALESCE(SUM(reasoning_tokens), 0) AS BIGINT) AS total_reasoning_tokens,
                                CAST(COALESCE(SUM(total_tokens), 0) AS BIGINT) AS total_tokens,
                                CAST(AVG(CASE
                                    WHEN upstream_request_sent_at IS NOT NULL
                                    AND first_response_body_at IS NOT NULL
                                    AND first_response_body_at >= upstream_request_sent_at
                                    THEN (first_response_body_at - upstream_request_sent_at)::DOUBLE PRECISION
                                    ELSE NULL
                                END) AS DOUBLE PRECISION) AS avg_time_to_first_response_body_ms,
                                CAST(COUNT(CASE
                                    WHEN upstream_request_sent_at IS NOT NULL
                                    AND first_response_body_at IS NOT NULL
                                    AND first_response_body_at >= upstream_request_sent_at
                                    THEN 1 ELSE NULL
                                END) AS BIGINT) AS time_to_first_response_body_sample_count,
                                CAST(AVG(CASE
                                    WHEN is_stream = TRUE
                                    AND upstream_request_sent_at IS NOT NULL
                                    AND first_token_at IS NOT NULL
                                    AND first_token_at >= upstream_request_sent_at
                                    THEN (first_token_at - upstream_request_sent_at)::DOUBLE PRECISION
                                    ELSE NULL
                                END) AS DOUBLE PRECISION) AS avg_ttft_ms,
                                CAST(COUNT(CASE
                                    WHEN is_stream = TRUE
                                    AND upstream_request_sent_at IS NOT NULL
                                    AND first_token_at IS NOT NULL
                                    AND first_token_at >= upstream_request_sent_at
                                    THEN 1 ELSE NULL
                                END) AS BIGINT) AS ttft_sample_count,
                                CAST(AVG(CASE
                                    WHEN upstream_request_sent_at IS NOT NULL
                                     AND completed_at IS NOT NULL
                                     AND completed_at >= upstream_request_sent_at
                                    THEN (completed_at - upstream_request_sent_at)::DOUBLE PRECISION
                                    ELSE NULL
                                END) AS DOUBLE PRECISION) AS avg_total_latency_ms,
                                CAST(COUNT(CASE
                                    WHEN upstream_request_sent_at IS NOT NULL
                                    AND completed_at IS NOT NULL
                                    AND completed_at >= upstream_request_sent_at
                                    THEN 1 ELSE NULL
                                END) AS BIGINT) AS total_latency_sample_count,
                                CAST(COUNT(DISTINCT provider_id) AS BIGINT) AS active_provider_count,
                                CAST(COUNT(DISTINCT model_id) AS BIGINT) AS active_model_count,
                                CAST(COUNT(DISTINCT api_key_id) AS BIGINT) AS active_api_key_count
                             FROM request_log
                             WHERE request_received_at >= $1",
                        )
                        .bind::<BigInt, _>(start_of_today);
                        diesel_async::RunQueryDsl::get_result::<DashboardTodayAggregateRow>(
                            query,
                            &mut **conn,
                        )
                        .await
                    }
                    RuntimeConnection::Sqlite(conn) => {
                        let query = sql_query(
                            "SELECT
                                CAST(COUNT(*) AS BIGINT) AS request_count,
                                CAST(COALESCE(SUM(CASE WHEN CAST(overall_status AS TEXT) = 'SUCCESS' THEN 1 ELSE 0 END), 0) AS BIGINT) AS success_count,
                                CAST(COALESCE(SUM(CASE WHEN CAST(overall_status AS TEXT) IN ('ERROR', 'CANCELLED') THEN 1 ELSE 0 END), 0) AS BIGINT) AS error_count,
                                CAST(COALESCE(SUM(total_input_tokens), 0) AS BIGINT) AS total_input_tokens,
                                CAST(COALESCE(SUM(total_output_tokens), 0) AS BIGINT) AS total_output_tokens,
                                CAST(COALESCE(SUM(reasoning_tokens), 0) AS BIGINT) AS total_reasoning_tokens,
                                CAST(COALESCE(SUM(total_tokens), 0) AS BIGINT) AS total_tokens,
                                CAST(AVG(CASE
                                    WHEN upstream_request_sent_at IS NOT NULL
                                    AND first_response_body_at IS NOT NULL
                                    AND first_response_body_at >= upstream_request_sent_at
                                    THEN (first_response_body_at - upstream_request_sent_at)
                                    ELSE NULL
                                END) AS REAL) AS avg_time_to_first_response_body_ms,
                                CAST(COUNT(CASE
                                    WHEN upstream_request_sent_at IS NOT NULL
                                    AND first_response_body_at IS NOT NULL
                                    AND first_response_body_at >= upstream_request_sent_at
                                    THEN 1 ELSE NULL
                                END) AS BIGINT) AS time_to_first_response_body_sample_count,
                                CAST(AVG(CASE
                                    WHEN is_stream = 1
                                    AND upstream_request_sent_at IS NOT NULL
                                    AND first_token_at IS NOT NULL
                                    AND first_token_at >= upstream_request_sent_at
                                    THEN (first_token_at - upstream_request_sent_at)
                                    ELSE NULL
                                END) AS REAL) AS avg_ttft_ms,
                                CAST(COUNT(CASE
                                    WHEN is_stream = 1
                                    AND upstream_request_sent_at IS NOT NULL
                                    AND first_token_at IS NOT NULL
                                    AND first_token_at >= upstream_request_sent_at
                                    THEN 1 ELSE NULL
                                END) AS BIGINT) AS ttft_sample_count,
                                CAST(AVG(CASE
                                    WHEN upstream_request_sent_at IS NOT NULL
                                     AND completed_at IS NOT NULL
                                     AND completed_at >= upstream_request_sent_at
                                    THEN (completed_at - upstream_request_sent_at)
                                    ELSE NULL
                                END) AS REAL) AS avg_total_latency_ms,
                                CAST(COUNT(CASE
                                    WHEN upstream_request_sent_at IS NOT NULL
                                    AND completed_at IS NOT NULL
                                    AND completed_at >= upstream_request_sent_at
                                    THEN 1 ELSE NULL
                                END) AS BIGINT) AS total_latency_sample_count,
                                CAST(COUNT(DISTINCT provider_id) AS BIGINT) AS active_provider_count,
                                CAST(COUNT(DISTINCT model_id) AS BIGINT) AS active_model_count,
                                CAST(COUNT(DISTINCT api_key_id) AS BIGINT) AS active_api_key_count
                             FROM request_log
                             WHERE request_received_at >= ?",
                        )
                        .bind::<BigInt, _>(start_of_today);
                        diesel_async::RunQueryDsl::get_result::<DashboardTodayAggregateRow>(
                            query,
                            &mut **conn,
                        )
                        .await
                    }
                };
                result.map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to load dashboard today aggregate: {}",
                        error
                    )))
                })
            })
        })
        .await
}

pub async fn get_dashboard_top_models(
    database: &DatabaseRuntime,
    limit: usize,
    timezone: Option<&str>,
) -> DbResult<Vec<DashboardTopModelItem>> {
    let start_of_today = start_of_today_timestamp_ms(timezone)?;
    load_dashboard_top_models_runtime(database, start_of_today, limit, false).await
}

pub async fn get_dashboard_top_cost_models(
    database: &DatabaseRuntime,
    limit: usize,
    timezone: Option<&str>,
) -> DbResult<Vec<DashboardTopModelItem>> {
    let start_of_today = start_of_today_timestamp_ms(timezone)?;
    load_dashboard_top_models_runtime(database, start_of_today, limit, true).await
}

async fn load_dashboard_top_models_runtime(
    database: &DatabaseRuntime,
    start_of_today: i64,
    limit: usize,
    order_by_cost: bool,
) -> DbResult<Vec<DashboardTopModelItem>> {
    database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                let (base_rows, cost_rows) = match connection {
                    RuntimeConnection::Postgres(conn) => {
                        let order = if order_by_cost {
                            "COALESCE(SUM(rl.estimated_cost_nanos), 0) DESC, request_count DESC, rl.provider_id ASC, rl.model_id ASC"
                        } else {
                            "request_count DESC, rl.provider_id ASC, rl.model_id ASC"
                        };
                        let base_query = sql_query(format!(
                            "SELECT
                                rl.provider_id AS provider_id,
                                COALESCE(p.provider_key, rl.provider_key_snapshot) AS provider_key,
                                rl.model_id AS model_id,
                                COALESCE(m.model_name, rl.model_name_snapshot) AS model_name,
                                COALESCE(m.real_model_name, rl.real_model_name_snapshot) AS real_model_name,
                                CAST(COUNT(*) AS BIGINT) AS request_count,
                                CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens
                             FROM request_log rl
                             LEFT JOIN provider p ON p.id = rl.provider_id
                             LEFT JOIN model m ON m.id = rl.model_id
                             WHERE rl.request_received_at >= $1
                               AND rl.provider_id IS NOT NULL
                               AND rl.model_id IS NOT NULL
                             GROUP BY rl.provider_id, p.provider_key, rl.provider_key_snapshot,
                                      rl.model_id, m.model_name, rl.model_name_snapshot,
                                      m.real_model_name, rl.real_model_name_snapshot
                             ORDER BY {order}
                             LIMIT $2"
                        ))
                        .bind::<BigInt, _>(start_of_today)
                        .bind::<BigInt, _>(limit as i64);
                        let base_rows = diesel_async::RunQueryDsl::load::<DashboardTopModelBaseRow>(
                            base_query,
                            &mut **conn,
                        )
                        .await
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to load dashboard top model rows: {}",
                                error
                            )))
                        })?;
                        let cost_query = sql_query(
                            "SELECT
                                rl.provider_id AS provider_id,
                                rl.model_id AS model_id,
                                rl.estimated_cost_currency AS currency,
                                CAST(SUM(rl.estimated_cost_nanos) AS BIGINT) AS total_cost_nanos
                             FROM request_log rl
                             WHERE rl.request_received_at >= $1
                               AND rl.provider_id IS NOT NULL
                               AND rl.model_id IS NOT NULL
                               AND rl.estimated_cost_nanos IS NOT NULL
                               AND rl.estimated_cost_currency IS NOT NULL
                             GROUP BY rl.provider_id, rl.model_id, rl.estimated_cost_currency",
                        )
                        .bind::<BigInt, _>(start_of_today);
                        let cost_rows =
                            diesel_async::RunQueryDsl::load::<DashboardTopModelCostRow>(
                                cost_query,
                                &mut **conn,
                            )
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to load dashboard top model costs: {}",
                                    error
                                )))
                            })?;
                        (base_rows, cost_rows)
                    }
                    RuntimeConnection::Sqlite(conn) => {
                        let order = if order_by_cost {
                            "COALESCE(SUM(rl.estimated_cost_nanos), 0) DESC, request_count DESC, rl.provider_id ASC, rl.model_id ASC"
                        } else {
                            "request_count DESC, rl.provider_id ASC, rl.model_id ASC"
                        };
                        let base_query = sql_query(format!(
                            "SELECT
                                rl.provider_id AS provider_id,
                                COALESCE(p.provider_key, rl.provider_key_snapshot) AS provider_key,
                                rl.model_id AS model_id,
                                COALESCE(m.model_name, rl.model_name_snapshot) AS model_name,
                                COALESCE(m.real_model_name, rl.real_model_name_snapshot) AS real_model_name,
                                CAST(COUNT(*) AS BIGINT) AS request_count,
                                CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens
                             FROM request_log rl
                             LEFT JOIN provider p ON p.id = rl.provider_id
                             LEFT JOIN model m ON m.id = rl.model_id
                             WHERE rl.request_received_at >= ?
                               AND rl.provider_id IS NOT NULL
                               AND rl.model_id IS NOT NULL
                             GROUP BY rl.provider_id, p.provider_key, rl.provider_key_snapshot,
                                      rl.model_id, m.model_name, rl.model_name_snapshot,
                                      m.real_model_name, rl.real_model_name_snapshot
                             ORDER BY {order}
                             LIMIT ?"
                        ))
                        .bind::<BigInt, _>(start_of_today)
                        .bind::<BigInt, _>(limit as i64);
                        let base_rows = diesel_async::RunQueryDsl::load::<DashboardTopModelBaseRow>(
                            base_query,
                            &mut **conn,
                        )
                        .await
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to load dashboard top model rows: {}",
                                error
                            )))
                        })?;
                        let cost_query = sql_query(
                            "SELECT
                                rl.provider_id AS provider_id,
                                rl.model_id AS model_id,
                                rl.estimated_cost_currency AS currency,
                                CAST(SUM(rl.estimated_cost_nanos) AS BIGINT) AS total_cost_nanos
                             FROM request_log rl
                             WHERE rl.request_received_at >= ?
                               AND rl.provider_id IS NOT NULL
                               AND rl.model_id IS NOT NULL
                               AND rl.estimated_cost_nanos IS NOT NULL
                               AND rl.estimated_cost_currency IS NOT NULL
                             GROUP BY rl.provider_id, rl.model_id, rl.estimated_cost_currency",
                        )
                        .bind::<BigInt, _>(start_of_today);
                        let cost_rows =
                            diesel_async::RunQueryDsl::load::<DashboardTopModelCostRow>(
                                cost_query,
                                &mut **conn,
                            )
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to load dashboard top model costs: {}",
                                    error
                                )))
                            })?;
                        (base_rows, cost_rows)
                    }
                };
                let mut items = base_rows
                    .into_iter()
                    .map(|row| {
                        (
                            (row.provider_id, row.model_id),
                            DashboardTopModelItem {
                                provider_id: row.provider_id,
                                provider_key: row.provider_key.unwrap_or_default(),
                                model_id: row.model_id,
                                model_name: row.model_name.unwrap_or_default(),
                                real_model_name: row.real_model_name,
                                request_count: row.request_count,
                                total_tokens: row.total_tokens,
                                total_cost: HashMap::new(),
                            },
                        )
                    })
                    .collect::<HashMap<_, _>>();
                for row in cost_rows {
                    if let Some(item) = items.get_mut(&(row.provider_id, row.model_id)) {
                        item.total_cost.insert(row.currency, row.total_cost_nanos);
                    }
                }
                let mut result = items.into_values().collect::<Vec<_>>();
                if order_by_cost {
                    result.sort_by(|left, right| {
                        let left_cost = left.total_cost.values().copied().sum::<i64>();
                        let right_cost = right.total_cost.values().copied().sum::<i64>();
                        right_cost
                            .cmp(&left_cost)
                            .then_with(|| right.request_count.cmp(&left.request_count))
                            .then_with(|| left.provider_id.cmp(&right.provider_id))
                            .then_with(|| left.model_id.cmp(&right.model_id))
                    });
                } else {
                    result.sort_by(|left, right| {
                        right
                            .request_count
                            .cmp(&left.request_count)
                            .then_with(|| left.provider_id.cmp(&right.provider_id))
                            .then_with(|| left.model_id.cmp(&right.model_id))
                    });
                }
                result.truncate(limit);
                Ok(result)
            })
        })
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn get_usage_stats_aggregates(
    database: &DatabaseRuntime,
    start_time_ms: i64,
    end_time_ms: i64,
    interval: &str,
    group_by: UsageStatsGroupBy,
    provider_id_filter: Option<i64>,
    model_id_filter: Option<i64>,
    api_key_id_filter: Option<i64>,
    provider_api_key_id_filter: Option<i64>,
) -> DbResult<Vec<UsageStatsQueryItem>> {
    let interval = interval.to_string();
    let (group_select_sql, group_by_sql, group_id_sql) = usage_group_sql(group_by);
    let (base_rows, cost_rows) = database
        .run_db(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                match connection {
                    RuntimeConnection::Postgres(conn) => {
                        let bucket_sql = usage_bucket_sql_postgres(&interval);
                        let base_query = sql_query(format!(
                            "SELECT
                                {bucket_sql} AS time_bucket,
                                {group_select_sql},
                                CAST(COALESCE(SUM(rl.total_input_tokens), 0) AS BIGINT) AS total_input_tokens,
                                CAST(COALESCE(SUM(rl.total_output_tokens), 0) AS BIGINT) AS total_output_tokens,
                                CAST(COALESCE(SUM(rl.reasoning_tokens), 0) AS BIGINT) AS total_reasoning_tokens,
                                CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens,
                                CAST(COUNT(*) AS BIGINT) AS request_count,
                                CAST(SUM(CASE WHEN CAST(rl.overall_status AS TEXT) = 'SUCCESS' THEN 1 ELSE 0 END) AS BIGINT) AS success_count,
                                CAST(SUM(CASE WHEN CAST(rl.overall_status AS TEXT) IN ('ERROR', 'CANCELLED') THEN 1 ELSE 0 END) AS BIGINT) AS error_count,
                                CAST(SUM(CASE WHEN rl.first_response_body_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.first_response_body_at >= rl.upstream_request_sent_at
                                             THEN (rl.first_response_body_at - rl.upstream_request_sent_at)::DOUBLE PRECISION
                                             ELSE 0 END) AS DOUBLE PRECISION) AS time_to_first_response_body_sum_ms,
                                CAST(SUM(CASE WHEN rl.first_response_body_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.first_response_body_at >= rl.upstream_request_sent_at
                                             THEN 1 ELSE 0 END) AS BIGINT) AS time_to_first_response_body_sample_count,
                                CAST(SUM(CASE WHEN rl.is_stream = TRUE
                                                  AND rl.first_token_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.first_token_at >= rl.upstream_request_sent_at
                                             THEN (rl.first_token_at - rl.upstream_request_sent_at)::DOUBLE PRECISION
                                             ELSE 0 END) AS DOUBLE PRECISION) AS ttft_sum_ms,
                                CAST(SUM(CASE WHEN rl.is_stream = TRUE
                                                  AND rl.first_token_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.first_token_at >= rl.upstream_request_sent_at
                                             THEN 1 ELSE 0 END) AS BIGINT) AS ttft_sample_count,
                                CAST(SUM(CASE WHEN rl.completed_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.completed_at >= rl.upstream_request_sent_at
                                             THEN (rl.completed_at - rl.upstream_request_sent_at)::DOUBLE PRECISION
                                             ELSE 0 END) AS DOUBLE PRECISION) AS total_latency_sum_ms,
                                CAST(SUM(CASE WHEN rl.completed_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.completed_at >= rl.upstream_request_sent_at
                                             THEN 1 ELSE 0 END) AS BIGINT) AS total_latency_sample_count
                             FROM request_log rl
                             LEFT JOIN provider p ON p.id = rl.provider_id
                             LEFT JOIN model m ON m.id = rl.model_id
                             LEFT JOIN api_key ak ON ak.id = rl.api_key_id
                             WHERE rl.request_received_at >= $1
                               AND rl.request_received_at < $2
                               AND {group_id_sql} IS NOT NULL
                               AND ($3 IS NULL OR rl.provider_id = $3)
                               AND ($4 IS NULL OR rl.model_id = $4)
                               AND ($5 IS NULL OR rl.api_key_id = $5)
                               AND ($6 IS NULL OR rl.provider_api_key_id = $6)
                             GROUP BY 1, {group_by_sql}
                             ORDER BY 1 ASC"
                        ))
                        .bind::<BigInt, _>(start_time_ms)
                        .bind::<BigInt, _>(end_time_ms)
                        .bind::<Nullable<BigInt>, _>(provider_id_filter)
                        .bind::<Nullable<BigInt>, _>(model_id_filter)
                        .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                        .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter);
                        let base_rows = diesel_async::RunQueryDsl::load::<UsageStatsBaseRow>(
                            base_query,
                            &mut **conn,
                        )
                        .await
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to load usage stats base rows: {}",
                                error
                            )))
                        })?;
                        let cost_query = sql_query(format!(
                            "SELECT
                                {bucket_sql} AS time_bucket,
                                {group_id_sql} AS group_id,
                                rl.estimated_cost_currency AS currency,
                                CAST(COALESCE(SUM(rl.estimated_cost_nanos), 0) AS BIGINT) AS total_cost_nanos
                             FROM request_log rl
                             LEFT JOIN provider p ON p.id = rl.provider_id
                             LEFT JOIN model m ON m.id = rl.model_id
                             LEFT JOIN api_key ak ON ak.id = rl.api_key_id
                             WHERE rl.request_received_at >= $1
                               AND rl.request_received_at < $2
                               AND {group_id_sql} IS NOT NULL
                               AND ($3 IS NULL OR rl.provider_id = $3)
                               AND ($4 IS NULL OR rl.model_id = $4)
                               AND ($5 IS NULL OR rl.api_key_id = $5)
                               AND ($6 IS NULL OR rl.provider_api_key_id = $6)
                               AND rl.estimated_cost_nanos IS NOT NULL
                               AND rl.estimated_cost_currency IS NOT NULL
                             GROUP BY 1, {group_id_sql}, rl.estimated_cost_currency, {group_by_sql}
                             ORDER BY 1 ASC"
                        ))
                        .bind::<BigInt, _>(start_time_ms)
                        .bind::<BigInt, _>(end_time_ms)
                        .bind::<Nullable<BigInt>, _>(provider_id_filter)
                        .bind::<Nullable<BigInt>, _>(model_id_filter)
                        .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                        .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter);
                        let cost_rows = diesel_async::RunQueryDsl::load::<UsageStatsCostRow>(
                            cost_query,
                            &mut **conn,
                        )
                        .await
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to load usage stats cost rows: {}",
                                error
                            )))
                        })?;
                        Ok((base_rows, cost_rows))
                    }
                    RuntimeConnection::Sqlite(conn) => {
                        let bucket_sql = usage_bucket_sql_sqlite(&interval);
                        let base_query = sql_query(format!(
                            "SELECT
                                {bucket_sql} AS time_bucket,
                                {group_select_sql},
                                CAST(COALESCE(SUM(rl.total_input_tokens), 0) AS BIGINT) AS total_input_tokens,
                                CAST(COALESCE(SUM(rl.total_output_tokens), 0) AS BIGINT) AS total_output_tokens,
                                CAST(COALESCE(SUM(rl.reasoning_tokens), 0) AS BIGINT) AS total_reasoning_tokens,
                                CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens,
                                CAST(COUNT(*) AS BIGINT) AS request_count,
                                CAST(SUM(CASE WHEN CAST(rl.overall_status AS TEXT) = 'SUCCESS' THEN 1 ELSE 0 END) AS BIGINT) AS success_count,
                                CAST(SUM(CASE WHEN CAST(rl.overall_status AS TEXT) IN ('ERROR', 'CANCELLED') THEN 1 ELSE 0 END) AS BIGINT) AS error_count,
                                CAST(SUM(CASE WHEN rl.first_response_body_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.first_response_body_at >= rl.upstream_request_sent_at
                                             THEN rl.first_response_body_at - rl.upstream_request_sent_at
                                             ELSE 0 END) AS REAL) AS time_to_first_response_body_sum_ms,
                                CAST(SUM(CASE WHEN rl.first_response_body_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.first_response_body_at >= rl.upstream_request_sent_at
                                             THEN 1 ELSE 0 END) AS BIGINT) AS time_to_first_response_body_sample_count,
                                CAST(SUM(CASE WHEN rl.is_stream = 1
                                                  AND rl.first_token_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.first_token_at >= rl.upstream_request_sent_at
                                             THEN rl.first_token_at - rl.upstream_request_sent_at
                                             ELSE 0 END) AS REAL) AS ttft_sum_ms,
                                CAST(SUM(CASE WHEN rl.is_stream = 1
                                                  AND rl.first_token_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.first_token_at >= rl.upstream_request_sent_at
                                             THEN 1 ELSE 0 END) AS BIGINT) AS ttft_sample_count,
                                CAST(SUM(CASE WHEN rl.completed_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.completed_at >= rl.upstream_request_sent_at
                                             THEN rl.completed_at - rl.upstream_request_sent_at
                                             ELSE 0 END) AS REAL) AS total_latency_sum_ms,
                                CAST(SUM(CASE WHEN rl.completed_at IS NOT NULL
                                                  AND rl.upstream_request_sent_at IS NOT NULL
                                                  AND rl.completed_at >= rl.upstream_request_sent_at
                                             THEN 1 ELSE 0 END) AS BIGINT) AS total_latency_sample_count
                             FROM request_log rl
                             LEFT JOIN provider p ON p.id = rl.provider_id
                             LEFT JOIN model m ON m.id = rl.model_id
                             LEFT JOIN api_key ak ON ak.id = rl.api_key_id
                             WHERE rl.request_received_at >= ?
                               AND rl.request_received_at < ?
                               AND {group_id_sql} IS NOT NULL
                               AND (? IS NULL OR rl.provider_id = ?)
                               AND (? IS NULL OR rl.model_id = ?)
                               AND (? IS NULL OR rl.api_key_id = ?)
                               AND (? IS NULL OR rl.provider_api_key_id = ?)
                             GROUP BY 1, {group_by_sql}
                             ORDER BY 1 ASC"
                        ))
                        .bind::<BigInt, _>(start_time_ms)
                        .bind::<BigInt, _>(end_time_ms)
                        .bind::<Nullable<BigInt>, _>(provider_id_filter)
                        .bind::<Nullable<BigInt>, _>(provider_id_filter)
                        .bind::<Nullable<BigInt>, _>(model_id_filter)
                        .bind::<Nullable<BigInt>, _>(model_id_filter)
                        .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                        .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                        .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
                        .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter);
                        let base_rows = diesel_async::RunQueryDsl::load::<UsageStatsBaseRow>(
                            base_query,
                            &mut **conn,
                        )
                        .await
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to load usage stats base rows: {}",
                                error
                            )))
                        })?;
                        let cost_query = sql_query(format!(
                            "SELECT
                                {bucket_sql} AS time_bucket,
                                {group_id_sql} AS group_id,
                                rl.estimated_cost_currency AS currency,
                                CAST(COALESCE(SUM(rl.estimated_cost_nanos), 0) AS BIGINT) AS total_cost_nanos
                             FROM request_log rl
                             LEFT JOIN provider p ON p.id = rl.provider_id
                             LEFT JOIN model m ON m.id = rl.model_id
                             LEFT JOIN api_key ak ON ak.id = rl.api_key_id
                             WHERE rl.request_received_at >= ?
                               AND rl.request_received_at < ?
                               AND {group_id_sql} IS NOT NULL
                               AND (? IS NULL OR rl.provider_id = ?)
                               AND (? IS NULL OR rl.model_id = ?)
                               AND (? IS NULL OR rl.api_key_id = ?)
                               AND (? IS NULL OR rl.provider_api_key_id = ?)
                               AND rl.estimated_cost_nanos IS NOT NULL
                               AND rl.estimated_cost_currency IS NOT NULL
                             GROUP BY 1, {group_id_sql}, rl.estimated_cost_currency, {group_by_sql}
                             ORDER BY 1 ASC"
                        ))
                        .bind::<BigInt, _>(start_time_ms)
                        .bind::<BigInt, _>(end_time_ms)
                        .bind::<Nullable<BigInt>, _>(provider_id_filter)
                        .bind::<Nullable<BigInt>, _>(provider_id_filter)
                        .bind::<Nullable<BigInt>, _>(model_id_filter)
                        .bind::<Nullable<BigInt>, _>(model_id_filter)
                        .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                        .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                        .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
                        .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter);
                        let cost_rows = diesel_async::RunQueryDsl::load::<UsageStatsCostRow>(
                            cost_query,
                            &mut **conn,
                        )
                        .await
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to load usage stats cost rows: {}",
                                error
                            )))
                        })?;
                        Ok((base_rows, cost_rows))
                    }
                }
            })
        })
        .await?;

    let mut items = base_rows
        .into_iter()
        .map(|row| {
            let time_to_first_response_body_sample_count =
                row.time_to_first_response_body_sample_count;
            let avg_time_to_first_response_body_ms = if time_to_first_response_body_sample_count > 0
            {
                Some(
                    row.time_to_first_response_body_sum_ms.unwrap_or(0.0)
                        / time_to_first_response_body_sample_count as f64,
                )
            } else {
                None
            };
            let avg_ttft_ms = (row.ttft_sample_count > 0)
                .then(|| row.ttft_sum_ms.unwrap_or(0.0) / row.ttft_sample_count as f64);
            let avg_total_latency_ms = (row.total_latency_sample_count > 0).then(|| {
                row.total_latency_sum_ms.unwrap_or(0.0) / row.total_latency_sample_count as f64
            });
            (
                (row.time_bucket, row.group_id),
                UsageStatsQueryItem {
                    time: row.time_bucket,
                    group_id: row.group_id,
                    provider_id: row.provider_id,
                    model_id: row.model_id,
                    api_key_id: row.api_key_id,
                    provider_key: row.provider_key,
                    model_name: row.model_name,
                    real_model_name: row.real_model_name,
                    api_key_name: row.api_key_name,
                    group_label: row.group_label.unwrap_or_default(),
                    group_detail: row.group_detail,
                    total_input_tokens: row.total_input_tokens,
                    total_output_tokens: row.total_output_tokens,
                    total_reasoning_tokens: row.total_reasoning_tokens,
                    total_tokens: row.total_tokens,
                    request_count: row.request_count,
                    success_count: row.success_count,
                    error_count: row.error_count,
                    success_rate: calculate_success_rate(row.request_count, row.success_count),
                    avg_time_to_first_response_body_ms,
                    time_to_first_response_body_sample_count,
                    avg_ttft_ms,
                    ttft_sample_count: row.ttft_sample_count,
                    avg_total_latency_ms,
                    total_latency_sample_count: row.total_latency_sample_count,
                    total_cost: HashMap::new(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    for row in cost_rows {
        if let Some(item) = items.get_mut(&(row.time_bucket, row.group_id)) {
            item.total_cost.insert(row.currency, row.total_cost_nanos);
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

pub(crate) fn start_of_today_timestamp_ms(timezone: Option<&str>) -> DbResult<i64> {
    start_of_day_timestamp_ms_at(Utc::now(), timezone)
}

fn start_of_day_timestamp_ms_at(now: DateTime<Utc>, timezone: Option<&str>) -> DbResult<i64> {
    let tz = parse_stats_timezone(timezone)?;
    let today_in_tz = now.with_timezone(&tz).date_naive();
    let local_start = today_in_tz.and_hms_opt(0, 0, 0).ok_or_else(|| {
        BaseError::InternalServerError(Some("Invalid local day start".to_string()))
    })?;
    tz.from_local_datetime(&local_start)
        .earliest()
        .map(|value| value.timestamp_millis())
        .ok_or_else(|| {
            BaseError::InternalServerError(Some(format!(
                "Invalid local day boundary for timezone '{}'",
                timezone.unwrap_or("UTC")
            )))
        })
}

fn parse_stats_timezone(timezone: Option<&str>) -> DbResult<Tz> {
    let Some(timezone) = timezone.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Tz::Etc__UTC);
    };
    timezone.parse::<Tz>().map_err(|_| {
        BaseError::InternalServerError(Some(format!(
            "Invalid configured timezone '{}'; update config.yaml and restart before reading today stats",
            timezone
        )))
    })
}

fn calculate_success_rate(request_count: i64, success_count: i64) -> Option<f64> {
    if request_count > 0 {
        Some(success_count as f64 / request_count as f64)
    } else {
        None
    }
}

fn usage_group_sql(group_by: UsageStatsGroupBy) -> (&'static str, &'static str, &'static str) {
    match group_by {
        UsageStatsGroupBy::Provider => (
            "rl.provider_id AS group_id,
             rl.provider_id AS provider_id,
             CAST(NULL AS BIGINT) AS model_id,
             CAST(NULL AS BIGINT) AS api_key_id,
             COALESCE(p.provider_key, rl.provider_key_snapshot) AS provider_key,
             CAST(NULL AS TEXT) AS model_name,
             CAST(NULL AS TEXT) AS real_model_name,
             CAST(NULL AS TEXT) AS api_key_name,
             COALESCE(p.provider_key, rl.provider_key_snapshot, '') AS group_label,
             COALESCE(p.name, rl.provider_name_snapshot) AS group_detail",
            "rl.provider_id, p.provider_key, rl.provider_key_snapshot, p.name, rl.provider_name_snapshot",
            "rl.provider_id",
        ),
        UsageStatsGroupBy::Model => (
            "rl.model_id AS group_id,
             rl.provider_id AS provider_id,
             rl.model_id AS model_id,
             CAST(NULL AS BIGINT) AS api_key_id,
             COALESCE(p.provider_key, rl.provider_key_snapshot) AS provider_key,
             COALESCE(m.model_name, rl.model_name_snapshot) AS model_name,
             COALESCE(m.real_model_name, rl.real_model_name_snapshot) AS real_model_name,
             CAST(NULL AS TEXT) AS api_key_name,
             COALESCE(p.provider_key, rl.provider_key_snapshot, '') || '/' || COALESCE(m.model_name, rl.model_name_snapshot, '') AS group_label,
             COALESCE(m.real_model_name, rl.real_model_name_snapshot) AS group_detail",
            "rl.model_id, rl.provider_id, p.provider_key, rl.provider_key_snapshot, m.model_name, rl.model_name_snapshot, m.real_model_name, rl.real_model_name_snapshot",
            "rl.model_id",
        ),
        UsageStatsGroupBy::ApiKey => (
            "rl.api_key_id AS group_id,
             CAST(NULL AS BIGINT) AS provider_id,
             CAST(NULL AS BIGINT) AS model_id,
             rl.api_key_id AS api_key_id,
             CAST(NULL AS TEXT) AS provider_key,
             CAST(NULL AS TEXT) AS model_name,
             CAST(NULL AS TEXT) AS real_model_name,
             ak.name AS api_key_name,
             COALESCE(ak.name, '') AS group_label,
             CASE
                 WHEN ak.id IS NULL THEN NULL
                 ELSE ak.key_prefix || '***' || ak.key_last4
             END AS group_detail",
            "rl.api_key_id, ak.id, ak.name, ak.key_prefix, ak.key_last4",
            "rl.api_key_id",
        ),
    }
}

fn usage_bucket_sql_postgres(interval: &str) -> &'static str {
    match interval {
        "minute" => {
            "CAST(FLOOR(EXTRACT(EPOCH FROM DATE_TRUNC('minute', TO_TIMESTAMP(rl.request_received_at / 1000.0)))) AS BIGINT) * 1000"
        }
        "hour" => {
            "CAST(FLOOR(EXTRACT(EPOCH FROM DATE_TRUNC('hour', TO_TIMESTAMP(rl.request_received_at / 1000.0)))) AS BIGINT) * 1000"
        }
        "day" => {
            "CAST(FLOOR(EXTRACT(EPOCH FROM DATE_TRUNC('day', TO_TIMESTAMP(rl.request_received_at / 1000.0)))) AS BIGINT) * 1000"
        }
        "month" => {
            "CAST(FLOOR(EXTRACT(EPOCH FROM DATE_TRUNC('month', TO_TIMESTAMP(rl.request_received_at / 1000.0)))) AS BIGINT) * 1000"
        }
        _ => {
            "CAST(FLOOR(EXTRACT(EPOCH FROM DATE_TRUNC('day', TO_TIMESTAMP(rl.request_received_at / 1000.0)))) AS BIGINT) * 1000"
        }
    }
}

fn usage_bucket_sql_sqlite(interval: &str) -> &'static str {
    match interval {
        "minute" => {
            "CAST(strftime('%s', strftime('%Y-%m-%d %H:%M:00', rl.request_received_at / 1000, 'unixepoch')) AS BIGINT) * 1000"
        }
        "hour" => {
            "CAST(strftime('%s', strftime('%Y-%m-%d %H:00:00', rl.request_received_at / 1000, 'unixepoch')) AS BIGINT) * 1000"
        }
        "day" => {
            "CAST(strftime('%s', datetime(rl.request_received_at / 1000, 'unixepoch', 'start of day')) AS BIGINT) * 1000"
        }
        "month" => {
            "CAST(strftime('%s', datetime(rl.request_received_at / 1000, 'unixepoch', 'start of month')) AS BIGINT) * 1000"
        }
        _ => {
            "CAST(strftime('%s', datetime(rl.request_received_at / 1000, 'unixepoch', 'start of day')) AS BIGINT) * 1000"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CostByCurrencyRow, UsageStatsGroupBy, calculate_success_rate, get_dashboard_overview_stats,
        get_dashboard_today_stats, get_dashboard_top_cost_models, get_dashboard_top_models,
        get_system_overview_stats, get_usage_stats_aggregates, start_of_day_timestamp_ms_at,
    };
    use crate::database::TestDatabase;
    use chrono::TimeZone;

    #[test]
    fn calculate_success_rate_handles_empty_and_non_empty_windows() {
        assert_eq!(calculate_success_rate(0, 0), None);
        assert_eq!(calculate_success_rate(10, 7), Some(0.7));
    }

    #[test]
    fn start_of_today_respects_supplied_timezone() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();

        let utc_start =
            start_of_day_timestamp_ms_at(now, None).expect("utc boundary should calculate");
        let shanghai_start = start_of_day_timestamp_ms_at(now, Some("Asia/Shanghai"))
            .expect("shanghai boundary should calculate");

        assert_ne!(utc_start, shanghai_start);
        assert_eq!(utc_start - shanghai_start, 8 * 60 * 60 * 1000);
        assert!(start_of_day_timestamp_ms_at(now, Some("Not/AZone")).is_err());
    }

    #[test]
    fn single_currency_cost_rows_collect_to_currency_map() {
        let rows = vec![CostByCurrencyRow {
            currency: "USD".to_string(),
            total_cost_nanos: 42,
        }];

        let map = rows
            .into_iter()
            .map(|row| (row.currency, row.total_cost_nanos))
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(map.len(), 1);
        assert_eq!(map.get("USD"), Some(&42));
    }

    #[test]
    fn cost_rows_collect_to_currency_map() {
        let rows = vec![
            CostByCurrencyRow {
                currency: "USD".to_string(),
                total_cost_nanos: 42,
            },
            CostByCurrencyRow {
                currency: "CNY".to_string(),
                total_cost_nanos: 99,
            },
        ];

        let map = rows
            .into_iter()
            .map(|row| (row.currency, row.total_cost_nanos))
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(map.get("USD"), Some(&42));
        assert_eq!(map.get("CNY"), Some(&99));
    }

    #[test]
    fn dashboard_stat_queries_keep_database_side_aggregation_guards() {
        let source = include_str!("stat.rs");
        let legacy_api_key_type = ["Full", "System", "Api", "Key"].join("");
        let legacy_api_key_name = ["system", "_api_key"].concat();
        let legacy_api_key_join = [
            "LEFT JOIN",
            legacy_api_key_name.as_str(),
            "sak ON sak.id = rl.api_key_id",
        ]
        .join(" ");

        assert!(
            source.contains("GROUP BY estimated_cost_currency"),
            "today cost aggregation should stay grouped in SQL",
        );
        assert!(
            !source.contains(&legacy_api_key_type) && !source.contains(&legacy_api_key_join),
            "dashboard and usage stats should not depend on legacy {legacy_api_key_name} queries",
        );
        assert!(
            source.contains("ak.key_prefix || '***' || ak.key_last4"),
            "api key grouping should expose masked key detail instead of the raw secret",
        );
        assert!(
            source.contains("CAST(COUNT(*) AS BIGINT) AS request_count")
                && source.contains(
                    "CAST(COUNT(DISTINCT provider_id) AS BIGINT) AS active_provider_count"
                )
                && source.contains(
                    "CAST(COALESCE(SUM(total_input_tokens), 0) AS BIGINT) AS total_input_tokens"
                )
                && source
                    .contains("CAST(COALESCE(SUM(total_tokens), 0) AS BIGINT) AS total_tokens"),
            "today summary should remain a database-side aggregate query",
        );
    }

    const STAT_SEED_SQL: &str = "INSERT INTO api_key (
                id, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm, max_concurrent_requests,
                quota_daily_requests, quota_daily_tokens, quota_monthly_tokens,
                budget_daily_nanos, budget_daily_currency, budget_monthly_nanos,
                budget_monthly_currency, deleted_at, created_at, updated_at
            ) VALUES (
                1, 'hash', 'ck-test', 'test', 'Ops key', NULL,
                'ALLOW', 1, NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, 1, 1
            );

            INSERT INTO provider (
                id, provider_key, name, is_enabled, deleted_at,
                created_at, updated_at, provider_api_key_mode
            ) VALUES (
                10, 'openai-main', 'OpenAI Main', 1, NULL,
                1, 1, 'QUEUE'
            );

            INSERT INTO upstream_source (
                id, provider_id, profile_type, base_url, use_proxy,
                chat_completions_enabled, embeddings_enabled, rerank_enabled,
                is_enabled, is_default, deleted_at, created_at, updated_at
            ) VALUES (
                15, 10, 'OPENAI', 'https://api.example.com/v1', 0,
                1, 1, 0,
                1, 1, NULL, 1, 1
            );

            INSERT INTO provider_api_key (
                id, provider_id, description, key_prefix, key_last4,
                secret_ciphertext, secret_nonce, secret_format_version,
                secret_key_fingerprint, secret_hmac,
                deleted_at, is_enabled, created_at, updated_at
            ) VALUES (
                20, 10, NULL, 'sk-prov', 'ider',
                X'00', X'000000000000000000000000000000000000000000000000', 1,
                '0000000000000000000000000000000000000000000000000000000000000000',
                '1111111111111111111111111111111111111111111111111111111111111111',
                NULL, 1, 1, 1
            );

            INSERT INTO model (
                id, provider_id, cost_catalog_id, model_name, real_model_name,
                model_kind, source_selection_mode,
                is_enabled, deleted_at, created_at, updated_at
            ) VALUES (
                30, 10, NULL, 'gpt-test', 'gpt-test-real',
                'CHAT', 'INHERIT_ALL', 1, NULL, 1, 1
            );

            INSERT INTO request_log (
                id, request_id, client_request_id, api_key_id, requested_model_name,
                downstream_protocol, overall_status, final_error_code, final_error_message,
                request_received_at,
                upstream_request_sent_at, first_response_body_at, completed_at,
                provider_id, provider_api_key_id, model_id, source_id,
                provider_key_snapshot, provider_name_snapshot,
                model_name_snapshot, real_model_name_snapshot,
                model_kind_snapshot,
                source_profile_type_snapshot, source_base_url_snapshot,
                upstream_protocol,
                estimated_cost_nanos, estimated_cost_currency,
                total_input_tokens, total_output_tokens, reasoning_tokens, total_tokens,
                created_at, updated_at
            ) VALUES
            (
                100, '018fa7d8-6a00-4c9a-8f7e-100000000000', 'stat-client', 1, 'gpt-test',
                'OPENAI', 'SUCCESS', NULL, NULL,
                1000,
                1100, 1200, 1500,
                10, 20, 30, 15,
                'openai-main', 'OpenAI Main',
                'gpt-test', 'gpt-test-real',
                'CHAT', 'OPENAI', 'https://api.example.com/v1', 'OPENAI',
                500, 'USD',
                10, 20, 5, 35,
                1000, 1500
            ),
            (
                101, '018fa7d8-6a00-4c9a-8f7e-101000000000', 'stat-client', 1, 'gpt-test',
                'OPENAI', 'ERROR', 'upstream_service_error', 'failed',
                2000,
                2250, 2300, 2550,
                10, 20, 30, 15,
                'openai-main', 'OpenAI Main',
                'gpt-test', 'gpt-test-real',
                'CHAT', 'OPENAI', 'https://api.example.com/v1', 'OPENAI',
                300, 'USD',
                7, 13, 2, 22,
                2000, 2550
            ),
            (
                102, '018fa7d8-6a00-4c9a-8f7e-102000000000', NULL, 1, 'gpt-empty',
                'OPENAI', 'SUCCESS', NULL, NULL,
                3000,
                NULL, NULL, 3100,
                NULL, NULL, NULL, NULL,
                NULL, NULL,
                NULL, NULL,
                NULL, NULL, NULL, NULL,
                NULL, NULL,
                1, 2, 0, 3,
                3000, 3100
            );";

    #[tokio::test]
    async fn sqlite_runtime_dashboard_and_usage_queries_preserve_aggregate_results() {
        let context = TestDatabase::new_sqlite_default("stat-runtime-parity.sqlite").await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        context
            .execute_sqlite_batch(STAT_SEED_SQL)
            .await
            .expect("stat seed rows should insert");
        context
            .execute_sqlite_batch(format!(
                "UPDATE request_log SET
                    upstream_request_sent_at = upstream_request_sent_at +
                        (({} - ((103 - id) * 1000)) - request_received_at),
                    first_response_body_at = first_response_body_at +
                        (({} - ((103 - id) * 1000)) - request_received_at),
                    completed_at = completed_at +
                        (({} - ((103 - id) * 1000)) - request_received_at),
                    created_at = created_at +
                        (({} - ((103 - id) * 1000)) - request_received_at),
                    updated_at = updated_at +
                        (({} - ((103 - id) * 1000)) - request_received_at),
                    request_received_at = {} - ((103 - id) * 1000)",
                now_ms, now_ms, now_ms, now_ms, now_ms, now_ms
            ))
            .await
            .expect("request logs should move into today's window");
        let database = context.runtime();

        let system = get_system_overview_stats(&database)
            .await
            .expect("system overview should load");
        assert_eq!(system.providers_count, 1);
        assert_eq!(system.models_count, 1);
        assert_eq!(system.provider_keys_count, 1);

        let overview = get_dashboard_overview_stats(&database)
            .await
            .expect("dashboard overview should load");
        assert_eq!(overview.provider_count, 1);
        assert_eq!(overview.enabled_provider_count, 1);
        assert_eq!(overview.model_count, 1);
        assert_eq!(overview.enabled_model_count, 1);
        assert_eq!(overview.provider_key_count, 1);
        assert_eq!(overview.enabled_provider_key_count, 1);
        assert_eq!(overview.api_key_count, 1);
        assert_eq!(overview.enabled_api_key_count, 1);

        let today = get_dashboard_today_stats(&database, None)
            .await
            .expect("dashboard today stats should load");
        assert_eq!(today.request_count, 3);
        assert_eq!(today.success_count, 2);
        assert_eq!(today.error_count, 1);
        assert_eq!(today.total_tokens, 60);
        assert_eq!(today.total_cost.get("USD"), Some(&800));

        let top_models = get_dashboard_top_models(&database, 10, None)
            .await
            .expect("dashboard top models should load");
        assert_eq!(top_models.len(), 1);
        assert_eq!(top_models[0].provider_id, 10);
        assert_eq!(top_models[0].model_id, 30);
        assert_eq!(top_models[0].request_count, 2);
        assert_eq!(top_models[0].total_tokens, 57);

        let top_cost_models = get_dashboard_top_cost_models(&database, 10, None)
            .await
            .expect("dashboard top cost models should load");
        assert_eq!(top_cost_models.len(), 1);
        assert_eq!(top_cost_models[0].total_cost.get("USD"), Some(&800));

        let usage = get_usage_stats_aggregates(
            &database,
            now_ms - 60_000,
            now_ms + 1_000,
            "day",
            UsageStatsGroupBy::Provider,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("usage stats should load");
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].provider_id, Some(10));
        assert_eq!(usage[0].request_count, 2);
        assert_eq!(usage[0].success_count, 1);
        assert_eq!(usage[0].error_count, 1);
        assert_eq!(usage[0].total_tokens, 57);
        assert_eq!(usage[0].total_cost.get("USD"), Some(&800));
    }
}
