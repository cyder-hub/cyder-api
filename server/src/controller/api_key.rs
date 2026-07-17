use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Serialize;

use crate::{
    database::api_key::{
        ApiKey, ApiKeyDetail, ApiKeyDetailWithSecret, ApiKeyReveal, ApiKeySummary,
        CreateApiKeyPayload, UpdateApiKeyMetadataPayload,
    },
    service::app_state::{AppState, StateRouter, create_state_router},
    service::runtime::{ApiKeyBilledAmountSnapshot, ApiKeyGovernanceSnapshot},
    utils::HttpResult,
};

use super::BaseError;

#[derive(Debug, Clone, Serialize)]
struct ApiKeyBilledAmountSnapshotResponse {
    currency: String,
    amount_nanos: i64,
}

#[derive(Debug, Clone, Serialize)]
struct ApiKeyRuntimeSnapshotResponse {
    api_key_id: i64,
    current_concurrency: u32,
    current_minute_bucket: Option<i64>,
    current_minute_request_count: u32,
    day_bucket: Option<i64>,
    daily_request_count: i64,
    daily_token_count: i64,
    month_bucket: Option<i64>,
    monthly_token_count: i64,
    daily_billed_amounts: Vec<ApiKeyBilledAmountSnapshotResponse>,
    monthly_billed_amounts: Vec<ApiKeyBilledAmountSnapshotResponse>,
}

impl From<ApiKeyBilledAmountSnapshot> for ApiKeyBilledAmountSnapshotResponse {
    fn from(value: ApiKeyBilledAmountSnapshot) -> Self {
        Self {
            currency: value.currency,
            amount_nanos: value.amount_nanos,
        }
    }
}

impl From<ApiKeyGovernanceSnapshot> for ApiKeyRuntimeSnapshotResponse {
    fn from(value: ApiKeyGovernanceSnapshot) -> Self {
        Self {
            api_key_id: value.api_key_id,
            current_concurrency: value.current_concurrency,
            current_minute_bucket: value.current_minute_bucket,
            current_minute_request_count: value.current_minute_request_count,
            day_bucket: value.day_bucket,
            daily_request_count: value.daily_request_count,
            daily_token_count: value.daily_token_count,
            month_bucket: value.month_bucket,
            monthly_token_count: value.monthly_token_count,
            daily_billed_amounts: value
                .daily_billed_amounts
                .into_iter()
                .map(Into::into)
                .collect(),
            monthly_billed_amounts: value
                .monthly_billed_amounts
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

async fn create_api_key(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateApiKeyPayload>,
) -> Result<HttpResult<ApiKeyDetailWithSecret>, BaseError> {
    let created = app_state.admin.api_key.create_api_key(payload).await?;
    Ok(HttpResult::new(created))
}

async fn list_api_keys() -> Result<HttpResult<Vec<ApiKeySummary>>, BaseError> {
    Ok(HttpResult::new(ApiKey::list_summary()?))
}

async fn get_api_key_detail(Path(id): Path<i64>) -> Result<HttpResult<ApiKeyDetail>, BaseError> {
    Ok(HttpResult::new(ApiKey::get_detail(id)?))
}

async fn update_api_key(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateApiKeyMetadataPayload>,
) -> Result<HttpResult<ApiKeyDetail>, BaseError> {
    Ok(HttpResult::new(
        app_state.admin.api_key.update_api_key(id, payload).await?,
    ))
}

async fn rotate_api_key(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyReveal>, BaseError> {
    Ok(HttpResult::new(
        app_state.admin.api_key.rotate_api_key(id).await?,
    ))
}

async fn reveal_api_key(Path(id): Path<i64>) -> Result<HttpResult<ApiKeyReveal>, BaseError> {
    let existing = ApiKey::get_by_id(id)?;
    let revealed = ApiKey::reveal_key(id)?;
    crate::info_event!(
        "manager.api_key_revealed",
        action = "reveal",
        api_key_id = revealed.id,
        api_key_name = &revealed.name,
        is_enabled = existing.is_enabled,
    );
    Ok(HttpResult::new(revealed))
}

async fn delete_api_key(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    app_state.admin.api_key.delete_api_key(id).await?;
    Ok(HttpResult::new(()))
}

async fn get_api_key_runtime_snapshot(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyRuntimeSnapshotResponse>, BaseError> {
    ApiKey::get_by_id(id)?;
    let snapshot = app_state
        .api_key_governance
        .get_api_key_governance_snapshot(id)
        .await?;
    Ok(HttpResult::new(snapshot.into()))
}

async fn list_api_key_runtime_snapshots(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<Vec<ApiKeyRuntimeSnapshotResponse>>, BaseError> {
    let snapshots = app_state
        .api_key_governance
        .list_api_key_governance_snapshots()
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(HttpResult::new(snapshots))
}

pub fn create_api_key_management_router() -> StateRouter {
    create_state_router().nest(
        "/api_key",
        create_state_router()
            .route("/", post(create_api_key))
            .route("/list", get(list_api_keys))
            .route("/runtime/list", get(list_api_key_runtime_snapshots))
            .route(
                "/{id}",
                get(get_api_key_detail)
                    .put(update_api_key)
                    .delete(delete_api_key),
            )
            .route("/{id}/rotate", post(rotate_api_key))
            .route("/{id}/reveal", get(reveal_api_key))
            .route("/{id}/runtime", get(get_api_key_runtime_snapshot)),
    )
}
