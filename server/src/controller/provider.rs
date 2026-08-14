use crate::config::{NonStreamResponseConfig, ProxyTimeoutConfig};
use crate::database::{
    DbResult,
    model::{Model, ModelDetail},
    model_source_binding::get_config as get_model_source_config,
    provider::{
        BootstrapProviderResult, Provider, ProviderAggregate, ProviderApiKeySummary,
        ProviderSummaryItem,
    },
    request_patch::RequestPatchVariantAggregate,
    upstream_source::UpstreamSource,
};
use crate::proxy::runtime::{
    request_patch::resolve_runtime_request_patch_trace, transport::send_with_deadline,
};
use crate::proxy::{ProxyCancellationContext, ProxyError, ProxyErrorCode, apply_request_patches};
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
use crate::schema::enum_def::{
    ModelKind, ProviderApiKeyMode, UpstreamProfileType, UpstreamProtocol,
};
use crate::service::cache::types::{CacheModel, CacheProvider, RuntimeResolvedRequestPatch};
use crate::service::provider_credential::{
    ProviderCredential, ProviderCredentialError, apply_provider_request_auth_header,
    resolve_draft_provider_credential, resolve_saved_provider_credential,
    upstream_protocol_for_profile,
};
use crate::service::provider_http::{
    GeminiModelOperation, anthropic_messages_url, base_url_is_default,
    enforce_anthropic_version_header, gemini_operation_target_url, gemini_source_check_body,
    join_base_url_and_operation_path, normalize_provider_base_url, normalize_source_base_url,
    sanitize_gemini_request_headers, validate_gemini_pre_auth_target,
    validate_gemini_source_check_response,
};
use crate::service::secret_encryption::SensitiveSecret;
use crate::service::transform::{finalize_request_data, validate_final_generation_request};
use crate::service::upstream_profile::{UpstreamOperation, resolve_source_operation_url};
use crate::service::upstream_response::{
    ResponseBodyReadTimeouts, apply_upstream_accept_encoding, normalize_content_type,
    read_complete_response_body_with_timeouts,
};

#[derive(Serialize)]
struct ProviderModelDetailResponse {
    #[serde(flatten)]
    detail: ModelDetail,
    source_config: ModelSourceConfigSummary,
}

#[derive(Serialize)]
struct SourceCommonResponse {
    id: i64,
    provider_id: i64,
    base_url: String,
    base_url_is_default: bool,
    use_proxy: bool,
    is_enabled: bool,
    is_default: bool,
    deleted_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Serialize)]
#[serde(tag = "profile_type")]
enum UpstreamSourceResponse {
    #[serde(rename = "OPENAI")]
    Openai {
        #[serde(flatten)]
        common: SourceCommonResponse,
        chat_completions_enabled: bool,
        chat_completions_path_override: Option<String>,
        embeddings_enabled: bool,
        embeddings_path_override: Option<String>,
    },
    #[serde(rename = "OPENAI_COMPATIBLE")]
    OpenaiCompatible {
        #[serde(flatten)]
        common: SourceCommonResponse,
        chat_completions_enabled: bool,
        chat_completions_path_override: Option<String>,
        embeddings_enabled: bool,
        embeddings_path_override: Option<String>,
        rerank_enabled: bool,
        rerank_path_override: Option<String>,
    },
    #[serde(rename = "GEMINI_OPENAI")]
    GeminiOpenai {
        #[serde(flatten)]
        common: SourceCommonResponse,
        chat_completions_enabled: bool,
        chat_completions_path_override: Option<String>,
        embeddings_enabled: bool,
        embeddings_path_override: Option<String>,
    },
    #[serde(rename = "GEMINI")]
    Gemini {
        #[serde(flatten)]
        common: SourceCommonResponse,
    },
    #[serde(rename = "VERTEX")]
    Vertex {
        #[serde(flatten)]
        common: SourceCommonResponse,
    },
    #[serde(rename = "ANTHROPIC")]
    Anthropic {
        #[serde(flatten)]
        common: SourceCommonResponse,
    },
    #[serde(rename = "RESPONSES")]
    Responses {
        #[serde(flatten)]
        common: SourceCommonResponse,
    },
}

impl From<UpstreamSource> for UpstreamSourceResponse {
    fn from(source: UpstreamSource) -> Self {
        let common = SourceCommonResponse {
            id: source.id,
            provider_id: source.provider_id,
            base_url: source.base_url.clone(),
            base_url_is_default: base_url_is_default(&source.profile_type, &source.base_url),
            use_proxy: source.use_proxy,
            is_enabled: source.is_enabled,
            is_default: source.is_default,
            deleted_at: source.deleted_at,
            created_at: source.created_at,
            updated_at: source.updated_at,
        };
        match source.profile_type {
            UpstreamProfileType::Openai => Self::Openai {
                common,
                chat_completions_enabled: source.chat_completions_enabled.unwrap_or(false),
                chat_completions_path_override: source.chat_completions_path_override,
                embeddings_enabled: source.embeddings_enabled.unwrap_or(false),
                embeddings_path_override: source.embeddings_path_override,
            },
            UpstreamProfileType::OpenaiCompatible => Self::OpenaiCompatible {
                common,
                chat_completions_enabled: source.chat_completions_enabled.unwrap_or(false),
                chat_completions_path_override: source.chat_completions_path_override,
                embeddings_enabled: source.embeddings_enabled.unwrap_or(false),
                embeddings_path_override: source.embeddings_path_override,
                rerank_enabled: source.rerank_enabled.unwrap_or(false),
                rerank_path_override: source.rerank_path_override,
            },
            UpstreamProfileType::GeminiOpenai => Self::GeminiOpenai {
                common,
                chat_completions_enabled: source.chat_completions_enabled.unwrap_or(false),
                chat_completions_path_override: source.chat_completions_path_override,
                embeddings_enabled: source.embeddings_enabled.unwrap_or(false),
                embeddings_path_override: source.embeddings_path_override,
            },
            UpstreamProfileType::Gemini => Self::Gemini { common },
            UpstreamProfileType::Vertex => Self::Vertex { common },
            UpstreamProfileType::Anthropic => Self::Anthropic { common },
            UpstreamProfileType::Responses => Self::Responses { common },
        }
    }
}

#[derive(Serialize)]
struct ProviderAggregateResponse {
    #[serde(flatten)]
    provider: crate::database::provider::Provider,
    upstream_sources: Vec<UpstreamSourceResponse>,
}

