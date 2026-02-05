use axum::{extract::Query, routing::get};
use chrono::{Datelike, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::config::CONFIG;
use crate::service::app_state::{create_state_router, StateRouter};
use crate::{
    controller::error::BaseError,
    database::stat::{
        get_system_overview_stats, get_today_request_log_stats, SystemOverviewStats,
        TodayRequestLogStats, get_request_logs_in_range,
    },
    utils::HttpResult,
};

#[derive(Deserialize, Debug)]
pub struct UsageStatsParams {
    #[serde(default = "default_start_time")]
    start_time: i64, // Milliseconds
    #[serde(default = "default_end_time")]
    end_time: i64, // Milliseconds
    #[serde(default = "default_interval")]
    interval: Interval, // "month", "day", "hour"
    provider_id: Option<i64>,
    model_id: Option<i64>,
    system_api_key_id: Option<i64>,
    provider_api_key_id: Option<i64>,
}

fn default_start_time() -> i64 {
    0
}

fn default_end_time() -> i64 {
    Utc::now().timestamp_millis()
}

fn default_interval() -> Interval {
    Interval::Day
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Interval {
    Minute,
    Hour,
    Day,
    Month,
}

impl Interval {
    #[allow(dead_code)]
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "minute" => Ok(Interval::Minute),
            "hour" => Ok(Interval::Hour),
            "day" => Ok(Interval::Day),
            "month" => Ok(Interval::Month),
            _ => Err(format!("Invalid interval: {}. Supported intervals are 'minute', 'hour', 'day', 'month'.", s)),
        }
    }
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct UsageStatItem {
    provider_id: i64,
    model_id: i64,
    provider_key: String,
    model_name: String,
    real_model_name: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    reasoning_tokens: i64,
    total_tokens: i64,
    request_count: i64,
    total_cost: HashMap<String, i64>,
}

#[derive(Serialize, Debug)]
pub struct UsageStatsPeriod {
    time: i64, // Timestamp for the beginning of the period (milliseconds)
    data: Vec<UsageStatItem>,
}

fn get_time_bucket(timestamp_ms: i64, interval: Interval, tz: Tz) -> i64 {
    // Create a DateTime<Utc> from the millisecond timestamp
    let dt_utc = Utc.timestamp_millis_opt(timestamp_ms).unwrap(); // Assuming valid timestamp

    // Convert it to the target timezone
    let dt_tz = dt_utc.with_timezone(&tz);

    // Perform bucketing in the target timezone
    let bucketed_dt_tz = match interval {
        Interval::Minute => dt_tz.with_second(0).unwrap().with_nanosecond(0).unwrap(),
        Interval::Hour => dt_tz
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap(),
        Interval::Day => dt_tz
            .with_hour(0)
            .unwrap()
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap(),
        Interval::Month => dt_tz
            .with_day(1)
            .unwrap()
            .with_hour(0)
            .unwrap()
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap(),
    };
    // Convert the bucketed DateTime<Tz> back to a UTC millisecond timestamp
    bucketed_dt_tz.with_timezone(&Utc).timestamp_millis()
}

async fn system_overview_stats() -> Result<HttpResult<SystemOverviewStats>, BaseError> {
    let stats = get_system_overview_stats()?;
    Ok(HttpResult::new(stats))
}

async fn today_request_log_stats() -> Result<HttpResult<TodayRequestLogStats>, BaseError> {
    let stats = get_today_request_log_stats()?;
    Ok(HttpResult::new(stats))
}

async fn system_usage_stats(
    Query(params): Query<UsageStatsParams>,
) -> Result<HttpResult<Vec<UsageStatsPeriod>>, BaseError> {
    let interval = params.interval;

    let time_range_ms = params.end_time - params.start_time;
    match interval {
        Interval::Minute => {
            if time_range_ms > 180 * 60 * 1000 {
                return Err(BaseError::ParamInvalid(Some(
                    "For minute interval, the time range cannot exceed 180 minutes.".to_string(),
                )));
            }
        }
        Interval::Hour => {
            if time_range_ms > 168 * 60 * 60 * 1000 {
                return Err(BaseError::ParamInvalid(Some(
                    "For hour interval, the time range cannot exceed 168 hours.".to_string(),
                )));
            }
        }
        Interval::Day => {
            let one_eighty_days_in_ms: i64 = 180 * 24 * 60 * 60 * 1000;
            if time_range_ms > one_eighty_days_in_ms {
                return Err(BaseError::ParamInvalid(Some(
                    "For day interval, the time range cannot exceed 180 days.".to_string(),
                )));
            }
        }
        Interval::Month => {}
    }

    let tz: Tz = CONFIG
        .timezone
        .as_deref()
        .and_then(|tz_str| tz_str.parse::<Tz>().ok())
        .unwrap_or(Tz::Etc__UTC);

    if params.start_time >= params.end_time {
        return Err(BaseError::ParamInvalid(Some(
            "startTime must be before endTime".to_string(),
        )));
    }

    let logs = get_request_logs_in_range(
        params.start_time,
        params.end_time,
        params.provider_id,
        params.model_id,
        params.system_api_key_id,
        params.provider_api_key_id,
    )?;

    let mut aggregated_data: HashMap<i64, HashMap<(i64, i64), UsageStatItem>> =
        HashMap::new();

    for log_entry in logs {
        let time_bucket = get_time_bucket(log_entry.created_at, interval, tz);
        let provider_model_key = (log_entry.provider_id, log_entry.model_id);

        let period_map = aggregated_data.entry(time_bucket).or_default();
        let stat_item = period_map.entry(provider_model_key).or_insert_with(|| {
            UsageStatItem {
                provider_id: log_entry.provider_id,
                model_id: log_entry.model_id,
                provider_key: log_entry.provider_key.clone().unwrap_or_default(),
                model_name: log_entry.model_name.clone().unwrap_or_default(),
                real_model_name: log_entry.real_model_name.clone(),
                ..Default::default()
            }
        });

        stat_item.input_tokens += log_entry.input_tokens.unwrap_or(0) as i64;
        stat_item.output_tokens += log_entry.output_tokens.unwrap_or(0) as i64;
        stat_item.reasoning_tokens += log_entry.reasoning_tokens.unwrap_or(0) as i64;
        stat_item.total_tokens += log_entry.total_tokens.unwrap_or(0) as i64;
        stat_item.request_count += 1;
        if let (Some(cost), Some(currency)) = (log_entry.calculated_cost, log_entry.cost_currency) {
            if cost > 0 {
                *stat_item.total_cost.entry(currency).or_insert(0) += cost;
            }
        }
    }

    let mut result: Vec<UsageStatsPeriod> = aggregated_data
        .into_iter()
        .map(|(time_bucket, provider_model_map)| UsageStatsPeriod {
            time: time_bucket,
            data: provider_model_map.into_values().collect(),
        })
        .collect();

    result.sort_by_key(|period| period.time);

    Ok(HttpResult::new(result))
}

pub fn routes() -> StateRouter {
    create_state_router()
        .route("/system/overview", get(system_overview_stats))
        .route("/system/today_log_stats", get(today_request_log_stats))
        .route("/system/usage_stats", get(system_usage_stats))
}
