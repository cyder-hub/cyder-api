use crate::config::ProxyTimeoutConfig;
use crate::database::{
    DbResult,
    model::{Model, ModelDetail},
    model_source_binding::get_config as get_model_source_config,
    provider::{
        BootstrapProviderResult, Provider, ProviderAggregate, ProviderApiKeySummary,
        ProviderSummaryItem,
    },
    request_patch::RequestPatchRuleResponse,
    upstream_source::UpstreamSource,
};
use crate::proxy::runtime::transport::send_with_deadline;
use crate::proxy::{
    ProxyCancellationContext, ProxyError, ProxyErrorCode, apply_request_patches,
    load_runtime_request_patch_trace,
};
use crate::service::admin::model::{
    ModelSourceConfigSummary, ModelSourceSnapshotOwner, load_model_source_config_summaries,
};
use crate::service::admin::provider::{
    BootstrapProviderCommand, CreateProviderApiKeyInput, ProviderApiKeyReveal, ProviderUpdateInput,
    ProviderUpsertInput, ReplaceProviderApiKeyInput, SourceImpactAction, SourceImpactReport,
    UpdateProviderApiKeyInput, UpstreamSourceCreateInput, UpstreamSourceUpdateInput,
};
use crate::service::app_state::{AppState, StateRouter, create_state_router}; // Added AppState
use axum::{
    Extension,
    extract::{Json, Path, State}, // Added State
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use cyder_tools::log::info;
use reqwest::{
    StatusCode, Url,
    header::{CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc; // Added Arc

use crate::utils::{HttpResult, ID_GENERATOR, auth::ManagerAuthContext};

use super::{BaseError, auth::authorize_secret_governance_command};
use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};
use crate::service::cache::types::{CacheModel, CacheProvider, RuntimeResolvedRequestPatch};
use crate::service::provider_credential::{
    ProviderCredential, ProviderCredentialError, apply_provider_request_auth_header,
    resolve_draft_provider_credential, resolve_saved_provider_credential,
    upstream_protocol_for_profile,
};
use crate::service::provider_http::normalize_provider_endpoint;
use crate::service::secret_encryption::SensitiveSecret;
use crate::service::upstream_response::apply_upstream_accept_encoding;

#[derive(Serialize)]
struct ProviderModelDetailResponse {
    #[serde(flatten)]
    detail: ModelDetail,
    source_config: ModelSourceConfigSummary,
}

#[derive(Serialize)]
struct ProviderDetailResponse {
    provider: ProviderAggregate,
    models: Vec<ProviderModelDetailResponse>,
    provider_keys: Vec<ProviderApiKeySummary>,
    request_patches: Vec<RequestPatchRuleResponse>,
}

fn provider_model_details(provider_id: i64) -> DbResult<Vec<ProviderModelDetailResponse>> {
    let models = Model::list_by_provider_id(provider_id)?
        .into_iter()
        .map(|model| Model::get_detail_by_id(model.id))
        .collect::<Result<Vec<_>, _>>()?;
    let owners = models
        .iter()
        .map(|detail| ModelSourceSnapshotOwner {
            model_id: detail.model.id,
            provider_id,
            source_selection_mode: detail.model.source_selection_mode.clone(),
        })
        .collect::<Vec<_>>();
    let summaries = load_model_source_config_summaries(&owners)?;
    models
        .into_iter()
        .map(|detail| {
            let source_config = summaries.get(&detail.model.id).cloned().ok_or_else(|| {
                BaseError::DatabaseFatal(Some(format!(
                    "source config summary for model {} was not loaded",
                    detail.model.id
                )))
            })?;
            Ok(ProviderModelDetailResponse {
                detail,
                source_config,
            })
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpstreamSourcePayload {
    endpoint: String,
    profile_type: UpstreamProfileType,
    #[serde(default)]
    use_proxy: bool,
    #[serde(default = "default_enabled")]
    is_enabled: bool,
    #[serde(default)]
    is_default: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapProviderPayload {
    initial_source: UpstreamSourcePayload,
    api_key: String,
    model_name: String,
    name: Option<String>,
    key: Option<String>,
    real_model_name: Option<String>,
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
    provider: ProviderAggregate,
    created_key: ProviderApiKeySummary,
    created_model: Model,
    provider_name: String,
    provider_key: String,
    check_result: Option<BootstrapCheckResult>,
}

async fn list() -> DbResult<HttpResult<Vec<ProviderAggregate>>> {
    let result = Provider::list_all()?;
    Ok(HttpResult::new(result))
}

async fn list_summary() -> DbResult<HttpResult<Vec<ProviderSummaryItem>>> {
    let result = Provider::list_summary()?;
    Ok(HttpResult::new(result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InserPayload {
    pub name: String,
    pub key: String,
    pub is_enabled: Option<bool>,
    pub initial_source: Option<UpstreamSourcePayload>,
    pub provider_api_key_mode: Option<ProviderApiKeyMode>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateProviderPayload {
    pub name: String,
    pub is_enabled: Option<bool>,
    pub provider_api_key_mode: Option<ProviderApiKeyMode>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateSourcePayload {
    endpoint: Option<String>,
    use_proxy: Option<bool>,
    is_enabled: Option<bool>,
    is_default: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceImpactPayload {
    action: SourceImpactAction,
}

fn source_create_input(payload: UpstreamSourcePayload) -> UpstreamSourceCreateInput {
    UpstreamSourceCreateInput {
        endpoint: payload.endpoint,
        use_proxy: payload.use_proxy,
        profile_type: payload.profile_type,
        is_enabled: payload.is_enabled,
        is_default: payload.is_default,
    }
}

async fn insert(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<InserPayload>,
) -> DbResult<HttpResult<ProviderAggregate>> {
    let created_provider = app_state
        .admin
        .provider
        .create_provider(ProviderUpsertInput {
            name: payload.name,
            key: payload.key,
            is_enabled: payload.is_enabled,
            initial_source: payload.initial_source.map(source_create_input),
            provider_api_key_mode: payload.provider_api_key_mode,
        })
        .await?;

    Ok(HttpResult::new(created_provider))
}

async fn get_provider(Path(id): Path<i64>) -> Result<HttpResult<ProviderAggregate>, BaseError> {
    let provider = Provider::get_by_id(id)?;

    Ok(HttpResult::new(provider))
}

async fn update_provider(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateProviderPayload>,
) -> Result<HttpResult<ProviderAggregate>, BaseError> {
    let updated_provider = app_state
        .admin
        .provider
        .update_provider(
            id,
            ProviderUpdateInput {
                name: payload.name,
                is_enabled: payload.is_enabled,
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

async fn create_source(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
    Json(payload): Json<UpstreamSourcePayload>,
) -> Result<HttpResult<UpstreamSource>, BaseError> {
    let source = app_state
        .admin
        .provider
        .create_source(provider_id, source_create_input(payload))
        .await?;
    Ok(HttpResult::new(source))
}

async fn update_source(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateSourcePayload>,
) -> Result<HttpResult<UpstreamSource>, BaseError> {
    let source = app_state
        .admin
        .provider
        .update_source(
            provider_id,
            source_id,
            UpstreamSourceUpdateInput {
                endpoint: payload.endpoint,
                use_proxy: payload.use_proxy,
                is_enabled: payload.is_enabled,
                is_default: payload.is_default,
            },
        )
        .await?;
    Ok(HttpResult::new(source))
}

async fn delete_source(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id)): Path<(i64, i64)>,
) -> Result<HttpResult<()>, BaseError> {
    app_state
        .admin
        .provider
        .delete_source(provider_id, source_id)
        .await?;
    Ok(HttpResult::new(()))
}

async fn preview_source_impact(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id)): Path<(i64, i64)>,
    Json(payload): Json<SourceImpactPayload>,
) -> Result<HttpResult<SourceImpactReport>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .provider
            .preview_source_impact(provider_id, source_id, payload.action)?,
    ))
}

async fn get_provider_detail(
    State(_app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ProviderDetailResponse>, BaseError> {
    let detail = Provider::get_detail_by_id(id)?;
    let models = provider_model_details(id)?;

    Ok(HttpResult::new(ProviderDetailResponse {
        provider: detail.provider,
        models,
        provider_keys: detail.api_keys,
        request_patches: detail.request_patches,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckProviderPayload {
    model_id: Option<i64>,
    model_name: Option<String>,
    provider_api_key_id: Option<i64>,
    provider_api_key: Option<String>,
}

fn validate_saved_model_source_for_check(model: &Model, source_id: i64) -> Result<(), BaseError> {
    let source_config = get_model_source_config(model.id)?;
    match source_config.source_selection_mode.as_str() {
        "INHERIT_ALL" => Ok(()),
        "EXPLICIT" => {
            if source_config
                .bindings
                .iter()
                .any(|binding| binding.source_id == source_id)
            {
                Ok(())
            } else {
                Err(BaseError::ParamInvalid(Some(format!(
                    "Source {} is not declared by model {}",
                    source_id, model.id
                ))))
            }
        }
        mode => Err(BaseError::DatabaseFatal(Some(format!(
            "model {} has invalid source selection mode {}",
            model.id, mode
        )))),
    }
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
    provider: &ProviderAggregate,
    model: Option<&Model>,
    source: &crate::service::cache::types::CacheUpstreamSource,
) -> Result<Vec<RuntimeResolvedRequestPatch>, BaseError> {
    let cache_provider = CacheProvider::from(provider.clone());
    let cache_model = model
        .cloned()
        .map(CacheModel::from_db)
        .transpose()
        .map_err(|error| BaseError::DatabaseFatal(Some(error)))?;
    let trace = load_runtime_request_patch_trace(
        &cache_provider,
        cache_model.as_ref(),
        None,
        Some(source),
        app_state,
    )
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
    _provider: &ProviderAggregate,
    source: &UpstreamSource,
    credential: &ProviderCredential,
    model_name: &str,
    request_patches: &[RuntimeResolvedRequestPatch],
) -> Result<ProviderCheckRequest, BaseError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let cache_source = crate::service::cache::types::CacheUpstreamSource {
        id: source.id,
        profile_type: source.profile_type,
        endpoint: source.endpoint.clone(),
        use_proxy: source.use_proxy,
        is_enabled: source.is_enabled,
        is_default: source.is_default,
    };
    apply_provider_request_auth_header(
        &mut headers,
        &cache_source,
        upstream_protocol_for_profile(&cache_source.profile_type),
        credential,
    )
    .map_err(provider_credential_error)?;

    let mut request = match source.profile_type {
        UpstreamProfileType::Gemini => ProviderCheckRequest {
            url: format_gemini_generate_content_url(source, model_name),
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
        UpstreamProfileType::Vertex => ProviderCheckRequest {
            url: format_gemini_generate_content_url(source, model_name),
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
        UpstreamProfileType::VertexOpenai => ProviderCheckRequest {
            url: format_openai_check_url(source),
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
        UpstreamProfileType::Anthropic => {
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            ProviderCheckRequest {
                url: format!("{}/messages", source.endpoint.trim_end_matches('/')),
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
        UpstreamProfileType::Ollama => ProviderCheckRequest {
            url: format!("{}/api/chat", source.endpoint.trim_end_matches('/')),
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
        UpstreamProfileType::Openai
        | UpstreamProfileType::Responses
        | UpstreamProfileType::GeminiOpenai => ProviderCheckRequest {
            url: format_openai_check_url(source),
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

fn format_openai_check_url(source: &UpstreamSource) -> String {
    format!("{}/chat/completions", source.endpoint.trim_end_matches('/'))
}

fn format_gemini_generate_content_url(source: &UpstreamSource, model_name: &str) -> String {
    format!(
        "{}/{}:generateContent",
        source.endpoint.trim_end_matches('/'),
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

fn normalize_source_for_outbound(mut source: UpstreamSource) -> Result<UpstreamSource, BaseError> {
    source.endpoint = normalize_provider_endpoint(&source.endpoint).map_err(|error| {
        BaseError::ParamInvalid(Some(format!(
            "upstream source endpoint is invalid and must be repaired before use: {error}"
        )))
    })?;
    Ok(source)
}

fn profile_type_label(profile_type: &UpstreamProfileType) -> &'static str {
    match profile_type {
        UpstreamProfileType::Openai => "OpenAI",
        UpstreamProfileType::Gemini => "Gemini",
        UpstreamProfileType::Vertex => "Vertex",
        UpstreamProfileType::VertexOpenai => "Vertex OpenAI",
        UpstreamProfileType::Ollama => "Ollama",
        UpstreamProfileType::Anthropic => "Anthropic",
        UpstreamProfileType::Responses => "Responses",
        UpstreamProfileType::GeminiOpenai => "Gemini OpenAI",
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

fn generated_provider_name(profile_type: &UpstreamProfileType, endpoint: &str) -> String {
    let host = endpoint_host(endpoint);
    if host.is_empty() {
        profile_type_label(profile_type).to_string()
    } else {
        format!("{} {}", profile_type_label(profile_type), host)
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
            "provider configuration was committed, but runtime refresh failed".to_string()
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
    profile_type: &UpstreamProfileType,
    endpoint: &str,
    name: Option<String>,
    key: Option<String>,
) -> Result<(String, String), BaseError> {
    let provider_name = normalize_optional_text(name)
        .unwrap_or_else(|| generated_provider_name(profile_type, endpoint));

    let provider_key = normalize_optional_text(key).ok_or_else(|| {
        BaseError::ParamInvalid(Some("provider_key must be provided".to_string()))
    })?;

    Ok((provider_name, provider_key))
}

async fn perform_provider_check(
    client: &reqwest::Client,
    provider: &ProviderAggregate,
    source: &UpstreamSource,
    credential: &ProviderCredential,
    model_name: &str,
    request_patches: &[RuntimeResolvedRequestPatch],
    proxy_timeouts: &ProxyTimeoutConfig,
) -> Result<(), BaseError> {
    let check_request =
        build_provider_check_request(provider, source, credential, model_name, request_patches)
            .await?;

    let mut headers = check_request.headers;
    apply_upstream_accept_encoding(&mut headers, false);
    let cancellation = ProxyCancellationContext::new();
    let response = send_with_deadline(
        &cancellation,
        client
            .post(&check_request.url)
            .headers(headers)
            .json(&check_request.body),
        "Provider check request",
        proxy_timeouts,
    )
    .await
    .map_err(|error| BaseError::ParamInvalid(Some(error.public_message().to_string())))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(BaseError::ParamInvalid(Some(format!(
            "Provider API returned status {}",
            status
        ))));
    }

    drop(response);
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
    Path((id, source_id)): Path<(i64, i64)>,
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

    let provider = Provider::get_by_id(id)?;
    let source = normalize_source_for_outbound(UpstreamSource::get_active_by_id_for_provider(
        source_id, id,
    )?)?;
    if let Some(model) = selected_model.as_ref() {
        validate_saved_model_source_for_check(model, source.id)?;
    }
    let cache_source = crate::service::cache::types::CacheUpstreamSource {
        id: source.id,
        profile_type: source.profile_type,
        endpoint: source.endpoint.clone(),
        use_proxy: source.use_proxy,
        is_enabled: source.is_enabled,
        is_default: source.is_default,
    };
    let request_patches = resolve_provider_check_request_patches(
        &app_state,
        &provider,
        selected_model.as_ref(),
        &cache_source,
    )
    .await?;
    let credential = match (payload.provider_api_key_id, payload.provider_api_key) {
        (Some(key_id), _) => {
            resolve_saved_provider_credential(&provider, &cache_source, key_id, &app_state)
                .await
                .map_err(provider_credential_error)?
        }
        (_, Some(api_key)) => resolve_draft_provider_credential(
            &provider,
            &cache_source,
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
        .provider_client(source.use_proxy)
        .await
        .map_err(|error| BaseError::ParamInvalid(Some(error.to_string())))?;

    let proxy_timeouts = app_state.infra.proxy_request_config().timeouts.clone();
    perform_provider_check(
        client.as_ref(),
        &provider,
        &source,
        &credential,
        &model_name,
        &request_patches,
        &proxy_timeouts,
    )
    .await?;
    info!(
        "provider check succeeded: provider_id={}, source_id={}, profile_type={:?}",
        provider.id, source.id, source.profile_type,
    );
    Ok(HttpResult::new(json!({
        "source_id": source.id,
        "profile_type": source.profile_type,
    })))
}

async fn bootstrap_provider(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<BootstrapProviderPayload>,
) -> Result<HttpResult<BootstrapProviderResponse>, BaseError> {
    let (provider_name, provider_key) = resolve_bootstrap_identity(
        &payload.initial_source.profile_type,
        &payload.initial_source.endpoint,
        payload.name.clone(),
        payload.key.clone(),
    )?;

    let provider_input = BootstrapProviderCommand {
        provider_id: ID_GENERATOR.generate_id(),
        provider_key: provider_key.clone(),
        name: provider_name.clone(),
        endpoint: payload.initial_source.endpoint.clone(),
        use_proxy: payload.initial_source.use_proxy,
        profile_type: payload.initial_source.profile_type,
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
        let source = created
            .provider
            .upstream_sources
            .first()
            .cloned()
            .ok_or_else(|| {
                BaseError::DatabaseFatal(Some("bootstrap source missing".to_string()))
            })?;
        let cache_source = crate::service::cache::types::CacheUpstreamSource {
            id: source.id,
            profile_type: source.profile_type,
            endpoint: source.endpoint.clone(),
            use_proxy: source.use_proxy,
            is_enabled: source.is_enabled,
            is_default: source.is_default,
        };
        let client = app_state.infra.provider_client(source.use_proxy).await;
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
            &cache_source,
        )
        .await;

        let credential_and_patches = match (client, request_patches) {
            (Ok(client), Ok(request_patches)) => resolve_draft_provider_credential(
                &created.provider,
                &cache_source,
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
            Ok((client, credential, request_patches)) => {
                let proxy_timeouts = app_state.infra.proxy_request_config().timeouts.clone();
                match perform_provider_check(
                    client.as_ref(),
                    &created.provider,
                    &source,
                    &credential,
                    &model_name_to_check,
                    &request_patches,
                    &proxy_timeouts,
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
            }
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

// Removed full_commit function as Provider::full_commit is no longer available.

async fn list_provider_details(
    State(_app_state): State<Arc<AppState>>,
) -> Result<(StatusCode, HttpResult<Vec<ProviderDetailResponse>>), BaseError> {
    let providers = Provider::list_all()?;
    let mut provider_details: Vec<ProviderDetailResponse> = Vec::new();

    for provider in providers {
        let detail = Provider::get_detail_by_id(provider.id)?;
        let models = provider_model_details(provider.id)?;

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
            .route("/{id}/sources", post(create_source))
            .route(
                "/{id}/sources/{source_id}",
                put(update_source).delete(delete_source),
            )
            .route(
                "/{id}/sources/{source_id}/model-impact",
                post(preview_source_impact),
            )
            .route("/{id}/sources/{source_id}/check", post(check_provider))
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
    use std::{net::SocketAddr, sync::Arc, time::Duration};

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode, header::CONTENT_TYPE},
    };
    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
        time::{sleep, timeout},
    };
    use tower::util::ServiceExt;

    use super::create_provider_router;
    use crate::controller::BaseError;
    use crate::database::TestDbContext;
    use crate::database::model::Model;
    use crate::database::model_source_binding::{ModelSourceBindingInput, ModelSourceConfig};
    use crate::database::provider::ProviderSummaryItem;
    use crate::database::provider::{
        Provider, ProviderAggregate, ProviderApiKeyRepository, ProviderApiKeySummary,
    };
    use crate::database::upstream_source::UpstreamSource;
    use crate::ingress::client_identity::{ClientIdentity, ClientIdentitySource};
    use crate::schema::enum_def::{
        ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement, UpstreamProfileType,
    };
    use crate::service::app_state::{AppState, create_test_app_state};
    use crate::service::cache::types::{
        RequestPatchRuleOrigin, RequestPatchSource, RuntimeResolvedRequestPatch,
    };
    use crate::service::provider_credential::ProviderCredential;
    use crate::service::runtime::SourceHealthStatus;
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
        let provider = sample_provider(UpstreamProfileType::Openai, "https://api.example.com/v1");
        let request = super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
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
            UpstreamProfileType::GeminiOpenai,
            "https://generativelanguage.googleapis.com/v1beta/openai",
        );
        let request = super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
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
        let provider = sample_provider(
            UpstreamProfileType::Anthropic,
            "https://api.anthropic.com/v1",
        );
        let request = super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
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
            UpstreamProfileType::Gemini,
            "https://generativelanguage.googleapis.com/v1beta/models",
        );
        let request = super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
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
        let provider = sample_provider(UpstreamProfileType::Ollama, "http://localhost:11434");
        let request = super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
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
        let provider = sample_provider(UpstreamProfileType::Openai, "https://api.example.com/v1");
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
            &provider.upstream_sources[0],
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

    #[tokio::test]
    async fn provider_check_drops_unbounded_success_body_without_reading_it() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();
        let (closed_tx, closed_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 8192];
            let read = socket.read(&mut request).await.unwrap();
            let _ = request_tx.send(String::from_utf8_lossy(&request[..read]).to_string());
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nfirst\r\n",
                )
                .await
                .unwrap();
            let mut byte = [0u8; 1];
            let closed = socket.read(&mut byte).await.unwrap() == 0;
            let _ = closed_tx.send(closed);
        });
        let provider =
            sample_provider(UpstreamProfileType::Openai, &format!("http://{address}/v1"));

        super::perform_provider_check(
            &reqwest::Client::new(),
            &provider,
            &provider.upstream_sources[0],
            &credential("provider-header-secret"),
            "model",
            &[],
            &crate::config::ProxyTimeoutConfig::default(),
        )
        .await
        .expect("status-only provider check should succeed");
        let request = request_rx.await.unwrap().to_ascii_lowercase();
        assert!(request.contains("accept-encoding: gzip, identity"));
        assert!(
            timeout(Duration::from_secs(2), closed_rx)
                .await
                .expect("response drop should close the socket")
                .expect("close signal should be delivered")
        );
    }

    #[tokio::test]
    async fn saved_model_check_enforces_source_scope_before_any_upstream_call() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-provider-check-source-scope-http.sqlite");

        test_db_context
            .run_async(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let (request_tx, request_rx) = oneshot::channel();
                tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = vec![0u8; 8192];
                    let read = socket.read(&mut request).await.unwrap();
                    let _ = request_tx.send(String::from_utf8_lossy(&request[..read]).to_string());
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await
                        .unwrap();
                });

                let provider_id = 26001;
                let checked_source_id = 26002;
                let unbound_source_id = 26003;
                let provider = Provider::create(
                    &crate::database::provider::NewProvider {
                        id: provider_id,
                        provider_key: "check-scope-provider".to_string(),
                        name: "Check Scope Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &crate::database::upstream_source::NewUpstreamSource {
                        id: checked_source_id,
                        provider_id,
                        profile_type: UpstreamProfileType::Openai,
                        endpoint: format!("http://{address}/v1"),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                    },
                )
                .expect("provider seed should succeed")
                .provider;
                UpstreamSource::create(&crate::database::upstream_source::NewUpstreamSource {
                    id: unbound_source_id,
                    provider_id,
                    profile_type: UpstreamProfileType::Responses,
                    endpoint: "http://127.0.0.1:9/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: false,
                    created_at: 1,
                    updated_at: 1,
                })
                .expect("unbound source seed should succeed");

                let source_config = ModelSourceConfig::explicit(vec![ModelSourceBindingInput {
                    source_id: checked_source_id,
                    is_default: true,
                }]);
                let model = Model::create_with_source_config(
                    provider.id,
                    "checkable-model",
                    Some("checkable-real-model"),
                    true,
                    Some(&source_config),
                )
                .expect("model seed should succeed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let valid = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{checked_source_id}/check"),
                        json!({
                            "model_id": model.id,
                            "provider_api_key": "draft-check-secret"
                        }),
                    ),
                )
                .await;
                assert_eq!(valid.status(), StatusCode::OK);
                let valid_body = response_json(valid).await;
                assert_eq!(valid_body["data"]["source_id"], checked_source_id);
                assert!(
                    request_rx
                        .await
                        .expect("valid check should call upstream")
                        .contains("POST")
                );

                let unbound = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{unbound_source_id}/check"),
                        json!({
                            "model_id": model.id,
                            "provider_api_key": "draft-check-secret"
                        }),
                    ),
                )
                .await;
                assert_eq!(unbound.status(), StatusCode::BAD_REQUEST);
                let unbound_body = response_json(unbound).await;
                assert!(unbound_body.to_string().contains("not declared"));

                let other_provider = Provider::create(
                    &crate::database::provider::NewProvider {
                        id: 26004,
                        provider_key: "other-check-provider".to_string(),
                        name: "Other Check Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &crate::database::upstream_source::NewUpstreamSource {
                        id: 26005,
                        provider_id: 26004,
                        profile_type: UpstreamProfileType::Openai,
                        endpoint: "http://127.0.0.1:9/v1".to_string(),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                    },
                )
                .expect("other provider seed should succeed")
                .provider;
                let other_model = Model::create(other_provider.id, "other-model", None, true)
                    .expect("other model should seed");
                let wrong_owner = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{checked_source_id}/check"),
                        json!({
                            "model_id": other_model.id,
                            "provider_api_key": "draft-check-secret"
                        }),
                    ),
                )
                .await;
                assert_eq!(wrong_owner.status(), StatusCode::BAD_REQUEST);

                UpstreamSource::delete(checked_source_id, provider_id)
                    .expect("checked source should soft delete");
                let deleted = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{checked_source_id}/check"),
                        json!({
                            "model_id": model.id,
                            "provider_api_key": "draft-check-secret"
                        }),
                    ),
                )
                .await;
                assert_eq!(deleted.status(), StatusCode::NOT_FOUND);
            })
            .await;
    }

    #[tokio::test]
    async fn provider_check_header_stall_uses_proxy_policy_without_circuit_attribution() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 8192];
            let read = socket.read(&mut request).await.unwrap();
            let _ = request_tx.send(String::from_utf8_lossy(&request[..read]).to_string());
            sleep(Duration::from_secs(2)).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
        });
        let provider =
            sample_provider(UpstreamProfileType::Openai, &format!("http://{address}/v1"));
        let mut proxy_timeouts = crate::config::ProxyTimeoutConfig::default();
        proxy_timeouts.request_send_seconds = 1;
        proxy_timeouts.total_seconds = 2;

        let error = timeout(
            Duration::from_secs(3),
            super::perform_provider_check(
                &reqwest::Client::new(),
                &provider,
                &provider.upstream_sources[0],
                &credential("provider-check-secret"),
                "model",
                &[],
                &proxy_timeouts,
            ),
        )
        .await
        .expect("header stall should be bounded")
        .expect_err("header stall should fail the provider check");

        assert!(matches!(error, BaseError::ParamInvalid(_)));
        let message = super::base_error_message(&error);
        assert!(!message.contains("provider-check-secret"));
        assert!(request_rx.await.unwrap().contains("POST"));
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
            super::generated_provider_name(
                &UpstreamProfileType::Openai,
                "https://api.example.com/v1"
            ),
            "OpenAI api.example.com"
        );

        let (provider_name, provider_key) = super::resolve_bootstrap_identity(
            &UpstreamProfileType::Openai,
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
            &UpstreamProfileType::Openai,
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
            &UpstreamProfileType::Openai,
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
            source_count: 1,
            enabled_source_count: 1,
            default_source_id: Some(43),
            default_source_profile_type: Some(UpstreamProfileType::Openai),
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
                "source_count".to_string(),
                "enabled_source_count".to_string(),
                "default_source_id".to_string(),
                "default_source_profile_type".to_string(),
            ])
        );
        assert_eq!(item["source_count"], 1);
        assert_eq!(item["default_source_profile_type"], "OPENAI");
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

                let legacy_flat_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider",
                        json!({
                            "name": "Legacy Flat Provider",
                            "key": "legacy-flat-provider",
                            "endpoint": "https://api.example.com/v1",
                            "use_proxy": false,
                            "provider_type": "OPENAI",
                            "provider_api_key_mode": "QUEUE"
                        }),
                    ),
                )
                .await;
                assert_eq!(
                    legacy_flat_response.status(),
                    StatusCode::UNPROCESSABLE_ENTITY
                );
                assert!(
                    Provider::list_all()
                        .expect("providers should list")
                        .is_empty()
                );

                let create_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider",
                        json!({
                            "name": "HTTP Provider",
                            "key": "http-provider",
                            "initial_source": {
                                "endpoint": "  HTTPS://API.EXAMPLE.COM:443/v1///  ",
                                "use_proxy": false,
                                "profile_type": "OPENAI"
                            },
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
                    create_body["data"]["upstream_sources"][0]["endpoint"],
                    "https://api.example.com/v1"
                );
                assert!(create_body["data"].get("endpoint").is_none());
                assert!(create_body["data"].get("profile_type").is_none());

                let provider_id = create_body["data"]["id"]
                    .as_i64()
                    .expect("provider id should be returned");
                let provider = Provider::get_by_id(provider_id).expect("provider should persist");
                assert_eq!(provider.name, "HTTP Provider");
                assert_eq!(
                    provider.upstream_sources[0].endpoint,
                    "https://api.example.com/v1"
                );

                let provider_cached = app_state
                    .catalog
                    .get_provider_by_id(provider_id)
                    .await
                    .expect("provider cache should load")
                    .expect("provider should exist in cache");
                assert_eq!(provider_cached.provider_key, "http-provider");
                assert_eq!(
                    provider_cached.upstream_sources[0].endpoint,
                    "https://api.example.com/v1"
                );

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
                            "initial_source": {
                                "endpoint": "https://api.example.com/v1?tenant=one",
                                "use_proxy": false,
                                "profile_type": "OPENAI"
                            },
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
                    "upstream source endpoint must not contain a query string"
                );
                assert!(
                    Provider::list_all()
                        .expect("providers should list")
                        .is_empty()
                );
            })
            .await;
    }

    #[tokio::test]
    async fn provider_http_source_routes_are_explicit_and_ownership_scoped() {
        let test_db_context = TestDbContext::new_sqlite("controller-provider-source-http.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let create_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider",
                        json!({
                            "name": "Source HTTP Provider",
                            "key": "source-http-provider",
                            "provider_api_key_mode": "QUEUE"
                        }),
                    ),
                )
                .await;
                assert_eq!(create_response.status(), StatusCode::OK);
                let create_body = response_json(create_response).await;
                assert_eq!(create_body["data"]["upstream_sources"], json!([]));
                let provider_id = create_body["data"]["id"]
                    .as_i64()
                    .expect("provider id should be returned");

                let source_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources"),
                        json!({
                            "profile_type": "OPENAI",
                            "endpoint": "https://api.example.com/v1",
                            "use_proxy": false,
                            "is_enabled": true,
                            "is_default": true
                        }),
                    ),
                )
                .await;
                assert_eq!(source_response.status(), StatusCode::OK);
                let source_body = response_json(source_response).await;
                assert_eq!(source_body["data"]["profile_type"], "OPENAI");
                let source_id = source_body["data"]["id"]
                    .as_i64()
                    .expect("source id should be returned");

                for _ in 0..5 {
                    app_state
                        .source_circuit
                        .record_source_failure(source_id, "stale source failure".to_string(), None)
                        .await
                        .expect("source failure should record");
                }
                assert_eq!(
                    app_state
                        .source_circuit
                        .get_source_health_snapshot(source_id)
                        .await
                        .expect("source snapshot should load")
                        .status,
                    SourceHealthStatus::Open
                );

                let default_only_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/provider/{provider_id}/sources/{source_id}"),
                        json!({
                            "endpoint": "https://api.example.com/v1",
                            "use_proxy": false,
                            "is_enabled": true,
                            "is_default": true
                        }),
                    ),
                )
                .await;
                assert_eq!(default_only_response.status(), StatusCode::OK);
                assert_eq!(
                    app_state
                        .source_circuit
                        .get_source_health_snapshot(source_id)
                        .await
                        .expect("source snapshot should load")
                        .status,
                    SourceHealthStatus::Open,
                    "default-only changes must not clear source circuit state"
                );

                let update_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/provider/{provider_id}/sources/{source_id}"),
                        json!({
                            "endpoint": "https://api.example.com/v1/updated",
                            "use_proxy": true,
                            "is_enabled": true,
                            "is_default": true
                        }),
                    ),
                )
                .await;
                assert_eq!(update_response.status(), StatusCode::OK);
                let update_body = response_json(update_response).await;
                assert_eq!(
                    update_body["data"]["endpoint"],
                    "https://api.example.com/v1/updated"
                );
                assert_eq!(update_body["data"]["is_default"], true);
                assert_eq!(
                    app_state
                        .source_circuit
                        .get_source_health_snapshot(source_id)
                        .await
                        .expect("source snapshot should load")
                        .status,
                    SourceHealthStatus::Healthy,
                    "endpoint/proxy changes must clear source circuit state"
                );

                for _ in 0..5 {
                    app_state
                        .source_circuit
                        .record_source_failure(
                            source_id,
                            "disabled source failure".to_string(),
                            None,
                        )
                        .await
                        .expect("source failure should record");
                }
                let disable_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/provider/{provider_id}/sources/{source_id}"),
                        json!({
                            "use_proxy": true,
                            "is_enabled": false,
                            "is_default": false
                        }),
                    ),
                )
                .await;
                assert_eq!(disable_response.status(), StatusCode::OK);
                assert_eq!(
                    app_state
                        .source_circuit
                        .get_source_health_snapshot(source_id)
                        .await
                        .expect("source snapshot should load")
                        .status,
                    SourceHealthStatus::Healthy,
                    "enabled-to-disabled changes must clear source circuit state"
                );

                for _ in 0..5 {
                    app_state
                        .source_circuit
                        .record_source_failure(
                            source_id,
                            "re-enabled source failure".to_string(),
                            None,
                        )
                        .await
                        .expect("source failure should record");
                }
                let enable_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/provider/{provider_id}/sources/{source_id}"),
                        json!({
                            "use_proxy": true,
                            "is_enabled": true,
                            "is_default": true
                        }),
                    ),
                )
                .await;
                assert_eq!(enable_response.status(), StatusCode::OK);
                assert_eq!(
                    app_state
                        .source_circuit
                        .get_source_health_snapshot(source_id)
                        .await
                        .expect("source snapshot should load")
                        .status,
                    SourceHealthStatus::Healthy,
                    "disabled-to-enabled changes must clear source circuit state"
                );

                let second_provider_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider",
                        json!({
                            "name": "Second Source HTTP Provider",
                            "key": "second-source-http-provider"
                        }),
                    ),
                )
                .await;
                assert_eq!(second_provider_response.status(), StatusCode::OK);
                let second_provider_id =
                    response_json(second_provider_response).await["data"]["id"]
                        .as_i64()
                        .expect("second provider id should be returned");

                let wrong_owner_response = send(
                    &app_state,
                    empty_request(
                        Method::DELETE,
                        &format!("/provider/{second_provider_id}/sources/{source_id}"),
                    ),
                )
                .await;
                assert_eq!(wrong_owner_response.status(), StatusCode::NOT_FOUND);

                let old_check_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/check"),
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(old_check_response.status(), StatusCode::NOT_FOUND);
                let old_discovery_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        &format!("/provider/{provider_id}/remote_models"),
                    ),
                )
                .await;
                assert_eq!(old_discovery_response.status(), StatusCode::NOT_FOUND);
                let removed_source_discovery_response = send(
                    &app_state,
                    empty_request(
                        Method::GET,
                        &format!("/provider/{provider_id}/sources/{source_id}/remote_models"),
                    ),
                )
                .await;
                assert_eq!(
                    removed_source_discovery_response.status(),
                    StatusCode::NOT_FOUND
                );

                let delete_response = send(
                    &app_state,
                    empty_request(
                        Method::DELETE,
                        &format!("/provider/{provider_id}/sources/{source_id}"),
                    ),
                )
                .await;
                assert_eq!(delete_response.status(), StatusCode::OK);
                assert!(response_json(delete_response).await["data"].is_null());
                assert_eq!(
                    app_state
                        .source_circuit
                        .get_source_health_snapshot(source_id)
                        .await
                        .expect("deleted source snapshot should load")
                        .status,
                    SourceHealthStatus::Healthy,
                    "deleting a source must clear source circuit state"
                );
                let provider_after_delete = Provider::get_by_id(provider_id)
                    .expect("provider should remain after deleting its final source");
                assert!(provider_after_delete.upstream_sources.is_empty());

                let bootstrap_without_source = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider/bootstrap",
                        json!({
                            "api_key": "bootstrap-secret",
                            "model_name": "bootstrap-model"
                        }),
                    ),
                )
                .await;
                assert_eq!(
                    bootstrap_without_source.status(),
                    StatusCode::UNPROCESSABLE_ENTITY
                );
            })
            .await;
    }

    fn sample_bootstrap_result() -> super::BootstrapProviderResult {
        super::BootstrapProviderResult {
            provider: sample_provider(UpstreamProfileType::Openai, "https://api.example.com/v1"),
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
                source_selection_mode: "INHERIT_ALL".to_string(),
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
        assert_eq!(document["info"]["version"].as_str(), Some("1.0.0-pre.5"));
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

        let provider_operations = [
            ("/ai/manager/api/provider", "post"),
            ("/ai/manager/api/provider/list", "get"),
            ("/ai/manager/api/provider/summary/list", "get"),
            ("/ai/manager/api/provider/bootstrap", "post"),
            ("/ai/manager/api/provider/{id}", "get"),
            ("/ai/manager/api/provider/{id}", "put"),
            ("/ai/manager/api/provider/{id}", "delete"),
            ("/ai/manager/api/provider/{id}/detail", "get"),
            ("/ai/manager/api/provider/detail/list", "get"),
            ("/ai/manager/api/provider/{id}/sources", "post"),
            ("/ai/manager/api/provider/{id}/sources/{source_id}", "put"),
            (
                "/ai/manager/api/provider/{id}/sources/{source_id}",
                "delete",
            ),
            (
                "/ai/manager/api/provider/{id}/sources/{source_id}/model-impact",
                "post",
            ),
            (
                "/ai/manager/api/provider/{id}/sources/{source_id}/check",
                "post",
            ),
        ];
        for (path, method) in provider_operations {
            let operation = &document["paths"][path][method];
            assert!(operation.is_mapping(), "missing {method} {path}");
            let success_ref = operation["responses"]["200"]["$ref"]
                .as_str()
                .expect("Provider success response must reference an explicit DTO");
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

        let model_operations = [
            ("/ai/manager/api/model", "post"),
            ("/ai/manager/api/model/list", "get"),
            ("/ai/manager/api/model/summary/list", "get"),
            ("/ai/manager/api/model/{id}", "put"),
            ("/ai/manager/api/model/{id}", "delete"),
            ("/ai/manager/api/model/{id}/detail", "get"),
            ("/ai/manager/api/model/{id}/source-config", "get"),
            ("/ai/manager/api/model/{id}/source-config", "put"),
            ("/ai/manager/api/model/{id}/source-config/explain", "get"),
        ];
        for (path, method) in model_operations {
            let operation = &document["paths"][path][method];
            assert!(operation.is_mapping(), "missing {method} {path}");
            let success_ref = operation["responses"]["200"]["$ref"]
                .as_str()
                .expect("Model success response must reference an explicit DTO");
            let response_name = success_ref
                .rsplit('/')
                .next()
                .expect("Model response ref should contain a name");
            assert_eq!(
                document["components"]["responses"][response_name]["headers"]
                    ["Cache-Control"]["$ref"]
                    .as_str(),
                Some("#/components/headers/NoStore")
            );
        }

        let aggregate_properties =
            document["components"]["schemas"]["ProviderAggregate"]["properties"]
                .as_mapping()
                .expect("Provider aggregate properties should exist");
        assert!(aggregate_properties.contains_key(serde_yaml::Value::from("upstream_sources")));
        assert!(!aggregate_properties.contains_key(serde_yaml::Value::from("upstream_source")));
        for forbidden in ["endpoint", "provider_type", "profile_type", "use_proxy"] {
            assert!(!aggregate_properties.contains_key(serde_yaml::Value::from(forbidden)));
        }
        let upsert = &document["components"]["schemas"]["ProviderUpsert"];
        assert_eq!(upsert["additionalProperties"].as_bool(), Some(false));
        let upsert_properties = upsert["properties"]
            .as_mapping()
            .expect("Provider upsert properties should exist");
        assert!(upsert_properties.contains_key(serde_yaml::Value::from("initial_source")));
        for forbidden in ["endpoint", "provider_type", "profile_type", "use_proxy"] {
            assert!(!upsert_properties.contains_key(serde_yaml::Value::from(forbidden)));
        }
        assert!(
            document["components"]["schemas"]["UpstreamSource"]["properties"]["source_key"]
                .is_null()
        );
        assert!(document["components"]["schemas"]["ProviderAggregate"]["properties"]
            ["upstream_sources"]
            .is_mapping());
        let model_properties = document["components"]["schemas"]["Model"]["properties"]
            .as_mapping()
            .expect("Model properties should exist");
        assert!(model_properties.contains_key(serde_yaml::Value::from("source_selection_mode")));
        for forbidden in [
            "supports_streaming",
            "supports_tools",
            "supports_reasoning",
            "supports_image_input",
            "supports_embeddings",
            "supports_rerank",
        ] {
            assert!(
                !model_properties.contains_key(serde_yaml::Value::from(forbidden)),
                "Model OpenAPI must not expose {forbidden}"
            );
        }
        let source_config_properties =
            document["components"]["schemas"]["ModelSourceConfig"]["properties"]
                .as_mapping()
                .expect("Model Source Config properties should exist");
        assert!(
            source_config_properties.contains_key(serde_yaml::Value::from("source_selection_mode"))
        );
        assert!(source_config_properties.contains_key(serde_yaml::Value::from("bindings")));
        assert_eq!(
            document["components"]["schemas"]["ModelSourceSelectionMode"]["enum"]
                .as_sequence()
                .expect("Source selection mode enum should exist")
                .len(),
            2
        );
        let source_impact_actions = document["components"]["schemas"]["SourceImpactAction"]["enum"]
            .as_sequence()
            .expect("Source impact actions should exist");
        for action in ["DISABLE", "DELETE", "SET_DEFAULT", "UNSET_DEFAULT"] {
            assert!(source_impact_actions.contains(&serde_yaml::Value::from(action)));
        }
        assert!(document["components"]["schemas"]["ModelSourceProtocolExplain"]
            ["properties"]["selection_reason"]
            .is_mapping());
        assert!(document["components"]["schemas"]["ModelSourceProtocolExplain"]
            ["properties"]["decision_trace"]
            .is_mapping());
        for field in ["is_enabled", "is_default"] {
            assert!(
                document["components"]["schemas"]["UpstreamSource"]["properties"][field]
                    .is_mapping()
            );
        }
        for schema in ["SourceEvidence"] {
            let properties = document["components"]["schemas"][schema]["properties"]
                .as_mapping()
                .expect("Source evidence properties should exist");
            for field in ["source_id", "profile_type"] {
                assert!(properties.contains_key(serde_yaml::Value::from(field)));
            }
            assert!(!properties.contains_key(serde_yaml::Value::from("source_key")));
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
            document["paths"]["/ai/manager/api/provider/{id}/check"].is_null(),
            "legacy provider check route must not be documented"
        );
        assert!(
            document["paths"]["/ai/manager/api/provider/{id}/remote_models"].is_null(),
            "legacy provider discovery route must not be documented"
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

    #[tokio::test]
    async fn source_impact_http_preview_covers_all_actions_without_side_effects() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-provider-source-impact-http.sqlite");

        test_db_context
            .run_async(async {
                let provider_id = 24301;
                let default_source_id = 24302;
                let responses_source_id = 24303;
                let provider = Provider::create(
                    &crate::database::provider::NewProvider {
                        id: provider_id,
                        provider_key: "source-impact-provider".to_string(),
                        name: "Source Impact Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &crate::database::upstream_source::NewUpstreamSource {
                        id: default_source_id,
                        provider_id,
                        profile_type: UpstreamProfileType::Openai,
                        endpoint: "https://impact-openai.example.com/v1".to_string(),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                    },
                )
                .expect("provider seed should succeed")
                .provider;
                UpstreamSource::create(&crate::database::upstream_source::NewUpstreamSource {
                    id: responses_source_id,
                    provider_id,
                    profile_type: UpstreamProfileType::Responses,
                    endpoint: "https://impact-responses.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: false,
                    created_at: 1,
                    updated_at: 1,
                })
                .expect("secondary source seed should succeed");

                let inherit_model = Model::create(provider.id, "impact-inherit", None, true)
                    .expect("inherit model should seed");
                let disabled_inherit_model =
                    Model::create(provider.id, "impact-disabled-inherit", None, false)
                        .expect("disabled inherit model should seed");
                let explicit_default_model = Model::create_with_source_config(
                    provider.id,
                    "impact-explicit-default",
                    None,
                    true,
                    Some(&ModelSourceConfig::explicit(vec![
                        ModelSourceBindingInput {
                            source_id: default_source_id,
                            is_default: true,
                        },
                    ])),
                )
                .expect("explicit default model should seed");
                let explicit_other_model = Model::create_with_source_config(
                    provider.id,
                    "impact-explicit-other",
                    None,
                    true,
                    Some(&ModelSourceConfig::explicit(vec![
                        ModelSourceBindingInput {
                            source_id: responses_source_id,
                            is_default: true,
                        },
                    ])),
                )
                .expect("explicit other model should seed");
                let explicit_empty_model = Model::create_with_source_config(
                    provider.id,
                    "impact-explicit-empty",
                    None,
                    true,
                    Some(&ModelSourceConfig::explicit(Vec::new())),
                )
                .expect("explicit empty model should seed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let mut reports = Vec::new();
                for action in ["DISABLE", "DELETE", "SET_DEFAULT", "UNSET_DEFAULT"] {
                    let response = send(
                        &app_state,
                        json_request(
                            Method::POST,
                            &format!(
                                "/provider/{provider_id}/sources/{default_source_id}/model-impact"
                            ),
                            json!({"action": action}),
                        ),
                    )
                    .await;
                    assert_eq!(response.status(), StatusCode::OK, "action {action}");
                    let body = response_json(response).await;
                    assert_eq!(body["code"], 0, "action {action}");
                    assert_eq!(body["data"]["action"], action, "action {action}");
                    assert_eq!(body["data"]["provider_id"], provider_id);
                    assert_eq!(body["data"]["source_id"], default_source_id);
                    assert_eq!(body["data"]["inherit_all_model_count"], 2);
                    assert_eq!(body["data"]["explicit_binding_model_count"], 3);
                    assert_eq!(body["data"]["explicit_default_model_count"], 2);
                    assert_eq!(body["data"].get("model_ids"), None);
                    assert_eq!(body["data"]["protocols"].as_array().unwrap().len(), 4);
                    reports.push((action, body));
                }

                let disable_openai = reports
                    .iter()
                    .find(|(action, _)| *action == "DISABLE")
                    .map(|(_, body)| body)
                    .unwrap();
                let openai_disable = disable_openai["data"]["protocols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|protocol| protocol["downstream_protocol"] == "OPENAI")
                    .unwrap();
                assert_eq!(openai_disable["selection_changed_count"], 3);
                assert_eq!(openai_disable["would_become_unselectable_count"], 3);

                let set_default = reports
                    .iter()
                    .find(|(action, _)| *action == "SET_DEFAULT")
                    .map(|(_, body)| body)
                    .unwrap();
                assert!(
                    set_default["data"]["protocols"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|protocol| protocol["selection_changed_count"] == 0
                            && protocol["would_become_unselectable_count"] == 0)
                );

                let unset_default = reports
                    .iter()
                    .find(|(action, _)| *action == "UNSET_DEFAULT")
                    .map(|(_, body)| body)
                    .unwrap();
                let anthropic_unset = unset_default["data"]["protocols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|protocol| protocol["downstream_protocol"] == "ANTHROPIC")
                    .unwrap();
                assert_eq!(anthropic_unset["selection_changed_count"], 2);
                assert_eq!(anthropic_unset["would_become_unselectable_count"], 2);

                let delete_openai = reports
                    .iter()
                    .find(|(action, _)| *action == "DELETE")
                    .map(|(_, body)| body)
                    .unwrap();
                let openai_delete = delete_openai["data"]["protocols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|protocol| protocol["downstream_protocol"] == "OPENAI")
                    .unwrap();
                assert_eq!(openai_delete["selection_changed_count"], 3);
                assert_eq!(openai_delete["would_become_unselectable_count"], 3);

                let unchanged_provider = Provider::get_by_id(provider_id)
                    .expect("provider should remain readable after previews");
                let unchanged_source = unchanged_provider
                    .upstream_sources
                    .iter()
                    .find(|source| source.id == default_source_id)
                    .expect("target source should remain visible");
                assert!(unchanged_source.is_enabled);
                assert!(unchanged_source.is_default);
                assert!(Model::get_by_id(inherit_model.id).is_ok());
                assert!(Model::get_by_id(disabled_inherit_model.id).is_ok());
                assert!(Model::get_by_id(explicit_default_model.id).is_ok());
                assert!(Model::get_by_id(explicit_other_model.id).is_ok());
                assert!(Model::get_by_id(explicit_empty_model.id).is_ok());
                assert_eq!(
                    crate::database::model_source_binding::get_config(explicit_default_model.id)
                        .expect("binding should remain readable")
                        .bindings
                        .len(),
                    1
                );
            })
            .await;
    }

    fn sample_provider(profile_type: UpstreamProfileType, endpoint: &str) -> ProviderAggregate {
        ProviderAggregate {
            provider: Provider {
                id: 1,
                provider_key: "provider".to_string(),
                name: "provider".to_string(),
                is_enabled: true,
                deleted_at: None,
                created_at: 0,
                updated_at: 0,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
            },
            upstream_sources: vec![UpstreamSource {
                id: 2,
                provider_id: 1,
                profile_type,
                endpoint: endpoint.to_string(),
                use_proxy: false,
                is_enabled: true,
                is_default: true,
                deleted_at: None,
                created_at: 0,
                updated_at: 0,
            }],
        }
    }
}
