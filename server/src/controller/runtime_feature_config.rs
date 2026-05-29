use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    controller::BaseError,
    service::{
        admin::runtime_feature_config::{
            RuntimeFeatureCatalog, RuntimeFeatureConfigAdminResponse,
            UpsertRuntimeFeatureConfigInput,
        },
        app_state::{AppState, StateRouter, create_state_router},
    },
    utils::HttpResult,
};

#[derive(Debug, Deserialize)]
struct UpsertRuntimeFeatureConfigPayload {
    enabled: bool,
}

async fn get_catalog(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<RuntimeFeatureCatalog>, BaseError> {
    Ok(HttpResult::new(
        app_state.admin.runtime_feature_config.catalog(),
    ))
}

async fn get_provider_config(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
) -> Result<HttpResult<RuntimeFeatureConfigAdminResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .runtime_feature_config
            .get_provider_config(provider_id)?,
    ))
}

async fn upsert_provider_config(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, feature_key)): Path<(i64, String)>,
    Json(payload): Json<UpsertRuntimeFeatureConfigPayload>,
) -> Result<HttpResult<RuntimeFeatureConfigAdminResponse>, BaseError> {
    let response = app_state
        .admin
        .runtime_feature_config
        .upsert_provider_config(
            provider_id,
            &feature_key,
            UpsertRuntimeFeatureConfigInput {
                enabled: payload.enabled,
            },
        )
        .await?;
    Ok(HttpResult::new(response))
}

async fn delete_provider_config(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, feature_key)): Path<(i64, String)>,
) -> Result<HttpResult<()>, BaseError> {
    app_state
        .admin
        .runtime_feature_config
        .delete_provider_config(provider_id, &feature_key)
        .await?;
    Ok(HttpResult::new(()))
}

async fn get_model_config(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
) -> Result<HttpResult<RuntimeFeatureConfigAdminResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .runtime_feature_config
            .get_model_config(model_id)?,
    ))
}

async fn upsert_model_config(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, feature_key)): Path<(i64, String)>,
    Json(payload): Json<UpsertRuntimeFeatureConfigPayload>,
) -> Result<HttpResult<RuntimeFeatureConfigAdminResponse>, BaseError> {
    let response = app_state
        .admin
        .runtime_feature_config
        .upsert_model_config(
            model_id,
            &feature_key,
            UpsertRuntimeFeatureConfigInput {
                enabled: payload.enabled,
            },
        )
        .await?;
    Ok(HttpResult::new(response))
}

async fn delete_model_config(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, feature_key)): Path<(i64, String)>,
) -> Result<HttpResult<()>, BaseError> {
    app_state
        .admin
        .runtime_feature_config
        .delete_model_config(model_id, &feature_key)
        .await?;
    Ok(HttpResult::new(()))
}

