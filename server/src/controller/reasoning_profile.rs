use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::{
    controller::BaseError,
    database::reasoning_profile::{
        ReasoningPatchFamily, ReasoningPreset, ReasoningProfile, ReasoningProfileModelBinding,
        ReasoningProfilePresetView, ReasoningProfileProviderBinding, ReasoningProfileWithPresets,
    },
    service::{
        admin::reasoning_profile::{
            CreateReasoningProfileInput, UpdateReasoningProfileInput,
            UpdateReasoningProfilePresetInput, UpsertReasoningProfilePresetInput,
        },
        app_state::{AppState, StateRouter, create_state_router},
    },
    utils::HttpResult,
};

#[derive(Debug, Serialize)]
struct ReasoningPresetMetadataResponse {
    preset_key: String,
    suffix: String,
    requires_reasoning: bool,
    allowed_operation_kinds: Vec<String>,
}

impl From<ReasoningPreset> for ReasoningPresetMetadataResponse {
    fn from(value: ReasoningPreset) -> Self {
        let metadata = value.metadata();
        Self {
            preset_key: metadata.preset_key,
            suffix: metadata.suffix,
            requires_reasoning: metadata.requires_reasoning,
            allowed_operation_kinds: metadata.allowed_operation_kinds,
        }
    }
}

