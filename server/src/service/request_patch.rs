use serde::Serialize;

use crate::controller::BaseError;
use crate::database::request_patch::{
    RequestPatchVariantInput, normalize_suffix, normalize_target, validate_reserved_target,
};
use crate::schema::enum_def::RequestPatchOperation;
use crate::schema::enum_def::RequestPatchPlacement;
use crate::service::cache::types::{
    CacheRequestPatchConflict, CacheRequestPatchExplainEntry, CacheRequestPatchRule,
    CacheRequestPatchVariant, CacheResolvedRequestPatch, RequestPatchExplainStatus,
    RequestPatchVariantOrigin,
};

/// Turn a validated Manager Preview payload into an ephemeral catalog Variant.
///
/// The database preview path remains responsible for owner, value, and
/// persistence-shape validation. This helper repeats reserved-target validation
/// so every Manager Preview caller shares the same fail-closed policy before the
/// candidate reaches the evaluator; it never allocates or persists IDs.
pub fn cache_variant_from_preview_input(
    input: &RequestPatchVariantInput,
    variant_id: Option<i64>,
) -> Result<CacheRequestPatchVariant, BaseError> {
    let id = variant_id.unwrap_or_default();
    let suffix = normalize_suffix(input.suffix.as_deref())?;
    let rules = input
        .rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let value_json = match rule.operation {
                RequestPatchOperation::Remove => None,
                RequestPatchOperation::Set => {
                    let value = rule.value_json.as_ref().ok_or_else(|| {
                        BaseError::ParamInvalid(Some(
                            "SET rules must include value_json".to_string(),
                        ))
                    })?;
                    Some(serde_json::to_string(value).map_err(|error| {
                        BaseError::ParamInvalid(Some(format!(
                            "failed to serialize value_json: {error}"
                        )))
                    })?)
                }
            };
            let target = normalize_target(rule.placement, &rule.target)?;
            validate_reserved_target(rule.placement, &target)?;
            Ok(CacheRequestPatchRule {
                id: -(index as i64 + 1),
                variant_id: id,
                placement: rule.placement,
                target,
                operation: rule.operation,
                value_json,
                description: rule.description.clone(),
                created_at: 0,
                updated_at: 0,
            })
        })
        .collect::<Result<Vec<_>, BaseError>>()?;

    Ok(CacheRequestPatchVariant {
        id,
        source_id: input.source_id,
        model_id: input.model_id,
        suffix,
        enabled: input.enabled,
        expose_in_models: input.expose_in_models,
        rules,
    })
}

fn placement_rank(placement: RequestPatchPlacement) -> u8 {
    match placement {
        RequestPatchPlacement::Header => 0,
        RequestPatchPlacement::Query => 1,
        RequestPatchPlacement::Body => 2,
    }
}

fn body_target_is_ancestor_or_descendant(left: &str, right: &str) -> bool {
    if left == right {
        return false;
    }
    let is_prefix = |prefix: &str, target: &str| {
        target
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
    };
    is_prefix(left, right) || is_prefix(right, left)
}

fn normalized_target(rule: &CacheRequestPatchRule) -> String {
    match rule.placement {
        RequestPatchPlacement::Header => rule.target.to_ascii_lowercase(),
        _ => rule.target.clone(),
    }
}

fn stable_rule_order(
    left: &CacheRequestPatchRule,
    right: &CacheRequestPatchRule,
) -> std::cmp::Ordering {
    placement_rank(left.placement)
        .cmp(&placement_rank(right.placement))
        .then_with(|| normalized_target(left).cmp(&normalized_target(right)))
        .then_with(|| left.created_at.cmp(&right.created_at))
        .then_with(|| left.id.cmp(&right.id))
}

fn stable_candidate_order(
    left: &(&CacheRequestPatchRule, RequestPatchVariantOrigin, usize),
    right: &(&CacheRequestPatchRule, RequestPatchVariantOrigin, usize),
) -> std::cmp::Ordering {
    placement_rank(left.0.placement)
        .cmp(&placement_rank(right.0.placement))
        .then_with(|| normalized_target(left.0).cmp(&normalized_target(right.0)))
        .then_with(|| left.2.cmp(&right.2))
        .then_with(|| left.0.created_at.cmp(&right.0.created_at))
        .then_with(|| left.0.id.cmp(&right.0.id))
}

