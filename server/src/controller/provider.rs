use crate::database::{
    DbResult,
    model::{Model, ModelDetail},
    model_route::ModelRoute,
    provider::{
        BootstrapProviderInput, BootstrapProviderResult, NewProvider, NewProviderApiKey, Provider,
        ProviderApiKey, ProviderSummaryItem, UpdateProviderApiKeyData, UpdateProviderData,
    },
    request_patch::{RequestPatchRule, RequestPatchRuleResponse},
};
use crate::proxy::{ProxyError, apply_request_patches, load_runtime_request_patch_trace};
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
use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
use crate::service::cache::types::{CacheModel, CacheProvider, CacheResolvedRequestPatch};

#[derive(Serialize)]
struct ProviderDetailResponse {
    provider: Provider,
    models: Vec<ModelDetail>,
    provider_keys: Vec<ProviderApiKey>,
    request_patches: Vec<RequestPatchRuleResponse>,
}

#[derive(Deserialize)]
struct BootstrapProviderPayload {
    endpoint: String,
    api_key: String,
    model_name: String,
    #[serde(default)]
    provider_type: ProviderType,
    name: Option<String>,
    key: Option<String>,
    real_model_name: Option<String>,
    #[serde(default)]
    use_proxy: bool,
    #[serde(default)]
    save_and_test: bool,
    api_key_description: Option<String>,
}

#[derive(Serialize)]
struct BootstrapCheckResult {
    success: bool,
    message: String,
}

#[derive(Serialize)]
struct BootstrapProviderResponse {
    provider: Provider,
    created_key: ProviderApiKey,
    created_model: Model,
    provider_name: String,
    provider_key: String,
    check_result: Option<BootstrapCheckResult>,
}

fn log_provider_audit(action: &'static str, provider: &Provider) {
    match action {
        "create" => crate::info_event!(
            "manager.provider_created",
            action = action,
            provider_id = provider.id,
            provider_key = &provider.provider_key,
            provider_name = &provider.name,
            is_enabled = provider.is_enabled,
        ),
        "update" => crate::info_event!(
            "manager.provider_updated",
            action = action,
            provider_id = provider.id,
            provider_key = &provider.provider_key,
            provider_name = &provider.name,
            is_enabled = provider.is_enabled,
        ),
        "delete" => crate::info_event!(
            "manager.provider_deleted",
            action = action,
            provider_id = provider.id,
            provider_key = &provider.provider_key,
            provider_name = &provider.name,
            is_enabled = provider.is_enabled,
        ),
        _ => unreachable!("unsupported provider audit action: {action}"),
    }
}

fn log_provider_api_key_audit(action: &'static str, key: &ProviderApiKey) {
    match action {
        "create" => crate::info_event!(
            "manager.provider_api_key_created",
            action = action,
            provider_id = key.provider_id,
            provider_api_key_id = key.id,
            is_enabled = key.is_enabled,
            description_present = key.description.is_some(),
        ),
        "update" => crate::info_event!(
            "manager.provider_api_key_updated",
            action = action,
            provider_id = key.provider_id,
            provider_api_key_id = key.id,
            is_enabled = key.is_enabled,
            description_present = key.description.is_some(),
        ),
        "delete" => crate::info_event!(
            "manager.provider_api_key_deleted",
            action = action,
            provider_id = key.provider_id,
            provider_api_key_id = key.id,
            is_enabled = key.is_enabled,
            description_present = key.description.is_some(),
        ),
        _ => unreachable!("unsupported provider api key audit action: {action}"),
    }
}

fn log_provider_bootstrap_audit(
    created: &BootstrapProviderResult,
    check_result: Option<&BootstrapCheckResult>,
) {
    crate::info_event!(
        "manager.provider_bootstrapped",
        action = "bootstrap",
        provider_id = created.provider.id,
        provider_key = &created.provider.provider_key,
        provider_name = &created.provider.name,
        is_enabled = created.provider.is_enabled,
        provider_api_key_id = created.created_key.id,
        model_id = created.created_model.id,
        model_name = &created.created_model.model_name,
        check_performed = check_result.is_some(),
        check_success = check_result.map(|result| result.success),
    );
}

