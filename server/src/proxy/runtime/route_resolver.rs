use std::{fmt, sync::Arc};

use crate::{
    database::reasoning_config::{
        ReasoningConfigMode, ReasoningConfigScope, ReasoningPatchFamily, ReasoningPreset,
    },
    database::runtime_feature_config::{RuntimeFeatureConfigScope, RuntimeFeatureKey},
    schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol},
    service::{
        app_state::AppState,
        cache::types::{
            CacheModel, CacheModelsCatalog, CacheProvider, CacheReasoningConfig,
            CacheUpstreamSource,
        },
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
    pub upstream_source: Arc<CacheUpstreamSource>,
    pub downstream_protocol: DownstreamProtocol,
    pub upstream_protocol: UpstreamProtocol,
    pub selection_reason: SourceSelectionReason,
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
pub enum SourceSelectionReason {
    ProtocolMatch,
    DefaultTransform,
}

impl SourceSelectionReason {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::ProtocolMatch => "protocol_match",
            Self::DefaultTransform => "default_transform",
        }
    }
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
    #[cfg(test)]
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
    pub(in crate::proxy) fn is_openai_compatible_generation(&self) -> bool {
        self.upstream_protocol == UpstreamProtocol::Openai
            && matches!(
                self.upstream_source.profile_type,
                UpstreamProfileType::Openai
                    | UpstreamProfileType::VertexOpenai
                    | UpstreamProfileType::GeminiOpenai
            )
    }

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
    ProviderConfiguration(String),
    CatalogUnavailable(String),
}

