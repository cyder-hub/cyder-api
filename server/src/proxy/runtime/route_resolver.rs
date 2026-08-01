use std::{fmt, sync::Arc};

use crate::{
    database::reasoning_config::{
        ReasoningConfigMode, ReasoningConfigScope, ReasoningPatchFamily, ReasoningPreset,
    },
    database::runtime_feature_config::{RuntimeFeatureConfigScope, RuntimeFeatureKey},
    schema::enum_def::UpstreamProtocol,
    service::{
        app_state::AppState,
        cache::types::{CacheModel, CacheModelsCatalog, CacheProvider, CacheReasoningConfig},
    },
};
use cyder_tools::log::error;

use super::super::{
    reasoning_suffix::{ReasoningPatchContext, generate_reasoning_patches},
    requested_model::{
        RequestedModelParseStatus, ResolvedRequestedModelName, enabled_reasoning_suffixes,
        parse_reasoning_suffix,
    },
    util::determine_upstream_protocol,
};

#[derive(Debug, Clone)]
pub struct ExecutionTarget {
    pub provider: Arc<CacheProvider>,
    pub model: Arc<CacheModel>,
    pub upstream_protocol: UpstreamProtocol,
    pub reasoning_config_id: Option<i64>,
    pub reasoning_config_scope: Option<ReasoningConfigScope>,
    pub reasoning_config_source: Option<ReasoningConfigSource>,
    pub reasoning_config_preset_id: Option<i64>,
    pub reasoning_family: Option<ReasoningPatchFamily>,
    pub reasoning_preset: Option<ReasoningPreset>,
    pub reasoning_suffix: Option<String>,
    pub runtime_features: TargetRuntimeFeatures,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetRuntimeFeatures {
    pub openai_reasoning_content_repair_enabled: bool,
    pub openai_reasoning_content_repair_source: RuntimeFeatureConfigSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFeatureConfigSource {
    DefaultFalse,
    ProviderDefault,
    ModelOverride,
}

impl RuntimeFeatureConfigSource {
    pub(crate) fn as_key(self) -> &'static str {
        match self {
            Self::DefaultFalse => "default_false",
            Self::ProviderDefault => "provider_default",
            Self::ModelOverride => "model_override",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReasoningConfigSource {
    ProviderDefault,
    ModelCustom,
    ModelDisabled,
    Missing,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EffectiveReasoningConfig<'a> {
    pub source: ReasoningConfigSource,
    pub config: Option<&'a CacheReasoningConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionTargetReasoningBinding {
    pub config_id: i64,
    pub config_scope: ReasoningConfigScope,
    pub config_source: ReasoningConfigSource,
    pub config_preset_id: i64,
    pub family: ReasoningPatchFamily,
    pub preset: ReasoningPreset,
    pub suffix: String,
}

impl ExecutionTarget {
    fn apply_reasoning_binding(&mut self, binding: ExecutionTargetReasoningBinding) {
        self.reasoning_config_id = Some(binding.config_id);
        self.reasoning_config_scope = Some(binding.config_scope);
        self.reasoning_config_source = Some(binding.config_source);
        self.reasoning_config_preset_id = Some(binding.config_preset_id);
        self.reasoning_family = Some(binding.family);
        self.reasoning_preset = Some(binding.preset);
        self.reasoning_suffix = Some(binding.suffix);
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub requested_name: String,
    pub base_requested_name: String,
    pub resolved_reasoning_suffix: Option<String>,
    pub resolved_reasoning_preset: Option<ReasoningPreset>,
    pub requested_model_parse_status: RequestedModelParseStatus,
    pub target: ExecutionTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionPlanBuildError {
    InvalidModelFormat(String),
    TargetNotFound(String),
    UnsupportedCapability(String),
    CatalogUnavailable(String),
}

impl ExecutionPlanBuildError {
    fn with_message(self, message: String) -> Self {
        match self {
            Self::InvalidModelFormat(_) => Self::InvalidModelFormat(message),
            Self::TargetNotFound(_) => Self::TargetNotFound(message),
            Self::UnsupportedCapability(_) => Self::UnsupportedCapability(message),
            Self::CatalogUnavailable(_) => Self::CatalogUnavailable(message),
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::InvalidModelFormat(message)
            | Self::TargetNotFound(message)
            | Self::UnsupportedCapability(message)
            | Self::CatalogUnavailable(message) => message,
        }
    }
}

impl fmt::Display for ExecutionPlanBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for ExecutionPlanBuildError {}

impl ExecutionPlan {
    pub fn target_summary_for_log(&self) -> String {
        let target = &self.target;
        format!(
            "base_name={}; provider={}/{}; model={}/{}; llm_api={:?}; reasoning_suffix={:?}; runtime_feature_openai_reasoning_content_repair={}/{}",
            self.base_requested_name,
            target.provider.id,
            target.provider.provider_key,
            target.model.id,
            target.model.model_name,
            target.upstream_protocol,
            self.resolved_reasoning_suffix,
            target
                .runtime_features
                .openai_reasoning_content_repair_enabled,
            target
                .runtime_features
                .openai_reasoning_content_repair_source
                .as_key(),
        )
    }

    fn apply_resolved_requested_model_name(&mut self, resolved: ResolvedRequestedModelName) {
        self.requested_name = resolved.original_requested_name;
        self.base_requested_name = resolved.base_requested_name;
        self.resolved_reasoning_suffix = resolved.requested_suffix;
        self.resolved_reasoning_preset = resolved.requested_preset;
        self.requested_model_parse_status = resolved.parse_status;
    }
}

fn parse_provider_model(value: &str) -> (&str, &str) {
    let mut parts = value.splitn(2, '/');
    (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
}

fn build_direct_execution_plan(
    catalog: &CacheModelsCatalog,
    requested_name: &str,
) -> Result<ExecutionPlan, ExecutionPlanBuildError> {
    let (provider_key, model_name) = parse_provider_model(requested_name);
    if provider_key.is_empty() || model_name.is_empty() {
        return Err(ExecutionPlanBuildError::InvalidModelFormat(format!(
            "Invalid model format: '{}'. Expected 'provider/model'.",
            requested_name
        )));
    }

    let provider = catalog
        .providers
        .iter()
        .find(|provider| provider.provider_key == provider_key && provider.is_enabled)
        .cloned()
        .ok_or_else(|| {
            ExecutionPlanBuildError::TargetNotFound(format!(
                "Enabled provider '{}' was not found.",
                provider_key
            ))
        })?;
    let model = catalog
        .models
        .iter()
        .find(|model| {
            model.provider_id == provider.id && model.model_name == model_name && model.is_enabled
        })
        .cloned()
        .ok_or_else(|| {
            ExecutionPlanBuildError::TargetNotFound(format!(
                "Enabled model '{}' was not found.",
                requested_name
            ))
        })?;
    let upstream_protocol = determine_upstream_protocol(&provider);
    let runtime_features = resolve_target_runtime_features(catalog, &provider, &model);
    Ok(ExecutionPlan {
        requested_name: requested_name.to_string(),
        base_requested_name: requested_name.to_string(),
        resolved_reasoning_suffix: None,
        resolved_reasoning_preset: None,
        requested_model_parse_status: RequestedModelParseStatus::Exact,
        target: ExecutionTarget {
            provider: Arc::new(provider),
            model: Arc::new(model),
            upstream_protocol,
            reasoning_config_id: None,
            reasoning_config_scope: None,
            reasoning_config_source: None,
            reasoning_config_preset_id: None,
            reasoning_family: None,
            reasoning_preset: None,
            reasoning_suffix: None,
            runtime_features,
        },
    })
}

pub(crate) fn target_supports_reasoning_preset(
    catalog: &CacheModelsCatalog,
    target: &ExecutionTarget,
    preset: ReasoningPreset,
) -> Result<ExecutionTargetReasoningBinding, String> {
    let effective = resolve_effective_reasoning_config(catalog, &target.provider, &target.model);
    let config = match effective.config {
        Some(_) if matches!(effective.source, ReasoningConfigSource::ModelDisabled) => {
            return Err(format!(
                "model '{}' has disabled reasoning suffix config",
                target.model.model_name
            ));
        }
        Some(config) if matches!(config.mode, ReasoningConfigMode::Custom) => config,
        Some(config) => {
            return Err(format!(
                "reasoning config {} for provider '{}' model '{}' is not custom",
                config.id, target.provider.provider_key, target.model.model_name
            ));
        }
        None => {
            return Err(format!(
                "provider '{}' model '{}' has no active reasoning config",
                target.provider.provider_key, target.model.model_name
            ));
        }
    };
    let family = config
        .family
        .ok_or_else(|| format!("reasoning config {} is missing a patch family", config.id))?;
    let config_preset = config
        .presets
        .iter()
        .find(|row| row.preset == preset && row.is_enabled)
        .ok_or_else(|| {
            format!(
                "reasoning config {} does not enable preset '{}'",
                config.id, preset
            )
        })?;

    generate_reasoning_patches(
        family,
        preset,
        ReasoningPatchContext::for_model(target.upstream_protocol, &target.model),
    )
    .map_err(|err| err.to_string())?;

    Ok(ExecutionTargetReasoningBinding {
        config_id: config.id,
        config_scope: config.scope_kind,
        config_source: effective.source,
        config_preset_id: config_preset.id,
        family,
        preset,
        suffix: preset.canonical_suffix().to_string(),
    })
}

pub(crate) fn resolve_effective_reasoning_config<'a>(
    catalog: &'a CacheModelsCatalog,
    provider: &CacheProvider,
    model: &CacheModel,
) -> EffectiveReasoningConfig<'a> {
    if let Some(model_config) = catalog.reasoning_configs.iter().find(|config| {
        matches!(config.scope_kind, ReasoningConfigScope::Model)
            && config.model_id == Some(model.id)
    }) {
        return EffectiveReasoningConfig {
            source: match model_config.mode {
                ReasoningConfigMode::Custom => ReasoningConfigSource::ModelCustom,
                ReasoningConfigMode::Disabled => ReasoningConfigSource::ModelDisabled,
            },
            config: Some(model_config),
        };
    }

    if let Some(provider_config) = catalog.reasoning_configs.iter().find(|config| {
        matches!(config.scope_kind, ReasoningConfigScope::Provider)
            && config.provider_id == Some(provider.id)
            && matches!(config.mode, ReasoningConfigMode::Custom)
    }) {
        return EffectiveReasoningConfig {
            source: ReasoningConfigSource::ProviderDefault,
            config: Some(provider_config),
        };
    }

    EffectiveReasoningConfig {
        source: ReasoningConfigSource::Missing,
        config: None,
    }
}

pub(crate) fn resolve_target_runtime_features(
    catalog: &CacheModelsCatalog,
    provider: &CacheProvider,
    model: &CacheModel,
) -> TargetRuntimeFeatures {
    let (enabled, source) = resolve_effective_runtime_feature(
        catalog,
        provider,
        model,
        RuntimeFeatureKey::OpenAiReasoningContentRepair,
    );
    TargetRuntimeFeatures {
        openai_reasoning_content_repair_enabled: enabled,
        openai_reasoning_content_repair_source: source,
    }
}

fn resolve_effective_runtime_feature(
    catalog: &CacheModelsCatalog,
    provider: &CacheProvider,
    model: &CacheModel,
    feature_key: RuntimeFeatureKey,
) -> (bool, RuntimeFeatureConfigSource) {
    if let Some(config) = catalog.runtime_feature_configs.iter().find(|config| {
        matches!(config.scope_kind, RuntimeFeatureConfigScope::Model)
            && config.model_id == Some(model.id)
            && config.feature_key == feature_key
    }) {
        return (config.enabled, RuntimeFeatureConfigSource::ModelOverride);
    }
    if let Some(config) = catalog.runtime_feature_configs.iter().find(|config| {
        matches!(config.scope_kind, RuntimeFeatureConfigScope::Provider)
            && config.provider_id == Some(provider.id)
            && config.feature_key == feature_key
    }) {
        return (config.enabled, RuntimeFeatureConfigSource::ProviderDefault);
    }
    (false, RuntimeFeatureConfigSource::DefaultFalse)
}

fn build_execution_plan_from_catalog(
    catalog: &CacheModelsCatalog,
    requested_name: &str,
) -> Result<ExecutionPlan, ExecutionPlanBuildError> {
    match build_direct_execution_plan(catalog, requested_name) {
        Ok(plan) => Ok(plan),
        Err(exact_error) => {
            let suffixes = enabled_reasoning_suffixes(catalog);
            let Some(resolved_name) = parse_reasoning_suffix(requested_name, &suffixes) else {
                return Err(exact_error);
            };
            let mut plan = build_direct_execution_plan(catalog, &resolved_name.base_requested_name)
                .map_err(|base_error| {
                    let message = format!(
                        "Model '{}' uses a known reasoning suffix, but base model '{}' could not be resolved: {}",
                        resolved_name.original_requested_name,
                        resolved_name.base_requested_name,
                        base_error
                    );
                    base_error.with_message(message)
                })?;
            let preset = resolved_name
                .requested_preset
                .expect("reasoning suffix parse includes a preset");
            let binding = target_supports_reasoning_preset(catalog, &plan.target, preset).map_err(
                |reason| {
                    ExecutionPlanBuildError::UnsupportedCapability(format!(
                        "Reasoning suffix '{}' is not supported by '{}': {}",
                        resolved_name.requested_suffix.as_deref().unwrap_or(""),
                        resolved_name.base_requested_name,
                        reason
                    ))
                },
            )?;
            plan.target.apply_reasoning_binding(binding);
            plan.apply_resolved_requested_model_name(resolved_name);
            Ok(plan)
        }
    }
}

pub(crate) async fn build_execution_plan(
    app_state: &Arc<AppState>,
    requested_name: &str,
) -> Result<ExecutionPlan, ExecutionPlanBuildError> {
    let catalog = app_state
        .catalog
        .get_models_catalog()
        .await
        .map_err(|err| {
            error!(
                "Error loading models catalog while resolving '{}': {:?}",
                requested_name, err
            );
            ExecutionPlanBuildError::CatalogUnavailable(format!(
                "Internal server error while loading model catalog for '{}'.",
                requested_name
            ))
        })?;
    build_execution_plan_from_catalog(catalog.as_ref(), requested_name)
}

#[cfg(test)]
mod execution_plan_error_tests {
    use super::{ExecutionPlanBuildError, build_execution_plan_from_catalog};
    use crate::service::cache::types::CacheModelsCatalog;

    fn empty_catalog() -> CacheModelsCatalog {
        CacheModelsCatalog {
            providers: vec![],
            models: vec![],
            reasoning_configs: vec![],
            runtime_feature_configs: vec![],
        }
    }

    #[test]
    fn malformed_model_name_is_a_parse_error() {
        let error = build_execution_plan_from_catalog(&empty_catalog(), "gpt-4o")
            .expect_err("provider prefix is required");

        assert!(matches!(
            error,
            ExecutionPlanBuildError::InvalidModelFormat(_)
        ));
    }

    #[test]
    fn missing_direct_target_is_not_a_parse_error() {
        let error = build_execution_plan_from_catalog(&empty_catalog(), "openai/gpt-4o")
            .expect_err("missing provider should fail resolution");

        assert!(matches!(error, ExecutionPlanBuildError::TargetNotFound(_)));
    }
}
