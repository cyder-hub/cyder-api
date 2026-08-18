use crate::{
    controller::BaseError,
    database::model::{Model, ModelDetail, ModelSummaryItem},
    database::model_source_binding::ModelSourceConfig,
    schema::enum_def::ModelKind,
    service::{
        admin::model::{
            CreateModelInput, ModelSourceConfigSummary, ModelSourceExplain,
            ModelSourceSnapshotOwner, UpdateModelInput, load_model_source_config_summaries,
        },
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
#[serde(deny_unknown_fields)]
pub struct InsertModelRequest {
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub model_kind: ModelKind,
    #[serde(default = "default_true")]
    pub is_enabled: bool,
    pub source_config: Option<ModelSourceConfig>,
}

#[derive(Debug, serde::Serialize)]
struct ModelResponse {
    #[serde(flatten)]
    model: Model,
    source_config: ModelSourceConfigSummary,
}

#[derive(Debug, serde::Serialize)]
struct ModelSummaryResponse {
    #[serde(flatten)]
    summary: ModelSummaryItem,
    source_config: ModelSourceConfigSummary,
}

#[derive(Debug, serde::Serialize)]
struct ModelDetailResponse {
    #[serde(flatten)]
    detail: ModelDetail,
    source_config: ModelSourceConfigSummary,
}

async fn insert_model(
    State(app_state): State<Arc<AppState>>,
    Json(request): Json<InsertModelRequest>,
) -> Result<HttpResult<ModelResponse>, BaseError> {
    let created_model = app_state
        .admin
        .model
        .create_model_with_source_config(
            CreateModelInput {
                provider_id: request.provider_id,
                model_name: request.model_name,
                real_model_name: request.real_model_name,
                model_kind: request.model_kind,
                is_enabled: request.is_enabled,
            },
            request.source_config,
        )
        .await?;
    let source_config = app_state
        .admin
        .model
        .get_model_source_config_summary(created_model.id)
        .await?;

    Ok(HttpResult::new(ModelResponse {
        model: created_model,
        source_config,
    }))
}

async fn delete_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    app_state.admin.model.delete_model(id).await?;
    Ok(HttpResult::new(()))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateModelRequest {
    // pub provider_id: Option<i64>, // Removed: Provider ID is not updatable this way
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub model_kind: Option<ModelKind>,
    pub is_enabled: bool,
    pub cost_catalog_id: Option<i64>,
}

async fn update_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateModelRequest>,
) -> Result<HttpResult<Model>, BaseError> {
    if request.model_kind.is_some() {
        return Err(BaseError::ParamInvalid(Some(
            "model_kind is immutable after model creation".to_string(),
        )));
    }
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
            },
        )
        .await?;

    Ok(HttpResult::new(updated_model))
}

async fn list_models(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<Vec<ModelResponse>>, BaseError> {
    let models = Model::list_all(&app_state.database).await?;
    let owners = models
        .iter()
        .map(|model| ModelSourceSnapshotOwner {
            model_id: model.id,
            provider_id: model.provider_id,
            model_kind: model.model_kind,
            source_selection_mode: model.source_selection_mode.clone(),
        })
        .collect::<Vec<_>>();
    let summaries = load_model_source_config_summaries(&app_state.database, &owners).await?;
    let response = models
        .into_iter()
        .map(|model| {
            let source_config = summaries.get(&model.id).cloned().ok_or_else(|| {
                BaseError::DatabaseFatal(Some(format!(
                    "source config summary for model {} was not loaded",
                    model.id
                )))
            })?;
            Ok(ModelResponse {
                model,
                source_config,
            })
        })
        .collect::<Result<Vec<_>, BaseError>>()?;
    Ok(HttpResult::new(response))
}

async fn list_model_summaries(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<Vec<ModelSummaryResponse>>, BaseError> {
    let summaries = Model::list_summary(&app_state.database).await?;
    let models = Model::list_all(&app_state.database).await?;
    let owners = models
        .iter()
        .map(|model| ModelSourceSnapshotOwner {
            model_id: model.id,
            provider_id: model.provider_id,
            model_kind: model.model_kind,
            source_selection_mode: model.source_selection_mode.clone(),
        })
        .collect::<Vec<_>>();
    let source_configs = load_model_source_config_summaries(&app_state.database, &owners).await?;
    let response = summaries
        .into_iter()
        .map(|summary| {
            let source_config = source_configs.get(&summary.id).cloned().ok_or_else(|| {
                BaseError::DatabaseFatal(Some(format!(
                    "source config summary for model {} was not loaded",
                    summary.id
                )))
            })?;
            Ok(ModelSummaryResponse {
                summary,
                source_config,
            })
        })
        .collect::<Result<Vec<_>, BaseError>>()?;
    Ok(HttpResult::new(response))
}

async fn get_model_detail(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ModelDetailResponse>, BaseError> {
    let detail = Model::get_detail_by_id(&app_state.database, id).await?;
    let source_config = app_state
        .admin
        .model
        .get_model_source_config_summary(id)
        .await?;
    Ok(HttpResult::new(ModelDetailResponse {
        detail,
        source_config,
    }))
}

async fn get_model_source_config(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ModelSourceConfigSummary>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .model
            .get_model_source_config_summary(id)
            .await?,
    ))
}

async fn put_model_source_config(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(source_config): Json<ModelSourceConfig>,
) -> Result<HttpResult<ModelSourceConfigSummary>, BaseError> {
    app_state
        .admin
        .model
        .replace_model_source_config(id, source_config)
        .await?;
    Ok(HttpResult::new(
        app_state
            .admin
            .model
            .get_model_source_config_summary(id)
            .await?,
    ))
}

async fn explain_model_source_config(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ModelSourceExplain>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .model
            .explain_model_source_config(id)
            .await?,
    ))
}

