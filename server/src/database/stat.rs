use crate::config::CONFIG;
use crate::database::model::Model;
use crate::database::provider::{Provider, ProviderApiKey};
// The legacy table still backs administrator-issued API keys. Keep the storage
// name isolated here and expose canonical api_key fields at return boundaries.
use crate::database::system_api_key::SystemApiKey as FullSystemApiKey;
use crate::database::{DbConnection, DbResult, get_connection};
use crate::{db_execute, db_object};
use chrono::{TimeZone, Utc};
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
    #[diesel(table_name = system_api_key)]
    pub struct SystemApiKey {
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
    pub avg_first_byte_ms: Option<f64>,
    pub avg_total_latency_ms: Option<f64>,
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

#[derive(Debug)]
struct TodayRequestLogSummaryRow {
    requests_count: i64,
    total_input_tokens: Option<i64>,
    total_output_tokens: Option<i64>,
    total_reasoning_tokens: Option<i64>,
    total_tokens: Option<i64>,
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
    avg_first_byte_ms: Option<f64>,
    #[diesel(sql_type = Nullable<Double>)]
    avg_total_latency_ms: Option<f64>,
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
    pub avg_total_latency_ms: Option<f64>,
    pub latency_sample_count: i64,
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
    latency_sum_ms: Option<f64>,
    #[diesel(sql_type = BigInt)]
    latency_sample_count: i64,
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

pub fn get_system_overview_stats() -> DbResult<SystemOverviewStats> {
    let conn = &mut get_connection()?;
    let mut stats = SystemOverviewStats::default();

    stats.providers_count = db_execute!(conn, {
        provider::table
            .filter(provider::dsl::deleted_at.is_null())
            .select(count_star())
            .first(conn)
    })?;

    stats.models_count = db_execute!(conn, {
        model::table
            .filter(model::dsl::deleted_at.is_null())
            .select(count_star())
            .first(conn)
    })?;

    stats.provider_keys_count = db_execute!(conn, {
        provider_api_key::table
            .filter(provider_api_key::dsl::deleted_at.is_null())
            .select(count_star())
            .first(conn)
    })?;

    Ok(stats)
}

pub fn get_request_logs_in_range(
    start_time_ms: i64,
    end_time_ms: i64,
    provider_id_filter: Option<i64>,
    model_id_filter: Option<i64>,
    api_key_id_filter: Option<i64>,
    provider_api_key_id_filter: Option<i64>,
) -> DbResult<Vec<RequestLogEntryForStats>> {
    match &mut get_connection()? {
        DbConnection::Postgres(conn) => sql_query(
            "SELECT
                rl.created_at,
                rl.final_provider_id AS provider_id,
                rl.final_model_id AS model_id,
                rl.total_input_tokens,
                rl.total_output_tokens,
                rl.reasoning_tokens,
                rl.total_tokens,
                rl.estimated_cost_nanos,
                rl.estimated_cost_currency,
                COALESCE(p.provider_key, rl.final_provider_key_snapshot) AS provider_key,
                COALESCE(m.model_name, rl.final_model_name_snapshot) AS model_name,
                COALESCE(m.real_model_name, rl.final_real_model_name_snapshot) AS real_model_name
             FROM request_log rl
             LEFT JOIN provider p ON p.id = rl.final_provider_id
             LEFT JOIN model m ON m.id = rl.final_model_id
             WHERE rl.created_at >= $1
               AND rl.created_at < $2
               AND rl.final_provider_id IS NOT NULL
               AND rl.final_model_id IS NOT NULL
               AND ($3 IS NULL OR rl.final_provider_id = $3)
               AND ($4 IS NULL OR rl.final_model_id = $4)
               AND ($5 IS NULL OR rl.api_key_id = $5)
               AND ($6 IS NULL OR rl.final_provider_api_key_id = $6)
             ORDER BY rl.created_at ASC",
        )
        .bind::<BigInt, _>(start_time_ms)
        .bind::<BigInt, _>(end_time_ms)
        .bind::<Nullable<BigInt>, _>(provider_id_filter)
        .bind::<Nullable<BigInt>, _>(model_id_filter)
        .bind::<Nullable<BigInt>, _>(api_key_id_filter)
        .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
        .load::<RequestLogEntryForStats>(conn)
        .map_err(Into::into),
        DbConnection::Sqlite(conn) => sql_query(
            "SELECT
                rl.created_at,
                rl.final_provider_id AS provider_id,
                rl.final_model_id AS model_id,
                rl.total_input_tokens,
                rl.total_output_tokens,
                rl.reasoning_tokens,
                rl.total_tokens,
                rl.estimated_cost_nanos,
                rl.estimated_cost_currency,
                COALESCE(p.provider_key, rl.final_provider_key_snapshot) AS provider_key,
                COALESCE(m.model_name, rl.final_model_name_snapshot) AS model_name,
                COALESCE(m.real_model_name, rl.final_real_model_name_snapshot) AS real_model_name
             FROM request_log rl
             LEFT JOIN provider p ON p.id = rl.final_provider_id
             LEFT JOIN model m ON m.id = rl.final_model_id
             WHERE rl.created_at >= ?
               AND rl.created_at < ?
               AND rl.final_provider_id IS NOT NULL
               AND rl.final_model_id IS NOT NULL
               AND (? IS NULL OR rl.final_provider_id = ?)
               AND (? IS NULL OR rl.final_model_id = ?)
               AND (? IS NULL OR rl.api_key_id = ?)
               AND (? IS NULL OR rl.final_provider_api_key_id = ?)
             ORDER BY rl.created_at ASC",
        )
        .bind::<BigInt, _>(start_time_ms)
        .bind::<BigInt, _>(end_time_ms)
        .bind::<Nullable<BigInt>, _>(provider_id_filter)
        .bind::<Nullable<BigInt>, _>(provider_id_filter)
        .bind::<Nullable<BigInt>, _>(model_id_filter)
        .bind::<Nullable<BigInt>, _>(model_id_filter)
        .bind::<Nullable<BigInt>, _>(api_key_id_filter)
        .bind::<Nullable<BigInt>, _>(api_key_id_filter)
        .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
        .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
        .load::<RequestLogEntryForStats>(conn)
        .map_err(Into::into),
    }
}

pub fn get_today_request_log_stats() -> DbResult<TodayRequestLogStats> {
    let conn = &mut get_connection()?;
    let start_of_today = start_of_today_timestamp_ms();
    let summary_row = load_today_request_log_summary(conn, start_of_today)?;

    Ok(TodayRequestLogStats {
        requests_count: summary_row.requests_count,
        total_input_tokens: summary_row.total_input_tokens.unwrap_or(0),
        total_output_tokens: summary_row.total_output_tokens.unwrap_or(0),
        total_reasoning_tokens: summary_row.total_reasoning_tokens.unwrap_or(0),
        total_tokens: summary_row.total_tokens.unwrap_or(0),
        total_cost: load_today_cost_by_currency(conn, start_of_today)?,
    })
}

pub fn get_dashboard_overview_stats() -> DbResult<DashboardOverviewStats> {
    let providers = Provider::list_all()?;
    let models = Model::list_all()?;
    let provider_keys = ProviderApiKey::list_all()?;
    // Legacy table read stays isolated here; returned counts are renamed to
    // canonical `api_key_*` fields below.
    let legacy_api_keys = FullSystemApiKey::list_all()?;

    Ok(DashboardOverviewStats {
        provider_count: providers.len() as i64,
        enabled_provider_count: providers.iter().filter(|item| item.is_enabled).count() as i64,
        model_count: models.len() as i64,
        enabled_model_count: models.iter().filter(|item| item.is_enabled).count() as i64,
        provider_key_count: provider_keys.len() as i64,
        enabled_provider_key_count: provider_keys.iter().filter(|item| item.is_enabled).count()
            as i64,
        api_key_count: legacy_api_keys.len() as i64,
        enabled_api_key_count: legacy_api_keys
            .iter()
            .filter(|item| item.is_enabled)
            .count() as i64,
    })
}

pub fn get_dashboard_today_stats() -> DbResult<DashboardTodayStats> {
    let conn = &mut get_connection()?;
    let start_of_today = start_of_today_timestamp_ms();
    let aggregate = load_dashboard_today_aggregate(conn, start_of_today)?;

    Ok(DashboardTodayStats {
        request_count: aggregate.request_count,
        success_count: aggregate.success_count,
        error_count: aggregate.error_count,
        success_rate: calculate_success_rate(aggregate.request_count, aggregate.success_count),
        total_input_tokens: aggregate.total_input_tokens,
        total_output_tokens: aggregate.total_output_tokens,
        total_reasoning_tokens: aggregate.total_reasoning_tokens,
        total_tokens: aggregate.total_tokens,
        total_cost: load_today_cost_by_currency(conn, start_of_today)?,
        avg_first_byte_ms: aggregate.avg_first_byte_ms,
        avg_total_latency_ms: aggregate.avg_total_latency_ms,
        active_provider_count: aggregate.active_provider_count,
        active_model_count: aggregate.active_model_count,
        active_api_key_count: aggregate.active_api_key_count,
    })
}

pub fn get_dashboard_top_models(limit: usize) -> DbResult<Vec<DashboardTopModelItem>> {
    let conn = &mut get_connection()?;
    let start_of_today = start_of_today_timestamp_ms();
    let mut items = load_dashboard_top_model_base_rows(conn, start_of_today, limit)?
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

    if items.is_empty() {
        return Ok(Vec::new());
    }

    for row in load_dashboard_top_model_cost_rows(conn, start_of_today)? {
        if let Some(item) = items.get_mut(&(row.provider_id, row.model_id)) {
            item.total_cost.insert(row.currency, row.total_cost_nanos);
        }
    }

    let mut result = items.into_values().collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right
            .request_count
            .cmp(&left.request_count)
            .then_with(|| left.provider_id.cmp(&right.provider_id))
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    result.truncate(limit);
    Ok(result)
}

pub fn get_dashboard_cost_alert_models(limit: usize) -> DbResult<Vec<DashboardTopModelItem>> {
    let conn = &mut get_connection()?;
    let start_of_today = start_of_today_timestamp_ms();
    let mut items = load_dashboard_top_model_base_rows_for_cost(conn, start_of_today, limit)?
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

    if items.is_empty() {
        return Ok(Vec::new());
    }

    for row in load_dashboard_top_model_cost_rows(conn, start_of_today)? {
        if let Some(item) = items.get_mut(&(row.provider_id, row.model_id)) {
            item.total_cost.insert(row.currency, row.total_cost_nanos);
        }
    }

    let mut result = items.into_values().collect::<Vec<_>>();
    result.sort_by(|left, right| {
        let left_cost = left.total_cost.values().copied().sum::<i64>();
        let right_cost = right.total_cost.values().copied().sum::<i64>();
        right_cost
            .cmp(&left_cost)
            .then_with(|| right.request_count.cmp(&left.request_count))
            .then_with(|| left.provider_id.cmp(&right.provider_id))
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    result.truncate(limit);
    Ok(result)
}

pub fn get_usage_stats_aggregates(
    start_time_ms: i64,
    end_time_ms: i64,
    interval: &str,
    group_by: UsageStatsGroupBy,
    provider_id_filter: Option<i64>,
    model_id_filter: Option<i64>,
    api_key_id_filter: Option<i64>,
    provider_api_key_id_filter: Option<i64>,
) -> DbResult<Vec<UsageStatsQueryItem>> {
    let conn = &mut get_connection()?;
    let base_rows = load_usage_stats_base_rows(
        conn,
        start_time_ms,
        end_time_ms,
        interval,
        group_by,
        provider_id_filter,
        model_id_filter,
        api_key_id_filter,
        provider_api_key_id_filter,
    )?;

    let mut items = base_rows
        .into_iter()
        .map(|row| {
            let latency_sample_count = row.latency_sample_count;
            let avg_total_latency_ms = if latency_sample_count > 0 {
                Some(row.latency_sum_ms.unwrap_or(0.0) / latency_sample_count as f64)
            } else {
                None
            };

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
                    avg_total_latency_ms,
                    latency_sample_count,
                    total_cost: HashMap::new(),
                },
            )
        })
        .collect::<HashMap<_, _>>();

    if items.is_empty() {
        return Ok(Vec::new());
    }

    for row in load_usage_stats_cost_rows(
        conn,
        start_time_ms,
        end_time_ms,
        interval,
        group_by,
        provider_id_filter,
        model_id_filter,
        api_key_id_filter,
        provider_api_key_id_filter,
    )? {
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

fn start_of_today_timestamp_ms() -> i64 {
    let tz: Tz = CONFIG
        .timezone
        .as_deref()
        .and_then(|tz_str| tz_str.parse::<Tz>().ok())
        .unwrap_or(Tz::Etc__UTC);

    let now = Utc::now();
    let today_in_tz = now.with_timezone(&tz).date_naive();
    tz.from_local_datetime(&today_in_tz.and_hms_opt(0, 0, 0).unwrap())
        .unwrap()
        .timestamp_millis()
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
            "rl.final_provider_id AS group_id,
             rl.final_provider_id AS provider_id,
             CAST(NULL AS BIGINT) AS model_id,
             CAST(NULL AS BIGINT) AS api_key_id,
             COALESCE(p.provider_key, rl.final_provider_key_snapshot) AS provider_key,
             CAST(NULL AS TEXT) AS model_name,
             CAST(NULL AS TEXT) AS real_model_name,
             CAST(NULL AS TEXT) AS api_key_name,
             COALESCE(p.provider_key, rl.final_provider_key_snapshot, '') AS group_label,
             COALESCE(p.name, rl.final_provider_name_snapshot) AS group_detail",
            "rl.final_provider_id, p.provider_key, rl.final_provider_key_snapshot, p.name, rl.final_provider_name_snapshot",
            "rl.final_provider_id",
        ),
        UsageStatsGroupBy::Model => (
            "rl.final_model_id AS group_id,
             rl.final_provider_id AS provider_id,
             rl.final_model_id AS model_id,
             CAST(NULL AS BIGINT) AS api_key_id,
             COALESCE(p.provider_key, rl.final_provider_key_snapshot) AS provider_key,
             COALESCE(m.model_name, rl.final_model_name_snapshot) AS model_name,
             COALESCE(m.real_model_name, rl.final_real_model_name_snapshot) AS real_model_name,
             CAST(NULL AS TEXT) AS api_key_name,
             COALESCE(p.provider_key, rl.final_provider_key_snapshot, '') || '/' || COALESCE(m.model_name, rl.final_model_name_snapshot, '') AS group_label,
             COALESCE(m.real_model_name, rl.final_real_model_name_snapshot) AS group_detail",
            "rl.final_model_id, rl.final_provider_id, p.provider_key, rl.final_provider_key_snapshot, m.model_name, rl.final_model_name_snapshot, m.real_model_name, rl.final_real_model_name_snapshot",
            "rl.final_model_id",
        ),
        UsageStatsGroupBy::ApiKey => (
            "rl.api_key_id AS group_id,
             CAST(NULL AS BIGINT) AS provider_id,
             CAST(NULL AS BIGINT) AS model_id,
             rl.api_key_id AS api_key_id,
             CAST(NULL AS TEXT) AS provider_key,
             CAST(NULL AS TEXT) AS model_name,
             CAST(NULL AS TEXT) AS real_model_name,
             sak.name AS api_key_name,
             COALESCE(sak.name, '') AS group_label,
             sak.api_key AS group_detail",
            "rl.api_key_id, sak.name, sak.api_key",
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

fn load_usage_stats_base_rows(
    conn: &mut DbConnection,
    start_time_ms: i64,
    end_time_ms: i64,
    interval: &str,
    group_by: UsageStatsGroupBy,
    provider_id_filter: Option<i64>,
    model_id_filter: Option<i64>,
    api_key_id_filter: Option<i64>,
    provider_api_key_id_filter: Option<i64>,
) -> DbResult<Vec<UsageStatsBaseRow>> {
    let (group_select_sql, group_by_sql, group_id_sql) = usage_group_sql(group_by);
    match conn {
        DbConnection::Postgres(pg_conn) => {
            let bucket_sql = usage_bucket_sql_postgres(interval);
            let query = format!(
                "SELECT \
                    {bucket_sql} AS time_bucket, \
                    {group_select_sql}, \
                    CAST(COALESCE(SUM(rl.total_input_tokens), 0) AS BIGINT) AS total_input_tokens, \
                    CAST(COALESCE(SUM(rl.total_output_tokens), 0) AS BIGINT) AS total_output_tokens, \
                    CAST(COALESCE(SUM(rl.reasoning_tokens), 0) AS BIGINT) AS total_reasoning_tokens, \
                    CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens, \
                    CAST(COUNT(*) AS BIGINT) AS request_count, \
                    CAST(SUM(CASE WHEN CAST(rl.overall_status AS TEXT) = 'SUCCESS' THEN 1 ELSE 0 END) AS BIGINT) AS success_count, \
                    CAST(SUM(CASE WHEN CAST(rl.overall_status AS TEXT) IN ('ERROR', 'CANCELLED') THEN 1 ELSE 0 END) AS BIGINT) AS error_count, \
                    CAST(SUM(CASE WHEN rl.completed_at IS NOT NULL \
                                      AND rl.first_attempt_started_at IS NOT NULL \
                                      AND rl.completed_at >= rl.first_attempt_started_at \
                                 THEN (rl.completed_at - rl.first_attempt_started_at)::DOUBLE PRECISION \
                                 ELSE 0 END) AS DOUBLE PRECISION) AS latency_sum_ms, \
                    CAST(SUM(CASE WHEN rl.completed_at IS NOT NULL \
                                      AND rl.first_attempt_started_at IS NOT NULL \
                                      AND rl.completed_at >= rl.first_attempt_started_at \
                                 THEN 1 ELSE 0 END) AS BIGINT) AS latency_sample_count \
                 FROM request_log rl \
                 LEFT JOIN provider p ON p.id = rl.final_provider_id \
                 LEFT JOIN model m ON m.id = rl.final_model_id \
                 LEFT JOIN system_api_key sak ON sak.id = rl.api_key_id \
                 WHERE rl.request_received_at >= $1 \
                   AND rl.request_received_at < $2 \
                   AND {group_id_sql} IS NOT NULL \
                   AND ($3 IS NULL OR rl.final_provider_id = $3) \
                   AND ($4 IS NULL OR rl.final_model_id = $4) \
                   AND ($5 IS NULL OR rl.api_key_id = $5) \
                   AND ($6 IS NULL OR rl.final_provider_api_key_id = $6) \
                 GROUP BY 1, {group_by_sql} \
                 ORDER BY 1 ASC"
            );
            sql_query(query)
                .bind::<BigInt, _>(start_time_ms)
                .bind::<BigInt, _>(end_time_ms)
                .bind::<Nullable<BigInt>, _>(provider_id_filter)
                .bind::<Nullable<BigInt>, _>(model_id_filter)
                .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
                .load::<UsageStatsBaseRow>(pg_conn)
                .map_err(|e| {
                    crate::controller::BaseError::DatabaseFatal(Some(format!(
                        "Failed to load usage stats base rows: {}",
                        e
                    )))
                })
        }
        DbConnection::Sqlite(sqlite_conn) => {
            let bucket_sql = usage_bucket_sql_sqlite(interval);
            let query = format!(
                "SELECT \
                    {bucket_sql} AS time_bucket, \
                    {group_select_sql}, \
                    CAST(COALESCE(SUM(rl.total_input_tokens), 0) AS BIGINT) AS total_input_tokens, \
                    CAST(COALESCE(SUM(rl.total_output_tokens), 0) AS BIGINT) AS total_output_tokens, \
                    CAST(COALESCE(SUM(rl.reasoning_tokens), 0) AS BIGINT) AS total_reasoning_tokens, \
                    CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens, \
                    CAST(COUNT(*) AS BIGINT) AS request_count, \
                    CAST(SUM(CASE WHEN CAST(rl.overall_status AS TEXT) = 'SUCCESS' THEN 1 ELSE 0 END) AS BIGINT) AS success_count, \
                    CAST(SUM(CASE WHEN CAST(rl.overall_status AS TEXT) IN ('ERROR', 'CANCELLED') THEN 1 ELSE 0 END) AS BIGINT) AS error_count, \
                    CAST(SUM(CASE WHEN rl.completed_at IS NOT NULL \
                                      AND rl.first_attempt_started_at IS NOT NULL \
                                      AND rl.completed_at >= rl.first_attempt_started_at \
                                 THEN rl.completed_at - rl.first_attempt_started_at \
                                 ELSE 0 END) AS REAL) AS latency_sum_ms, \
                    CAST(SUM(CASE WHEN rl.completed_at IS NOT NULL \
                                      AND rl.first_attempt_started_at IS NOT NULL \
                                      AND rl.completed_at >= rl.first_attempt_started_at \
                                 THEN 1 ELSE 0 END) AS BIGINT) AS latency_sample_count \
                 FROM request_log rl \
                 LEFT JOIN provider p ON p.id = rl.final_provider_id \
                 LEFT JOIN model m ON m.id = rl.final_model_id \
                 LEFT JOIN system_api_key sak ON sak.id = rl.api_key_id \
                 WHERE rl.request_received_at >= ? \
                   AND rl.request_received_at < ? \
                   AND {group_id_sql} IS NOT NULL \
                   AND (? IS NULL OR rl.final_provider_id = ?) \
                   AND (? IS NULL OR rl.final_model_id = ?) \
                   AND (? IS NULL OR rl.api_key_id = ?) \
                   AND (? IS NULL OR rl.final_provider_api_key_id = ?) \
                 GROUP BY 1, {group_by_sql} \
                 ORDER BY 1 ASC"
            );
            sql_query(query)
                .bind::<BigInt, _>(start_time_ms)
                .bind::<BigInt, _>(end_time_ms)
                .bind::<Nullable<BigInt>, _>(provider_id_filter)
                .bind::<Nullable<BigInt>, _>(provider_id_filter)
                .bind::<Nullable<BigInt>, _>(model_id_filter)
                .bind::<Nullable<BigInt>, _>(model_id_filter)
                .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
                .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
                .load::<UsageStatsBaseRow>(sqlite_conn)
                .map_err(|e| {
                    crate::controller::BaseError::DatabaseFatal(Some(format!(
                        "Failed to load usage stats base rows: {}",
                        e
                    )))
                })
        }
    }
}

fn load_usage_stats_cost_rows(
    conn: &mut DbConnection,
    start_time_ms: i64,
    end_time_ms: i64,
    interval: &str,
    group_by: UsageStatsGroupBy,
    provider_id_filter: Option<i64>,
    model_id_filter: Option<i64>,
    api_key_id_filter: Option<i64>,
    provider_api_key_id_filter: Option<i64>,
) -> DbResult<Vec<UsageStatsCostRow>> {
    let (_, group_by_sql, group_id_sql) = usage_group_sql(group_by);

    match conn {
        DbConnection::Postgres(pg_conn) => {
            let bucket_sql = usage_bucket_sql_postgres(interval);
            let query = format!(
                "SELECT \
                    {bucket_sql} AS time_bucket, \
                    {group_id_sql} AS group_id, \
                    rl.estimated_cost_currency AS currency, \
                    CAST(COALESCE(SUM(rl.estimated_cost_nanos), 0) AS BIGINT) AS total_cost_nanos \
                 FROM request_log rl \
                 LEFT JOIN provider p ON p.id = rl.final_provider_id \
                 LEFT JOIN model m ON m.id = rl.final_model_id \
                 LEFT JOIN system_api_key sak ON sak.id = rl.api_key_id \
                 WHERE rl.request_received_at >= $1 \
                   AND rl.request_received_at < $2 \
                   AND {group_id_sql} IS NOT NULL \
                   AND ($3 IS NULL OR rl.final_provider_id = $3) \
                   AND ($4 IS NULL OR rl.final_model_id = $4) \
                   AND ($5 IS NULL OR rl.api_key_id = $5) \
                   AND ($6 IS NULL OR rl.final_provider_api_key_id = $6) \
                   AND rl.estimated_cost_nanos IS NOT NULL \
                   AND rl.estimated_cost_currency IS NOT NULL \
                 GROUP BY 1, {group_id_sql}, rl.estimated_cost_currency, {group_by_sql} \
                 ORDER BY 1 ASC"
            );
            sql_query(query)
                .bind::<BigInt, _>(start_time_ms)
                .bind::<BigInt, _>(end_time_ms)
                .bind::<Nullable<BigInt>, _>(provider_id_filter)
                .bind::<Nullable<BigInt>, _>(model_id_filter)
                .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
                .load::<UsageStatsCostRow>(pg_conn)
                .map_err(|e| {
                    crate::controller::BaseError::DatabaseFatal(Some(format!(
                        "Failed to load usage stats cost rows: {}",
                        e
                    )))
                })
        }
        DbConnection::Sqlite(sqlite_conn) => {
            let bucket_sql = usage_bucket_sql_sqlite(interval);
            let query = format!(
                "SELECT \
                    {bucket_sql} AS time_bucket, \
                    {group_id_sql} AS group_id, \
                    rl.estimated_cost_currency AS currency, \
                    CAST(COALESCE(SUM(rl.estimated_cost_nanos), 0) AS BIGINT) AS total_cost_nanos \
                 FROM request_log rl \
                 LEFT JOIN provider p ON p.id = rl.final_provider_id \
                 LEFT JOIN model m ON m.id = rl.final_model_id \
                 LEFT JOIN system_api_key sak ON sak.id = rl.api_key_id \
                 WHERE rl.request_received_at >= ? \
                   AND rl.request_received_at < ? \
                   AND {group_id_sql} IS NOT NULL \
                   AND (? IS NULL OR rl.final_provider_id = ?) \
                   AND (? IS NULL OR rl.final_model_id = ?) \
                   AND (? IS NULL OR rl.api_key_id = ?) \
                   AND (? IS NULL OR rl.final_provider_api_key_id = ?) \
                   AND rl.estimated_cost_nanos IS NOT NULL \
                   AND rl.estimated_cost_currency IS NOT NULL \
                 GROUP BY 1, {group_id_sql}, rl.estimated_cost_currency, {group_by_sql} \
                 ORDER BY 1 ASC"
            );
            sql_query(query)
                .bind::<BigInt, _>(start_time_ms)
                .bind::<BigInt, _>(end_time_ms)
                .bind::<Nullable<BigInt>, _>(provider_id_filter)
                .bind::<Nullable<BigInt>, _>(provider_id_filter)
                .bind::<Nullable<BigInt>, _>(model_id_filter)
                .bind::<Nullable<BigInt>, _>(model_id_filter)
                .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                .bind::<Nullable<BigInt>, _>(api_key_id_filter)
                .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
                .bind::<Nullable<BigInt>, _>(provider_api_key_id_filter)
                .load::<UsageStatsCostRow>(sqlite_conn)
                .map_err(|e| {
                    crate::controller::BaseError::DatabaseFatal(Some(format!(
                        "Failed to load usage stats cost rows: {}",
                        e
                    )))
                })
        }
    }
}

fn load_today_request_log_summary(
    conn: &mut DbConnection,
    start_of_today: i64,
) -> DbResult<TodayRequestLogSummaryRow> {
    let row: (i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>) = db_execute!(conn, {
        request_log::table
            .filter(request_log::dsl::request_received_at.ge(start_of_today))
            .select((
                count_star(),
                sum(request_log::dsl::total_input_tokens),
                sum(request_log::dsl::total_output_tokens),
                sum(request_log::dsl::reasoning_tokens),
                sum(request_log::dsl::total_tokens),
            ))
            .first(conn)
    })?;

    Ok(TodayRequestLogSummaryRow {
        requests_count: row.0,
        total_input_tokens: row.1,
        total_output_tokens: row.2,
        total_reasoning_tokens: row.3,
        total_tokens: row.4,
    })
}

fn load_today_cost_by_currency(
    conn: &mut DbConnection,
    start_of_today: i64,
) -> DbResult<HashMap<String, i64>> {
    let rows = match conn {
        DbConnection::Postgres(pg_conn) => sql_query(
            "SELECT estimated_cost_currency AS currency, \
                    CAST(SUM(estimated_cost_nanos) AS BIGINT) AS total_cost_nanos \
             FROM request_log \
             WHERE request_received_at >= $1 \
               AND estimated_cost_nanos IS NOT NULL \
               AND estimated_cost_currency IS NOT NULL \
             GROUP BY estimated_cost_currency",
        )
        .bind::<BigInt, _>(start_of_today)
        .load::<CostByCurrencyRow>(pg_conn)?,
        DbConnection::Sqlite(sqlite_conn) => sql_query(
            "SELECT estimated_cost_currency AS currency, \
                    CAST(SUM(estimated_cost_nanos) AS BIGINT) AS total_cost_nanos \
             FROM request_log \
             WHERE request_received_at >= ? \
               AND estimated_cost_nanos IS NOT NULL \
               AND estimated_cost_currency IS NOT NULL \
             GROUP BY estimated_cost_currency",
        )
        .bind::<BigInt, _>(start_of_today)
        .load::<CostByCurrencyRow>(sqlite_conn)?,
    };

    Ok(rows
        .into_iter()
        .map(|row| (row.currency, row.total_cost_nanos))
        .collect())
}

fn load_dashboard_today_aggregate(
    conn: &mut DbConnection,
    start_of_today: i64,
) -> DbResult<DashboardTodayAggregateRow> {
    match conn {
        DbConnection::Postgres(pg_conn) => sql_query(
            "SELECT \
                CAST(COUNT(*) AS BIGINT) AS request_count, \
                CAST(COALESCE(SUM(CASE WHEN CAST(overall_status AS TEXT) = 'SUCCESS' THEN 1 ELSE 0 END), 0) AS BIGINT) AS success_count, \
                CAST(COALESCE(SUM(CASE WHEN CAST(overall_status AS TEXT) IN ('ERROR', 'CANCELLED') THEN 1 ELSE 0 END), 0) AS BIGINT) AS error_count, \
                CAST(COALESCE(SUM(total_input_tokens), 0) AS BIGINT) AS total_input_tokens, \
                CAST(COALESCE(SUM(total_output_tokens), 0) AS BIGINT) AS total_output_tokens, \
                CAST(COALESCE(SUM(reasoning_tokens), 0) AS BIGINT) AS total_reasoning_tokens, \
                CAST(COALESCE(SUM(total_tokens), 0) AS BIGINT) AS total_tokens, \
                CAST(AVG(CASE \
                    WHEN first_attempt_started_at IS NOT NULL \
                     AND response_started_to_client_at IS NOT NULL \
                     AND response_started_to_client_at >= first_attempt_started_at \
                    THEN (response_started_to_client_at - first_attempt_started_at)::DOUBLE PRECISION \
                    ELSE NULL \
                END) AS DOUBLE PRECISION) AS avg_first_byte_ms, \
                CAST(AVG(CASE \
                    WHEN first_attempt_started_at IS NOT NULL \
                     AND completed_at IS NOT NULL \
                     AND completed_at >= first_attempt_started_at \
                    THEN (completed_at - first_attempt_started_at)::DOUBLE PRECISION \
                    ELSE NULL \
                END) AS DOUBLE PRECISION) AS avg_total_latency_ms, \
                CAST(COUNT(DISTINCT final_provider_id) AS BIGINT) AS active_provider_count, \
                CAST(COUNT(DISTINCT final_model_id) AS BIGINT) AS active_model_count, \
                CAST(COUNT(DISTINCT api_key_id) AS BIGINT) AS active_api_key_count \
             FROM request_log \
             WHERE request_received_at >= $1",
        )
        .bind::<BigInt, _>(start_of_today)
        .get_result::<DashboardTodayAggregateRow>(pg_conn)
        .map_err(|e| {
            crate::controller::BaseError::DatabaseFatal(Some(format!(
                "Failed to load dashboard today aggregate: {}",
                e
            )))
        }),
        DbConnection::Sqlite(sqlite_conn) => sql_query(
            "SELECT \
                CAST(COUNT(*) AS BIGINT) AS request_count, \
                CAST(COALESCE(SUM(CASE WHEN CAST(overall_status AS TEXT) = 'SUCCESS' THEN 1 ELSE 0 END), 0) AS BIGINT) AS success_count, \
                CAST(COALESCE(SUM(CASE WHEN CAST(overall_status AS TEXT) IN ('ERROR', 'CANCELLED') THEN 1 ELSE 0 END), 0) AS BIGINT) AS error_count, \
                CAST(COALESCE(SUM(total_input_tokens), 0) AS BIGINT) AS total_input_tokens, \
                CAST(COALESCE(SUM(total_output_tokens), 0) AS BIGINT) AS total_output_tokens, \
                CAST(COALESCE(SUM(reasoning_tokens), 0) AS BIGINT) AS total_reasoning_tokens, \
                CAST(COALESCE(SUM(total_tokens), 0) AS BIGINT) AS total_tokens, \
                CAST(AVG(CASE \
                    WHEN first_attempt_started_at IS NOT NULL \
                     AND response_started_to_client_at IS NOT NULL \
                     AND response_started_to_client_at >= first_attempt_started_at \
                    THEN (response_started_to_client_at - first_attempt_started_at) \
                    ELSE NULL \
                END) AS REAL) AS avg_first_byte_ms, \
                CAST(AVG(CASE \
                    WHEN first_attempt_started_at IS NOT NULL \
                     AND completed_at IS NOT NULL \
                     AND completed_at >= first_attempt_started_at \
                    THEN (completed_at - first_attempt_started_at) \
                    ELSE NULL \
                END) AS REAL) AS avg_total_latency_ms, \
                CAST(COUNT(DISTINCT final_provider_id) AS BIGINT) AS active_provider_count, \
                CAST(COUNT(DISTINCT final_model_id) AS BIGINT) AS active_model_count, \
                CAST(COUNT(DISTINCT api_key_id) AS BIGINT) AS active_api_key_count \
             FROM request_log \
             WHERE request_received_at >= ?",
        )
        .bind::<BigInt, _>(start_of_today)
        .get_result::<DashboardTodayAggregateRow>(sqlite_conn)
        .map_err(|e| {
            crate::controller::BaseError::DatabaseFatal(Some(format!(
                "Failed to load dashboard today aggregate: {}",
                e
            )))
        }),
    }
}

fn load_dashboard_top_model_base_rows(
    conn: &mut DbConnection,
    start_of_today: i64,
    limit: usize,
) -> DbResult<Vec<DashboardTopModelBaseRow>> {
    match conn {
        DbConnection::Postgres(pg_conn) => sql_query(
            "SELECT \
                rl.final_provider_id AS provider_id, \
                COALESCE(p.provider_key, rl.final_provider_key_snapshot) AS provider_key, \
                rl.final_model_id AS model_id, \
                COALESCE(m.model_name, rl.final_model_name_snapshot) AS model_name, \
                COALESCE(m.real_model_name, rl.final_real_model_name_snapshot) AS real_model_name, \
                COUNT(*)::BIGINT AS request_count, \
                CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens \
             FROM request_log rl \
             LEFT JOIN provider p ON p.id = rl.final_provider_id \
             LEFT JOIN model m ON m.id = rl.final_model_id \
             WHERE rl.request_received_at >= $1 \
               AND rl.final_provider_id IS NOT NULL \
               AND rl.final_model_id IS NOT NULL \
             GROUP BY rl.final_provider_id, p.provider_key, rl.final_provider_key_snapshot, rl.final_model_id, m.model_name, rl.final_model_name_snapshot, m.real_model_name, rl.final_real_model_name_snapshot \
             ORDER BY request_count DESC, rl.final_provider_id ASC, rl.final_model_id ASC \
             LIMIT $2",
        )
        .bind::<BigInt, _>(start_of_today)
        .bind::<BigInt, _>(limit as i64)
        .load::<DashboardTopModelBaseRow>(pg_conn)
        .map_err(|e| {
            crate::controller::BaseError::DatabaseFatal(Some(format!(
                "Failed to load dashboard top model rows: {}",
                e
            )))
        }),
        DbConnection::Sqlite(sqlite_conn) => sql_query(
            "SELECT \
                rl.final_provider_id AS provider_id, \
                COALESCE(p.provider_key, rl.final_provider_key_snapshot) AS provider_key, \
                rl.final_model_id AS model_id, \
                COALESCE(m.model_name, rl.final_model_name_snapshot) AS model_name, \
                COALESCE(m.real_model_name, rl.final_real_model_name_snapshot) AS real_model_name, \
                CAST(COUNT(*) AS BIGINT) AS request_count, \
                CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens \
             FROM request_log rl \
             LEFT JOIN provider p ON p.id = rl.final_provider_id \
             LEFT JOIN model m ON m.id = rl.final_model_id \
             WHERE rl.request_received_at >= ? \
               AND rl.final_provider_id IS NOT NULL \
               AND rl.final_model_id IS NOT NULL \
             GROUP BY rl.final_provider_id, p.provider_key, rl.final_provider_key_snapshot, rl.final_model_id, m.model_name, rl.final_model_name_snapshot, m.real_model_name, rl.final_real_model_name_snapshot \
             ORDER BY request_count DESC, rl.final_provider_id ASC, rl.final_model_id ASC \
             LIMIT ?",
        )
        .bind::<BigInt, _>(start_of_today)
        .bind::<BigInt, _>(limit as i64)
        .load::<DashboardTopModelBaseRow>(sqlite_conn)
        .map_err(|e| {
            crate::controller::BaseError::DatabaseFatal(Some(format!(
                "Failed to load dashboard top model rows: {}",
                e
            )))
        }),
    }
}

fn load_dashboard_top_model_base_rows_for_cost(
    conn: &mut DbConnection,
    start_of_today: i64,
    limit: usize,
) -> DbResult<Vec<DashboardTopModelBaseRow>> {
    match conn {
        DbConnection::Postgres(pg_conn) => sql_query(
            "SELECT \
                rl.final_provider_id AS provider_id, \
                COALESCE(p.provider_key, rl.final_provider_key_snapshot) AS provider_key, \
                rl.final_model_id AS model_id, \
                COALESCE(m.model_name, rl.final_model_name_snapshot) AS model_name, \
                COALESCE(m.real_model_name, rl.final_real_model_name_snapshot) AS real_model_name, \
                COUNT(*)::BIGINT AS request_count, \
                CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens \
             FROM request_log rl \
             LEFT JOIN provider p ON p.id = rl.final_provider_id \
             LEFT JOIN model m ON m.id = rl.final_model_id \
             WHERE rl.request_received_at >= $1 \
               AND rl.final_provider_id IS NOT NULL \
               AND rl.final_model_id IS NOT NULL \
             GROUP BY rl.final_provider_id, p.provider_key, rl.final_provider_key_snapshot, rl.final_model_id, m.model_name, rl.final_model_name_snapshot, m.real_model_name, rl.final_real_model_name_snapshot \
             ORDER BY COALESCE(SUM(rl.estimated_cost_nanos), 0) DESC, request_count DESC, rl.final_provider_id ASC, rl.final_model_id ASC \
             LIMIT $2",
        )
        .bind::<BigInt, _>(start_of_today)
        .bind::<BigInt, _>(limit as i64)
        .load::<DashboardTopModelBaseRow>(pg_conn)
        .map_err(|e| {
            crate::controller::BaseError::DatabaseFatal(Some(format!(
                "Failed to load dashboard top cost model rows: {}",
                e
            )))
        }),
        DbConnection::Sqlite(sqlite_conn) => sql_query(
            "SELECT \
                rl.final_provider_id AS provider_id, \
                COALESCE(p.provider_key, rl.final_provider_key_snapshot) AS provider_key, \
                rl.final_model_id AS model_id, \
                COALESCE(m.model_name, rl.final_model_name_snapshot) AS model_name, \
                COALESCE(m.real_model_name, rl.final_real_model_name_snapshot) AS real_model_name, \
                CAST(COUNT(*) AS BIGINT) AS request_count, \
                CAST(COALESCE(SUM(rl.total_tokens), 0) AS BIGINT) AS total_tokens \
             FROM request_log rl \
             LEFT JOIN provider p ON p.id = rl.final_provider_id \
             LEFT JOIN model m ON m.id = rl.final_model_id \
             WHERE rl.request_received_at >= ? \
               AND rl.final_provider_id IS NOT NULL \
               AND rl.final_model_id IS NOT NULL \
             GROUP BY rl.final_provider_id, p.provider_key, rl.final_provider_key_snapshot, rl.final_model_id, m.model_name, rl.final_model_name_snapshot, m.real_model_name, rl.final_real_model_name_snapshot \
             ORDER BY COALESCE(SUM(rl.estimated_cost_nanos), 0) DESC, request_count DESC, rl.final_provider_id ASC, rl.final_model_id ASC \
             LIMIT ?",
        )
        .bind::<BigInt, _>(start_of_today)
        .bind::<BigInt, _>(limit as i64)
        .load::<DashboardTopModelBaseRow>(sqlite_conn)
        .map_err(|e| {
            crate::controller::BaseError::DatabaseFatal(Some(format!(
                "Failed to load dashboard top cost model rows: {}",
                e
            )))
        }),
    }
}

fn load_dashboard_top_model_cost_rows(
    conn: &mut DbConnection,
    start_of_today: i64,
) -> DbResult<Vec<DashboardTopModelCostRow>> {
    match conn {
        DbConnection::Postgres(pg_conn) => sql_query(
            "SELECT \
                rl.final_provider_id AS provider_id, \
                rl.final_model_id AS model_id, \
                rl.estimated_cost_currency AS currency, \
                CAST(SUM(rl.estimated_cost_nanos) AS BIGINT) AS total_cost_nanos \
             FROM request_log rl \
             WHERE rl.request_received_at >= $1 \
               AND rl.final_provider_id IS NOT NULL \
               AND rl.final_model_id IS NOT NULL \
               AND rl.estimated_cost_nanos IS NOT NULL \
               AND rl.estimated_cost_currency IS NOT NULL \
             GROUP BY rl.final_provider_id, rl.final_model_id, rl.estimated_cost_currency",
        )
        .bind::<BigInt, _>(start_of_today)
        .load::<DashboardTopModelCostRow>(pg_conn)
        .map_err(|e| {
            crate::controller::BaseError::DatabaseFatal(Some(format!(
                "Failed to load dashboard top model costs: {}",
                e
            )))
        }),
        DbConnection::Sqlite(sqlite_conn) => sql_query(
            "SELECT \
                rl.final_provider_id AS provider_id, \
                rl.final_model_id AS model_id, \
                rl.estimated_cost_currency AS currency, \
                CAST(SUM(rl.estimated_cost_nanos) AS BIGINT) AS total_cost_nanos \
             FROM request_log rl \
             WHERE rl.request_received_at >= ? \
               AND rl.final_provider_id IS NOT NULL \
               AND rl.final_model_id IS NOT NULL \
               AND rl.estimated_cost_nanos IS NOT NULL \
               AND rl.estimated_cost_currency IS NOT NULL \
             GROUP BY rl.final_provider_id, rl.final_model_id, rl.estimated_cost_currency",
        )
        .bind::<BigInt, _>(start_of_today)
        .load::<DashboardTopModelCostRow>(sqlite_conn)
        .map_err(|e| {
            crate::controller::BaseError::DatabaseFatal(Some(format!(
                "Failed to load dashboard top model costs: {}",
                e
            )))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CostByCurrencyRow, TodayRequestLogSummaryRow, UsageStatsGroupBy, calculate_success_rate,
        load_dashboard_today_aggregate, load_dashboard_top_model_base_rows,
        load_dashboard_top_model_cost_rows, load_usage_stats_base_rows, load_usage_stats_cost_rows,
    };
    use crate::database::DbConnection;
    use diesel::connection::SimpleConnection;
    use diesel::r2d2::{ConnectionManager, Pool};
    use diesel::sqlite::SqliteConnection;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use tempfile::tempdir;

    const SQLITE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/sqlite");

    #[test]
    fn today_summary_row_maps_missing_sums_to_zero() {
        let row = TodayRequestLogSummaryRow {
            requests_count: 3,
            total_input_tokens: None,
            total_output_tokens: Some(11),
            total_reasoning_tokens: None,
            total_tokens: Some(21),
        };

        assert_eq!(row.requests_count, 3);
        assert_eq!(row.total_input_tokens.unwrap_or(0), 0);
        assert_eq!(row.total_output_tokens.unwrap_or(0), 11);
        assert_eq!(row.total_reasoning_tokens.unwrap_or(0), 0);
        assert_eq!(row.total_tokens.unwrap_or(0), 21);
    }

    #[test]
    fn calculate_success_rate_handles_empty_and_non_empty_windows() {
        assert_eq!(calculate_success_rate(0, 0), None);
        assert_eq!(calculate_success_rate(10, 7), Some(0.7));
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

        assert!(
            source.contains("GROUP BY estimated_cost_currency"),
            "today cost aggregation should stay grouped in SQL",
        );
        assert!(
            source.contains("CAST(COUNT(*) AS BIGINT) AS request_count")
                && source.contains(
                    "CAST(COUNT(DISTINCT final_provider_id) AS BIGINT) AS active_provider_count"
                )
                && source.contains(
                    "CAST(COALESCE(SUM(total_input_tokens), 0) AS BIGINT) AS total_input_tokens"
                )
                && source
                    .contains("CAST(COALESCE(SUM(total_tokens), 0) AS BIGINT) AS total_tokens"),
            "today summary should remain a database-side aggregate query",
        );
    }

    fn sqlite_stat_connection() -> (tempfile::TempDir, DbConnection) {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("stat.sqlite");
        std::fs::File::create(&db_path).expect("db file should be created");
        let db_url = db_path.to_string_lossy().into_owned();
        let manager = ConnectionManager::<SqliteConnection>::new(db_url);
        let pool = Pool::builder()
            .max_size(1)
            .build(manager)
            .expect("sqlite pool should be created");
        let mut conn = pool.get().expect("sqlite connection should be checked out");
        conn.run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("migrations should run");
        seed_stat_rows(&mut conn);
        (temp_dir, DbConnection::Sqlite(conn))
    }

    fn seed_stat_rows(conn: &mut SqliteConnection) {
        conn.batch_execute(
            "INSERT INTO system_api_key (
                id, api_key, name, description, access_control_policy_id,
                usage_limit_policy_id, is_enabled, deleted_at, created_at, updated_at
            ) VALUES (
                1, 'ck-test', 'Ops key', NULL, NULL,
                NULL, 1, NULL, 1, 1
            );

            INSERT INTO api_key (
                id, api_key, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm, max_concurrent_requests,
                quota_daily_requests, quota_daily_tokens, quota_monthly_tokens,
                budget_daily_nanos, budget_daily_currency, budget_monthly_nanos,
                budget_monthly_currency, deleted_at, created_at, updated_at
            ) VALUES (
                1, 'ck-test', 'hash', 'ck-test', 'test', 'Ops key', NULL,
                'ALLOW', 1, NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, 1, 1
            );

            INSERT INTO provider (
                id, provider_key, name, endpoint, use_proxy, is_enabled, deleted_at,
                created_at, updated_at, provider_type, provider_api_key_mode
            ) VALUES (
                10, 'openai-main', 'OpenAI Main', 'https://api.example.com/v1', 0, 1, NULL,
                1, 1, 'OPENAI', 'QUEUE'
            );

            INSERT INTO provider_api_key (
                id, provider_id, api_key, description, deleted_at, is_enabled, created_at, updated_at
            ) VALUES (
                20, 10, 'sk-provider', NULL, NULL, 1, 1, 1
            );

            INSERT INTO model (
                id, provider_id, cost_catalog_id, model_name, real_model_name,
                supports_streaming, supports_tools, supports_reasoning, supports_image_input,
                supports_embeddings, supports_rerank, is_enabled, deleted_at, created_at, updated_at
            ) VALUES (
                30, 10, NULL, 'gpt-test', 'gpt-test-real',
                1, 1, 1, 1,
                0, 0, 1, NULL, 1, 1
            );

            INSERT INTO request_log (
                id, api_key_id, requested_model_name, resolved_name_scope,
                user_api_type, overall_status, final_error_code, final_error_message,
                attempt_count, retry_count, fallback_count, request_received_at,
                first_attempt_started_at, response_started_to_client_at, completed_at,
                final_provider_id, final_provider_api_key_id, final_model_id,
                final_provider_key_snapshot, final_provider_name_snapshot,
                final_model_name_snapshot, final_real_model_name_snapshot, final_llm_api_type,
                estimated_cost_nanos, estimated_cost_currency,
                total_input_tokens, total_output_tokens, reasoning_tokens, total_tokens,
                created_at, updated_at
            ) VALUES
            (
                100, 1, 'gpt-test', 'direct',
                'OPENAI', 'SUCCESS', NULL, NULL,
                1, 0, 0, 1000,
                1100, 1200, 1500,
                10, 20, 30,
                'openai-main', 'OpenAI Main',
                'gpt-test', 'gpt-test-real', 'OPENAI',
                500, 'USD',
                10, 20, 5, 35,
                1000, 1500
            ),
            (
                101, 1, 'gpt-test', 'direct',
                'OPENAI', 'ERROR', 'upstream_service_error', 'failed',
                1, 0, 0, 2000,
                2250, 2300, 2550,
                10, 20, 30,
                'openai-main', 'OpenAI Main',
                'gpt-test', 'gpt-test-real', 'OPENAI',
                300, 'USD',
                7, 13, 2, 22,
                2000, 2550
            ),
            (
                102, 1, 'gpt-empty', 'direct',
                'OPENAI', 'SUCCESS', NULL, NULL,
                1, 0, 0, 3000,
                NULL, NULL, 3100,
                NULL, NULL, NULL,
                NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL,
                1, 2, 0, 3,
                3000, 3100
            );",
        )
        .expect("stat seed rows should insert");
    }

    #[test]
    fn sqlite_usage_stats_queries_request_log_aggregate_columns() {
        let (_temp_dir, mut conn) = sqlite_stat_connection();

        let rows = load_usage_stats_base_rows(
            &mut conn,
            0,
            10_000,
            "day",
            UsageStatsGroupBy::Provider,
            None,
            None,
            None,
            None,
        )
        .expect("usage stats base rows should load");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.group_id, 10);
        assert_eq!(row.provider_id, Some(10));
        assert_eq!(row.provider_key.as_deref(), Some("openai-main"));
        assert_eq!(row.request_count, 2);
        assert_eq!(row.success_count, 1);
        assert_eq!(row.error_count, 1);
        assert_eq!(row.total_tokens, 57);
        assert_eq!(row.latency_sample_count, 2);
        assert_eq!(row.latency_sum_ms, Some(700.0));

        let costs = load_usage_stats_cost_rows(
            &mut conn,
            0,
            10_000,
            "day",
            UsageStatsGroupBy::Provider,
            None,
            None,
            None,
            None,
        )
        .expect("usage stats cost rows should load");
        assert_eq!(costs.len(), 1);
        assert_eq!(costs[0].group_id, 10);
        assert_eq!(costs[0].currency, "USD");
        assert_eq!(costs[0].total_cost_nanos, 800);
    }

    #[test]
    fn sqlite_dashboard_queries_request_log_aggregate_columns() {
        let (_temp_dir, mut conn) = sqlite_stat_connection();

        let aggregate =
            load_dashboard_today_aggregate(&mut conn, 0).expect("dashboard aggregate should load");
        assert_eq!(aggregate.request_count, 3);
        assert_eq!(aggregate.success_count, 2);
        assert_eq!(aggregate.error_count, 1);
        assert_eq!(aggregate.total_tokens, 60);
        assert_eq!(aggregate.avg_first_byte_ms, Some(75.0));
        assert_eq!(aggregate.avg_total_latency_ms, Some(350.0));
        assert_eq!(aggregate.active_provider_count, 1);
        assert_eq!(aggregate.active_model_count, 1);
        assert_eq!(aggregate.active_api_key_count, 1);

        let top_models = load_dashboard_top_model_base_rows(&mut conn, 0, 10)
            .expect("dashboard top models should load");
        assert_eq!(top_models.len(), 1);
        assert_eq!(top_models[0].provider_id, 10);
        assert_eq!(top_models[0].model_id, 30);
        assert_eq!(top_models[0].model_name.as_deref(), Some("gpt-test"));
        assert_eq!(top_models[0].request_count, 2);
        assert_eq!(top_models[0].total_tokens, 57);

        let top_costs = load_dashboard_top_model_cost_rows(&mut conn, 0)
            .expect("dashboard top model costs should load");
        assert_eq!(top_costs.len(), 1);
        assert_eq!(top_costs[0].provider_id, 10);
        assert_eq!(top_costs[0].model_id, 30);
        assert_eq!(top_costs[0].currency, "USD");
        assert_eq!(top_costs[0].total_cost_nanos, 800);
    }
}
