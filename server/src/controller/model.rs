use crate::{
    controller::BaseError,
    database::model::{Model, ModelCapabilityFlags, ModelDetail, ModelSummaryItem},
    service::{
        admin::model::{CreateModelInput, UpdateModelInput},
        app_state::{AppState, StateRouter, create_state_router},
    },
    utils::HttpResult, // Import HttpResult
};
use axum::{
    extract::{Json, Path, State}, // Added State
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use std::sync::Arc; // Added Arc

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct InsertModelRequest {
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    #[serde(default = "default_true")]
    pub is_enabled: bool,
    #[serde(default = "default_true")]
    pub supports_streaming: bool,
    #[serde(default = "default_true")]
    pub supports_tools: bool,
    #[serde(default = "default_true")]
    pub supports_reasoning: bool,
    #[serde(default = "default_true")]
    pub supports_image_input: bool,
    #[serde(default = "default_true")]
    pub supports_embeddings: bool,
    #[serde(default = "default_true")]
    pub supports_rerank: bool,
}

async fn insert_model(
    State(app_state): State<Arc<AppState>>,
    Json(request): Json<InsertModelRequest>,
) -> Result<HttpResult<Model>, BaseError> {
    let created_model = app_state
        .admin
        .model
        .create_model(CreateModelInput {
            provider_id: request.provider_id,
            model_name: request.model_name,
            real_model_name: request.real_model_name,
            is_enabled: request.is_enabled,
            capabilities: ModelCapabilityFlags {
                supports_streaming: request.supports_streaming,
                supports_tools: request.supports_tools,
                supports_reasoning: request.supports_reasoning,
                supports_image_input: request.supports_image_input,
                supports_embeddings: request.supports_embeddings,
                supports_rerank: request.supports_rerank,
            },
        })
        .await?;

    Ok(HttpResult::new(created_model))
}

async fn delete_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    app_state.admin.model.delete_model(id).await?;
    Ok(HttpResult::new(()))
}

#[derive(Debug, Deserialize)]
pub struct UpdateModelRequest {
    // pub provider_id: Option<i64>, // Removed: Provider ID is not updatable this way
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
    pub cost_catalog_id: Option<i64>,
    pub supports_streaming: Option<bool>,
    pub supports_tools: Option<bool>,
    pub supports_reasoning: Option<bool>,
    pub supports_image_input: Option<bool>,
    pub supports_embeddings: Option<bool>,
    pub supports_rerank: Option<bool>,
}

async fn update_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateModelRequest>,
) -> Result<HttpResult<Model>, BaseError> {
    let updated_model = app_state
        .admin
        .model
        .update_model(
            id,
            UpdateModelInput {
                model_name: request.model_name,
                real_model_name: request.real_model_name,
                is_enabled: request.is_enabled,
                cost_catalog_id: request.cost_catalog_id,
                supports_streaming: request.supports_streaming,
                supports_tools: request.supports_tools,
                supports_reasoning: request.supports_reasoning,
                supports_image_input: request.supports_image_input,
                supports_embeddings: request.supports_embeddings,
                supports_rerank: request.supports_rerank,
            },
        )
        .await?;

    Ok(HttpResult::new(updated_model))
}

async fn list_models() -> Result<HttpResult<Vec<Model>>, BaseError> {
    let models = Model::list_all()?; // Use list_all
    Ok(HttpResult::new(models))
}

async fn list_model_summaries() -> Result<HttpResult<Vec<ModelSummaryItem>>, BaseError> {
    let models = Model::list_summary()?;
    Ok(HttpResult::new(models))
}

async fn get_model_detail(Path(id): Path<i64>) -> Result<HttpResult<ModelDetail>, BaseError> {
    let detail = Model::get_detail_by_id(id)?;
    Ok(HttpResult::new(detail))
}

// Price related structs and functions (InsertPriceRequest, insert_model_price, list_model_prices)
// are removed as they are not supported by the new server/src/database/model.rs.

