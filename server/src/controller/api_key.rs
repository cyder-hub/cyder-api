use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::{
    database::api_key::{
        ApiKey, ApiKeyDetail, ApiKeyReveal, ApiKeySummary, CreateApiKeyPayload,
        UpdateApiKeyMetadataPayload,
    },
    database::model_route::{ApiKeyModelOverride, ModelRoute},
    service::admin::api_key::ApiKeyModelOverrideInput,
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

fn log_api_key_reveal_audit(api_key_id: i64, api_key_name: &str, is_enabled: Option<bool>) {
    crate::info_event!(
        "manager.api_key_revealed",
        action = "reveal",
        api_key_id = api_key_id,
        api_key_name = api_key_name,
        is_enabled = is_enabled,
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

fn map_api_key_model_override_inputs(
    payloads: Vec<ApiKeyModelOverridePayload>,
) -> Vec<ApiKeyModelOverrideInput> {
    payloads
        .into_iter()
        .map(|payload| ApiKeyModelOverrideInput {
            source_name: payload.source_name,
            target_route_id: payload.target_route_id,
            description: payload.description,
            is_enabled: payload.is_enabled,
        })
        .collect()
}

async fn create_api_key(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateApiKeyRequest>,
) -> Result<HttpResult<ApiKeyDetailWithSecretResponse>, BaseError> {
    let created = app_state
        .admin
        .api_key
        .create_api_key(
            payload.detail,
            map_api_key_model_override_inputs(payload.model_overrides),
        )
        .await?;
    let overrides = load_api_key_model_override_responses(created.detail.id)?;

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
    let updated = app_state
        .admin
        .api_key
        .update_api_key(
            id,
            payload.detail,
            map_api_key_model_override_inputs(payload.model_overrides),
        )
        .await?;
    let overrides = load_api_key_model_override_responses(id)?;

    Ok(HttpResult::new(ApiKeyDetailResponse {
        detail: updated,
        model_overrides: overrides,
    }))
}

async fn rotate_api_key(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyReveal>, BaseError> {
    let rotated = app_state.admin.api_key.rotate_api_key(id).await?;

    Ok(HttpResult::new(rotated))
}

async fn reveal_api_key(Path(id): Path<i64>) -> Result<HttpResult<ApiKeyReveal>, BaseError> {
    let existing = ApiKey::get_by_id(id)?;
    let revealed = ApiKey::reveal_key(id)?;
    log_api_key_reveal_audit(revealed.id, &revealed.name, Some(existing.is_enabled));
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
    app_state
        .admin
        .api_key
        .replace_api_key_model_overrides(id, map_api_key_model_override_inputs(payload))
        .await?;
    let overrides = load_api_key_model_override_responses(id)?;
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
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode, header::CONTENT_TYPE},
    };
    use serde_json::{Value, json};
    use tower::util::ServiceExt;

    use super::{
        ApiKeyDetailResponse, ApiKeyModelOverrideResponse, create_api_key_management_router,
    };
    use crate::database::TestDbContext;
    use crate::database::api_key::{ApiKey, ApiKeyDetail, ApiKeyReveal, ApiKeySummary};
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::model_route::{
        ApiKeyModelOverride, CreateModelRoutePayload, ModelRoute, ModelRouteCandidateInput,
    };
    use crate::database::provider::{NewProvider, Provider};
    use crate::schema::enum_def::{Action, ProviderApiKeyMode, ProviderType};
    use crate::service::app_state::{AppState, create_test_app_state};

    fn seed_provider(id: i64, provider_key: &str) -> Provider {
        Provider::create(&NewProvider {
            id,
            provider_key: provider_key.to_string(),
            name: provider_key.to_string(),
            endpoint: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        })
        .expect("provider seed should succeed")
    }

    fn seed_model_for_provider(provider_id: i64, model_name: &str) -> Model {
        Model::create(
            provider_id,
            model_name,
            None,
            true,
            ModelCapabilityFlags {
                supports_streaming: true,
                supports_tools: true,
                supports_reasoning: true,
                supports_image_input: true,
                supports_embeddings: true,
                supports_rerank: true,
            },
        )
        .expect("model seed should succeed")
    }

    fn seed_route(route_name: &str, model_id: i64) -> ModelRoute {
        ModelRoute::create(&CreateModelRoutePayload {
            route_name: route_name.to_string(),
            description: Some("seed route".to_string()),
            is_enabled: Some(true),
            expose_in_models: Some(true),
            candidates: vec![ModelRouteCandidateInput {
                model_id,
                priority: 0,
                is_enabled: Some(true),
            }],
        })
        .expect("route seed should succeed")
        .route
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_api_key_management_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("api key router should respond")
    }

    fn empty_request(method: Method, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request should build")
    }

    fn json_request(method: Method, uri: &str, payload: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&payload).expect("payload should serialize"),
            ))
            .expect("request should build")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

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

    #[tokio::test]
    async fn api_key_http_write_paths_update_response_database_and_overrides() {
        let test_db_context = TestDbContext::new_sqlite("controller-api-key-write-http.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(40101, "api-key-http-provider");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let primary_route = seed_route("api-key-http-primary", model.id);
                let replacement_route = seed_route("api-key-http-replacement", model.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let create_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/api_key",
                        json!({
                            "name": "operator-key",
                            "description": "created through HTTP",
                            "default_action": "ALLOW",
                            "is_enabled": true,
                            "model_overrides": [
                                {
                                    "source_name": "operator-cli-model",
                                    "target_route_id": primary_route.id,
                                    "description": "initial override",
                                    "is_enabled": true
                                }
                            ]
                        }),
                    ),
                )
                .await;
                assert_eq!(create_response.status(), StatusCode::OK);
                let create_body = response_json(create_response).await;
                assert_eq!(create_body["code"], 0);
                assert_eq!(create_body["data"]["detail"]["name"], "operator-key");
                assert_eq!(
                    create_body["data"]["detail"]["model_overrides"][0]["source_name"],
                    "operator-cli-model"
                );
                assert_eq!(
                    create_body["data"]["detail"]["model_overrides"][0]["target_route_id"],
                    primary_route.id
                );

                let api_key_id = create_body["data"]["detail"]["id"]
                    .as_i64()
                    .expect("api key id should be returned");
                let original_secret = create_body["data"]["reveal"]["api_key"]
                    .as_str()
                    .expect("created secret should be returned")
                    .to_string();
                let api_key = ApiKey::get_by_id(api_key_id).expect("api key should persist");
                assert_eq!(api_key.name, "operator-key");

                let overrides = ApiKeyModelOverride::list_by_api_key_id(api_key_id)
                    .expect("created overrides should load");
                assert_eq!(overrides.len(), 1);
                assert_eq!(overrides[0].source_name, "operator-cli-model");
                assert_eq!(overrides[0].target_route_id, primary_route.id);

                let replace_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/api_key/{api_key_id}/model_override/replace"),
                        json!([
                            {
                                "source_name": "operator-cli-model",
                                "target_route_id": replacement_route.id,
                                "description": "replacement override",
                                "is_enabled": true
                            }
                        ]),
                    ),
                )
                .await;
                assert_eq!(replace_response.status(), StatusCode::OK);
                let replace_body = response_json(replace_response).await;
                assert_eq!(replace_body["code"], 0);
                assert_eq!(replace_body["data"][0]["source_name"], "operator-cli-model");
                assert_eq!(
                    replace_body["data"][0]["target_route_id"],
                    replacement_route.id
                );
                assert_eq!(
                    replace_body["data"][0]["target_route_name"],
                    "api-key-http-replacement"
                );

                let overrides_after_replace = ApiKeyModelOverride::list_by_api_key_id(api_key_id)
                    .expect("replacement overrides should load");
                assert_eq!(overrides_after_replace.len(), 1);
                assert_eq!(
                    overrides_after_replace[0].target_route_id,
                    replacement_route.id
                );
                assert_eq!(
                    overrides_after_replace[0].description.as_deref(),
                    Some("replacement override")
                );

                let rotate_response = send(
                    &app_state,
                    empty_request(Method::POST, &format!("/api_key/{api_key_id}/rotate")),
                )
                .await;
                assert_eq!(rotate_response.status(), StatusCode::OK);
                let rotate_body = response_json(rotate_response).await;
                assert_eq!(rotate_body["code"], 0);
                assert_eq!(rotate_body["data"]["id"], api_key_id);
                let rotated_secret = rotate_body["data"]["api_key"]
                    .as_str()
                    .expect("rotated secret should be returned");
                assert_ne!(rotated_secret, original_secret);
                let rotated_api_key =
                    ApiKey::get_by_id(api_key_id).expect("rotated api key should load");
                assert_eq!(rotated_api_key.api_key, rotated_secret);

                let delete_response = send(
                    &app_state,
                    empty_request(Method::DELETE, &format!("/api_key/{api_key_id}")),
                )
                .await;
                assert_eq!(delete_response.status(), StatusCode::OK);
                let delete_body = response_json(delete_response).await;
                assert_eq!(delete_body["code"], 0);
                assert!(delete_body["data"].is_null());

                assert!(ApiKey::get_by_id(api_key_id).is_err());
                let overrides_after_delete = ApiKeyModelOverride::list_by_api_key_id(api_key_id)
                    .expect("deleted api key overrides should list");
                assert!(overrides_after_delete.is_empty());
            })
            .await;
    }
}