fn stable_effective_order(
    left: &CacheResolvedRequestPatch,
    right: &CacheResolvedRequestPatch,
) -> std::cmp::Ordering {
    placement_rank(left.placement)
        .cmp(&placement_rank(right.placement))
        .then_with(|| left.target.cmp(&right.target))
        .then_with(|| left.source_rule_id.cmp(&right.source_rule_id))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RequestPatchLayerState {
    pub origin: RequestPatchVariantOrigin,
    pub variant_id: Option<i64>,
    pub enabled: bool,
    pub rule_count: usize,
    pub expose_in_models: bool,
    pub status: RequestPatchExplainStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestPatchEvaluation {
    pub source_id: i64,
    pub model_id: Option<i64>,
    pub suffix: Option<String>,
    pub layers: Vec<RequestPatchLayerState>,
    pub effective_rules: Vec<CacheResolvedRequestPatch>,
    pub explain: Vec<CacheRequestPatchExplainEntry>,
    pub conflicts: Vec<CacheRequestPatchConflict>,
    pub has_conflicts: bool,
    pub exposed_in_models: bool,
    pub executable: bool,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct SelectedLayer<'a> {
    origin: RequestPatchVariantOrigin,
    variant: Option<&'a CacheRequestPatchVariant>,
    active: bool,
    status: RequestPatchExplainStatus,
    reason: Option<String>,
}

fn select_variant<'a>(
    variants: &'a [CacheRequestPatchVariant],
    source_id: i64,
    model_id: Option<i64>,
    suffix: Option<&str>,
) -> Option<&'a CacheRequestPatchVariant> {
    variants.iter().find(|variant| {
        variant.source_id == source_id
            && variant.model_id == model_id
            && variant.suffix.as_deref() == suffix
    })
}

fn layer_state(layer: &SelectedLayer<'_>) -> RequestPatchLayerState {
    RequestPatchLayerState {
        origin: layer.origin.clone(),
        variant_id: layer.variant.map(|variant| variant.id),
        enabled: layer.variant.is_some_and(|variant| variant.enabled),
        rule_count: layer.variant.map_or(0, |variant| variant.rules.len()),
        expose_in_models: layer
            .variant
            .is_some_and(|variant| variant.expose_in_models),
        status: layer.status.clone(),
        reason: layer.reason.clone(),
    }
}

fn add_conflict(
    conflicts: &mut Vec<CacheRequestPatchConflict>,
    lower: (&CacheRequestPatchRule, &RequestPatchVariantOrigin),
    higher: (&CacheRequestPatchRule, &RequestPatchVariantOrigin),
) {
    conflicts.push(CacheRequestPatchConflict {
        lower_priority_variant_id: lower.0.variant_id,
        higher_priority_variant_id: higher.0.variant_id,
        lower_priority_origin: lower.1.clone(),
        higher_priority_origin: higher.1.clone(),
        placement: RequestPatchPlacement::Body,
        lower_priority_target: lower.0.target.clone(),
        higher_priority_target: higher.0.target.clone(),
        reason: format!(
            "{} BODY target '{}' conflicts with higher-priority {} BODY target '{}'",
            lower.0.variant_id, lower.0.target, higher.0.variant_id, higher.0.target
        ),
    });
}

