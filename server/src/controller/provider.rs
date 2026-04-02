use crate::database::{
    DbResult,
    provider::{
        NewProvider, NewProviderApiKey, Provider, ProviderApiKey, UpdateProviderApiKeyData,
        UpdateProviderData,
    },
};
use crate::service::app_state::{AppState, StateRouter, create_state_router}; // Added AppState
use axum::{
    extract::{Json, Path, State}, // Added State
    routing::{delete, get, post, put},
};
use chrono::Utc;
use cyder_tools::log::warn;
use reqwest::{
    StatusCode, Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc; // Added Arc

use crate::service::vertex::get_vertex_token;
use crate::utils::{HttpResult, ID_GENERATOR};

use super::BaseError;
use crate::database::custom_field::{ApiCustomFieldDefinition, CustomFieldDefinition};
use crate::database::model::Model;
use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};

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

async fn list() -> DbResult<HttpResult<Vec<Provider>>> {
    let result = Provider::list_all()?;
    Ok(HttpResult::new(result))
}

#[derive(Deserialize)]
struct InserPayload {
    pub name: String,
    pub key: String,
    pub endpoint: String,
    pub use_proxy: bool,
    pub provider_type: Option<ProviderType>,
    pub provider_api_key_mode: Option<ProviderApiKeyMode>,
}

async fn insert(Json(payload): Json<InserPayload>) -> DbResult<HttpResult<Provider>> {
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
        provider_type: payload
            .provider_type
            .unwrap_or_else(|| ProviderType::Openai),
        provider_api_key_mode: payload
            .provider_api_key_mode
            .unwrap_or_else(|| ProviderApiKeyMode::Queue),
    };
    let created_provider = Provider::create(&new_provider_data)?;

    // No need to manually update cache - it will be loaded on first read
    // Cache follows Cache-Aside pattern now

    Ok(HttpResult::new(created_provider))
}

async fn get_provider(Path(id): Path<i64>) -> Result<HttpResult<Provider>, BaseError> {
    let provider = Provider::get_by_id(id)?;

    Ok(HttpResult::new(provider))
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
        provider_api_key_mode: payload.provider_api_key_mode,
    };
    // Note: payload.api_keys, payload.omit_config, payload.limit_model are not used by Provider::update.
    let updated_provider = Provider::update(id, &update_data)?;

    // Invalidate cache - next read will load fresh data from database
    if let Err(e) = app_state
        .invalidate_provider(id, Some(&updated_provider.provider_key))
        .await
    {
        warn!("Failed to invalidate Provider id {} in cache: {:?}", id, e);
    }

    Ok(HttpResult::new(updated_provider))
}

