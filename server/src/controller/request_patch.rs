use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::{get, put},
};
use serde::Serialize;

use crate::{
    database::{
        model::Model,
        request_patch::{
            CreateRequestPatchPayload, RequestPatchMutationOutcome, RequestPatchRule,
            RequestPatchRuleResponse, UpdateRequestPatchPayload,
        },
    },
    service::{
        app_state::{AppState, StateRouter, create_state_router},
        cache::types::{
            CacheRequestPatchConflict, CacheRequestPatchExplainEntry, CacheResolvedRequestPatch,
        },
    },
    utils::HttpResult,
};

use super::BaseError;

#[derive(Debug, Serialize)]
struct ModelRequestPatchEffectiveResponse {
    provider_id: i64,
    model_id: i64,
    effective_rules: Vec<CacheResolvedRequestPatch>,
    conflicts: Vec<CacheRequestPatchConflict>,
    has_conflicts: bool,
}

#[derive(Debug, Serialize)]
struct ModelRequestPatchExplainResponse {
    provider_id: i64,
    model_id: i64,
    direct_rules: Vec<crate::service::cache::types::CacheRequestPatchRule>,
    inherited_rules: Vec<crate::service::cache::types::CacheInheritedRequestPatch>,
    effective_rules: Vec<CacheResolvedRequestPatch>,
    explain: Vec<CacheRequestPatchExplainEntry>,
    conflicts: Vec<CacheRequestPatchConflict>,
    has_conflicts: bool,
}

async fn list_provider_request_patches(
    Path(provider_id): Path<i64>,
) -> Result<HttpResult<Vec<RequestPatchRuleResponse>>, BaseError> {
    Ok(HttpResult::new(RequestPatchRule::list_by_provider_id(
        provider_id,
    )?))
}

async fn create_provider_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
    Json(payload): Json<CreateRequestPatchPayload>,
) -> Result<HttpResult<RequestPatchMutationOutcome>, BaseError> {
    let outcome = app_state
        .admin
        .request_patch
        .create_provider_request_patch(provider_id, payload)
        .await?;
    Ok(HttpResult::new(outcome))
}

async fn update_provider_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, rule_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateRequestPatchPayload>,
) -> Result<HttpResult<RequestPatchMutationOutcome>, BaseError> {
    let outcome = app_state
        .admin
        .request_patch
        .update_provider_request_patch(provider_id, rule_id, payload)
        .await?;
    Ok(HttpResult::new(outcome))
}

async fn delete_provider_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, rule_id)): Path<(i64, i64)>,
) -> Result<HttpResult<()>, BaseError> {
    app_state
        .admin
        .request_patch
        .delete_provider_request_patch(provider_id, rule_id)
        .await?;
    Ok(HttpResult::new(()))
}

async fn list_model_request_patches(
    Path(model_id): Path<i64>,
) -> Result<HttpResult<Vec<RequestPatchRuleResponse>>, BaseError> {
    Ok(HttpResult::new(RequestPatchRule::list_by_model_id(
        model_id,
    )?))
}

async fn create_model_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
    Json(payload): Json<CreateRequestPatchPayload>,
) -> Result<HttpResult<RequestPatchMutationOutcome>, BaseError> {
    let outcome = app_state
        .admin
        .request_patch
        .create_model_request_patch(model_id, payload)
        .await?;
    Ok(HttpResult::new(outcome))
}

async fn update_model_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, rule_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateRequestPatchPayload>,
) -> Result<HttpResult<RequestPatchMutationOutcome>, BaseError> {
    let outcome = app_state
        .admin
        .request_patch
        .update_model_request_patch(model_id, rule_id, payload)
        .await?;
    Ok(HttpResult::new(outcome))
}

async fn delete_model_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, rule_id)): Path<(i64, i64)>,
) -> Result<HttpResult<()>, BaseError> {
    app_state
        .admin
        .request_patch
        .delete_model_request_patch(model_id, rule_id)
        .await?;
    Ok(HttpResult::new(()))
}

async fn get_model_request_patch_effective(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
) -> Result<HttpResult<ModelRequestPatchEffectiveResponse>, BaseError> {
    let model = Model::get_by_id(model_id)?;
    let Some(resolved) = app_state
        .catalog
        .get_model_effective_request_patches(model_id)
        .await?
    else {
        return Err(BaseError::NotFound(Some(format!(
            "Model request patch effective result for {} not found",
            model_id
        ))));
    };

    Ok(HttpResult::new(ModelRequestPatchEffectiveResponse {
        provider_id: model.provider_id,
        model_id,
        effective_rules: resolved.effective_rules.clone(),
        conflicts: resolved.conflicts.clone(),
        has_conflicts: resolved.has_conflicts,
    }))
}

async fn get_model_request_patch_explain(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
) -> Result<HttpResult<ModelRequestPatchExplainResponse>, BaseError> {
    let model = Model::get_by_id(model_id)?;
    let Some(resolved) = app_state
        .catalog
        .get_model_effective_request_patches(model_id)
        .await?
    else {
        return Err(BaseError::NotFound(Some(format!(
            "Model request patch explain result for {} not found",
            model_id
        ))));
    };

    Ok(HttpResult::new(ModelRequestPatchExplainResponse {
        provider_id: model.provider_id,
        model_id,
        direct_rules: resolved.direct_rules.clone(),
        inherited_rules: resolved.inherited_rules.clone(),
        effective_rules: resolved.effective_rules.clone(),
        explain: resolved.explain.clone(),
        conflicts: resolved.conflicts.clone(),
        has_conflicts: resolved.has_conflicts,
    }))
}

