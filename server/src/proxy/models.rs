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
                let supports_preset =
                    enabled_source_supports_reasoning_preset(catalog, provider, model, preset);
                if supports_preset {
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

fn enabled_source_supports_reasoning_preset(
    catalog: &CacheModelsCatalog,
    provider: &CacheProvider,
    model: &CacheModel,
    preset: ReasoningPreset,
) -> bool {
    provider
        .upstream_sources
        .iter()
        .filter(|source| source.is_enabled)
        .any(|source| {
            let target = build_direct_reasoning_target(provider, model, source);
            target_supports_reasoning_preset(catalog, &target, preset).is_ok()
        })
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

fn build_direct_reasoning_target(
    provider: &CacheProvider,
    model: &CacheModel,
    source: &crate::service::cache::types::CacheUpstreamSource,
) -> ExecutionTarget {
    ExecutionTarget {
        provider: Arc::new(provider.clone()),
        model: Arc::new(model.clone()),
        upstream_source: Arc::new(source.clone()),
        downstream_protocol: DownstreamProtocol::Openai,
        upstream_protocol: determine_upstream_protocol(source),
        selection_reason:
            crate::proxy::runtime::route_resolver::SourceSelectionReason::ProtocolMatch,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::reasoning_config::{ReasoningConfigMode, ReasoningPatchFamily};
    use crate::schema::enum_def::{Action, ProviderApiKeyMode, UpstreamProfileType};
    use crate::service::cache::types::{CacheReasoningConfigPreset, CacheUpstreamSource};

    fn catalog_with_source_enabled(source_enabled: bool) -> CacheModelsCatalog {
        CacheModelsCatalog {
            providers: vec![CacheProvider {
                id: 1,
                provider_key: "openai".to_string(),
                name: "OpenAI".to_string(),
                provider_api_key_mode: ProviderApiKeyMode::Queue,
                is_enabled: true,
                upstream_sources: vec![CacheUpstreamSource {
                    id: 2,
                    profile_type: UpstreamProfileType::Openai,
                    endpoint: "https://api.openai.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: source_enabled,
                    is_default: true,
                }],
            }],
            models: vec![CacheModel {
                id: 3,
                provider_id: 1,
                model_name: "gpt-4o".to_string(),
                real_model_name: None,
                cost_catalog_id: None,
                supports_streaming: true,
                supports_tools: true,
                supports_reasoning: true,
                supports_image_input: false,
                supports_embeddings: false,
                supports_rerank: false,
                is_enabled: true,
            }],
            reasoning_configs: vec![CacheReasoningConfig {
                id: 4,
                scope_kind: crate::database::reasoning_config::ReasoningConfigScope::Provider,
                provider_id: Some(1),
                model_id: None,
                mode: ReasoningConfigMode::Custom,
                family: Some(ReasoningPatchFamily::OpenAiChatReasoningEffort),
                presets: vec![CacheReasoningConfigPreset {
                    id: 5,
                    config_id: 4,
                    preset: ReasoningPreset::High,
                    suffix: "high".to_string(),
                    requires_reasoning: true,
                    allowed_operation_kinds: vec!["generation".to_string()],
                    expose_in_models: true,
                    is_enabled: true,
                }],
            }],
            runtime_feature_configs: vec![],
        }
    }

    fn allow_all_api_key() -> CacheApiKey {
        CacheApiKey {
            id: 10,
            api_key_hash: "hash".to_string(),
            key_prefix: "prefix".to_string(),
            key_last4: "last4".to_string(),
            name: "test".to_string(),
            description: None,
            default_action: Action::Allow,
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
            acl_rules: vec![],
        }
    }

    #[test]
    fn disabled_sources_do_not_advertise_reasoning_suffixes() {
        let disabled_catalog = catalog_with_source_enabled(false);
        let disabled_models = collect_accessible_models(&disabled_catalog, &allow_all_api_key());
        assert_eq!(
            disabled_models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["openai/gpt-4o"]
        );

        let enabled_catalog = catalog_with_source_enabled(true);
        let enabled_models = collect_accessible_models(&enabled_catalog, &allow_all_api_key());
        assert!(
            enabled_models
                .iter()
                .any(|model| model.id == "openai/gpt-4o-high")
        );
    }
}