#[derive(Debug, Serialize)]
struct ReasoningFamilyMetadataResponse {
    family_key: String,
    supported_presets: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ReasoningProfileCatalogResponse {
    families: Vec<ReasoningFamilyMetadataResponse>,
    presets: Vec<ReasoningPresetMetadataResponse>,
}

#[derive(Debug, Serialize)]
struct ReasoningProfilePresetResponse {
    id: i64,
    profile_id: i64,
    preset_key: String,
    suffix: String,
    requires_reasoning: bool,
    allowed_operation_kinds: Vec<String>,
    expose_in_models: bool,
    is_enabled: bool,
    created_at: i64,
    updated_at: i64,
}

impl From<ReasoningProfilePresetView> for ReasoningProfilePresetResponse {
    fn from(value: ReasoningProfilePresetView) -> Self {
        Self {
            id: value.preset.id,
            profile_id: value.preset.profile_id,
            preset_key: value.preset_key.as_key().to_string(),
            suffix: value.suffix,
            requires_reasoning: value.requires_reasoning,
            allowed_operation_kinds: value.allowed_operation_kinds,
            expose_in_models: value.preset.expose_in_models,
            is_enabled: value.preset.is_enabled,
            created_at: value.preset.created_at,
            updated_at: value.preset.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct ReasoningProfileResponse {
    profile: ReasoningProfile,
    family: String,
    presets: Vec<ReasoningProfilePresetResponse>,
    provider_bindings: Vec<ReasoningProfileProviderBinding>,
    model_bindings: Vec<ReasoningProfileModelBinding>,
}

impl ReasoningProfileResponse {
    fn from_profile(value: ReasoningProfileWithPresets) -> Result<Self, BaseError> {
        let profile_id = value.profile.id;
        Ok(Self {
            family: value.family.as_key().to_string(),
            presets: value
                .presets
                .into_iter()
                .map(ReasoningProfilePresetResponse::from)
                .collect(),
            provider_bindings: ReasoningProfile::list_provider_bindings(profile_id)?,
            model_bindings: ReasoningProfile::list_model_bindings(profile_id)?,
            profile: value.profile,
        })
    }
}

#[derive(Debug, Deserialize)]
struct CreateReasoningProfilePayload {
    profile_key: String,
    name: String,
    description: Option<String>,
    family_key: String,
    is_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateReasoningProfilePayload {
    profile_key: Option<String>,
    name: Option<String>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    description: Option<Option<String>>,
    family_key: Option<String>,
    is_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpsertReasoningProfilePresetPayload {
    preset_key: String,
    expose_in_models: Option<bool>,
    is_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateReasoningProfilePresetPayload {
    preset_key: Option<String>,
    expose_in_models: Option<bool>,
    is_enabled: Option<bool>,
}

async fn get_catalog() -> Result<HttpResult<ReasoningProfileCatalogResponse>, BaseError> {
    Ok(HttpResult::new(ReasoningProfileCatalogResponse {
        families: ReasoningPatchFamily::ALL
            .into_iter()
            .map(|family| ReasoningFamilyMetadataResponse {
                family_key: family.as_key().to_string(),
                supported_presets: family
                    .supported_presets()
                    .into_iter()
                    .map(|preset| preset.as_key().to_string())
                    .collect(),
            })
            .collect(),
        presets: ReasoningPreset::ALL
            .into_iter()
            .map(ReasoningPresetMetadataResponse::from)
            .collect(),
    }))
}

async fn list_profiles(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<Vec<ReasoningProfileResponse>>, BaseError> {
    let items = app_state
        .admin
        .reasoning_profile
        .list_profiles()?
        .into_iter()
        .map(ReasoningProfileResponse::from_profile)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(HttpResult::new(items))
}

async fn get_profile(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ReasoningProfileResponse>, BaseError> {
    Ok(HttpResult::new(ReasoningProfileResponse::from_profile(
        app_state.admin.reasoning_profile.get_profile(id)?,
    )?))
}

async fn create_profile(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateReasoningProfilePayload>,
) -> Result<HttpResult<ReasoningProfileResponse>, BaseError> {
    let profile = app_state
        .admin
        .reasoning_profile
        .create_profile(CreateReasoningProfileInput {
            profile_key: payload.profile_key,
            name: payload.name,
            description: payload.description,
            family_key: payload.family_key,
            is_enabled: payload.is_enabled.unwrap_or(true),
        })
        .await?;
    Ok(HttpResult::new(ReasoningProfileResponse::from_profile(
        profile,
    )?))
}

async fn update_profile(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateReasoningProfilePayload>,
) -> Result<HttpResult<ReasoningProfileResponse>, BaseError> {
    let profile = app_state
        .admin
        .reasoning_profile
        .update_profile(
            id,
            UpdateReasoningProfileInput {
                profile_key: payload.profile_key,
                name: payload.name,
                description: payload.description,
                family_key: payload.family_key,
                is_enabled: payload.is_enabled,
            },
        )
        .await?;
    Ok(HttpResult::new(ReasoningProfileResponse::from_profile(
        profile,
    )?))
}

async fn delete_profile(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    app_state.admin.reasoning_profile.delete_profile(id).await?;
    Ok(HttpResult::new(()))
}

async fn upsert_profile_preset(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpsertReasoningProfilePresetPayload>,
) -> Result<HttpResult<ReasoningProfileResponse>, BaseError> {
    let profile = app_state
        .admin
        .reasoning_profile
        .upsert_profile_preset(
            id,
            UpsertReasoningProfilePresetInput {
                preset_key: payload.preset_key,
                expose_in_models: payload.expose_in_models.unwrap_or(true),
                is_enabled: payload.is_enabled.unwrap_or(true),
            },
        )
        .await?;
    Ok(HttpResult::new(ReasoningProfileResponse::from_profile(
        profile,
    )?))
}

async fn update_profile_preset(
    State(app_state): State<Arc<AppState>>,
    Path((profile_id, preset_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateReasoningProfilePresetPayload>,
) -> Result<HttpResult<ReasoningProfileResponse>, BaseError> {
    let profile = app_state
        .admin
        .reasoning_profile
        .update_profile_preset(
            profile_id,
            preset_id,
            UpdateReasoningProfilePresetInput {
                preset_key: payload.preset_key,
                expose_in_models: payload.expose_in_models,
                is_enabled: payload.is_enabled,
            },
        )
        .await?;
    Ok(HttpResult::new(ReasoningProfileResponse::from_profile(
        profile,
    )?))
}

async fn delete_profile_preset(
    State(app_state): State<Arc<AppState>>,
    Path((profile_id, preset_id)): Path<(i64, i64)>,
) -> Result<HttpResult<ReasoningProfileResponse>, BaseError> {
    let profile = app_state
        .admin
        .reasoning_profile
        .delete_profile_preset(profile_id, preset_id)
        .await?;
    Ok(HttpResult::new(ReasoningProfileResponse::from_profile(
        profile,
    )?))
}

pub fn create_reasoning_profile_router() -> StateRouter {
    create_state_router()
        .route("/reasoning_profile", post(create_profile))
        .nest(
            "/reasoning_profile",
            create_state_router()
                .route("/catalog", get(get_catalog))
                .route("/list", get(list_profiles))
                .route(
                    "/{id}",
                    get(get_profile).put(update_profile).delete(delete_profile),
                )
                .route("/{id}/preset", post(upsert_profile_preset))
                .route(
                    "/{profile_id}/preset/{preset_id}",
                    put(update_profile_preset).delete(delete_profile_preset),
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
    use serde_json::{Value, json};
    use tower::util::ServiceExt;

    use crate::database::TestDbContext;
    use crate::service::app_state::{AppState, create_test_app_state};

    use super::create_reasoning_profile_router;

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_reasoning_profile_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("reasoning profile router should respond")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

    #[tokio::test]
    async fn reasoning_profile_http_crud_exposes_derived_preset_metadata() {
        let test_db_context = TestDbContext::new_sqlite("controller-reasoning-profile-http.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let create_response = send(
                    &app_state,
                    Request::builder()
                        .method(Method::POST)
                        .uri("/reasoning_profile")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "profile_key": "openai_responses_reasoning",
                                "name": "OpenAI Responses Reasoning",
                                "family_key": "openai_responses_reasoning",
                                "is_enabled": true
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await;
                assert_eq!(create_response.status(), StatusCode::OK);
                let create_body = response_json(create_response).await;
                let profile_id = create_body["data"]["profile"]["id"]
                    .as_i64()
                    .expect("profile id should be present");

                let preset_response = send(
                    &app_state,
                    Request::builder()
                        .method(Method::POST)
                        .uri(format!("/reasoning_profile/{profile_id}/preset"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "preset_key": "high",
                                "expose_in_models": true,
                                "is_enabled": true
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await;
                assert_eq!(preset_response.status(), StatusCode::OK);
                let preset_body = response_json(preset_response).await;
                assert_eq!(preset_body["data"]["presets"][0]["preset_key"], "high");
                assert_eq!(preset_body["data"]["presets"][0]["suffix"], "high");
                assert_eq!(
                    preset_body["data"]["presets"][0]["allowed_operation_kinds"][0],
                    "generation"
                );

                let unsupported_response = send(
                    &app_state,
                    Request::builder()
                        .method(Method::POST)
                        .uri(format!("/reasoning_profile/{profile_id}/preset"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "preset_key": "auto",
                                "expose_in_models": true,
                                "is_enabled": true
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await;
                assert_eq!(unsupported_response.status(), StatusCode::BAD_REQUEST);
            })
            .await;
    }

    #[tokio::test]
    async fn reasoning_profile_http_rejects_family_change_with_enabled_unsupported_preset() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-reasoning-profile-family-change.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let create_response = send(
                    &app_state,
                    Request::builder()
                        .method(Method::POST)
                        .uri("/reasoning_profile")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "profile_key": "gemini_budget_reasoning",
                                "name": "Gemini Budget Reasoning",
                                "family_key": "gemini25_thinking_budget",
                                "is_enabled": true
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await;
                assert_eq!(create_response.status(), StatusCode::OK);
                let create_body = response_json(create_response).await;
                let profile_id = create_body["data"]["profile"]["id"]
                    .as_i64()
                    .expect("profile id should be present");

                let preset_response = send(
                    &app_state,
                    Request::builder()
                        .method(Method::POST)
                        .uri(format!("/reasoning_profile/{profile_id}/preset"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "preset_key": "auto",
                                "expose_in_models": true,
                                "is_enabled": true
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await;
                assert_eq!(preset_response.status(), StatusCode::OK);
                let preset_body = response_json(preset_response).await;
                let preset_id = preset_body["data"]["presets"][0]["id"]
                    .as_i64()
                    .expect("preset id should be present");

                let rejected_response = send(
                    &app_state,
                    Request::builder()
                        .method(Method::PUT)
                        .uri(format!("/reasoning_profile/{profile_id}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "family_key": "anthropic_thinking_budget"
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await;
                assert_eq!(rejected_response.status(), StatusCode::BAD_REQUEST);
                let rejected_body = response_json(rejected_response).await;
                let message = rejected_body["msg"]
                    .as_str()
                    .expect("error response should include msg");
                assert!(message.contains("gemini_budget_reasoning"));
                assert!(message.contains("anthropic_thinking_budget"));
                assert!(message.contains("auto"));

                let disable_response = send(
                    &app_state,
                    Request::builder()
                        .method(Method::PUT)
                        .uri(format!(
                            "/reasoning_profile/{profile_id}/preset/{preset_id}"
                        ))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "is_enabled": false
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await;
                assert_eq!(disable_response.status(), StatusCode::OK);

                let accepted_response = send(
                    &app_state,
                    Request::builder()
                        .method(Method::PUT)
                        .uri(format!("/reasoning_profile/{profile_id}"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "family_key": "anthropic_thinking_budget"
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await;
                assert_eq!(accepted_response.status(), StatusCode::OK);
                let accepted_body = response_json(accepted_response).await;
                assert_eq!(accepted_body["data"]["family"], "anthropic_thinking_budget");
                assert_eq!(accepted_body["data"]["presets"][0]["preset_key"], "auto");
                assert_eq!(accepted_body["data"]["presets"][0]["is_enabled"], false);
            })
            .await;
    }
}
