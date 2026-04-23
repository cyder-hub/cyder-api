use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use axum::{body::Body, response::Response};
use serde::Serialize;

use super::{ProxyError, auth::admit_api_key_request};
use crate::{
    schema::enum_def::{LlmApiType, ProviderType},
    service::{
        app_state::AppState,
        cache::types::{
            CacheApiKey, CacheApiKeyModelOverride, CacheModel, CacheModelRoute, CacheModelsCatalog,
            CacheProvider,
        },
    },
    utils::acl::ACL_EVALUATOR,
};
use cyder_tools::log::{debug, error, warn};

#[derive(Debug)]
pub(super) struct AccessibleModel {
    pub id: String,
    pub owned_by: String,
    pub provider_type: ProviderType,
}

pub(super) async fn get_accessible_models(
    app_state: &Arc<AppState>,
    api_key: &CacheApiKey,
) -> Result<Vec<AccessibleModel>, ProxyError> {
    debug!("Fetching accessible models for API key ID: {}", api_key.id);

    let catalog = app_state.get_models_catalog().await.map_err(|store_err| {
        error!("Failed to fetch models catalog from cache: {:?}", store_err);
        ProxyError::InternalError("Failed to retrieve models catalog".to_string())
    })?;

    let available_models = collect_accessible_models(catalog.as_ref(), api_key);

    debug!(
        "Total accessible models (including routes and overrides): {}",
        available_models.len()
    );

    Ok(available_models)
}

pub(super) async fn execute_models_listing(
    app_state: Arc<AppState>,
    api_key: Arc<CacheApiKey>,
    api_type: LlmApiType,
) -> Result<Response<Body>, ProxyError> {
    let _api_key_concurrency_guard =
        admit_api_key_request(&app_state, &api_key)
            .await
            .map_err(|e| {
                error!("API key request admission failed for /models: {:?}", e);
                e
            })?;
    let accessible_models = get_accessible_models(&app_state, &api_key).await?;
    let response_body = render_models_response(api_type, &accessible_models)?;

    Ok(Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Body::from(response_body))
        .unwrap())
}

// --- Structs for /models endpoint response ---
#[derive(Serialize, Debug)]
pub(super) struct ModelListResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

#[derive(Serialize, Debug)]
pub(super) struct ModelInfo {
    pub id: String, // model.model_name
    pub object: String,
    pub owned_by: String, // provider.provider_key
}

// --- Structs for Gemini /models endpoint response ---
#[derive(Serialize, Debug)]
pub(super) struct GeminiModelListResponse {
    pub models: Vec<GeminiModelInfo>,
}

#[derive(Serialize, Debug)]
pub(super) struct GeminiModelInfo {
    pub name: String,
}

fn render_models_response(
    api_type: LlmApiType,
    accessible_models: &[AccessibleModel],
) -> Result<String, ProxyError> {
    match api_type {
        LlmApiType::Gemini => {
            let models = accessible_models
                .iter()
                .map(|m| GeminiModelInfo {
                    name: format!("models/{}", m.id),
                })
                .collect();
            serde_json::to_string(&GeminiModelListResponse { models }).map_err(|e| {
                ProxyError::InternalError(format!("Failed to serialize Gemini models list: {}", e))
            })
        }
        LlmApiType::Ollama => {
            let models = accessible_models
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "name": m.id,
                        "model": m.id,
                        "modified_at": "",
                        "size": 0,
                        "digest": "",
                        "details": {
                            "format": "",
                            "family": "",
                            "families": null,
                            "parameter_size": "",
                            "quantization_level": ""
                        }
                    })
                })
                .collect::<Vec<_>>();
            serde_json::to_string(&serde_json::json!({ "models": models })).map_err(|e| {
                ProxyError::InternalError(format!("Failed to serialize Ollama models list: {}", e))
            })
        }
        _ => {
            let data = accessible_models
                .iter()
                .map(|m| ModelInfo {
                    id: m.id.clone(),
                    object: "model".to_string(),
                    owned_by: m.owned_by.clone(),
                })
                .collect();
            serde_json::to_string(&ModelListResponse {
                object: "list".to_string(),
                data,
            })
            .map_err(|e| {
                ProxyError::InternalError(format!("Failed to serialize models list: {}", e))
            })
        }
    }
}