impl From<ProviderAggregate> for ProviderAggregateResponse {
    fn from(aggregate: ProviderAggregate) -> Self {
        Self {
            provider: aggregate.provider,
            upstream_sources: aggregate
                .upstream_sources
                .into_iter()
                .map(UpstreamSourceResponse::from)
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct ProviderDetailResponse {
    provider: ProviderAggregateResponse,
    models: Vec<ProviderModelDetailResponse>,
    provider_keys: Vec<ProviderApiKeySummary>,
    request_patch_variants: Vec<RequestPatchVariantAggregate>,
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
            model_kind: detail.model.model_kind,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceLifecyclePayload {
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenAiSourcePayload {
    base_url: Option<String>,
    #[serde(flatten)]
    lifecycle: SourceLifecyclePayload,
    chat_completions_enabled: Option<bool>,
    chat_completions_path_override: Option<String>,
    embeddings_enabled: Option<bool>,
    embeddings_path_override: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenAiCompatibleSourcePayload {
    base_url: String,
    #[serde(flatten)]
    lifecycle: SourceLifecyclePayload,
    chat_completions_enabled: Option<bool>,
    chat_completions_path_override: Option<String>,
    embeddings_enabled: Option<bool>,
    embeddings_path_override: Option<String>,
    rerank_enabled: Option<bool>,
    rerank_path_override: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeSourcePayload {
    base_url: String,
    #[serde(flatten)]
    lifecycle: SourceLifecyclePayload,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "profile_type")]
enum UpstreamSourcePayload {
    #[serde(rename = "OPENAI")]
    Openai(OpenAiSourcePayload),
    #[serde(rename = "OPENAI_COMPATIBLE")]
    OpenaiCompatible(OpenAiCompatibleSourcePayload),
    #[serde(rename = "GEMINI_OPENAI")]
    GeminiOpenai(OpenAiSourcePayload),
    #[serde(rename = "GEMINI")]
    Gemini(NativeSourcePayload),
    #[serde(rename = "VERTEX")]
    Vertex(NativeSourcePayload),
    #[serde(rename = "ANTHROPIC")]
    Anthropic(NativeSourcePayload),
    #[serde(rename = "RESPONSES")]
    Responses(NativeSourcePayload),
}

impl UpstreamSourcePayload {
    fn into_input(self) -> UpstreamSourceCreateInput {
        match self {
            Self::Openai(payload) => openai_source_input(UpstreamProfileType::Openai, payload),
            Self::GeminiOpenai(payload) => {
                openai_source_input(UpstreamProfileType::GeminiOpenai, payload)
            }
            Self::OpenaiCompatible(payload) => UpstreamSourceCreateInput {
                profile_type: UpstreamProfileType::OpenaiCompatible,
                base_url: Some(payload.base_url),
                use_proxy: payload.lifecycle.use_proxy,
                chat_completions_enabled: payload.chat_completions_enabled,
                chat_completions_path_override: payload.chat_completions_path_override,
                embeddings_enabled: payload.embeddings_enabled,
                embeddings_path_override: payload.embeddings_path_override,
                rerank_enabled: payload.rerank_enabled,
                rerank_path_override: payload.rerank_path_override,
                is_enabled: payload.lifecycle.is_enabled,
                is_default: payload.lifecycle.is_default,
            },
            Self::Gemini(payload) => native_source_input(UpstreamProfileType::Gemini, payload),
            Self::Vertex(payload) => native_source_input(UpstreamProfileType::Vertex, payload),
            Self::Anthropic(payload) => {
                native_source_input(UpstreamProfileType::Anthropic, payload)
            }
            Self::Responses(payload) => {
                native_source_input(UpstreamProfileType::Responses, payload)
            }
        }
    }
}

fn openai_source_input(
    profile_type: UpstreamProfileType,
    payload: OpenAiSourcePayload,
) -> UpstreamSourceCreateInput {
    UpstreamSourceCreateInput {
        profile_type,
        base_url: payload.base_url,
        use_proxy: payload.lifecycle.use_proxy,
        chat_completions_enabled: payload.chat_completions_enabled,
        chat_completions_path_override: payload.chat_completions_path_override,
        embeddings_enabled: payload.embeddings_enabled,
        embeddings_path_override: payload.embeddings_path_override,
        rerank_enabled: None,
        rerank_path_override: None,
        is_enabled: payload.lifecycle.is_enabled,
        is_default: payload.lifecycle.is_default,
    }
}

fn native_source_input(
    profile_type: UpstreamProfileType,
    payload: NativeSourcePayload,
) -> UpstreamSourceCreateInput {
    UpstreamSourceCreateInput {
        profile_type,
        base_url: Some(payload.base_url),
        use_proxy: payload.lifecycle.use_proxy,
        chat_completions_enabled: None,
        chat_completions_path_override: None,
        embeddings_enabled: None,
        embeddings_path_override: None,
        rerank_enabled: None,
        rerank_path_override: None,
        is_enabled: payload.lifecycle.is_enabled,
        is_default: payload.lifecycle.is_default,
    }
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
    model_kind: ModelKind,
    #[serde(default)]
    save_and_test: bool,
    api_key_description: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BootstrapCheckStatus {
    Success,
    Failed,
    CheckSkipped,
}

#[derive(Serialize)]
struct BootstrapCheckResult {
    status: BootstrapCheckStatus,
    message: String,
}

impl BootstrapCheckResult {
    fn success(message: impl Into<String>) -> Self {
        Self {
            status: BootstrapCheckStatus::Success,
            message: message.into(),
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            status: BootstrapCheckStatus::Failed,
            message: message.into(),
        }
    }

    fn skipped(message: impl Into<String>) -> Self {
        Self {
            status: BootstrapCheckStatus::CheckSkipped,
            message: message.into(),
        }
    }

    fn audit_success(&self) -> Option<bool> {
        match self.status {
            BootstrapCheckStatus::Success => Some(true),
            BootstrapCheckStatus::Failed => Some(false),
            BootstrapCheckStatus::CheckSkipped => None,
        }
    }
}

#[derive(Serialize)]
struct BootstrapProviderResponse {
    provider: ProviderAggregateResponse,
    created_key: ProviderApiKeySummary,
    created_model: Model,
    provider_name: String,
    provider_key: String,
    check_result: Option<BootstrapCheckResult>,
}

async fn list() -> DbResult<HttpResult<Vec<ProviderAggregateResponse>>> {
    let result = Provider::list_all()?;
    Ok(HttpResult::new(
        result
            .into_iter()
            .map(ProviderAggregateResponse::from)
            .collect(),
    ))
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
    profile_type: Option<UpstreamProfileType>,
    #[serde(default, with = "serde_with::rust::double_option")]
    base_url: Option<Option<String>>,
    use_proxy: Option<bool>,
    chat_completions_enabled: Option<bool>,
    #[serde(default, with = "serde_with::rust::double_option")]
    chat_completions_path_override: Option<Option<String>>,
    embeddings_enabled: Option<bool>,
    #[serde(default, with = "serde_with::rust::double_option")]
    embeddings_path_override: Option<Option<String>>,
    rerank_enabled: Option<bool>,
    #[serde(default, with = "serde_with::rust::double_option")]
    rerank_path_override: Option<Option<String>>,
    is_enabled: Option<bool>,
    is_default: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceImpactPayload {
    action: SourceImpactAction,
}

fn source_create_input(payload: UpstreamSourcePayload) -> UpstreamSourceCreateInput {
    payload.into_input()
}

async fn insert(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<InserPayload>,
) -> DbResult<HttpResult<ProviderAggregateResponse>> {
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

    Ok(HttpResult::new(created_provider.into()))
}

async fn get_provider(
    Path(id): Path<i64>,
) -> Result<HttpResult<ProviderAggregateResponse>, BaseError> {
    let provider = Provider::get_by_id(id)?;

    Ok(HttpResult::new(provider.into()))
}

async fn update_provider(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateProviderPayload>,
) -> Result<HttpResult<ProviderAggregateResponse>, BaseError> {
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

    Ok(HttpResult::new(updated_provider.into()))
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
) -> Result<HttpResult<UpstreamSourceResponse>, BaseError> {
    let source = app_state
        .admin
        .provider
        .create_source(provider_id, source_create_input(payload))
        .await?;
    Ok(HttpResult::new(source.into()))
}

async fn update_source(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateSourcePayload>,
) -> Result<HttpResult<UpstreamSourceResponse>, BaseError> {
    if payload.profile_type.is_some() {
        return Err(BaseError::ParamInvalid(Some(
            "upstream source profile_type is immutable after creation".to_string(),
        )));
    }
    let source = app_state
        .admin
        .provider
        .update_source(
            provider_id,
            source_id,
            UpstreamSourceUpdateInput {
                base_url: payload.base_url,
                use_proxy: payload.use_proxy,
                chat_completions_enabled: payload.chat_completions_enabled,
                chat_completions_path_override: payload.chat_completions_path_override,
                embeddings_enabled: payload.embeddings_enabled,
                embeddings_path_override: payload.embeddings_path_override,
                rerank_enabled: payload.rerank_enabled,
                rerank_path_override: payload.rerank_path_override,
                is_enabled: payload.is_enabled,
                is_default: payload.is_default,
            },
        )
        .await?;
    Ok(HttpResult::new(source.into()))
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
        provider: detail.provider.into(),
        models,
        provider_keys: detail.api_keys,
        request_patch_variants: detail.request_patch_variants,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftCheckModel {
    model_kind: ModelKind,
    upstream_model_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckProviderPayload {
    model_id: Option<i64>,
    draft_model: Option<DraftCheckModel>,
    provider_api_key_id: Option<i64>,
    provider_api_key: Option<String>,
}

enum ProviderCheckModel {
    Saved(Model),
    Draft { upstream_model_name: String },
}

impl ProviderCheckModel {
    fn resolve(
        payload: CheckProviderPayload,
        provider_id: i64,
    ) -> Result<(Self, CheckProviderCredentialInput), BaseError> {
        let selected = match (payload.model_id, payload.draft_model) {
            (Some(model_id), None) => {
                let model = Model::get_by_id(model_id)?;
                if model.provider_id != provider_id {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "Model {} does not belong to provider {}",
                        model_id, provider_id
                    ))));
                }
                if model.model_kind != ModelKind::Chat {
                    return Err(BaseError::ParamInvalid(Some(
                        "Source Check only supports CHAT models".to_string(),
                    )));
                }
                if !model.is_enabled {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "Model {} is disabled",
                        model_id
                    ))));
                }
                Self::Saved(model)
            }
            (None, Some(draft)) => {
                if draft.model_kind != ModelKind::Chat {
                    return Err(BaseError::ParamInvalid(Some(
                        "Source Check only supports CHAT draft models".to_string(),
                    )));
                }
                let upstream_model_name = draft.upstream_model_name.trim().to_string();
                if upstream_model_name.is_empty() {
                    return Err(BaseError::ParamInvalid(Some(
                        "draft_model.upstream_model_name must not be empty".to_string(),
                    )));
                }
                Self::Draft {
                    upstream_model_name,
                }
            }
            _ => {
                return Err(BaseError::ParamInvalid(Some(
                    "Exactly one of model_id or draft_model must be provided".to_string(),
                )));
            }
        };

        Ok((
            selected,
            CheckProviderCredentialInput {
                provider_api_key_id: payload.provider_api_key_id,
                provider_api_key: payload.provider_api_key,
            },
        ))
    }

    fn upstream_model_name(&self) -> String {
        match self {
            Self::Saved(model) => model
                .real_model_name
                .clone()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| model.model_name.clone()),
            Self::Draft {
                upstream_model_name,
            } => upstream_model_name.clone(),
        }
    }

    fn saved_model(&self) -> Option<&Model> {
        match self {
            Self::Saved(model) => Some(model),
            Self::Draft { .. } => None,
        }
    }
}

struct CheckProviderCredentialInput {
    provider_api_key_id: Option<i64>,
    provider_api_key: Option<String>,
}

#[derive(Serialize)]
struct SourceEvidence {
    source_id: i64,
    profile_type: UpstreamProfileType,
    provider_api_key_id: Option<i64>,
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
    let variants = app_state
        .catalog
        .get_request_patch_variants()
        .await
        .map_err(|error| BaseError::InternalServerError(Some(error.to_string())))?;
    let trace = resolve_runtime_request_patch_trace(
        source,
        cache_model.as_ref().map(|model| model.id),
        None,
        variants.as_ref(),
    );
    if let Some(error) = trace.execution_error(
        &cache_provider.provider_key,
        cache_model
            .as_ref()
            .map_or("<provider-check>", |model| model.model_name.as_str()),
    ) {
        return Err(provider_check_patch_error(error));
    }

