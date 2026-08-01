use std::{collections::HashSet, sync::Arc};

use axum::{body::Body, response::Response};
use serde::Serialize;

use super::{
    ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility,
    auth::admit_api_key_request,
    request_context::ProxyRequestContext,
    runtime::{
        api_key_lease::ApiKeyRequestLeaseFinalizer,
        route_resolver::{
            ExecutionTarget, RuntimeFeatureConfigSource, TargetRuntimeFeatures,
            resolve_effective_reasoning_config, target_supports_reasoning_preset,
        },
    },
    util::determine_upstream_protocol,
};
use crate::{
    database::reasoning_config::{ReasoningConfigMode, ReasoningPreset},
    schema::enum_def::DownstreamProtocol,
    service::{
        app_state::AppState,
        cache::types::{
            CacheApiKey, CacheModel, CacheModelsCatalog, CacheProvider, CacheReasoningConfig,
        },
    },
    utils::acl::ACL_EVALUATOR,
};
use cyder_tools::log::{debug, error};

#[derive(Debug)]
pub(super) struct AccessibleModel {
    pub id: String,
    pub owned_by: String,
}

pub(super) async fn get_accessible_models(
    app_state: &Arc<AppState>,
    api_key: &CacheApiKey,
) -> Result<Vec<AccessibleModel>, ProxyError> {
    let catalog = app_state
        .catalog
        .get_models_catalog()
        .await
        .map_err(|store_err| {
            error!("Failed to fetch models catalog from cache: {:?}", store_err);
            ProxyError::gateway(
                ProxyErrorCode::ServerError,
                ExecutionStage::Capability,
                ResponseVisibility::NotVisible,
                None,
                format!("Failed to retrieve models catalog: {store_err:?}"),
            )
        })?;
    Ok(collect_accessible_models(catalog.as_ref(), api_key))
}

pub(super) async fn execute_models_listing(
    app_state: Arc<AppState>,
    api_key: Arc<CacheApiKey>,
    downstream_protocol: DownstreamProtocol,
    request_context: Arc<ProxyRequestContext>,
) -> Result<Response<Body>, ProxyError> {
    let request_lease = admit_api_key_request(&app_state, &api_key).await?;
    let mut request_lease = ApiKeyRequestLeaseFinalizer::new(
        &app_state,
        request_lease,
        request_context.request_id.clone(),
    );
    let result = async {
        let models = get_accessible_models(&app_state, &api_key).await?;
        let response_body = render_models_response(downstream_protocol, &models)?;
        Ok(Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(Body::from(response_body))
            .expect("models response is valid"))
    }
    .await;
    request_lease.release().await;
    result
}

#[derive(Serialize, Debug)]
pub(super) struct ModelListResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

#[derive(Serialize, Debug)]
pub(super) struct ModelInfo {
    pub id: String,
    pub object: String,
    pub owned_by: String,
}

#[derive(Serialize, Debug)]
pub(super) struct GeminiModelListResponse {
    pub models: Vec<GeminiModelInfo>,
}

#[derive(Serialize, Debug)]
pub(super) struct GeminiModelInfo {
    pub name: String,
}

fn render_models_response(
    downstream_protocol: DownstreamProtocol,
    accessible_models: &[AccessibleModel],
) -> Result<String, ProxyError> {
    match downstream_protocol {
        DownstreamProtocol::Gemini => serde_json::to_string(&GeminiModelListResponse {
            models: accessible_models
                .iter()
                .map(|model| GeminiModelInfo {
                    name: format!("models/{}", model.id),
                })
                .collect(),
        }),
        _ => serde_json::to_string(&ModelListResponse {
            object: "list".to_string(),
            data: accessible_models
                .iter()
                .map(|model| ModelInfo {
                    id: model.id.clone(),
                    object: "model".to_string(),
                    owned_by: model.owned_by.clone(),
                })
                .collect(),
        }),
    }
    .map_err(|err| {
        ProxyError::gateway(
            ProxyErrorCode::DownstreamSendError,
            ExecutionStage::DownstreamSend,
            ResponseVisibility::NotVisible,
            None,
            format!("Failed to serialize models list: {err}"),
        )
    })
}

fn collect_accessible_models(
    catalog: &CacheModelsCatalog,
    api_key: &CacheApiKey,
) -> Vec<AccessibleModel> {
    let mut result = Vec::new();
    let mut seen_ids = HashSet::new();

    for provider in catalog
        .providers
        .iter()
        .filter(|provider| provider.is_enabled)
    {
        let mut models = catalog
            .models
            .iter()
            .filter(|model| model.is_enabled && model.provider_id == provider.id)
            .collect::<Vec<_>>();
        models.sort_by(|left, right| left.model_name.cmp(&right.model_name));

        for model in models {
            if !is_model_allowed(api_key, provider, model) {
                continue;
            }
            push_model(
                &mut result,
                &mut seen_ids,
                format!("{}/{}", provider.provider_key, model.model_name),
                provider,
            );

            for preset in exposed_presets_for_model(catalog, provider, model) {
                let target = build_direct_reasoning_target(provider, model);
                if target_supports_reasoning_preset(catalog, &target, preset).is_ok() {
                    push_model(
                        &mut result,
                        &mut seen_ids,
                        format!(
                            "{}/{}-{}",
                            provider.provider_key,
                            model.model_name,
                            preset.canonical_suffix()
                        ),
                        provider,
                    );
                }
            }
        }
    }
    result
}

fn exposed_presets_for_model(
    catalog: &CacheModelsCatalog,
    provider: &CacheProvider,
    model: &CacheModel,
) -> Vec<ReasoningPreset> {
    let Some(config) = resolve_effective_reasoning_config(catalog, provider, model).config else {
        return Vec::new();
    };
    if !matches!(config.mode, ReasoningConfigMode::Custom) {
        return Vec::new();
    }
    exposed_presets_for_config(config)
}

fn exposed_presets_for_config(config: &CacheReasoningConfig) -> Vec<ReasoningPreset> {
    ReasoningPreset::ALL
        .into_iter()
        .filter(|preset| {
            config
                .presets
                .iter()
                .any(|row| row.preset == *preset && row.is_enabled && row.expose_in_models)
        })
        .collect()
}

fn build_direct_reasoning_target(provider: &CacheProvider, model: &CacheModel) -> ExecutionTarget {
    ExecutionTarget {
        provider: Arc::new(provider.clone()),
        model: Arc::new(model.clone()),
        upstream_protocol: determine_upstream_protocol(provider),
        reasoning_config_id: None,
        reasoning_config_scope: None,
        reasoning_config_source: None,
        reasoning_config_preset_id: None,
        reasoning_family: None,
        reasoning_preset: None,
        reasoning_suffix: None,
        runtime_features: TargetRuntimeFeatures {
            openai_reasoning_content_repair_enabled: false,
            openai_reasoning_content_repair_source: RuntimeFeatureConfigSource::DefaultFalse,
        },
    }
}

fn push_model(
    result: &mut Vec<AccessibleModel>,
    seen_ids: &mut HashSet<String>,
    id: String,
    provider: &CacheProvider,
) {
    if seen_ids.insert(id.clone()) {
        result.push(AccessibleModel {
            id,
            owned_by: provider.provider_key.clone(),
        });
    }
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
