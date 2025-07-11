use crate::database::{
    provider::{
        NewProvider, Provider, UpdateProviderData, ProviderApiKey, NewProviderApiKey,
        UpdateProviderApiKeyData
    },
    DbResult,
};
use axum::{
    extract::{Json, Path, State}, // Added State
    routing::{delete, get, post, put},
};
use chrono::Utc;
use crate::config::CONFIG;
use crate::service::app_state::{create_state_router, StateRouter, AppState}; // Added AppState
use reqwest::{Client, Proxy, StatusCode, Url};
use std::sync::Arc; // Added Arc
use serde::{Deserialize, Serialize};
use serde_json::Value;
use cyder_tools::log::{warn, info};

use crate::service::vertex::get_vertex_token;
use crate::utils::{HttpResult, ID_GENERATOR};

use super::BaseError;
use crate::database::custom_field::{ApiCustomFieldDefinition, CustomFieldDefinition};
use crate::database::model::Model;
use crate::schema::enum_def::ProviderType;

#[derive(Serialize)]
struct ModelDetail {
    model: Model,
    custom_fields: Vec<ApiCustomFieldDefinition>,
}

#[derive(Serialize)]
struct ProviderDetail {
    provider: Provider,
    models: Vec<ModelDetail>,
    provider_keys: Vec<ProviderApiKey>,
    custom_fields: Vec<ApiCustomFieldDefinition>,
}

async fn list() -> (StatusCode, HttpResult<Vec<Provider>>) {
    let result = Provider::list_all().unwrap_or_else(|_| vec![]); // Adjusted to list_all and handle error case
    (StatusCode::OK, HttpResult::new(result))
}

#[derive(Deserialize)]
struct InserPayload {
    pub name: String,
    pub key: String,
    pub endpoint: String,
    pub use_proxy: bool,
    pub provider_type: Option<ProviderType>,
}

async fn insert(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<InserPayload>
) -> DbResult<HttpResult<Provider>> {
    let current_time = Utc::now().timestamp_millis();
    let new_provider_data = NewProvider {
        id: ID_GENERATOR.generate_id(),
        provider_key: payload.key,
        name: payload.name,
        endpoint: payload.endpoint,
        use_proxy: payload.use_proxy,
        is_enabled: true, // Default for new providers
        created_at: current_time,
        updated_at: current_time,
        provider_type: payload.provider_type.unwrap_or_else(|| ProviderType::Openai),
    };
    let created_provider = Provider::create(&new_provider_data)?;

    // Update the provider_store
    let _ = app_state.provider_store.add(created_provider.clone());

    Ok(HttpResult::new(created_provider))
}

async fn get_provider(Path(id): Path<i64>) -> Result<HttpResult<Provider>, BaseError> {
    match Provider::get_by_id(id) {
        Ok(pro) => Ok(HttpResult::new(pro)),
        Err(err) => Err(err),
    }
}

async fn update_provider(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<InserPayload>,
) -> Result<HttpResult<Provider>, BaseError> {
    let update_data = UpdateProviderData {
        provider_key: Some(payload.key),
        name: Some(payload.name),
        endpoint: Some(payload.endpoint),
        use_proxy: Some(payload.use_proxy),
        is_enabled: None, // InserPayload doesn't have is_enabled. Set to None to not change it unless specified.
        provider_type: payload.provider_type,
    };
    // Note: payload.api_keys, payload.omit_config, payload.limit_model are not used by Provider::update.
    let updated_provider = Provider::update(id, &update_data)?;

    // Update the provider_store
    let _ = app_state.provider_store.update(updated_provider.clone());

    Ok(HttpResult::new(updated_provider))
}