async fn delete_provider(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    // Fetch provider details before deleting to get the key for store removal
    // Fetch provider details to ensure it exists before DB delete.
    // Not strictly needed for cache operation if ID is the only thing used, but good practice.
    let _provider_to_delete_from_db = Provider::get_by_id(id)?;

    match Provider::delete(id) {
        // This is DB soft-delete
        Ok(num_deleted_db) => {
            if num_deleted_db > 0 {
                // Invalidate provider cache (key not available after delete, so pass None)
                if let Err(e) = app_state.invalidate_provider(id, None).await {
                    warn!(
                        "Provider id {} successfully deleted from DB, but failed to invalidate cache: {:?}",
                        id, e
                    );
                }

                // Invalidate associated API keys cache
                if let Err(e) = app_state.invalidate_provider_api_keys(id).await {
                    warn!(
                        "Error invalidating provider API keys cache for provider {}: {:?}",
                        id, e
                    );
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

#[derive(Deserialize)]
struct CheckProviderPayload {
    model_id: Option<i64>,
    model_name: Option<String>,
    provider_api_key_id: Option<i64>,
    provider_api_key: Option<String>,
}

struct ProviderCheckRequest {
    url: String,
    headers: HeaderMap,
    body: Value,
}

async fn build_provider_check_request(
    client: &reqwest::Client,
    provider: &Provider,
    provider_api_key_id: i64,
    api_key: &str,
    model_name: &str,
) -> Result<ProviderCheckRequest, BaseError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let request = match provider.provider_type {
        ProviderType::Gemini => {
            headers.insert("x-goog-api-key", header_value(api_key)?);
            ProviderCheckRequest {
                url: format_gemini_generate_content_url(provider, model_name),
                headers,
                body: json!({
                    "contents": [
                        {
                            "parts": [
                                { "text": "hi" }
                            ]
                        }
                    ]
                }),
            }
        }
        ProviderType::Vertex => {
            let token = get_vertex_token(client, provider_api_key_id, api_key)
                .await
                .map_err(|e| {
                    BaseError::ParamInvalid(Some(format!("Failed to get vertex token: {}", e)))
                })?;
            headers.insert(AUTHORIZATION, header_value(&format!("Bearer {}", token))?);
            ProviderCheckRequest {
                url: format_gemini_generate_content_url(provider, model_name),
                headers,
                body: json!({
                    "contents": [
                        {
                            "parts": [
                                { "text": "hi" }
                            ]
                        }
                    ]
                }),
            }
        }
        ProviderType::VertexOpenai => {
            let token = get_vertex_token(client, provider_api_key_id, api_key)
                .await
                .map_err(|e| {
                    BaseError::ParamInvalid(Some(format!("Failed to get vertex token: {}", e)))
                })?;
            headers.insert(AUTHORIZATION, header_value(&format!("Bearer {}", token))?);
            ProviderCheckRequest {
                url: format_openai_check_url(provider),
                headers,
                body: json!({
                    "model": model_name,
                    "messages": [
                        {
                            "role": "user",
                            "content": "hi"
                        }
                    ]
                }),
            }
        }
        ProviderType::Anthropic => {
            headers.insert("x-api-key", header_value(api_key)?);
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            ProviderCheckRequest {
                url: format!("{}/messages", provider.endpoint.trim_end_matches('/')),
                headers,
                body: json!({
                    "model": model_name,
                    "max_tokens": 1,
                    "messages": [
                        {
                            "role": "user",
                            "content": "hi"
                        }
                    ]
                }),
            }
        }
        ProviderType::Ollama => {
            headers.insert(AUTHORIZATION, header_value(&format!("Bearer {}", api_key))?);
            ProviderCheckRequest {
                url: format!("{}/api/chat", provider.endpoint.trim_end_matches('/')),
                headers,
                body: json!({
                    "model": model_name,
                    "stream": false,
                    "messages": [
                        {
                            "role": "user",
                            "content": "hi"
                        }
                    ]
                }),
            }
        }
        ProviderType::Openai | ProviderType::Responses => {
            headers.insert(AUTHORIZATION, header_value(&format!("Bearer {}", api_key))?);
            ProviderCheckRequest {
                url: format_openai_check_url(provider),
                headers,
                body: json!({
                    "model": model_name,
                    "messages": [
                        {
                            "role": "user",
                            "content": "hi"
                        }
                    ]
                }),
            }
        }
    };

    Ok(request)
}

fn format_openai_check_url(provider: &Provider) -> String {
    format!(
        "{}/chat/completions",
        provider.endpoint.trim_end_matches('/')
    )
}

fn format_gemini_generate_content_url(provider: &Provider, model_name: &str) -> String {
    format!(
        "{}/{}:generateContent",
        provider.endpoint.trim_end_matches('/'),
        model_name
    )
}

fn header_value(value: &str) -> Result<HeaderValue, BaseError> {
    HeaderValue::from_str(value)
        .map_err(|e| BaseError::ParamInvalid(Some(format!("Invalid request header value: {}", e))))
}

async fn check_provider(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<CheckProviderPayload>,
) -> Result<HttpResult<Value>, BaseError> {
    let model_name = match (payload.model_id, payload.model_name) {
        (Some(model_id), _) => {
            let model = Model::get_by_id(model_id)?;
            if model.provider_id != id {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "Model {} does not belong to provider {}",
                    model_id, id
                ))));
            }
            model
                .real_model_name
                .filter(|s| !s.is_empty())
                .unwrap_or(model.model_name)
        }
        (_, Some(model_name)) => model_name,
        (None, None) => {
            return Err(BaseError::ParamInvalid(Some(
                "Either model_id or model_name must be provided.".to_string(),
            )));
        }
    };

    let (provider_api_key_id, api_key) =
        match (payload.provider_api_key_id, payload.provider_api_key) {
            (Some(key_id), _) => {
                let provider_api_key = ProviderApiKey::get_by_id(key_id)?;
                if provider_api_key.provider_id != id {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "API key {} does not belong to provider {}",
                        key_id, id
                    ))));
                }
                (provider_api_key.id, provider_api_key.api_key)
            }
            (_, Some(api_key)) => (0, api_key),
            (None, None) => {
                return Err(BaseError::ParamInvalid(Some(
                    "Either provider_api_key_id or provider_api_key must be provided.".to_string(),
                )));
            }
        };

    let provider = Provider::get_by_id(id)?;

    let client = if provider.use_proxy {
        &app_state.proxy_client
    } else {
        &app_state.client
    };

    let check_request = build_provider_check_request(
        client,
        &provider,
        provider_api_key_id,
        &api_key,
        &model_name,
    )
    .await?;

    let response = client
        .post(&check_request.url)
        .headers(check_request.headers)
        .json(&check_request.body)
        .send()
        .await
        .map_err(|e| {
            BaseError::ParamInvalid(Some(format!("Failed to send check request: {}", e)))
        })?;

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

    // On success, we don't need to return the original response.
    // Just consume the body to free up the connection and return a success indicator.
    let _ = response.text().await;
    Ok(HttpResult::new(serde_json::Value::Null))
}

