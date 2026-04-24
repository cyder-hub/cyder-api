use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post},
};
use std::sync::Arc;

use crate::{
    controller::BaseError,
    database::model_route::{
        CreateModelRoutePayload, ModelRoute, ModelRouteDetail, ModelRouteListItem,
        UpdateModelRoutePayload,
    },
    service::app_state::{AppState, StateRouter, create_state_router},
    utils::HttpResult,
};

async fn create_model_route(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateModelRoutePayload>,
) -> Result<HttpResult<ModelRouteDetail>, BaseError> {
    let detail = app_state
        .admin
        .model_route
        .create_model_route(payload)
        .await?;
    Ok(HttpResult::new(detail))
}

async fn list_model_routes() -> Result<HttpResult<Vec<ModelRouteListItem>>, BaseError> {
    Ok(HttpResult::new(ModelRoute::list_summary()?))
}

async fn get_model_route(Path(id): Path<i64>) -> Result<HttpResult<ModelRouteDetail>, BaseError> {
    Ok(HttpResult::new(ModelRoute::get_detail(id)?))
}

async fn update_model_route(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateModelRoutePayload>,
) -> Result<HttpResult<ModelRouteDetail>, BaseError> {
    let detail = app_state
        .admin
        .model_route
        .update_model_route(id, payload)
        .await?;
    Ok(HttpResult::new(detail))
}

async fn delete_model_route(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    app_state.admin.model_route.delete_model_route(id).await?;
    Ok(HttpResult::new(()))
}

pub fn create_model_route_router() -> StateRouter {
    create_state_router()
        .route("/model_route", post(create_model_route))
        .nest(
            "/model_route",
            create_state_router()
                .route("/list", get(list_model_routes))
                .route(
                    "/{id}",
                    get(get_model_route)
                        .put(update_model_route)
                        .delete(delete_model_route),
                ),
        )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode},
    };
    use serde_json::Value;
    use tower::util::ServiceExt;

    use crate::database::TestDbContext;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload};
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::model_route::{
        ApiKeyModelOverride, CreateApiKeyModelOverridePayload, CreateModelRoutePayload, ModelRoute,
        ModelRouteCandidateInput,
    };
    use crate::database::provider::{NewProvider, Provider};
    use crate::schema::enum_def::{Action, ProviderApiKeyMode, ProviderType};
    use crate::service::app_state::{AppState, create_test_app_state};

    use super::create_model_route_router;

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

    fn seed_api_key() -> crate::database::api_key::ApiKeyDetailWithSecret {
        ApiKey::create(&CreateApiKeyPayload {
            name: "route-delete".to_string(),
            description: Some("seed".to_string()),
            default_action: Some(Action::Allow),
            is_enabled: Some(true),
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            acl_rules: None,
        })
        .expect("api key seed should succeed")
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_model_route_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("model route router should respond")
    }

    fn empty_request(method: Method, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request should build")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

    #[test]
    fn create_model_route_router_registers_routes() {
        let _router = create_model_route_router();
    }

    #[tokio::test]
    async fn delete_model_route_http_endpoint_clears_override_and_route_state() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-model-route-delete-http.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(21101, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini", model.id);
                let api_key = seed_api_key();
                ApiKeyModelOverride::create(&CreateApiKeyModelOverridePayload {
                    api_key_id: api_key.detail.id,
                    source_name: "alias-a".to_string(),
                    target_route_id: route.id,
                    description: Some("override".to_string()),
                    is_enabled: Some(true),
                })
                .expect("override seed should succeed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let override_before = app_state
                    .catalog
                    .get_api_key_override_route(api_key.detail.id, "alias-a")
                    .await
                    .expect("override route cache should load")
                    .expect("override route should exist");
                assert_eq!(override_before.id, route.id);

                let response = send(
                    &app_state,
                    empty_request(Method::DELETE, &format!("/model_route/{}", route.id)),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;

                assert_eq!(body["code"], 0);
                assert!(body["data"].is_null());
                assert!(ModelRoute::get_by_id(route.id).is_err());
                assert!(
                    ApiKeyModelOverride::list_by_api_key_id(api_key.detail.id)
                        .expect("override list should load")
                        .is_empty()
                );

                let override_after = app_state
                    .catalog
                    .get_api_key_override_route(api_key.detail.id, "alias-a")
                    .await
                    .expect("override route cache should reload");
                let route_after = app_state
                    .catalog
                    .get_model_route_by_id(route.id)
                    .await
                    .expect("route cache should reload");

                assert!(override_after.is_none());
                assert!(route_after.is_none());
            })
            .await;
    }
}
