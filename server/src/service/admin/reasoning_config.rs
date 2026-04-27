use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;

use crate::controller::BaseError;
use crate::database::model::Model;
use crate::database::provider::Provider;
use crate::database::reasoning_config::{
    ReasoningConfig, ReasoningConfigMode, ReasoningConfigPresetInput, ReasoningConfigScope,
    ReasoningConfigWithPresets, ReasoningPatchFamily, ReasoningPreset,
};
use crate::proxy::reasoning_suffix::{
    ReasoningGeneratedPatchPreview as ProxyReasoningGeneratedPatchPreview, ReasoningPatchContext,
    ReasoningPresetPatchPreview as ProxyReasoningPresetPatchPreview, ReasoningPresetPreviewInput,
    preview_reasoning_patches, target_api_type_for_provider_type,
    target_api_types_for_reasoning_family,
};
use crate::schema::enum_def::{LlmApiType, ProviderType};
use crate::service::cache::types::CacheModel;

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningPresetCatalogItem {
    pub preset_key: String,
    pub suffix: String,
    pub requires_reasoning: bool,
    pub allowed_operation_kinds: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningFamilyCatalogItem {
    pub family_key: String,
    pub supported_presets: Vec<String>,
    pub target_api_types: Vec<LlmApiType>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningConfigCatalog {
    pub families: Vec<ReasoningFamilyCatalogItem>,
    pub presets: Vec<ReasoningPresetCatalogItem>,
}

#[derive(Debug, Clone)]
pub struct ReasoningConfigPresetAdminInput {
    pub preset_key: String,
    pub expose_in_models: bool,
    pub is_enabled: bool,
}

impl From<ReasoningConfigPresetAdminInput> for ReasoningConfigPresetInput {
    fn from(input: ReasoningConfigPresetAdminInput) -> Self {
        Self {
            preset_key: input.preset_key,
            expose_in_models: input.expose_in_models,
            is_enabled: input.is_enabled,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpsertProviderReasoningConfigInput {
    pub family_key: String,
    pub presets: Vec<ReasoningConfigPresetAdminInput>,
}

#[derive(Debug, Clone)]
pub struct PreviewProviderReasoningConfigInput {
    pub provider_type: Option<ProviderType>,
    pub family_key: Option<String>,
    pub presets: Vec<ReasoningConfigPresetAdminInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelReasoningConfigWriteMode {
    Inherit,
    Disabled,
    Custom,
}

#[derive(Debug, Clone)]
pub struct UpsertModelReasoningConfigInput {
    pub mode: ModelReasoningConfigWriteMode,
    pub family_key: Option<String>,
    pub presets: Vec<ReasoningConfigPresetAdminInput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningConfigPresetAdminView {
    pub id: i64,
    pub config_id: i64,
    pub preset_key: String,
    pub suffix: String,
    pub requires_reasoning: bool,
    pub allowed_operation_kinds: Vec<String>,
    pub expose_in_models: bool,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningConfigAdminView {
    pub id: i64,
    pub scope_kind: ReasoningConfigScope,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub mode: ReasoningConfigMode,
    pub family_key: Option<String>,
    pub presets: Vec<ReasoningConfigPresetAdminView>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningConfigEffectiveSource {
    ProviderDefault,
    ModelCustom,
    ModelDisabled,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningConfigOwnerStatus {
    Custom,
    Disabled,
    Inherited,
    Missing,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningConfigAdminResponse {
    pub owner_kind: ReasoningConfigScope,
    pub owner_id: i64,
    pub owner_config: Option<ReasoningConfigAdminView>,
    pub provider_config: Option<ReasoningConfigAdminView>,
    pub effective_config: Option<ReasoningConfigAdminView>,
    pub effective_source: ReasoningConfigEffectiveSource,
    pub status: ReasoningConfigOwnerStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningConfigPreviewResponse {
    pub config: ReasoningConfigAdminResponse,
    pub target_api_type: LlmApiType,
    pub presets: Vec<ReasoningConfigPresetPreview>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningGeneratedPatchPreview {
    pub placement: String,
    pub target: String,
    pub operation: String,
    pub value_json: Option<serde_json::Value>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningConfigPresetPreview {
    pub preset_key: String,
    pub suffix: String,
    pub requires_reasoning: bool,
    pub allowed_operation_kinds: Vec<String>,
    pub family_supported: bool,
    pub enabled: bool,
    pub expose_in_models: bool,
    pub runtime_supported: bool,
    pub unsupported_reason: Option<String>,
    pub generated_patches: Vec<ReasoningGeneratedPatchPreview>,
}

pub struct ReasoningConfigAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl ReasoningConfigAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub fn catalog(&self) -> ReasoningConfigCatalog {
        ReasoningConfigCatalog {
            families: ReasoningPatchFamily::ALL
                .into_iter()
                .map(|family| ReasoningFamilyCatalogItem {
                    family_key: family.as_key().to_string(),
                    supported_presets: family
                        .supported_presets()
                        .into_iter()
                        .map(|preset| preset.as_key().to_string())
                        .collect(),
                    target_api_types: target_api_types_for_reasoning_family(family).to_vec(),
                })
                .collect(),
            presets: ReasoningPreset::ALL
                .into_iter()
                .map(|preset| {
                    let metadata = preset.metadata();
                    ReasoningPresetCatalogItem {
                        preset_key: metadata.preset_key,
                        suffix: metadata.suffix,
                        requires_reasoning: metadata.requires_reasoning,
                        allowed_operation_kinds: metadata.allowed_operation_kinds,
                    }
                })
                .collect(),
        }
    }

    pub fn get_provider_config(
        &self,
        provider_id: i64,
    ) -> Result<ReasoningConfigAdminResponse, BaseError> {
        ensure_provider(provider_id)?;
        provider_config_response(provider_id)
    }

    pub async fn upsert_provider_config(
        &self,
        provider_id: i64,
        input: UpsertProviderReasoningConfigInput,
    ) -> Result<ReasoningConfigAdminResponse, BaseError> {
        ensure_provider(provider_id)?;
        let presets = normalize_preset_inputs(input.presets);
        let config = ReasoningConfig::upsert_provider_config(
            provider_id,
            input.family_key.as_str(),
            &presets,
        )?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ReasoningProviderConfig { provider_id },
            ),
            AdminMutationEffect::audit(reasoning_config_audit_event(
                "provider_upserted",
                ReasoningConfigScope::Provider,
                provider_id,
                Some(&config),
            )),
        ])
        .await;

        provider_config_response(provider_id)
    }

    pub async fn delete_provider_config(&self, provider_id: i64) -> Result<(), BaseError> {
        ensure_provider(provider_id)?;
        let before = ReasoningConfig::get_active_provider_config(provider_id)?;
        ReasoningConfig::delete_provider_config(provider_id)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ReasoningProviderConfig { provider_id },
            ),
            AdminMutationEffect::audit(reasoning_config_audit_event(
                "provider_deleted",
                ReasoningConfigScope::Provider,
                provider_id,
                before.as_ref(),
            )),
        ])
        .await;

        Ok(())
    }

    pub fn preview_provider_config(
        &self,
        provider_id: i64,
    ) -> Result<ReasoningConfigPreviewResponse, BaseError> {
        let provider = ensure_provider(provider_id)?;
        let target_api_type = target_api_type_for_provider_type(&provider.provider_type);
        let config = provider_config_response(provider_id)?;
        Ok(build_preview_response(
            config,
            target_api_type,
            ReasoningPatchContext {
                target_api_type,
                model_id: None,
                model_name: None,
                supports_reasoning: true,
            },
        ))
    }

    pub fn preview_provider_config_draft(
        &self,
        provider_id: i64,
        input: PreviewProviderReasoningConfigInput,
    ) -> Result<ReasoningConfigPreviewResponse, BaseError> {
        let provider = ensure_provider(provider_id)?;
        let target_api_type = target_api_type_for_provider_type(
            input
                .provider_type
                .as_ref()
                .unwrap_or(&provider.provider_type),
        );
        let config = provider_draft_config_response(provider_id, input)?;
        Ok(build_preview_response(
            config,
            target_api_type,
            ReasoningPatchContext {
                target_api_type,
                model_id: None,
                model_name: None,
                supports_reasoning: true,
            },
        ))
    }

    pub fn get_model_config(
        &self,
        model_id: i64,
    ) -> Result<ReasoningConfigAdminResponse, BaseError> {
        ensure_model(model_id)?;
        model_config_response(model_id)
    }

    pub async fn upsert_model_config(
        &self,
        model_id: i64,
        input: UpsertModelReasoningConfigInput,
    ) -> Result<ReasoningConfigAdminResponse, BaseError> {
        ensure_model(model_id)?;

        match input.mode {
            ModelReasoningConfigWriteMode::Inherit => {
                if input.family_key.is_some() || !input.presets.is_empty() {
                    return Err(BaseError::ParamInvalid(Some(
                        "inherit model reasoning config must not include family_key or presets"
                            .to_string(),
                    )));
                }
                self.delete_model_config_internal(model_id).await?;
            }
            ModelReasoningConfigWriteMode::Disabled => {
                if input.family_key.is_some() {
                    return Err(BaseError::ParamInvalid(Some(
                        "disabled model reasoning config must not include family_key".to_string(),
                    )));
                }
                if !input.presets.is_empty() {
                    return Err(BaseError::ParamInvalid(Some(
                        "disabled model reasoning config must not include presets".to_string(),
                    )));
                }
                let config = ReasoningConfig::upsert_model_config(
                    model_id,
                    ReasoningConfigMode::Disabled,
                    None,
                    &[],
                )?;
                self.run_post_commit_effects(vec![
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ReasoningModelConfig { model_id },
                    ),
                    AdminMutationEffect::audit(reasoning_config_audit_event(
                        "model_upserted",
                        ReasoningConfigScope::Model,
                        model_id,
                        Some(&config),
                    )),
                ])
                .await;
            }
            ModelReasoningConfigWriteMode::Custom => {
                let family_key = input.family_key.as_deref().ok_or_else(|| {
                    BaseError::ParamInvalid(Some(
                        "custom model reasoning config requires family_key".to_string(),
                    ))
                })?;
                let presets = normalize_preset_inputs(input.presets);
                let config = ReasoningConfig::upsert_model_config(
                    model_id,
                    ReasoningConfigMode::Custom,
                    Some(family_key),
                    &presets,
                )?;
                self.run_post_commit_effects(vec![
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ReasoningModelConfig { model_id },
                    ),
                    AdminMutationEffect::audit(reasoning_config_audit_event(
                        "model_upserted",
                        ReasoningConfigScope::Model,
                        model_id,
                        Some(&config),
                    )),
                ])
                .await;
            }
        }

        model_config_response(model_id)
    }

    pub async fn delete_model_config(&self, model_id: i64) -> Result<(), BaseError> {
        ensure_model(model_id)?;
        self.delete_model_config_internal(model_id).await
    }

    pub fn preview_model_config(
        &self,
        model_id: i64,
    ) -> Result<ReasoningConfigPreviewResponse, BaseError> {
        let model = ensure_model(model_id)?;
        let provider = ensure_provider(model.provider_id)?;
        let target_api_type = target_api_type_for_provider_type(&provider.provider_type);
        let cache_model = CacheModel::from(model);
        let config = model_config_response(model_id)?;
        Ok(build_preview_response(
            config,
            target_api_type,
            ReasoningPatchContext::for_model(target_api_type, &cache_model),
        ))
    }

    pub fn preview_model_config_draft(
        &self,
        model_id: i64,
        input: UpsertModelReasoningConfigInput,
    ) -> Result<ReasoningConfigPreviewResponse, BaseError> {
        let model = ensure_model(model_id)?;
        let provider = ensure_provider(model.provider_id)?;
        let target_api_type = target_api_type_for_provider_type(&provider.provider_type);
        let cache_model = CacheModel::from(model);
        let config = model_draft_config_response(&cache_model, input)?;
        Ok(build_preview_response(
            config,
            target_api_type,
            ReasoningPatchContext::for_model(target_api_type, &cache_model),
        ))
    }

    async fn delete_model_config_internal(&self, model_id: i64) -> Result<(), BaseError> {
        let before = ReasoningConfig::get_active_model_config(model_id)?;
        ReasoningConfig::delete_model_config(model_id)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ReasoningModelConfig { model_id },
            ),
            AdminMutationEffect::audit(reasoning_config_audit_event(
                "model_deleted",
                ReasoningConfigScope::Model,
                model_id,
                before.as_ref(),
            )),
        ])
        .await;

        Ok(())
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn normalize_preset_inputs(
    presets: Vec<ReasoningConfigPresetAdminInput>,
) -> Vec<ReasoningConfigPresetInput> {
    presets.into_iter().map(Into::into).collect()
}

fn ensure_provider(provider_id: i64) -> Result<Provider, BaseError> {
    Provider::get_by_id(provider_id)
        .map_err(|err| map_owner_not_found(err, "provider", provider_id))
}

fn ensure_model(model_id: i64) -> Result<Model, BaseError> {
    Model::get_by_id(model_id).map_err(|err| map_owner_not_found(err, "model", model_id))
}

fn map_owner_not_found(err: BaseError, owner_kind: &'static str, owner_id: i64) -> BaseError {
    match err {
        BaseError::ParamInvalid(_) => {
            BaseError::NotFound(Some(format!("{owner_kind} {owner_id} not found")))
        }
        other => other,
    }
}

fn provider_config_response(provider_id: i64) -> Result<ReasoningConfigAdminResponse, BaseError> {
    let owner_config = ReasoningConfig::get_active_provider_config(provider_id)?.map(config_view);
    let (effective_source, status, effective_config) = match owner_config.clone() {
        Some(config) => (
            ReasoningConfigEffectiveSource::ProviderDefault,
            ReasoningConfigOwnerStatus::Custom,
            Some(config),
        ),
        None => (
            ReasoningConfigEffectiveSource::Missing,
            ReasoningConfigOwnerStatus::Missing,
            None,
        ),
    };

    Ok(ReasoningConfigAdminResponse {
        owner_kind: ReasoningConfigScope::Provider,
        owner_id: provider_id,
        owner_config,
        provider_config: None,
        effective_config,
        effective_source,
        status,
    })
}

fn model_config_response(model_id: i64) -> Result<ReasoningConfigAdminResponse, BaseError> {
    let model = ensure_model(model_id)?;
    let _provider = ensure_provider(model.provider_id)?;
    let owner_config = ReasoningConfig::get_active_model_config(model_id)?.map(config_view);
    let provider_config =
        ReasoningConfig::get_active_provider_config(model.provider_id)?.map(config_view);

    let (effective_source, status, effective_config) = match owner_config.clone() {
        Some(config) if matches!(config.mode, ReasoningConfigMode::Disabled) => (
            ReasoningConfigEffectiveSource::ModelDisabled,
            ReasoningConfigOwnerStatus::Disabled,
            Some(config),
        ),
        Some(config) => (
            ReasoningConfigEffectiveSource::ModelCustom,
            ReasoningConfigOwnerStatus::Custom,
            Some(config),
        ),
        None => match provider_config.clone() {
            Some(config) => (
                ReasoningConfigEffectiveSource::ProviderDefault,
                ReasoningConfigOwnerStatus::Inherited,
                Some(config),
            ),
            None => (
                ReasoningConfigEffectiveSource::Missing,
                ReasoningConfigOwnerStatus::Missing,
                None,
            ),
        },
    };

    Ok(ReasoningConfigAdminResponse {
        owner_kind: ReasoningConfigScope::Model,
        owner_id: model_id,
        owner_config,
        provider_config,
        effective_config,
        effective_source,
        status,
    })
}

fn provider_draft_config_response(
    provider_id: i64,
    input: PreviewProviderReasoningConfigInput,
) -> Result<ReasoningConfigAdminResponse, BaseError> {
    let owner_config = match normalize_optional_family_key(input.family_key) {
        Some(family_key) => Some(draft_custom_config_view(
            ReasoningConfigScope::Provider,
            provider_id,
            family_key,
            input.presets,
        )?),
        None => None,
    };

    let (effective_source, status, effective_config) = match owner_config.clone() {
        Some(config) => (
            ReasoningConfigEffectiveSource::ProviderDefault,
            ReasoningConfigOwnerStatus::Custom,
            Some(config),
        ),
        None => (
            ReasoningConfigEffectiveSource::Missing,
            ReasoningConfigOwnerStatus::Missing,
            None,
        ),
    };

    Ok(ReasoningConfigAdminResponse {
        owner_kind: ReasoningConfigScope::Provider,
        owner_id: provider_id,
        owner_config,
        provider_config: None,
        effective_config,
        effective_source,
        status,
    })
}

fn model_draft_config_response(
    model: &CacheModel,
    input: UpsertModelReasoningConfigInput,
) -> Result<ReasoningConfigAdminResponse, BaseError> {
    let provider_config =
        ReasoningConfig::get_active_provider_config(model.provider_id)?.map(config_view);

    let (owner_config, effective_source, status, effective_config) = match input.mode {
        ModelReasoningConfigWriteMode::Inherit => {
            if input.family_key.is_some() || !input.presets.is_empty() {
                return Err(BaseError::ParamInvalid(Some(
                    "inherit model reasoning config preview must not include family_key or presets"
                        .to_string(),
                )));
            }
            match provider_config.clone() {
                Some(config) => (
                    None,
                    ReasoningConfigEffectiveSource::ProviderDefault,
                    ReasoningConfigOwnerStatus::Inherited,
                    Some(config),
                ),
                None => (
                    None,
                    ReasoningConfigEffectiveSource::Missing,
                    ReasoningConfigOwnerStatus::Missing,
                    None,
                ),
            }
        }
        ModelReasoningConfigWriteMode::Disabled => {
            if input.family_key.is_some() {
                return Err(BaseError::ParamInvalid(Some(
                    "disabled model reasoning config preview must not include family_key"
                        .to_string(),
                )));
            }
            if !input.presets.is_empty() {
                return Err(BaseError::ParamInvalid(Some(
                    "disabled model reasoning config preview must not include presets".to_string(),
                )));
            }
            let config = draft_disabled_config_view(ReasoningConfigScope::Model, model.id);
            (
                Some(config.clone()),
                ReasoningConfigEffectiveSource::ModelDisabled,
                ReasoningConfigOwnerStatus::Disabled,
                Some(config),
            )
        }
        ModelReasoningConfigWriteMode::Custom => {
            let family_key = normalize_optional_family_key(input.family_key).ok_or_else(|| {
                BaseError::ParamInvalid(Some(
                    "custom model reasoning config preview requires family_key".to_string(),
                ))
            })?;
            let config = draft_custom_config_view(
                ReasoningConfigScope::Model,
                model.id,
                family_key,
                input.presets,
            )?;
            (
                Some(config.clone()),
                ReasoningConfigEffectiveSource::ModelCustom,
                ReasoningConfigOwnerStatus::Custom,
                Some(config),
            )
        }
    };

    Ok(ReasoningConfigAdminResponse {
        owner_kind: ReasoningConfigScope::Model,
        owner_id: model.id,
        owner_config,
        provider_config,
        effective_config,
        effective_source,
        status,
    })
}

fn normalize_optional_family_key(family_key: Option<String>) -> Option<String> {
    family_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn draft_disabled_config_view(
    scope: ReasoningConfigScope,
    owner_id: i64,
) -> ReasoningConfigAdminView {
    draft_config_view(
        scope,
        owner_id,
        ReasoningConfigMode::Disabled,
        None,
        Vec::new(),
    )
}

fn draft_custom_config_view(
    scope: ReasoningConfigScope,
    owner_id: i64,
    family_key: String,
    presets: Vec<ReasoningConfigPresetAdminInput>,
) -> Result<ReasoningConfigAdminView, BaseError> {
    let family = family_key
        .parse::<ReasoningPatchFamily>()
        .map_err(|err| BaseError::ParamInvalid(Some(err)))?;
    let mut seen = HashSet::new();
    let mut rows = Vec::with_capacity(presets.len());

    for (index, input) in presets.into_iter().enumerate() {
        let preset = input
            .preset_key
            .parse::<ReasoningPreset>()
            .map_err(|err| BaseError::ParamInvalid(Some(err)))?;
        if !seen.insert(preset) {
            return Err(BaseError::ParamInvalid(Some(format!(
                "duplicate reasoning preset '{}'",
                preset.as_key()
            ))));
        }
        if let Some(reason) = family.unsupported_preset_reason(preset) {
            return Err(BaseError::ParamInvalid(Some(format!(
                "reasoning family '{}' does not support preset '{}': {}",
                family.as_key(),
                preset.as_key(),
                reason
            ))));
        }
        let metadata = preset.metadata();
        rows.push(ReasoningConfigPresetAdminView {
            id: -((index as i64) + 1),
            config_id: 0,
            preset_key: metadata.preset_key,
            suffix: metadata.suffix,
            requires_reasoning: metadata.requires_reasoning,
            allowed_operation_kinds: metadata.allowed_operation_kinds,
            expose_in_models: input.expose_in_models,
            is_enabled: input.is_enabled,
            created_at: 0,
            updated_at: 0,
        });
    }

    Ok(draft_config_view(
        scope,
        owner_id,
        ReasoningConfigMode::Custom,
        Some(family.as_key().to_string()),
        rows,
    ))
}

fn draft_config_view(
    scope: ReasoningConfigScope,
    owner_id: i64,
    mode: ReasoningConfigMode,
    family_key: Option<String>,
    presets: Vec<ReasoningConfigPresetAdminView>,
) -> ReasoningConfigAdminView {
    ReasoningConfigAdminView {
        id: 0,
        scope_kind: scope,
        provider_id: matches!(scope, ReasoningConfigScope::Provider).then_some(owner_id),
        model_id: matches!(scope, ReasoningConfigScope::Model).then_some(owner_id),
        mode,
        family_key,
        presets,
        created_at: 0,
        updated_at: 0,
    }
}

fn config_view(config: ReasoningConfigWithPresets) -> ReasoningConfigAdminView {
    ReasoningConfigAdminView {
        id: config.config.id,
        scope_kind: config.scope,
        provider_id: config.config.provider_id,
        model_id: config.config.model_id,
        mode: config.mode,
        family_key: config.family.map(|family| family.as_key().to_string()),
        presets: config
            .presets
            .into_iter()
            .map(|preset| ReasoningConfigPresetAdminView {
                id: preset.preset.id,
                config_id: preset.preset.config_id,
                preset_key: preset.preset_key.as_key().to_string(),
                suffix: preset.suffix,
                requires_reasoning: preset.requires_reasoning,
                allowed_operation_kinds: preset.allowed_operation_kinds,
                expose_in_models: preset.preset.expose_in_models,
                is_enabled: preset.preset.is_enabled,
                created_at: preset.preset.created_at,
                updated_at: preset.preset.updated_at,
            })
            .collect(),
        created_at: config.config.created_at,
        updated_at: config.config.updated_at,
    }
}

fn build_preview_response(
    config: ReasoningConfigAdminResponse,
    target_api_type: LlmApiType,
    context: ReasoningPatchContext<'_>,
) -> ReasoningConfigPreviewResponse {
    let presets = match &config.effective_config {
        Some(effective_config) if matches!(effective_config.mode, ReasoningConfigMode::Custom) => {
            build_custom_preview_presets(effective_config, context)
        }
        Some(_) => build_unsupported_preview_presets("model reasoning config is disabled"),
        None => build_unsupported_preview_presets("reasoning config is missing"),
    };

    ReasoningConfigPreviewResponse {
        config,
        target_api_type,
        presets,
    }
}

fn build_custom_preview_presets(
    config: &ReasoningConfigAdminView,
    context: ReasoningPatchContext<'_>,
) -> Vec<ReasoningConfigPresetPreview> {
    let Some(family) = config
        .family_key
        .as_deref()
        .and_then(|value| value.parse::<ReasoningPatchFamily>().ok())
    else {
        return build_unsupported_preview_presets("reasoning family is missing");
    };

    let preset_inputs = config
        .presets
        .iter()
        .filter_map(|row| {
            row.preset_key
                .parse::<ReasoningPreset>()
                .ok()
                .map(|preset| ReasoningPresetPreviewInput {
                    preset,
                    enabled: row.is_enabled,
                    expose_in_models: row.expose_in_models,
                })
        })
        .collect::<Vec<_>>();

    preview_reasoning_patches(family, &preset_inputs, context)
        .into_iter()
        .map(ReasoningConfigPresetPreview::from)
        .collect()
}

fn build_unsupported_preview_presets(reason: &'static str) -> Vec<ReasoningConfigPresetPreview> {
    ReasoningPreset::ALL
        .into_iter()
        .map(|preset| {
            let metadata = preset.metadata();
            ReasoningConfigPresetPreview {
                preset_key: metadata.preset_key,
                suffix: metadata.suffix,
                requires_reasoning: metadata.requires_reasoning,
                allowed_operation_kinds: metadata.allowed_operation_kinds,
                family_supported: false,
                enabled: false,
                expose_in_models: false,
                runtime_supported: false,
                unsupported_reason: Some(reason.to_string()),
                generated_patches: Vec::new(),
            }
        })
        .collect()
}

impl From<ProxyReasoningGeneratedPatchPreview> for ReasoningGeneratedPatchPreview {
    fn from(value: ProxyReasoningGeneratedPatchPreview) -> Self {
        Self {
            placement: value.placement,
            target: value.target,
            operation: value.operation,
            value_json: value.value_json,
            description: value.description,
        }
    }
}

impl From<ProxyReasoningPresetPatchPreview> for ReasoningConfigPresetPreview {
    fn from(value: ProxyReasoningPresetPatchPreview) -> Self {
        Self {
            preset_key: value.preset_key,
            suffix: value.suffix,
            requires_reasoning: value.requires_reasoning,
            allowed_operation_kinds: value.allowed_operation_kinds,
            family_supported: value.family_supported,
            enabled: value.enabled,
            expose_in_models: value.expose_in_models,
            runtime_supported: value.runtime_supported,
            unsupported_reason: value.unsupported_reason,
            generated_patches: value
                .generated_patches
                .into_iter()
                .map(ReasoningGeneratedPatchPreview::from)
                .collect(),
        }
    }
}

fn reasoning_config_audit_event(
    action: &'static str,
    scope: ReasoningConfigScope,
    owner_id: i64,
    config: Option<&ReasoningConfigWithPresets>,
) -> AdminAuditEvent {
    let event_name = match action {
        "provider_upserted" => "manager.reasoning_config_provider_upserted",
        "provider_deleted" => "manager.reasoning_config_provider_deleted",
        "model_upserted" => "manager.reasoning_config_model_upserted",
        "model_deleted" => "manager.reasoning_config_model_deleted",
        _ => unreachable!("unsupported reasoning config audit action: {action}"),
    };

    let mode = config
        .map(|config| config.mode.as_key())
        .unwrap_or("missing");
    let family = config
        .and_then(|config| config.family.map(|family| family.as_key()))
        .unwrap_or("");
    let config_id = config.map(|config| config.config.id);

    let mut fields = vec![
        AdminAuditField::new("action", action),
        AdminAuditField::new("scope", scope.as_key()),
        AdminAuditField::new("owner_id", owner_id),
        AdminAuditField::new("mode", mode),
        AdminAuditField::new("family_key", family),
    ];
    fields.extend(AdminAuditField::optional("reasoning_config_id", config_id));

    AdminAuditEvent::with_fields(event_name, fields)
}

#[cfg(test)]
mod tests {
    use crate::database::TestDbContext;
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::reasoning_config::ReasoningConfig;
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::app_state::create_test_app_state;

    use super::{
        ModelReasoningConfigWriteMode, ReasoningConfigEffectiveSource, ReasoningConfigOwnerStatus,
        ReasoningConfigPresetAdminInput, UpsertModelReasoningConfigInput,
        UpsertProviderReasoningConfigInput,
    };

    fn seed_provider(id: i64, provider_key: &str) -> Provider {
        Provider::create(&NewProvider {
            id,
            provider_key: provider_key.to_string(),
            name: provider_key.to_string(),
            endpoint: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        })
        .expect("provider seed should succeed")
    }

    fn seed_model(provider_id: i64, model_name: &str) -> Model {
        Model::create(
            provider_id,
            model_name,
            None,
            true,
            ModelCapabilityFlags::default(),
        )
        .expect("model seed should succeed")
    }

    fn preset(preset_key: &str) -> ReasoningConfigPresetAdminInput {
        ReasoningConfigPresetAdminInput {
            preset_key: preset_key.to_string(),
            expose_in_models: true,
            is_enabled: true,
        }
    }

    #[tokio::test]
    async fn provider_reasoning_config_lifecycle_replaces_presets_and_refreshes_catalog() {
        let test_db_context = TestDbContext::new_sqlite("admin-reasoning-config-provider.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(40101, "provider-reasoning-config");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let cached_before = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should load before mutation");
                assert!(cached_before.reasoning_configs.is_empty());

                let created = app_state
                    .admin
                    .reasoning_config
                    .upsert_provider_config(
                        provider.id,
                        UpsertProviderReasoningConfigInput {
                            family_key: "openai_chat_reasoning_effort".to_string(),
                            presets: vec![preset("high")],
                        },
                    )
                    .await
                    .expect("provider config should create");
                assert_eq!(
                    created.effective_source,
                    ReasoningConfigEffectiveSource::ProviderDefault
                );
                assert_eq!(created.status, ReasoningConfigOwnerStatus::Custom);
                assert_eq!(
                    created.owner_config.as_ref().unwrap().presets[0].preset_key,
                    "high"
                );

                let cached_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload after invalidation");
                assert_eq!(cached_after.reasoning_configs.len(), 1);

                let replaced = app_state
                    .admin
                    .reasoning_config
                    .upsert_provider_config(
                        provider.id,
                        UpsertProviderReasoningConfigInput {
                            family_key: "openai_chat_reasoning_effort".to_string(),
                            presets: vec![preset("low")],
                        },
                    )
                    .await
                    .expect("provider config should replace");
                let presets = &replaced.owner_config.as_ref().unwrap().presets;
                assert_eq!(presets.len(), 1);
                assert_eq!(presets[0].preset_key, "low");

                app_state
                    .admin
                    .reasoning_config
                    .delete_provider_config(provider.id)
                    .await
                    .expect("provider config should delete");
                let deleted = app_state
                    .admin
                    .reasoning_config
                    .get_provider_config(provider.id)
                    .expect("provider config response should load");
                assert_eq!(
                    deleted.effective_source,
                    ReasoningConfigEffectiveSource::Missing
                );
                assert!(
                    ReasoningConfig::get_active_provider_config(provider.id)
                        .expect("provider config lookup should succeed")
                        .is_none()
                );
            })
            .await;
    }

    #[tokio::test]
    async fn model_reasoning_config_supports_inherit_disabled_and_custom() {
        let test_db_context = TestDbContext::new_sqlite("admin-reasoning-config-model.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(40201, "model-reasoning-provider");
                let model = seed_model(provider.id, "gpt-4o-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                app_state
                    .admin
                    .reasoning_config
                    .upsert_provider_config(
                        provider.id,
                        UpsertProviderReasoningConfigInput {
                            family_key: "openai_chat_reasoning_effort".to_string(),
                            presets: vec![preset("high")],
                        },
                    )
                    .await
                    .expect("provider config should create");

                let inherited = app_state
                    .admin
                    .reasoning_config
                    .get_model_config(model.id)
                    .expect("model config response should load");
                assert_eq!(
                    inherited.effective_source,
                    ReasoningConfigEffectiveSource::ProviderDefault
                );
                assert_eq!(inherited.status, ReasoningConfigOwnerStatus::Inherited);

                let disabled = app_state
                    .admin
                    .reasoning_config
                    .upsert_model_config(
                        model.id,
                        UpsertModelReasoningConfigInput {
                            mode: ModelReasoningConfigWriteMode::Disabled,
                            family_key: None,
                            presets: Vec::new(),
                        },
                    )
                    .await
                    .expect("model config should disable");
                assert_eq!(
                    disabled.effective_source,
                    ReasoningConfigEffectiveSource::ModelDisabled
                );
                assert_eq!(disabled.status, ReasoningConfigOwnerStatus::Disabled);
                assert!(disabled.effective_config.unwrap().presets.is_empty());

                let custom = app_state
                    .admin
                    .reasoning_config
                    .upsert_model_config(
                        model.id,
                        UpsertModelReasoningConfigInput {
                            mode: ModelReasoningConfigWriteMode::Custom,
                            family_key: Some("openai_chat_reasoning_effort".to_string()),
                            presets: vec![preset("low")],
                        },
                    )
                    .await
                    .expect("model config should customize");
                assert_eq!(
                    custom.effective_source,
                    ReasoningConfigEffectiveSource::ModelCustom
                );
                assert_eq!(
                    custom.owner_config.as_ref().unwrap().presets[0].preset_key,
                    "low"
                );

                let inherited_again = app_state
                    .admin
                    .reasoning_config
                    .upsert_model_config(
                        model.id,
                        UpsertModelReasoningConfigInput {
                            mode: ModelReasoningConfigWriteMode::Inherit,
                            family_key: None,
                            presets: Vec::new(),
                        },
                    )
                    .await
                    .expect("model config should return to inherit");
                assert_eq!(
                    inherited_again.effective_source,
                    ReasoningConfigEffectiveSource::ProviderDefault
                );
                assert!(inherited_again.owner_config.is_none());
            })
            .await;
    }
}