    Ok(trace.applied_rules)
}

async fn build_provider_check_request_pre_auth(
    source: &UpstreamSource,
    model_name: &str,
    request_patches: &[RuntimeResolvedRequestPatch],
) -> Result<ProviderCheckRequest, BaseError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let cache_source = crate::service::cache::types::CacheUpstreamSource {
        id: source.id,
        profile_type: source.profile_type,
        base_url: source.base_url.clone(),
        use_proxy: source.use_proxy,
        chat_completions_enabled: source.chat_completions_enabled,
        chat_completions_path_override: source.chat_completions_path_override.clone(),
        embeddings_enabled: source.embeddings_enabled,
        embeddings_path_override: source.embeddings_path_override.clone(),
        rerank_enabled: source.rerank_enabled,
        rerank_path_override: source.rerank_path_override.clone(),
        is_enabled: source.is_enabled,
        is_default: source.is_default,
    };
    let mut request = match source.profile_type {
        UpstreamProfileType::Gemini | UpstreamProfileType::Vertex => ProviderCheckRequest {
            url: gemini_operation_target_url(
                &source.base_url,
                model_name,
                GeminiModelOperation::GenerateContent,
            )
            .map_err(|error| {
                BaseError::ParamInvalid(Some(format!(
                    "Source does not support Gemini generateContent check: {error}"
                )))
            })?
            .to_string(),
            headers: sanitize_gemini_request_headers(&headers),
            body: gemini_source_check_body(),
        },
        UpstreamProfileType::OpenaiCompatible => ProviderCheckRequest {
            url: resolve_source_operation_url(&cache_source, UpstreamOperation::ChatCompletions)
                .map_err(source_chat_check_error)?,
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
            enforce_anthropic_version_header(&mut headers);
            ProviderCheckRequest {
                url: anthropic_messages_url(&source.base_url).map_err(|error| {
                    BaseError::ParamInvalid(Some(format!(
                        "Source does not support Anthropic Messages check: {error}"
                    )))
                })?,
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
        UpstreamProfileType::Openai | UpstreamProfileType::GeminiOpenai => ProviderCheckRequest {
            url: resolve_source_operation_url(&cache_source, UpstreamOperation::ChatCompletions)
                .map_err(source_chat_check_error)?,
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
        UpstreamProfileType::Responses => ProviderCheckRequest {
            url: join_base_url_and_operation_path(&source.base_url, "responses").map_err(
                |error| {
                    BaseError::ParamInvalid(Some(format!(
                        "Source does not support Responses check: {error}"
                    )))
                },
            )?,
            headers,
            body: json!({
                "model": model_name,
                "input": "hi",
                "store": false,
                "stream": false,
                "max_output_tokens": 1
            }),
        },
    };

    let upstream_protocol = upstream_protocol_for_profile(&cache_source.profile_type);
    request.body = finalize_request_data(
        request.body,
        upstream_protocol,
        &cache_source.profile_type,
        "source-check",
    );
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
    if upstream_protocol == UpstreamProtocol::Anthropic {
        enforce_anthropic_version_header(&mut request.headers);
    }
    if matches!(
        upstream_protocol,
        UpstreamProtocol::Openai
            | UpstreamProtocol::Responses
            | UpstreamProtocol::Anthropic
            | UpstreamProtocol::Gemini
    ) {
        validate_final_generation_request(
            &request.body,
            upstream_protocol,
            &cache_source.profile_type,
        )
        .map_err(|error| {
            BaseError::ParamInvalid(Some(format!(
                "final target Profile validation failed at {error}"
            )))
        })?;
    }
    if upstream_protocol == UpstreamProtocol::Gemini {
        validate_gemini_pre_auth_target(
            &url,
            &request.headers,
            GeminiModelOperation::GenerateContent,
        )
        .map_err(|error| {
            BaseError::ParamInvalid(Some(format!(
                "final Gemini target validation failed: {error}"
            )))
        })?;
    }
    request.url = url.to_string();

    Ok(request)
}

#[cfg(test)]
async fn build_provider_check_request(
    _provider: &ProviderAggregate,
    source: &UpstreamSource,
    credential: &ProviderCredential,
    model_name: &str,
    request_patches: &[RuntimeResolvedRequestPatch],
) -> Result<ProviderCheckRequest, BaseError> {
    let mut request =
        build_provider_check_request_pre_auth(source, model_name, request_patches).await?;
    let cache_source = crate::service::cache::types::CacheUpstreamSource::from(source.clone());
    apply_provider_request_auth_header(
        &mut request.headers,
        &cache_source,
        upstream_protocol_for_profile(&source.profile_type),
        credential,
    )
    .map_err(provider_credential_error)?;
    Ok(request)
}

fn source_chat_check_error(error: impl std::fmt::Display) -> BaseError {
    BaseError::ParamInvalid(Some(format!("Source does not support Chat check: {error}")))
}

fn validate_source_for_chat_check(
    source: &crate::service::cache::types::CacheUpstreamSource,
) -> Result<(), BaseError> {
    if matches!(
        source.profile_type,
        UpstreamProfileType::Openai
            | UpstreamProfileType::OpenaiCompatible
            | UpstreamProfileType::GeminiOpenai
    ) {
        resolve_source_operation_url(source, UpstreamOperation::ChatCompletions)
            .map_err(source_chat_check_error)?;
    }
    Ok(())
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
    source.base_url = normalize_provider_base_url(&source.base_url).map_err(|error| {
        BaseError::ParamInvalid(Some(format!(
            "upstream source base URL is invalid and must be repaired before use: {error}"
        )))
    })?;
    Ok(source)
}

fn profile_type_label(profile_type: &UpstreamProfileType) -> &'static str {
    match profile_type {
        UpstreamProfileType::Openai => "OpenAI",
        UpstreamProfileType::Gemini => "Gemini",
        UpstreamProfileType::Vertex => "Vertex",
        UpstreamProfileType::OpenaiCompatible => "OpenAI Compatible",
        UpstreamProfileType::Anthropic => "Anthropic",
        UpstreamProfileType::Responses => "Responses",
        UpstreamProfileType::GeminiOpenai => "Gemini OpenAI",
    }
}

fn base_url_host(base_url: &str) -> String {
    let trimmed = base_url.trim();
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

fn generated_provider_name(profile_type: &UpstreamProfileType, base_url: &str) -> String {
    let host = base_url_host(base_url);
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
    base_url: &str,
    name: Option<String>,
    key: Option<String>,
) -> Result<(String, String), BaseError> {
    let provider_name = normalize_optional_text(name)
        .unwrap_or_else(|| generated_provider_name(profile_type, base_url));

    let provider_key = normalize_optional_text(key).ok_or_else(|| {
        BaseError::ParamInvalid(Some("provider_key must be provided".to_string()))
    })?;

    Ok((provider_name, provider_key))
}

async fn send_provider_check_request(
    client: &reqwest::Client,
    source: &UpstreamSource,
    credential: &ProviderCredential,
    mut check_request: ProviderCheckRequest,
    proxy_timeouts: &ProxyTimeoutConfig,
    response_limits: &NonStreamResponseConfig,
) -> Result<(), BaseError> {
    let cache_source = crate::service::cache::types::CacheUpstreamSource::from(source.clone());
    let upstream_protocol = upstream_protocol_for_profile(&source.profile_type);
    apply_provider_request_auth_header(
        &mut check_request.headers,
        &cache_source,
        upstream_protocol,
        credential,
    )
    .map_err(provider_credential_error)?;

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

    if matches!(
        source.profile_type,
        UpstreamProfileType::Gemini | UpstreamProfileType::Vertex
    ) {
        if normalize_content_type(response.headers())
            .is_none_or(|content_type| content_type.essence != "application/json")
        {
            return Err(BaseError::ParamInvalid(Some(
                "Gemini Provider check returned an invalid content type".to_string(),
            )));
        }
        let complete = read_complete_response_body_with_timeouts(
            response,
            response_limits,
            Some(ResponseBodyReadTimeouts {
                first_byte: proxy_timeouts.first_byte(),
                response_idle: proxy_timeouts.response_idle(),
            }),
            |_, _, _| {},
        )
        .await
        .map_err(|error| {
            BaseError::ParamInvalid(Some(format!(
                "Gemini Provider check response could not be read: {error}"
            )))
        })?;
        let body: Value = serde_json::from_slice(&complete.bytes).map_err(|_| {
            BaseError::ParamInvalid(Some(
                "Gemini Provider check returned invalid JSON".to_string(),
            ))
        })?;
        validate_gemini_source_check_response(&body).map_err(|error| {
            BaseError::ParamInvalid(Some(format!("Gemini Provider check failed: {error}")))
        })?;
    } else {
        drop(response);
    }
    Ok(())
}

#[cfg(test)]
async fn perform_provider_check(
    client: &reqwest::Client,
    _provider: &ProviderAggregate,
    source: &UpstreamSource,
    credential: &ProviderCredential,
    model_name: &str,
    request_patches: &[RuntimeResolvedRequestPatch],
    proxy_timeouts: &ProxyTimeoutConfig,
    response_limits: &NonStreamResponseConfig,
) -> Result<(), BaseError> {
    let request =
        build_provider_check_request_pre_auth(source, model_name, request_patches).await?;
    send_provider_check_request(
        client,
        source,
        credential,
        request,
        proxy_timeouts,
        response_limits,
    )
    .await
}

fn build_bootstrap_response(
    created: BootstrapProviderResult,
    provider_name: String,
    provider_key: String,
    check_result: Option<BootstrapCheckResult>,
) -> BootstrapProviderResponse {
    BootstrapProviderResponse {
        provider: created.provider.into(),
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
) -> Result<HttpResult<SourceEvidence>, BaseError> {
    let (selected_model, credential_input) = ProviderCheckModel::resolve(payload, id)?;
    let model_name = selected_model.upstream_model_name();

    let provider = Provider::get_by_id(id)?;
    let source = normalize_source_for_outbound(UpstreamSource::get_active_by_id_for_provider(
        source_id, id,
    )?)?;
    if let Some(model) = selected_model.saved_model() {
        validate_saved_model_source_for_check(model, source.id)?;
    }
    let cache_source = crate::service::cache::types::CacheUpstreamSource::from(source.clone());
    validate_source_for_chat_check(&cache_source)?;
    let request_patches = resolve_provider_check_request_patches(
        &app_state,
        &provider,
        selected_model.saved_model(),
        &cache_source,
    )
    .await?;
    let check_request =
        build_provider_check_request_pre_auth(&source, &model_name, &request_patches).await?;
    // Capture the durable identity before resolving the request credential. Draft
    // credentials use an internal key_id of 0 for provider-specific materialization,
    // but that implementation detail must never cross the manager API boundary.
    let provider_api_key_id = credential_input.provider_api_key_id;
    let provider_api_key_identity = provider_api_key_id
        .map(|key_id| format!("saved:{key_id}"))
        .unwrap_or_else(|| "draft".to_string());
    let credential = match (provider_api_key_id, credential_input.provider_api_key) {
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

    let proxy_request_config = app_state.infra.proxy_request_config();
    send_provider_check_request(
        client.as_ref(),
        &source,
        &credential,
        check_request,
        &proxy_request_config.timeouts,
        &proxy_request_config.non_stream_response,
    )
    .await?;
    info!(
        "provider check succeeded: provider_id={}, source_id={}, profile_type={:?}, provider_api_key_identity={}",
        provider.id, source.id, source.profile_type, provider_api_key_identity,
    );
    Ok(HttpResult::new(SourceEvidence {
        source_id: source.id,
        profile_type: source.profile_type,
        provider_api_key_id,
    }))
}

async fn bootstrap_provider(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<BootstrapProviderPayload>,
) -> Result<HttpResult<BootstrapProviderResponse>, BaseError> {
    let initial_source = payload.initial_source.into_input();
    let effective_base_url = normalize_source_base_url(
        &initial_source.profile_type,
        initial_source.base_url.as_deref(),
    )
    .map_err(|error| BaseError::ParamInvalid(Some(format!("upstream source base URL {error}"))))?;
    let (provider_name, provider_key) = resolve_bootstrap_identity(
        &initial_source.profile_type,
        &effective_base_url,
        payload.name.clone(),
        payload.key.clone(),
    )?;

    let provider_input = BootstrapProviderCommand {
        provider_id: ID_GENERATOR.generate_id(),
        provider_key: provider_key.clone(),
        name: provider_name.clone(),
        source: initial_source,
        provider_api_key_mode: ProviderApiKeyMode::Queue,
        api_key: payload.api_key.clone(),
        api_key_description: normalize_optional_text(payload.api_key_description.clone()),
        model_name: payload.model_name.clone(),
        real_model_name: normalize_optional_text(payload.real_model_name.clone()),
        model_kind: payload.model_kind,
    };

    let created = app_state
        .admin
        .provider
        .bootstrap_provider_persist(provider_input)
        .await?;

    let check_result = if payload.save_and_test && payload.model_kind != ModelKind::Chat {
        Some(BootstrapCheckResult::skipped(
            "Source Check only supports CHAT models; the Provider, Source, credential, and model were saved without a check",
        ))
    } else if payload.save_and_test {
        let source = created
            .provider
            .upstream_sources
            .first()
            .cloned()
            .ok_or_else(|| {
                BaseError::DatabaseFatal(Some("bootstrap source missing".to_string()))
            })?;
        let cache_source = crate::service::cache::types::CacheUpstreamSource::from(source.clone());
        let client = app_state.infra.provider_client(source.use_proxy).await;
        let chat_operation = validate_source_for_chat_check(&cache_source);
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

        let prepared_check = match (chat_operation, client, request_patches) {
            (Ok(()), Ok(client), Ok(request_patches)) => build_provider_check_request_pre_auth(
                &source,
                &model_name_to_check,
                &request_patches,
            )
            .await
            .map(|request| (client, request)),
            (Err(error), _, _) => Err(error),
            (_, Err(error), _) => Err(BaseError::ParamInvalid(Some(error.to_string()))),
            (_, _, Err(error)) => Err(error),
        };
        let credential_and_request = match prepared_check {
            Ok((client, request)) => resolve_draft_provider_credential(
                &created.provider,
                &cache_source,
                created.created_key.id,
                SensitiveSecret::new(payload.api_key),
                &app_state,
            )
            .await
            .map(|credential| (client, credential, request))
            .map_err(provider_credential_error),
            Err(error) => Err(error),
        };
        match credential_and_request {
            Err(error) => Some(BootstrapCheckResult::failed(base_error_message(&error))),
            Ok((client, credential, request)) => {
                let proxy_request_config = app_state.infra.proxy_request_config();
                match send_provider_check_request(
                    client.as_ref(),
                    &source,
                    &credential,
                    request,
                    &proxy_request_config.timeouts,
                    &proxy_request_config.non_stream_response,
                )
                .await
                {
                    Ok(()) => Some(BootstrapCheckResult::success("Provider check succeeded")),
                    Err(e) => Some(BootstrapCheckResult::failed(base_error_message(&e))),
                }
            }
        }
    } else {
        None
    };

    app_state
        .admin
        .provider
        .record_bootstrap_audit(
            &created,
            check_result
                .as_ref()
                .and_then(BootstrapCheckResult::audit_success),
        )
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
            provider: detail.provider.into(),
            models,
            provider_keys: detail.api_keys,
            request_patch_variants: detail.request_patch_variants,
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
    use crate::database::request_log::{RequestLog, RequestLogQueryPayload};
    use crate::database::request_patch::{
        RequestPatchRuleInput, RequestPatchVariantInput, RequestPatchVariantRepository,
    };
    use crate::database::upstream_source::UpstreamSource;
    use crate::ingress::client_identity::{ClientIdentity, ClientIdentitySource};
    use crate::schema::enum_def::{
        ModelKind, ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement,
        UpstreamProfileType,
    };
    use crate::service::app_state::{AppState, create_test_app_state};
    use crate::service::cache::types::{
        RequestPatchSource, RequestPatchVariantOrigin, RuntimeResolvedRequestPatch,
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
            source: RequestPatchSource::Variant {
                variant_id: id,
                origin: RequestPatchVariantOrigin::SourceBase,
            },
            source_rule_id: Some(id),
            source_origin: Some(RequestPatchVariantOrigin::SourceBase),
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
        for base_url in [
            "https://api.anthropic.com/v1",
            "https://api.anthropic.com/v1/",
        ] {
            let provider = sample_provider(UpstreamProfileType::Anthropic, base_url);
            let request = super::build_provider_check_request(
                &provider,
                &provider.upstream_sources[0],
                &credential("ak-test"),
                "claude-3-5-haiku-latest",
                &[
                    request_patch(
                        21,
                        RequestPatchPlacement::Header,
                        "anthropic-beta",
                        RequestPatchOperation::Set,
                        Some(json!("source-check-beta")),
                    ),
                    request_patch(
                        22,
                        RequestPatchPlacement::Body,
                        "/max_tokens",
                        RequestPatchOperation::Set,
                        Some(json!(2)),
                    ),
                ],
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
            assert_eq!(
                request
                    .headers
                    .get("anthropic-beta")
                    .expect("anthropic-beta"),
                "source-check-beta"
            );
            assert_eq!(request.body["model"], "claude-3-5-haiku-latest");
            assert_eq!(request.body["max_tokens"], 2);
            assert_eq!(request.body["messages"][0]["content"], "hi");
        }
    }

    #[tokio::test]
    async fn anthropic_check_rejects_invalid_patch_and_auth_before_transport() {
        let provider = sample_provider(
            UpstreamProfileType::Anthropic,
            "https://api.anthropic.com/v1",
        );
        let error = match super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
            &credential("ak-private-secret"),
            "claude-model",
            &[request_patch(
                23,
                RequestPatchPlacement::Body,
                "/max_tokens",
                RequestPatchOperation::Set,
                Some(json!(0)),
            )],
        )
        .await
        {
            Ok(_) => panic!("invalid Patch must fail final validation"),
            Err(error) => error,
        };
        let message = super::base_error_message(&error);
        assert!(message.contains("/max_tokens"));
        assert!(!message.contains("ak-private-secret"));

        let error = match super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
            &credential("invalid\ncredential"),
            "claude-model",
            &[],
        )
        .await
        {
            Ok(_) => panic!("invalid auth header must fail request construction"),
            Err(error) => error,
        };
        let message = super::base_error_message(&error);
        assert!(message.contains("auth header"));
        assert!(!message.contains("invalid\ncredential"));
    }

    #[tokio::test]
    async fn responses_check_request_uses_native_stateless_contract() {
        let provider = sample_provider(
            UpstreamProfileType::Responses,
            "https://relay.example/proxy/openai/v1/",
        );
        let request = super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
            &credential("responses-test-secret"),
            "gpt-5-responses",
            &[],
        )
        .await
        .expect("Responses check request should build");

        assert_eq!(
            request.url,
            "https://relay.example/proxy/openai/v1/responses"
        );
        assert_eq!(
            request
                .headers
                .get(reqwest::header::AUTHORIZATION)
                .expect("Responses auth header"),
            "Bearer responses-test-secret"
        );
        assert_eq!(
            request.body,
            json!({
                "model": "gpt-5-responses",
                "input": "hi",
                "store": false,
                "stream": false,
                "max_output_tokens": 1
            })
        );
        assert!(request.body.get("messages").is_none());
    }

    #[tokio::test]
    async fn responses_check_rejects_stateless_patch_target() {
        let provider =
            sample_provider(UpstreamProfileType::Responses, "https://api.example.com/v1");
        let error = match super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
            &credential("responses-private-secret"),
            "gpt-5-responses",
            &[request_patch(
                4,
                RequestPatchPlacement::Body,
                "/store",
                RequestPatchOperation::Set,
                Some(json!(true)),
            )],
        )
        .await
        {
            Ok(_) => panic!("Source Check must not bypass the stateless reserved target"),
            Err(error) => error,
        };
        let message = super::base_error_message(&error);
        assert!(message.contains("reserved"));
        assert!(!message.contains("responses-private-secret"));
    }

    #[tokio::test]
    async fn responses_source_check_non_success_calls_native_endpoint_once() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();
        let upstream = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 8192];
            let read = socket.read(&mut request).await.unwrap();
            let _ = request_tx.send(String::from_utf8_lossy(&request[..read]).to_string());
            socket
                .write_all(
                    b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let provider = sample_provider(
            UpstreamProfileType::Responses,
            &format!("http://{address}/proxy/v1/"),
        );

        let error = super::perform_provider_check(
            &reqwest::Client::new(),
            &provider,
            &provider.upstream_sources[0],
            &credential("responses-non-success-secret"),
            "responses-non-success-model",
            &[],
            &crate::config::ProxyTimeoutConfig::default(),
            &crate::config::NonStreamResponseConfig::default(),
        )
        .await
        .expect_err("non-success Responses check should fail");

        assert!(super::base_error_message(&error).contains("429"));
        let request = request_rx.await.expect("one Source Check request");
        let request_lower = request.to_ascii_lowercase();
        assert!(request_lower.starts_with("post /proxy/v1/responses http/1.1"));
        assert!(request_lower.contains("authorization: bearer responses-non-success-secret"));
        assert!(!request_lower.contains("chat/completions"));
        assert!(!request_lower.contains("\"messages\""));
        timeout(Duration::from_secs(2), upstream)
            .await
            .expect("single upstream call should complete")
            .expect("upstream fixture should join");
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
        assert_eq!(request.body["generationConfig"]["candidateCount"], 1);
        assert_eq!(request.body["generationConfig"]["maxOutputTokens"], 1);
    }

    #[tokio::test]
    async fn gemini_check_revalidates_patched_body_before_authentication() {
        let provider = sample_provider(
            UpstreamProfileType::Gemini,
            "https://generativelanguage.googleapis.com/v1beta/models",
        );
        let error = match super::build_provider_check_request(
            &provider,
            &provider.upstream_sources[0],
            &credential("gemini-source-check-private-key"),
            "gemini-2.0-flash",
            &[request_patch(
                19,
                RequestPatchPlacement::Body,
                "/contents/0/parts",
                RequestPatchOperation::Set,
                Some(json!("gemini-source-check-private-marker")),
            )],
        )
        .await
        {
            Ok(_) => panic!("invalid patched Gemini body must fail before authentication"),
            Err(error) => error,
        };

        let message = super::base_error_message(&error);
        assert!(message.contains("/contents/*/parts"));
        assert!(!message.contains("private-marker"));
        assert!(!message.contains("private-key"));
    }

    #[tokio::test]
    async fn gemini_and_vertex_source_check_send_shared_minimal_contract_once() {
        let response_body = br#"{"responseId":"check-response","candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"ok"}]},"finishReason":"STOP"}]}"#;
        for profile_type in [UpstreamProfileType::Gemini, UpstreamProfileType::Vertex] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let (request_tx, request_rx) = oneshot::channel();
            let upstream = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0u8; 8192];
                let read = socket.read(&mut request).await.unwrap();
                let _ = request_tx.send(String::from_utf8_lossy(&request[..read]).to_string());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response_body.len(),
                    String::from_utf8_lossy(response_body)
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            });
            let base_url = match profile_type {
                UpstreamProfileType::Gemini => format!("http://{address}/v1beta/models"),
                UpstreamProfileType::Vertex => {
                    format!("http://{address}/v1/projects/p/locations/l/publishers/google/models")
                }
                _ => unreachable!(),
            };
            let provider = sample_provider(profile_type.clone(), &base_url);

            super::perform_provider_check(
                &reqwest::Client::new(),
                &provider,
                &provider.upstream_sources[0],
                &credential("gemini-check-credential"),
                "gemini-check-model",
                &[],
                &crate::config::ProxyTimeoutConfig::default(),
                &crate::config::NonStreamResponseConfig::default(),
            )
            .await
            .expect("valid native Gemini check should succeed");

            let request = request_rx.await.expect("one captured Source Check");
            let request_lower = request.to_ascii_lowercase();
            assert!(request_lower.starts_with(&format!(
                "post /{}",
                match profile_type {
                    UpstreamProfileType::Gemini => {
                        "v1beta/models/gemini-check-model:generatecontent"
                    }
                    UpstreamProfileType::Vertex => "v1/projects/p/locations/l/publishers/google/models/gemini-check-model:generatecontent",
                    _ => unreachable!(),
                }
            )));
            match profile_type {
                UpstreamProfileType::Gemini => {
                    assert!(request_lower.contains("x-goog-api-key: gemini-check-credential"));
                    assert!(!request_lower.contains("authorization: bearer"));
                }
                UpstreamProfileType::Vertex => {
                    assert!(
                        request_lower.contains("authorization: bearer gemini-check-credential")
                    );
                    assert!(!request_lower.contains("x-goog-api-key"));
                }
                _ => unreachable!(),
            }
            let body = request
                .split_once("\r\n\r\n")
                .map(|(_, body)| body)
                .expect("captured HTTP body");
            assert_eq!(
                serde_json::from_str::<Value>(body).expect("Source Check JSON body"),
                crate::service::provider_http::gemini_source_check_body()
            );
            timeout(Duration::from_secs(2), upstream)
                .await
                .expect("single check should finish")
                .expect("upstream fixture should join");
        }
    }

    #[tokio::test]
    async fn gemini_source_check_rejects_malformed_2xx_application_and_http_errors() {
        for (status, content_type, body, expected) in [
            (200, "application/json", "", "invalid JSON"),
            (200, "application/json", "{}", "exactly one candidate"),
            (
                200,
                "application/json",
                r#"{"error":{"message":"response-private-marker"}}"#,
                "application error",
            ),
            (200, "text/plain", "not-json", "invalid content type"),
            (
                429,
                "application/json",
                r#"{"error":{"message":"rate-private-marker"}}"#,
                "429",
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let upstream = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0u8; 8192];
                let _ = socket.read(&mut request).await.unwrap();
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            });
            let provider = sample_provider(
                UpstreamProfileType::Gemini,
                &format!("http://{address}/v1beta/models"),
            );

            let error = super::perform_provider_check(
                &reqwest::Client::new(),
                &provider,
                &provider.upstream_sources[0],
                &credential("gemini-check-private-key"),
                "gemini-check-model",
                &[],
                &crate::config::ProxyTimeoutConfig::default(),
                &crate::config::NonStreamResponseConfig::default(),
            )
            .await
            .expect_err("invalid Gemini check response must fail");
            let message = super::base_error_message(&error);
            assert!(message.contains(expected), "message={message}");
            assert!(!message.contains("private-marker"));
            assert!(!message.contains("private-key"));
            timeout(Duration::from_secs(2), upstream)
                .await
                .expect("failed check should finish")
                .expect("upstream fixture should join");
        }
    }

    #[tokio::test]
    async fn gemini_source_check_saved_and_draft_share_patch_contract_without_proxy_logs() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-gemini-source-check-native-http.sqlite");
        test_db_context
            .run_async(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let (requests_tx, requests_rx) = oneshot::channel();
                let upstream = tokio::spawn(async move {
                    let response_body = br#"{"responseId":"check-response","candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"ok"}]},"finishReason":"STOP"}]}"#;
                    let mut requests = Vec::new();
                    for _ in 0..2 {
                        let (mut socket, _) = listener.accept().await.unwrap();
                        let mut request = vec![0u8; 8192];
                        let read = socket.read(&mut request).await.unwrap();
                        requests.push(String::from_utf8_lossy(&request[..read]).to_string());
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            response_body.len(),
                            String::from_utf8_lossy(response_body)
                        );
                        socket.write_all(response.as_bytes()).await.unwrap();
                    }
                    let _ = requests_tx.send(requests);
                });

                let provider_id = 26190;
                let source_id = 26191;
                let provider = Provider::create(
                    &crate::database::provider::NewProvider {
                        id: provider_id,
                        provider_key: "gemini-check-provider".to_string(),
                        name: "Gemini Check Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &crate::database::upstream_source::NewUpstreamSource {
                        id: source_id,
                        provider_id,
                        profile_type: UpstreamProfileType::Gemini,
                        base_url: format!("http://{address}/v1beta/models"),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                        ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                            UpstreamProfileType::Gemini,
                        )
                    },
                )
                .expect("Gemini provider should seed")
                .provider;
                let saved_model = Model::create(
                    provider.id,
                    "saved-gemini-check-model",
                    Some("saved-gemini-upstream-model"),
                    ModelKind::Chat,
                    true,
                )
                .expect("saved Gemini model should seed");
                let patch_input = |model_id, placement, target: &str, value: Value| {
                    RequestPatchVariantInput {
                        source_id,
                        model_id,
                        suffix: None,
                        enabled: true,
                        expose_in_models: false,
                        rules: vec![RequestPatchRuleInput {
                            placement,
                            target: target.to_string(),
                            operation: RequestPatchOperation::Set,
                            value_json: Some(Some(value)),
                            description: None,
                        }],
                    }
                };
                RequestPatchVariantRepository::create(&patch_input(
                    None,
                    RequestPatchPlacement::Query,
                    "trace",
                    json!("source-check"),
                ))
                .expect("source Patch should seed");
                RequestPatchVariantRepository::create(&patch_input(
                    Some(saved_model.id),
                    RequestPatchPlacement::Header,
                    "x-model-check-patch",
                    json!("saved-model"),
                ))
                .expect("model Patch should seed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let created = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/provider_keys"),
                        json!({"api_key":"saved-gemini-check-secret"}),
                    ),
                )
                .await;
                assert_eq!(created.status(), StatusCode::OK);
                let key_id = response_json(created).await["data"]["id"]
                    .as_i64()
                    .expect("saved key id");

                let saved = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({"model_id":saved_model.id,"provider_api_key_id":key_id}),
                    ),
                )
                .await;
                assert_eq!(saved.status(), StatusCode::OK);
                let saved_body = response_json(saved).await;
                assert_eq!(saved_body["data"]["provider_api_key_id"], key_id);

                let draft = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "draft_model":{"model_kind":"CHAT","upstream_model_name":"draft-gemini-upstream-model"},
                            "provider_api_key":"draft-gemini-check-secret"
                        }),
                    ),
                )
                .await;
                assert_eq!(draft.status(), StatusCode::OK);
                assert_eq!(response_json(draft).await["data"]["provider_api_key_id"], Value::Null);

                let requests = timeout(Duration::from_secs(2), requests_rx)
                    .await
                    .expect("two Gemini checks should finish")
                    .expect("captured Gemini Source Checks");
                assert_eq!(requests.len(), 2);
                let saved_request = requests[0].to_ascii_lowercase();
                let draft_request = requests[1].to_ascii_lowercase();
                assert!(saved_request.starts_with("post /v1beta/models/saved-gemini-upstream-model:generatecontent?trace=source-check "));
                assert!(draft_request.starts_with("post /v1beta/models/draft-gemini-upstream-model:generatecontent?trace=source-check "));
                assert!(saved_request.contains("x-goog-api-key: saved-gemini-check-secret"));
                assert!(draft_request.contains("x-goog-api-key: draft-gemini-check-secret"));
                assert!(saved_request.contains("x-model-check-patch: saved-model"));
                assert!(!draft_request.contains("x-model-check-patch"));
                for request in &requests {
                    let body = request
                        .split_once("\r\n\r\n")
                        .map(|(_, body)| body)
                        .expect("Source Check body");
                    assert_eq!(
                        serde_json::from_str::<Value>(body).expect("Gemini check body"),
                        crate::service::provider_http::gemini_source_check_body()
                    );
                }
                timeout(Duration::from_secs(2), upstream)
                    .await
                    .expect("Gemini Source Check upstream should finish")
                    .expect("Gemini Source Check task should join");
                assert!(RequestLog::list_full(RequestLogQueryPayload {
                    provider_id: Some(provider_id),
                    page: Some(1),
                    page_size: Some(10),
                    ..Default::default()
                })
                .expect("request logs should query")
                .list
                .is_empty());
            })
            .await;
    }

    #[tokio::test]
    async fn gemini_source_check_invalid_model_and_patch_fail_before_saved_key_decryption() {
        const SENTINEL: &str = "gemini-check-private-marker";
        let test_db_context =
            TestDbContext::new_sqlite("controller-gemini-check-zero-decrypt-http.sqlite");
        test_db_context
            .run_async(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let provider_id = 26192;
                let source_id = 26193;
                let provider = Provider::create(
                    &crate::database::provider::NewProvider {
                        id: provider_id,
                        provider_key: "gemini-check-zero-decrypt".to_string(),
                        name: "Gemini Check Zero Decrypt".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &crate::database::upstream_source::NewUpstreamSource {
                        id: source_id,
                        provider_id,
                        profile_type: UpstreamProfileType::Gemini,
                        base_url: format!("http://{address}/v1beta/models"),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                        ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                            UpstreamProfileType::Gemini,
                        )
                    },
                )
                .expect("Gemini provider should seed")
                .provider;
                let invalid_model = Model::create(
                    provider.id,
                    "invalid-gemini-model",
                    Some(&format!("models/{SENTINEL}")),
                    ModelKind::Chat,
                    true,
                )
                .expect("invalid legacy model should seed");
                let patched_model = Model::create(
                    provider.id,
                    "patched-gemini-model",
                    Some("valid-gemini-model"),
                    ModelKind::Chat,
                    true,
                )
                .expect("patched model should seed");
                RequestPatchVariantRepository::create(&RequestPatchVariantInput {
                    source_id,
                    model_id: Some(patched_model.id),
                    suffix: None,
                    enabled: true,
                    expose_in_models: false,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/contents/0/parts".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!(SENTINEL))),
                        description: None,
                    }],
                })
                .expect("invalid final-shape Patch should seed");
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let created = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/provider_keys"),
                        json!({"api_key":"gemini-check-saved-private-key"}),
                    ),
                )
                .await;
                assert_eq!(created.status(), StatusCode::OK);
                let key_id = response_json(created).await["data"]["id"]
                    .as_i64()
                    .expect("saved key id");

                for model_id in [invalid_model.id, patched_model.id] {
                    app_state.secret_encryption.reset_decrypt_call_count();
                    let response = send(
                        &app_state,
                        json_request(
                            Method::POST,
                            &format!("/provider/{provider_id}/sources/{source_id}/check"),
                            json!({"model_id":model_id,"provider_api_key_id":key_id}),
                        ),
                    )
                    .await;
                    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
                    let body = response_json(response).await.to_string();
                    assert!(!body.contains(SENTINEL));
                    assert!(!body.contains("private-key"));
                    assert_eq!(app_state.secret_encryption.decrypt_call_count(), 0);
                }
                assert!(
                    timeout(Duration::from_millis(100), listener.accept())
                        .await
                        .is_err(),
                    "invalid model and Patch must make zero upstream calls"
                );
                assert!(
                    RequestLog::list_full(RequestLogQueryPayload {
                        provider_id: Some(provider_id),
                        page: Some(1),
                        page_size: Some(10),
                        ..Default::default()
                    })
                    .expect("request logs should query")
                    .list
                    .is_empty()
                );
            })
            .await;
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
    async fn provider_check_rejects_patched_bodies_outside_the_final_target_profile() {
        let cases = [
            (
                UpstreamProfileType::Openai,
                request_patch(
                    1,
                    RequestPatchPlacement::Body,
                    "/messages",
                    RequestPatchOperation::Set,
                    Some(json!("invalid-messages-private-marker")),
                ),
                "/messages",
            ),
            (
                UpstreamProfileType::GeminiOpenai,
                request_patch(
                    2,
                    RequestPatchPlacement::Body,
                    "/unregistered_field",
                    RequestPatchOperation::Set,
                    Some(json!("gemini-private-marker")),
                ),
                "$",
            ),
            (
                UpstreamProfileType::Responses,
                request_patch(
                    3,
                    RequestPatchPlacement::Body,
                    "/input",
                    RequestPatchOperation::Set,
                    Some(json!({"responses-private-marker": true})),
                ),
                "/input",
            ),
        ];

        for (profile_type, patch, expected_path) in cases {
            let provider = sample_provider(profile_type, "https://api.example.com/v1");
            let error = match super::build_provider_check_request(
                &provider,
                &provider.upstream_sources[0],
                &credential("source-check-private-key"),
                "model",
                &[patch],
            )
            .await
            {
                Ok(_) => panic!("invalid patched body must fail before Source Check transport"),
                Err(error) => error,
            };

            let message = super::base_error_message(&error);
            assert!(message.contains("final target Profile validation failed"));
            assert!(message.contains(expected_path));
            assert!(!message.contains("private-marker"));
            assert!(!message.contains("source-check-private-key"));
        }
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
            &crate::config::NonStreamResponseConfig::default(),
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
    async fn provider_check_source_evidence_distinguishes_saved_and_draft_keys() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-provider-check-key-evidence-http.sqlite");

        test_db_context
            .run_async(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let upstream = tokio::spawn(async move {
                    for _ in 0..3 {
                        let (mut socket, _) = listener.accept().await.unwrap();
                        let mut request = vec![0u8; 8192];
                        let _ = socket.read(&mut request).await.unwrap();
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .await
                            .unwrap();
                    }
                });

                let provider_id = 26006;
                let source_id = 26007;
                let provider = Provider::create(
                    &crate::database::provider::NewProvider {
                        id: provider_id,
                        provider_key: "check-key-evidence-provider".to_string(),
                        name: "Check Key Evidence Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &crate::database::upstream_source::NewUpstreamSource {
                        id: source_id,
                        provider_id,
                        profile_type: UpstreamProfileType::Openai,
                        base_url: format!("http://{address}/v1"),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                        ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                            UpstreamProfileType::Openai,
                        )
                    },
                )
                .expect("provider seed should succeed")
                .provider;
                let saved_model = Model::create(
                    provider.id,
                    "saved-check-model",
                    Some("saved-check-upstream-model"),
                    ModelKind::Chat,
                    true,
                )
                .expect("saved check model should seed");
                let model_count_before_draft = Model::list_by_provider_id(provider.id)
                    .expect("provider models should list")
                    .len();
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let created = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/provider_keys"),
                        json!({
                            "api_key": "saved-check-secret",
                            "description": "saved check key"
                        }),
                    ),
                )
                .await;
                assert_eq!(created.status(), StatusCode::OK);
                let created_body = response_json(created).await;
                let key_id = created_body["data"]["id"]
                    .as_i64()
                    .expect("created key should expose an id");
                assert!(key_id > 0);
                assert!(!created_body.to_string().contains("saved-check-secret"));

                let saved = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "model_id": saved_model.id,
                            "provider_api_key_id": key_id
                        }),
                    ),
                )
                .await;
                assert_eq!(saved.status(), StatusCode::OK);
                let saved_body = response_json(saved).await;
                assert_eq!(saved_body["data"]["source_id"], source_id);
                assert_eq!(saved_body["data"]["provider_api_key_id"], key_id);
                assert!(!saved_body.to_string().contains("saved-check-secret"));

                let disabled = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/provider/{provider_id}/provider_keys/{key_id}"),
                        json!({ "is_enabled": false }),
                    ),
                )
                .await;
                assert_eq!(disabled.status(), StatusCode::OK);

                let disabled_saved = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "model_id": saved_model.id,
                            "provider_api_key_id": key_id
                        }),
                    ),
                )
                .await;
                assert_eq!(disabled_saved.status(), StatusCode::OK);
                let disabled_saved_body = response_json(disabled_saved).await;
                assert_eq!(
                    disabled_saved_body["data"]["provider_api_key_id"],
                    key_id
                );
                assert!(!disabled_saved_body.to_string().contains("saved-check-secret"));
                let detail = Provider::get_detail_by_id(provider.id)
                    .expect("provider detail should remain readable");
                assert!(!detail
                    .api_keys
                    .iter()
                    .find(|summary| summary.id == key_id)
                    .expect("checked key should remain listed")
                    .is_enabled);

                let draft = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "draft_model": {
                                "model_kind": "CHAT",
                                "upstream_model_name": "draft-check-model"
                            },
                            "provider_api_key": "draft-check-secret"
                        }),
                    ),
                )
                .await;
                assert_eq!(draft.status(), StatusCode::OK);
                let draft_body = response_json(draft).await;
                assert_eq!(draft_body["data"]["provider_api_key_id"], Value::Null);
                assert!(!draft_body.to_string().contains("draft-check-secret"));
                assert_ne!(draft_body["data"]["provider_api_key_id"], 0);
                assert_eq!(
                    Model::list_by_provider_id(provider.id)
                        .expect("draft check must not affect persisted models")
                        .len(),
                    model_count_before_draft
                );

                timeout(Duration::from_secs(2), upstream)
                    .await
                    .expect("all key evidence checks should reach upstream")
                    .expect("upstream fixture should finish");
                assert!(Provider::get_by_id(provider.id).is_ok());
            })
            .await;
    }

    #[tokio::test]
    async fn responses_source_check_saved_and_draft_keys_share_native_contract_without_proxy_logs()
    {
        let test_db_context =
            TestDbContext::new_sqlite("controller-responses-source-check-native-http.sqlite");

        test_db_context
            .run_async(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let (requests_tx, requests_rx) = oneshot::channel();
                let upstream = tokio::spawn(async move {
                    let mut requests = Vec::new();
                    for _ in 0..2 {
                        let (mut socket, _) = listener.accept().await.unwrap();
                        let mut request = vec![0u8; 8192];
                        let read = socket.read(&mut request).await.unwrap();
                        requests.push(String::from_utf8_lossy(&request[..read]).to_string());
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .await
                            .unwrap();
                    }
                    let _ = requests_tx.send(requests);
                });

                let provider_id = 26040;
                let source_id = 26041;
                let provider = Provider::create(
                    &crate::database::provider::NewProvider {
                        id: provider_id,
                        provider_key: "responses-check-provider".to_string(),
                        name: "Responses Check Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &crate::database::upstream_source::NewUpstreamSource {
                        id: source_id,
                        provider_id,
                        profile_type: UpstreamProfileType::Responses,
                        base_url: format!("http://{address}/proxy/v1/"),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                        ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                            UpstreamProfileType::Responses,
                        )
                    },
                )
                .expect("Responses provider seed should succeed")
                .provider;
                let saved_model = Model::create(
                    provider.id,
                    "saved-responses-model",
                    Some("saved-responses-real-model"),
                    ModelKind::Chat,
                    true,
                )
                .expect("saved Responses model should seed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let created = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/provider_keys"),
                        json!({
                            "api_key": "saved-responses-secret",
                            "description": "saved Responses check key"
                        }),
                    ),
                )
                .await;
                assert_eq!(created.status(), StatusCode::OK);
                let key_id = response_json(created).await["data"]["id"]
                    .as_i64()
                    .expect("saved key id");

                let saved = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "model_id": saved_model.id,
                            "provider_api_key_id": key_id
                        }),
                    ),
                )
                .await;
                assert_eq!(saved.status(), StatusCode::OK);
                let saved_evidence = response_json(saved).await;
                assert_eq!(saved_evidence["data"]["profile_type"], "RESPONSES");
                assert_eq!(saved_evidence["data"]["provider_api_key_id"], key_id);

                let draft = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "draft_model": {
                                "model_kind": "CHAT",
                                "upstream_model_name": "draft-responses-model"
                            },
                            "provider_api_key": "draft-responses-secret"
                        }),
                    ),
                )
                .await;
                assert_eq!(draft.status(), StatusCode::OK);
                let draft_evidence = response_json(draft).await;
                assert_eq!(draft_evidence["data"]["profile_type"], "RESPONSES");
                assert_eq!(draft_evidence["data"]["provider_api_key_id"], Value::Null);

                let requests = timeout(Duration::from_secs(2), requests_rx)
                    .await
                    .expect("both Responses checks should reach upstream")
                    .expect("request evidence should be returned");
                assert_eq!(requests.len(), 2);
                for (request, expected_model, expected_secret) in [
                    (
                        &requests[0],
                        "saved-responses-real-model",
                        "saved-responses-secret",
                    ),
                    (
                        &requests[1],
                        "draft-responses-model",
                        "draft-responses-secret",
                    ),
                ] {
                    let (head, body) = request
                        .split_once("\r\n\r\n")
                        .expect("HTTP request should contain a body");
                    let head = head.to_ascii_lowercase();
                    assert!(head.starts_with("post /proxy/v1/responses http/1.1"));
                    assert!(head.contains(&format!("authorization: bearer {expected_secret}")));
                    assert!(head.contains("accept-encoding: gzip, identity"));
                    let body: Value =
                        serde_json::from_str(body.trim()).expect("check body should be JSON");
                    assert_eq!(body["model"], expected_model);
                    assert_eq!(body["input"], "hi");
                    assert_eq!(body["store"], false);
                    assert_eq!(body["stream"], false);
                    assert_eq!(body["max_output_tokens"], 1);
                    assert!(body.get("messages").is_none());
                }

                app_state.flush_proxy_logs().await;
                let logs = RequestLog::list_full(RequestLogQueryPayload {
                    page: Some(1),
                    page_size: Some(10),
                    ..Default::default()
                });
                assert!(logs.expect("Request Logs should query").list.is_empty());
                timeout(Duration::from_secs(2), upstream)
                    .await
                    .expect("upstream fixture should finish")
                    .expect("upstream fixture should join");
            })
            .await;
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
                        base_url: format!("http://{address}/v1"),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                        ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                            UpstreamProfileType::Openai,
                        )
                    },
                )
                .expect("provider seed should succeed")
                .provider;
                UpstreamSource::create(&crate::database::upstream_source::NewUpstreamSource {
                    id: unbound_source_id,
                    provider_id,
                    profile_type: UpstreamProfileType::Responses,
                    base_url: "http://127.0.0.1:9/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: false,
                    created_at: 1,
                    updated_at: 1,
                    ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                        UpstreamProfileType::Responses,
                    )
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
                    crate::schema::enum_def::ModelKind::Chat,
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
                assert_eq!(valid_body["data"]["provider_api_key_id"], Value::Null);
                assert!(!valid_body.to_string().contains("draft-check-secret"));
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
                        base_url: "http://127.0.0.1:9/v1".to_string(),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                        ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                            UpstreamProfileType::Openai,
                        )
                    },
                )
                .expect("other provider seed should succeed")
                .provider;
                let other_model = Model::create(
                    other_provider.id,
                    "other-model",
                    None,
                    crate::schema::enum_def::ModelKind::Chat,
                    true,
                )
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
    async fn source_check_rejects_non_chat_saved_and_draft_models_without_upstream_call() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-provider-check-chat-only-http.sqlite");

        test_db_context
            .run_async(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let provider_id = 26020;
                let source_id = 26021;
                let provider = Provider::create(
                    &crate::database::provider::NewProvider {
                        id: provider_id,
                        provider_key: "chat-only-check-provider".to_string(),
                        name: "Chat-only Check Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &crate::database::upstream_source::NewUpstreamSource {
                        id: source_id,
                        provider_id,
                        profile_type: UpstreamProfileType::Openai,
                        base_url: format!("http://{address}/v1"),
                        use_proxy: false,
                        chat_completions_enabled: Some(false),
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                        ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                            UpstreamProfileType::Openai,
                        )
                    },
                )
                .expect("provider seed should succeed")
                .provider;
                let embedding_model = Model::create(
                    provider.id,
                    "embedding-check-model",
                    None,
                    ModelKind::Embedding,
                    true,
                )
                .expect("embedding model should seed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let saved = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "model_id": embedding_model.id,
                            "provider_api_key": "must-not-leak-saved-kind"
                        }),
                    ),
                )
                .await;
                assert_eq!(saved.status(), StatusCode::BAD_REQUEST);
                let saved_body = response_json(saved).await;
                assert!(saved_body.to_string().contains("CHAT"));
                assert!(!saved_body.to_string().contains("must-not-leak-saved-kind"));

                let draft = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "draft_model": {
                                "model_kind": "RERANK",
                                "upstream_model_name": "rerank-check-model"
                            },
                            "provider_api_key": "must-not-leak-draft-kind"
                        }),
                    ),
                )
                .await;
                assert_eq!(draft.status(), StatusCode::BAD_REQUEST);
                let draft_body = response_json(draft).await;
                assert!(draft_body.to_string().contains("CHAT"));
                assert!(!draft_body.to_string().contains("must-not-leak-draft-kind"));

                for invalid_payload in [
                    json!({ "provider_api_key": "must-not-leak-empty-model" }),
                    json!({
                        "model_id": embedding_model.id,
                        "draft_model": {
                            "model_kind": "CHAT",
                            "upstream_model_name": "ambiguous-model"
                        },
                        "provider_api_key": "must-not-leak-ambiguous-model"
                    }),
                ] {
                    let invalid = send(
                        &app_state,
                        json_request(
                            Method::POST,
                            &format!("/provider/{provider_id}/sources/{source_id}/check"),
                            invalid_payload,
                        ),
                    )
                    .await;
                    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
                    let invalid_body = response_json(invalid).await;
                    assert!(invalid_body.to_string().contains("Exactly one"));
                    assert!(!invalid_body.to_string().contains("must-not-leak"));
                }

                let disabled_operation = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "draft_model": {
                                "model_kind": "CHAT",
                                "upstream_model_name": "chat-model"
                            },
                            "provider_api_key": "must-not-leak-disabled-operation"
                        }),
                    ),
                )
                .await;
                assert_eq!(disabled_operation.status(), StatusCode::BAD_REQUEST);
                let disabled_operation_body = response_json(disabled_operation).await;
                assert!(
                    disabled_operation_body
                        .to_string()
                        .contains("operation is disabled")
                );
                assert!(
                    !disabled_operation_body
                        .to_string()
                        .contains("must-not-leak-disabled-operation")
                );

                assert!(
                    timeout(Duration::from_millis(100), listener.accept())
                        .await
                        .is_err(),
                    "non-CHAT Source Checks must not contact the upstream"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn saved_check_applies_model_patch_while_draft_check_uses_source_patch_only() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-provider-check-patch-scope-http.sqlite");

        test_db_context
            .run_async(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let (requests_tx, requests_rx) = oneshot::channel();
                tokio::spawn(async move {
                    let mut requests = Vec::new();
                    for _ in 0..2 {
                        let (mut socket, _) = listener.accept().await.unwrap();
                        let mut request = vec![0u8; 8192];
                        let read = socket.read(&mut request).await.unwrap();
                        requests.push(String::from_utf8_lossy(&request[..read]).to_string());
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .await
                            .unwrap();
                    }
                    let _ = requests_tx.send(requests);
                });

                let provider_id = 26030;
                let source_id = 26031;
                let provider = Provider::create(
                    &crate::database::provider::NewProvider {
                        id: provider_id,
                        provider_key: "check-patch-scope-provider".to_string(),
                        name: "Check Patch Scope Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &crate::database::upstream_source::NewUpstreamSource {
                        id: source_id,
                        provider_id,
                        profile_type: UpstreamProfileType::Openai,
                        base_url: format!("http://{address}/v1"),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                        ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                            UpstreamProfileType::Openai,
                        )
                    },
                )
                .expect("provider seed should succeed")
                .provider;
                let saved_model = Model::create(
                    provider.id,
                    "saved-patch-check-model",
                    None,
                    ModelKind::Chat,
                    true,
                )
                .expect("saved model should seed");

                let patch_input = |model_id, target: &str, value: &str| {
                    RequestPatchVariantInput {
                        source_id,
                        model_id,
                        suffix: None,
                        enabled: true,
                        expose_in_models: false,
                        rules: vec![RequestPatchRuleInput {
                            placement: RequestPatchPlacement::Header,
                            target: target.to_string(),
                            operation: RequestPatchOperation::Set,
                            value_json: Some(Some(json!(value))),
                            description: None,
                        }],
                    }
                };
                RequestPatchVariantRepository::create(&patch_input(
                    None,
                    "x-source-check-patch",
                    "source",
                ))
                .expect("source patch should seed");
                RequestPatchVariantRepository::create(&patch_input(
                    Some(saved_model.id),
                    "x-model-check-patch",
                    "model",
                ))
                .expect("model patch should seed");

                let app_state = create_test_app_state(test_db_context.clone()).await;
                let saved = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "model_id": saved_model.id,
                            "provider_api_key": "saved-patch-secret"
                        }),
                    ),
                )
                .await;
                assert_eq!(saved.status(), StatusCode::OK);

                let draft = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{provider_id}/sources/{source_id}/check"),
                        json!({
                            "draft_model": {
                                "model_kind": "CHAT",
                                "upstream_model_name": "draft-patch-check-model"
                            },
                            "provider_api_key": "draft-patch-secret"
                        }),
                    ),
                )
                .await;
                assert_eq!(draft.status(), StatusCode::OK);

                let requests = timeout(Duration::from_secs(2), requests_rx)
                    .await
                    .expect("both checks should reach upstream")
                    .expect("request evidence should be returned");
                let saved_request = requests[0].to_ascii_lowercase();
                let draft_request = requests[1].to_ascii_lowercase();
                assert!(saved_request.contains("x-source-check-patch: source"));
                assert!(saved_request.contains("x-model-check-patch: model"));
                assert!(draft_request.contains("x-source-check-patch: source"));
                assert!(!draft_request.contains("x-model-check-patch"));
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
                &crate::config::NonStreamResponseConfig::default(),
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
            Some(super::BootstrapCheckResult::failed("boom")),
        );

        let check_result = response.check_result.expect("check result should exist");
        assert_eq!(check_result.status, super::BootstrapCheckStatus::Failed);
        assert_eq!(check_result.message, "boom");
    }

    #[tokio::test]
    async fn bootstrap_save_and_test_checks_chat_and_skips_non_chat_after_persisting() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-provider-bootstrap-chat-only-http.sqlite");

        test_db_context
            .run_async(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let upstream = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = vec![0u8; 8192];
                    let read = socket.read(&mut request).await.unwrap();
                    let request = String::from_utf8_lossy(&request[..read]).to_string();
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await
                        .unwrap();
                    request
                });
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let chat_secret = "bootstrap-chat-secret-must-not-return";
                let chat = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider/bootstrap",
                        json!({
                            "initial_source": {
                                "profile_type": "OPENAI",
                                "base_url": format!("http://{address}/v1"),
                                "use_proxy": false,
                                "is_enabled": true,
                                "is_default": true
                            },
                            "api_key": chat_secret,
                            "model_name": "bootstrap-chat-model",
                            "model_kind": "CHAT",
                            "key": "bootstrap-chat-provider",
                            "save_and_test": true
                        }),
                    ),
                )
                .await;
                assert_eq!(chat.status(), StatusCode::OK);
                let chat_body = response_json(chat).await;
                assert_eq!(chat_body["data"]["check_result"]["status"], "success");
                assert!(!chat_body.to_string().contains(chat_secret));
                assert_eq!(chat_body["data"]["created_model"]["model_kind"], "CHAT");

                let upstream_request = timeout(Duration::from_secs(2), upstream)
                    .await
                    .expect("CHAT bootstrap should reach upstream")
                    .expect("upstream fixture should finish");
                assert!(upstream_request.contains("bootstrap-chat-model"));

                let embedding_secret = "bootstrap-embedding-secret-must-not-return";
                let embedding = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider/bootstrap",
                        json!({
                            "initial_source": {
                                "profile_type": "OPENAI",
                                "base_url": format!("http://{address}/v1"),
                                "use_proxy": false,
                                "is_enabled": true,
                                "is_default": true
                            },
                            "api_key": embedding_secret,
                            "model_name": "bootstrap-embedding-model",
                            "model_kind": "EMBEDDING",
                            "key": "bootstrap-embedding-provider",
                            "save_and_test": true
                        }),
                    ),
                )
                .await;
                assert_eq!(embedding.status(), StatusCode::OK);
                let embedding_body = response_json(embedding).await;
                assert_eq!(
                    embedding_body["data"]["check_result"]["status"],
                    "check_skipped"
                );
                assert_eq!(
                    embedding_body["data"]["created_model"]["model_kind"],
                    "EMBEDDING"
                );
                assert!(!embedding_body.to_string().contains(embedding_secret));
                assert_eq!(
                    Provider::list_all()
                        .expect("both bootstrap providers should persist")
                        .len(),
                    2
                );
            })
            .await;
    }

    #[tokio::test]
    async fn gemini_bootstrap_save_and_test_uses_validated_native_probe_without_proxy_logs() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-gemini-bootstrap-check-http.sqlite");
        test_db_context
            .run_async(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let (request_tx, request_rx) = oneshot::channel();
                let upstream = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = vec![0u8; 8192];
                    let read = socket.read(&mut request).await.unwrap();
                    let _ = request_tx.send(String::from_utf8_lossy(&request[..read]).to_string());
                    let body = br#"{"responseId":"bootstrap-response","candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"ok"}]},"finishReason":"STOP"}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        String::from_utf8_lossy(body)
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let secret = "gemini-bootstrap-private-key";
                let response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/provider/bootstrap",
                        json!({
                            "initial_source":{
                                "profile_type":"GEMINI",
                                "base_url":format!("http://{address}/v1beta/models"),
                                "use_proxy":false,
                                "is_enabled":true,
                                "is_default":true
                            },
                            "api_key":secret,
                            "model_name":"gemini-bootstrap-model",
                            "model_kind":"CHAT",
                            "key":"gemini-bootstrap-provider",
                            "save_and_test":true
                        }),
                    ),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(body["data"]["check_result"]["status"], "success");
                assert!(!body.to_string().contains(secret));

                let request = request_rx.await.expect("one Gemini bootstrap check");
                let request_lower = request.to_ascii_lowercase();
                assert!(request_lower.starts_with(
                    "post /v1beta/models/gemini-bootstrap-model:generatecontent http/1.1"
                ));
                assert!(request_lower.contains("x-goog-api-key: gemini-bootstrap-private-key"));
                timeout(Duration::from_secs(2), upstream)
                    .await
                    .expect("Gemini bootstrap check should finish")
                    .expect("Gemini bootstrap fixture should join");
                assert!(RequestLog::list_full(RequestLogQueryPayload {
                    provider_id: body["data"]["provider"]["id"].as_i64(),
                    page: Some(1),
                    page_size: Some(10),
                    ..Default::default()
                })
                .expect("request logs should query")
                .list
                .is_empty());
            })
            .await;
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
                                "base_url": "  HTTPS://API.EXAMPLE.COM:443/v1///  ",
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
                    create_body["data"]["upstream_sources"][0]["base_url"],
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
                    provider.upstream_sources[0].base_url,
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
                    provider_cached.upstream_sources[0].base_url,
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
                                "base_url": "https://api.example.com/v1?tenant=one",
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
                    "upstream source base URL must not contain a query string"
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
                assert_eq!(source_body["data"]["base_url"], "https://api.openai.com/v1");
                assert_eq!(source_body["data"]["base_url_is_default"], true);
                assert_eq!(source_body["data"]["chat_completions_enabled"], true);
                assert_eq!(source_body["data"]["embeddings_enabled"], true);
                assert!(source_body["data"].get("rerank_enabled").is_none());
                let source_id = source_body["data"]["id"]
                    .as_i64()
                    .expect("source id should be returned");

                let immutable_profile_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/provider/{provider_id}/sources/{source_id}"),
                        json!({"profile_type": "OPENAI_COMPATIBLE"}),
                    ),
                )
                .await;
                assert_eq!(immutable_profile_response.status(), StatusCode::BAD_REQUEST);
                let immutable_profile_body = response_json(immutable_profile_response).await;
                assert_eq!(immutable_profile_body["code"], 1001);
                assert_eq!(
                    immutable_profile_body["msg"],
                    "upstream source profile_type is immutable after creation"
                );

                let default_only_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/provider/{provider_id}/sources/{source_id}"),
                        json!({
                            "base_url": "https://api.example.com/v1",
                            "use_proxy": false,
                            "is_enabled": true,
                            "is_default": true
                        }),
                    ),
                )
                .await;
                assert_eq!(default_only_response.status(), StatusCode::OK);

                let update_response = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("/provider/{provider_id}/sources/{source_id}"),
                        json!({
                            "base_url": "https://api.example.com/v1/updated",
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
                    update_body["data"]["base_url"],
                    "https://api.example.com/v1/updated"
                );
                assert_eq!(update_body["data"]["is_default"], true);

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

                let missing_compatible_base_url = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{second_provider_id}/sources"),
                        json!({
                            "profile_type": "OPENAI_COMPATIBLE",
                            "rerank_enabled": true,
                            "is_enabled": true,
                            "is_default": true
                        }),
                    ),
                )
                .await;
                assert_eq!(
                    missing_compatible_base_url.status(),
                    StatusCode::UNPROCESSABLE_ENTITY
                );

                let compatible_source = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{second_provider_id}/sources"),
                        json!({
                            "profile_type": "OPENAI_COMPATIBLE",
                            "base_url": "https://compat.example.test/api/",
                            "rerank_enabled": true,
                            "rerank_path_override": "rerank/v2/",
                            "is_enabled": true,
                            "is_default": true
                        }),
                    ),
                )
                .await;
                assert_eq!(compatible_source.status(), StatusCode::OK);
                let compatible_body = response_json(compatible_source).await;
                assert_eq!(
                    compatible_body["data"]["base_url"],
                    "https://compat.example.test/api"
                );
                assert_eq!(compatible_body["data"]["base_url_is_default"], false);
                assert_eq!(compatible_body["data"]["rerank_enabled"], true);
                assert_eq!(compatible_body["data"]["rerank_path_override"], "rerank/v2");

                let native_operation_field = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("/provider/{second_provider_id}/sources"),
                        json!({
                            "profile_type": "RESPONSES",
                            "base_url": "https://responses.example.test/v1",
                            "embeddings_enabled": true
                        }),
                    ),
                )
                .await;
                assert_eq!(
                    native_operation_field.status(),
                    StatusCode::UNPROCESSABLE_ENTITY
                );

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
                model_kind: crate::schema::enum_def::ModelKind::Chat,
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
        let profile_values = document["components"]["schemas"]["UpstreamProfileType"]["enum"]
            .as_sequence()
            .expect("upstream Profile enum should exist");
        assert_eq!(profile_values.len(), 7);
        assert!(profile_values.contains(&serde_yaml::Value::from("OPENAI_COMPATIBLE")));
        assert!(!profile_values.contains(&serde_yaml::Value::from("VERTEX_OPENAI")));

        let provider_check = &document["components"]["schemas"]["ProviderCheck"];
        assert_eq!(
            provider_check["oneOf"]
                .as_sequence()
                .expect("Provider Check must distinguish saved and draft models")
                .len(),
            2
        );
        let draft_check = &document["components"]["schemas"]["ProviderCheckDraftModelValue"];
        assert_eq!(
            draft_check["properties"]["model_kind"]["const"].as_str(),
            Some("CHAT")
        );
        assert!(
            draft_check["required"]
                .as_sequence()
                .expect("draft check required fields should exist")
                .contains(&serde_yaml::Value::from("upstream_model_name"))
        );
        let bootstrap_check = &document["components"]["schemas"]["BootstrapCheckResult"];
        assert!(
            bootstrap_check["properties"]["status"]["enum"]
                .as_sequence()
                .expect("bootstrap check statuses should exist")
                .contains(&serde_yaml::Value::from("check_skipped"))
        );

        for schema in ["UpstreamSourceInput", "UpstreamSource"] {
            assert_eq!(
                document["components"]["schemas"][schema]["oneOf"]
                    .as_sequence()
                    .expect("Source contract must be a tagged oneOf")
                    .len(),
                4
            );
            assert_eq!(
                document["components"]["schemas"][schema]["discriminator"]["propertyName"].as_str(),
                Some("profile_type")
            );
        }
        let openai_input = &document["components"]["schemas"]["OpenAiSourceInput"]["allOf"][1];
        assert!(openai_input["properties"]["base_url"].is_mapping());
        assert!(
            !openai_input["required"]
                .as_sequence()
                .expect("OPENAI required fields should exist")
                .contains(&serde_yaml::Value::from("base_url"))
        );
        for schema in ["OpenAiCompatibleSourceInput", "NativeSourceInput"] {
            assert!(
                document["components"]["schemas"][schema]["allOf"][1]["required"]
                    .as_sequence()
                    .expect("required fields should exist")
                    .contains(&serde_yaml::Value::from("base_url"))
            );
        }
        let source_common = document["components"]["schemas"]["SourceCommon"]["properties"]
            .as_mapping()
            .expect("Source common properties should exist");
        for field in [
            "base_url",
            "base_url_is_default",
            "use_proxy",
            "is_enabled",
            "is_default",
        ] {
            assert!(source_common.contains_key(serde_yaml::Value::from(field)));
        }
        for forbidden in ["endpoint", "source_key"] {
            assert!(!source_common.contains_key(serde_yaml::Value::from(forbidden)));
        }
        let compatible_properties =
            document["components"]["schemas"]["OpenAiCompatibleSource"]["allOf"][1]["properties"]
                .as_mapping()
                .expect("compatible Source properties should exist");
        for field in [
            "chat_completions_enabled",
            "chat_completions_path_override",
            "embeddings_enabled",
            "embeddings_path_override",
            "rerank_enabled",
            "rerank_path_override",
        ] {
            assert!(compatible_properties.contains_key(serde_yaml::Value::from(field)));
        }
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
        assert_eq!(
            document["components"]["schemas"]["ModelKind"]["enum"]
                .as_sequence()
                .expect("Model kind enum should exist"),
            &vec![
                serde_yaml::Value::from("CHAT"),
                serde_yaml::Value::from("EMBEDDING"),
                serde_yaml::Value::from("RERANK"),
            ]
        );
        for schema in ["ModelCreate", "ProviderBootstrap"] {
            assert!(
                document["components"]["schemas"][schema]["required"]
                    .as_sequence()
                    .expect("create required fields should exist")
                    .contains(&serde_yaml::Value::from("model_kind"))
            );
        }
        assert!(model_properties.contains_key(serde_yaml::Value::from("model_kind")));
        for schema in ["SourceEvidence"] {
            let properties = document["components"]["schemas"][schema]["properties"]
                .as_mapping()
                .expect("Source evidence properties should exist");
            for field in ["source_id", "profile_type", "provider_api_key_id"] {
                assert!(properties.contains_key(serde_yaml::Value::from(field)));
            }
            let key_id = &properties["provider_api_key_id"];
            assert_eq!(
                key_id["type"].as_sequence(),
                Some(&vec![
                    serde_yaml::Value::from("integer"),
                    serde_yaml::Value::from("null"),
                ])
            );
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
                        base_url: "https://impact-openai.example.com/v1".to_string(),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                        ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                            UpstreamProfileType::Openai,
                        )
                    },
                )
                .expect("provider seed should succeed")
                .provider;
                UpstreamSource::create(&crate::database::upstream_source::NewUpstreamSource {
                    id: responses_source_id,
                    provider_id,
                    profile_type: UpstreamProfileType::Responses,
                    base_url: "https://impact-responses.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: false,
                    created_at: 1,
                    updated_at: 1,
                    ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                        UpstreamProfileType::Responses,
                    )
                })
                .expect("secondary source seed should succeed");

                let inherit_model = Model::create(
                    provider.id,
                    "impact-inherit",
                    None,
                    crate::schema::enum_def::ModelKind::Chat,
                    true,
                )
                .expect("inherit model should seed");
                let disabled_inherit_model = Model::create(
                    provider.id,
                    "impact-disabled-inherit",
                    None,
                    crate::schema::enum_def::ModelKind::Chat,
                    false,
                )
                .expect("disabled inherit model should seed");
                let explicit_default_model = Model::create_with_source_config(
                    provider.id,
                    "impact-explicit-default",
                    None,
                    crate::schema::enum_def::ModelKind::Chat,
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
                    crate::schema::enum_def::ModelKind::Chat,
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
                    crate::schema::enum_def::ModelKind::Chat,
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
                profile_type: profile_type.clone(),
                base_url: endpoint.to_string(),
                use_proxy: false,
                is_enabled: true,
                is_default: true,
                deleted_at: None,
                created_at: 0,
                updated_at: 0,
                ..UpstreamSource::test_defaults(profile_type)
            }],
        }
    }
}
