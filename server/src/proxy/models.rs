use std::sync::Arc;

use reqwest::StatusCode;
use serde::Serialize;

use crate::{
    database::{
        access_control::ApiAccessControlPolicy, model::Model, provider::Provider,
        system_api_key::SystemApiKey,
    },
    schema::enum_def::ProviderType,
    service::app_state::AppState,
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
    system_api_key: &SystemApiKey,
) -> Result<Vec<AccessibleModel>, (StatusCode, String)> {
    debug!(
        "Fetching accessible models for SystemApiKey ID: {}",
        system_api_key.id
    );

    // 1. Fetch Access Control Policy if ID is present
    let access_control_policy_opt: Option<ApiAccessControlPolicy> =
        if let Some(policy_id) = system_api_key.access_control_policy_id {
            match app_state.access_control_store.get_by_id(policy_id) {
                Ok(Some(policy)) => Some(policy),
                Ok(None) => {
                    error!("Access control policy with id {} not found in store (configured on SystemApiKey {}).", policy_id, system_api_key.id);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!(
                            "Access control policy id {} configured but not found in application cache.",
                            policy_id
                        ),
                    ));
                }
                Err(store_err) => {
                    error!("Failed to fetch access control policy with id {} from store: {:?}", policy_id, store_err);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error accessing application cache for access control policy id {}: {}", policy_id, store_err),
                    ));
                }
            }
        } else {
            None
        };

    let mut available_models: Vec<AccessibleModel> = Vec::new();

    // 2. Get all active providers
    let active_providers = Provider::list_all_active().map_err(|e| {
        error!("Failed to list active providers: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to retrieve provider list".to_string(),
        )
    })?;

    debug!("Found {} active providers", active_providers.len());

    for provider in active_providers {
        // 3. Get all active models for this provider
        let active_models = Model::list_active_by_provider_id(provider.id).map_err(|e| {
            error!(
                "Failed to list active models for provider {}: {:?}",
                provider.id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Failed to retrieve model list for provider {}",
                    provider.name
                ),
            )
        })?;

        for model in active_models {
            let mut allowed = false;
            if let Some(ref policy) = access_control_policy_opt {
                // 4a. Check against policy if one is loaded
                match LIMITER.check_limit_strategy(policy, provider.id, model.id) {
                    Ok(_) => {
                        allowed = true;
                    }
                    Err(reason) => {
                        debug!(
                            "Model {}/{} denied by policy '{}' for SystemApiKey ID {}. Reason: {}",
                            provider.provider_key,
                            model.model_name,
                            policy.name,
                            system_api_key.id,
                            reason
                        );
                    }
                }
            } else {
                // 4b. No policy loaded, model is allowed by default
                allowed = true;
            }

            if allowed {
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

    // 5. Get all model aliases and check their accessibility
    let all_aliases = app_state.model_alias_store.get_all().map_err(|e| {
        error!("Failed to get model aliases from store: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to retrieve model alias list".to_string(),
        )
    })?;

    for alias in all_aliases {
        if !alias.is_enabled {
            continue;
        }

        // Find target model and provider
        if let Ok(Some(model)) = app_state.model_store.get_by_id(alias.target_model_id) {
            if !model.is_enabled {
                continue;
            }
            if let Ok(Some(provider)) = app_state.provider_store.get_by_id(model.provider_id) {
                if !provider.is_enabled {
                    continue;
                }

                let mut allowed = false;
                if let Some(ref policy) = access_control_policy_opt {
                    // Check policy against the target model
                    match LIMITER.check_limit_strategy(policy, provider.id, model.id) {
                        Ok(_) => {
                            allowed = true;
                        }
                        Err(reason) => {
                            debug!(
                                "Model alias '{}' (target: {}/{}) denied by policy '{}' for SystemApiKey ID {}. Reason: {}",
                                alias.alias_name, provider.provider_key, model.model_name, policy.name, system_api_key.id, reason
                            );
                        }
                    }
                } else {
                    // No policy, allowed by default
                    allowed = true;
                }

                if allowed {
                    available_models.push(AccessibleModel {
                        id: alias.alias_name.clone(),
                        owned_by: "cyder-api".to_string(),
                        provider_type: provider.provider_type.clone(),
                    });
                }
            }
        }
    }

    debug!(
        "Total accessible models (including aliases): {}",
        available_models.len()
    );

    Ok(available_models)
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