async fn get_remote_models(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<Value>, BaseError> {
    let provider = Provider::get_by_id(id)?;
    let provider_keys = ProviderApiKey::list_by_provider_id(id)?;

    let api_key_record = provider_keys.first().ok_or_else(|| {
        BaseError::ParamInvalid(Some("No API key found for this provider.".to_string()))
    })?;

    let client = if provider.use_proxy {
        &app_state.proxy_client
    } else {
        &app_state.client
    };

    let response = if provider.provider_type == ProviderType::Gemini {
        let mut url = Url::parse(&provider.endpoint).map_err(|e| {
            BaseError::ParamInvalid(Some(format!(
                "Failed to parse provider endpoint as URL: {}",
                e
            )))
        })?;
        url.query_pairs_mut()
            .append_pair("key", &api_key_record.api_key);

        client.get(url).send().await.map_err(|e| {
            BaseError::ParamInvalid(Some(format!("Failed to fetch remote models: {}", e)))
        })?
    } else if provider.provider_type == ProviderType::Vertex {
        let token = get_vertex_token(client, api_key_record.id, &api_key_record.api_key)
            .await
            .map_err(|e| {
                BaseError::ParamInvalid(Some(format!("Failed to get vertex token: {}", e)))
            })?;

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

    // No need to manually update cache - it will be loaded on first read
    // Also invalidate the provider's key list cache
    if let Err(e) = app_state.invalidate_provider_api_keys(provider_id).await {
        warn!(
            "Failed to invalidate provider API keys cache for provider {}: {:?}",
            provider_id, e
        );
    }

    Ok(HttpResult::new(created_key))
}

async fn list_provider_api_keys(
    Path(provider_id): Path<i64>,
) -> Result<HttpResult<Vec<ProviderApiKey>>, BaseError> {
    let _provider = Provider::get_by_id(provider_id)?;

    let keys = ProviderApiKey::list_by_provider_id(provider_id)?;

    Ok(HttpResult::new(keys))
}

async fn get_provider_api_key(
    Path((provider_id, key_id)): Path<(i64, i64)>,
) -> Result<HttpResult<ProviderApiKey>, BaseError> {
    let _provider = Provider::get_by_id(provider_id)?;

    let key = ProviderApiKey::get_by_id(key_id)?;

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

    // Also invalidate the provider's key list
    if let Err(e) = app_state.invalidate_provider_api_keys(provider_id).await {
        warn!(
            "Failed to invalidate provider API keys cache for provider {}: {:?}",
            provider_id, e
        );
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

    // Also invalidate the provider's key list
    if let Err(e) = app_state.invalidate_provider_api_keys(provider_id).await {
        warn!(
            "Failed to invalidate provider API keys cache for provider {}: {:?}",
            provider_id, e
        );
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
            .route("/{id}/check", post(check_provider))
            .route("/{id}", delete(delete_provider))
            .route("/{id}", put(update_provider))
            // Provider API Key routes
            .route("/{id}/provider_key", post(add_provider_api_key))
            .route("/{id}/provider_keys", get(list_provider_api_keys)) // List keys for a provider
            .route("/{id}/provider_key/{key_id}", get(get_provider_api_key)) // Get specific key
            .route("/{id}/provider_key/{key_id}", put(update_provider_api_key)) // Update specific key
            .route(
                "/{id}/provider_key/{key_id}",
                delete(delete_provider_api_key),
            ), // Delete specific key
    )
}

#[cfg(test)]
mod tests {
    use super::header_value;
    use crate::database::provider::Provider;
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};

    #[tokio::test]
    async fn openai_style_check_request_uses_chat_completions() {
        let provider = sample_provider(ProviderType::Openai, "https://api.example.com/v1");
        let request = super::build_provider_check_request(
            &reqwest::Client::new(),
            &provider,
            0,
            "sk-test",
            "gpt-4o-mini",
        )
        .await
        .expect("request should build");

        assert_eq!(request.url, "https://api.example.com/v1/chat/completions");
        assert_eq!(
            request
                .headers
                .get(reqwest::header::AUTHORIZATION)
                .expect("auth header"),
            &header_value("Bearer sk-test").unwrap()
        );
        assert_eq!(request.body["model"], "gpt-4o-mini");
        assert_eq!(request.body["messages"][0]["content"], "hi");
    }

    #[tokio::test]
    async fn anthropic_check_request_uses_messages_and_version_header() {
        let provider = sample_provider(ProviderType::Anthropic, "https://api.anthropic.com/v1");
        let request = super::build_provider_check_request(
            &reqwest::Client::new(),
            &provider,
            0,
            "ak-test",
            "claude-3-5-haiku-latest",
        )
        .await
        .expect("request should build");

        assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(
            request.headers.get("x-api-key").expect("x-api-key"),
            &header_value("ak-test").unwrap()
        );
        assert_eq!(
            request
                .headers
                .get("anthropic-version")
                .expect("anthropic-version"),
            "2023-06-01"
        );
        assert_eq!(request.body["model"], "claude-3-5-haiku-latest");
        assert_eq!(request.body["max_tokens"], 1);
    }

    #[tokio::test]
    async fn gemini_check_request_uses_generate_content() {
        let provider = sample_provider(
            ProviderType::Gemini,
            "https://generativelanguage.googleapis.com/v1beta/models",
        );
        let request = super::build_provider_check_request(
            &reqwest::Client::new(),
            &provider,
            0,
            "gm-test",
            "gemini-2.0-flash",
        )
        .await
        .expect("request should build");

        assert_eq!(
            request.url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent"
        );
        assert_eq!(
            request
                .headers
                .get("x-goog-api-key")
                .expect("x-goog-api-key"),
            &header_value("gm-test").unwrap()
        );
        assert_eq!(request.body["contents"][0]["parts"][0]["text"], "hi");
    }

    #[tokio::test]
    async fn ollama_check_request_uses_api_chat() {
        let provider = sample_provider(ProviderType::Ollama, "http://localhost:11434");
        let request = super::build_provider_check_request(
            &reqwest::Client::new(),
            &provider,
            0,
            "ollama-key",
            "llama3.1",
        )
        .await
        .expect("request should build");

        assert_eq!(request.url, "http://localhost:11434/api/chat");
        assert_eq!(request.body["model"], "llama3.1");
        assert_eq!(request.body["stream"], false);
        assert_eq!(request.body["messages"][0]["content"], "hi");
    }

    fn sample_provider(provider_type: ProviderType, endpoint: &str) -> Provider {
        Provider {
            id: 1,
            provider_key: "provider".to_string(),
            name: "provider".to_string(),
            endpoint: endpoint.to_string(),
            use_proxy: false,
            is_enabled: true,
            deleted_at: None,
            created_at: 0,
            updated_at: 0,
            provider_type,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        }
    }
}