async fn delete_provider(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>
) -> Result<HttpResult<()>, BaseError> {
    // Fetch provider details before deleting to get the key for store removal
    // Fetch provider details to ensure it exists before DB delete.
    // Not strictly needed for cache operation if ID is the only thing used, but good practice.
    let _provider_to_delete_from_db = Provider::get_by_id(id)?;

    match Provider::delete(id) { // This is DB soft-delete
        Ok(num_deleted_db) => {
            if num_deleted_db > 0 {
                // Remove provider from its store in AppState
                if let Err(e) = app_state.provider_store.delete(id) {
                    warn!("Provider id {} successfully deleted from DB, but failed to remove from provider_store cache: {:?}", id, e);
                    // Log the cache error but the DB operation was successful.
                }

                // Remove associated API keys from their store in AppState
                match app_state.provider_api_key_store.delete_by_group_id(id) {
                    Ok(deleted_keys) => {
                        info!("Removed {} ProviderApiKeys from cache for provider_id {}.", deleted_keys.len(), id);
                    }
                    Err(e) => {
                        warn!("Error during provider_api_key_store.delete_by_group_id for provider {}: {:?}. Associated API keys might remain in cache.", id, e);
                    }
                }
            }
            Ok(HttpResult::new(())) // Success if DB operation was successful
        }
        Err(err) => Err(err), // DB operation failed
    }
}

async fn get_provider_detail(Path(id): Path<i64>) -> Result<HttpResult<ProviderDetail>, BaseError> {
    let provider = Provider::get_by_id(id)?;
    let models_list = Model::list_by_provider_id(id)?;
    let provider_keys = ProviderApiKey::list_by_provider_id(id)?;
    let custom_fields = CustomFieldDefinition::list_by_provider_id(id)?;

    let mut models_with_details: Vec<ModelDetail> = Vec::new();
    for model in models_list {
        let model_custom_fields = CustomFieldDefinition::list_by_model_id(model.id)?;
        models_with_details.push(ModelDetail {
            model,
            custom_fields: model_custom_fields,
        });
    }

    let detail = ProviderDetail {
        provider,
        models: models_with_details,
        provider_keys,
        custom_fields,
    };

    Ok(HttpResult::new(detail))
}

async fn get_remote_models(
    Path(id): Path<i64>,
) -> Result<HttpResult<Value>, BaseError> {
    let provider = Provider::get_by_id(id)?;
    let provider_keys = ProviderApiKey::list_by_provider_id(id)?;

    let api_key_record = provider_keys.first().ok_or_else(|| {
        BaseError::ParamInvalid(Some("No API key found for this provider.".to_string()))
    })?;

    let client = if provider.use_proxy {
        let proxy = Proxy::https(&CONFIG.proxy.url).unwrap();
        reqwest::Client::builder().proxy(proxy).build().unwrap()
    } else {
        Client::new()
    };

    let response = if provider.provider_type == ProviderType::Gemini {
        let mut url = Url::parse(&provider.endpoint).map_err(|e| {
            BaseError::ParamInvalid(Some(format!("Failed to parse provider endpoint as URL: {}", e)))
        })?;
        url.query_pairs_mut()
            .append_pair("key", &api_key_record.api_key);

        client.get(url).send().await.map_err(|e| {
            BaseError::ParamInvalid(Some(format!("Failed to fetch remote models: {}", e)))
        })?
    } else if provider.provider_type == ProviderType::Vertex {
        let token = get_vertex_token(api_key_record.id, &api_key_record.api_key)
            .await
            .map_err(|e| BaseError::ParamInvalid(Some(format!("Failed to get vertex token: {}", e))))?;

        client
            .get(&provider.endpoint)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| {
                BaseError::ParamInvalid(Some(format!("Failed to fetch remote models: {}", e)))
            })?
    } else {
        // For OpenAI-style providers (including VERTEX_OPENAI), append /models and use Bearer auth.
        let url = format!("{}/models", provider.endpoint.trim_end_matches('/'));
        client
            .get(&url)
            .bearer_auth(&api_key_record.api_key)
            .send()
            .await
            .map_err(|e| {
                BaseError::ParamInvalid(Some(format!("Failed to fetch remote models: {}", e)))
            })?
    };

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Could not retrieve error body".to_string());
        return Err(BaseError::ParamInvalid(Some(format!(
            "Provider API returned status {}: {}",
            status, error_body
        ))));
    }

    let models = response.json::<Value>().await.map_err(|e| {
        BaseError::ParamInvalid(Some(format!(
            "Failed to parse remote models response: {}",
            e
        )))
    })?;

    Ok(HttpResult::new(models))
}

