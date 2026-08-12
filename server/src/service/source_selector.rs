use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::service::cache::types::{
    CacheModel, CacheModelSourceBinding, CacheProvider, CacheUpstreamSource,
};
use crate::service::upstream_profile::upstream_runtime_profile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceSelectionReason {
    ProtocolMatch,
    ProviderDefaultTransform,
    ModelDefaultTransform,
}

impl SourceSelectionReason {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::ProtocolMatch => "protocol_match",
            Self::ProviderDefaultTransform => "provider_default_transform",
            Self::ModelDefaultTransform => "model_default_transform",
        }
    }

    pub fn transform_required(self) -> bool {
        !matches!(self, Self::ProtocolMatch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceSelectionFailure {
    InvalidModeBindingState,
    BindingProviderMismatch,
    BindingSourceMissing,
    DuplicateSource,
    DuplicateDefault,
    ExplicitEmpty,
    NoEnabledSource,
    NoProtocolMatch,
    ProviderDefaultMissing,
    ProviderDefaultUnavailable,
    ModelDefaultMissing,
    ModelDefaultUnavailable,
}

impl SourceSelectionFailure {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::InvalidModeBindingState => "invalid_mode_binding_state",
            Self::BindingProviderMismatch => "binding_provider_mismatch",
            Self::BindingSourceMissing => "binding_source_missing",
            Self::DuplicateSource => "duplicate_source",
            Self::DuplicateDefault => "duplicate_default",
            Self::ExplicitEmpty => "explicit_empty",
            Self::NoEnabledSource => "no_enabled_source",
            Self::NoProtocolMatch => "no_protocol_match",
            Self::ProviderDefaultMissing => "provider_default_missing",
            Self::ProviderDefaultUnavailable => "provider_default_unavailable",
            Self::ModelDefaultMissing => "model_default_missing",
            Self::ModelDefaultUnavailable => "model_default_unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSelectionTraceEntry {
    pub key: String,
    pub source_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSelectionError {
    pub failure: SourceSelectionFailure,
    pub trace: Vec<SourceSelectionTraceEntry>,
}

impl SourceSelectionError {
    fn new(failure: SourceSelectionFailure, trace: Vec<SourceSelectionTraceEntry>) -> Self {
        Self { failure, trace }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSelection {
    pub source: CacheUpstreamSource,
    pub reason: SourceSelectionReason,
    pub transform_required: bool,
    pub trace: Vec<SourceSelectionTraceEntry>,
}

pub fn downstream_wire_family(protocol: DownstreamProtocol) -> UpstreamProtocol {
    match protocol {
        DownstreamProtocol::Openai => UpstreamProtocol::Openai,
        DownstreamProtocol::Responses => UpstreamProtocol::Responses,
        DownstreamProtocol::Anthropic => UpstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini => UpstreamProtocol::Gemini,
    }
}

pub fn upstream_wire_family(source: &CacheUpstreamSource) -> UpstreamProtocol {
    upstream_runtime_profile(&source.profile_type).upstream_protocol
}

pub fn select_source(
    provider: &CacheProvider,
    model: &CacheModel,
    downstream_protocol: DownstreamProtocol,
) -> Result<SourceSelection, SourceSelectionError> {
    select_source_with_sources(
        provider,
        model,
        downstream_protocol,
        &provider.upstream_sources,
    )
}

pub fn select_source_with_sources(
    provider: &CacheProvider,
    model: &CacheModel,
    downstream_protocol: DownstreamProtocol,
    sources: &[CacheUpstreamSource],
) -> Result<SourceSelection, SourceSelectionError> {
    let mut trace = Vec::new();

    if model.provider_id != provider.id {
        return Err(SourceSelectionError::new(
            SourceSelectionFailure::BindingProviderMismatch,
            trace,
        ));
    }

    let mode = model.source_selection_mode.as_str();
    if mode != "INHERIT_ALL" && mode != "EXPLICIT" {
        return Err(SourceSelectionError::new(
            SourceSelectionFailure::InvalidModeBindingState,
            trace,
        ));
    }

    validate_source_inventory(sources, &mut trace)?;
    let source_by_id = sources
        .iter()
        .map(|source| (source.id, source))
        .collect::<HashMap<_, _>>();

    let declared = if mode == "INHERIT_ALL" {
        if !model.source_bindings.is_empty() {
            return Err(SourceSelectionError::new(
                SourceSelectionFailure::InvalidModeBindingState,
                trace,
            ));
        }
        sources.iter().collect::<Vec<_>>()
    } else {
        if model.source_bindings.is_empty() {
            trace.push(SourceSelectionTraceEntry {
                key: SourceSelectionFailure::ExplicitEmpty.as_key().to_string(),
                source_ids: Vec::new(),
            });
            return Err(SourceSelectionError::new(
                SourceSelectionFailure::ExplicitEmpty,
                trace,
            ));
        }

        let mut binding_ids = HashSet::new();
        let mut default_count = 0;
        let mut declared = Vec::with_capacity(model.source_bindings.len());
        for binding in &model.source_bindings {
            if !binding_ids.insert(binding.source_id) {
                return Err(SourceSelectionError::new(
                    SourceSelectionFailure::DuplicateSource,
                    trace,
                ));
            }
            if binding.is_default {
                default_count += 1;
            }
            let Some(source) = source_by_id.get(&binding.source_id).copied() else {
                return Err(SourceSelectionError::new(
                    SourceSelectionFailure::BindingSourceMissing,
                    trace,
                ));
            };
            declared.push(source);
        }
        if default_count > 1 {
            return Err(SourceSelectionError::new(
                SourceSelectionFailure::DuplicateDefault,
                trace,
            ));
        }
        declared
    };

    let declared_ids = declared.iter().map(|source| source.id).collect::<Vec<_>>();
    trace.push(SourceSelectionTraceEntry {
        key: format!("declared_{}", mode.to_ascii_lowercase()),
        source_ids: declared_ids,
    });

    let desired_family = downstream_wire_family(downstream_protocol);
    let enabled = declared
        .iter()
        .filter(|source| source.is_enabled)
        .copied()
        .collect::<Vec<_>>();
    let exact = enabled
        .iter()
        .filter(|source| upstream_wire_family(source) == desired_family)
        .copied()
        .collect::<Vec<_>>();

    if let [source] = exact.as_slice() {
        trace.push(SourceSelectionTraceEntry {
            key: "protocol_match".to_string(),
            source_ids: vec![source.id],
        });
        return Ok(selection(
            source,
            SourceSelectionReason::ProtocolMatch,
            trace,
        ));
    }
    if exact.len() > 1 {
        return Err(SourceSelectionError::new(
            SourceSelectionFailure::InvalidModeBindingState,
            trace,
        ));
    }

    if enabled.is_empty() {
        trace.push(SourceSelectionTraceEntry {
            key: SourceSelectionFailure::NoEnabledSource.as_key().to_string(),
            source_ids: Vec::new(),
        });
    } else {
        trace.push(SourceSelectionTraceEntry {
            key: SourceSelectionFailure::NoProtocolMatch.as_key().to_string(),
            source_ids: enabled.iter().map(|source| source.id).collect(),
        });
    }

    if mode == "INHERIT_ALL" {
        let defaults = declared
            .iter()
            .filter(|source| source.is_default)
            .copied()
            .collect::<Vec<_>>();
        return select_default(
            defaults,
            SourceSelectionReason::ProviderDefaultTransform,
            SourceSelectionFailure::ProviderDefaultMissing,
            SourceSelectionFailure::ProviderDefaultUnavailable,
            trace,
        );
    }

    let default_ids = model
        .source_bindings
        .iter()
        .filter(|binding| binding.is_default)
        .map(|binding| binding.source_id)
        .collect::<Vec<_>>();
    if default_ids.is_empty() {
        trace.push(SourceSelectionTraceEntry {
            key: SourceSelectionFailure::ModelDefaultMissing
                .as_key()
                .to_string(),
            source_ids: Vec::new(),
        });
        return Err(SourceSelectionError::new(
            SourceSelectionFailure::ModelDefaultMissing,
            trace,
        ));
    }
    let defaults = default_ids
        .iter()
        .filter_map(|source_id| source_by_id.get(source_id).copied())
        .collect::<Vec<_>>();
    select_default(
        defaults,
        SourceSelectionReason::ModelDefaultTransform,
        SourceSelectionFailure::ModelDefaultMissing,
        SourceSelectionFailure::ModelDefaultUnavailable,
        trace,
    )
}

fn validate_source_inventory(
    sources: &[CacheUpstreamSource],
    trace: &mut Vec<SourceSelectionTraceEntry>,
) -> Result<(), SourceSelectionError> {
    let mut ids = HashSet::new();
    let mut family_counts = HashMap::<UpstreamProtocol, usize>::new();
    let mut default_count = 0usize;
    for source in sources {
        if !ids.insert(source.id) {
            return Err(SourceSelectionError::new(
                SourceSelectionFailure::DuplicateSource,
                trace.clone(),
            ));
        }
        *family_counts
            .entry(upstream_wire_family(source))
            .or_default() += 1;
        if source.is_default {
            default_count += 1;
        }
    }
    if family_counts.values().any(|count| *count > 1) {
        return Err(SourceSelectionError::new(
            SourceSelectionFailure::InvalidModeBindingState,
            trace.clone(),
        ));
    }
    if default_count > 1 {
        return Err(SourceSelectionError::new(
            SourceSelectionFailure::DuplicateDefault,
            trace.clone(),
        ));
    }
    Ok(())
}

fn select_default(
    defaults: Vec<&CacheUpstreamSource>,
    reason: SourceSelectionReason,
    missing_failure: SourceSelectionFailure,
    unavailable_failure: SourceSelectionFailure,
    mut trace: Vec<SourceSelectionTraceEntry>,
) -> Result<SourceSelection, SourceSelectionError> {
    match defaults.as_slice() {
        [source] if source.is_enabled => {
            trace.push(SourceSelectionTraceEntry {
                key: reason.as_key().to_string(),
                source_ids: vec![source.id],
            });
            Ok(selection(source, reason, trace))
        }
        [] => {
            trace.push(SourceSelectionTraceEntry {
                key: missing_failure.as_key().to_string(),
                source_ids: Vec::new(),
            });
            Err(SourceSelectionError::new(missing_failure, trace))
        }
        sources => {
            trace.push(SourceSelectionTraceEntry {
                key: unavailable_failure.as_key().to_string(),
                source_ids: sources.iter().map(|source| source.id).collect(),
            });
            Err(SourceSelectionError::new(unavailable_failure, trace))
        }
    }
}

fn selection(
    source: &CacheUpstreamSource,
    reason: SourceSelectionReason,
    trace: Vec<SourceSelectionTraceEntry>,
) -> SourceSelection {
    SourceSelection {
        source: source.clone(),
        reason,
        transform_required: reason.transform_required(),
        trace,
    }
}

pub fn source_binding(source_id: i64, is_default: bool) -> CacheModelSourceBinding {
    CacheModelSourceBinding {
        source_id,
        is_default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};

    fn source(
        id: i64,
        profile_type: UpstreamProfileType,
        enabled: bool,
        default: bool,
    ) -> CacheUpstreamSource {
        let (chat_completions_enabled, embeddings_enabled, rerank_enabled) = match profile_type {
            UpstreamProfileType::Openai | UpstreamProfileType::GeminiOpenai => {
                (Some(true), Some(true), Some(false))
            }
            UpstreamProfileType::OpenaiCompatible => (Some(true), Some(false), Some(false)),
            _ => (None, None, None),
        };
        CacheUpstreamSource {
            id,
            profile_type,
            base_url: format!("https://source-{id}.example.com"),
            use_proxy: false,
            chat_completions_enabled,
            chat_completions_path_override: None,
            embeddings_enabled,
            embeddings_path_override: None,
            rerank_enabled,
            rerank_path_override: None,
            is_enabled: enabled,
            is_default: default,
        }
    }

    fn provider(sources: Vec<CacheUpstreamSource>) -> CacheProvider {
        CacheProvider {
            id: 10,
            provider_key: "provider".to_string(),
            name: "Provider".to_string(),
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
            upstream_sources: sources,
        }
    }

    fn model(mode: &str, bindings: Vec<CacheModelSourceBinding>) -> CacheModel {
        CacheModel {
            id: 20,
            provider_id: 10,
            model_name: "model".to_string(),
            real_model_name: None,
            model_kind: crate::schema::enum_def::ModelKind::Chat,
            cost_catalog_id: None,
            source_selection_mode: mode.to_string(),
            source_bindings: bindings,
            is_enabled: true,
        }
    }

    #[test]
    fn inherit_exact_match_precedes_provider_default() {
        let provider = provider(vec![
            source(1, UpstreamProfileType::Openai, true, true),
            source(2, UpstreamProfileType::Anthropic, true, false),
        ]);
        let selected = select_source(
            &provider,
            &model("INHERIT_ALL", vec![]),
            DownstreamProtocol::Anthropic,
        )
        .expect("exact source should be selected");
        assert_eq!(selected.source.id, 2);
        assert_eq!(selected.reason, SourceSelectionReason::ProtocolMatch);
    }

    #[test]
    fn explicit_default_is_the_only_fallback_and_disabled_is_unavailable() {
        let provider = provider(vec![
            source(1, UpstreamProfileType::Openai, true, true),
            source(2, UpstreamProfileType::Anthropic, false, false),
        ]);
        let selected = select_source(
            &provider,
            &model(
                "EXPLICIT",
                vec![source_binding(2, true), source_binding(1, false)],
            ),
            DownstreamProtocol::Gemini,
        )
        .expect_err("disabled model default must not fall back to another source");
        assert_eq!(
            selected.failure,
            SourceSelectionFailure::ModelDefaultUnavailable
        );
    }

    #[test]
    fn explicit_empty_and_singleton_without_match_are_fail_closed() {
        let provider = provider(vec![source(1, UpstreamProfileType::Openai, true, true)]);
        let empty = select_source(
            &provider,
            &model("EXPLICIT", vec![]),
            DownstreamProtocol::Anthropic,
        )
        .expect_err("explicit empty must be rejected");
        assert_eq!(empty.failure, SourceSelectionFailure::ExplicitEmpty);

        let singleton = select_source(
            &provider,
            &model("EXPLICIT", vec![source_binding(1, false)]),
            DownstreamProtocol::Anthropic,
        )
        .expect_err("singleton must not become a fallback");
        assert_eq!(
            singleton.failure,
            SourceSelectionFailure::ModelDefaultMissing
        );
    }

    #[test]
    fn selector_matrix_covers_all_public_protocols_and_both_modes() {
        let sources = vec![
            source(1, UpstreamProfileType::Openai, true, true),
            source(2, UpstreamProfileType::Responses, true, false),
            source(3, UpstreamProfileType::Anthropic, true, false),
            source(4, UpstreamProfileType::Gemini, true, false),
        ];
        let provider = provider(sources);
        let explicit_bindings = vec![
            source_binding(1, false),
            source_binding(2, false),
            source_binding(3, false),
            source_binding(4, true),
        ];

        for (protocol, expected_source_id) in [
            (DownstreamProtocol::Openai, 1),
            (DownstreamProtocol::Responses, 2),
            (DownstreamProtocol::Anthropic, 3),
            (DownstreamProtocol::Gemini, 4),
        ] {
            let inherited = select_source(&provider, &model("INHERIT_ALL", Vec::new()), protocol)
                .expect("inherit mode should select the exact family");
            assert_eq!(inherited.source.id, expected_source_id);
            assert_eq!(inherited.reason, SourceSelectionReason::ProtocolMatch);

            let explicit = select_source(
                &provider,
                &model("EXPLICIT", explicit_bindings.clone()),
                protocol,
            )
            .expect("explicit mode should select the exact family");
            assert_eq!(explicit.source.id, expected_source_id);
            assert_eq!(explicit.reason, SourceSelectionReason::ProtocolMatch);
        }
    }

    #[test]
    fn selector_simulation_override_uses_the_same_algorithm_as_production() {
        let provider = provider(vec![source(1, UpstreamProfileType::Openai, true, true)]);
        let simulated_sources = vec![source(2, UpstreamProfileType::Anthropic, true, false)];
        let selected = select_source_with_sources(
            &provider,
            &model("INHERIT_ALL", Vec::new()),
            DownstreamProtocol::Anthropic,
            &simulated_sources,
        )
        .expect("simulation source should be selected by the shared algorithm");

        assert_eq!(selected.source.id, 2);
        assert_eq!(selected.reason, SourceSelectionReason::ProtocolMatch);
    }

    #[test]
    fn explicit_simulation_without_deleted_binding_can_select_remaining_source() {
        let provider = provider(vec![
            source(1, UpstreamProfileType::Openai, true, true),
            source(2, UpstreamProfileType::Anthropic, true, false),
        ]);
        let simulated_model = model("EXPLICIT", vec![source_binding(2, false)]);
        let simulated_sources = vec![source(2, UpstreamProfileType::Anthropic, true, false)];

        let selected = select_source_with_sources(
            &provider,
            &simulated_model,
            DownstreamProtocol::Anthropic,
            &simulated_sources,
        )
        .expect("the remaining declared Source should remain selectable");

        assert_eq!(selected.source.id, 2);
    }

    #[test]
    fn selector_reports_each_mode_specific_default_reason() {
        let provider = provider(vec![source(1, UpstreamProfileType::Openai, true, true)]);
        let inherited = select_source(
            &provider,
            &model("INHERIT_ALL", Vec::new()),
            DownstreamProtocol::Anthropic,
        )
        .expect("provider default should be the inherit fallback");
        assert_eq!(
            inherited.reason,
            SourceSelectionReason::ProviderDefaultTransform
        );

        let explicit = select_source(
            &provider,
            &model("EXPLICIT", vec![source_binding(1, true)]),
            DownstreamProtocol::Anthropic,
        )
        .expect("model default should be the explicit fallback");
        assert_eq!(
            explicit.reason,
            SourceSelectionReason::ModelDefaultTransform
        );
    }

    #[test]
    fn selector_fails_closed_for_disabled_inventory_and_invalid_snapshots() {
        let disabled_provider = provider(vec![source(1, UpstreamProfileType::Openai, false, true)]);
        let disabled = select_source(
            &disabled_provider,
            &model("INHERIT_ALL", Vec::new()),
            DownstreamProtocol::Anthropic,
        )
        .expect_err("a disabled provider default must not be selected");
        assert_eq!(
            disabled.failure,
            SourceSelectionFailure::ProviderDefaultUnavailable
        );

        let invalid_mode = select_source(
            &provider(vec![source(1, UpstreamProfileType::Openai, true, false)]),
            &model("UNKNOWN", Vec::new()),
            DownstreamProtocol::Openai,
        )
        .expect_err("unknown mode must be rejected");
        assert_eq!(
            invalid_mode.failure,
            SourceSelectionFailure::InvalidModeBindingState
        );

        let duplicate_default = select_source(
            &provider(vec![
                source(1, UpstreamProfileType::Openai, true, false),
                source(2, UpstreamProfileType::Anthropic, true, false),
            ]),
            &model(
                "EXPLICIT",
                vec![source_binding(1, true), source_binding(2, true)],
            ),
            DownstreamProtocol::Gemini,
        )
        .expect_err("two model defaults must be rejected");
        assert_eq!(
            duplicate_default.failure,
            SourceSelectionFailure::DuplicateDefault
        );

        let duplicate_source = select_source(
            &provider(vec![source(1, UpstreamProfileType::Openai, true, false)]),
            &model(
                "EXPLICIT",
                vec![source_binding(1, false), source_binding(1, false)],
            ),
            DownstreamProtocol::Openai,
        )
        .expect_err("duplicate model bindings must be rejected");
        assert_eq!(
            duplicate_source.failure,
            SourceSelectionFailure::DuplicateSource
        );
    }
}
