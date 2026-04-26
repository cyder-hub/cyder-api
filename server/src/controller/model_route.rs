use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Serialize;
use std::sync::Arc;

use crate::{
    controller::BaseError,
    database::model_route::{
        CreateModelRoutePayload, ModelRoute, ModelRouteDetail, ModelRouteListItem,
        UpdateModelRoutePayload,
    },
    database::reasoning_profile::ReasoningPreset,
    proxy::{candidate_supports_reasoning_preset, resolve_route_runtime_candidates},
    service::{
        app_state::{AppState, StateRouter, create_state_router},
        cache::types::{CacheModelRoute, CacheModelsCatalog},
    },
    utils::HttpResult,
};

#[derive(Debug, Serialize)]
struct ModelRouteReasoningCandidatePreview {
    candidate_position: usize,
    runtime_status: String,
    provider_id: Option<i64>,
    provider_key: Option<String>,
    model_id: i64,
    model_name: Option<String>,
    preset_key: String,
    suffix: String,
    supported: bool,
    reason: Option<String>,
    reasoning_profile_id: Option<i64>,
    reasoning_profile_key: Option<String>,
    reasoning_profile_preset_id: Option<i64>,
    reasoning_family: Option<String>,
}

#[derive(Debug, Serialize)]
struct ModelRouteReasoningPresetPreview {
    preset_key: String,
    suffix: String,
    requires_reasoning: bool,
    allowed_operation_kinds: Vec<String>,
    stable: bool,
    reason: Option<String>,
    candidates: Vec<ModelRouteReasoningCandidatePreview>,
}

#[derive(Debug, Serialize)]
struct ModelRouteReasoningPreviewResponse {
    route_id: i64,
    route_name: String,
    presets: Vec<ModelRouteReasoningPresetPreview>,
}

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