// Removed full_commit function as Provider::full_commit is no longer available.

async fn list_provider_details() -> Result<(StatusCode, HttpResult<Vec<ProviderDetail>>), BaseError>
{
    let providers = Provider::list_all()?;
    let mut provider_details: Vec<ProviderDetail> = Vec::new();

    for provider in providers {
        let models_list = Model::list_by_provider_id(provider.id)?;
        let provider_keys = ProviderApiKey::list_by_provider_id(provider.id)?;
        let custom_fields = CustomFieldDefinition::list_by_provider_id(provider.id)?;

        let mut models_with_details: Vec<ModelDetail> = Vec::new();
        for model in models_list {
            let model_custom_fields = CustomFieldDefinition::list_by_model_id(model.id)?;
            models_with_details.push(ModelDetail {
                model,
                custom_fields: model_custom_fields,
            });
        }

        let detail = ProviderDetail {
            provider,
            models: models_with_details,
            provider_keys,
            custom_fields,
        };
        provider_details.push(detail);
    }

    Ok((StatusCode::OK, HttpResult::new(provider_details)))
}

#[derive(Deserialize)]
struct CreateProviderApiKeyPayload {
    api_key: String,
    description: Option<String>,
    is_enabled: Option<bool>,
}

async fn add_provider_api_key(
    State(app_state): State<Arc<AppState>>, // Added AppState
    Path(provider_id): Path<i64>,
    Json(payload): Json<CreateProviderApiKeyPayload>,
) -> Result<HttpResult<ProviderApiKey>, BaseError> {
    // Ensure the provider exists to associate the key with.
    let _provider = Provider::get_by_id(provider_id)?; // Or use app_state.provider_store.get_by_id(provider_id)

    let current_time = Utc::now().timestamp_millis();
    let new_key_data = NewProviderApiKey {
        id: ID_GENERATOR.generate_id(),
        provider_id,
        api_key: payload.api_key,
        description: payload.description,
        is_enabled: payload.is_enabled.unwrap_or(true), // Default to true if not specified
        created_at: current_time,
        updated_at: current_time,
    };

    let created_key = ProviderApiKey::insert(&new_key_data)?;

    // Add to app_state.provider_api_key_store
    if let Err(e) = app_state.provider_api_key_store.add(created_key.clone()) {
        warn!("Failed to add ProviderApiKey id {} to store after DB insert: {:?}", created_key.id, e);
        // Depending on policy, this could be an error propagated to client, or just logged.
        // For now, just log, as DB operation was successful.
    }

    Ok(HttpResult::new(created_key))
}

async fn list_provider_api_keys(
    Path(provider_id): Path<i64>,
) -> Result<HttpResult<Vec<ProviderApiKey>>, BaseError> {
    // Ensure the provider exists
    let _provider = Provider::get_by_id(provider_id)?;
    let keys = ProviderApiKey::list_by_provider_id(provider_id)?;
    Ok(HttpResult::new(keys))
}

async fn get_provider_api_key(
    Path((provider_id, key_id)): Path<(i64, i64)>,
) -> Result<HttpResult<ProviderApiKey>, BaseError> {
    // Optional: Ensure the key belongs to the provider_id path, though get_by_id is global by key_id
    let _provider = Provider::get_by_id(provider_id)?;
    let key = ProviderApiKey::get_by_id(key_id)?;
    // Additional check if key.provider_id matches provider_id from path
    if key.provider_id != provider_id {
        return Err(BaseError::ParamInvalid(Some(format!(
            "API key {} does not belong to provider {}",
            key_id, provider_id
        ))));
    }
    Ok(HttpResult::new(key))
}

#[derive(Deserialize)]
struct UpdateProviderApiKeyPayload {
    api_key: Option<String>,
    description: Option<String>, // To clear description, send null or handle empty string as None
    is_enabled: Option<bool>,
}