impl ExecutionPlanBuildError {
    fn with_message(self, message: String) -> Self {
        match self {
            Self::InvalidModelFormat(_) => Self::InvalidModelFormat(message),
            Self::TargetNotFound(_) => Self::TargetNotFound(message),
            Self::UnsupportedCapability(_) => Self::UnsupportedCapability(message),
            Self::ProviderConfiguration(_) => Self::ProviderConfiguration(message),
            Self::CatalogUnavailable(_) => Self::CatalogUnavailable(message),
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::InvalidModelFormat(message)
            | Self::TargetNotFound(message)
            | Self::UnsupportedCapability(message)
            | Self::ProviderConfiguration(message)
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
    #[cfg(test)]
    pub fn target_summary_for_log(&self) -> String {
        let target = &self.target;
        format!(
            "base_name={}; provider={}/{}; model={}/{}; source={}({:?}); downstream_protocol={:?}; upstream_protocol={:?}; selection_reason={}; reasoning_suffix={:?}; runtime_feature_openai_reasoning_content_repair={}/{}",
            self.base_requested_name,
            target.provider.id,
            target.provider.provider_key,
            target.model.id,
            target.model.model_name,
            target.upstream_source.id,
            target.upstream_source.profile_type,
            target.downstream_protocol,
            target.upstream_protocol,
            target.selection_reason.as_key(),
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
    downstream_protocol: DownstreamProtocol,
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
    let (selected_source, selection_reason) = select_source(&provider, downstream_protocol)?;
    // Freeze the selected source before resolving Provider/Model-scoped runtime
    // configuration so every later stage observes the same execution entry.
    let upstream_source = Arc::new(selected_source);
    let upstream_protocol = determine_upstream_protocol(&upstream_source);
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
            upstream_source,
            downstream_protocol,
            upstream_protocol,
            selection_reason,
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

fn downstream_wire_family(protocol: DownstreamProtocol) -> UpstreamProtocol {
    match protocol {
        DownstreamProtocol::Openai => UpstreamProtocol::Openai,
        DownstreamProtocol::Responses => UpstreamProtocol::Responses,
        DownstreamProtocol::Anthropic => UpstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini => UpstreamProtocol::Gemini,
    }
}

fn select_source(
    provider: &CacheProvider,
    downstream_protocol: DownstreamProtocol,
) -> Result<(CacheUpstreamSource, SourceSelectionReason), ExecutionPlanBuildError> {
    let mut family_counts = std::collections::HashMap::<UpstreamProtocol, usize>::new();
    let mut default_count = 0usize;
    for source in &provider.upstream_sources {
        let family = determine_upstream_protocol(source);
        *family_counts.entry(family).or_default() += 1;
        if source.is_default {
            default_count += 1;
        }
    }
    if family_counts.values().any(|count| *count > 1) || default_count > 1 {
        return Err(ExecutionPlanBuildError::ProviderConfiguration(format!(
            "Provider '{}' has duplicate active Source family/default state",
            provider.provider_key
        )));
    }

    let desired_family = downstream_wire_family(downstream_protocol);
    let exact = provider
        .upstream_sources
        .iter()
        .filter(|source| source.is_enabled && determine_upstream_protocol(source) == desired_family)
        .cloned()
        .collect::<Vec<_>>();
    match exact.as_slice() {
        [source] => return Ok((source.clone(), SourceSelectionReason::ProtocolMatch)),
        [] => {}
        _ => {
            return Err(ExecutionPlanBuildError::ProviderConfiguration(format!(
                "Provider '{}' has multiple enabled Sources for downstream protocol {:?}",
                provider.provider_key, downstream_protocol
            )));
        }
    }

    let defaults = provider
        .upstream_sources
        .iter()
        .filter(|source| source.is_enabled && source.is_default)
        .cloned()
        .collect::<Vec<_>>();
    match defaults.as_slice() {
        [source] => Ok((source.clone(), SourceSelectionReason::DefaultTransform)),
        [] => Err(ExecutionPlanBuildError::ProviderConfiguration(format!(
            "Provider '{}' has no enabled Source matching downstream protocol {:?} and no enabled default Source",
            provider.provider_key, downstream_protocol
        ))),
        _ => Err(ExecutionPlanBuildError::ProviderConfiguration(format!(
            "Provider '{}' has multiple enabled default Sources",
            provider.provider_key
        ))),
    }
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
    downstream_protocol: DownstreamProtocol,
) -> Result<ExecutionPlan, ExecutionPlanBuildError> {
    match build_direct_execution_plan(catalog, requested_name, downstream_protocol) {
        Ok(plan) => Ok(plan),
        Err(exact_error) => {
            let suffixes = enabled_reasoning_suffixes(catalog);
            let Some(resolved_name) = parse_reasoning_suffix(requested_name, &suffixes) else {
                return Err(exact_error);
            };
            let mut plan = build_direct_execution_plan(
                catalog,
                &resolved_name.base_requested_name,
                downstream_protocol,
            )
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
    downstream_protocol: DownstreamProtocol,
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
    build_execution_plan_from_catalog(catalog.as_ref(), requested_name, downstream_protocol)
}

#[cfg(test)]
mod execution_plan_error_tests {
    use super::{
        ExecutionPlanBuildError, SourceSelectionReason, build_execution_plan_from_catalog,
        select_source,
    };
    use crate::{
        schema::enum_def::{
            DownstreamProtocol, ProviderApiKeyMode, UpstreamProfileType, UpstreamProtocol,
        },
        service::cache::types::{
            CacheModel, CacheModelsCatalog, CacheProvider, CacheUpstreamSource,
        },
    };

    fn empty_catalog() -> CacheModelsCatalog {
        CacheModelsCatalog {
            providers: vec![],
            models: vec![],
            reasoning_configs: vec![],
            runtime_feature_configs: vec![],
        }
    }

    fn direct_catalog(provider_enabled: bool) -> CacheModelsCatalog {
        let mut catalog = empty_catalog();
        catalog.providers.push(CacheProvider {
            id: 11,
            provider_key: "gemini-openai".to_string(),
            name: "Gemini OpenAI".to_string(),
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: provider_enabled,
            upstream_sources: vec![CacheUpstreamSource {
                id: 12,
                profile_type: UpstreamProfileType::GeminiOpenai,
                endpoint: "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
                use_proxy: true,
                is_enabled: true,
                is_default: true,
            }],
        });
        catalog.models.push(CacheModel {
            id: 13,
            provider_id: 11,
            model_name: "gemini-2.5-flash".to_string(),
            real_model_name: None,
            cost_catalog_id: None,
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: true,
            supports_image_input: true,
            supports_embeddings: false,
            supports_rerank: false,
            is_enabled: true,
        });
        catalog
    }

    fn source(
        id: i64,
        profile_type: UpstreamProfileType,
        is_enabled: bool,
        is_default: bool,
    ) -> CacheUpstreamSource {
        CacheUpstreamSource {
            id,
            profile_type,
            endpoint: format!("https://source-{id}.example.com"),
            use_proxy: false,
            is_enabled,
            is_default,
        }
    }

    fn provider_with_sources(sources: Vec<CacheUpstreamSource>) -> CacheProvider {
        CacheProvider {
            id: 21,
            provider_key: "multi-source".to_string(),
            name: "Multi Source".to_string(),
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
            upstream_sources: sources,
        }
    }

    #[test]
    fn malformed_model_name_is_a_parse_error() {
        let error = build_execution_plan_from_catalog(
            &empty_catalog(),
            "gpt-4o",
            DownstreamProtocol::Openai,
        )
        .expect_err("provider prefix is required");

        assert!(matches!(
            error,
            ExecutionPlanBuildError::InvalidModelFormat(_)
        ));
    }

    #[test]
    fn missing_direct_target_is_not_a_parse_error() {
        let error = build_execution_plan_from_catalog(
            &empty_catalog(),
            "openai/gpt-4o",
            DownstreamProtocol::Openai,
        )
        .expect_err("missing provider should fail resolution");

        assert!(matches!(error, ExecutionPlanBuildError::TargetNotFound(_)));
    }

    #[test]
    fn direct_plan_freezes_selected_source_before_runtime_config_resolution() {
        let mut catalog = direct_catalog(true);

        let plan = build_execution_plan_from_catalog(
            &catalog,
            "gemini-openai/gemini-2.5-flash",
            DownstreamProtocol::Openai,
        )
        .expect("direct plan should resolve");
        catalog.providers[0].upstream_sources[0].endpoint = "https://changed.invalid".to_string();

        assert_eq!(plan.target.upstream_source.id, 12);
        assert_eq!(
            plan.target.upstream_source.endpoint,
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
        assert_eq!(plan.target.upstream_protocol, UpstreamProtocol::Openai);
        assert!(plan.target.is_openai_compatible_generation());
        assert!(
            plan.target_summary_for_log()
                .contains("source=12(GeminiOpenai)")
        );
    }

    #[test]
    fn disabled_logical_provider_cannot_resolve_its_source_or_model() {
        let error = build_execution_plan_from_catalog(
            &direct_catalog(false),
            "gemini-openai/gemini-2.5-flash",
            DownstreamProtocol::Openai,
        )
        .expect_err("disabled provider must fail before execution");

        assert!(matches!(error, ExecutionPlanBuildError::TargetNotFound(_)));
    }

    #[test]
    fn selector_maps_all_downstream_protocols_to_exact_wire_families() {
        let provider = provider_with_sources(vec![
            source(31, UpstreamProfileType::Openai, true, true),
            source(32, UpstreamProfileType::Responses, true, false),
            source(33, UpstreamProfileType::Anthropic, true, false),
            source(34, UpstreamProfileType::Gemini, true, false),
            source(35, UpstreamProfileType::Ollama, true, false),
        ]);

        for (downstream_protocol, expected_source_id) in [
            (DownstreamProtocol::Openai, 31),
            (DownstreamProtocol::Responses, 32),
            (DownstreamProtocol::Anthropic, 33),
            (DownstreamProtocol::Gemini, 34),
        ] {
            let (selected, reason) =
                select_source(&provider, downstream_protocol).expect("exact Source should win");
            assert_eq!(selected.id, expected_source_id);
            assert_eq!(reason, SourceSelectionReason::ProtocolMatch);
        }
    }

    #[test]
    fn selector_uses_enabled_default_when_exact_source_is_disabled() {
        let provider = provider_with_sources(vec![
            source(41, UpstreamProfileType::Openai, false, false),
            source(42, UpstreamProfileType::Ollama, true, true),
        ]);

        let (selected, reason) =
            select_source(&provider, DownstreamProtocol::Openai).expect("default should apply");
        assert_eq!(selected.id, 42);
        assert_eq!(reason, SourceSelectionReason::DefaultTransform);
    }

    #[test]
    fn selector_fails_closed_for_zero_sources_and_missing_default() {
        let zero_source = provider_with_sources(vec![]);
        assert!(matches!(
            select_source(&zero_source, DownstreamProtocol::Openai),
            Err(ExecutionPlanBuildError::ProviderConfiguration(_))
        ));

        let no_default = provider_with_sources(vec![source(
            51,
            UpstreamProfileType::Responses,
            true,
            false,
        )]);
        assert!(matches!(
            select_source(&no_default, DownstreamProtocol::Openai),
            Err(ExecutionPlanBuildError::ProviderConfiguration(_))
        ));
    }

    #[test]
    fn selector_fails_closed_for_duplicate_family_or_default_state() {
        let duplicate_family = provider_with_sources(vec![
            source(61, UpstreamProfileType::Openai, true, true),
            source(62, UpstreamProfileType::GeminiOpenai, false, false),
        ]);
        assert!(matches!(
            select_source(&duplicate_family, DownstreamProtocol::Openai),
            Err(ExecutionPlanBuildError::ProviderConfiguration(_))
        ));

        let duplicate_default = provider_with_sources(vec![
            source(63, UpstreamProfileType::Responses, true, true),
            source(64, UpstreamProfileType::Ollama, true, true),
        ]);
        assert!(matches!(
            select_source(&duplicate_default, DownstreamProtocol::Openai),
            Err(ExecutionPlanBuildError::ProviderConfiguration(_))
        ));
    }
}