pub fn create_model_router() -> StateRouter {
    create_state_router().nest(
        "/model",
        create_state_router()
            .route("/", post(insert_model))
            .route("/summary/list", get(list_model_summaries))
            .route("/list", get(list_models))
            .route("/{id}", delete(delete_model))
            .route("/{id}", put(update_model))
            .route("/{id}/detail", get(get_model_detail)),
        // .route("/{id}/prices", get(list_model_prices)) // Removed price route
        // .route("/{id}/price", post(insert_model_price)), // Removed price route
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode},
    };
    use serde_json::{Value, json};
    use tower::util::ServiceExt;

    use crate::database::TestDbContext;
    use crate::database::model::{Model, ModelCapabilityFlags, ModelSummaryItem};
    use crate::database::model_route::{
        CreateModelRoutePayload, ModelRoute, ModelRouteCandidateInput,
    };
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::request_patch::{CreateRequestPatchPayload, RequestPatchRule};
    use crate::schema::enum_def::{
        ProviderApiKeyMode, ProviderType, RequestPatchOperation, RequestPatchPlacement,
    };
    use crate::service::app_state::{AppState, create_test_app_state};
    use crate::utils::HttpResult;
    use std::collections::BTreeSet;

    use super::create_model_router;

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

    fn create_request_patch_payload() -> CreateRequestPatchPayload {
        CreateRequestPatchPayload {
            placement: RequestPatchPlacement::Body,
            target: "/temperature".to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some(Some(json!(0.2))),
            description: Some("patch".to_string()),
            is_enabled: Some(true),
            confirm_dangerous_target: None,
        }
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_model_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("model router should respond")
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
    fn model_summary_api_contract_includes_provider_context() {
        let payload = HttpResult::new(vec![ModelSummaryItem {
            id: 7,
            provider_id: 3,
            provider_key: "openai-api-example-com".to_string(),
            provider_name: "OpenAI api.example.com".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            real_model_name: Some("gpt-4o-mini-2024-07-18".to_string()),
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: true,
            supports_image_input: true,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled: true,
        }]);

        let value = serde_json::to_value(payload).expect("summary payload should serialize");
        let root = value.as_object().expect("payload should be an object");
        assert_eq!(
            root.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from(["code".to_string(), "data".to_string()])
        );
        assert_eq!(root["code"], 0);

        let items = root["data"].as_array().expect("data should be an array");
        let item = items[0]
            .as_object()
            .expect("summary row should be an object");
        assert_eq!(
            item.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "id".to_string(),
                "provider_id".to_string(),
                "provider_key".to_string(),
                "provider_name".to_string(),
                "model_name".to_string(),
                "real_model_name".to_string(),
                "supports_streaming".to_string(),
                "supports_tools".to_string(),
                "supports_reasoning".to_string(),
                "supports_image_input".to_string(),
                "supports_embeddings".to_string(),
                "supports_rerank".to_string(),
                "is_enabled".to_string(),
            ])
        );
        assert_eq!(item["provider_id"], 3);
        assert_eq!(item["provider_key"], "openai-api-example-com");
        assert_eq!(item["provider_name"], "OpenAI api.example.com");
        assert_eq!(item["model_name"], "gpt-4o-mini");
        assert_eq!(item["real_model_name"], "gpt-4o-mini-2024-07-18");
        assert_eq!(item["is_enabled"], true);
        assert!(item.get("model").is_none());
        assert!(item.get("custom_fields").is_none());
    }

    #[tokio::test]
    async fn delete_model_http_endpoint_updates_response_and_runtime_state() {
        let test_db_context = TestDbContext::new_sqlite("controller-model-delete-http.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(20101, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini", model.id);
                RequestPatchRule::create_for_model(model.id, &create_request_patch_payload())
                    .expect("request patch seed should succeed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let cached_route = app_state
                    .catalog
                    .get_model_route_by_id(route.id)
                    .await
                    .expect("route cache should load")
                    .expect("route should exist");
                let effective_before = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective patch cache should load")
                    .expect("effective patch should exist");
                assert_eq!(cached_route.candidates.len(), 1);
                assert_eq!(effective_before.effective_rules.len(), 1);

                let response = send(
                    &app_state,
                    empty_request(Method::DELETE, &format!("/model/{}", model.id)),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;

                assert_eq!(body["code"], 0);
                assert!(body["data"].is_null());
                assert!(Model::get_by_id(model.id).is_err());

                let cached_route_after = app_state
                    .catalog
                    .get_model_route_by_id(route.id)
                    .await
                    .expect("route cache should reload")
                    .expect("route should still exist");
                let effective_after = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective patch cache should reload");

                assert!(cached_route_after.candidates.is_empty());
                assert!(effective_after.is_none());
            })
            .await;
    }
}