pub fn create_runtime_feature_config_router() -> StateRouter {
    create_state_router()
        .route("/runtime_feature_config/catalog", get(get_catalog))
        .route(
            "/provider/{provider_id}/runtime_feature_config",
            get(get_provider_config),
        )
        .route(
            "/provider/{provider_id}/runtime_feature_config/{feature_key}",
            axum::routing::put(upsert_provider_config).delete(delete_provider_config),
        )
        .route(
            "/model/{model_id}/runtime_feature_config",
            get(get_model_config),
        )
        .route(
            "/model/{model_id}/runtime_feature_config/{feature_key}",
            axum::routing::put(upsert_model_config).delete(delete_model_config),
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
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::provider::{NewProvider, Provider};
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::admin::audit::AdminAuditEvent;
    use crate::service::app_state::{AppState, create_test_app_state};

    use super::create_runtime_feature_config_router;

    const FEATURE_KEY: &str = "openai_reasoning_content_repair";

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

    fn seed_model(provider_id: i64, model_name: &str) -> Model {
        Model::create(
            provider_id,
            model_name,
            None,
            true,
            ModelCapabilityFlags::default(),
        )
        .expect("model seed should succeed")
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_runtime_feature_config_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("runtime feature config router should respond")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

    fn empty_request(method: Method, uri: impl AsRef<str>) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri.as_ref())
            .body(Body::empty())
            .expect("request should build")
    }

    fn json_request(method: Method, uri: impl AsRef<str>, payload: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri.as_ref())
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request should build")
    }

    fn audit_field(event: &AdminAuditEvent, key: &str) -> String {
        event
            .fields()
            .iter()
            .find(|field| field.key() == key)
            .map(|field| field.value().to_string())
            .unwrap_or_else(|| panic!("audit field {key} should exist"))
    }

    #[tokio::test]
    async fn runtime_feature_catalog_http_exposes_builtin_metadata() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-runtime-feature-config-catalog.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let response = send(
                    &app_state,
                    empty_request(Method::GET, "/runtime_feature_config/catalog"),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(body["data"]["features"][0]["feature_key"], FEATURE_KEY);
                assert_eq!(body["data"]["features"][0]["default_enabled"], false);
                assert_eq!(
                    body["data"]["features"][0]["supported_scope_kinds"],
                    json!(["provider", "model"])
                );
            })
            .await;
    }

    #[tokio::test]
    async fn provider_runtime_feature_config_http_lifecycle_uses_feature_key_path() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-runtime-feature-config-provider.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(54101, "provider-http-runtime-feature");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let missing_response = send(
                    &app_state,
                    empty_request(Method::GET, "/provider/999999/runtime_feature_config"),
                )
                .await;
                assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);

                let default_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        format!("/provider/{}/runtime_feature_config", provider.id),
                    ),
                )
                .await;
                assert_eq!(default_response.status(), StatusCode::OK);
                let default_body = response_json(default_response).await;
                assert_eq!(default_body["data"]["owner_kind"], "provider");
                assert_eq!(
                    default_body["data"]["features"][0]["effective_source"],
                    "default_false"
                );
                assert_eq!(
                    default_body["data"]["features"][0]["effective_enabled"],
                    false
                );
                assert!(default_body["data"]["features"][0]["owner_config"].is_null());

                let invalid_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!(
                            "/provider/{}/runtime_feature_config/not_a_real_feature",
                            provider.id
                        ),
                        json!({ "enabled": true }),
                    ),
                )
                .await;
                assert_eq!(invalid_response.status(), StatusCode::BAD_REQUEST);

                let upsert_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!(
                            "/provider/{}/runtime_feature_config/{}",
                            provider.id, FEATURE_KEY
                        ),
                        json!({ "enabled": true }),
                    ),
                )
                .await;
                assert_eq!(upsert_response.status(), StatusCode::OK);
                let upsert_body = response_json(upsert_response).await;
                assert_eq!(
                    upsert_body["data"]["features"][0]["effective_source"],
                    "provider_default"
                );
                assert_eq!(
                    upsert_body["data"]["features"][0]["effective_enabled"],
                    true
                );
                assert_eq!(
                    upsert_body["data"]["features"][0]["owner_config"]["feature_key"],
                    FEATURE_KEY
                );
                assert_eq!(
                    upsert_body["data"]["features"][0]["owner_config"]["enabled"],
                    true
                );

                let events = app_state
                    .admin
                    .runtime_feature_config
                    .mutation_runner()
                    .drain_audit_events();
                let upsert_event = events
                    .iter()
                    .find(|event| {
                        event.event_name() == "manager.runtime_feature_config_provider_upserted"
                    })
                    .expect("provider upsert audit event should be emitted");
                assert_eq!(audit_field(upsert_event, "scope"), "provider");
                assert_eq!(
                    audit_field(upsert_event, "owner_id"),
                    provider.id.to_string()
                );
                assert_eq!(audit_field(upsert_event, "feature_key"), FEATURE_KEY);
                assert_eq!(audit_field(upsert_event, "enabled"), "true");

                let delete_response = send(
                    &app_state,
                    empty_request(
                        Method::DELETE,
                        format!(
                            "/provider/{}/runtime_feature_config/{}",
                            provider.id, FEATURE_KEY
                        ),
                    ),
                )
                .await;
                assert_eq!(delete_response.status(), StatusCode::OK);

                let get_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        format!("/provider/{}/runtime_feature_config", provider.id),
                    ),
                )
                .await;
                let get_body = response_json(get_response).await;
                assert_eq!(
                    get_body["data"]["features"][0]["effective_source"],
                    "default_false"
                );
                assert_eq!(get_body["data"]["features"][0]["effective_enabled"], false);
                assert!(get_body["data"]["features"][0]["owner_config"].is_null());
            })
            .await;
    }

    #[tokio::test]
    async fn model_runtime_feature_config_http_inherits_overrides_and_deletes() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-runtime-feature-config-model.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(54201, "model-http-runtime-feature");
                let model = seed_model(provider.id, "gpt-5-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let missing_response = send(
                    &app_state,
                    empty_request(Method::GET, "/model/999999/runtime_feature_config"),
                )
                .await;
                assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);

                let provider_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!(
                            "/provider/{}/runtime_feature_config/{}",
                            provider.id, FEATURE_KEY
                        ),
                        json!({ "enabled": true }),
                    ),
                )
                .await;
                assert_eq!(provider_response.status(), StatusCode::OK);
                app_state
                    .admin
                    .runtime_feature_config
                    .mutation_runner()
                    .drain_audit_events();

                let inherited_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        format!("/model/{}/runtime_feature_config", model.id),
                    ),
                )
                .await;
                assert_eq!(inherited_response.status(), StatusCode::OK);
                let inherited_body = response_json(inherited_response).await;
                assert_eq!(inherited_body["data"]["owner_kind"], "model");
                assert_eq!(
                    inherited_body["data"]["features"][0]["effective_source"],
                    "provider_default"
                );
                assert_eq!(
                    inherited_body["data"]["features"][0]["effective_enabled"],
                    true
                );
                assert!(inherited_body["data"]["features"][0]["owner_config"].is_null());
                assert!(
                    inherited_body["data"]["features"][0]["provider_config"]["enabled"]
                        .as_bool()
                        .unwrap()
                );

                let override_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!("/model/{}/runtime_feature_config/{}", model.id, FEATURE_KEY),
                        json!({ "enabled": false }),
                    ),
                )
                .await;
                assert_eq!(override_response.status(), StatusCode::OK);
                let override_body = response_json(override_response).await;
                assert_eq!(
                    override_body["data"]["features"][0]["effective_source"],
                    "model_override"
                );
                assert_eq!(
                    override_body["data"]["features"][0]["effective_enabled"],
                    false
                );
                assert_eq!(
                    override_body["data"]["features"][0]["owner_config"]["enabled"],
                    false
                );

                let events = app_state
                    .admin
                    .runtime_feature_config
                    .mutation_runner()
                    .drain_audit_events();
                let model_event = events
                    .iter()
                    .find(|event| {
                        event.event_name() == "manager.runtime_feature_config_model_upserted"
                    })
                    .expect("model upsert audit event should be emitted");
                assert_eq!(audit_field(model_event, "scope"), "model");
                assert_eq!(audit_field(model_event, "owner_id"), model.id.to_string());
                assert_eq!(audit_field(model_event, "feature_key"), FEATURE_KEY);
                assert_eq!(audit_field(model_event, "enabled"), "false");

                let delete_response = send(
                    &app_state,
                    empty_request(
                        Method::DELETE,
                        format!("/model/{}/runtime_feature_config/{}", model.id, FEATURE_KEY),
                    ),
                )
                .await;
                assert_eq!(delete_response.status(), StatusCode::OK);

                let inherited_again_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        format!("/model/{}/runtime_feature_config", model.id),
                    ),
                )
                .await;
                let inherited_again_body = response_json(inherited_again_response).await;
                assert_eq!(
                    inherited_again_body["data"]["features"][0]["effective_source"],
                    "provider_default"
                );
                assert_eq!(
                    inherited_again_body["data"]["features"][0]["effective_enabled"],
                    true
                );
                assert!(inherited_again_body["data"]["features"][0]["owner_config"].is_null());
            })
            .await;
    }
}
