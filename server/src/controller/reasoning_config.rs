use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    controller::BaseError,
    schema::enum_def::ProviderType,
    service::{
        admin::reasoning_config::{
            ModelReasoningConfigWriteMode, PreviewProviderReasoningConfigInput,
            ReasoningConfigAdminResponse, ReasoningConfigCatalog, ReasoningConfigPresetAdminInput,
            ReasoningConfigPreviewResponse, UpsertModelReasoningConfigInput,
            UpsertProviderReasoningConfigInput,
        },
        app_state::{AppState, StateRouter, create_state_router},
    },
    utils::HttpResult,
};

#[derive(Debug, Deserialize)]
struct ReasoningConfigPresetPayload {
    preset_key: String,
    expose_in_models: bool,
    is_enabled: bool,
}

impl From<ReasoningConfigPresetPayload> for ReasoningConfigPresetAdminInput {
    fn from(payload: ReasoningConfigPresetPayload) -> Self {
        Self {
            preset_key: payload.preset_key,
            expose_in_models: payload.expose_in_models,
            is_enabled: payload.is_enabled,
        }
    }
}

#[derive(Debug, Deserialize)]
struct UpsertProviderReasoningConfigPayload {
    family_key: String,
    #[serde(default)]
    presets: Vec<ReasoningConfigPresetPayload>,
}

#[derive(Debug, Deserialize)]
struct PreviewProviderReasoningConfigPayload {
    provider_type: Option<ProviderType>,
    family_key: Option<String>,
    #[serde(default)]
    presets: Vec<ReasoningConfigPresetPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModelReasoningConfigPayloadMode {
    Inherit,
    Disabled,
    Custom,
}

impl From<ModelReasoningConfigPayloadMode> for ModelReasoningConfigWriteMode {
    fn from(mode: ModelReasoningConfigPayloadMode) -> Self {
        match mode {
            ModelReasoningConfigPayloadMode::Inherit => Self::Inherit,
            ModelReasoningConfigPayloadMode::Disabled => Self::Disabled,
            ModelReasoningConfigPayloadMode::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Deserialize)]
struct UpsertModelReasoningConfigPayload {
    mode: ModelReasoningConfigPayloadMode,
    family_key: Option<String>,
    #[serde(default)]
    presets: Vec<ReasoningConfigPresetPayload>,
}

async fn get_catalog(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<ReasoningConfigCatalog>, BaseError> {
    Ok(HttpResult::new(app_state.admin.reasoning_config.catalog()))
}

async fn get_provider_config(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
) -> Result<HttpResult<ReasoningConfigAdminResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .reasoning_config
            .get_provider_config(provider_id)?,
    ))
}

async fn upsert_provider_config(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
    Json(payload): Json<UpsertProviderReasoningConfigPayload>,
) -> Result<HttpResult<ReasoningConfigAdminResponse>, BaseError> {
    let response = app_state
        .admin
        .reasoning_config
        .upsert_provider_config(
            provider_id,
            UpsertProviderReasoningConfigInput {
                family_key: payload.family_key,
                presets: payload.presets.into_iter().map(Into::into).collect(),
            },
        )
        .await?;
    Ok(HttpResult::new(response))
}

async fn delete_provider_config(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    app_state
        .admin
        .reasoning_config
        .delete_provider_config(provider_id)
        .await?;
    Ok(HttpResult::new(()))
}

async fn preview_provider_config(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
) -> Result<HttpResult<ReasoningConfigPreviewResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .reasoning_config
            .preview_provider_config(provider_id)?,
    ))
}

async fn preview_provider_config_draft(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
    Json(payload): Json<PreviewProviderReasoningConfigPayload>,
) -> Result<HttpResult<ReasoningConfigPreviewResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .reasoning_config
            .preview_provider_config_draft(
                provider_id,
                PreviewProviderReasoningConfigInput {
                    provider_type: payload.provider_type,
                    family_key: payload.family_key,
                    presets: payload.presets.into_iter().map(Into::into).collect(),
                },
            )?,
    ))
}

async fn get_model_config(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
) -> Result<HttpResult<ReasoningConfigAdminResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .reasoning_config
            .get_model_config(model_id)?,
    ))
}

