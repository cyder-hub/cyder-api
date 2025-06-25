use crate::config::CONFIG;
use crate::database::{get_connection, DbResult};
use crate::{db_execute, db_object};
use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use diesel::dsl::{count_star, sum};
use diesel::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug)]
    #[diesel(table_name = system_api_key)]
    pub struct SystemApiKey {
        pub id: i64,
    }
}

#[derive(Queryable, Debug)]
pub struct RequestLogEntryForStats {
    // from request_log
    pub created_at: i64,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub reasoning_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub calculated_cost: Option<i64>,
    pub cost_currency: Option<String>,
    // from joined tables
    pub provider_key: Option<String>,
    pub model_name: Option<String>,
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
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_reasoning_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: HashMap<String, i64>,
}

pub fn get_system_overview_stats() -> DbResult<SystemOverviewStats> {
    let conn = &mut get_connection();
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
    system_api_key_id_filter: Option<i64>,
    provider_api_key_id_filter: Option<i64>,
) -> DbResult<Vec<RequestLogEntryForStats>> {
    let conn = &mut get_connection();
    let result = db_execute!(conn, {
        let mut query = request_log::table
            .left_join(provider::table.on(request_log::dsl::provider_id.eq(provider::dsl::id.nullable())))
            .left_join(model::table.on(request_log::dsl::model_id.eq(model::dsl::id.nullable())))
            .filter(request_log::dsl::created_at.ge(start_time_ms))
            .filter(request_log::dsl::created_at.lt(end_time_ms))
            .into_boxed();

        if let Some(provider_id) = provider_id_filter {
            query = query.filter(request_log::dsl::provider_id.eq(provider_id));
        }
        if let Some(model_id) = model_id_filter {
            query = query.filter(request_log::dsl::model_id.eq(model_id));
        }
        if let Some(system_api_key_id) = system_api_key_id_filter {
            query = query.filter(request_log::dsl::system_api_key_id.eq(system_api_key_id));
        }
        if let Some(provider_api_key_id) = provider_api_key_id_filter {
            query = query.filter(request_log::dsl::provider_api_key_id.eq(provider_api_key_id));
        }

        query
            .select((
                request_log::dsl::created_at,
                request_log::dsl::provider_id,
                request_log::dsl::model_id,
                request_log::dsl::prompt_tokens,
                request_log::dsl::completion_tokens,
                request_log::dsl::reasoning_tokens,
                request_log::dsl::total_tokens,
                request_log::dsl::calculated_cost,
                request_log::dsl::cost_currency.nullable(),
                provider::dsl::provider_key.nullable(),
                model::dsl::model_name.nullable(),
                model::dsl::real_model_name.nullable(),
            ))
            .order(request_log::dsl::created_at.asc()) // Order by created_at for potentially easier processing later, though not strictly necessary for aggregation
            .load::<RequestLogEntryForStats>(conn)
    })?;
    Ok(result)
}

pub fn get_today_request_log_stats() -> DbResult<TodayRequestLogStats> {
    let conn = &mut get_connection();
    let mut stats = TodayRequestLogStats::default();

    let tz: Tz = CONFIG
        .timezone
        .as_deref()
        .and_then(|tz_str| tz_str.parse::<Tz>().ok())
        .unwrap_or(Tz::Etc__UTC);

    let now = Utc::now();
    let today_in_tz = now.with_timezone(&tz).date_naive();
    let start_of_today = tz
        .from_local_datetime(&today_in_tz.and_hms_opt(0, 0, 0).unwrap())
        .unwrap()
        .timestamp_millis();

    stats.requests_count = db_execute!(conn, {
        request_log::table
            .filter(request_log::dsl::created_at.ge(start_of_today))
            .select(count_star())
            .first(conn)
    })?;

    let prompt_tokens_sum: Option<i64> = db_execute!(conn, {
        request_log::table
            .filter(request_log::dsl::created_at.ge(start_of_today))
            .select(sum(request_log::dsl::prompt_tokens))
            .first(conn)
    })?;
    stats.total_prompt_tokens = prompt_tokens_sum.unwrap_or(0);

    let completion_tokens_sum: Option<i64> = db_execute!(conn, {
        request_log::table
            .filter(request_log::dsl::created_at.ge(start_of_today))
            .select(sum(request_log::dsl::completion_tokens))
            .first(conn)
    })?;
    stats.total_completion_tokens = completion_tokens_sum.unwrap_or(0);

    let reasoning_tokens_sum: Option<i64> = db_execute!(conn, {
        request_log::table
            .filter(request_log::dsl::created_at.ge(start_of_today))
            .select(sum(request_log::dsl::reasoning_tokens))
            .first(conn)
    })?;
    stats.total_reasoning_tokens = reasoning_tokens_sum.unwrap_or(0);

    let total_tokens_sum: Option<i64> = db_execute!(conn, {
        request_log::table
            .filter(request_log::dsl::created_at.ge(start_of_today))
            .select(sum(request_log::dsl::total_tokens))
            .first(conn)
    })?;
    stats.total_tokens = total_tokens_sum.unwrap_or(0);

    // Note: The sum of `calculated_cost` (a BigInt) is not portable across database backends in Diesel.
    // PostgreSQL returns a `Numeric` type, while SQLite returns a `Float`.
    // To ensure compatibility, we fetch the individual costs and sum them in the application.
    // This is less efficient than a database-level SUM, but it is portable.
    // For high-volume scenarios, a database-specific query or a different schema approach might be needed.
    let costs_with_currency: Vec<(Option<i64>, Option<String>)> = db_execute!(conn, {
        request_log::table
            .filter(request_log::dsl::created_at.ge(start_of_today))
            .select((
                request_log::dsl::calculated_cost,
                request_log::dsl::cost_currency,
            ))
            .load(conn)
    })?;

    let mut total_cost_by_currency: HashMap<String, i64> = HashMap::new();
    for (cost_micros_opt, currency_opt) in costs_with_currency {
        if let (Some(cost_micros), Some(currency)) = (cost_micros_opt, currency_opt) {
            *total_cost_by_currency.entry(currency).or_insert(0) += cost_micros;
        }
    }

    stats.total_cost = total_cost_by_currency;

    Ok(stats)
}