pub fn create_request_patch_router() -> StateRouter {
    create_state_router()
        .route(
            "/provider/{id}/request_patch",
            get(list_provider_request_patches).post(create_provider_request_patch),
        )
        .route(
            "/provider/{id}/request_patch/{rule_id}",
            put(update_provider_request_patch).delete(delete_provider_request_patch),
        )
        .route(
            "/model/{id}/request_patch",
            get(list_model_request_patches).post(create_model_request_patch),
        )
        .route(
            "/model/{id}/request_patch/effective",
            get(get_model_request_patch_effective),
        )
        .route(
            "/model/{id}/request_patch/explain",
            get(get_model_request_patch_explain),
        )
        .route(
            "/model/{id}/request_patch/{rule_id}",
            put(update_model_request_patch).delete(delete_model_request_patch),
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

    use crate::database::TestDbContext;
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::request_patch::RequestPatchRule;
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::app_state::{AppState, create_test_app_state};

    use super::create_request_patch_router;

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

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_request_patch_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("request patch router should respond")
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
    fn create_request_patch_router_registers_routes() {
        let _router = create_request_patch_router();
    }

    #[tokio::test]
    async fn provider_scope_request_patch_http_lifecycle_updates_effective_endpoint() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-request-patch-provider-http.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(22101, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let create_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{}/request_patch", provider.id),
                        json!({
                            "placement": "BODY",
                            "target": "/temperature",
                            "operation": "SET",
                            "value_json": 0.2,
                            "description": "provider patch",
                            "is_enabled": true
                        }),
                    ),
                )
                .await;
                assert_eq!(create_response.status(), StatusCode::OK);
                let create_body = response_json(create_response).await;
                let rule_id = create_body["data"]["rule"]["id"]
                    .as_i64()
                    .expect("saved provider rule id should exist");
                assert_eq!(create_body["code"], 0);
                assert_eq!(create_body["data"]["result"], "saved");

                let effective_after_create = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        &format!("/model/{}/request_patch/effective", model.id),
                    ),
                )
                .await;
                assert_eq!(effective_after_create.status(), StatusCode::OK);
                let effective_after_create_body = response_json(effective_after_create).await;
                assert_eq!(
                    effective_after_create_body["data"]["effective_rules"]
                        .as_array()
                        .expect("effective rules should be an array")
                        .len(),
                    1
                );
                assert_eq!(
                    effective_after_create_body["data"]["effective_rules"][0]["target"],
                    "/temperature"
                );

                let delete_response = send(
                    &app_state,
                    empty_request(
                        Method::DELETE,
                        &format!("/provider/{}/request_patch/{}", provider.id, rule_id),
                    ),
                )
                .await;
                assert_eq!(delete_response.status(), StatusCode::OK);
                let delete_body = response_json(delete_response).await;
                assert_eq!(delete_body["code"], 0);
                assert!(delete_body["data"].is_null());

                let effective_after_delete = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        &format!("/model/{}/request_patch/effective", model.id),
                    ),
                )
                .await;
                assert_eq!(effective_after_delete.status(), StatusCode::OK);
                let effective_after_delete_body = response_json(effective_after_delete).await;
                assert!(
                    effective_after_delete_body["data"]["effective_rules"]
                        .as_array()
                        .expect("effective rules should be an array")
                        .is_empty()
                );
                assert!(
                    RequestPatchRule::list_by_provider_id(provider.id)
                        .expect("provider rules should load")
                        .is_empty()
                );
            })
            .await;
    }

    #[tokio::test]
    async fn model_scope_request_patch_http_lifecycle_updates_effective_endpoint() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-request-patch-model-http.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(22201, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let create_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/model/{}/request_patch", model.id),
                        json!({
                            "placement": "BODY",
                            "target": "/top_p",
                            "operation": "SET",
                            "value_json": 0.7,
                            "description": "model patch",
                            "is_enabled": true
                        }),
                    ),
                )
                .await;
                assert_eq!(create_response.status(), StatusCode::OK);
                let create_body = response_json(create_response).await;
                let rule_id = create_body["data"]["rule"]["id"]
                    .as_i64()
                    .expect("saved model rule id should exist");
                assert_eq!(create_body["code"], 0);
                assert_eq!(create_body["data"]["result"], "saved");

                let effective_after_create = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        &format!("/model/{}/request_patch/effective", model.id),
                    ),
                )
                .await;
                assert_eq!(effective_after_create.status(), StatusCode::OK);
                let effective_after_create_body = response_json(effective_after_create).await;
                assert_eq!(
                    effective_after_create_body["data"]["effective_rules"]
                        .as_array()
                        .expect("effective rules should be an array")
                        .len(),
                    1
                );
                assert_eq!(
                    effective_after_create_body["data"]["effective_rules"][0]["target"],
                    "/top_p"
                );

                let delete_response = send(
                    &app_state,
                    empty_request(
                        Method::DELETE,
                        &format!("/model/{}/request_patch/{}", model.id, rule_id),
                    ),
                )
                .await;
                assert_eq!(delete_response.status(), StatusCode::OK);
                let delete_body = response_json(delete_response).await;
                assert_eq!(delete_body["code"], 0);
                assert!(delete_body["data"].is_null());

                let effective_after_delete = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        &format!("/model/{}/request_patch/effective", model.id),
                    ),
                )
                .await;
                assert_eq!(effective_after_delete.status(), StatusCode::OK);
                let effective_after_delete_body = response_json(effective_after_delete).await;
                assert!(
                    effective_after_delete_body["data"]["effective_rules"]
                        .as_array()
                        .expect("effective rules should be an array")
                        .is_empty()
                );
                assert!(
                    RequestPatchRule::list_by_model_id(model.id)
                        .expect("model rules should load")
                        .is_empty()
                );
            })
            .await;
    }
}