async fn upsert_model_config(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
    Json(payload): Json<UpsertModelReasoningConfigPayload>,
) -> Result<HttpResult<ReasoningConfigAdminResponse>, BaseError> {
    let response = app_state
        .admin
        .reasoning_config
        .upsert_model_config(
            model_id,
            UpsertModelReasoningConfigInput {
                mode: payload.mode.into(),
                family_key: payload.family_key,
                presets: payload.presets.into_iter().map(Into::into).collect(),
            },
        )
        .await?;
    Ok(HttpResult::new(response))
}

async fn delete_model_config(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    app_state
        .admin
        .reasoning_config
        .delete_model_config(model_id)
        .await?;
    Ok(HttpResult::new(()))
}

async fn preview_model_config(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
) -> Result<HttpResult<ReasoningConfigPreviewResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .reasoning_config
            .preview_model_config(model_id)?,
    ))
}

async fn preview_model_config_draft(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
    Json(payload): Json<UpsertModelReasoningConfigPayload>,
) -> Result<HttpResult<ReasoningConfigPreviewResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .reasoning_config
            .preview_model_config_draft(
                model_id,
                UpsertModelReasoningConfigInput {
                    mode: payload.mode.into(),
                    family_key: payload.family_key,
                    presets: payload.presets.into_iter().map(Into::into).collect(),
                },
            )?,
    ))
}

