use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post, put},
};
use cyder_tools::log::warn;
use serde::Serialize;

use crate::{
    database::api_key::{
        ApiKey, ApiKeyDetail, ApiKeyDetailWithSecret, ApiKeyReveal, ApiKeySummary,
        CreateApiKeyPayload, UpdateApiKeyMetadataPayload, hash_api_key,
    },
    service::app_state::{ApiKeyBilledAmountSnapshot, ApiKeyGovernanceSnapshot, AppState},
    service::app_state::{StateRouter, create_state_router},
    utils::HttpResult,
};

use super::BaseError;

#[derive(Debug, Clone, Serialize)]
struct ApiKeyBilledAmountSnapshotResponse {
    pub currency: String,
    pub amount_nanos: i64,
}

#[derive(Debug, Clone, Serialize)]
struct ApiKeyRuntimeSnapshotResponse {
    pub api_key_id: i64,
    pub current_concurrency: u32,
    pub current_minute_bucket: Option<i64>,
    pub current_minute_request_count: u32,
    pub day_bucket: Option<i64>,
    pub daily_request_count: i64,
    pub daily_token_count: i64,
    pub month_bucket: Option<i64>,
    pub monthly_token_count: i64,
    pub daily_billed_amounts: Vec<ApiKeyBilledAmountSnapshotResponse>,
    pub monthly_billed_amounts: Vec<ApiKeyBilledAmountSnapshotResponse>,
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
    Json(payload): Json<CreateApiKeyPayload>,
) -> Result<HttpResult<ApiKeyDetailWithSecret>, BaseError> {
    Ok(HttpResult::new(ApiKey::create(&payload)?))
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
    let updated = ApiKey::update_metadata(id, &payload)?;

    if let Err(err) = app_state.invalidate_api_key_id(id).await {
        warn!(
            "Failed to invalidate api key {} after update: {:?}",
            id, err
        );
    }

    Ok(HttpResult::new(updated))
}

async fn rotate_api_key(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyReveal>, BaseError> {
    let existing = ApiKey::get_by_id(id)?;
    let old_hash = hash_api_key(&existing.api_key);
    let rotated = ApiKey::rotate_key(id)?;

    if let Err(err) = app_state.invalidate_api_key_hash(&old_hash).await {
        warn!(
            "Failed to invalidate old api key hash for rotated key {}: {:?}",
            id, err
        );
    }
    if let Err(err) = app_state.invalidate_api_key_id(id).await {
        warn!("Failed to invalidate rotated api key {}: {:?}", id, err);
    }

    Ok(HttpResult::new(rotated))
}

async fn reveal_api_key(Path(id): Path<i64>) -> Result<HttpResult<ApiKeyReveal>, BaseError> {
    Ok(HttpResult::new(ApiKey::reveal_key(id)?))
}

async fn delete_api_key(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    let existing = ApiKey::get_by_id(id)?;
    let api_key_hash = existing
        .api_key_hash
        .clone()
        .unwrap_or_else(|| hash_api_key(&existing.api_key));

    ApiKey::delete(id)?;

    if let Err(err) = app_state.invalidate_api_key_hash(&api_key_hash).await {
        warn!("Failed to invalidate deleted api key {}: {:?}", id, err);
    }

    Ok(HttpResult::new(()))
}

async fn get_api_key_runtime_snapshot(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyRuntimeSnapshotResponse>, BaseError> {
    // Validate key existence so deleted/nonexistent IDs return 404 instead of an empty snapshot.
    ApiKey::get_by_id(id)?;
    let snapshot = app_state.get_api_key_governance_snapshot(id)?;
    Ok(HttpResult::new(snapshot.into()))
}

async fn list_api_key_runtime_snapshots(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<Vec<ApiKeyRuntimeSnapshotResponse>>, BaseError> {
    let snapshots = app_state
        .list_api_key_governance_snapshots()?
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

#[cfg(test)]
mod tests {
    use super::create_api_key_management_router;
    use crate::database::api_key::{ApiKeyDetail, ApiKeyReveal, ApiKeySummary};
    use crate::schema::enum_def::Action;
    use serde_json::json;

    #[test]
    fn create_api_key_management_router_registers_routes() {
        let _router = create_api_key_management_router();
    }

    #[test]
    fn list_and_detail_shapes_do_not_include_secret_but_reveal_does() {
        let summary = ApiKeySummary {
            id: 1,
            key_prefix: "cyder-abcdef".to_string(),
            key_last4: "1234".to_string(),
            name: "summary".to_string(),
            description: None,
            default_action: Action::Allow,
            is_enabled: true,
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: Some(10),
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            created_at: 1,
            updated_at: 2,
        };
        let detail = ApiKeyDetail {
            id: 1,
            key_prefix: "cyder-abcdef".to_string(),
            key_last4: "1234".to_string(),
            name: "detail".to_string(),
            description: None,
            default_action: Action::Allow,
            is_enabled: true,
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: Some(10),
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            created_at: 1,
            updated_at: 2,
            acl_rules: vec![],
        };
        let reveal = ApiKeyReveal {
            id: 1,
            name: "reveal".to_string(),
            key_prefix: "cyder-abcdef".to_string(),
            key_last4: "1234".to_string(),
            api_key: "cyder-secret".to_string(),
            updated_at: 2,
        };

        let summary_json = serde_json::to_value(summary).expect("serialize summary");
        let detail_json = serde_json::to_value(detail).expect("serialize detail");
        let reveal_json = serde_json::to_value(reveal).expect("serialize reveal");

        assert_eq!(summary_json.get("api_key"), None);
        assert_eq!(detail_json.get("api_key"), None);
        assert_eq!(reveal_json.get("api_key"), Some(&json!("cyder-secret")));
    }
}
