use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    routing::{get, post},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::controller::BaseError;
use crate::database::alert::{AlertEvent, AlertListFilter};
use crate::service::alerts::types::{
    AlertAcknowledgeInput, AlertSeverity, AlertStatus, AlertSuppressInput,
};
use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::utils::HttpResult;

#[derive(Debug, Deserialize, Default)]
struct AlertListParams {
    status: Option<String>,
    acknowledged: Option<bool>,
    suppressed: Option<bool>,
    severity: Option<String>,
    rule_key: Option<String>,
    scope_type: Option<String>,
    scope_id: Option<String>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AlertAckRequest {
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlertSuppressRequest {
    suppressed_until: i64,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct AlertListResponse {
    items: Vec<AlertEvent>,
    limit: i64,
    offset: i64,
    next_offset: Option<i64>,
}

async fn list_alerts(
    State(app_state): State<Arc<AppState>>,
    Query(params): Query<AlertListParams>,
) -> Result<HttpResult<AlertListResponse>, BaseError> {
    let now_ms = Utc::now().timestamp_millis();
    let (mut filter, limit, offset) = alert_list_filter(params, now_ms)?;
    filter.limit = Some(limit + 1);
    filter.offset = Some(offset);

    let mut items = app_state.alerts.list_alerts(filter)?;
    let next_offset = if items.len() as i64 > limit {
        items.truncate(limit as usize);
        Some(offset + limit)
    } else {
        None
    };

    Ok(HttpResult::new(AlertListResponse {
        items,
        limit,
        offset,
        next_offset,
    }))
}

async fn get_alert(
    State(app_state): State<Arc<AppState>>,
    Path(alert_id): Path<i64>,
) -> Result<HttpResult<AlertEvent>, BaseError> {
    Ok(HttpResult::new(app_state.alerts.get_alert(alert_id)?))
}

async fn acknowledge_alert(
    State(app_state): State<Arc<AppState>>,
    Path(alert_id): Path<i64>,
    Json(payload): Json<AlertAckRequest>,
) -> Result<HttpResult<AlertEvent>, BaseError> {
    Ok(HttpResult::new(app_state.alerts.acknowledge_alert(
        alert_id,
        AlertAcknowledgeInput { note: payload.note },
        Utc::now().timestamp_millis(),
    )?))
}

async fn suppress_alert(
    State(app_state): State<Arc<AppState>>,
    Path(alert_id): Path<i64>,
    Json(payload): Json<AlertSuppressRequest>,
) -> Result<HttpResult<AlertEvent>, BaseError> {
    let now_ms = Utc::now().timestamp_millis();
    let input = suppress_input(payload, now_ms)?;
    Ok(HttpResult::new(
        app_state.alerts.suppress_alert(alert_id, input, now_ms)?,
    ))
}

async fn unsuppress_alert(
    State(app_state): State<Arc<AppState>>,
    Path(alert_id): Path<i64>,
) -> Result<HttpResult<AlertEvent>, BaseError> {
    Ok(HttpResult::new(app_state.alerts.unsuppress_alert(
        alert_id,
        Utc::now().timestamp_millis(),
    )?))
}

async fn resolve_alert(
    State(app_state): State<Arc<AppState>>,
    Path(alert_id): Path<i64>,
) -> Result<HttpResult<AlertEvent>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .alerts
            .resolve_alert(alert_id, Utc::now().timestamp_millis())?,
    ))
}