pub fn create_reasoning_config_router() -> StateRouter {
    create_state_router()
        .route("/reasoning_config/catalog", get(get_catalog))
        .route(
            "/provider/{provider_id}/reasoning_config",
            get(get_provider_config)
                .put(upsert_provider_config)
                .delete(delete_provider_config),
        )
        .route(
            "/provider/{provider_id}/reasoning_config/preview",
            get(preview_provider_config).post(preview_provider_config_draft),
        )
        .route(
            "/model/{model_id}/reasoning_config",
            get(get_model_config)
                .put(upsert_model_config)
                .delete(delete_model_config),
        )
        .route(
            "/model/{model_id}/reasoning_config/preview",
            get(preview_model_config).post(preview_model_config_draft),
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
    use crate::service::app_state::{AppState, create_test_app_state};

    use super::create_reasoning_config_router;

    fn seed_provider(id: i64, provider_key: &str) -> Provider {
        seed_provider_of_type(id, provider_key, ProviderType::Openai)
    }

    fn seed_provider_of_type(id: i64, provider_key: &str, provider_type: ProviderType) -> Provider {
        Provider::create(&NewProvider {
            id,
            provider_key: provider_key.to_string(),
            name: provider_key.to_string(),
            endpoint: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_type,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        })
        .expect("provider seed should succeed")
    }

    fn seed_model(provider_id: i64, model_name: &str) -> Model {
        seed_model_with_capabilities(provider_id, model_name, ModelCapabilityFlags::default())
    }

    fn seed_model_with_capabilities(
        provider_id: i64,
        model_name: &str,
        capabilities: ModelCapabilityFlags,
    ) -> Model {
        Model::create(provider_id, model_name, None, true, capabilities)
            .expect("model seed should succeed")
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_reasoning_config_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("reasoning config router should respond")
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

    #[tokio::test]
    async fn reasoning_config_catalog_http_exposes_builtin_metadata() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-reasoning-config-catalog.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let response = send(
                    &app_state,
                    empty_request(Method::GET, "/reasoning_config/catalog"),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(
                    body["data"]["families"][0]["family_key"],
                    "openai_chat_reasoning_effort"
                );
                assert_eq!(body["data"]["families"][0]["target_api_types"][0], "OPENAI");
                let siliconflow_family = body["data"]["families"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|family| family["family_key"] == "siliconflow_openai_enable_thinking")
                    .expect("SiliconFlow family should be exposed");
                assert_eq!(siliconflow_family["target_api_types"], json!(["OPENAI"]));
                assert_eq!(
                    siliconflow_family["supported_presets"],
                    json!(["disabled", "enabled"])
                );
                assert_eq!(body["data"]["presets"][0]["suffix"], "no-think");
            })
            .await;
    }

    #[tokio::test]
    async fn provider_reasoning_config_http_lifecycle_uses_whole_config_payload() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-reasoning-config-provider.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(50101, "provider-http-reasoning");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let missing_response = send(
                    &app_state,
                    empty_request(Method::GET, "/provider/999999/reasoning_config"),
                )
                .await;
                assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);

                let draft_preview_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        format!("/provider/{}/reasoning_config/preview", provider.id),
                        json!({
                            "provider_type": "RESPONSES",
                            "family_key": "openai_responses_reasoning",
                            "presets": []
                        }),
                    ),
                )
                .await;
                assert_eq!(draft_preview_response.status(), StatusCode::OK);
                let draft_preview_body = response_json(draft_preview_response).await;
                assert_eq!(draft_preview_body["data"]["target_api_type"], "RESPONSES");
                let draft_low = draft_preview_body["data"]["presets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["preset_key"] == "low")
                    .expect("low preset should be present");
                assert_eq!(draft_low["enabled"], false);
                assert_eq!(draft_low["runtime_supported"], false);
                assert_eq!(
                    draft_low["generated_patches"][0]["target"],
                    "/reasoning/effort"
                );
                assert_eq!(
                    draft_low["generated_patches"][0]["value_json"],
                    json!("low")
                );

                let upsert_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!("/provider/{}/reasoning_config", provider.id),
                        json!({
                            "family_key": "openai_chat_reasoning_effort",
                            "presets": [{
                                "preset_key": "high",
                                "is_enabled": true,
                                "expose_in_models": true
                            }]
                        }),
                    ),
                )
                .await;
                assert_eq!(upsert_response.status(), StatusCode::OK);
                let upsert_body = response_json(upsert_response).await;
                assert_eq!(upsert_body["data"]["owner_kind"], "provider");
                assert_eq!(upsert_body["data"]["status"], "custom");
                assert_eq!(
                    upsert_body["data"]["owner_config"]["family_key"],
                    "openai_chat_reasoning_effort"
                );
                assert_eq!(
                    upsert_body["data"]["owner_config"]["presets"][0]["preset_key"],
                    "high"
                );

                let preview_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        format!("/provider/{}/reasoning_config/preview", provider.id),
                    ),
                )
                .await;
                assert_eq!(preview_response.status(), StatusCode::OK);
                let preview_body = response_json(preview_response).await;
                assert_eq!(preview_body["data"]["target_api_type"], "OPENAI");
                assert_eq!(preview_body["data"]["presets"].as_array().unwrap().len(), 7);
                let high = preview_body["data"]["presets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["preset_key"] == "high")
                    .expect("high preset should be present");
                assert_eq!(high["enabled"], true);
                assert_eq!(high["runtime_supported"], true);
                assert_eq!(high["generated_patches"][0]["placement"], "BODY");
                assert_eq!(high["generated_patches"][0]["operation"], "SET");
                assert_eq!(high["generated_patches"][0]["target"], "/reasoning_effort");
                assert_eq!(high["generated_patches"][0]["value_json"], json!("high"));

                let delete_response = send(
                    &app_state,
                    empty_request(
                        Method::DELETE,
                        format!("/provider/{}/reasoning_config", provider.id),
                    ),
                )
                .await;
                assert_eq!(delete_response.status(), StatusCode::OK);

                let get_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        format!("/provider/{}/reasoning_config", provider.id),
                    ),
                )
                .await;
                let get_body = response_json(get_response).await;
                assert_eq!(get_body["data"]["effective_source"], "missing");
                assert!(get_body["data"]["owner_config"].is_null());
            })
            .await;
    }

    #[tokio::test]
    async fn model_reasoning_config_http_supports_inherit_disabled_and_custom() {
        let test_db_context = TestDbContext::new_sqlite("controller-reasoning-config-model.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(50201, "model-http-reasoning");
                let model = seed_model(provider.id, "gpt-4o-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let provider_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!("/provider/{}/reasoning_config", provider.id),
                        json!({
                            "family_key": "openai_chat_reasoning_effort",
                            "presets": [{
                                "preset_key": "high",
                                "is_enabled": true,
                                "expose_in_models": true
                            }]
                        }),
                    ),
                )
                .await;
                assert_eq!(provider_response.status(), StatusCode::OK);

                let draft_preview_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        format!("/model/{}/reasoning_config/preview", model.id),
                        json!({
                            "mode": "custom",
                            "family_key": "openai_chat_reasoning_effort",
                            "presets": [{
                                "preset_key": "low",
                                "is_enabled": false,
                                "expose_in_models": false
                            }]
                        }),
                    ),
                )
                .await;
                assert_eq!(draft_preview_response.status(), StatusCode::OK);
                let draft_preview_body = response_json(draft_preview_response).await;
                assert_eq!(
                    draft_preview_body["data"]["config"]["effective_source"],
                    "model_custom"
                );
                let draft_low = draft_preview_body["data"]["presets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["preset_key"] == "low")
                    .expect("low preset should be present");
                assert_eq!(draft_low["enabled"], false);
                assert_eq!(
                    draft_low["generated_patches"][0]["value_json"],
                    json!("low")
                );

                let inherited_response = send(
                    &app_state,
                    empty_request(Method::GET, format!("/model/{}/reasoning_config", model.id)),
                )
                .await;
                let inherited_body = response_json(inherited_response).await;
                assert_eq!(
                    inherited_body["data"]["effective_source"],
                    "provider_default"
                );
                assert_eq!(inherited_body["data"]["status"], "inherited");

                let disabled_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!("/model/{}/reasoning_config", model.id),
                        json!({
                            "mode": "disabled"
                        }),
                    ),
                )
                .await;
                assert_eq!(disabled_response.status(), StatusCode::OK);
                let disabled_body = response_json(disabled_response).await;
                assert_eq!(disabled_body["data"]["effective_source"], "model_disabled");
                assert_eq!(disabled_body["data"]["status"], "disabled");
                assert!(
                    disabled_body["data"]["effective_config"]["presets"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );

                let custom_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!("/model/{}/reasoning_config", model.id),
                        json!({
                            "mode": "custom",
                            "family_key": "openai_chat_reasoning_effort",
                            "presets": [{
                                "preset_key": "low",
                                "is_enabled": true,
                                "expose_in_models": false
                            }]
                        }),
                    ),
                )
                .await;
                assert_eq!(custom_response.status(), StatusCode::OK);
                let custom_body = response_json(custom_response).await;
                assert_eq!(custom_body["data"]["effective_source"], "model_custom");
                assert_eq!(
                    custom_body["data"]["owner_config"]["presets"][0]["preset_key"],
                    "low"
                );

                let inherit_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!("/model/{}/reasoning_config", model.id),
                        json!({
                            "mode": "inherit"
                        }),
                    ),
                )
                .await;
                assert_eq!(inherit_response.status(), StatusCode::OK);
                let inherit_body = response_json(inherit_response).await;
                assert_eq!(inherit_body["data"]["effective_source"], "provider_default");
                assert!(inherit_body["data"]["owner_config"].is_null());

                let delete_response = send(
                    &app_state,
                    empty_request(
                        Method::DELETE,
                        format!("/model/{}/reasoning_config", model.id),
                    ),
                )
                .await;
                assert_eq!(delete_response.status(), StatusCode::OK);
            })
            .await;
    }

    #[tokio::test]
    async fn model_reasoning_config_preview_uses_model_reasoning_capability() {
        let test_db_context = TestDbContext::new_sqlite(
            "controller-reasoning-config-model-preview-capability.sqlite",
        );

        test_db_context
            .run_async(async {
                let provider = seed_provider(50301, "model-preview-capability");
                let mut capabilities = ModelCapabilityFlags::default();
                capabilities.supports_reasoning = false;
                let model = seed_model_with_capabilities(provider.id, "plain-model", capabilities);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let provider_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        format!("/provider/{}/reasoning_config", provider.id),
                        json!({
                            "family_key": "openai_chat_reasoning_effort",
                            "presets": [{
                                "preset_key": "high",
                                "is_enabled": true,
                                "expose_in_models": true
                            }]
                        }),
                    ),
                )
                .await;
                assert_eq!(provider_response.status(), StatusCode::OK);

                let preview_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        format!("/model/{}/reasoning_config/preview", model.id),
                    ),
                )
                .await;
                assert_eq!(preview_response.status(), StatusCode::OK);
                let preview_body = response_json(preview_response).await;
                let high = preview_body["data"]["presets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["preset_key"] == "high")
                    .expect("high preset should be present");

                assert_eq!(preview_body["data"]["target_api_type"], "OPENAI");
                assert_eq!(high["enabled"], true);
                assert_eq!(high["runtime_supported"], false);
                assert!(high["generated_patches"].as_array().unwrap().is_empty());
                assert!(
                    high["unsupported_reason"]
                        .as_str()
                        .unwrap()
                        .contains("capability")
                );
            })
            .await;
    }
}