async fn list() -> DbResult<HttpResult<Vec<Provider>>> {
    let result = Provider::list_all()?;
    Ok(HttpResult::new(result))
}

async fn list_summary() -> DbResult<HttpResult<Vec<ProviderSummaryItem>>> {
    let result = Provider::list_summary()?;
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

async fn insert(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<InserPayload>,
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
        provider_type: payload
            .provider_type
            .unwrap_or_else(|| ProviderType::Openai),
        provider_api_key_mode: payload
            .provider_api_key_mode
            .unwrap_or_else(|| ProviderApiKeyMode::Queue),
    };
    let created_provider = Provider::create(&new_provider_data)?;

    if let Err(e) = app_state.catalog.invalidate_models_catalog().await {
        warn!(
            "Failed to invalidate models catalog after provider create {}: {:?}",
            created_provider.id, e
        );
    }

    log_provider_audit("create", &created_provider);

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
        .catalog
        .invalidate_provider(id, Some(&updated_provider.provider_key))
        .await
    {
        warn!("Failed to invalidate Provider id {} in cache: {:?}", id, e);
    }

    log_provider_audit("update", &updated_provider);

    Ok(HttpResult::new(updated_provider))
}

async fn delete_provider(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    // Fetch provider details before deleting to get the key for store removal
    // Fetch provider details to ensure it exists before DB delete.
    // Not strictly needed for cache operation if ID is the only thing used, but good practice.
    let provider_to_delete_from_db = Provider::get_by_id(id)?;
    let affected_routes = ModelRoute::list_by_provider_id(id)?;

    match Provider::delete(id) {
        // This is DB soft-delete
        Ok(num_deleted_db) => {
            if num_deleted_db > 0 {
                if let Err(err) = ModelRoute::soft_delete_candidates_for_provider(id) {
                    warn!(
                        "Failed to delete model route candidates for deleted provider {}: {:?}",
                        id, err
                    );
                }

                for route in &affected_routes {
                    if let Err(store_err) = app_state
                        .catalog
                        .invalidate_model_route(route.id, Some(&route.route_name))
                        .await
                    {
                        warn!(
                            "Failed to invalidate model route {} after provider delete {}: {:?}",
                            route.id, id, store_err
                        );
                    }
                }

                if let Err(err) = ProviderApiKey::soft_delete_by_provider_id(id) {
                    warn!(
                        "Failed to delete provider API keys for deleted provider {}: {:?}",
                        id, err
                    );
                }

                if let Err(err) = RequestPatchRule::soft_delete_by_provider_id(id) {
                    warn!(
                        "Failed to delete provider request patch rules for deleted provider {}: {:?}",
                        id, err
                    );
                }

                // Invalidate provider cache (key not available after delete, so pass None)
                if let Err(e) = app_state.catalog.invalidate_provider(id, None).await {
                    warn!(
                        "Provider id {} successfully deleted from DB, but failed to invalidate cache: {:?}",
                        id, e
                    );
                }

                // Invalidate associated API keys cache
                if let Err(e) = app_state.catalog.invalidate_provider_api_keys(id).await {
                    warn!(
                        "Error invalidating provider API keys cache for provider {}: {:?}",
                        id, e
                    );
                }

                log_provider_audit("delete", &provider_to_delete_from_db);
            }
            Ok(HttpResult::new(())) // Success if DB operation was successful
        }
        Err(err) => Err(err), // DB operation failed
    }
}

