use std::sync::Arc;

use crate::{
    database::reasoning_config::{
        ReasoningConfigMode, ReasoningConfigScope, ReasoningPatchFamily, ReasoningPreset,
    },
    database::runtime_feature_config::{RuntimeFeatureConfigScope, RuntimeFeatureKey},
    schema::enum_def::{LlmApiType, ProviderApiKeyMode},
    service::{
        app_state::AppState,
        cache::types::{
            CacheApiKeyModelOverride, CacheModel, CacheModelRoute, CacheModelRouteCandidate,
            CacheModelsCatalog, CacheProvider, CacheReasoningConfig,
        },
    },
};
use cyder_tools::log::{debug, error, warn};

use super::super::{
    reasoning_suffix::{ReasoningPatchContext, generate_reasoning_patches},
    requested_model::{
        RequestedModelParseStatus, ResolvedRequestedModelName, enabled_reasoning_suffixes,
        parse_reasoning_suffix,
    },
    util::determine_target_api_type,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedNameScope {
    Direct,
    GlobalRoute,
    ApiKeyOverride,
}

impl ResolvedNameScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::GlobalRoute => "global_route",
            Self::ApiKeyOverride => "api_key_override",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionCandidate {
    pub candidate_position: usize,
    pub route_id: Option<i64>,
    pub route_name: Option<String>,
    pub route_candidate_priority: Option<i32>,
    pub provider: Arc<CacheProvider>,
    pub model: Arc<CacheModel>,
    pub llm_api_type: LlmApiType,
    pub provider_api_key_mode: ProviderApiKeyMode,
    pub reasoning_config_id: Option<i64>,
    pub reasoning_config_scope: Option<ReasoningConfigScope>,
    pub reasoning_config_source: Option<ReasoningConfigSource>,
    pub reasoning_config_preset_id: Option<i64>,
    pub reasoning_family: Option<ReasoningPatchFamily>,
    pub reasoning_preset: Option<ReasoningPreset>,
    pub reasoning_suffix: Option<String>,
    pub runtime_features: CandidateRuntimeFeatures,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateRuntimeFeatures {
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

impl ReasoningConfigSource {
    pub(crate) fn as_key(self) -> &'static str {
        match self {
            Self::ProviderDefault => "provider_default",
            Self::ModelCustom => "model_custom",
            Self::ModelDisabled => "model_disabled",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EffectiveReasoningConfig<'a> {
    pub source: ReasoningConfigSource,
    pub config: Option<&'a CacheReasoningConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionCandidateReasoningBinding {
    pub config_id: i64,
    pub config_scope: ReasoningConfigScope,
    pub config_source: ReasoningConfigSource,
    pub config_preset_id: i64,
    pub family: ReasoningPatchFamily,
    pub preset: ReasoningPreset,
    pub suffix: String,
}

impl ExecutionCandidate {
    fn apply_reasoning_binding(&mut self, binding: ExecutionCandidateReasoningBinding) {
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
pub(crate) struct RouteCandidateRuntimeResolution {
    pub route_candidate_position: usize,
    pub route_candidate: CacheModelRouteCandidate,
    pub candidate: Option<ExecutionCandidate>,
    pub stale_reason: Option<String>,
}

impl RouteCandidateRuntimeResolution {
    pub(crate) fn runtime_status_key(&self) -> &'static str {
        if self.candidate.is_some() {
            "valid"
        } else {
            "stale_skipped"
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub requested_name: String,
    pub base_requested_name: String,
    pub resolved_reasoning_suffix: Option<String>,
    pub resolved_reasoning_preset: Option<ReasoningPreset>,
    pub requested_model_parse_status: RequestedModelParseStatus,
    pub resolved_scope: ResolvedNameScope,
    pub resolved_route_id: Option<i64>,
    pub resolved_route_name: Option<String>,
    pub candidates: Vec<ExecutionCandidate>,
}

impl ExecutionPlan {
    #[cfg(test)]
    pub fn primary_candidate(&self) -> Result<&ExecutionCandidate, String> {
        self.candidates.first().ok_or_else(|| {
            format!(
                "Execution plan for '{}' does not have any candidates.",
                self.requested_name
            )
        })
    }

    pub fn candidate_model_ids(&self) -> Vec<i64> {
        self.candidates
            .iter()
            .map(|candidate| candidate.model.id)
            .collect()
    }

    pub fn candidate_summary_for_log(&self) -> String {
        let candidate_details = self
            .candidates
            .iter()
            .map(|candidate| {
                format!(
                    "#{} route={:?}/{} priority={:?} provider={}/{} model={}/{} llm_api={:?} key_mode={:?} reasoning_config={:?}/{}/{} reasoning_preset_row={:?} reasoning_family={:?} reasoning_preset={:?} reasoning_suffix={:?} runtime_feature_openai_reasoning_content_repair={}/{}",
                    candidate.candidate_position,
                    candidate.route_id,
                    candidate.route_name.as_deref().unwrap_or("direct"),
                    candidate.route_candidate_priority,
                    candidate.provider.id,
                    candidate.provider.provider_key,
                    candidate.model.id,
                    candidate.model.model_name,
                    candidate.llm_api_type,
                    candidate.provider_api_key_mode,
                    candidate.reasoning_config_id,
                    candidate
                        .reasoning_config_scope
                        .map(|scope| scope.as_key())
                        .unwrap_or("none"),
                    candidate
                        .reasoning_config_source
                        .map(|source| source.as_key())
                        .unwrap_or("none"),
                    candidate.reasoning_config_preset_id,
                    candidate.reasoning_family,
                    candidate.reasoning_preset,
                    candidate.reasoning_suffix,
                    candidate
                        .runtime_features
                        .openai_reasoning_content_repair_enabled,
                    candidate
                        .runtime_features
                        .openai_reasoning_content_repair_source
                        .as_key()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "base_name={}; reasoning_suffix={:?}; reasoning_preset={:?}; model_ids={:?}; {}",
            self.base_requested_name,
            self.resolved_reasoning_suffix,
            self.resolved_reasoning_preset,
            self.candidate_model_ids(),
            candidate_details
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

fn parse_provider_model(pm: &str) -> (&str, &str) {
    let mut parts = pm.splitn(2, '/');
    let provider = parts.next().unwrap_or("");
    let model_id = parts.next().unwrap_or("");
    (provider, model_id)
}

fn build_candidate(
    catalog: &CacheModelsCatalog,
    requested_name: &str,
    route: Option<&CacheModelRoute>,
    route_candidate: Option<&CacheModelRouteCandidate>,
    model_id: i64,
    candidate_position: usize,
) -> Result<ExecutionCandidate, String> {
    let model = catalog
        .models
        .iter()
        .find(|model| model.id == model_id)
        .cloned()
        .ok_or_else(|| match route {
            Some(route) => format!(
                "Candidate model {} for route '{}' was not found.",
                model_id, route.route_name
            ),
            None => format!("Model '{}' was not found.", requested_name),
        })?;
    let provider = catalog
        .providers
        .iter()
        .find(|provider| provider.id == model.provider_id)
        .cloned()
        .ok_or_else(|| match route {
            Some(route) => format!(
                "Provider ID {} for route '{}' was not found.",
                model.provider_id, route.route_name
            ),
            None => format!(
                "Provider ID {} for model '{}' was not found.",
                model.provider_id, model.model_name
            ),
        })?;
    let llm_api_type = determine_target_api_type(&provider);
    let provider_api_key_mode = provider.provider_api_key_mode.clone();
    let runtime_features = resolve_candidate_runtime_features(catalog, &provider, &model);

    Ok(ExecutionCandidate {
        candidate_position,
        route_id: route.map(|route| route.id),
        route_name: route.map(|route| route.route_name.clone()),
        route_candidate_priority: route_candidate.map(|candidate| candidate.priority),
        provider: Arc::new(provider),
        model: Arc::new(model),
        llm_api_type,
        provider_api_key_mode,
        reasoning_config_id: None,
        reasoning_config_scope: None,
        reasoning_config_source: None,
        reasoning_config_preset_id: None,
        reasoning_family: None,
        reasoning_preset: None,
        reasoning_suffix: None,
        runtime_features,
    })
}

fn build_route_execution_plan(
    catalog: &CacheModelsCatalog,
    requested_name: &str,
    resolved_scope: ResolvedNameScope,
    route: &CacheModelRoute,
) -> Result<ExecutionPlan, String> {
    let runtime_candidates = resolve_route_runtime_candidates(catalog, requested_name, route)?;
    let candidates = runtime_candidates
        .into_iter()
        .filter_map(|runtime_candidate| runtime_candidate.candidate)
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return Err(format!(
            "Model route '{}' does not have any valid candidates.",
            route.route_name
        ));
    }

    Ok(ExecutionPlan {
        requested_name: requested_name.to_string(),
        base_requested_name: requested_name.to_string(),
        resolved_reasoning_suffix: None,
        resolved_reasoning_preset: None,
        requested_model_parse_status: RequestedModelParseStatus::Exact,
        resolved_scope,
        resolved_route_id: Some(route.id),
        resolved_route_name: Some(route.route_name.clone()),
        candidates,
    })
}

pub(crate) fn resolve_route_runtime_candidates(
    catalog: &CacheModelsCatalog,
    requested_name: &str,
    route: &CacheModelRoute,
) -> Result<Vec<RouteCandidateRuntimeResolution>, String> {
    if !route.is_enabled {
        return Err(format!("Model route '{}' is disabled.", route.route_name));
    }

    let enabled_candidates = route
        .candidates
        .iter()
        .filter(|candidate| candidate.is_enabled)
        .collect::<Vec<_>>();
    if enabled_candidates.is_empty() {
        return Err(format!(
            "Model route '{}' does not have any enabled candidates.",
            route.route_name
        ));
    }

    let mut resolutions = Vec::with_capacity(enabled_candidates.len());
    let mut valid_candidate_position = 1usize;
    for (index, route_candidate) in enabled_candidates.into_iter().enumerate() {
        match build_candidate(
            catalog,
            requested_name,
            Some(route),
            Some(route_candidate),
            route_candidate.model_id,
            valid_candidate_position,
        ) {
            Ok(candidate) => {
                valid_candidate_position += 1;
                resolutions.push(RouteCandidateRuntimeResolution {
                    route_candidate_position: index + 1,
                    route_candidate: route_candidate.clone(),
                    candidate: Some(candidate),
                    stale_reason: None,
                });
            }
            Err(error) => {
                warn!(
                    "Skipping stale execution candidate for route '{}' model_id {}: {}",
                    route.route_name, route_candidate.model_id, error
                );
                resolutions.push(RouteCandidateRuntimeResolution {
                    route_candidate_position: index + 1,
                    route_candidate: route_candidate.clone(),
                    candidate: None,
                    stale_reason: Some(error),
                });
            }
        }
    }

    Ok(resolutions)
}

fn build_direct_execution_plan(
    catalog: &CacheModelsCatalog,
    requested_name: &str,
) -> Result<ExecutionPlan, String> {
    let (provider_key_str, model_name_str) = parse_provider_model(requested_name);
    if provider_key_str.is_empty() || model_name_str.is_empty() {
        return Err(format!(
            "Invalid model format: '{}'. Expected a configured route or 'provider/model'.",
            requested_name
        ));
    }

    let provider = catalog
        .providers
        .iter()
        .find(|provider| provider.provider_key == provider_key_str)
        .ok_or_else(|| format!("Provider '{}' not found.", provider_key_str))?;

    let model = catalog
        .models
        .iter()
        .find(|model| model.provider_id == provider.id && model.model_name == model_name_str)
        .ok_or_else(|| format!("Model '{}' not found.", requested_name))?;

    if model.provider_id != provider.id {
        return Err(format!(
            "Model '{}' does not belong to provider '{}'.",
            model.model_name, provider.name
        ));
    }

    let candidate = build_candidate(catalog, requested_name, None, None, model.id, 1)?;

    Ok(ExecutionPlan {
        requested_name: requested_name.to_string(),
        base_requested_name: requested_name.to_string(),
        resolved_reasoning_suffix: None,
        resolved_reasoning_preset: None,
        requested_model_parse_status: RequestedModelParseStatus::Exact,
        resolved_scope: ResolvedNameScope::Direct,
        resolved_route_id: None,
        resolved_route_name: None,
        candidates: vec![candidate],
    })
}

fn find_enabled_override<'a>(
    catalog: &'a CacheModelsCatalog,
    api_key_id: i64,
    requested_name: &str,
) -> Option<&'a CacheApiKeyModelOverride> {
    catalog.api_key_overrides.iter().find(|override_row| {
        override_row.api_key_id == api_key_id
            && override_row.source_name == requested_name
            && override_row.is_enabled
    })
}

fn build_exact_execution_plan_from_catalog(
    catalog: &CacheModelsCatalog,
    api_key_id: i64,
    requested_name: &str,
) -> Result<ExecutionPlan, String> {
    if let Some(override_row) = find_enabled_override(catalog, api_key_id, requested_name) {
        let route = catalog
            .routes
            .iter()
            .find(|route| route.id == override_row.target_route_id)
            .ok_or_else(|| {
                format!(
                    "API key override for '{}' references missing route {}.",
                    requested_name, override_row.target_route_id
                )
            })?;
        debug!(
            "Resolved '{}' via api key override for api_key_id {} to route '{}'",
            requested_name, api_key_id, route.route_name
        );
        return build_route_execution_plan(
            catalog,
            requested_name,
            ResolvedNameScope::ApiKeyOverride,
            route,
        );
    }

    if let Some(route) = catalog
        .routes
        .iter()
        .find(|route| route.route_name == requested_name)
    {
        debug!(
            "Resolved '{}' as a global model route '{}'",
            requested_name, route.route_name
        );
        return build_route_execution_plan(
            catalog,
            requested_name,
            ResolvedNameScope::GlobalRoute,
            route,
        );
    }

    debug!(
        "'{}' is not a configured route. Attempting direct provider/model parsing.",
        requested_name
    );
    build_direct_execution_plan(catalog, requested_name)
}

pub(crate) fn candidate_supports_reasoning_preset(
    catalog: &CacheModelsCatalog,
    candidate: &ExecutionCandidate,
    preset: ReasoningPreset,
) -> Result<ExecutionCandidateReasoningBinding, String> {
    let effective =
        resolve_effective_reasoning_config(catalog, &candidate.provider, &candidate.model);
    let config = match effective.config {
        Some(_) if matches!(effective.source, ReasoningConfigSource::ModelDisabled) => {
            return Err(format!(
                "model '{}' has disabled reasoning suffix config",
                candidate.model.model_name
            ));
        }
        Some(config) if matches!(config.mode, ReasoningConfigMode::Custom) => config,
        Some(config) => {
            return Err(format!(
                "reasoning config {} for provider '{}' model '{}' is not a custom config",
                config.id, candidate.provider.provider_key, candidate.model.model_name
            ));
        }
        None => {
            return Err(format!(
                "provider '{}' model '{}' does not have an active reasoning config",
                candidate.provider.provider_key, candidate.model.model_name
            ));
        }
    };
    let family = config.family.ok_or_else(|| {
        format!(
            "reasoning config {} for provider '{}' model '{}' is custom but missing family",
            config.id, candidate.provider.provider_key, candidate.model.model_name
        )
    })?;
    let config_preset = config
        .presets
        .iter()
        .find(|config_preset| config_preset.preset == preset && config_preset.is_enabled)
        .ok_or_else(|| {
            format!(
                "reasoning config {} does not enable preset '{}'",
                config.id, preset
            )
        })?;

    generate_reasoning_patches(
        family,
        preset,
        ReasoningPatchContext::for_model(candidate.llm_api_type, &candidate.model),
    )
    .map_err(|err| err.to_string())?;

    Ok(ExecutionCandidateReasoningBinding {
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
        return match model_config.mode {
            ReasoningConfigMode::Custom => EffectiveReasoningConfig {
                source: ReasoningConfigSource::ModelCustom,
                config: Some(model_config),
            },
            ReasoningConfigMode::Disabled => EffectiveReasoningConfig {
                source: ReasoningConfigSource::ModelDisabled,
                config: Some(model_config),
            },
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

pub(crate) fn resolve_candidate_runtime_features(
    catalog: &CacheModelsCatalog,
    provider: &CacheProvider,
    model: &CacheModel,
) -> CandidateRuntimeFeatures {
    let (openai_reasoning_content_repair_enabled, openai_reasoning_content_repair_source) =
        resolve_effective_runtime_feature(
            catalog,
            provider,
            model,
            RuntimeFeatureKey::OpenAiReasoningContentRepair,
        );

    CandidateRuntimeFeatures {
        openai_reasoning_content_repair_enabled,
        openai_reasoning_content_repair_source,
    }
}

fn resolve_effective_runtime_feature(
    catalog: &CacheModelsCatalog,
    provider: &CacheProvider,
    model: &CacheModel,
    feature_key: RuntimeFeatureKey,
) -> (bool, RuntimeFeatureConfigSource) {
    if let Some(model_config) = catalog.runtime_feature_configs.iter().find(|config| {
        matches!(config.scope_kind, RuntimeFeatureConfigScope::Model)
            && config.model_id == Some(model.id)
            && config.feature_key == feature_key
    }) {
        return (
            model_config.enabled,
            RuntimeFeatureConfigSource::ModelOverride,
        );
    }

    if let Some(provider_config) = catalog.runtime_feature_configs.iter().find(|config| {
        matches!(config.scope_kind, RuntimeFeatureConfigScope::Provider)
            && config.provider_id == Some(provider.id)
            && config.feature_key == feature_key
    }) {
        return (
            provider_config.enabled,
            RuntimeFeatureConfigSource::ProviderDefault,
        );
    }

    (false, RuntimeFeatureConfigSource::DefaultFalse)
}

pub(crate) fn route_supports_reasoning_preset(
    catalog: &CacheModelsCatalog,
    route: &CacheModelRoute,
    preset: ReasoningPreset,
) -> Result<Vec<ExecutionCandidateReasoningBinding>, String> {
    let plan = build_route_execution_plan(
        catalog,
        &route.route_name,
        ResolvedNameScope::GlobalRoute,
        route,
    )?;

    let mut bindings = Vec::with_capacity(plan.candidates.len());
    for candidate in &plan.candidates {
        let binding = candidate_supports_reasoning_preset(catalog, candidate, preset).map_err(
            |reason| {
                format!(
                    "route '{}' candidate provider '{}' model '{}' does not support preset '{}': {}",
                    route.route_name,
                    candidate.provider.provider_key,
                    candidate.model.model_name,
                    preset,
                    reason
                )
            },
        )?;
        bindings.push(binding);
    }

    Ok(bindings)
}

fn reasoning_bindings_for_execution_plan(
    catalog: &CacheModelsCatalog,
    plan: &ExecutionPlan,
    preset: ReasoningPreset,
) -> Result<Vec<ExecutionCandidateReasoningBinding>, String> {
    if let Some(route_id) = plan.resolved_route_id {
        let route = catalog
            .routes
            .iter()
            .find(|route| route.id == route_id)
            .ok_or_else(|| {
                format!(
                    "resolved route {} for '{}' was not found while binding reasoning preset '{}'",
                    route_id, plan.base_requested_name, preset
                )
            })?;
        return route_supports_reasoning_preset(catalog, route, preset);
    }

    plan.candidates
        .iter()
        .map(|candidate| {
            candidate_supports_reasoning_preset(catalog, candidate, preset).map_err(|reason| {
                format!(
                    "direct candidate provider '{}' model '{}' does not support preset '{}': {}",
                    candidate.provider.provider_key, candidate.model.model_name, preset, reason
                )
            })
        })
        .collect()
}

fn bind_execution_plan_reasoning_preset(
    catalog: &CacheModelsCatalog,
    plan: &mut ExecutionPlan,
    suffix: &str,
    preset: ReasoningPreset,
) -> Result<(), String> {
    let bindings =
        reasoning_bindings_for_execution_plan(catalog, plan, preset).map_err(|reason| {
            format!(
                "Reasoning suffix '{}' (preset '{}') is not supported by base model '{}': {}",
                suffix, preset, plan.base_requested_name, reason
            )
        })?;

    if bindings.len() != plan.candidates.len() {
        return Err(format!(
            "Reasoning suffix '{}' (preset '{}') resolved {} bindings for {} candidates on base model '{}'.",
            suffix,
            preset,
            bindings.len(),
            plan.candidates.len(),
            plan.base_requested_name
        ));
    }

    for (candidate, binding) in plan.candidates.iter_mut().zip(bindings) {
        candidate.apply_reasoning_binding(binding);
    }

    Ok(())
}

fn build_execution_plan_from_catalog(
    catalog: &CacheModelsCatalog,
    api_key_id: i64,
    requested_name: &str,
) -> Result<ExecutionPlan, String> {
    match build_exact_execution_plan_from_catalog(catalog, api_key_id, requested_name) {
        Ok(plan) => return Ok(plan),
        Err(exact_error) => {
            let suffixes = enabled_reasoning_suffixes(catalog);
            let Some(resolved_name) = parse_reasoning_suffix(requested_name, &suffixes) else {
                return Err(exact_error);
            };

            let mut plan = build_exact_execution_plan_from_catalog(
                catalog,
                api_key_id,
                &resolved_name.base_requested_name,
            )
            .map_err(|base_error| {
                format!(
                    "Model '{}' uses known reasoning suffix '{}' (preset '{}'), but base model '{}' could not be resolved: {}",
                    resolved_name.original_requested_name,
                    resolved_name.requested_suffix.as_deref().unwrap_or(""),
                    resolved_name
                        .requested_preset
                        .map(|preset| preset.as_key())
                        .unwrap_or(""),
                    resolved_name.base_requested_name,
                    base_error
                )
            })?;

            let suffix = resolved_name.requested_suffix.clone().unwrap_or_default();
            let preset = resolved_name
                .requested_preset
                .expect("reasoning suffix parse should include preset");
            bind_execution_plan_reasoning_preset(catalog, &mut plan, &suffix, preset)?;
            plan.apply_resolved_requested_model_name(resolved_name);
            Ok(plan)
        }
    }
}

pub async fn build_execution_plan(
    app_state: &Arc<AppState>,
    api_key_id: i64,
    requested_name: &str,
) -> Result<ExecutionPlan, String> {
    let catalog = app_state.catalog.get_models_catalog().await.map_err(|e| {
        error!(
            "Error loading models catalog while resolving '{}': {:?}",
            requested_name, e
        );
        format!(
            "Internal server error while loading model catalog for '{}'.",
            requested_name
        )
    })?;

    build_execution_plan_from_catalog(catalog.as_ref(), api_key_id, requested_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::{
            reasoning_config::{
                ReasoningConfigMode, ReasoningConfigScope, ReasoningPatchFamily, ReasoningPreset,
            },
            runtime_feature_config::{RuntimeFeatureConfigScope, RuntimeFeatureKey},
        },
        schema::enum_def::{ProviderApiKeyMode, ProviderType},
        service::cache::types::{
            CacheApiKeyModelOverride, CacheModel, CacheModelRoute, CacheModelRouteCandidate,
            CacheModelsCatalog, CacheProvider, CacheReasoningConfig, CacheReasoningConfigPreset,
            CacheRuntimeFeatureConfig,
        },
    };

    fn provider(id: i64, provider_key: &str, provider_type: ProviderType) -> CacheProvider {
        CacheProvider {
            id,
            provider_key: provider_key.to_string(),
            name: provider_key.to_string(),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
        }
    }

    fn provider_reasoning_config(
        id: i64,
        provider_id: i64,
        family: ReasoningPatchFamily,
        presets: &[ReasoningPreset],
    ) -> CacheReasoningConfig {
        reasoning_config(
            id,
            ReasoningConfigScope::Provider,
            Some(provider_id),
            None,
            family,
            presets,
        )
    }

    fn model_reasoning_config(
        id: i64,
        model_id: i64,
        family: ReasoningPatchFamily,
        presets: &[ReasoningPreset],
    ) -> CacheReasoningConfig {
        reasoning_config(
            id,
            ReasoningConfigScope::Model,
            None,
            Some(model_id),
            family,
            presets,
        )
    }

    fn model_disabled_reasoning_config(id: i64, model_id: i64) -> CacheReasoningConfig {
        CacheReasoningConfig {
            id,
            scope_kind: ReasoningConfigScope::Model,
            provider_id: None,
            model_id: Some(model_id),
            mode: ReasoningConfigMode::Disabled,
            family: None,
            presets: Vec::new(),
        }
    }

    fn provider_runtime_feature_config(
        id: i64,
        provider_id: i64,
        feature_key: RuntimeFeatureKey,
        enabled: bool,
    ) -> CacheRuntimeFeatureConfig {
        CacheRuntimeFeatureConfig {
            id,
            scope_kind: RuntimeFeatureConfigScope::Provider,
            provider_id: Some(provider_id),
            model_id: None,
            feature_key,
            enabled,
        }
    }

    fn model_runtime_feature_config(
        id: i64,
        model_id: i64,
        feature_key: RuntimeFeatureKey,
        enabled: bool,
    ) -> CacheRuntimeFeatureConfig {
        CacheRuntimeFeatureConfig {
            id,
            scope_kind: RuntimeFeatureConfigScope::Model,
            provider_id: None,
            model_id: Some(model_id),
            feature_key,
            enabled,
        }
    }

    fn reasoning_config(
        id: i64,
        scope_kind: ReasoningConfigScope,
        provider_id: Option<i64>,
        model_id: Option<i64>,
        family: ReasoningPatchFamily,
        presets: &[ReasoningPreset],
    ) -> CacheReasoningConfig {
        CacheReasoningConfig {
            id,
            scope_kind,
            provider_id,
            model_id,
            mode: ReasoningConfigMode::Custom,
            family: Some(family),
            presets: presets
                .iter()
                .enumerate()
                .map(|(index, preset)| CacheReasoningConfigPreset {
                    id: id * 10 + index as i64,
                    config_id: id,
                    preset: *preset,
                    suffix: preset.canonical_suffix().to_string(),
                    requires_reasoning: preset.requires_reasoning(),
                    allowed_operation_kinds: preset
                        .allowed_operation_kinds()
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    expose_in_models: true,
                    is_enabled: true,
                })
                .collect(),
        }
    }

    fn model_with_id(
        id: i64,
        provider_id: i64,
        model_name: &str,
        real_model_name: Option<&str>,
    ) -> CacheModel {
        CacheModel {
            id,
            provider_id,
            model_name: model_name.to_string(),
            real_model_name: real_model_name.map(str::to_string),
            cost_catalog_id: None,
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: true,
            supports_image_input: true,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled: true,
        }
    }

    fn route_with_candidates(
        id: i64,
        route_name: &str,
        candidates: &[(i64, i32, bool)],
    ) -> CacheModelRoute {
        CacheModelRoute {
            id,
            route_name: route_name.to_string(),
            description: None,
            is_enabled: true,
            expose_in_models: true,
            candidates: candidates
                .iter()
                .map(
                    |(model_id, priority, is_enabled)| CacheModelRouteCandidate {
                        route_id: id,
                        model_id: *model_id,
                        provider_id: 2,
                        priority: *priority,
                        is_enabled: *is_enabled,
                    },
                )
                .collect(),
        }
    }

    fn catalog() -> CacheModelsCatalog {
        CacheModelsCatalog {
            providers: vec![
                provider(1, "openai", ProviderType::Openai),
                provider(2, "gemini", ProviderType::Gemini),
            ],
            models: vec![
                model_with_id(10, 1, "gpt-primary", Some("gpt-real")),
                model_with_id(20, 2, "gemini-primary", None),
                model_with_id(30, 1, "gpt-fallback", None),
            ],
            routes: vec![
                route_with_candidates(
                    100,
                    "smart-route",
                    &[(10, 10, true), (20, 20, false), (30, 30, true)],
                ),
                route_with_candidates(200, "override-route", &[(20, 5, true), (10, 10, true)]),
            ],
            api_key_overrides: vec![CacheApiKeyModelOverride {
                id: 500,
                api_key_id: 42,
                source_name: "smart-route".to_string(),
                target_route_id: 200,
                description: None,
                is_enabled: true,
            }],
            reasoning_configs: vec![],
            runtime_feature_configs: vec![],
        }
    }

    fn catalog_with_openai_high_reasoning() -> CacheModelsCatalog {
        let mut catalog = catalog();
        catalog.reasoning_configs.push(provider_reasoning_config(
            900,
            1,
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            &[ReasoningPreset::High],
        ));
        catalog
    }

    fn add_gemini_high_reasoning(catalog: &mut CacheModelsCatalog) {
        catalog.reasoning_configs.push(provider_reasoning_config(
            901,
            2,
            ReasoningPatchFamily::Gemini25ThinkingBudget,
            &[ReasoningPreset::High],
        ));
    }

    #[test]
    fn parse_provider_model_splits_only_on_first_separator() {
        assert_eq!(
            parse_provider_model("openai/gpt-4.1"),
            ("openai", "gpt-4.1")
        );
        assert_eq!(
            parse_provider_model("openai/family/model"),
            ("openai", "family/model")
        );
        assert_eq!(parse_provider_model("alias-only"), ("alias-only", ""));
        assert_eq!(parse_provider_model("/model"), ("", "model"));
    }

    #[test]
    fn resolved_name_scope_labels_are_stable() {
        assert_eq!(ResolvedNameScope::Direct.as_str(), "direct");
        assert_eq!(ResolvedNameScope::GlobalRoute.as_str(), "global_route");
        assert_eq!(
            ResolvedNameScope::ApiKeyOverride.as_str(),
            "api_key_override"
        );
    }

    #[test]
    fn build_execution_plan_outputs_single_direct_candidate() {
        let catalog = catalog();

        let plan = build_execution_plan_from_catalog(&catalog, 42, "openai/gpt-primary")
            .expect("direct model should resolve");

        assert_eq!(plan.requested_name, "openai/gpt-primary");
        assert_eq!(plan.base_requested_name, "openai/gpt-primary");
        assert_eq!(plan.resolved_reasoning_suffix, None);
        assert_eq!(plan.resolved_reasoning_preset, None);
        assert_eq!(plan.resolved_scope, ResolvedNameScope::Direct);
        assert_eq!(plan.resolved_route_id, None);
        assert_eq!(plan.candidate_model_ids(), vec![10]);
        let candidate = plan.primary_candidate().unwrap();
        assert_eq!(candidate.candidate_position, 1);
        assert_eq!(candidate.route_id, None);
        assert_eq!(candidate.llm_api_type, LlmApiType::Openai);
        assert_eq!(candidate.provider_api_key_mode, ProviderApiKeyMode::Queue);
        assert_eq!(
            candidate
                .runtime_features
                .openai_reasoning_content_repair_source,
            RuntimeFeatureConfigSource::DefaultFalse
        );
        assert!(
            !candidate
                .runtime_features
                .openai_reasoning_content_repair_enabled
        );
    }

    #[test]
    fn runtime_feature_provider_default_true_is_inherited_by_model_candidate() {
        let mut catalog = catalog();
        catalog
            .runtime_feature_configs
            .push(provider_runtime_feature_config(
                701,
                1,
                RuntimeFeatureKey::OpenAiReasoningContentRepair,
                true,
            ));

        let plan = build_execution_plan_from_catalog(&catalog, 42, "openai/gpt-primary")
            .expect("direct model should resolve");
        let candidate = plan.primary_candidate().unwrap();

        assert!(
            candidate
                .runtime_features
                .openai_reasoning_content_repair_enabled
        );
        assert_eq!(
            candidate
                .runtime_features
                .openai_reasoning_content_repair_source,
            RuntimeFeatureConfigSource::ProviderDefault
        );
    }

    #[test]
    fn runtime_feature_provider_default_false_is_inherited_by_model_candidate() {
        let mut catalog = catalog();
        catalog
            .runtime_feature_configs
            .push(provider_runtime_feature_config(
                702,
                1,
                RuntimeFeatureKey::OpenAiReasoningContentRepair,
                false,
            ));

        let plan = build_execution_plan_from_catalog(&catalog, 42, "openai/gpt-primary")
            .expect("direct model should resolve");
        let candidate = plan.primary_candidate().unwrap();

        assert!(
            !candidate
                .runtime_features
                .openai_reasoning_content_repair_enabled
        );
        assert_eq!(
            candidate
                .runtime_features
                .openai_reasoning_content_repair_source,
            RuntimeFeatureConfigSource::ProviderDefault
        );
    }

    #[test]
    fn runtime_feature_model_override_true_overrides_provider_false() {
        let mut catalog = catalog();
        catalog
            .runtime_feature_configs
            .push(provider_runtime_feature_config(
                703,
                1,
                RuntimeFeatureKey::OpenAiReasoningContentRepair,
                false,
            ));
        catalog
            .runtime_feature_configs
            .push(model_runtime_feature_config(
                704,
                10,
                RuntimeFeatureKey::OpenAiReasoningContentRepair,
                true,
            ));

        let plan = build_execution_plan_from_catalog(&catalog, 42, "openai/gpt-primary")
            .expect("direct model should resolve");
        let candidate = plan.primary_candidate().unwrap();

        assert!(
            candidate
                .runtime_features
                .openai_reasoning_content_repair_enabled
        );
        assert_eq!(
            candidate
                .runtime_features
                .openai_reasoning_content_repair_source,
            RuntimeFeatureConfigSource::ModelOverride
        );
    }

    #[test]
    fn runtime_feature_model_override_false_overrides_provider_true() {
        let mut catalog = catalog();
        catalog
            .runtime_feature_configs
            .push(provider_runtime_feature_config(
                705,
                1,
                RuntimeFeatureKey::OpenAiReasoningContentRepair,
                true,
            ));
        catalog
            .runtime_feature_configs
            .push(model_runtime_feature_config(
                706,
                10,
                RuntimeFeatureKey::OpenAiReasoningContentRepair,
                false,
            ));

        let plan = build_execution_plan_from_catalog(&catalog, 42, "openai/gpt-primary")
            .expect("direct model should resolve");
        let candidate = plan.primary_candidate().unwrap();

        assert!(
            !candidate
                .runtime_features
                .openai_reasoning_content_repair_enabled
        );
        assert_eq!(
            candidate
                .runtime_features
                .openai_reasoning_content_repair_source,
            RuntimeFeatureConfigSource::ModelOverride
        );
    }

    #[test]
    fn runtime_feature_resolution_does_not_depend_on_reasoning_config() {
        let mut catalog = catalog();
        catalog
            .runtime_feature_configs
            .push(provider_runtime_feature_config(
                707,
                1,
                RuntimeFeatureKey::OpenAiReasoningContentRepair,
                true,
            ));
        assert!(
            catalog.reasoning_configs.is_empty(),
            "fixture intentionally has no reasoning config"
        );

        let plan = build_execution_plan_from_catalog(&catalog, 42, "openai/gpt-primary")
            .expect("direct model should resolve");
        let candidate = plan.primary_candidate().unwrap();

        assert!(
            candidate
                .runtime_features
                .openai_reasoning_content_repair_enabled
        );
        assert!(candidate.reasoning_config_id.is_none());
    }

    #[test]
    fn build_execution_plan_outputs_global_route_candidates_in_order() {
        let catalog = catalog();

        let plan = build_execution_plan_from_catalog(&catalog, 7, "smart-route")
            .expect("global route should resolve");

        assert_eq!(plan.resolved_scope, ResolvedNameScope::GlobalRoute);
        assert_eq!(plan.resolved_route_id, Some(100));
        assert_eq!(plan.resolved_route_name.as_deref(), Some("smart-route"));
        assert_eq!(plan.candidate_model_ids(), vec![10, 30]);
        assert_eq!(plan.candidates[0].candidate_position, 1);
        assert_eq!(plan.candidates[0].route_candidate_priority, Some(10));
        assert_eq!(plan.candidates[1].candidate_position, 2);
        assert_eq!(plan.candidates[1].route_candidate_priority, Some(30));
    }

    #[test]
    fn build_execution_plan_keeps_exact_route_before_reasoning_suffix_parse() {
        let mut catalog = catalog_with_openai_high_reasoning();
        catalog.routes.push(route_with_candidates(
            300,
            "smart-route-high",
            &[(10, 10, true)],
        ));

        let plan = build_execution_plan_from_catalog(&catalog, 7, "smart-route-high")
            .expect("exact route should win");

        assert_eq!(plan.requested_name, "smart-route-high");
        assert_eq!(plan.base_requested_name, "smart-route-high");
        assert_eq!(plan.resolved_reasoning_suffix, None);
        assert_eq!(plan.resolved_reasoning_preset, None);
        assert_eq!(plan.resolved_scope, ResolvedNameScope::GlobalRoute);
        assert_eq!(plan.resolved_route_id, Some(300));
    }

    #[test]
    fn build_execution_plan_resolves_direct_reasoning_suffix_after_exact_miss() {
        let catalog = catalog_with_openai_high_reasoning();

        let plan = build_execution_plan_from_catalog(&catalog, 7, "openai/gpt-primary-high")
            .expect("direct model reasoning suffix should resolve");

        assert_eq!(plan.requested_name, "openai/gpt-primary-high");
        assert_eq!(plan.base_requested_name, "openai/gpt-primary");
        assert_eq!(plan.resolved_reasoning_suffix.as_deref(), Some("high"));
        assert_eq!(plan.resolved_reasoning_preset, Some(ReasoningPreset::High));
        assert_eq!(plan.resolved_scope, ResolvedNameScope::Direct);
        assert_eq!(plan.candidate_model_ids(), vec![10]);
        let candidate = plan.primary_candidate().unwrap();
        assert_eq!(candidate.reasoning_config_id, Some(900));
        assert_eq!(
            candidate.reasoning_config_scope,
            Some(ReasoningConfigScope::Provider)
        );
        assert_eq!(
            candidate.reasoning_config_source,
            Some(ReasoningConfigSource::ProviderDefault)
        );
        assert_eq!(candidate.reasoning_config_preset_id, Some(9000));
        assert_eq!(
            candidate.reasoning_family,
            Some(ReasoningPatchFamily::OpenAiChatReasoningEffort)
        );
        assert_eq!(candidate.reasoning_preset, Some(ReasoningPreset::High));
        assert_eq!(candidate.reasoning_suffix.as_deref(), Some("high"));
    }

    #[test]
    fn build_execution_plan_resolves_route_reasoning_suffix_after_exact_miss() {
        let catalog = catalog_with_openai_high_reasoning();

        let plan = build_execution_plan_from_catalog(&catalog, 7, "smart-route-high")
            .expect("route reasoning suffix should resolve");

        assert_eq!(plan.requested_name, "smart-route-high");
        assert_eq!(plan.base_requested_name, "smart-route");
        assert_eq!(plan.resolved_reasoning_suffix.as_deref(), Some("high"));
        assert_eq!(plan.resolved_reasoning_preset, Some(ReasoningPreset::High));
        assert_eq!(plan.resolved_scope, ResolvedNameScope::GlobalRoute);
        assert_eq!(plan.candidate_model_ids(), vec![10, 30]);
        assert!(plan.candidates.iter().all(|candidate| {
            candidate.reasoning_family == Some(ReasoningPatchFamily::OpenAiChatReasoningEffort)
                && candidate.reasoning_preset == Some(ReasoningPreset::High)
                && candidate.reasoning_suffix.as_deref() == Some("high")
        }));
        let summary = plan.candidate_summary_for_log();
        assert!(summary.contains("base_name=smart-route"), "{summary}");
        assert!(
            summary.contains("reasoning_family=Some(OpenAiChatReasoningEffort)"),
            "{summary}"
        );
        assert!(
            summary.contains("reasoning_suffix=Some(\"high\")"),
            "{summary}"
        );
        assert!(
            summary.contains("runtime_feature_openai_reasoning_content_repair=false/default_false"),
            "{summary}"
        );
    }

    #[test]
    fn route_reasoning_suffix_allows_different_families_with_same_preset() {
        let mut catalog = catalog_with_openai_high_reasoning();
        add_gemini_high_reasoning(&mut catalog);
        catalog.routes[0].candidates[1].is_enabled = true;

        let plan = build_execution_plan_from_catalog(&catalog, 7, "smart-route-high")
            .expect("route should allow different families when preset is stable");

        assert_eq!(plan.candidate_model_ids(), vec![10, 20, 30]);
        assert_eq!(
            plan.candidates[0].reasoning_family,
            Some(ReasoningPatchFamily::OpenAiChatReasoningEffort)
        );
        assert_eq!(
            plan.candidates[1].reasoning_family,
            Some(ReasoningPatchFamily::Gemini25ThinkingBudget)
        );
        assert_eq!(
            plan.candidates
                .iter()
                .map(|candidate| candidate.reasoning_preset)
                .collect::<Vec<_>>(),
            vec![
                Some(ReasoningPreset::High),
                Some(ReasoningPreset::High),
                Some(ReasoningPreset::High)
            ]
        );
    }

    #[test]
    fn build_execution_plan_resolves_override_reasoning_suffix_after_exact_miss() {
        let mut catalog = catalog_with_openai_high_reasoning();
        catalog.api_key_overrides.push(CacheApiKeyModelOverride {
            id: 501,
            api_key_id: 7,
            source_name: "operator-alias".to_string(),
            target_route_id: 100,
            description: None,
            is_enabled: true,
        });

        let plan = build_execution_plan_from_catalog(&catalog, 7, "operator-alias-high")
            .expect("override reasoning suffix should resolve through the base override");

        assert_eq!(plan.requested_name, "operator-alias-high");
        assert_eq!(plan.base_requested_name, "operator-alias");
        assert_eq!(plan.resolved_reasoning_suffix.as_deref(), Some("high"));
        assert_eq!(plan.resolved_reasoning_preset, Some(ReasoningPreset::High));
        assert_eq!(plan.resolved_scope, ResolvedNameScope::ApiKeyOverride);
        assert_eq!(plan.candidate_model_ids(), vec![10, 30]);
    }

    #[test]
    fn build_execution_plan_returns_clear_error_when_reasoning_suffix_base_is_missing() {
        let catalog = catalog_with_openai_high_reasoning();

        let err = build_execution_plan_from_catalog(&catalog, 7, "openai/missing-high")
            .expect_err("known suffix with missing base should be rejected");

        assert!(err.contains("known reasoning suffix 'high'"), "{err}");
        assert!(err.contains("base model 'openai/missing'"), "{err}");
    }

    #[test]
    fn build_execution_plan_does_not_downgrade_unknown_suffix_to_base_model() {
        let catalog = catalog_with_openai_high_reasoning();

        let err = build_execution_plan_from_catalog(&catalog, 7, "openai/gpt-primary-ultra")
            .expect_err("unknown suffix should stay an exact miss");

        assert!(err.contains("openai/gpt-primary-ultra"), "{err}");
        assert!(!err.contains("known reasoning suffix"), "{err}");
    }

    #[test]
    fn build_execution_plan_rejects_suffix_when_base_candidate_lacks_config() {
        let mut catalog = catalog_with_openai_high_reasoning();
        catalog.reasoning_configs.clear();
        add_gemini_high_reasoning(&mut catalog);

        let err = build_execution_plan_from_catalog(&catalog, 7, "openai/gpt-primary-high")
            .expect_err("base without config should not support suffix");

        assert!(err.contains("Reasoning suffix 'high'"), "{err}");
        assert!(
            err.contains("does not have an active reasoning config"),
            "{err}"
        );
    }

    #[test]
    fn build_execution_plan_rejects_suffix_when_model_disables_provider_default_config() {
        let mut catalog = catalog_with_openai_high_reasoning();
        catalog
            .reasoning_configs
            .push(model_disabled_reasoning_config(902, 10));

        let err = build_execution_plan_from_catalog(&catalog, 7, "openai/gpt-primary-high")
            .expect_err("model disabled config should block provider default");

        assert!(err.contains("Reasoning suffix 'high'"), "{err}");
        assert!(
            err.contains("model 'gpt-primary' has disabled reasoning suffix config"),
            "{err}"
        );
    }

    #[test]
    fn build_execution_plan_allows_no_think_preset_in_custom_config() {
        let mut catalog = catalog();
        catalog.reasoning_configs.push(provider_reasoning_config(
            900,
            1,
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            &[ReasoningPreset::Disabled],
        ));

        let plan = build_execution_plan_from_catalog(&catalog, 7, "openai/gpt-primary-no-think")
            .expect("no-think should resolve as a custom disabled preset");

        assert_eq!(plan.base_requested_name, "openai/gpt-primary");
        assert_eq!(plan.resolved_reasoning_suffix.as_deref(), Some("no-think"));
        assert_eq!(
            plan.resolved_reasoning_preset,
            Some(ReasoningPreset::Disabled)
        );
        let candidate = plan.primary_candidate().unwrap();
        assert_eq!(
            candidate.reasoning_config_source,
            Some(ReasoningConfigSource::ProviderDefault)
        );
        assert_eq!(candidate.reasoning_config_id, Some(900));
        assert_eq!(candidate.reasoning_config_preset_id, Some(9000));
        assert_eq!(candidate.reasoning_preset, Some(ReasoningPreset::Disabled));
    }

    #[test]
    fn build_execution_plan_model_disabled_config_rejects_no_think_too() {
        let mut catalog = catalog();
        catalog.reasoning_configs.push(provider_reasoning_config(
            900,
            1,
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            &[ReasoningPreset::Disabled],
        ));
        catalog
            .reasoning_configs
            .push(model_disabled_reasoning_config(902, 10));

        let err = build_execution_plan_from_catalog(&catalog, 7, "openai/gpt-primary-no-think")
            .expect_err("model disabled config should reject every suffix");

        assert!(err.contains("Reasoning suffix 'no-think'"), "{err}");
        assert!(
            err.contains("model 'gpt-primary' has disabled reasoning suffix config"),
            "{err}"
        );
    }

    #[test]
    fn build_execution_plan_rejects_route_suffix_when_any_valid_candidate_lacks_preset() {
        let mut catalog = catalog_with_openai_high_reasoning();
        catalog.routes[0].candidates[1].is_enabled = true;

        let err = build_execution_plan_from_catalog(&catalog, 7, "smart-route-high")
            .expect_err("route suffix should fail when one valid candidate lacks high");

        assert!(err.contains("Reasoning suffix 'high'"), "{err}");
        assert!(err.contains("candidate provider 'gemini'"), "{err}");
        assert!(
            err.contains("does not have an active reasoning config"),
            "{err}"
        );
    }

    #[test]
    fn route_supports_reasoning_preset_skips_stale_candidates() {
        let mut catalog = catalog_with_openai_high_reasoning();
        catalog.routes[0] =
            route_with_candidates(100, "smart-route", &[(999, 1, true), (10, 10, true)]);
        let route = &catalog.routes[0];

        let bindings = route_supports_reasoning_preset(&catalog, route, ReasoningPreset::High)
            .expect("stale candidate should be skipped before stability check");

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].config_id, 900);
        assert_eq!(bindings[0].config_scope, ReasoningConfigScope::Provider);
        assert_eq!(
            bindings[0].config_source,
            ReasoningConfigSource::ProviderDefault
        );
        assert_eq!(bindings[0].config_preset_id, 9000);
        assert_eq!(bindings[0].preset, ReasoningPreset::High);
    }

    #[test]
    fn same_suffix_with_different_preset_key_does_not_pass_route_stability() {
        let mut catalog = catalog_with_openai_high_reasoning();
        catalog.reasoning_configs.push(model_reasoning_config(
            901,
            30,
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            &[ReasoningPreset::Medium],
        ));

        let err = build_execution_plan_from_catalog(&catalog, 7, "smart-route-high")
            .expect_err("suffix must resolve to the same preset key for every candidate");

        assert!(err.contains("preset 'high'"), "{err}");
        assert!(err.contains("does not enable preset 'high'"), "{err}");
    }

    #[test]
    fn build_execution_plan_outputs_override_route_before_global_route() {
        let catalog = catalog();

        let plan = build_execution_plan_from_catalog(&catalog, 42, "smart-route")
            .expect("api key override should resolve");

        assert_eq!(plan.resolved_scope, ResolvedNameScope::ApiKeyOverride);
        assert_eq!(plan.resolved_route_id, Some(200));
        assert_eq!(plan.resolved_route_name.as_deref(), Some("override-route"));
        assert_eq!(plan.candidate_model_ids(), vec![20, 10]);
        assert_eq!(plan.candidates[0].candidate_position, 1);
        assert_eq!(plan.candidates[0].llm_api_type, LlmApiType::Gemini);
        assert_eq!(plan.candidates[1].candidate_position, 2);
    }

    #[test]
    fn build_execution_plan_skips_stale_route_candidates_and_keeps_valid_order() {
        let mut catalog = catalog();
        catalog.routes[0] = route_with_candidates(
            100,
            "smart-route",
            &[(999, 5, true), (10, 10, true), (30, 30, true)],
        );

        let plan = build_execution_plan_from_catalog(&catalog, 7, "smart-route")
            .expect("route should skip stale candidates");

        assert_eq!(plan.resolved_scope, ResolvedNameScope::GlobalRoute);
        assert_eq!(plan.candidate_model_ids(), vec![10, 30]);
        assert_eq!(plan.candidates[0].candidate_position, 1);
        assert_eq!(plan.candidates[0].route_candidate_priority, Some(10));
        assert_eq!(plan.candidates[1].candidate_position, 2);
        assert_eq!(plan.candidates[1].route_candidate_priority, Some(30));
    }
}
