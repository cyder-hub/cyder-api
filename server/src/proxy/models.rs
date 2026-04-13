use std::sync::Arc;

use axum::{body::Body, response::Response};
use serde::Serialize;

use super::ProxyError;
use crate::{
    schema::enum_def::{LlmApiType, ProviderType},
    service::{
        app_state::AppState,
        cache::types::{
            CacheAccessControl, CacheModel, CacheModelsCatalog, CacheProvider, CacheSystemApiKey,
        },
    },
    utils::limit::LIMITER,
};
use cyder_tools::log::{debug, error};

#[derive(Debug)]
pub(super) struct AccessibleModel {
    pub id: String,
    pub owned_by: String,
    pub provider_type: ProviderType,
}

pub(super) async fn get_accessible_models(
    app_state: &Arc<AppState>,
    system_api_key: &CacheSystemApiKey,
) -> Result<Vec<AccessibleModel>, ProxyError> {
    debug!(
        "Fetching accessible models for SystemApiKey ID: {}",
        system_api_key.id
    );

    // 1. Fetch Access Control Policy if ID is present
    let access_control_policy_opt: Option<Arc<CacheAccessControl>> = if let Some(policy_id) =
        system_api_key.access_control_policy_id
    {
        match app_state.get_access_control_policy(policy_id).await {
            Ok(Some(policy)) => Some(policy),
            Ok(None) => {
                error!(
                    "Access control policy with id {} not found in store (configured on SystemApiKey {}).",
                    policy_id, system_api_key.id
                );
                return Err(ProxyError::InternalError(format!(
                    "Access control policy id {} configured but not found in application cache.",
                    policy_id
                )));
            }
            Err(store_err) => {
                error!(
                    "Failed to fetch access control policy with id {} from store: {:?}",
                    policy_id, store_err
                );
                return Err(ProxyError::InternalError(format!(
                    "Error accessing application cache for access control policy id {}: {}",
                    policy_id, store_err
                )));
            }
        }
    } else {
        None
    };

    let catalog = app_state.get_models_catalog().await.map_err(|store_err| {
        error!("Failed to fetch models catalog from cache: {:?}", store_err);
        ProxyError::InternalError("Failed to retrieve models catalog".to_string())
    })?;

    let available_models = collect_accessible_models(
        catalog.as_ref(),
        access_control_policy_opt.as_deref(),
        system_api_key.id,
    );

    debug!(
        "Total accessible models (including aliases): {}",
        available_models.len()
    );

    Ok(available_models)
}

pub(super) async fn execute_models_listing(
    app_state: Arc<AppState>,
    system_api_key: Arc<CacheSystemApiKey>,
    api_type: LlmApiType,
) -> Result<Response<Body>, ProxyError> {
    let accessible_models = get_accessible_models(&app_state, &system_api_key).await?;
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
    access_control_policy: Option<&CacheAccessControl>,
    system_api_key_id: i64,
) -> Vec<AccessibleModel> {
    let providers_by_id = catalog
        .providers
        .iter()
        .filter(|provider| provider.is_enabled)
        .map(|provider| (provider.id, provider))
        .collect::<std::collections::HashMap<_, _>>();
    let models_by_id = catalog
        .models
        .iter()
        .filter(|model| model.is_enabled)
        .map(|model| (model.id, model))
        .collect::<std::collections::HashMap<_, _>>();

    let mut available_models = Vec::new();

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
            if is_model_allowed(access_control_policy, provider, model, system_api_key_id) {
                available_models.push(AccessibleModel {
                    id: format!("{}/{}", provider.provider_key, model.model_name),
                    owned_by: provider.provider_key.clone(),
                    provider_type: provider.provider_type.clone(),
                });
            }
        }
    }

    debug!(
        "Found {} accessible models from providers",
        available_models.len()
    );

    for alias in catalog.aliases.iter().filter(|alias| alias.is_enabled) {
        let Some(model) = models_by_id.get(&alias.target_model_id) else {
            continue;
        };
        let Some(provider) = providers_by_id.get(&model.provider_id) else {
            continue;
        };

        if is_model_allowed(access_control_policy, provider, model, system_api_key_id) {
            available_models.push(AccessibleModel {
                id: alias.alias_name.clone(),
                owned_by: "cyder-api".to_string(),
                provider_type: provider.provider_type.clone(),
            });
        }
    }

    available_models
}

