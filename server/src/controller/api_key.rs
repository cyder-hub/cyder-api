use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post, put},
};
use cyder_tools::log::warn;
use serde::{Deserialize, Serialize};

use crate::{
    database::api_key::{
        ApiKey, ApiKeyDetail, ApiKeyReveal, ApiKeySummary, CreateApiKeyPayload,
        UpdateApiKeyMetadataPayload, hash_api_key,
    },
    database::model_route::{ApiKeyModelOverride, CreateApiKeyModelOverridePayload, ModelRoute},
    service::app_state::{AppState, StateRouter, create_state_router},
    service::runtime::{ApiKeyBilledAmountSnapshot, ApiKeyGovernanceSnapshot},
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiKeyModelOverridePayload {
    pub source_name: String,
    pub target_route_id: i64,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct ApiKeyModelOverrideResponse {
    pub id: i64,
    pub source_name: String,
    pub target_route_id: i64,
    pub target_route_name: Option<String>,
    pub description: Option<String>,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ApiKeyDetailResponse {
    #[serde(flatten)]
    pub detail: ApiKeyDetail,
    pub model_overrides: Vec<ApiKeyModelOverrideResponse>,
}

#[derive(Debug, Clone, Serialize)]
struct ApiKeyDetailWithSecretResponse {
    pub detail: ApiKeyDetailResponse,
    pub reveal: ApiKeyReveal,
}

#[derive(Debug, Clone, Deserialize)]
struct CreateApiKeyRequest {
    #[serde(flatten)]
    pub detail: CreateApiKeyPayload,
    #[serde(default)]
    pub model_overrides: Vec<ApiKeyModelOverridePayload>,
}

#[derive(Debug, Clone, Deserialize)]
struct UpdateApiKeyRequest {
    #[serde(flatten)]
    pub detail: UpdateApiKeyMetadataPayload,
    #[serde(default)]
    pub model_overrides: Vec<ApiKeyModelOverridePayload>,
}

fn log_api_key_audit(
    action: &'static str,
    api_key_id: i64,
    api_key_name: &str,
    is_enabled: Option<bool>,
) {
    match action {
        "create" => crate::info_event!(
            "manager.api_key_created",
            action = action,
            api_key_id = api_key_id,
            api_key_name = api_key_name,
            is_enabled = is_enabled,
        ),
        "update" => crate::info_event!(
            "manager.api_key_updated",
            action = action,
            api_key_id = api_key_id,
            api_key_name = api_key_name,
            is_enabled = is_enabled,
        ),
        "rotate" => crate::info_event!(
            "manager.api_key_rotated",
            action = action,
            api_key_id = api_key_id,
            api_key_name = api_key_name,
            is_enabled = is_enabled,
        ),
        "reveal" => crate::info_event!(
            "manager.api_key_revealed",
            action = action,
            api_key_id = api_key_id,
            api_key_name = api_key_name,
            is_enabled = is_enabled,
        ),
        "delete" => crate::info_event!(
            "manager.api_key_deleted",
            action = action,
            api_key_id = api_key_id,
            api_key_name = api_key_name,
            is_enabled = is_enabled,
        ),
        _ => unreachable!("unsupported api key audit action: {action}"),
    }
}

fn log_api_key_override_replace_audit(
    api_key_id: i64,
    api_key_name: &str,
    overrides: &[ApiKeyModelOverrideResponse],
) {
    crate::info_event!(
        "manager.api_key_model_overrides_replaced",
        action = "replace",
        api_key_id = api_key_id,
        api_key_name = api_key_name,
        override_count = overrides.len(),
        enabled_override_count = overrides.iter().filter(|item| item.is_enabled).count(),
    );
}

fn load_api_key_model_override_responses(
    api_key_id: i64,
) -> Result<Vec<ApiKeyModelOverrideResponse>, BaseError> {
    ApiKeyModelOverride::list_by_api_key_id(api_key_id)?
        .into_iter()
        .map(|override_row| {
            let target_route_name = ModelRoute::get_by_id(override_row.target_route_id)
                .ok()
                .map(|route| route.route_name);

            Ok(ApiKeyModelOverrideResponse {
                id: override_row.id,
                source_name: override_row.source_name,
                target_route_id: override_row.target_route_id,
                target_route_name,
                description: override_row.description,
                is_enabled: override_row.is_enabled,
            })
        })
        .collect()
}

fn load_api_key_detail_response(api_key_id: i64) -> Result<ApiKeyDetailResponse, BaseError> {
    Ok(ApiKeyDetailResponse {
        detail: ApiKey::get_detail(api_key_id)?,
        model_overrides: load_api_key_model_override_responses(api_key_id)?,
    })
}

async fn replace_api_key_model_overrides(
    app_state: &Arc<AppState>,
    api_key_id: i64,
    payloads: &[ApiKeyModelOverridePayload],
) -> Result<Vec<ApiKeyModelOverrideResponse>, BaseError> {
    let existing = ApiKeyModelOverride::list_by_api_key_id(api_key_id)?;
    for override_row in existing {
        let source_name = override_row.source_name.clone();
        ApiKeyModelOverride::delete(override_row.id)?;
        if let Err(err) = app_state
            .catalog
            .invalidate_api_key_model_override(api_key_id, &source_name)
            .await
        {
            warn!(
                "Failed to invalidate deleted api key model override {}:{}: {:?}",
                api_key_id, source_name, err
            );
        }
    }

    for payload in payloads {
        ApiKeyModelOverride::create(&CreateApiKeyModelOverridePayload {
            api_key_id,
            source_name: payload.source_name.clone(),
            target_route_id: payload.target_route_id,
            description: payload.description.clone(),
            is_enabled: payload.is_enabled,
        })?;

        if let Err(err) = app_state
            .catalog
            .invalidate_api_key_model_override(api_key_id, &payload.source_name)
            .await
        {
            warn!(
                "Failed to invalidate created api key model override {}:{}: {:?}",
                api_key_id, payload.source_name, err
            );
        }
    }

    if let Err(err) = app_state.catalog.invalidate_models_catalog().await {
        warn!(
            "Failed to invalidate models catalog after api key override replace {}: {:?}",
            api_key_id, err
        );
    }

    load_api_key_model_override_responses(api_key_id)
}

async fn create_api_key(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateApiKeyRequest>,
) -> Result<HttpResult<ApiKeyDetailWithSecretResponse>, BaseError> {
    let created = ApiKey::create(&payload.detail)?;
    let overrides =
        replace_api_key_model_overrides(&app_state, created.detail.id, &payload.model_overrides)
            .await?;

    log_api_key_audit(
        "create",
        created.detail.id,
        &created.detail.name,
        Some(created.detail.is_enabled),
    );
    log_api_key_override_replace_audit(created.detail.id, &created.detail.name, &overrides);

    Ok(HttpResult::new(ApiKeyDetailWithSecretResponse {
        detail: ApiKeyDetailResponse {
            detail: created.detail,
            model_overrides: overrides,
        },
        reveal: created.reveal,
    }))
}

async fn list_api_keys() -> Result<HttpResult<Vec<ApiKeySummary>>, BaseError> {
    Ok(HttpResult::new(ApiKey::list_summary()?))
}

async fn get_api_key_detail(
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyDetailResponse>, BaseError> {
    Ok(HttpResult::new(load_api_key_detail_response(id)?))
}

async fn update_api_key(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateApiKeyRequest>,
) -> Result<HttpResult<ApiKeyDetailResponse>, BaseError> {
    let updated = ApiKey::update_metadata(id, &payload.detail)?;
    let overrides =
        replace_api_key_model_overrides(&app_state, id, &payload.model_overrides).await?;

    if let Err(err) = app_state.catalog.invalidate_api_key_id(id).await {
        warn!(
            "Failed to invalidate api key {} after update: {:?}",
            id, err
        );
    }

    log_api_key_audit(
        "update",
        updated.id,
        &updated.name,
        Some(updated.is_enabled),
    );
    log_api_key_override_replace_audit(updated.id, &updated.name, &overrides);

    Ok(HttpResult::new(ApiKeyDetailResponse {
        detail: updated,
        model_overrides: overrides,
    }))
}

async fn rotate_api_key(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyReveal>, BaseError> {
    let existing = ApiKey::get_by_id(id)?;
    let old_hash = hash_api_key(&existing.api_key);
    let rotated = ApiKey::rotate_key(id)?;

    if let Err(err) = app_state.catalog.invalidate_api_key_hash(&old_hash).await {
        warn!(
            "Failed to invalidate old api key hash for rotated key {}: {:?}",
            id, err
        );
    }
    if let Err(err) = app_state.catalog.invalidate_api_key_id(id).await {
        warn!("Failed to invalidate rotated api key {}: {:?}", id, err);
    }

    log_api_key_audit(
        "rotate",
        rotated.id,
        &rotated.name,
        Some(existing.is_enabled),
    );

    Ok(HttpResult::new(rotated))
}

async fn reveal_api_key(Path(id): Path<i64>) -> Result<HttpResult<ApiKeyReveal>, BaseError> {
    let existing = ApiKey::get_by_id(id)?;
    let revealed = ApiKey::reveal_key(id)?;
    log_api_key_audit(
        "reveal",
        revealed.id,
        &revealed.name,
        Some(existing.is_enabled),
    );
    Ok(HttpResult::new(revealed))
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
    let overrides = ApiKeyModelOverride::list_by_api_key_id(id)?;
    for override_row in overrides {
        let source_name = override_row.source_name.clone();
        ApiKeyModelOverride::delete(override_row.id)?;
        if let Err(err) = app_state
            .catalog
            .invalidate_api_key_model_override(id, &source_name)
            .await
        {
            warn!(
                "Failed to invalidate deleted api key override {}:{}: {:?}",
                id, source_name, err
            );
        }
    }

    if let Err(err) = app_state
        .catalog
        .invalidate_api_key_hash(&api_key_hash)
        .await
    {
        warn!("Failed to invalidate deleted api key {}: {:?}", id, err);
    }

    log_api_key_audit(
        "delete",
        existing.id,
        &existing.name,
        Some(existing.is_enabled),
    );

    Ok(HttpResult::new(()))
}

async fn get_api_key_runtime_snapshot(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyRuntimeSnapshotResponse>, BaseError> {
    // Validate key existence so deleted/nonexistent IDs return 404 instead of an empty snapshot.
    ApiKey::get_by_id(id)?;
    let snapshot = app_state
        .api_key_governance
        .get_api_key_governance_snapshot(id)?;
    Ok(HttpResult::new(snapshot.into()))
}

async fn list_api_key_runtime_snapshots(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<Vec<ApiKeyRuntimeSnapshotResponse>>, BaseError> {
    let snapshots = app_state
        .api_key_governance
        .list_api_key_governance_snapshots()?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(HttpResult::new(snapshots))
}

async fn list_api_key_model_overrides(
    Path(id): Path<i64>,
) -> Result<HttpResult<Vec<ApiKeyModelOverrideResponse>>, BaseError> {
    ApiKey::get_by_id(id)?;
    Ok(HttpResult::new(load_api_key_model_override_responses(id)?))
}

async fn replace_api_key_model_override_routes(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<Vec<ApiKeyModelOverridePayload>>,
) -> Result<HttpResult<Vec<ApiKeyModelOverrideResponse>>, BaseError> {
    let api_key = ApiKey::get_by_id(id)?;
    let overrides = replace_api_key_model_overrides(&app_state, id, &payload).await?;
    log_api_key_override_replace_audit(id, &api_key.name, &overrides);
    Ok(HttpResult::new(overrides))
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
            .route(
                "/{id}/model_override/list",
                get(list_api_key_model_overrides),
            )
            .route(
                "/{id}/model_override/replace",
                put(replace_api_key_model_override_routes),
            )
            .route("/{id}/runtime", get(get_api_key_runtime_snapshot)),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ApiKeyDetailResponse, ApiKeyModelOverrideResponse, create_api_key_management_router,
    };
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

    #[test]
    fn detail_shape_includes_model_overrides_at_top_level() {
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
        let response = ApiKeyDetailResponse {
            detail,
            model_overrides: vec![ApiKeyModelOverrideResponse {
                id: 7,
                source_name: "manual-cli-model".to_string(),
                target_route_id: 3,
                target_route_name: Some("manual-smoke-route".to_string()),
                description: Some("cli shim".to_string()),
                is_enabled: true,
            }],
        };

        let json = serde_json::to_value(response).expect("serialize response");

        assert_eq!(json.get("name"), Some(&json!("detail")));
        assert_eq!(
            json.pointer("/model_overrides/0/source_name"),
            Some(&json!("manual-cli-model"))
        );
    }
}
