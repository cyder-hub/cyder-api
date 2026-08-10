use std::{collections::HashSet, sync::Arc};

use axum::{body::Body, response::Response};
use serde::Serialize;

use super::{
    ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, auth::admit_api_key_request,
    request_context::ProxyRequestContext,
};
use crate::{
    schema::enum_def::DownstreamProtocol,
    service::{
        app_state::AppState,
        cache::types::{CacheApiKey, CacheModel, CacheModelsCatalog, CacheProvider},
        request_patch::evaluate_request_patch_variants,
        source_selector::select_source,
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
    downstream_protocol: DownstreamProtocol,
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
    Ok(collect_accessible_models(
        catalog.as_ref(),
        api_key,
        downstream_protocol,
    ))
}

pub(super) async fn execute_models_listing(
    app_state: Arc<AppState>,
    api_key: Arc<CacheApiKey>,
    downstream_protocol: DownstreamProtocol,
    request_context: Arc<ProxyRequestContext>,
) -> Result<Response<Body>, ProxyError> {
    let request_lease = admit_api_key_request(&app_state, &api_key).await?;
    let mut request_lease = super::runtime::api_key_lease::ApiKeyRequestLeaseFinalizer::new(
        &app_state,
        request_lease,
        request_context.request_id.clone(),
    );
    let result = async {
        let models = get_accessible_models(&app_state, &api_key, downstream_protocol).await?;
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
    downstream_protocol: DownstreamProtocol,
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
            let Ok(selection) = select_source(provider, model, downstream_protocol) else {
                continue;
            };
            push_model(
                &mut result,
                &mut seen_ids,
                format!("{}/{}", provider.provider_key, model.model_name),
                provider,
            );

            let mut suffixes = catalog
                .request_patch_variants
                .iter()
                .filter(|variant| {
                    variant.enabled
                        && variant.source_id == selection.source.id
                        && (variant.model_id.is_some_and(|id| id == model.id)
                            || variant.model_id.is_none())
                })
                .filter_map(|variant| variant.suffix.clone())
                .collect::<Vec<_>>();
            suffixes
                .sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
            suffixes.dedup();
            for suffix in suffixes {
                let evaluation = evaluate_request_patch_variants(
                    &catalog.request_patch_variants,
                    selection.source.id,
                    Some(model.id),
                    Some(&suffix),
                );
                if evaluation.executable && evaluation.exposed_in_models {
                    push_model(
                        &mut result,
                        &mut seen_ids,
                        format!("{}/{}-{}", provider.provider_key, model.model_name, suffix),
                        provider,
                    );
                }
            }
        }
    }
    result
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
    use crate::schema::enum_def::{
        Action, ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement,
        UpstreamProfileType,
    };
    use crate::service::cache::types::{
        CacheRequestPatchRule, CacheRequestPatchVariant, CacheUpstreamSource,
    };

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

    fn catalog(source_enabled: bool, with_suffix: bool) -> CacheModelsCatalog {
        let variants = with_suffix.then(|| CacheRequestPatchVariant {
            id: 4,
            source_id: 2,
            model_id: None,
            suffix: Some("fast".to_string()),
            enabled: true,
            expose_in_models: true,
            rules: vec![CacheRequestPatchRule {
                id: 5,
                variant_id: 4,
                placement: RequestPatchPlacement::Body,
                target: "/temperature".to_string(),
                operation: RequestPatchOperation::Set,
                value_json: Some("0.2".to_string()),
                description: None,
                created_at: 1,
                updated_at: 1,
            }],
        });
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
                    endpoint: "https://example.test".to_string(),
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
                source_selection_mode: "INHERIT_ALL".to_string(),
                source_bindings: vec![],
                is_enabled: true,
            }],
            request_patch_variants: variants.into_iter().collect(),
        }
    }

    #[test]
    fn models_listing_uses_selected_source_and_exposure() {
        let disabled = collect_accessible_models(
            &catalog(false, true),
            &allow_all_api_key(),
            DownstreamProtocol::Openai,
        );
        assert!(disabled.is_empty());

        let enabled = collect_accessible_models(
            &catalog(true, true),
            &allow_all_api_key(),
            DownstreamProtocol::Openai,
        );
        assert!(enabled.iter().any(|model| model.id == "openai/gpt-4o"));
        assert!(enabled.iter().any(|model| model.id == "openai/gpt-4o-fast"));
    }

    #[test]
    fn models_listing_excludes_disabled_masked_and_hidden_suffix_aliases() {
        let mut disabled = catalog(true, true);
        disabled.request_patch_variants[0].enabled = false;
        assert!(
            collect_accessible_models(&disabled, &allow_all_api_key(), DownstreamProtocol::Openai,)
                .iter()
                .all(|model| model.id != "openai/gpt-4o-fast")
        );

        let mut masked = catalog(true, true);
        masked
            .request_patch_variants
            .push(CacheRequestPatchVariant {
                id: 6,
                source_id: 2,
                model_id: Some(3),
                suffix: Some("fast".to_string()),
                enabled: false,
                expose_in_models: false,
                rules: Vec::new(),
            });
        assert!(
            collect_accessible_models(&masked, &allow_all_api_key(), DownstreamProtocol::Openai)
                .iter()
                .all(|model| model.id != "openai/gpt-4o-fast")
        );

        let mut hidden = catalog(true, true);
        hidden.request_patch_variants[0].expose_in_models = false;
        assert!(
            collect_accessible_models(&hidden, &allow_all_api_key(), DownstreamProtocol::Openai)
                .iter()
                .all(|model| model.id != "openai/gpt-4o-fast")
        );
    }

    #[test]
    fn models_listing_applies_base_acl_once_to_base_and_suffix_aliases() {
        let mut denied = allow_all_api_key();
        denied.default_action = Action::Deny;
        let models =
            collect_accessible_models(&catalog(true, true), &denied, DownstreamProtocol::Openai);
        assert!(models.is_empty());
    }

    #[test]
    fn model_exposure_override_is_respected_without_disabling_explicit_resolution() {
        let mut catalog = catalog(true, true);
        catalog
            .request_patch_variants
            .push(CacheRequestPatchVariant {
                id: 7,
                source_id: 2,
                model_id: Some(3),
                suffix: Some("fast".to_string()),
                enabled: true,
                expose_in_models: false,
                rules: vec![CacheRequestPatchRule {
                    id: 8,
                    variant_id: 7,
                    placement: RequestPatchPlacement::Query,
                    target: "mode".to_string(),
                    operation: RequestPatchOperation::Set,
                    value_json: Some("\"fast\"".to_string()),
                    description: None,
                    created_at: 1,
                    updated_at: 1,
                }],
            });
        let models =
            collect_accessible_models(&catalog, &allow_all_api_key(), DownstreamProtocol::Openai);
        assert!(models.iter().any(|model| model.id == "openai/gpt-4o"));
        assert!(models.iter().all(|model| model.id != "openai/gpt-4o-fast"));
    }

    #[test]
    fn models_listing_uses_the_protocol_selected_source_for_suffix_exposure() {
        let mut catalog = catalog(true, true);
        catalog.providers[0]
            .upstream_sources
            .push(CacheUpstreamSource {
                id: 3,
                profile_type: UpstreamProfileType::Gemini,
                endpoint: "https://gemini.example".to_string(),
                use_proxy: false,
                is_enabled: true,
                is_default: false,
            });
        catalog
            .request_patch_variants
            .push(CacheRequestPatchVariant {
                id: 9,
                source_id: 3,
                model_id: None,
                suffix: Some("fast".to_string()),
                enabled: true,
                expose_in_models: true,
                rules: vec![CacheRequestPatchRule {
                    id: 10,
                    variant_id: 9,
                    placement: RequestPatchPlacement::Body,
                    target: "/generationConfig/temperature".to_string(),
                    operation: RequestPatchOperation::Set,
                    value_json: Some("0.2".to_string()),
                    description: None,
                    created_at: 1,
                    updated_at: 1,
                }],
            });

        let openai =
            collect_accessible_models(&catalog, &allow_all_api_key(), DownstreamProtocol::Openai);
        let gemini =
            collect_accessible_models(&catalog, &allow_all_api_key(), DownstreamProtocol::Gemini);
        assert!(openai.iter().any(|model| model.id == "openai/gpt-4o-fast"));
        assert!(gemini.iter().any(|model| model.id == "openai/gpt-4o-fast"));
        assert_eq!(openai.len(), gemini.len());
    }
}