async fn preview_model_route_reasoning(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ModelRouteReasoningPreviewResponse>, BaseError> {
    let catalog = app_state.catalog.get_models_catalog().await?;
    let route = catalog
        .routes
        .iter()
        .find(|route| route.id == id)
        .ok_or_else(|| BaseError::NotFound(Some(format!("Model route {} not found", id))))?;

    Ok(HttpResult::new(build_model_route_reasoning_preview(
        catalog.as_ref(),
        route,
    )))
}

fn build_model_route_reasoning_preview(
    catalog: &CacheModelsCatalog,
    route: &CacheModelRoute,
) -> ModelRouteReasoningPreviewResponse {
    let runtime_resolutions = resolve_route_runtime_candidates(catalog, &route.route_name, route);

    let presets = ReasoningPreset::ALL
        .into_iter()
        .map(|preset| {
            let metadata = preset.metadata();
            let mut valid_candidate_count = 0usize;
            let candidates = match &runtime_resolutions {
                Ok(resolutions) => resolutions
                    .iter()
                    .map(|runtime_candidate| {
                        let Some(candidate) = runtime_candidate.candidate.as_ref() else {
                            let model = catalog.models.iter().find(|model| {
                                model.id == runtime_candidate.route_candidate.model_id
                            });
                            let provider = model
                                .and_then(|model| {
                                    catalog
                                        .providers
                                        .iter()
                                        .find(|provider| provider.id == model.provider_id)
                                })
                                .or_else(|| {
                                    catalog.providers.iter().find(|provider| {
                                        provider.id == runtime_candidate.route_candidate.provider_id
                                    })
                                });
                            return ModelRouteReasoningCandidatePreview {
                                candidate_position: runtime_candidate.route_candidate_position,
                                runtime_status: runtime_candidate.runtime_status_key().to_string(),
                                provider_id: model
                                    .map(|model| model.provider_id)
                                    .or(Some(runtime_candidate.route_candidate.provider_id)),
                                provider_key: provider
                                    .map(|provider| provider.provider_key.clone()),
                                model_id: runtime_candidate.route_candidate.model_id,
                                model_name: model.map(|model| model.model_name.clone()),
                                preset_key: preset.as_key().to_string(),
                                suffix: preset.canonical_suffix().to_string(),
                                supported: false,
                                reason: runtime_candidate.stale_reason.as_ref().map(|reason| {
                                    format!("stale candidate skipped by runtime: {reason}")
                                }),
                                reasoning_profile_id: None,
                                reasoning_profile_key: None,
                                reasoning_profile_preset_id: None,
                                reasoning_family: None,
                            };
                        };

                        valid_candidate_count += 1;
                        match candidate_supports_reasoning_preset(catalog, candidate, preset) {
                            Ok(binding) => ModelRouteReasoningCandidatePreview {
                                candidate_position: runtime_candidate.route_candidate_position,
                                runtime_status: runtime_candidate.runtime_status_key().to_string(),
                                provider_id: Some(candidate.provider.id),
                                provider_key: Some(candidate.provider.provider_key.clone()),
                                model_id: candidate.model.id,
                                model_name: Some(candidate.model.model_name.clone()),
                                preset_key: preset.as_key().to_string(),
                                suffix: binding.suffix,
                                supported: true,
                                reason: None,
                                reasoning_profile_id: Some(binding.profile_id),
                                reasoning_profile_key: Some(binding.profile_key),
                                reasoning_profile_preset_id: Some(binding.profile_preset_id),
                                reasoning_family: Some(binding.family.as_key().to_string()),
                            },
                            Err(reason) => ModelRouteReasoningCandidatePreview {
                                candidate_position: runtime_candidate.route_candidate_position,
                                runtime_status: runtime_candidate.runtime_status_key().to_string(),
                                provider_id: Some(candidate.provider.id),
                                provider_key: Some(candidate.provider.provider_key.clone()),
                                model_id: candidate.model.id,
                                model_name: Some(candidate.model.model_name.clone()),
                                preset_key: preset.as_key().to_string(),
                                suffix: preset.canonical_suffix().to_string(),
                                supported: false,
                                reason: Some(reason),
                                reasoning_profile_id: None,
                                reasoning_profile_key: None,
                                reasoning_profile_preset_id: None,
                                reasoning_family: None,
                            },
                        }
                    })
                    .collect::<Vec<_>>(),
                Err(_) => Vec::new(),
            };
            let stable = valid_candidate_count > 0
                && candidates
                    .iter()
                    .filter(|candidate| candidate.runtime_status == "valid")
                    .all(|candidate| candidate.supported);
            let reason = match &runtime_resolutions {
                Err(error) => Some(error.clone()),
                Ok(_) if valid_candidate_count == 0 => {
                    Some("route has no runtime-valid candidates".to_string())
                }
                Ok(_) if stable => None,
                Ok(_) => Some(
                    "one or more runtime-valid candidates do not support this preset".to_string(),
                ),
            };

            ModelRouteReasoningPresetPreview {
                preset_key: metadata.preset_key,
                suffix: metadata.suffix,
                requires_reasoning: metadata.requires_reasoning,
                allowed_operation_kinds: metadata.allowed_operation_kinds,
                stable,
                reason,
                candidates,
            }
        })
        .collect();

    ModelRouteReasoningPreviewResponse {
        route_id: route.id,
        route_name: route.route_name.clone(),
        presets,
    }
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
                    "/{id}/reasoning_preview",
                    get(preview_model_route_reasoning),
                )
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
    use crate::database::reasoning_profile::{ReasoningPatchFamily, ReasoningPreset};
    use crate::schema::enum_def::{Action, ProviderApiKeyMode, ProviderType};
    use crate::service::{
        app_state::{AppState, create_test_app_state},
        cache::types::{
            CacheApiKeyModelOverride, CacheModel, CacheModelRoute, CacheModelRouteCandidate,
            CacheModelsCatalog, CacheProvider, CacheReasoningProfile, CacheReasoningProfilePreset,
        },
    };

    use super::{build_model_route_reasoning_preview, create_model_route_router};

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
            default_reasoning_profile_id: None,
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

    fn cache_provider(
        id: i64,
        provider_key: &str,
        default_reasoning_profile_id: Option<i64>,
    ) -> CacheProvider {
        CacheProvider {
            id,
            provider_key: provider_key.to_string(),
            name: provider_key.to_string(),
            endpoint: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            default_reasoning_profile_id,
            is_enabled: true,
        }
    }

    fn cache_model(id: i64, provider_id: i64, model_name: &str) -> CacheModel {
        CacheModel {
            id,
            provider_id,
            model_name: model_name.to_string(),
            real_model_name: None,
            cost_catalog_id: None,
            reasoning_profile_override_id: None,
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: true,
            supports_image_input: true,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled: true,
        }
    }

    fn cache_route(candidates: &[(i64, i64, i32, bool)]) -> CacheModelRoute {
        CacheModelRoute {
            id: 9100,
            route_name: "smart-route".to_string(),
            description: None,
            is_enabled: true,
            expose_in_models: true,
            candidates: candidates
                .iter()
                .map(
                    |(model_id, provider_id, priority, is_enabled)| CacheModelRouteCandidate {
                        route_id: 9100,
                        model_id: *model_id,
                        provider_id: *provider_id,
                        priority: *priority,
                        is_enabled: *is_enabled,
                    },
                )
                .collect(),
        }
    }

    fn cache_reasoning_profile(
        id: i64,
        family: ReasoningPatchFamily,
        presets: &[ReasoningPreset],
    ) -> CacheReasoningProfile {
        CacheReasoningProfile {
            id,
            profile_key: "openai-chat".to_string(),
            name: "OpenAI Chat".to_string(),
            description: None,
            family,
            is_enabled: true,
            presets: presets
                .iter()
                .enumerate()
                .map(|(index, preset)| CacheReasoningProfilePreset {
                    id: id * 10 + index as i64,
                    profile_id: id,
                    preset: *preset,
                    suffix: preset.canonical_suffix().to_string(),
                    requires_reasoning: preset.requires_reasoning(),
                    expose_in_models: true,
                    is_enabled: true,
                })
                .collect(),
        }
    }

    fn preview_catalog(
        providers: Vec<CacheProvider>,
        models: Vec<CacheModel>,
        route: CacheModelRoute,
        reasoning_profiles: Vec<CacheReasoningProfile>,
    ) -> CacheModelsCatalog {
        CacheModelsCatalog {
            providers,
            models,
            routes: vec![route],
            api_key_overrides: Vec::<CacheApiKeyModelOverride>::new(),
            reasoning_profiles,
        }
    }

    fn find_preset<'a>(
        preview: &'a super::ModelRouteReasoningPreviewResponse,
        preset: ReasoningPreset,
    ) -> &'a super::ModelRouteReasoningPresetPreview {
        preview
            .presets
            .iter()
            .find(|item| item.preset_key == preset.as_key())
            .expect("preset preview should exist")
    }

    #[test]
    fn create_model_route_router_registers_routes() {
        let _router = create_model_route_router();
    }

    #[test]
    fn reasoning_preview_skips_stale_candidate_for_stability_but_keeps_diagnostic() {
        let route = cache_route(&[(10, 1, 10, true), (999, 999, 20, true)]);
        let catalog = preview_catalog(
            vec![cache_provider(1, "openai", Some(900))],
            vec![cache_model(10, 1, "gpt-primary")],
            route,
            vec![cache_reasoning_profile(
                900,
                ReasoningPatchFamily::OpenAiChatReasoningEffort,
                &[ReasoningPreset::High],
            )],
        );

        let preview = build_model_route_reasoning_preview(&catalog, &catalog.routes[0]);
        let high = find_preset(&preview, ReasoningPreset::High);

        assert!(high.stable);
        assert_eq!(high.candidates.len(), 2);
        assert_eq!(high.candidates[0].runtime_status, "valid");
        assert!(high.candidates[0].supported);
        assert_eq!(high.candidates[1].runtime_status, "stale_skipped");
        assert!(!high.candidates[1].supported);
        assert!(
            high.candidates[1]
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("stale candidate skipped by runtime")
        );
    }

    #[test]
    fn reasoning_preview_marks_valid_unsupported_candidate_unstable() {
        let route = cache_route(&[(10, 1, 10, true), (20, 2, 20, true)]);
        let catalog = preview_catalog(
            vec![
                cache_provider(1, "openai-a", Some(900)),
                cache_provider(2, "openai-b", None),
            ],
            vec![
                cache_model(10, 1, "gpt-primary"),
                cache_model(20, 2, "gpt-secondary"),
            ],
            route,
            vec![cache_reasoning_profile(
                900,
                ReasoningPatchFamily::OpenAiChatReasoningEffort,
                &[ReasoningPreset::High],
            )],
        );

        let preview = build_model_route_reasoning_preview(&catalog, &catalog.routes[0]);
        let high = find_preset(&preview, ReasoningPreset::High);

        assert!(!high.stable);
        assert_eq!(
            high.reason.as_deref(),
            Some("one or more runtime-valid candidates do not support this preset")
        );
        assert_eq!(high.candidates.len(), 2);
        assert_eq!(high.candidates[0].runtime_status, "valid");
        assert!(high.candidates[0].supported);
        assert_eq!(high.candidates[1].runtime_status, "valid");
        assert!(!high.candidates[1].supported);
        assert!(
            high.candidates[1]
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("does not have an enabled reasoning profile")
        );
    }

    #[test]
    fn reasoning_preview_marks_all_presets_unstable_when_route_has_no_valid_candidates() {
        let route = cache_route(&[(999, 999, 10, true)]);
        let catalog = preview_catalog(
            vec![cache_provider(1, "openai", Some(900))],
            vec![],
            route,
            vec![cache_reasoning_profile(
                900,
                ReasoningPatchFamily::OpenAiChatReasoningEffort,
                &[ReasoningPreset::High],
            )],
        );

        let preview = build_model_route_reasoning_preview(&catalog, &catalog.routes[0]);

        assert!(preview.presets.iter().all(|preset| !preset.stable));
        for preset in preview.presets {
            assert_eq!(
                preset.reason.as_deref(),
                Some("route has no runtime-valid candidates")
            );
            assert_eq!(preset.candidates.len(), 1);
            assert_eq!(preset.candidates[0].runtime_status, "stale_skipped");
        }
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
