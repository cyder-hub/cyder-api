use crate::{
    database::reasoning_config::{ReasoningConfigMode, ReasoningPreset},
    service::cache::types::CacheModelsCatalog,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestedModelParseStatus {
    Exact,
    ReasoningSuffix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRequestedModelName {
    pub original_requested_name: String,
    pub base_requested_name: String,
    pub requested_suffix: Option<String>,
    pub requested_preset: Option<ReasoningPreset>,
    pub parse_status: RequestedModelParseStatus,
}

impl ResolvedRequestedModelName {
    fn reasoning_suffix(
        requested_name: &str,
        base_requested_name: String,
        suffix: &str,
        preset: ReasoningPreset,
    ) -> Self {
        Self {
            original_requested_name: requested_name.to_string(),
            base_requested_name,
            requested_suffix: Some(suffix.to_string()),
            requested_preset: Some(preset),
            parse_status: RequestedModelParseStatus::ReasoningSuffix,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReasoningSuffixDefinition {
    pub suffix: String,
    pub preset: ReasoningPreset,
}

pub(crate) fn enabled_reasoning_suffixes(
    catalog: &CacheModelsCatalog,
) -> Vec<ReasoningSuffixDefinition> {
    let mut suffixes = Vec::new();

    for config in &catalog.reasoning_configs {
        if !matches!(config.mode, ReasoningConfigMode::Custom) {
            continue;
        }

        for preset in &config.presets {
            if !preset.is_enabled {
                continue;
            }

            let suffix = preset.preset.canonical_suffix();
            if suffixes
                .iter()
                .any(|definition: &ReasoningSuffixDefinition| definition.suffix == suffix)
            {
                continue;
            }

            suffixes.push(ReasoningSuffixDefinition {
                suffix: suffix.to_string(),
                preset: preset.preset,
            });
        }
    }

    suffixes.sort_by(|left, right| {
        right
            .suffix
            .len()
            .cmp(&left.suffix.len())
            .then_with(|| left.suffix.cmp(&right.suffix))
    });
    suffixes
}

pub(crate) fn parse_reasoning_suffix(
    requested_name: &str,
    suffixes: &[ReasoningSuffixDefinition],
) -> Option<ResolvedRequestedModelName> {
    let (direct_provider, suffix_target) = requested_name
        .split_once('/')
        .map_or((None, requested_name), |(provider, model)| {
            (Some(provider), model)
        });

    for definition in suffixes {
        let suffix_marker = format!("-{}", definition.suffix);
        let Some(base_target) = suffix_target.strip_suffix(&suffix_marker) else {
            continue;
        };
        if base_target.is_empty() {
            continue;
        }

        let base_requested_name = match direct_provider {
            Some(provider) => format!("{provider}/{base_target}"),
            None => base_target.to_string(),
        };

        return Some(ResolvedRequestedModelName::reasoning_suffix(
            requested_name,
            base_requested_name,
            &definition.suffix,
            definition.preset,
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::cache::types::{
        CacheModelsCatalog, CacheReasoningConfig, CacheReasoningConfigPreset,
    };

    fn suffix(suffix: &str, preset: ReasoningPreset) -> ReasoningSuffixDefinition {
        ReasoningSuffixDefinition {
            suffix: suffix.to_string(),
            preset,
        }
    }

    fn parse_with_builtin_suffixes(requested_name: &str) -> Option<ResolvedRequestedModelName> {
        parse_reasoning_suffix(
            requested_name,
            &[
                suffix("no-think", ReasoningPreset::Disabled),
                suffix("think", ReasoningPreset::Enabled),
                suffix("high", ReasoningPreset::High),
            ],
        )
    }

    #[test]
    fn parse_reasoning_suffix_uses_longest_enabled_suffix() {
        let parsed = parse_with_builtin_suffixes("smart-chat-no-think")
            .expect("no-think should parse before think");

        assert_eq!(parsed.base_requested_name, "smart-chat");
        assert_eq!(parsed.requested_suffix.as_deref(), Some("no-think"));
        assert_eq!(parsed.requested_preset, Some(ReasoningPreset::Disabled));
        assert_eq!(
            parsed.parse_status,
            RequestedModelParseStatus::ReasoningSuffix
        );
    }

    #[test]
    fn parse_reasoning_suffix_preserves_hyphenated_base_names() {
        let parsed = parse_with_builtin_suffixes("openai/gpt-4o-mini-high")
            .expect("high suffix should parse");

        assert_eq!(parsed.base_requested_name, "openai/gpt-4o-mini");
        assert_eq!(parsed.requested_suffix.as_deref(), Some("high"));
        assert_eq!(parsed.requested_preset, Some(ReasoningPreset::High));
    }

    #[test]
    fn parse_reasoning_suffix_only_splits_direct_model_part() {
        let parsed = parse_with_builtin_suffixes("openai-high/gpt-4o-high")
            .expect("suffix should parse on model part");

        assert_eq!(parsed.base_requested_name, "openai-high/gpt-4o");
    }

    #[test]
    fn parse_reasoning_suffix_ignores_unknown_suffixes() {
        assert!(parse_with_builtin_suffixes("openai/gpt-4o-ultra").is_none());
    }

    #[test]
    fn enabled_reasoning_suffixes_are_derived_from_enabled_presets() {
        let catalog = CacheModelsCatalog {
            providers: vec![],
            models: vec![],
            routes: vec![],
            api_key_overrides: vec![],
            reasoning_configs: vec![CacheReasoningConfig {
                id: 1,
                scope_kind: crate::database::reasoning_config::ReasoningConfigScope::Provider,
                provider_id: Some(1),
                model_id: None,
                mode: ReasoningConfigMode::Custom,
                family: Some(crate::database::reasoning_config::ReasoningPatchFamily::OpenAiChatReasoningEffort),
                presets: vec![
                    CacheReasoningConfigPreset {
                        id: 10,
                        config_id: 1,
                        preset: ReasoningPreset::High,
                        suffix: "ignored-cache-value".to_string(),
                        requires_reasoning: true,
                        allowed_operation_kinds: vec!["generation".to_string()],
                        expose_in_models: true,
                        is_enabled: true,
                    },
                    CacheReasoningConfigPreset {
                        id: 11,
                        config_id: 1,
                        preset: ReasoningPreset::Medium,
                        suffix: "medium".to_string(),
                        requires_reasoning: true,
                        allowed_operation_kinds: vec!["generation".to_string()],
                        expose_in_models: true,
                        is_enabled: false,
                    },
                ],
            }],
            runtime_feature_configs: vec![],
        };

        assert_eq!(
            enabled_reasoning_suffixes(&catalog),
            vec![ReasoningSuffixDefinition {
                suffix: "high".to_string(),
                preset: ReasoningPreset::High,
            }]
        );
    }
}