fn collect_accessible_models(
    catalog: &CacheModelsCatalog,
    api_key: &CacheApiKey,
) -> Vec<AccessibleModel> {
    let providers_by_id = catalog
        .providers
        .iter()
        .filter(|provider| provider.is_enabled)
        .map(|provider| (provider.id, provider))
        .collect::<HashMap<_, _>>();
    let models_by_id = catalog
        .models
        .iter()
        .filter(|model| model.is_enabled)
        .map(|model| (model.id, model))
        .collect::<HashMap<_, _>>();

    let mut available_models = Vec::new();
    let mut seen_ids = HashSet::new();

    for provider in catalog
        .providers
        .iter()
        .filter(|provider| provider.is_enabled)
    {
        let mut provider_models = catalog
            .models
            .iter()
            .filter(|model| model.is_enabled && model.provider_id == provider.id)
            .collect::<Vec<_>>();
        provider_models.sort_by(|left, right| left.model_name.cmp(&right.model_name));

        for model in provider_models {
            if is_model_allowed(api_key, provider, model) {
                push_unique_accessible_model(
                    &mut available_models,
                    &mut seen_ids,
                    AccessibleModel {
                        id: format!("{}/{}", provider.provider_key, model.model_name),
                        owned_by: provider.provider_key.clone(),
                        provider_type: provider.provider_type.clone(),
                    },
                    "direct",
                );
            }
        }
    }

    debug!(
        "Found {} accessible models from providers",
        available_models.len()
    );

    let mut routes = catalog
        .routes
        .iter()
        .filter(|route| route.is_enabled && route.expose_in_models)
        .collect::<Vec<_>>();
    routes.sort_by(|left, right| left.route_name.cmp(&right.route_name));

    for route in routes {
        let Some(accessible_model) =
            build_route_accessible_model(route, &providers_by_id, &models_by_id, api_key)
        else {
            continue;
        };
        push_unique_accessible_model(
            &mut available_models,
            &mut seen_ids,
            accessible_model,
            "route",
        );
    }

    let mut overrides = catalog
        .api_key_overrides
        .iter()
        .filter(|override_row| override_row.is_enabled && override_row.api_key_id == api_key.id)
        .collect::<Vec<_>>();
    overrides.sort_by(|left, right| left.source_name.cmp(&right.source_name));

    for override_row in overrides {
        let Some(route) = catalog
            .routes
            .iter()
            .find(|route| route.id == override_row.target_route_id)
        else {
            continue;
        };
        let Some(accessible_model) = build_override_accessible_model(
            override_row,
            route,
            &providers_by_id,
            &models_by_id,
            api_key,
        ) else {
            continue;
        };
        push_unique_accessible_model(
            &mut available_models,
            &mut seen_ids,
            accessible_model,
            "override",
        );
    }

    available_models
}

fn push_unique_accessible_model(
    available_models: &mut Vec<AccessibleModel>,
    seen_ids: &mut HashSet<String>,
    accessible_model: AccessibleModel,
    source_kind: &str,
) {
    if !seen_ids.insert(accessible_model.id.clone()) {
        warn!(
            "Skipping duplicate /models id '{}' from {} entry; preserving earlier entry",
            accessible_model.id, source_kind
        );
        return;
    }

    available_models.push(accessible_model);
}