fn is_model_allowed(
    access_control_policy: Option<&CacheAccessControl>,
    provider: &CacheProvider,
    model: &CacheModel,
    system_api_key_id: i64,
) -> bool {
    if let Some(policy) = access_control_policy {
        match LIMITER.check_limit_strategy(policy, provider.id, model.id) {
            Ok(_) => true,
            Err(reason) => {
                debug!(
                    "Model {}/{} denied by policy '{}' for SystemApiKey ID {}. Reason: {}",
                    provider.provider_key, model.model_name, policy.name, system_api_key_id, reason
                );
                false
            }
        }
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_accessible_models, render_models_response};
    use crate::schema::enum_def::{Action, LlmApiType, ProviderApiKeyMode, ProviderType};
    use crate::service::cache::types::{
        CacheAccessControl, CacheModel, CacheModelAlias, CacheModelsCatalog, CacheProvider,
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
            is_enabled,
        }
    }

    #[test]
    fn collects_models_from_cached_catalog_and_preserves_ordering() {
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
            aliases: vec![
                CacheModelAlias {
                    id: 100,
                    alias_name: "alias-z".to_string(),
                    target_model_id: 11,
                    is_enabled: true,
                },
                CacheModelAlias {
                    id: 101,
                    alias_name: "alias-disabled".to_string(),
                    target_model_id: 21,
                    is_enabled: false,
                },
            ],
        };

        let models = collect_accessible_models(&catalog, None, 1);

        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "provider-b/a-model".to_string(),
                "provider-b/z-model".to_string(),
                "provider-a/middle".to_string(),
                "alias-z".to_string(),
            ]
        );
    }

    #[test]
    fn skips_inactive_provider_model_and_alias_targets() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "active", true), provider(2, "inactive", false)],
            models: vec![
                model(11, 1, "active-model", true),
                model(12, 1, "disabled-model", false),
                model(21, 2, "hidden-by-provider", true),
            ],
            aliases: vec![
                CacheModelAlias {
                    id: 100,
                    alias_name: "alias-active".to_string(),
                    target_model_id: 11,
                    is_enabled: true,
                },
                CacheModelAlias {
                    id: 101,
                    alias_name: "alias-disabled-model".to_string(),
                    target_model_id: 12,
                    is_enabled: true,
                },
                CacheModelAlias {
                    id: 102,
                    alias_name: "alias-inactive-provider".to_string(),
                    target_model_id: 21,
                    is_enabled: true,
                },
            ],
        };

        let models = collect_accessible_models(&catalog, None, 1);
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "active/active-model".to_string(),
                "alias-active".to_string()
            ]
        );
    }

    #[test]
    fn filters_models_and_aliases_with_access_control() {
        let catalog = CacheModelsCatalog {
            providers: vec![provider(1, "provider", true)],
            models: vec![model(11, 1, "allowed", true), model(12, 1, "denied", true)],
            aliases: vec![CacheModelAlias {
                id: 100,
                alias_name: "alias-denied".to_string(),
                target_model_id: 12,
                is_enabled: true,
            }],
        };
        let policy = CacheAccessControl {
            id: 1,
            name: "default-deny".to_string(),
            default_action: Action::Deny,
            rules: vec![crate::service::cache::types::CacheAccessControlRule {
                id: 1,
                rule_type: Action::Allow,
                priority: 1,
                scope: crate::schema::enum_def::RuleScope::Model,
                provider_id: Some(1),
                model_id: Some(11),
            }],
        };

        let models = collect_accessible_models(&catalog, Some(&policy), 1);
        let ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
        assert_eq!(ids, vec!["provider/allowed".to_string()]);
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