fn alert_list_filter(
    params: AlertListParams,
    now_ms: i64,
) -> Result<(AlertListFilter, i64, i64), BaseError> {
    let limit = params.limit.unwrap_or(50);
    if !(1..=200).contains(&limit) {
        return Err(BaseError::ParamInvalid(Some(
            "limit must be between 1 and 200".to_string(),
        )));
    }
    let offset = params.offset.unwrap_or(0);
    if offset < 0 {
        return Err(BaseError::ParamInvalid(Some(
            "offset must be greater than or equal to 0".to_string(),
        )));
    }
    if let Some(status) = params.status.as_deref() {
        AlertStatus::try_from(status).map_err(|err| BaseError::ParamInvalid(Some(err)))?;
    }
    if let Some(severity) = params.severity.as_deref() {
        AlertSeverity::try_from(severity).map_err(|err| BaseError::ParamInvalid(Some(err)))?;
    }
    if let Some(scope_type) = params.scope_type.as_deref() {
        validate_scope_type(scope_type)?;
    }
    if let (Some(start), Some(end)) = (params.start_time, params.end_time) {
        if start >= end {
            return Err(BaseError::ParamInvalid(Some(
                "start_time must be before end_time".to_string(),
            )));
        }
    }

    Ok((
        AlertListFilter {
            status: params.status,
            severity: params.severity,
            rule_key: params.rule_key,
            scope_type: params.scope_type,
            scope_id: params.scope_id,
            acknowledged: params.acknowledged,
            suppressed: params.suppressed,
            seen_from: params.start_time,
            seen_to: params.end_time,
            now_ms: Some(now_ms),
            limit: Some(limit),
            offset: Some(offset),
        },
        limit,
        offset,
    ))
}

fn suppress_input(
    payload: AlertSuppressRequest,
    now_ms: i64,
) -> Result<AlertSuppressInput, BaseError> {
    if payload.suppressed_until <= now_ms {
        return Err(BaseError::ParamInvalid(Some(
            "suppressed_until must be in the future".to_string(),
        )));
    }
    Ok(AlertSuppressInput {
        suppressed_until: payload.suppressed_until,
        reason: payload.reason,
    })
}

fn validate_scope_type(scope_type: &str) -> Result<(), BaseError> {
    match scope_type {
        "global" => Ok(()),
        "provider" => Ok(()),
        "model" => Ok(()),
        "api_key" => Ok(()),
        "provider_api_key" => Ok(()),
        "provider_model" => Ok(()),
        "system" => Ok(()),
        _ => Err(BaseError::ParamInvalid(Some(format!(
            "invalid alert scope_type '{}'",
            scope_type
        )))),
    }
}

pub fn create_alert_router() -> StateRouter {
    create_state_router().nest(
        "/alerts",
        create_state_router()
            .route("/list", get(list_alerts))
            .route("/{id}", get(get_alert))
            .route("/{id}/ack", post(acknowledge_alert))
            .route("/{id}/suppress", post(suppress_alert))
            .route("/{id}/unsuppress", post(unsuppress_alert))
            .route("/{id}/resolve", post(resolve_alert)),
    )
}

#[cfg(test)]
mod tests {
    use super::{AlertListParams, AlertSuppressRequest, alert_list_filter, suppress_input};
    use crate::controller::BaseError;

    #[test]
    fn list_filter_validates_enums_and_pagination() {
        let err = alert_list_filter(
            AlertListParams {
                status: Some("unknown".to_string()),
                limit: Some(50),
                ..AlertListParams::default()
            },
            1_000,
        )
        .expect_err("invalid status should fail");
        assert!(matches!(err, BaseError::ParamInvalid(Some(_))));

        let (filter, limit, offset) = alert_list_filter(
            AlertListParams {
                status: Some("active".to_string()),
                severity: Some("critical".to_string()),
                suppressed: Some(true),
                limit: Some(25),
                offset: Some(50),
                ..AlertListParams::default()
            },
            1_000,
        )
        .unwrap();
        assert_eq!(filter.status.as_deref(), Some("active"));
        assert_eq!(filter.severity.as_deref(), Some("critical"));
        assert_eq!(filter.suppressed, Some(true));
        assert_eq!(limit, 25);
        assert_eq!(offset, 50);
    }

    #[test]
    fn suppress_requires_future_until() {
        let err = suppress_input(
            AlertSuppressRequest {
                suppressed_until: 999,
                reason: None,
            },
            1_000,
        )
        .expect_err("past suppress until should fail");
        assert!(matches!(err, BaseError::ParamInvalid(Some(_))));

        let input = suppress_input(
            AlertSuppressRequest {
                suppressed_until: 2_000,
                reason: Some("maintenance".to_string()),
            },
            1_000,
        )
        .unwrap();
        assert_eq!(input.suppressed_until, 2_000);
        assert_eq!(input.reason.as_deref(), Some("maintenance"));
    }
}