/// Resolve the only supported Request Patch contract.
///
/// The input is an immutable catalog snapshot. Resolution is deliberately pure so
/// Manager Preview, Explain, `/models`, and execution all share the same four-layer
/// state machine and fail-closed conflict behavior.
pub fn evaluate_request_patch_variants(
    variants: &[CacheRequestPatchVariant],
    source_id: i64,
    model_id: Option<i64>,
    suffix: Option<&str>,
) -> RequestPatchEvaluation {
    let source_base = SelectedLayer {
        origin: RequestPatchVariantOrigin::SourceBase,
        variant: select_variant(variants, source_id, None, None),
        active: false,
        status: RequestPatchExplainStatus::Dormant,
        reason: Some("Variant is not configured".to_string()),
    };
    let model_base = SelectedLayer {
        origin: RequestPatchVariantOrigin::ModelBase,
        variant: model_id
            .and_then(|model_id| select_variant(variants, source_id, Some(model_id), None)),
        active: false,
        status: RequestPatchExplainStatus::Dormant,
        reason: Some("Variant is not configured".to_string()),
    };

    let mut selected_layers = vec![source_base, model_base];
    if let Some(suffix) = suffix {
        let source_suffix = SelectedLayer {
            origin: RequestPatchVariantOrigin::SourceSuffix,
            variant: select_variant(variants, source_id, None, Some(suffix)),
            active: false,
            status: RequestPatchExplainStatus::Dormant,
            reason: Some("Variant is not configured".to_string()),
        };
        let model_suffix = SelectedLayer {
            origin: RequestPatchVariantOrigin::ModelSuffix,
            variant: model_id.and_then(|model_id| {
                select_variant(variants, source_id, Some(model_id), Some(suffix))
            }),
            active: false,
            status: RequestPatchExplainStatus::Dormant,
            reason: Some("Variant is not configured".to_string()),
        };
        selected_layers.push(source_suffix);
        selected_layers.push(model_suffix);
    }

    for layer in &mut selected_layers[..2] {
        layer.active = layer.variant.is_some_and(|variant| variant.enabled);
        if layer.variant.is_none() {
            layer.status = RequestPatchExplainStatus::Dormant;
            layer.reason = Some("Variant is not configured".to_string());
        } else if layer.variant.is_some_and(|variant| !variant.enabled) {
            layer.status = RequestPatchExplainStatus::Dormant;
            layer.reason = Some("disabled base Variant is skipped".to_string());
        } else {
            layer.status = RequestPatchExplainStatus::Effective;
            layer.reason = None;
        }
    }

    if suffix.is_some() {
        let source_suffix_index = 2;
        let model_suffix_index = 3;
        let model_tombstone = selected_layers[model_suffix_index]
            .variant
            .is_some_and(|variant| !variant.enabled);

        if model_tombstone {
            selected_layers[source_suffix_index].active = false;
            selected_layers[source_suffix_index].status = RequestPatchExplainStatus::Masked;
            selected_layers[source_suffix_index].reason =
                Some("model suffix tombstone masks the Source suffix".to_string());
            selected_layers[model_suffix_index].status = RequestPatchExplainStatus::Masked;
            selected_layers[model_suffix_index].reason =
                Some("disabled Model suffix Variant is a tombstone".to_string());
        } else {
            for index in [source_suffix_index, model_suffix_index] {
                selected_layers[index].active = selected_layers[index]
                    .variant
                    .is_some_and(|variant| variant.enabled);
                if selected_layers[index]
                    .variant
                    .is_some_and(|variant| !variant.enabled)
                {
                    selected_layers[index].status = RequestPatchExplainStatus::Dormant;
                    selected_layers[index].reason =
                        Some("disabled suffix Variant is skipped".to_string());
                } else if selected_layers[index].variant.is_none() {
                    selected_layers[index].status = RequestPatchExplainStatus::Dormant;
                    selected_layers[index].reason = Some("Variant is not configured".to_string());
                } else {
                    selected_layers[index].status = RequestPatchExplainStatus::Effective;
                    selected_layers[index].reason = None;
                }
            }
        }
    }

    let mut candidates: Vec<(&CacheRequestPatchRule, RequestPatchVariantOrigin, usize)> =
        Vec::new();
    for (layer_index, layer) in selected_layers.iter().enumerate() {
        if !layer.active {
            continue;
        }
        if let Some(variant) = layer.variant {
            candidates.extend(
                variant
                    .rules
                    .iter()
                    .map(|rule| (rule, layer.origin.clone(), layer_index)),
            );
        }
    }
    candidates.sort_by(stable_candidate_order);

    let mut winners: Vec<(&CacheRequestPatchRule, RequestPatchVariantOrigin, usize)> = Vec::new();
    let mut overridden_by: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();
    let mut conflict_ids: std::collections::BTreeMap<i64, Vec<i64>> =
        std::collections::BTreeMap::new();
    let mut conflicts = Vec::new();

    for candidate in candidates {
        let mut retained = Vec::with_capacity(winners.len());
        for existing in winners {
            if existing.0.placement == RequestPatchPlacement::Body
                && candidate.0.placement == RequestPatchPlacement::Body
                && body_target_is_ancestor_or_descendant(&existing.0.target, &candidate.0.target)
            {
                add_conflict(
                    &mut conflicts,
                    (&existing.0, &existing.1),
                    (&candidate.0, &candidate.1),
                );
                conflict_ids
                    .entry(existing.0.id)
                    .or_default()
                    .push(candidate.0.id);
                conflict_ids
                    .entry(candidate.0.id)
                    .or_default()
                    .push(existing.0.id);
                retained.push(existing);
                continue;
            }

            if existing.0.placement == candidate.0.placement
                && normalized_target(existing.0) == normalized_target(candidate.0)
            {
                overridden_by.insert(existing.0.id, candidate.0.id);
                continue;
            }
            retained.push(existing);
        }
        retained.push(candidate);
        winners = retained;
    }

    conflicts.sort_by(|left, right| {
        left.lower_priority_target
            .cmp(&right.lower_priority_target)
            .then_with(|| {
                left.higher_priority_target
                    .cmp(&right.higher_priority_target)
            })
            .then_with(|| {
                left.lower_priority_variant_id
                    .cmp(&right.lower_priority_variant_id)
            })
    });

    let effective_rules = winners
        .iter()
        .map(|(rule, origin, _)| CacheResolvedRequestPatch {
            placement: rule.placement,
            target: rule.target.clone(),
            operation: rule.operation,
            value_json: rule.value_json.clone(),
            source_variant_id: rule.variant_id,
            source_rule_id: rule.id,
            source_origin: origin.clone(),
            overridden_rule_ids: overridden_by
                .iter()
                .filter_map(|(lower, higher)| (*higher == rule.id).then_some(*lower))
                .collect(),
            description: rule.description.clone(),
        })
        .collect::<Vec<_>>();
    let mut effective_rules = effective_rules;
    effective_rules.sort_by(stable_effective_order);

    let mut explain = Vec::new();
    let mut all_rules = selected_layers
        .iter()
        .filter_map(|layer| layer.variant.map(|variant| (variant, layer)))
        .flat_map(|(variant, layer)| {
            variant
                .rules
                .iter()
                .map(move |rule| (rule, layer.origin.clone()))
        })
        .collect::<Vec<_>>();
    all_rules.sort_by(|left, right| stable_rule_order(left.0, right.0));
    for (rule, origin) in all_rules {
        let selected_layer = selected_layers
            .iter()
            .find(|layer| layer.origin == origin && layer.variant.is_some());
        let conflict_with_rule_ids = conflict_ids.get(&rule.id).cloned().unwrap_or_default();
        let (status, effective_rule_id, message) =
            if selected_layer.is_some_and(|layer| !layer.active) {
                let layer = selected_layer.expect("selected layer should exist");
                (layer.status.clone(), None, layer.reason.clone())
            } else if !conflict_with_rule_ids.is_empty() {
                (
                    RequestPatchExplainStatus::Conflicted,
                    None,
                    Some("BODY target has an ancestor/descendant conflict".to_string()),
                )
            } else if let Some(effective_rule_id) = overridden_by.get(&rule.id) {
                (
                    RequestPatchExplainStatus::Overridden,
                    Some(*effective_rule_id),
                    Some(format!(
                        "overridden by higher-priority Rule {effective_rule_id}"
                    )),
                )
            } else {
                (RequestPatchExplainStatus::Effective, Some(rule.id), None)
            };
        explain.push(CacheRequestPatchExplainEntry {
            rule: rule.clone(),
            origin,
            status,
            effective_rule_id,
            conflict_with_rule_ids,
            message,
        });
    }

    for layer in &mut selected_layers {
        if !layer.active {
            continue;
        }
        let Some(variant) = layer.variant else {
            continue;
        };
        if variant.rules.is_empty() {
            layer.status = RequestPatchExplainStatus::Dormant;
            layer.reason = Some("enabled Variant has no Rules".to_string());
            continue;
        }
        let has_conflicted_rule = variant
            .rules
            .iter()
            .any(|rule| conflict_ids.contains_key(&rule.id));
        if has_conflicted_rule {
            layer.status = RequestPatchExplainStatus::Conflicted;
            layer.reason = Some("one or more Rules have a BODY ancestor conflict".to_string());
        } else if variant
            .rules
            .iter()
            .all(|rule| overridden_by.contains_key(&rule.id))
        {
            layer.status = RequestPatchExplainStatus::Overridden;
            layer.reason = Some("all Rules are overridden by a higher-priority layer".to_string());
        }
    }

    let layers = selected_layers.iter().map(layer_state).collect::<Vec<_>>();
    let has_conflicts = !conflicts.is_empty();
    let suffix_has_effective_rules = suffix.is_none()
        || effective_rules.iter().any(|rule| {
            matches!(
                rule.source_origin,
                RequestPatchVariantOrigin::SourceSuffix | RequestPatchVariantOrigin::ModelSuffix
            )
        });
    let masked = suffix.is_some()
        && selected_layers[3]
            .variant
            .is_some_and(|variant| !variant.enabled);
    let model_suffix_requires_source_rules = suffix.is_some()
        && selected_layers[3]
            .variant
            .is_some_and(|variant| variant.enabled && variant.rules.is_empty())
        && !selected_layers[2]
            .variant
            .is_some_and(|variant| variant.enabled && !variant.rules.is_empty());
    let source_suffix_is_empty = suffix.is_some()
        && selected_layers[2]
            .variant
            .is_some_and(|variant| variant.enabled && variant.rules.is_empty());
    let enabled_base_is_empty = selected_layers[..2].iter().any(|layer| {
        layer.active
            && layer
                .variant
                .is_some_and(|variant| variant.rules.is_empty())
    });
    let executable = !has_conflicts
        && suffix_has_effective_rules
        && !masked
        && !model_suffix_requires_source_rules
        && !source_suffix_is_empty
        && !enabled_base_is_empty;
    let exposed_in_models = suffix.is_some_and(|_| {
        if masked {
            return false;
        }
        selected_layers[3]
            .variant
            .filter(|variant| variant.enabled)
            .or_else(|| selected_layers[2].variant.filter(|variant| variant.enabled))
            .is_some_and(|variant| variant.expose_in_models)
    });
    let failure_reason = if has_conflicts {
        Some("request patch BODY targets have an ancestor/descendant conflict".to_string())
    } else if masked {
        Some("Model suffix Variant is disabled and masks the Source suffix".to_string())
    } else if model_suffix_requires_source_rules {
        Some(
            "enabled Model suffix Variant has no Rules and no enabled Source suffix Rules"
                .to_string(),
        )
    } else if source_suffix_is_empty {
        Some("enabled Source suffix Variant has no Rules".to_string())
    } else if enabled_base_is_empty {
        Some("enabled base Variant has no Rules".to_string())
    } else if suffix.is_some() && effective_rules.is_empty() {
        Some("suffix has no effective Request Patch Rules".to_string())
    } else {
        None
    };

    RequestPatchEvaluation {
        source_id,
        model_id,
        suffix: suffix.map(str::to_string),
        layers,
        effective_rules,
        explain,
        conflicts,
        has_conflicts,
        exposed_in_models,
        executable,
        failure_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::enum_def::{RequestPatchOperation, RequestPatchPlacement};

    fn variant(
        id: i64,
        model_id: Option<i64>,
        suffix: Option<&str>,
        enabled: bool,
        expose_in_models: bool,
        rules: Vec<CacheRequestPatchRule>,
    ) -> CacheRequestPatchVariant {
        CacheRequestPatchVariant {
            id,
            source_id: 10,
            model_id,
            suffix: suffix.map(str::to_string),
            enabled,
            expose_in_models,
            rules,
        }
    }

    fn rule(
        id: i64,
        variant_id: i64,
        placement: RequestPatchPlacement,
        target: &str,
    ) -> CacheRequestPatchRule {
        CacheRequestPatchRule {
            id,
            variant_id,
            placement,
            target: target.to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some(id.to_string()),
            description: None,
            created_at: id,
            updated_at: id,
        }
    }

    #[test]
    fn preview_cache_conversion_rejects_responses_stateless_targets() {
        for target in [
            "/store",
            "/previous_response_id/value",
            "/conversation/id",
            "/background",
        ] {
            let input = RequestPatchVariantInput {
                source_id: 10,
                model_id: None,
                suffix: Some("stateless".to_string()),
                enabled: true,
                expose_in_models: true,
                rules: vec![crate::database::request_patch::RequestPatchRuleInput {
                    placement: RequestPatchPlacement::Body,
                    target: target.to_string(),
                    operation: RequestPatchOperation::Remove,
                    value_json: None,
                    description: None,
                }],
            };
            assert!(
                cache_variant_from_preview_input(&input, None).is_err(),
                "{target}"
            );
        }
    }

    #[test]
    fn four_layers_merge_with_highest_exact_target_winning() {
        let variants = vec![
            variant(
                1,
                None,
                None,
                true,
                false,
                vec![rule(11, 1, RequestPatchPlacement::Header, "X-Test")],
            ),
            variant(
                2,
                Some(20),
                None,
                true,
                false,
                vec![rule(12, 2, RequestPatchPlacement::Header, "x-test")],
            ),
            variant(
                3,
                None,
                Some("fast"),
                true,
                true,
                vec![rule(13, 3, RequestPatchPlacement::Query, "temperature")],
            ),
            variant(
                4,
                Some(20),
                Some("fast"),
                true,
                false,
                vec![rule(14, 4, RequestPatchPlacement::Body, "/metadata/fast")],
            ),
        ];

        let result = evaluate_request_patch_variants(&variants, 10, Some(20), Some("fast"));
        assert!(result.executable);
        assert!(result.exposed_in_models == false);
        assert_eq!(result.effective_rules.len(), 3);
        assert!(result.explain.iter().any(|entry| {
            entry.rule.id == 11 && entry.status == RequestPatchExplainStatus::Overridden
        }));
        assert_eq!(result.effective_rules[0].source_rule_id, 12);
    }

    #[test]
    fn disabled_model_suffix_masks_source_suffix_and_allows_zero_rules() {
        let variants = vec![
            variant(
                3,
                None,
                Some("fast"),
                true,
                true,
                vec![rule(13, 3, RequestPatchPlacement::Query, "temperature")],
            ),
            variant(4, Some(20), Some("fast"), false, false, vec![]),
        ];
        let result = evaluate_request_patch_variants(&variants, 10, Some(20), Some("fast"));
        assert!(!result.executable);
        assert!(!result.exposed_in_models);
        assert!(result.layers.iter().any(|layer| {
            layer.origin == RequestPatchVariantOrigin::SourceSuffix
                && layer.status == RequestPatchExplainStatus::Masked
        }));
    }

    #[test]
    fn body_ancestor_conflict_fails_closed_in_both_directions() {
        let variants = vec![
            variant(
                1,
                None,
                None,
                true,
                false,
                vec![rule(11, 1, RequestPatchPlacement::Body, "/options")],
            ),
            variant(
                2,
                Some(20),
                None,
                true,
                false,
                vec![rule(
                    12,
                    2,
                    RequestPatchPlacement::Body,
                    "/options/temperature",
                )],
            ),
        ];
        let result = evaluate_request_patch_variants(&variants, 10, Some(20), None);
        assert!(result.has_conflicts);
        assert!(!result.executable);
        assert_eq!(result.conflicts.len(), 1);
        assert!(
            result
                .explain
                .iter()
                .all(|entry| { entry.status == RequestPatchExplainStatus::Conflicted })
        );
    }

    #[test]
    fn source_suffix_is_used_when_model_suffix_is_absent() {
        let variants = vec![variant(
            3,
            None,
            Some("fast"),
            true,
            true,
            vec![rule(13, 3, RequestPatchPlacement::Query, "temperature")],
        )];
        let result = evaluate_request_patch_variants(&variants, 10, Some(20), Some("fast"));
        assert!(result.executable);
        assert!(result.exposed_in_models);
        assert_eq!(result.effective_rules[0].source_variant_id, 3);
    }

    #[test]
    fn exact_override_uses_layer_priority_even_when_rule_ids_sort_oppositely() {
        let variants = vec![
            variant(
                100,
                None,
                None,
                true,
                false,
                vec![rule(900, 100, RequestPatchPlacement::Header, "x-test")],
            ),
            variant(
                1,
                Some(20),
                None,
                true,
                false,
                vec![rule(2, 1, RequestPatchPlacement::Header, "X-Test")],
            ),
        ];
        let result = evaluate_request_patch_variants(&variants, 10, Some(20), None);
        assert!(result.executable);
        assert_eq!(result.effective_rules.len(), 1);
        assert_eq!(result.effective_rules[0].source_rule_id, 2);
        assert!(result.explain.iter().any(|entry| {
            entry.rule.id == 900 && entry.status == RequestPatchExplainStatus::Overridden
        }));
        assert_eq!(
            result.layers[0].status,
            RequestPatchExplainStatus::Overridden
        );
    }

    #[test]
    fn disabled_source_suffix_can_be_replaced_by_enabled_model_suffix() {
        let variants = vec![
            variant(
                30,
                None,
                Some("tools"),
                false,
                true,
                vec![rule(301, 30, RequestPatchPlacement::Query, "mode")],
            ),
            variant(
                31,
                Some(20),
                Some("tools"),
                true,
                true,
                vec![rule(311, 31, RequestPatchPlacement::Query, "mode")],
            ),
        ];
        let result = evaluate_request_patch_variants(&variants, 10, Some(20), Some("tools"));
        assert!(result.executable);
        assert_eq!(result.effective_rules[0].source_rule_id, 311);
        assert_eq!(result.layers[0].status, RequestPatchExplainStatus::Dormant);
        assert!(result.exposed_in_models);
    }

    #[test]
    fn empty_model_suffix_inherits_only_from_an_enabled_source_suffix() {
        let source = variant(
            40,
            None,
            Some("fast"),
            true,
            true,
            vec![rule(401, 40, RequestPatchPlacement::Body, "/options/temp")],
        );
        let model = variant(41, Some(20), Some("fast"), true, false, vec![]);
        let inherited = evaluate_request_patch_variants(
            &[source.clone(), model.clone()],
            10,
            Some(20),
            Some("fast"),
        );
        assert!(inherited.executable);
        assert_eq!(inherited.effective_rules[0].source_rule_id, 401);
        assert!(inherited.exposed_in_models == false);

        let no_source = evaluate_request_patch_variants(&[model], 10, Some(20), Some("fast"));
        assert!(!no_source.executable);
        assert_eq!(
            no_source.failure_reason.as_deref(),
            Some("enabled Model suffix Variant has no Rules and no enabled Source suffix Rules")
        );
    }

    #[test]
    fn source_suffix_with_zero_rules_fails_closed_even_when_model_has_rules() {
        let variants = vec![
            variant(50, None, Some("fast"), true, true, vec![]),
            variant(
                51,
                Some(20),
                Some("fast"),
                true,
                false,
                vec![rule(511, 51, RequestPatchPlacement::Body, "/options/temp")],
            ),
        ];
        let result = evaluate_request_patch_variants(&variants, 10, Some(20), Some("fast"));
        assert!(!result.executable);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("enabled Source suffix Variant has no Rules")
        );
    }

    #[test]
    fn model_tombstone_masks_source_and_removes_exposure() {
        let variants = vec![
            variant(
                60,
                None,
                Some("fast"),
                true,
                true,
                vec![rule(601, 60, RequestPatchPlacement::Query, "mode")],
            ),
            variant(61, Some(20), Some("fast"), false, false, vec![]),
        ];
        let result = evaluate_request_patch_variants(&variants, 10, Some(20), Some("fast"));
        assert!(!result.executable);
        assert!(!result.exposed_in_models);
        assert_eq!(result.effective_rules.len(), 0);
        assert!(
            result
                .explain
                .iter()
                .all(|entry| { entry.status == RequestPatchExplainStatus::Masked })
        );
    }

    #[test]
    fn absent_suffix_is_not_executable_and_base_request_remains_valid() {
        let variants = vec![variant(
            70,
            None,
            Some("disabled"),
            false,
            true,
            vec![rule(701, 70, RequestPatchPlacement::Query, "mode")],
        )];
        let suffix = evaluate_request_patch_variants(&variants, 10, Some(20), Some("disabled"));
        assert!(!suffix.executable);
        assert!(!suffix.exposed_in_models);
        assert!(suffix.failure_reason.is_some());

        let base = evaluate_request_patch_variants(&[], 10, Some(20), None);
        assert!(base.executable);
        assert!(base.effective_rules.is_empty());
        assert!(
            base.layers
                .iter()
                .all(|layer| { layer.status == RequestPatchExplainStatus::Dormant })
        );
    }

    #[test]
    fn body_conflict_is_symmetric_for_parent_direction() {
        let variants = vec![
            variant(
                80,
                None,
                None,
                true,
                false,
                vec![rule(801, 80, RequestPatchPlacement::Body, "/options/temp")],
            ),
            variant(
                81,
                Some(20),
                None,
                true,
                false,
                vec![rule(811, 81, RequestPatchPlacement::Body, "/options")],
            ),
        ];
        let result = evaluate_request_patch_variants(&variants, 10, Some(20), None);
        assert!(result.has_conflicts);
        assert!(!result.executable);
        assert_eq!(result.conflicts.len(), 1);
    }
}