// Price related structs and functions (InsertPriceRequest, insert_model_price, list_model_prices)
// are removed as they are not supported by the new server/src/database/model.rs.

pub fn create_model_controller_router() -> StateRouter {
    create_state_router().nest(
        "/model",
        create_state_router()
            .route("/", post(insert_model))
            .route("/summary/list", get(list_model_summaries))
            .route("/list", get(list_models))
            .route(
                "/{id}/source-config",
                get(get_model_source_config).put(put_model_source_config),
            )
            .route(
                "/{id}/source-config/explain",
                get(explain_model_source_config),
            )
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
        http::{Method, Request, StatusCode, header::CONTENT_TYPE},
    };
    use serde_json::{Value, json};
    use tower::util::ServiceExt;

    use crate::database::TestDatabase;
    use crate::database::model::Model;
    use crate::database::provider::{NewProvider, Provider, UpdateProviderData};
    use crate::database::upstream_source::{NewUpstreamSource, UpstreamSource};
    use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};
    use crate::service::app_state::{AppState, create_test_app_state};

    use super::create_model_controller_router;

    async fn seed_provider(
        database: &crate::database::runtime::DatabaseRuntime,
        provider_id: i64,
        source_id: i64,
    ) -> Provider {
        Provider::create(
            database,
            &NewProvider {
                id: provider_id,
                provider_key: format!("model-api-{provider_id}"),
                name: format!("Model API {provider_id}"),
                is_enabled: true,
                created_at: 1,
                updated_at: 1,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
            },
            &NewUpstreamSource {
                id: source_id,
                provider_id,
                profile_type: UpstreamProfileType::Openai,
                base_url: "https://model-api.example.com/v1".to_string(),
                use_proxy: false,
                chat_completions_enabled: Some(true),
                chat_completions_path_override: None,
                embeddings_enabled: Some(true),
                embeddings_path_override: None,
                rerank_enabled: Some(false),
                rerank_path_override: None,
                is_enabled: true,
                is_default: true,
                created_at: 1,
                updated_at: 1,
            },
        )
        .await
        .expect("provider seed should succeed")
        .provider
    }

    async fn seed_source(
        database: &crate::database::runtime::DatabaseRuntime,
        provider_id: i64,
        source_id: i64,
        profile_type: UpstreamProfileType,
    ) -> UpstreamSource {
        let (chat_completions_enabled, embeddings_enabled, rerank_enabled) = match profile_type {
            UpstreamProfileType::Openai | UpstreamProfileType::GeminiOpenai => {
                (Some(true), Some(true), Some(false))
            }
            UpstreamProfileType::OpenaiCompatible => (Some(true), Some(false), Some(false)),
            _ => (None, None, None),
        };
        UpstreamSource::create(
            database,
            &NewUpstreamSource {
                id: source_id,
                provider_id,
                profile_type,
                base_url: format!("https://model-api-{source_id}.example.com/v1"),
                use_proxy: false,
                chat_completions_enabled,
                chat_completions_path_override: None,
                embeddings_enabled,
                embeddings_path_override: None,
                rerank_enabled,
                rerank_path_override: None,
                is_enabled: true,
                is_default: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .await
        .expect("source seed should succeed")
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_model_controller_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("model router should respond")
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
    fn create_model_router_registers_source_config_routes() {
        let _router = create_model_controller_router();
    }

    #[tokio::test]
    async fn source_config_http_lifecycle_is_atomic_and_explain_survives_disabled_entities() {
        let test_db_context =
            TestDatabase::new_sqlite_default("controller-model-source-config-http.sqlite").await;

        (async {
            let provider = seed_provider(&test_db_context, 24101, 24102).await;
            let responses_source = seed_source(
                &test_db_context,
                provider.id,
                24103,
                UpstreamProfileType::Responses,
            )
            .await;
            let app_state = create_test_app_state(test_db_context.clone()).await;

            let create_response = send(
                &app_state,
                json_request(
                    Method::POST,
                    "/model",
                    json!({
                        "provider_id": provider.id,
                        "model_name": "configured-model",
                        "real_model_name": null,
                        "model_kind": "CHAT",
                        "is_enabled": true,
                        "source_config": {
                            "source_selection_mode": "EXPLICIT",
                            "bindings": [
                                {"source_id": 24102, "is_default": true},
                                {"source_id": responses_source.id, "is_default": false}
                            ]
                        }
                    }),
                ),
            )
            .await;
            let create_status = create_response.status();
            let create_body = response_json(create_response).await;
            assert_eq!(
                create_status,
                StatusCode::OK,
                "model create failed: {create_body}"
            );
            assert_eq!(create_body["code"], 0);
            assert_eq!(
                create_body["data"]["source_config"]["source_selection_mode"],
                "EXPLICIT"
            );
            assert_eq!(
                create_body["data"]["source_config"]["declared_source_count"],
                2
            );
            assert_eq!(
                create_body["data"]["source_config"]["enabled_source_count"],
                2
            );
            assert_eq!(
                create_body["data"]["source_config"]["model_default_source_id"],
                24102
            );
            assert_eq!(
                create_body["data"]["source_config"]["bindings"]
                    .as_array()
                    .expect("bindings should be returned")
                    .len(),
                2
            );
            let model_id = create_body["data"]["id"]
                .as_i64()
                .expect("created model id should exist");

            let immutable_kind_response = send(
                &app_state,
                json_request(
                    Method::PUT,
                    &format!("/model/{model_id}"),
                    json!({
                        "model_name": "configured-model",
                        "real_model_name": null,
                        "model_kind": "EMBEDDING",
                        "is_enabled": true,
                        "cost_catalog_id": null
                    }),
                ),
            )
            .await;
            assert_eq!(immutable_kind_response.status(), StatusCode::BAD_REQUEST);
            let immutable_kind_body = response_json(immutable_kind_response).await;
            assert_eq!(immutable_kind_body["code"], 1001);
            assert_eq!(
                immutable_kind_body["msg"],
                "model_kind is immutable after model creation"
            );

            for uri in [
                format!("/model/{model_id}/source-config"),
                "/model/list".to_string(),
                "/model/summary/list".to_string(),
                format!("/model/{model_id}/detail"),
            ] {
                let response = send(&app_state, empty_request(Method::GET, &uri)).await;
                assert_eq!(response.status(), StatusCode::OK, "GET {uri}");
                let body = response_json(response).await;
                assert_eq!(body["code"], 0, "GET {uri}");
                let source_config = if uri.ends_with("/source-config") {
                    &body["data"]
                } else if uri.ends_with("/list") || uri.ends_with("/summary/list") {
                    &body["data"][0]
                } else {
                    &body["data"]
                };
                let source_config = if uri.ends_with("/source-config") {
                    source_config
                } else {
                    &source_config["source_config"]
                };
                assert_eq!(source_config["source_selection_mode"], "EXPLICIT");
            }

            let explain = send(
                &app_state,
                empty_request(
                    Method::GET,
                    &format!("/model/{model_id}/source-config/explain"),
                ),
            )
            .await;
            assert_eq!(explain.status(), StatusCode::OK);
            let explain_body = response_json(explain).await;
            assert_eq!(
                explain_body["data"]["protocols"].as_array().unwrap().len(),
                4
            );
            assert_eq!(explain_body["data"]["provider_enabled"], true);
            assert_eq!(explain_body["data"]["model_enabled"], true);
            let openai_protocol = explain_body["data"]["protocols"]
                .as_array()
                .unwrap()
                .iter()
                .find(|protocol| protocol["downstream_protocol"] == "OPENAI")
                .expect("OpenAI explanation should exist");
            assert_eq!(openai_protocol["selection_status"], "selected");
            assert_eq!(openai_protocol["source_id"], 24102);
            assert_eq!(openai_protocol["selection_reason"], "protocol_match");
            assert!(
                !openai_protocol["decision_trace"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                openai_protocol["generation_execution_status"],
                "runtime_validation_required"
            );

            let replace_empty = send(
                &app_state,
                json_request(
                    Method::PUT,
                    &format!("/model/{model_id}/source-config"),
                    json!({
                        "source_selection_mode": "EXPLICIT",
                        "bindings": []
                    }),
                ),
            )
            .await;
            assert_eq!(replace_empty.status(), StatusCode::OK);
            let replace_empty_body = response_json(replace_empty).await;
            assert_eq!(
                replace_empty_body["data"]["warnings"],
                json!(["explicit_empty"])
            );

            let empty_explain = send(
                &app_state,
                empty_request(
                    Method::GET,
                    &format!("/model/{model_id}/source-config/explain"),
                ),
            )
            .await;
            let empty_explain_body = response_json(empty_explain).await;
            assert!(
                empty_explain_body["data"]["protocols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|protocol| protocol["selection_status"] == "unselectable")
            );

            let core_update = send(
                &app_state,
                json_request(
                    Method::PUT,
                    &format!("/model/{model_id}"),
                    json!({
                        "model_name": "configured-model-renamed",
                        "real_model_name": null,
                        "is_enabled": true,
                        "cost_catalog_id": null
                    }),
                ),
            )
            .await;
            assert_eq!(core_update.status(), StatusCode::OK);
            let config_after_core_update = send(
                &app_state,
                empty_request(Method::GET, &format!("/model/{model_id}/source-config")),
            )
            .await;
            let config_after_core_update_body = response_json(config_after_core_update).await;
            assert_eq!(
                config_after_core_update_body["data"]["source_selection_mode"],
                "EXPLICIT"
            );
            assert_eq!(
                config_after_core_update_body["data"]["warnings"],
                json!(["explicit_empty"])
            );

            let restore_config = send(
                &app_state,
                json_request(
                    Method::PUT,
                    &format!("/model/{model_id}/source-config"),
                    json!({
                        "source_selection_mode": "EXPLICIT",
                        "bindings": [{"source_id": 24102, "is_default": true}]
                    }),
                ),
            )
            .await;
            assert_eq!(restore_config.status(), StatusCode::OK);

            Provider::update(
                &test_db_context,
                provider.id,
                &UpdateProviderData {
                    provider_key: None,
                    name: None,
                    is_enabled: Some(false),
                    provider_api_key_mode: None,
                },
            )
            .await
            .expect("provider should be disabled");
            Model::update(
                &test_db_context,
                model_id,
                &crate::database::model::UpdateModelData {
                    model_name: None,
                    real_model_name: Some(None),
                    is_enabled: Some(false),
                    cost_catalog_id: Some(None),
                },
            )
            .await
            .expect("model should be disabled");

            let disabled_explain = send(
                &app_state,
                empty_request(
                    Method::GET,
                    &format!("/model/{model_id}/source-config/explain"),
                ),
            )
            .await;
            assert_eq!(disabled_explain.status(), StatusCode::OK);
            let disabled_explain_body = response_json(disabled_explain).await;
            assert_eq!(disabled_explain_body["data"]["provider_enabled"], false);
            assert_eq!(disabled_explain_body["data"]["model_enabled"], false);
            assert_eq!(
                disabled_explain_body["data"]["protocols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|protocol| protocol["selection_status"] == "selected")
                    .count(),
                4
            );
        })
        .await;
    }

    #[tokio::test]
    async fn source_config_rejects_cross_provider_binding_without_creating_model() {
        let test_db_context = TestDatabase::new_sqlite_default(
            "controller-model-source-config-ownership-http.sqlite",
        )
        .await;

        (async {
            let first_provider = seed_provider(&test_db_context, 24201, 24202).await;
            let second_provider = seed_provider(&test_db_context, 24203, 24204).await;
            let app_state = create_test_app_state(test_db_context.clone()).await;
            let response = send(
                &app_state,
                json_request(
                    Method::POST,
                    "/model",
                    json!({
                        "provider_id": first_provider.id,
                        "model_name": "cross-provider-model",
                        "real_model_name": null,
                        "model_kind": "CHAT",
                        "is_enabled": true,
                        "source_config": {
                            "source_selection_mode": "EXPLICIT",
                            "bindings": [{"source_id": second_provider.id + 1, "is_default": true}]
                        }
                    }),
                ),
            )
            .await;
            assert_ne!(response.status(), StatusCode::OK);
            assert!(
                Model::get_by_name_and_provider_id(
                    &test_db_context,
                    "cross-provider-model",
                    first_provider.id
                )
                .await
                .expect("model lookup should succeed")
                .is_none()
            );
        })
        .await;
    }
}
