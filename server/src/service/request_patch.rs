use std::collections::{BTreeMap, BTreeSet};

use crate::schema::enum_def::RequestPatchPlacement;
use crate::service::cache::types::{
    CacheInheritedRequestPatch, CacheRequestPatchConflict, CacheRequestPatchExplainEntry,
    CacheRequestPatchRule, CacheResolvedModelRequestPatches, CacheResolvedRequestPatch,
    RequestPatchExplainStatus, RequestPatchRuleOrigin,
};

fn placement_rank(placement: RequestPatchPlacement) -> u8 {
    match placement {
        RequestPatchPlacement::Header => 0,
        RequestPatchPlacement::Query => 1,
        RequestPatchPlacement::Body => 2,
    }
}

fn stable_sort_rules(rules: &mut [CacheRequestPatchRule]) {
    rules.sort_by(|left, right| {
        placement_rank(left.placement)
            .cmp(&placement_rank(right.placement))
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn stable_sort_effective_rules(rules: &mut [CacheResolvedRequestPatch]) {
    rules.sort_by(|left, right| {
        placement_rank(left.placement)
            .cmp(&placement_rank(right.placement))
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.source_rule_id.cmp(&right.source_rule_id))
    });
}

fn matches_body_prefix(target: &str, prefix: &str) -> bool {
    target == prefix || target.starts_with(&format!("{prefix}/"))
}

fn is_cross_scope_body_conflict(provider_target: &str, model_target: &str) -> bool {
    provider_target != model_target
        && (matches_body_prefix(provider_target, model_target)
            || matches_body_prefix(model_target, provider_target))
}

pub fn resolve_effective_request_patches(
    provider_id: i64,
    model_id: i64,
    provider_rules: &[CacheRequestPatchRule],
    model_rules: &[CacheRequestPatchRule],
) -> CacheResolvedModelRequestPatches {
    let mut provider_active: Vec<CacheRequestPatchRule> = provider_rules
        .iter()
        .filter(|rule| rule.is_enabled)
        .cloned()
        .collect();
    let mut model_active: Vec<CacheRequestPatchRule> = model_rules
        .iter()
        .filter(|rule| rule.is_enabled)
        .cloned()
        .collect();
    stable_sort_rules(&mut provider_active);
    stable_sort_rules(&mut model_active);

    let model_exact_rules: BTreeMap<(u8, String), CacheRequestPatchRule> = model_active
        .iter()
        .map(|rule| {
            (
                (placement_rank(rule.placement), rule.target.clone()),
                rule.clone(),
            )
        })
        .collect();

    let mut provider_conflict_map: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let mut model_conflict_map: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let mut conflicts = Vec::new();

    let provider_body_rules: Vec<&CacheRequestPatchRule> = provider_active
        .iter()
        .filter(|rule| rule.placement == RequestPatchPlacement::Body)
        .collect();
    let model_body_rules: Vec<&CacheRequestPatchRule> = model_active
        .iter()
        .filter(|rule| rule.placement == RequestPatchPlacement::Body)
        .collect();

    for provider_rule in &provider_body_rules {
        for model_rule in &model_body_rules {
            if !is_cross_scope_body_conflict(&provider_rule.target, &model_rule.target) {
                continue;
            }

            provider_conflict_map
                .entry(provider_rule.id)
                .or_default()
                .push(model_rule.id);
            model_conflict_map
                .entry(model_rule.id)
                .or_default()
                .push(provider_rule.id);
            conflicts.push(CacheRequestPatchConflict {
                provider_rule_id: provider_rule.id,
                model_rule_id: model_rule.id,
                placement: RequestPatchPlacement::Body,
                provider_target: provider_rule.target.clone(),
                model_target: model_rule.target.clone(),
                reason: format!(
                    "provider BODY target '{}' conflicts with model BODY target '{}'",
                    provider_rule.target, model_rule.target
                ),
            });
        }
    }

    conflicts.sort_by(|left, right| {
        left.provider_target
            .cmp(&right.provider_target)
            .then_with(|| left.model_target.cmp(&right.model_target))
            .then_with(|| left.provider_rule_id.cmp(&right.provider_rule_id))
            .then_with(|| left.model_rule_id.cmp(&right.model_rule_id))
    });

    let inherited_rules = provider_active
        .iter()
        .map(|rule| {
            let overridden_by_rule_id = model_exact_rules
                .get(&(placement_rank(rule.placement), rule.target.clone()))
                .map(|model_rule| model_rule.id);
            let conflict_with_rule_ids = provider_conflict_map
                .get(&rule.id)
                .cloned()
                .unwrap_or_default();

            CacheInheritedRequestPatch {
                rule: rule.clone(),
                overridden_by_rule_id,
                is_effective: overridden_by_rule_id.is_none() && conflict_with_rule_ids.is_empty(),
                conflict_with_rule_ids,
            }
        })
        .collect();

    let mut effective_rules = Vec::new();
    for rule in &provider_active {
        if let Some(overriding_model_rule) =
            model_exact_rules.get(&(placement_rank(rule.placement), rule.target.clone()))
        {
            effective_rules.push(CacheResolvedRequestPatch {
                placement: overriding_model_rule.placement,
                target: overriding_model_rule.target.clone(),
                operation: overriding_model_rule.operation,
                value_json: overriding_model_rule.value_json.clone(),
                source_rule_id: overriding_model_rule.id,
                source_origin: RequestPatchRuleOrigin::ModelDirect,
                overridden_rule_ids: vec![rule.id],
                description: overriding_model_rule.description.clone(),
            });
            continue;
        }

        effective_rules.push(CacheResolvedRequestPatch {
            placement: rule.placement,
            target: rule.target.clone(),
            operation: rule.operation,
            value_json: rule.value_json.clone(),
            source_rule_id: rule.id,
            source_origin: RequestPatchRuleOrigin::ProviderDirect,
            overridden_rule_ids: Vec::new(),
            description: rule.description.clone(),
        });
    }

    for rule in &model_active {
        if provider_active.iter().any(|provider_rule| {
            provider_rule.placement == rule.placement && provider_rule.target == rule.target
        }) {
            continue;
        }

        effective_rules.push(CacheResolvedRequestPatch {
            placement: rule.placement,
            target: rule.target.clone(),
            operation: rule.operation,
            value_json: rule.value_json.clone(),
            source_rule_id: rule.id,
            source_origin: RequestPatchRuleOrigin::ModelDirect,
            overridden_rule_ids: Vec::new(),
            description: rule.description.clone(),
        });
    }
    stable_sort_effective_rules(&mut effective_rules);

    let mut explain = Vec::new();
    for rule in &provider_active {
        let overridden_by_rule_id = model_exact_rules
            .get(&(placement_rank(rule.placement), rule.target.clone()))
            .map(|model_rule| model_rule.id);
        let conflict_with_rule_ids = provider_conflict_map
            .get(&rule.id)
            .cloned()
            .unwrap_or_default();

        let (status, effective_rule_id, message) = if !conflict_with_rule_ids.is_empty() {
            (
                RequestPatchExplainStatus::Conflicted,
                None,
                Some(format!(
                    "Conflicts with model rule(s): {}",
                    join_rule_ids(&conflict_with_rule_ids)
                )),
            )
        } else if let Some(model_rule_id) = overridden_by_rule_id {
            (
                RequestPatchExplainStatus::Overridden,
                Some(model_rule_id),
                Some(format!(
                    "Overridden by model rule {} on the same target",
                    model_rule_id
                )),
            )
        } else {
            (RequestPatchExplainStatus::Effective, Some(rule.id), None)
        };

        explain.push(CacheRequestPatchExplainEntry {
            rule: rule.clone(),
            origin: RequestPatchRuleOrigin::ProviderDirect,
            status,
            effective_rule_id,
            conflict_with_rule_ids,
            message,
        });
    }

    for rule in &model_active {
        let conflict_with_rule_ids = model_conflict_map
            .get(&rule.id)
            .cloned()
            .unwrap_or_default();

        let (status, effective_rule_id, message) = if !conflict_with_rule_ids.is_empty() {
            (
                RequestPatchExplainStatus::Conflicted,
                None,
                Some(format!(
                    "Conflicts with provider rule(s): {}",
                    join_rule_ids(&conflict_with_rule_ids)
                )),
            )
        } else {
            (RequestPatchExplainStatus::Effective, Some(rule.id), None)
        };

        explain.push(CacheRequestPatchExplainEntry {
            rule: rule.clone(),
            origin: RequestPatchRuleOrigin::ModelDirect,
            status,
            effective_rule_id,
            conflict_with_rule_ids,
            message,
        });
    }
    explain.sort_by(|left, right| {
        placement_rank(left.rule.placement)
            .cmp(&placement_rank(right.rule.placement))
            .then_with(|| left.rule.target.cmp(&right.rule.target))
            .then_with(|| left.rule.id.cmp(&right.rule.id))
    });

    CacheResolvedModelRequestPatches {
        provider_id,
        model_id,
        direct_rules: model_active,
        inherited_rules,
        effective_rules,
        explain,
        has_conflicts: !conflicts.is_empty(),
        conflicts,
    }
}

fn join_rule_ids(rule_ids: &[i64]) -> String {
    let unique_ids: BTreeSet<i64> = rule_ids.iter().copied().collect();
    unique_ids
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use crate::schema::enum_def::{RequestPatchOperation, RequestPatchPlacement};

    use super::*;

    fn rule(
        id: i64,
        placement: RequestPatchPlacement,
        target: &str,
        origin: RequestPatchRuleOrigin,
    ) -> CacheRequestPatchRule {
        CacheRequestPatchRule {
            id,
            provider_id: match origin {
                RequestPatchRuleOrigin::ProviderDirect => Some(11),
                RequestPatchRuleOrigin::ModelDirect => None,
            },
            model_id: match origin {
                RequestPatchRuleOrigin::ProviderDirect => None,
                RequestPatchRuleOrigin::ModelDirect => Some(22),
            },
            placement,
            target: target.to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some("1".to_string()),
            description: Some(format!("rule-{id}")),
            is_enabled: true,
            created_at: id,
            updated_at: id,
        }
    }

    #[test]
    fn model_exact_target_overrides_provider_rule() {
        let mut provider_rule = rule(
            1,
            RequestPatchPlacement::Header,
            "x-test",
            RequestPatchRuleOrigin::ProviderDirect,
        );
        provider_rule.value_json = Some("true".to_string());
        provider_rule.description = Some("provider value".to_string());

        let mut model_rule = rule(
            2,
            RequestPatchPlacement::Header,
            "x-test",
            RequestPatchRuleOrigin::ModelDirect,
        );
        model_rule.value_json = Some("false".to_string());
        model_rule.description = Some("model value".to_string());

        let provider_rules = vec![provider_rule];
        let model_rules = vec![model_rule];

        let resolved = resolve_effective_request_patches(11, 22, &provider_rules, &model_rules);
        assert!(!resolved.has_conflicts);
        assert_eq!(resolved.inherited_rules.len(), 1);
        assert_eq!(resolved.inherited_rules[0].overridden_by_rule_id, Some(2));
        assert_eq!(resolved.effective_rules.len(), 1);
        assert_eq!(resolved.effective_rules[0].source_rule_id, 2);
        assert_eq!(
            resolved.effective_rules[0].source_origin,
            RequestPatchRuleOrigin::ModelDirect
        );
        assert_eq!(resolved.effective_rules[0].overridden_rule_ids, vec![1]);
        assert_eq!(
            resolved.effective_rules[0].value_json.as_deref(),
            Some("false")
        );
        assert_eq!(
            resolved.effective_rules[0].description.as_deref(),
            Some("model value")
        );
        assert!(resolved.explain.iter().any(
            |entry| entry.rule.id == 1 && entry.status == RequestPatchExplainStatus::Overridden
        ));
    }

    #[test]
    fn cross_scope_body_ancestor_conflict_is_reported() {
        let provider_rules = vec![rule(
            1,
            RequestPatchPlacement::Body,
            "/generation_config",
            RequestPatchRuleOrigin::ProviderDirect,
        )];
        let model_rules = vec![rule(
            2,
            RequestPatchPlacement::Body,
            "/generation_config/temperature",
            RequestPatchRuleOrigin::ModelDirect,
        )];

        let resolved = resolve_effective_request_patches(11, 22, &provider_rules, &model_rules);
        assert!(resolved.has_conflicts);
        assert_eq!(resolved.conflicts.len(), 1);
        assert_eq!(resolved.conflicts[0].provider_rule_id, 1);
        assert_eq!(resolved.conflicts[0].model_rule_id, 2);
        assert!(resolved.explain.iter().any(
            |entry| entry.rule.id == 1 && entry.status == RequestPatchExplainStatus::Conflicted
        ));
        assert!(resolved.explain.iter().any(
            |entry| entry.rule.id == 2 && entry.status == RequestPatchExplainStatus::Conflicted
        ));
    }

    #[test]
    fn stable_output_order_does_not_depend_on_input_order() {
        let provider_rules = vec![
            rule(
                3,
                RequestPatchPlacement::Query,
                "z-last",
                RequestPatchRuleOrigin::ProviderDirect,
            ),
            rule(
                1,
                RequestPatchPlacement::Header,
                "x-first",
                RequestPatchRuleOrigin::ProviderDirect,
            ),
        ];
        let model_rules = vec![rule(
            2,
            RequestPatchPlacement::Body,
            "/alpha",
            RequestPatchRuleOrigin::ModelDirect,
        )];

        let resolved = resolve_effective_request_patches(11, 22, &provider_rules, &model_rules);
        let ordered_targets: Vec<&str> = resolved
            .effective_rules
            .iter()
            .map(|rule| rule.target.as_str())
            .collect();
        assert_eq!(ordered_targets, vec!["x-first", "z-last", "/alpha"]);
    }
}