fn select_first_accessible_route_candidate<'a>(
    route: &'a CacheModelRoute,
    models_by_id: &'a HashMap<i64, &'a CacheModel>,
    providers_by_id: &'a HashMap<i64, &'a CacheProvider>,
    api_key: &CacheApiKey,
) -> Option<&'a CacheModel> {
    route
        .candidates
        .iter()
        .filter(|candidate| candidate.is_enabled)
        .find_map(|candidate| {
            let model = models_by_id.get(&candidate.model_id).copied()?;
            let provider = providers_by_id.get(&model.provider_id).copied()?;
            is_model_allowed(api_key, provider, model).then_some(model)
        })
}

fn build_route_accessible_model(
    route: &CacheModelRoute,
    providers_by_id: &HashMap<i64, &CacheProvider>,
    models_by_id: &HashMap<i64, &CacheModel>,
    api_key: &CacheApiKey,
) -> Option<AccessibleModel> {
    let model =
        select_first_accessible_route_candidate(route, models_by_id, providers_by_id, api_key)?;
    let provider = providers_by_id.get(&model.provider_id)?;

    Some(AccessibleModel {
        id: route.route_name.clone(),
        owned_by: "cyder-api".to_string(),
        provider_type: provider.provider_type.clone(),
    })
}

fn build_override_accessible_model(
    override_row: &CacheApiKeyModelOverride,
    route: &CacheModelRoute,
    providers_by_id: &HashMap<i64, &CacheProvider>,
    models_by_id: &HashMap<i64, &CacheModel>,
    api_key: &CacheApiKey,
) -> Option<AccessibleModel> {
    let model =
        select_first_accessible_route_candidate(route, models_by_id, providers_by_id, api_key)?;
    let provider = providers_by_id.get(&model.provider_id)?;

    Some(AccessibleModel {
        id: override_row.source_name.clone(),
        owned_by: "cyder-api".to_string(),
        provider_type: provider.provider_type.clone(),
    })
}