async fn get_provider_detail(
    State(_app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ProviderDetailResponse>, BaseError> {
    let detail = Provider::get_detail_by_id(id)?;
    let models = Model::list_by_provider_id(id)?
        .into_iter()
        .map(|model| Model::get_detail_by_id(model.id))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(HttpResult::new(ProviderDetailResponse {
        provider: detail.provider,
        models,
        provider_keys: detail.api_keys,
        request_patches: detail.request_patches,
    }))
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

fn provider_check_patch_error(err: ProxyError) -> BaseError {
    let formatted_error = err.to_string();
    match err {
        ProxyError::BadRequest(message) | ProxyError::UpstreamBadRequest(message) => {
            BaseError::ParamInvalid(Some(message))
        }
        ProxyError::RequestPatchConflict(_) => {
            BaseError::InternalServerError(Some(formatted_error))
        }
        ProxyError::Unauthorized(message)
        | ProxyError::KeyDisabled(message)
        | ProxyError::KeyExpired(message)
        | ProxyError::Forbidden(message)
        | ProxyError::RateLimited(message)
        | ProxyError::ConcurrencyLimited(message)
        | ProxyError::QuotaExhausted(message)
        | ProxyError::BudgetExhausted(message)
        | ProxyError::ProviderOpenSkipped(message)
        | ProxyError::ProviderHalfOpenProbeInFlight(message)
        | ProxyError::PayloadTooLarge(message)
        | ProxyError::ClientCancelled(message)
        | ProxyError::InternalError(message)
        | ProxyError::ProtocolTransformError(message)
        | ProxyError::UpstreamRateLimited(message)
        | ProxyError::UpstreamAuthentication(message)
        | ProxyError::BadGateway(message)
        | ProxyError::UpstreamService(message)
        | ProxyError::UpstreamTimeout(message) => BaseError::InternalServerError(Some(message)),
    }
}

async fn resolve_provider_check_request_patches(
    app_state: &Arc<AppState>,
    provider: &Provider,
    model: Option<&Model>,
) -> Result<Vec<CacheResolvedRequestPatch>, BaseError> {
    let cache_provider = CacheProvider::from(provider.clone());
    let cache_model = model.cloned().map(CacheModel::from);
    let trace = load_runtime_request_patch_trace(&cache_provider, cache_model.as_ref(), app_state)
        .await
        .map_err(provider_check_patch_error)?;
    if let Some(model) = model {
        if let Some(conflict_error) = trace.conflict_error(&model.model_name) {
            return Err(provider_check_patch_error(conflict_error));
        }
    }

    Ok(trace.applied_rules)
}

async fn build_provider_check_request(
    client: &reqwest::Client,
    provider: &Provider,
    provider_api_key_id: i64,
    api_key: &str,
    model_name: &str,
    request_patches: &[CacheResolvedRequestPatch],
) -> Result<ProviderCheckRequest, BaseError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let mut request = match provider.provider_type {
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
        ProviderType::Openai | ProviderType::Responses | ProviderType::GeminiOpenai => {
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

    let mut url = Url::parse(&request.url).map_err(|e| {
        BaseError::ParamInvalid(Some(format!("Failed to parse request URL: {}", e)))
    })?;
    apply_request_patches(
        &mut request.body,
        &mut url,
        &mut request.headers,
        request_patches,
    )
    .map_err(provider_check_patch_error)?;
    request.url = url.to_string();

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

fn provider_type_label(provider_type: &ProviderType) -> &'static str {
    match provider_type {
        ProviderType::Openai => "OpenAI",
        ProviderType::Gemini => "Gemini",
        ProviderType::Vertex => "Vertex",
        ProviderType::VertexOpenai => "Vertex OpenAI",
        ProviderType::Ollama => "Ollama",
        ProviderType::Anthropic => "Anthropic",
        ProviderType::Responses => "Responses",
        ProviderType::GeminiOpenai => "Gemini OpenAI",
    }
}

fn endpoint_host(endpoint: &str) -> String {
    let trimmed = endpoint.trim();
    if let Ok(url) = Url::parse(trimmed) {
        if let Some(host) = url.host_str() {
            return match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            };
        }
    }

    trimmed
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_separator = false;

    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            if !slug.is_empty() {
                slug.push('-');
            }
            last_was_separator = true;
        }
    }

    slug.trim_matches('-').to_string()
}

fn generated_provider_name(provider_type: &ProviderType, endpoint: &str) -> String {
    let host = endpoint_host(endpoint);
    if host.is_empty() {
        provider_type_label(provider_type).to_string()
    } else {
        format!("{} {}", provider_type_label(provider_type), host)
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn generated_provider_key(
    provider_type: &ProviderType,
    endpoint: &str,
    provider_name: &str,
) -> String {
    let fallback = generated_provider_name(provider_type, endpoint);
    let candidate = slugify(provider_name);
    if !candidate.is_empty() {
        candidate
    } else {
        let fallback_slug = slugify(&fallback);
        if !fallback_slug.is_empty() {
            fallback_slug
        } else {
            slugify(provider_type_label(provider_type))
        }
    }
}

fn base_error_message(error: &BaseError) -> String {
    match error {
        BaseError::ParamInvalid(msg) => msg
            .clone()
            .unwrap_or_else(|| "request params invalid".to_string()),
        BaseError::DatabaseFatal(msg) => msg
            .clone()
            .unwrap_or_else(|| "database unknown error".to_string()),
        BaseError::DatabaseDup(msg) => msg
            .clone()
            .unwrap_or_else(|| "some unique keys have conflicted".to_string()),
        BaseError::NotFound(msg) => msg.clone().unwrap_or_else(|| "data not found".to_string()),
        BaseError::Unauthorized(msg) => msg.clone().unwrap_or_else(|| "Unauthorized".to_string()),
        BaseError::StoreError(msg) => msg
            .clone()
            .unwrap_or_else(|| "Application cache/store operation failed".to_string()),
        BaseError::InternalServerError(msg) => msg
            .clone()
            .unwrap_or_else(|| "internal server error".to_string()),
    }
}

fn resolve_bootstrap_identity(
    provider_type: &ProviderType,
    endpoint: &str,
    name: Option<String>,
    key: Option<String>,
) -> Result<(String, String), BaseError> {
    let provider_name = normalize_optional_text(name)
        .unwrap_or_else(|| generated_provider_name(provider_type, endpoint));

    if let Some(explicit_key) = normalize_optional_text(key) {
        return Ok((provider_name, explicit_key));
    }

    let base_key = generated_provider_key(provider_type, endpoint, &provider_name);
    let mut candidate = if base_key.is_empty() {
        slugify(&generated_provider_name(provider_type, endpoint))
    } else {
        base_key
    };

    if candidate.is_empty() {
        candidate = slugify(provider_type_label(provider_type));
    }

    let base_candidate = candidate.clone();
    let mut suffix = 2;
    while Provider::get_by_key(&candidate)?.is_some() {
        candidate = format!("{}-{}", base_candidate, suffix);
        suffix += 1;
    }

    Ok((provider_name, candidate))
}

async fn perform_provider_check(
    app_state: &Arc<AppState>,
    client: &reqwest::Client,
    provider: &Provider,
    model: Option<&Model>,
    provider_api_key_id: i64,
    api_key: &str,
    model_name: &str,
) -> Result<(), BaseError> {
    let request_patches =
        resolve_provider_check_request_patches(app_state, provider, model).await?;
    let check_request = build_provider_check_request(
        client,
        provider,
        provider_api_key_id,
        api_key,
        model_name,
        &request_patches,
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

    let _ = response.text().await;
    Ok(())
}

fn build_bootstrap_response(
    created: BootstrapProviderResult,
    provider_name: String,
    provider_key: String,
    check_result: Option<BootstrapCheckResult>,
) -> BootstrapProviderResponse {
    BootstrapProviderResponse {
        provider: created.provider,
        created_key: created.created_key,
        created_model: created.created_model,
        provider_name,
        provider_key,
        check_result,
    }
}

async fn check_provider(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<CheckProviderPayload>,
) -> Result<HttpResult<Value>, BaseError> {
    let mut selected_model: Option<Model> = None;
    let model_name = match (payload.model_id, payload.model_name) {
        (Some(model_id), _) => {
            let model = Model::get_by_id(model_id)?;
            if model.provider_id != id {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "Model {} does not belong to provider {}",
                    model_id, id
                ))));
            }
            let resolved_name = model
                .real_model_name
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| model.model_name.clone());
            selected_model = Some(model);
            resolved_name
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
        app_state.infra.proxy_client()
    } else {
        app_state.infra.client()
    };

    perform_provider_check(
        &app_state,
        client,
        &provider,
        selected_model.as_ref(),
        provider_api_key_id,
        &api_key,
        &model_name,
    )
    .await?;
    Ok(HttpResult::new(serde_json::Value::Null))
}

