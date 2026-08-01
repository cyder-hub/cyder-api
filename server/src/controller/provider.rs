use crate::database::{
    DbResult,
    model::{Model, ModelDetail},
    provider::{BootstrapProviderResult, Provider, ProviderApiKeySummary, ProviderSummaryItem},
    request_patch::RequestPatchRuleResponse,
};
use crate::proxy::{
    ProxyError, ProxyErrorCode, apply_request_patches, load_runtime_request_patch_trace,
};
use crate::service::admin::provider::{
    BootstrapProviderCommand, CreateProviderApiKeyInput, ProviderApiKeyReveal, ProviderUpsertInput,
    ReplaceProviderApiKeyInput, UpdateProviderApiKeyInput,
};
use crate::service::app_state::{AppState, StateRouter, create_state_router}; // Added AppState
use axum::{
    Extension,
    extract::{Json, Path, State}, // Added State
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use reqwest::{
    StatusCode, Url,
    header::{CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc; // Added Arc

use crate::utils::{HttpResult, ID_GENERATOR, auth::ManagerAuthContext};

use super::{BaseError, auth::authorize_secret_governance_command};
use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
use crate::service::cache::types::{CacheModel, CacheProvider, RuntimeResolvedRequestPatch};
use crate::service::provider_credential::{
    ProviderCredential, ProviderCredentialError, apply_provider_request_auth_header,
    provider_upstream_protocol, resolve_draft_provider_credential,
    resolve_saved_provider_credential, resolve_selected_provider_credential,
};
use crate::service::provider_http::normalize_provider_endpoint;
use crate::service::secret_encryption::SensitiveSecret;

#[derive(Serialize)]
struct ProviderDetailResponse {
    provider: Provider,
    models: Vec<ModelDetail>,
    provider_keys: Vec<ProviderApiKeySummary>,
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
    created_key: ProviderApiKeySummary,
    created_model: Model,
    provider_name: String,
    provider_key: String,
    check_result: Option<BootstrapCheckResult>,
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
    let created_provider = app_state
        .admin
        .provider
        .create_provider(ProviderUpsertInput {
            name: payload.name,
            key: payload.key,
            endpoint: payload.endpoint,
            use_proxy: payload.use_proxy,
            provider_type: payload.provider_type,
            provider_api_key_mode: payload.provider_api_key_mode,
        })
        .await?;

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
    let updated_provider = app_state
        .admin
        .provider
        .update_provider(
            id,
            ProviderUpsertInput {
                name: payload.name,
                key: payload.key,
                endpoint: payload.endpoint,
                use_proxy: payload.use_proxy,
                provider_type: payload.provider_type,
                provider_api_key_mode: payload.provider_api_key_mode,
            },
        )
        .await?;

    Ok(HttpResult::new(updated_provider))
}

async fn delete_provider(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    app_state.admin.provider.delete_provider(id).await?;
    Ok(HttpResult::new(()))
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
    match err.code() {
        ProxyErrorCode::InvalidRequestError | ProxyErrorCode::UpstreamInvalidRequestError => {
            BaseError::ParamInvalid(Some(err.operator_message().to_string()))
        }
        _ => BaseError::InternalServerError(Some(formatted_error)),
    }
}

async fn resolve_provider_check_request_patches(
    app_state: &Arc<AppState>,
    provider: &Provider,
    model: Option<&Model>,
) -> Result<Vec<RuntimeResolvedRequestPatch>, BaseError> {
    let cache_provider = CacheProvider::from(provider.clone());
    let cache_model = model.cloned().map(CacheModel::from);
    let trace =
        load_runtime_request_patch_trace(&cache_provider, cache_model.as_ref(), None, app_state)
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
    provider: &Provider,
    credential: &ProviderCredential,
    model_name: &str,
    request_patches: &[RuntimeResolvedRequestPatch],
) -> Result<ProviderCheckRequest, BaseError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let cache_provider = CacheProvider::from(provider.clone());
    apply_provider_request_auth_header(
        &mut headers,
        &cache_provider,
        provider_upstream_protocol(&cache_provider.provider_type),
        credential,
    )
    .map_err(provider_credential_error)?;

    let mut request = match provider.provider_type {
        ProviderType::Gemini => ProviderCheckRequest {
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
        },
        ProviderType::Vertex => ProviderCheckRequest {
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
        },
        ProviderType::VertexOpenai => ProviderCheckRequest {
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
        },
        ProviderType::Anthropic => {
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
        ProviderType::Ollama => ProviderCheckRequest {
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
        },
        ProviderType::Openai | ProviderType::Responses | ProviderType::GeminiOpenai => {
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

fn provider_credential_error(error: ProviderCredentialError) -> BaseError {
    match error {
        ProviderCredentialError::CredentialUnavailable => {
            BaseError::ProviderApiKeySecretUnavailable
        }
        ProviderCredentialError::RuntimeStateUnavailable => BaseError::ProviderRuntimeRefreshFailed,
        ProviderCredentialError::NoEnabledCredential
        | ProviderCredentialError::VertexTokenUnavailable
        | ProviderCredentialError::ProxyRequiredButNotConfigured
        | ProviderCredentialError::UnsupportedProtocol
        | ProviderCredentialError::InvalidAuthHeader => {
            BaseError::ParamInvalid(Some(error.to_string()))
        }
    }
}

fn normalize_provider_for_outbound(mut provider: Provider) -> Result<Provider, BaseError> {
    provider.endpoint = normalize_provider_endpoint(&provider.endpoint).map_err(|error| {
        BaseError::ParamInvalid(Some(format!(
            "provider endpoint is invalid and must be repaired before use: {error}"
        )))
    })?;
    Ok(provider)
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
        BaseError::ApiKeySecretUnavailable => "api key secret is unavailable".to_string(),
        BaseError::ProviderApiKeySecretUnavailable => {
            "provider API key secret is unavailable; replace the credential".to_string()
        }
        BaseError::ProviderRuntimeRefreshFailed => {
            "provider credential change was committed, but runtime refresh failed".to_string()
        }
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

    let provider_key = normalize_optional_text(key).ok_or_else(|| {
        BaseError::ParamInvalid(Some("provider_key must be provided".to_string()))
    })?;

    Ok((provider_name, provider_key))
}

async fn perform_provider_check(
    client: &reqwest::Client,
    provider: &Provider,
    credential: &ProviderCredential,
    model_name: &str,
    request_patches: &[RuntimeResolvedRequestPatch],
) -> Result<(), BaseError> {
    let check_request =
        build_provider_check_request(provider, credential, model_name, request_patches).await?;

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
        return Err(BaseError::ParamInvalid(Some(format!(
            "Provider API returned status {}",
            status
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

    let provider = normalize_provider_for_outbound(Provider::get_by_id(id)?)?;
    let request_patches =
        resolve_provider_check_request_patches(&app_state, &provider, selected_model.as_ref())
            .await?;
    let credential = match (payload.provider_api_key_id, payload.provider_api_key) {
        (Some(key_id), _) => resolve_saved_provider_credential(&provider, key_id, &app_state)
            .await
            .map_err(provider_credential_error)?,
        (_, Some(api_key)) => resolve_draft_provider_credential(
            &provider,
            0,
            SensitiveSecret::new(api_key),
            &app_state,
        )
        .await
        .map_err(provider_credential_error)?,
        (None, None) => {
            return Err(BaseError::ParamInvalid(Some(
                "Either provider_api_key_id or provider_api_key must be provided.".to_string(),
            )));
        }
    };

    let client = app_state
        .infra
        .provider_client(provider.use_proxy)
        .await
        .map_err(|error| BaseError::ParamInvalid(Some(error.to_string())))?;

    perform_provider_check(
        client.as_ref(),
        &provider,
        &credential,
        &model_name,
        &request_patches,
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

    let provider_input = BootstrapProviderCommand {
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

    let created = app_state
        .admin
        .provider
        .bootstrap_provider_persist(provider_input)
        .await?;

    let check_result = if payload.save_and_test {
        let client = app_state
            .infra
            .provider_client(created.provider.use_proxy)
            .await;
        let model_name_to_check = created
            .created_model
            .real_model_name
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| created.created_model.model_name.clone());

        let request_patches = resolve_provider_check_request_patches(
            &app_state,
            &created.provider,
            Some(&created.created_model),
        )
        .await;

        let credential_and_patches = match (client, request_patches) {
            (Ok(client), Ok(request_patches)) => resolve_draft_provider_credential(
                &created.provider,
                created.created_key.id,
                SensitiveSecret::new(payload.api_key),
                &app_state,
            )
            .await
            .map(|credential| (client, credential, request_patches))
            .map_err(provider_credential_error),
            (Err(error), _) => Err(BaseError::ParamInvalid(Some(error.to_string()))),
            (_, Err(error)) => Err(error),
        };
        match credential_and_patches {
            Err(error) => Some(BootstrapCheckResult {
                success: false,
                message: base_error_message(&error),
            }),
            Ok((client, credential, request_patches)) => match perform_provider_check(
                client.as_ref(),
                &created.provider,
                &credential,
                &model_name_to_check,
                &request_patches,
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
            },
        }
    } else {
        None
    };

    app_state
        .admin
        .provider
        .record_bootstrap_audit(&created, check_result.as_ref().map(|result| result.success))
        .await;

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
    let provider = normalize_provider_for_outbound(Provider::get_by_id(id)?)?;
    let cache_provider = CacheProvider::from(provider.clone());
    let credential = resolve_selected_provider_credential(&cache_provider, &app_state)
        .await
        .map_err(provider_credential_error)?;

    let client = app_state
        .infra
        .provider_client(provider.use_proxy)
        .await
        .map_err(|error| BaseError::ParamInvalid(Some(error.to_string())))?;

    let (url, headers) = build_remote_models_request(&provider, &cache_provider, &credential)?;
    let response = client.get(url).headers(headers).send().await.map_err(|e| {
        BaseError::ParamInvalid(Some(format!("Failed to fetch remote models: {}", e)))
    })?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(BaseError::ParamInvalid(Some(format!(
            "Provider API returned status {}",
            status
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

fn build_remote_models_request(
    provider: &Provider,
    cache_provider: &CacheProvider,
    credential: &ProviderCredential,
) -> Result<(Url, HeaderMap), BaseError> {
    let url = if matches!(
        provider.provider_type,
        ProviderType::Gemini | ProviderType::Vertex
    ) {
        Url::parse(&provider.endpoint).map_err(|e| {
            BaseError::ParamInvalid(Some(format!(
                "Failed to parse provider endpoint as URL: {}",
                e
            )))
        })?
    } else {
        Url::parse(&format!(
            "{}/models",
            provider.endpoint.trim_end_matches('/')
        ))
        .map_err(|e| {
            BaseError::ParamInvalid(Some(format!(
                "Failed to parse provider endpoint as URL: {}",
                e
            )))
        })?
    };
    let mut headers = HeaderMap::new();
    apply_provider_request_auth_header(
        &mut headers,
        &cache_provider,
        provider_upstream_protocol(&cache_provider.provider_type),
        &credential,
    )
    .map_err(provider_credential_error)?;
    Ok((url, headers))
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
#[serde(deny_unknown_fields)]
struct CreateProviderApiKeyPayload {
    api_key: String,
    description: Option<String>,
}

async fn add_provider_api_key(
    State(app_state): State<Arc<AppState>>, // Added AppState
    Path(provider_id): Path<i64>,
    Json(payload): Json<CreateProviderApiKeyPayload>,
) -> Result<HttpResult<ProviderApiKeySummary>, BaseError> {
    let created_key = app_state
        .admin
        .provider
        .create_provider_api_key(
            provider_id,
            CreateProviderApiKeyInput {
                api_key: payload.api_key,
                description: payload.description,
            },
        )
        .await?;

    Ok(HttpResult::new(created_key))
}

async fn list_provider_api_keys(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
) -> Result<HttpResult<Vec<ProviderApiKeySummary>>, BaseError> {
    let keys = app_state
        .admin
        .provider
        .list_provider_api_keys(provider_id)?;

    Ok(HttpResult::new(keys))
}

async fn get_provider_api_key(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, key_id)): Path<(i64, i64)>,
) -> Result<HttpResult<ProviderApiKeySummary>, BaseError> {
    let key = app_state
        .admin
        .provider
        .get_provider_api_key(provider_id, key_id)?;

    Ok(HttpResult::new(key))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateProviderApiKeyPayload {
    description: Option<String>, // To clear description, send null or handle empty string as None
    is_enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceProviderApiKeyPayload {
    api_key: String,
}

async fn replace_provider_api_key(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, key_id)): Path<(i64, i64)>,
    Json(payload): Json<ReplaceProviderApiKeyPayload>,
) -> Result<HttpResult<ProviderApiKeySummary>, BaseError> {
    let updated = app_state
        .admin
        .provider
        .replace_provider_api_key(
            provider_id,
            key_id,
            ReplaceProviderApiKeyInput {
                api_key: payload.api_key,
            },
        )
        .await?;
    Ok(HttpResult::new(updated))
}

async fn reveal_provider_api_key(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_context): Extension<ManagerAuthContext>,
    Path((provider_id, key_id)): Path<(i64, i64)>,
) -> Result<HttpResult<ProviderApiKeyReveal>, Response> {
    authorize_secret_governance_command(&app_state, &auth_context)?;
    let revealed = app_state
        .admin
        .provider
        .reveal_provider_api_key(provider_id, key_id)
        .await
        .map_err(IntoResponse::into_response)?;
    Ok(HttpResult::new(revealed))
}

async fn update_provider_api_key(
    State(app_state): State<Arc<AppState>>, // Added AppState
    Path((provider_id, key_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateProviderApiKeyPayload>,
) -> Result<HttpResult<ProviderApiKeySummary>, BaseError> {
    let updated_key = app_state
        .admin
        .provider
        .update_provider_api_key(
            provider_id,
            key_id,
            UpdateProviderApiKeyInput {
                description: payload.description,
                is_enabled: payload.is_enabled,
            },
        )
        .await?;

    Ok(HttpResult::new(updated_key))
}

async fn delete_provider_api_key(
    State(app_state): State<Arc<AppState>>, // Added AppState
    Path((provider_id, key_id)): Path<(i64, i64)>,
) -> Result<HttpResult<()>, BaseError> {
    app_state
        .admin
        .provider
        .delete_provider_api_key(provider_id, key_id)
        .await?;

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
            .route(
                "/{id}/provider_keys",
                get(list_provider_api_keys).post(add_provider_api_key),
            )
            .route(
                "/{id}/provider_keys/{key_id}",
                get(get_provider_api_key)
                    .put(update_provider_api_key)
                    .delete(delete_provider_api_key),
            )
            .route(
                "/{id}/provider_keys/{key_id}/replace",
                post(replace_provider_api_key),
            )
            .route(
                "/{id}/provider_keys/{key_id}/reveal",
                post(reveal_provider_api_key),
            ),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::{net::SocketAddr, sync::Arc};

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode, header::CONTENT_TYPE},
    };
    use serde_json::{Value, json};
    use tower::util::ServiceExt;

    use super::create_provider_router;
    use crate::controller::BaseError;
    use crate::database::TestDbContext;
    use crate::database::model::Model;
    use crate::database::provider::ProviderSummaryItem;
    use crate::database::provider::{Provider, ProviderApiKeyRepository, ProviderApiKeySummary};
    use crate::ingress::client_identity::{ClientIdentity, ClientIdentitySource};
    use crate::schema::enum_def::{
        ProviderApiKeyMode, ProviderType, RequestPatchOperation, RequestPatchPlacement,
    };
    use crate::service::app_state::{AppState, create_test_app_state};
    use crate::service::cache::types::{
        CacheProvider, RequestPatchRuleOrigin, RequestPatchSource, RuntimeResolvedRequestPatch,
    };
    use crate::service::provider_credential::ProviderCredential;
    use crate::service::secret_encryption::SecretDomain;
    use crate::service::vertex::{cache_vertex_token_for_test, vertex_token_is_cached_for_test};
    use crate::utils::HttpResult;
    use crate::utils::auth::decode_access_token;

    fn request_patch(
        id: i64,
        placement: RequestPatchPlacement,
        target: &str,
        operation: RequestPatchOperation,
        value: Option<serde_json::Value>,
    ) -> RuntimeResolvedRequestPatch {
        RuntimeResolvedRequestPatch {
            placement,
            target: target.to_string(),
            operation,
            value_json: value.map(|item| serde_json::to_string(&item).unwrap()),
            source: RequestPatchSource::ProviderRule { rule_id: id },
            source_rule_id: Some(id),
            source_origin: Some(RequestPatchRuleOrigin::ProviderDirect),
            overridden_rule_ids: Vec::new(),
            overridden_sources: Vec::new(),
            description: None,
        }
    }

    fn credential(secret: &str) -> ProviderCredential {
        ProviderCredential::for_test(0, secret)
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_provider_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("provider router should respond")
    }

    fn empty_request(method: Method, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request should build")
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

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

    #[tokio::test]
    async fn openai_style_check_request_uses_chat_completions() {
        let provider = sample_provider(ProviderType::Openai, "https://api.example.com/v1");
        let request = super::build_provider_check_request(
            &provider,
            &credential("sk-test"),
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
            "Bearer sk-test"
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
            &provider,
            &credential("sk-gemini"),
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
            "Bearer sk-gemini"
        );
        assert_eq!(request.body["model"], "gemini-2.5-flash");
        assert_eq!(request.body["messages"][0]["content"], "hi");
    }

    #[tokio::test]
    async fn anthropic_check_request_uses_messages_and_version_header() {
        let provider = sample_provider(ProviderType::Anthropic, "https://api.anthropic.com/v1");
        let request = super::build_provider_check_request(
            &provider,
            &credential("ak-test"),
            "claude-3-5-haiku-latest",
            &[],
        )
        .await
        .expect("request should build");

        assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(
            request.headers.get("x-api-key").expect("x-api-key"),
            "ak-test"
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
            &provider,
            &credential("gm-test"),
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
            "gm-test"
        );
        assert_eq!(request.body["contents"][0]["parts"][0]["text"], "hi");
    }

    #[tokio::test]
    async fn ollama_check_request_uses_api_chat() {
        let provider = sample_provider(ProviderType::Ollama, "http://localhost:11434");
        let request = super::build_provider_check_request(
            &provider,
            &credential("ollama-key"),
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
            &provider,
            &credential("sk-test"),
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
    fn remote_models_uses_shared_auth_headers_and_never_query_credentials() {
        for (provider_type, endpoint, expected_header, expected_path) in [
            (
                ProviderType::Openai,
                "https://api.example.com/v1",
                "authorization",
                "/v1/models",
            ),
            (
                ProviderType::Gemini,
                "https://api.example.com/v1/models",
                "x-goog-api-key",
                "/v1/models",
            ),
            (
                ProviderType::Anthropic,
                "https://api.example.com/v1",
                "x-api-key",
                "/v1/models",
            ),
        ] {
            let provider = sample_provider(provider_type, endpoint);
            let cache_provider = CacheProvider::from(provider.clone());
            let (url, headers) = super::build_remote_models_request(
                &provider,
                &cache_provider,
                &credential("remote-secret"),
            )
            .expect("remote models request should build");

            assert_eq!(url.path(), expected_path);
            assert!(url.query().is_none());
            assert!(headers.get(expected_header).is_some());
        }
    }

    #[test]
    fn provider_check_request_patch_conflict_preserves_proxy_error_code() {
        let error = super::provider_check_patch_error(crate::proxy::ProxyError::gateway(
            crate::proxy::ProxyErrorCode::RequestPatchConflictError,
            crate::proxy::ExecutionStage::Patch,
            crate::proxy::ResponseVisibility::NotVisible,
            None,
            "conflicting request patch rules",
        ));

        let message = super::base_error_message(&error);
        assert!(message.contains("request_patch_conflict_error"));
        assert!(message.contains("conflicting request patch rules"));
    }

    #[test]
    fn bootstrap_provider_defaults_name_and_requires_explicit_key() {
        assert_eq!(
            super::generated_provider_name(&ProviderType::Openai, "https://api.example.com/v1"),
            "OpenAI api.example.com"
        );

        let (provider_name, provider_key) = super::resolve_bootstrap_identity(
            &ProviderType::Openai,
            "https://api.example.com/v1",
            None,
            Some("  ds  ".to_string()),
        )
        .expect("explicit provider key should be accepted");
        assert_eq!(
            (provider_name.as_str(), provider_key.as_str()),
            ("OpenAI api.example.com", "ds")
        );

        let err = super::resolve_bootstrap_identity(
            &ProviderType::Openai,
            "https://api.example.com/v1",
            None,
            Some("  ".to_string()),
        )
        .expect_err("blank provider key should be rejected");

        assert!(super::base_error_message(&err).contains("provider_key must be provided"));
    }

    #[test]
    fn bootstrap_provider_preserves_explicit_name_and_key() {
        let (provider_name, provider_key) = super::resolve_bootstrap_identity(
            &ProviderType::Openai,
            "https://api.example.com/v1",
            Some("  DeepSeek Main  ".to_string()),
            Some("  ds  ".to_string()),
        )
        .expect("explicit provider identity should be accepted");

        assert_eq!(
            (provider_name.as_str(), provider_key.as_str()),
            ("DeepSeek Main", "ds")
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

    #[tokio::test]
    async fn provider_http_write_paths_update_response_database_and_cache() {
        let test_db_context = TestDbContext::new_sqlite("controller-provider-write-http.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let tokens = app_state
                    .admin
                    .auth
                    .bootstrap("controller provider disabled TOTP password")
                    .await
                    .expect("manager bootstrap should succeed");
                let auth_context = decode_access_token(&tokens.access_token)
                    .expect("bootstrap access should decode");

                let create_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider",
                        json!({
                            "name": "HTTP Provider",
                            "key": "http-provider",
                            "endpoint": "  HTTPS://API.EXAMPLE.COM:443/v1///  ",
                            "use_proxy": false,
                            "provider_type": "OPENAI",
                            "provider_api_key_mode": "QUEUE"
                        }),
                    ),
                )
                .await;
                assert_eq!(create_response.status(), StatusCode::OK);
                let create_body = response_json(create_response).await;
                assert_eq!(create_body["code"], 0);
                assert_eq!(create_body["data"]["provider_key"], "http-provider");
                assert_eq!(
                    create_body["data"]["endpoint"],
                    "https://api.example.com/v1"
                );

                let provider_id = create_body["data"]["id"]
                    .as_i64()
                    .expect("provider id should be returned");
                let provider = Provider::get_by_id(provider_id).expect("provider should persist");
                assert_eq!(provider.name, "HTTP Provider");
                assert_eq!(provider.endpoint, "https://api.example.com/v1");

                let provider_cached = app_state
                    .catalog
                    .get_provider_by_id(provider_id)
                    .await
                    .expect("provider cache should load")
                    .expect("provider should exist in cache");
                assert_eq!(provider_cached.provider_key, "http-provider");
                assert_eq!(provider_cached.endpoint, "https://api.example.com/v1");

                let key_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/provider_keys"),
                        json!({
                            "api_key": "sk-http-provider",
                            "description": "primary"
                        }),
                    ),
                )
                .await;
                assert_eq!(key_response.status(), StatusCode::OK);
                let key_body = response_json(key_response).await;
                assert_eq!(key_body["code"], 0);
                assert_eq!(key_body["data"]["provider_id"], provider_id);
                assert_eq!(key_body["data"]["description"], "primary");
                assert!(key_body["data"].get("api_key").is_none());
                assert!(key_body["data"].get("secret_ciphertext").is_none());

                let key_id = key_body["data"]["id"]
                    .as_i64()
                    .expect("provider key id should be returned");
                let provider_keys_cached = app_state
                    .catalog
                    .get_provider_api_keys(provider_id)
                    .await
                    .expect("provider key cache should load");
                assert_eq!(provider_keys_cached.len(), 1);
                assert_eq!(provider_keys_cached[0].id, key_id);
                cache_vertex_token_for_test(key_id, "stale-oauth-token");
                assert!(vertex_token_is_cached_for_test(key_id));

                let update_key_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/provider_keys/{key_id}/replace"),
                        json!({
                            "api_key": "sk-http-provider-updated"
                        }),
                    ),
                )
                .await;
                assert_eq!(update_key_response.status(), StatusCode::OK);
                let update_key_body = response_json(update_key_response).await;
                assert_eq!(update_key_body["data"]["description"], "primary");
                assert_eq!(update_key_body["data"]["is_enabled"], true);
                assert!(update_key_body["data"].get("api_key").is_none());
                assert!(!vertex_token_is_cached_for_test(key_id));
                let provider_keys_after_replace = app_state
                    .catalog
                    .get_provider_api_keys(provider_id)
                    .await
                    .expect("provider key cache should refresh after replace");
                assert_ne!(
                    provider_keys_after_replace[0].secret_ciphertext,
                    provider_keys_cached[0].secret_ciphertext
                );
                let replaced_plaintext = app_state
                    .secret_encryption
                    .decrypt_current(
                        SecretDomain::ProviderApiKey(key_id),
                        &provider_keys_after_replace[0]
                            .encrypted_secret()
                            .expect("refreshed encrypted secret should be valid"),
                    )
                    .expect("refreshed secret should decrypt");
                assert_eq!(replaced_plaintext.expose(), "sk-http-provider-updated");

                let mut reveal_request = empty_request(
                    Method::POST,
                    &format!("/provider/{provider_id}/provider_keys/{key_id}/reveal"),
                );
                reveal_request.extensions_mut().insert(auth_context);
                reveal_request.extensions_mut().insert(ClientIdentity {
                    client_ip: "127.0.0.1".parse().expect("test IP should parse"),
                    peer_addr: SocketAddr::from(([127, 0, 0, 1], 31_201)),
                    source: ClientIdentitySource::TcpPeer,
                    trusted_proxy_hops: 0,
                });
                let reveal_response = send(&app_state, reveal_request).await;
                assert_eq!(reveal_response.status(), StatusCode::OK);
                let reveal_body = response_json(reveal_response).await;
                assert_eq!(reveal_body["data"]["api_key"], "sk-http-provider-updated");

                let get_reveal_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        &format!("/provider/{provider_id}/provider_keys/{key_id}/reveal"),
                    ),
                )
                .await;
                assert_eq!(get_reveal_response.status(), StatusCode::METHOD_NOT_ALLOWED);

                let update_key_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/provider/{provider_id}/provider_keys/{key_id}"),
                        json!({
                            "description": "rotated",
                            "is_enabled": false
                        }),
                    ),
                )
                .await;
                assert_eq!(update_key_response.status(), StatusCode::OK);
                let update_key_body = response_json(update_key_response).await;
                assert_eq!(update_key_body["data"]["description"], "rotated");
                assert_eq!(update_key_body["data"]["is_enabled"], false);

                let updated_key = ProviderApiKeyRepository::get_summary_by_id(provider_id, key_id)
                    .expect("updated key should persist");
                assert!(!updated_key.is_enabled);

                let provider_keys_after_update = app_state
                    .catalog
                    .get_provider_api_keys(provider_id)
                    .await
                    .expect("provider key cache should reload");
                assert!(provider_keys_after_update.is_empty());

                let legacy_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        &format!("/provider/{provider_id}/provider_key/{key_id}"),
                    ),
                )
                .await;
                assert_eq!(legacy_response.status(), StatusCode::NOT_FOUND);

                let delete_key_response = send(
                    &app_state,
                    empty_request(
                        Method::DELETE,
                        &format!("/provider/{provider_id}/provider_keys/{key_id}"),
                    ),
                )
                .await;
                assert_eq!(delete_key_response.status(), StatusCode::OK);
                assert!(ProviderApiKeyRepository::get_summary_by_id(provider_id, key_id).is_err());

                let delete_response = send(
                    &app_state,
                    empty_request(Method::DELETE, &format!("/provider/{provider_id}")),
                )
                .await;
                assert_eq!(delete_response.status(), StatusCode::OK);
                let delete_body = response_json(delete_response).await;
                assert_eq!(delete_body["code"], 0);
                assert!(delete_body["data"].is_null());

                assert!(Provider::get_by_id(provider_id).is_err());
                assert!(ProviderApiKeyRepository::get_summary_by_id(provider_id, key_id).is_err());
                let provider_after_delete = app_state
                    .catalog
                    .get_provider_by_id(provider_id)
                    .await
                    .expect("provider cache should reload");
                let provider_keys_after_delete = app_state
                    .catalog
                    .get_provider_api_keys(provider_id)
                    .await
                    .expect("provider key cache should reload after provider delete");
                assert!(provider_after_delete.is_none());
                assert!(provider_keys_after_delete.is_empty());
            })
            .await;
    }

    #[tokio::test]
    async fn provider_http_write_rejects_query_endpoint_before_persistence() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-provider-invalid-endpoint-http.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider",
                        json!({
                            "name": "Invalid Provider",
                            "key": "invalid-provider",
                            "endpoint": "https://api.example.com/v1?tenant=one",
                            "use_proxy": false,
                            "provider_type": "OPENAI",
                            "provider_api_key_mode": "QUEUE"
                        }),
                    ),
                )
                .await;

                assert_eq!(response.status(), StatusCode::BAD_REQUEST);
                let body = response_json(response).await;
                assert_eq!(body["code"], 1001);
                assert_eq!(
                    body["msg"],
                    "provider endpoint must not contain a query string"
                );
                assert!(
                    Provider::list_all()
                        .expect("providers should list")
                        .is_empty()
                );
            })
            .await;
    }

    fn sample_bootstrap_result() -> super::BootstrapProviderResult {
        super::BootstrapProviderResult {
            provider: sample_provider(ProviderType::Openai, "https://api.example.com/v1"),
            created_key: ProviderApiKeySummary {
                id: 2,
                provider_id: 1,
                description: Some("bootstrap key".to_string()),
                key_prefix: "sk-t".to_string(),
                key_last4: "test".to_string(),
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

    #[tokio::test]
    async fn manager_provider_openapi_matches_routes_safe_dtos_and_errors() {
        use axum::response::IntoResponse;

        let document: serde_yaml::Value = serde_yaml::from_str(include_str!(
            "../../../docs/openapi/manager-provider.openapi.yaml"
        ))
        .expect("manager Provider OpenAPI should parse");
        assert_eq!(document["openapi"].as_str(), Some("3.1.0"));
        assert_eq!(document["info"]["version"].as_str(), Some("1.0.0-pre.3"));
        assert_eq!(
            document["x-cyder-default-cache-control"].as_str(),
            Some("no-store")
        );

        let expected_operations = [
            ("/ai/manager/api/provider/{id}/provider_keys", "get"),
            ("/ai/manager/api/provider/{id}/provider_keys", "post"),
            (
                "/ai/manager/api/provider/{id}/provider_keys/{key_id}",
                "get",
            ),
            (
                "/ai/manager/api/provider/{id}/provider_keys/{key_id}",
                "put",
            ),
            (
                "/ai/manager/api/provider/{id}/provider_keys/{key_id}",
                "delete",
            ),
            (
                "/ai/manager/api/provider/{id}/provider_keys/{key_id}/replace",
                "post",
            ),
            (
                "/ai/manager/api/provider/{id}/provider_keys/{key_id}/reveal",
                "post",
            ),
        ];
        for (path, method) in expected_operations {
            let operation = &document["paths"][path][method];
            assert!(operation.is_mapping(), "missing {method} {path}");
            assert!(operation["x-cyder-error-codes"].is_sequence());
            let success_ref = operation["responses"]["200"]["$ref"]
                .as_str()
                .expect("success response must reference an explicit DTO");
            let response_name = success_ref
                .rsplit('/')
                .next()
                .expect("response ref should contain a name");
            assert_eq!(
                document["components"]["responses"][response_name]["headers"]
                    ["Cache-Control"]["$ref"]
                    .as_str(),
                Some("#/components/headers/NoStore")
            );
        }

        let reveal = &document["paths"]["/ai/manager/api/provider/{id}/provider_keys/{key_id}/reveal"]
            ["post"];
        assert!(
            reveal["parameters"].is_null(),
            "provider reveal must use the session grant instead of a TOTP header"
        );
        let reveal_codes = reveal["x-cyder-error-codes"]
            .as_sequence()
            .expect("reveal error codes should be a sequence")
            .iter()
            .filter_map(serde_yaml::Value::as_u64)
            .collect::<Vec<_>>();
        for code in [1479, 1480, 1491] {
            assert!(reveal_codes.contains(&code), "reveal must include {code}");
        }
        for code in [1471, 1472, 1473, 1474, 1475, 1476, 1484, 1485] {
            assert!(
                !reveal_codes.contains(&code),
                "reveal must not expose per-command TOTP error {code}"
            );
        }
        for (path, method) in expected_operations.into_iter().filter(|(path, method)| {
            !(*path == "/ai/manager/api/provider/{id}/provider_keys/{key_id}/reveal"
                && *method == "post")
        }) {
            assert!(
                document["paths"][path][method]["parameters"].is_null(),
                "{method} {path} must not require sensitive TOTP"
            );
        }

        assert!(
            document["paths"]["/ai/manager/api/provider/{id}/provider_key"].is_null(),
            "legacy singular collection must not be documented"
        );
        assert!(
            document["paths"]["/ai/manager/api/provider/{id}/provider_keys/{key_id}/reveal"]["get"]
                .is_null(),
            "GET Reveal must not be documented"
        );
        assert!(
            document["paths"]
                ["/ai/manager/api/provider/{id}/provider_keys/{key_id}/reveal"]["post"]
                ["requestBody"]
                .is_null(),
            "POST Reveal must have no body"
        );

        let summary_properties =
            document["components"]["schemas"]["ProviderKeySummary"]["properties"]
                .as_mapping()
                .expect("summary properties should exist");
        for forbidden in [
            "api_key",
            "secret_ciphertext",
            "secret_nonce",
            "secret_format_version",
            "secret_key_fingerprint",
            "secret_hmac",
            "can_reveal",
        ] {
            assert!(!summary_properties.contains_key(serde_yaml::Value::from(forbidden)));
        }
        let update_properties =
            document["components"]["schemas"]["ProviderKeyMetadataUpdate"]["properties"]
                .as_mapping()
                .expect("metadata update properties should exist");
        assert_eq!(update_properties.len(), 2);
        assert!(update_properties.contains_key(serde_yaml::Value::from("description")));
        assert!(update_properties.contains_key(serde_yaml::Value::from("is_enabled")));

        let unavailable = BaseError::ProviderApiKeySecretUnavailable.into_response();
        assert_eq!(unavailable.status(), StatusCode::CONFLICT);
        let unavailable_body = response_json(unavailable).await;
        assert_eq!(unavailable_body["code"], 1005);
        let refresh_failed = BaseError::ProviderRuntimeRefreshFailed.into_response();
        assert_eq!(refresh_failed.status(), StatusCode::SERVICE_UNAVAILABLE);
        let refresh_body = response_json(refresh_failed).await;
        assert_eq!(refresh_body["code"], 1201);
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
