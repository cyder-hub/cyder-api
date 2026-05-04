use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    routing::{get, post},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    controller::BaseError,
    service::{
        app_state::{AppState, StateRouter, create_state_router},
        metrics::types::{
            MetricsIngestStatus, MetricsReconciliationParams, MetricsReconciliationSummary,
            MetricsRepairParams, MetricsRepairSummary,
        },
    },
    utils::HttpResult,
};

#[derive(Debug, Deserialize)]
struct MetricsIngestStatusParams {
    start_time: Option<i64>,
    end_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct MetricsReconciliationRequest {
    start_time: i64,
    end_time: i64,
    limit: Option<usize>,
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MetricsRepairRequest {
    start_time: i64,
    end_time: i64,
    limit: Option<usize>,
    dry_run: Option<bool>,
}

#[derive(Debug, Serialize)]
struct MetricsReconciliationPreviewResponse {
    summary: MetricsReconciliationSummary,
}

async fn metrics_ingest_status(
    State(app_state): State<Arc<AppState>>,
    Query(params): Query<MetricsIngestStatusParams>,
) -> Result<HttpResult<MetricsIngestStatus>, BaseError> {
    let mut status = app_state.metrics.ingest_status()?;
    if let (Some(start_time), Some(end_time)) = (params.start_time, params.end_time) {
        status.pending_reconciliation_count = Some(
            app_state
                .metrics
                .count_pending_reconciliation(start_time, end_time)?,
        );
    }
    Ok(HttpResult::new(status))
}

async fn preview_metrics_reconciliation(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<MetricsReconciliationRequest>,
) -> Result<HttpResult<MetricsReconciliationPreviewResponse>, BaseError> {
    let params = reconciliation_params(payload, true)?;
    let summary = app_state.metrics.reconcile_request_logs(params)?;
    Ok(HttpResult::new(MetricsReconciliationPreviewResponse {
        summary,
    }))
}

async fn run_metrics_reconciliation(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<MetricsReconciliationRequest>,
) -> Result<HttpResult<MetricsReconciliationSummary>, BaseError> {
    let dry_run = payload.dry_run.unwrap_or(false);
    let params = reconciliation_params(payload, dry_run)?;
    Ok(HttpResult::new(
        app_state.metrics.reconcile_request_logs(params)?,
    ))
}

async fn repair_metrics_reconciliation(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<MetricsRepairRequest>,
) -> Result<HttpResult<MetricsRepairSummary>, BaseError> {
    let params = repair_params(payload)?;
    Ok(HttpResult::new(
        app_state.metrics.repair_request_logs(params)?,
    ))
}

fn reconciliation_params(
    payload: MetricsReconciliationRequest,
    dry_run: bool,
) -> Result<MetricsReconciliationParams, BaseError> {
    if payload.start_time >= payload.end_time {
        return Err(BaseError::ParamInvalid(Some(
            "start_time must be before end_time".to_string(),
        )));
    }
    if payload.end_time > Utc::now().timestamp_millis() + 60_000 {
        return Err(BaseError::ParamInvalid(Some(
            "end_time cannot be more than 60 seconds in the future".to_string(),
        )));
    }

    Ok(MetricsReconciliationParams {
        start_time: payload.start_time,
        end_time: payload.end_time,
        limit: payload.limit.unwrap_or(500),
        dry_run,
    })
}

fn repair_params(payload: MetricsRepairRequest) -> Result<MetricsRepairParams, BaseError> {
    if payload.start_time >= payload.end_time {
        return Err(BaseError::ParamInvalid(Some(
            "start_time must be before end_time".to_string(),
        )));
    }
    if payload.end_time > Utc::now().timestamp_millis() + 60_000 {
        return Err(BaseError::ParamInvalid(Some(
            "end_time cannot be more than 60 seconds in the future".to_string(),
        )));
    }

    Ok(MetricsRepairParams {
        start_time: payload.start_time,
        end_time: payload.end_time,
        limit: payload.limit.unwrap_or(500),
        dry_run: payload.dry_run.unwrap_or(true),
    })
}

pub fn create_metrics_router() -> StateRouter {
    create_state_router().nest(
        "/metrics",
        create_state_router()
            .route("/ingest/status", get(metrics_ingest_status))
            .route(
                "/reconciliation/preview",
                post(preview_metrics_reconciliation),
            )
            .route("/reconciliation/run", post(run_metrics_reconciliation))
            .route(
                "/reconciliation/repair",
                post(repair_metrics_reconciliation),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MetricsReconciliationRequest, MetricsRepairRequest, reconciliation_params, repair_params,
    };
    use crate::controller::BaseError;

    #[test]
    fn reconciliation_params_require_explicit_range() {
        let err = reconciliation_params(
            MetricsReconciliationRequest {
                start_time: 2,
                end_time: 1,
                limit: Some(10),
                dry_run: None,
            },
            true,
        )
        .expect_err("range should be rejected");

        match err {
            BaseError::ParamInvalid(Some(message)) => {
                assert!(message.contains("start_time must be before end_time"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn reconciliation_preview_forces_dry_run() {
        let params = reconciliation_params(
            MetricsReconciliationRequest {
                start_time: 1,
                end_time: 2,
                limit: None,
                dry_run: Some(false),
            },
            true,
        )
        .expect("params should build");

        assert!(params.dry_run);
        assert_eq!(params.limit, 500);
    }

    #[test]
    fn repair_defaults_to_dry_run() {
        let params = repair_params(MetricsRepairRequest {
            start_time: 1,
            end_time: 2,
            limit: Some(10),
            dry_run: None,
        })
        .expect("repair params should build");

        assert!(params.dry_run);
        assert_eq!(params.limit, 10);
    }
}