async fn bootstrap_provider(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<BootstrapProviderPayload>,
) -> Result<HttpResult<BootstrapProviderResponse>, BaseError> {
    let (provider_name, provider_key) = resolve_bootstrap_identity(
        &payload.provider_type,
        &payload.endpoint,
        payload.name.clone(),
        payload.key.clone(),
    )?;

    let provider_input = BootstrapProviderInput {
        provider_id: ID_GENERATOR.generate_id(),
        provider_key: provider_key.clone(),
        name: provider_name.clone(),
        endpoint: payload.endpoint.clone(),
        use_proxy: payload.use_proxy,
        provider_type: payload.provider_type.clone(),
        provider_api_key_mode: ProviderApiKeyMode::Queue,
        api_key: payload.api_key.clone(),
        api_key_description: normalize_optional_text(payload.api_key_description.clone()),
        model_name: payload.model_name.clone(),
        real_model_name: normalize_optional_text(payload.real_model_name.clone()),
    };

    let created = Provider::bootstrap(&provider_input)?;

    if let Err(e) = app_state.catalog.invalidate_models_catalog().await {
        warn!(
            "Failed to invalidate models catalog after bootstrap provider {}: {:?}",
            created.provider.id, e
        );
    }

    if let Err(e) = app_state
        .catalog
        .invalidate_provider(created.provider.id, Some(&created.provider.provider_key))
        .await
    {
        warn!(
            "Failed to invalidate provider cache after bootstrap provider {}: {:?}",
            created.provider.id, e
        );
    }

    if let Err(e) = app_state
        .catalog
        .invalidate_provider_api_keys(created.provider.id)
        .await
    {
        warn!(
            "Failed to invalidate provider API keys cache after bootstrap provider {}: {:?}",
            created.provider.id, e
        );
    }

    let check_result = if payload.save_and_test {
        let client = if created.provider.use_proxy {
            app_state.infra.proxy_client()
        } else {
            app_state.infra.client()
        };
        let model_name_to_check = created
            .created_model
            .real_model_name
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| created.created_model.model_name.clone());

        match perform_provider_check(
            &app_state,
            client,
            &created.provider,
            Some(&created.created_model),
            created.created_key.id,
            &created.created_key.api_key,
            &model_name_to_check,
        )
        .await
        {
            Ok(()) => Some(BootstrapCheckResult {
                success: true,
                message: "Provider check succeeded".to_string(),
            }),
            Err(e) => Some(BootstrapCheckResult {
                success: false,
                message: base_error_message(&e),
            }),
        }
    } else {
        None
    };

    log_provider_bootstrap_audit(&created, check_result.as_ref());

    Ok(HttpResult::new(build_bootstrap_response(
        created,
        provider_name,
        provider_key,
        check_result,
    )))
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
        app_state.infra.proxy_client()
    } else {
        app_state.infra.client()
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

async fn list_provider_details(
    State(_app_state): State<Arc<AppState>>,
) -> Result<(StatusCode, HttpResult<Vec<ProviderDetailResponse>>), BaseError> {
    let providers = Provider::list_all()?;
    let mut provider_details: Vec<ProviderDetailResponse> = Vec::new();

    for provider in providers {
        let detail = Provider::get_detail_by_id(provider.id)?;
        let models = Model::list_by_provider_id(provider.id)?
            .into_iter()
            .map(|model| Model::get_detail_by_id(model.id))
            .collect::<Result<Vec<_>, _>>()?;

        provider_details.push(ProviderDetailResponse {
            provider: detail.provider,
            models,
            provider_keys: detail.api_keys,
            request_patches: detail.request_patches,
        });
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
    if let Err(e) = app_state
        .catalog
        .invalidate_provider_api_keys(provider_id)
        .await
    {
        warn!(
            "Failed to invalidate provider API keys cache for provider {}: {:?}",
            provider_id, e
        );
    }

    log_provider_api_key_audit("create", &created_key);

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
    if let Err(e) = app_state
        .catalog
        .invalidate_provider_api_keys(provider_id)
        .await
    {
        warn!(
            "Failed to invalidate provider API keys cache for provider {}: {:?}",
            provider_id, e
        );
    }

    log_provider_api_key_audit("update", &updated_key);

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
    if let Err(e) = app_state
        .catalog
        .invalidate_provider_api_keys(provider_id)
        .await
    {
        warn!(
            "Failed to invalidate provider API keys cache for provider {}: {:?}",
            provider_id, e
        );
    }

    log_provider_api_key_audit("delete", &key_to_delete_from_db);

    Ok(HttpResult::new(()))
}

pub fn create_provider_router() -> StateRouter {
    create_state_router().nest(
        "/provider",
        create_state_router()
            .route("/", post(insert))
            .route("/bootstrap", post(bootstrap_provider))
            // .route("/commit", post(full_commit)) // Removed full_commit route
            .route("/summary/list", get(list_summary))
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
    use crate::database::model::Model;
    use crate::database::provider::Provider;
    use crate::database::provider::ProviderSummaryItem;
    use crate::schema::enum_def::{
        ProviderApiKeyMode, ProviderType, RequestPatchOperation, RequestPatchPlacement,
    };
    use crate::service::cache::types::{CacheResolvedRequestPatch, RequestPatchRuleOrigin};
    use crate::utils::HttpResult;
    use std::collections::BTreeSet;

    fn request_patch(
        id: i64,
        placement: RequestPatchPlacement,
        target: &str,
        operation: RequestPatchOperation,
        value: Option<serde_json::Value>,
    ) -> CacheResolvedRequestPatch {
        CacheResolvedRequestPatch {
            placement,
            target: target.to_string(),
            operation,
            value_json: value.map(|item| serde_json::to_string(&item).unwrap()),
            source_rule_id: id,
            source_origin: RequestPatchRuleOrigin::ProviderDirect,
            overridden_rule_ids: Vec::new(),
            description: None,
        }
    }

    #[tokio::test]
    async fn openai_style_check_request_uses_chat_completions() {
        let provider = sample_provider(ProviderType::Openai, "https://api.example.com/v1");
        let request = super::build_provider_check_request(
            &reqwest::Client::new(),
            &provider,
            0,
            "sk-test",
            "gpt-4o-mini",
            &[],
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
    async fn gemini_openai_check_request_uses_openai_chat_completions() {
        let provider = sample_provider(
            ProviderType::GeminiOpenai,
            "https://generativelanguage.googleapis.com/v1beta/openai",
        );
        let request = super::build_provider_check_request(
            &reqwest::Client::new(),
            &provider,
            0,
            "sk-gemini",
            "gemini-2.5-flash",
            &[],
        )
        .await
        .expect("request should build");

        assert_eq!(
            request.url,
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            request
                .headers
                .get(reqwest::header::AUTHORIZATION)
                .expect("auth header"),
            &header_value("Bearer sk-gemini").unwrap()
        );
        assert_eq!(request.body["model"], "gemini-2.5-flash");
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
            &[],
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
            &[],
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
            &[],
        )
        .await
        .expect("request should build");

        assert_eq!(request.url, "http://localhost:11434/api/chat");
        assert_eq!(request.body["model"], "llama3.1");
        assert_eq!(request.body["stream"], false);
        assert_eq!(request.body["messages"][0]["content"], "hi");
    }

    #[tokio::test]
    async fn provider_check_request_applies_request_patches_to_body_query_and_headers() {
        let provider = sample_provider(ProviderType::Openai, "https://api.example.com/v1");
        let request_patches = vec![
            request_patch(
                1,
                RequestPatchPlacement::Header,
                "x-check-mode",
                RequestPatchOperation::Set,
                Some(serde_json::json!("strict")),
            ),
            request_patch(
                2,
                RequestPatchPlacement::Query,
                "trace",
                RequestPatchOperation::Set,
                Some(serde_json::json!(true)),
            ),
            request_patch(
                3,
                RequestPatchPlacement::Body,
                "/messages/0/content",
                RequestPatchOperation::Set,
                Some(serde_json::json!("patched")),
            ),
        ];
        let request = super::build_provider_check_request(
            &reqwest::Client::new(),
            &provider,
            0,
            "sk-test",
            "gpt-4o-mini",
            &request_patches,
        )
        .await
        .expect("request should build");

        assert_eq!(
            request.url,
            "https://api.example.com/v1/chat/completions?trace=true"
        );
        assert_eq!(
            request.headers.get("x-check-mode").expect("patched header"),
            "strict"
        );
        assert_eq!(request.body["messages"][0]["content"], "patched");
    }

    #[test]
    fn provider_check_request_patch_conflict_preserves_proxy_error_code() {
        let error =
            super::provider_check_patch_error(crate::proxy::ProxyError::RequestPatchConflict(
                "conflicting request patch rules".to_string(),
            ));

        let message = super::base_error_message(&error);
        assert!(message.contains("request_patch_conflict_error"));
        assert!(message.contains("conflicting request patch rules"));
    }

    #[test]
    fn bootstrap_provider_defaults_use_provider_type_and_endpoint_host() {
        assert_eq!(
            super::generated_provider_name(&ProviderType::Openai, "https://api.example.com/v1"),
            "OpenAI api.example.com"
        );
        assert_eq!(
            super::generated_provider_key(
                &ProviderType::Openai,
                "https://api.example.com/v1",
                "OpenAI api.example.com"
            ),
            "openai-api-example-com"
        );
    }

    #[test]
    fn bootstrap_provider_response_without_check_result_remains_null() {
        let response = super::build_bootstrap_response(
            sample_bootstrap_result(),
            "OpenAI api.example.com".to_string(),
            "openai-api-example-com".to_string(),
            None,
        );

        assert_eq!(response.provider_name, "OpenAI api.example.com");
        assert_eq!(response.provider_key, "openai-api-example-com");
        assert!(response.check_result.is_none());
    }

    #[test]
    fn bootstrap_provider_response_can_carry_check_failure() {
        let response = super::build_bootstrap_response(
            sample_bootstrap_result(),
            "OpenAI api.example.com".to_string(),
            "openai-api-example-com".to_string(),
            Some(super::BootstrapCheckResult {
                success: false,
                message: "boom".to_string(),
            }),
        );

        let check_result = response.check_result.expect("check result should exist");
        assert!(!check_result.success);
        assert_eq!(check_result.message, "boom");
    }

    #[test]
    fn provider_summary_api_contract_serializes_lightweight_rows() {
        let payload = HttpResult::new(vec![ProviderSummaryItem {
            id: 42,
            provider_key: "openai-api-example-com".to_string(),
            name: "OpenAI api.example.com".to_string(),
            is_enabled: true,
        }]);

        let value = serde_json::to_value(payload).expect("summary payload should serialize");
        let root = value.as_object().expect("payload should be an object");
        assert_eq!(
            root.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from(["code".to_string(), "data".to_string()])
        );
        assert_eq!(root["code"], 0);

        let items = root["data"].as_array().expect("data should be an array");
        let item = items[0]
            .as_object()
            .expect("summary row should be an object");
        assert_eq!(
            item.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "id".to_string(),
                "provider_key".to_string(),
                "name".to_string(),
                "is_enabled".to_string(),
            ])
        );
        assert!(item.get("models").is_none());
        assert!(item.get("provider_keys").is_none());
        assert!(item.get("custom_fields").is_none());
    }

    fn sample_bootstrap_result() -> super::BootstrapProviderResult {
        super::BootstrapProviderResult {
            provider: sample_provider(ProviderType::Openai, "https://api.example.com/v1"),
            created_key: super::ProviderApiKey {
                id: 2,
                provider_id: 1,
                api_key: "sk-test".to_string(),
                description: Some("bootstrap key".to_string()),
                deleted_at: None,
                is_enabled: true,
                created_at: 0,
                updated_at: 0,
            },
            created_model: Model {
                id: 3,
                provider_id: 1,
                model_name: "gpt-4o-mini".to_string(),
                real_model_name: None,
                cost_catalog_id: None,
                supports_streaming: true,
                supports_tools: true,
                supports_reasoning: true,
                supports_image_input: true,
                supports_embeddings: true,
                supports_rerank: true,
                deleted_at: None,
                is_enabled: true,
                created_at: 0,
                updated_at: 0,
            },
        }
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