fn is_model_allowed(api_key: &CacheApiKey, provider: &CacheProvider, model: &CacheModel) -> bool {
    match ACL_EVALUATOR.authorize(
        &api_key.name,
        &api_key.default_action,
        &api_key.acl_rules,
        provider.id,
        model.id,
    ) {
        Ok(_) => true,
        Err(reason) => {
            debug!(
                "Model {}/{} denied for ApiKey ID {}. Reason: {}",
                provider.provider_key, model.model_name, api_key.id, reason
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_accessible_models, render_models_response};
    use crate::schema::enum_def::{Action, LlmApiType, ProviderApiKeyMode, ProviderType};
    use crate::service::cache::types::{
        CacheApiKey, CacheApiKeyAclRule, CacheApiKeyModelOverride, CacheModel, CacheModelRoute,
        CacheModelRouteCandidate, CacheModelsCatalog, CacheProvider,
    };

    fn provider(id: i64, provider_key: &str, is_enabled: bool) -> CacheProvider {
        CacheProvider {
            id,
            provider_key: provider_key.to_string(),
            name: provider_key.to_string(),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled,
        }
    }

    fn model(id: i64, provider_id: i64, model_name: &str, is_enabled: bool) -> CacheModel {
        CacheModel {
            id,
            provider_id,
            model_name: model_name.to_string(),
            real_model_name: None,
            cost_catalog_id: None,
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: true,
            supports_image_input: true,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled,
        }
    }

    fn api_key(default_action: Action, acl_rules: Vec<CacheApiKeyAclRule>) -> CacheApiKey {
        CacheApiKey {
            id: 1,
            api_key_hash: "hash".to_string(),
            key_prefix: "cyder-prefix".to_string(),
            key_last4: "1234".to_string(),
            name: "test-key".to_string(),
            description: None,
            default_action,
            is_enabled: true,
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
            acl_rules,
        }
    }

    #[test]
    fn collects_direct_models_from_cached_catalog_and_preserves_ordering() {
        let catalog = CacheModelsCatalog {
            providers: vec![
                provider(1, "provider-b", true),
                provider(2, "provider-a", true),
            ],
            models: vec![
                model(11, 1, "z-model", true),
                model(12, 1, "a-model", true),
                model(21, 2, "middle", true),
            ],
            routes: vec![],
            api_key_overrides: vec![],
        };

        let models = collect_accessible_models(&catalog, &api_key(Action::Allow, vec![]));

        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "provider-b/a-model".to_string(),
                "provider-b/z-model".to_string(),
                "provider-a/middle".to_string(),
            ]
        );
    }

    #[test]
    fn skips_inactive_provider_and_model_direct_entries() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "active", true), provider(2, "inactive", false)],
            models: vec![
                model(11, 1, "active-model", true),
                model(12, 1, "disabled-model", false),
                model(21, 2, "hidden-by-provider", true),
            ],
            routes: vec![],
            api_key_overrides: vec![],
        };

        let models = collect_accessible_models(&catalog, &api_key(Action::Allow, vec![]));
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(ids, vec!["active/active-model".to_string()]);
    }

    #[test]
    fn filters_direct_models_with_access_control() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "provider", true)],
            models: vec![model(11, 1, "allowed", true), model(12, 1, "denied", true)],
            routes: vec![],
            api_key_overrides: vec![],
        };
        let api_key = api_key(
            Action::Deny,
            vec![CacheApiKeyAclRule {
                id: 1,
                effect: Action::Allow,
                priority: 1,
                scope: crate::schema::enum_def::RuleScope::Model,
                provider_id: Some(1),
                model_id: Some(11),
                is_enabled: true,
                description: None,
            }],
        );

        let models = collect_accessible_models(&catalog, &api_key);
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(ids, vec!["provider/allowed".to_string()]);
    }

    #[test]
    fn exposes_route_and_override_names_when_primary_candidate_is_allowed() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "provider", true)],
            models: vec![model(11, 1, "allowed", true)],
            routes: vec![CacheModelRoute {
                id: 200,
                route_name: "manual-smoke-route".to_string(),
                description: None,
                is_enabled: true,
                expose_in_models: true,
                candidates: vec![CacheModelRouteCandidate {
                    route_id: 200,
                    model_id: 11,
                    provider_id: 1,
                    priority: 0,
                    is_enabled: true,
                }],
            }],
            api_key_overrides: vec![CacheApiKeyModelOverride {
                id: 1,
                api_key_id: 1,
                source_name: "manual-cli-model".to_string(),
                target_route_id: 200,
                description: None,
                is_enabled: true,
            }],
        };

        let models = collect_accessible_models(&catalog, &api_key(Action::Allow, vec![]));
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "provider/allowed".to_string(),
                "manual-smoke-route".to_string(),
                "manual-cli-model".to_string(),
            ]
        );
    }

    #[test]
    fn exposes_route_name_without_override_when_route_is_visible() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "provider", true)],
            models: vec![model(11, 1, "allowed", true)],
            routes: vec![CacheModelRoute {
                id: 200,
                route_name: "manual-smoke-route".to_string(),
                description: None,
                is_enabled: true,
                expose_in_models: true,
                candidates: vec![CacheModelRouteCandidate {
                    route_id: 200,
                    model_id: 11,
                    provider_id: 1,
                    priority: 0,
                    is_enabled: true,
                }],
            }],
            api_key_overrides: vec![],
        };

        let models = collect_accessible_models(&catalog, &api_key(Action::Allow, vec![]));
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "provider/allowed".to_string(),
                "manual-smoke-route".to_string(),
            ]
        );
    }

    #[test]
    fn exposes_override_name_even_when_route_is_hidden_from_models() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "provider", true)],
            models: vec![model(11, 1, "allowed", true)],
            routes: vec![CacheModelRoute {
                id: 200,
                route_name: "hidden-route".to_string(),
                description: None,
                is_enabled: true,
                expose_in_models: false,
                candidates: vec![CacheModelRouteCandidate {
                    route_id: 200,
                    model_id: 11,
                    provider_id: 1,
                    priority: 0,
                    is_enabled: true,
                }],
            }],
            api_key_overrides: vec![CacheApiKeyModelOverride {
                id: 1,
                api_key_id: 1,
                source_name: "manual-cli-model".to_string(),
                target_route_id: 200,
                description: None,
                is_enabled: true,
            }],
        };

        let models = collect_accessible_models(&catalog, &api_key(Action::Allow, vec![]));
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "provider/allowed".to_string(),
                "manual-cli-model".to_string(),
            ]
        );
    }

    #[test]
    fn keeps_route_visible_when_primary_candidate_is_denied_but_secondary_is_allowed() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "provider", true)],
            models: vec![model(11, 1, "allowed", true), model(12, 1, "denied", true)],
            routes: vec![CacheModelRoute {
                id: 200,
                route_name: "manual-smoke-route".to_string(),
                description: None,
                is_enabled: true,
                expose_in_models: true,
                candidates: vec![
                    CacheModelRouteCandidate {
                        route_id: 200,
                        model_id: 12,
                        provider_id: 1,
                        priority: 0,
                        is_enabled: true,
                    },
                    CacheModelRouteCandidate {
                        route_id: 200,
                        model_id: 11,
                        provider_id: 1,
                        priority: 10,
                        is_enabled: true,
                    },
                ],
            }],
            api_key_overrides: vec![],
        };
        let api_key = api_key(
            Action::Deny,
            vec![CacheApiKeyAclRule {
                id: 1,
                effect: Action::Allow,
                priority: 1,
                scope: crate::schema::enum_def::RuleScope::Model,
                provider_id: Some(1),
                model_id: Some(11),
                is_enabled: true,
                description: None,
            }],
        );

        let models = collect_accessible_models(&catalog, &api_key);
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "provider/allowed".to_string(),
                "manual-smoke-route".to_string(),
            ]
        );
    }

    #[test]
    fn hides_route_and_override_when_all_candidates_are_denied() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "provider", true)],
            models: vec![
                model(11, 1, "denied-a", true),
                model(12, 1, "denied-b", true),
            ],
            routes: vec![CacheModelRoute {
                id: 200,
                route_name: "manual-smoke-route".to_string(),
                description: None,
                is_enabled: true,
                expose_in_models: true,
                candidates: vec![
                    CacheModelRouteCandidate {
                        route_id: 200,
                        model_id: 11,
                        provider_id: 1,
                        priority: 0,
                        is_enabled: true,
                    },
                    CacheModelRouteCandidate {
                        route_id: 200,
                        model_id: 12,
                        provider_id: 1,
                        priority: 10,
                        is_enabled: true,
                    },
                ],
            }],
            api_key_overrides: vec![CacheApiKeyModelOverride {
                id: 1,
                api_key_id: 1,
                source_name: "manual-cli-model".to_string(),
                target_route_id: 200,
                description: None,
                is_enabled: true,
            }],
        };
        let api_key = api_key(Action::Deny, vec![]);

        let models = collect_accessible_models(&catalog, &api_key);
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert!(ids.is_empty());
    }

    #[test]
    fn keeps_override_visible_when_primary_candidate_is_denied_but_secondary_is_allowed() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "provider", true)],
            models: vec![model(11, 1, "allowed", true), model(12, 1, "denied", true)],
            routes: vec![CacheModelRoute {
                id: 200,
                route_name: "manual-smoke-route".to_string(),
                description: None,
                is_enabled: true,
                expose_in_models: false,
                candidates: vec![
                    CacheModelRouteCandidate {
                        route_id: 200,
                        model_id: 12,
                        provider_id: 1,
                        priority: 0,
                        is_enabled: true,
                    },
                    CacheModelRouteCandidate {
                        route_id: 200,
                        model_id: 11,
                        provider_id: 1,
                        priority: 10,
                        is_enabled: true,
                    },
                ],
            }],
            api_key_overrides: vec![CacheApiKeyModelOverride {
                id: 1,
                api_key_id: 1,
                source_name: "manual-cli-model".to_string(),
                target_route_id: 200,
                description: None,
                is_enabled: true,
            }],
        };
        let api_key = api_key(
            Action::Deny,
            vec![CacheApiKeyAclRule {
                id: 1,
                effect: Action::Allow,
                priority: 1,
                scope: crate::schema::enum_def::RuleScope::Model,
                provider_id: Some(1),
                model_id: Some(11),
                is_enabled: true,
                description: None,
            }],
        );

        let models = collect_accessible_models(&catalog, &api_key);
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "provider/allowed".to_string(),
                "manual-cli-model".to_string(),
            ]
        );
    }

    #[test]
    fn deduplicates_route_and_override_effective_names_and_preserves_order() {
        let catalog = CacheModelsCatalog {
            providers: vec![
                provider(1, "provider-b", true),
                provider(2, "provider-a", true),
            ],
            models: vec![
                model(11, 1, "z-model", true),
                model(12, 1, "a-model", true),
                model(21, 2, "middle", true),
            ],
            routes: vec![
                CacheModelRoute {
                    id: 200,
                    route_name: "manual-smoke-route".to_string(),
                    description: None,
                    is_enabled: true,
                    expose_in_models: true,
                    candidates: vec![CacheModelRouteCandidate {
                        route_id: 200,
                        model_id: 11,
                        provider_id: 1,
                        priority: 0,
                        is_enabled: true,
                    }],
                },
                CacheModelRoute {
                    id: 201,
                    route_name: "provider-a/middle".to_string(),
                    description: None,
                    is_enabled: true,
                    expose_in_models: true,
                    candidates: vec![CacheModelRouteCandidate {
                        route_id: 201,
                        model_id: 21,
                        provider_id: 2,
                        priority: 0,
                        is_enabled: true,
                    }],
                },
            ],
            api_key_overrides: vec![
                CacheApiKeyModelOverride {
                    id: 1,
                    api_key_id: 1,
                    source_name: "manual-smoke-route".to_string(),
                    target_route_id: 200,
                    description: None,
                    is_enabled: true,
                },
                CacheApiKeyModelOverride {
                    id: 2,
                    api_key_id: 1,
                    source_name: "provider-b/a-model".to_string(),
                    target_route_id: 200,
                    description: None,
                    is_enabled: true,
                },
            ],
        };

        let models = collect_accessible_models(&catalog, &api_key(Action::Allow, vec![]));
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "provider-b/a-model".to_string(),
                "provider-b/z-model".to_string(),
                "provider-a/middle".to_string(),
                "manual-smoke-route".to_string(),
            ]
        );
    }

    #[test]
    fn renders_openai_style_models_response() {
        let response_body = render_models_response(
            LlmApiType::Openai,
            &[super::AccessibleModel {
                id: "provider/model".to_string(),
                owned_by: "provider".to_string(),
                provider_type: ProviderType::Openai,
            }],
        )
        .unwrap();

        let value: serde_json::Value = serde_json::from_str(&response_body).unwrap();
        assert_eq!(value["object"], "list");
        assert_eq!(value["data"][0]["id"], "provider/model");
    }

    #[test]
    fn renders_gemini_style_models_response() {
        let response_body = render_models_response(
            LlmApiType::Gemini,
            &[super::AccessibleModel {
                id: "provider/model".to_string(),
                owned_by: "provider".to_string(),
                provider_type: ProviderType::Gemini,
            }],
        )
        .unwrap();

        let value: serde_json::Value = serde_json::from_str(&response_body).unwrap();
        assert_eq!(value["models"][0]["name"], "models/provider/model");
    }
}