async fn update_provider_api_key(
    State(app_state): State<Arc<AppState>>, // Added AppState
    Path((provider_id, key_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateProviderApiKeyPayload>,
) -> Result<HttpResult<ProviderApiKey>, BaseError> {
    // Ensure the provider exists
    let _provider = Provider::get_by_id(provider_id)?; // Or use app_state.provider_store.get_by_id(provider_id)
    // Fetch the key to ensure it belongs to the provider before updating
    let key_to_update = ProviderApiKey::get_by_id(key_id)?; // Or use app_state.provider_api_key_store.get_by_id(key_id)
    if key_to_update.provider_id != provider_id {
        return Err(BaseError::ParamInvalid(Some(format!(
            "API key {} does not belong to provider {}",
            key_id, provider_id
        ))));
    }

    let update_data = UpdateProviderApiKeyData {
        api_key: payload.api_key,
        description: payload.description, // Consider how to handle clearing: Option<Option<String>> or specific value
        is_enabled: payload.is_enabled,
    };

    let updated_key = ProviderApiKey::update(key_id, &update_data)?;

    // Update in app_state.provider_api_key_store
    if let Err(e) = app_state.provider_api_key_store.update(updated_key.clone()) {
        warn!("Failed to update ProviderApiKey id {} in store after DB update: {:?}", updated_key.id, e);
    }

    Ok(HttpResult::new(updated_key))
}

async fn delete_provider_api_key(
    State(app_state): State<Arc<AppState>>, // Added AppState
    Path((provider_id, key_id)): Path<(i64, i64)>,
) -> Result<HttpResult<()>, BaseError> {
    // Ensure the provider exists
    let _provider = Provider::get_by_id(provider_id)?; // Or use app_state.provider_store.get_by_id(provider_id)
    // Fetch the key to ensure it belongs to the provider before deleting
    let key_to_delete_from_db = ProviderApiKey::get_by_id(key_id)?; // Or use app_state.provider_api_key_store.get_by_id(key_id)
    if key_to_delete_from_db.provider_id != provider_id {
         return Err(BaseError::ParamInvalid(Some(format!(
            "API key {} does not belong to provider {}",
            key_id, provider_id
        ))));
    }

    ProviderApiKey::delete(key_id)?; // DB soft-delete

    // Delete from app_state.provider_api_key_store
    if let Err(e) = app_state.provider_api_key_store.delete(key_id) {
        match e {
            crate::service::app_state::AppStoreError::NotFound(_) => {
                // If not found in cache, it might have been already removed or never added. Usually not critical.
                info!("ProviderApiKey id {} not found in store for deletion after DB delete.", key_id);
            }
            _ => {
                warn!("Failed to delete ProviderApiKey id {} from store after DB delete: {:?}", key_id, e);
            }
        }
    }

    Ok(HttpResult::new(()))
}

pub fn create_provider_router() -> StateRouter {
    create_state_router().nest(
        "/provider",
        create_state_router()
            .route("/", post(insert))
            // .route("/commit", post(full_commit)) // Removed full_commit route
            .route("/list", get(list))
            .route("/detail/list", get(list_provider_details))
            .route("/{id}", get(get_provider))
            .route("/{id}/detail", get(get_provider_detail))
            .route("/{id}/remote_models", get(get_remote_models))
            .route("/{id}", delete(delete_provider))
            .route("/{id}", put(update_provider))
            // Provider API Key routes
            .route("/{id}/provider_key", post(add_provider_api_key))
            .route("/{id}/provider_keys", get(list_provider_api_keys)) // List keys for a provider
            .route(
                "/{id}/provider_key/{key_id}",
                get(get_provider_api_key),
            ) // Get specific key
            .route(
                "/{id}/provider_key/{key_id}",
                put(update_provider_api_key),
            ) // Update specific key
            .route(
                "/{id}/provider_key/{key_id}",
                delete(delete_provider_api_key),
            ) // Delete specific key
    )
}
