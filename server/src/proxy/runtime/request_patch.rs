use std::sync::Arc;

use axum::http::HeaderMap;
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue as ReqwestHeaderValue},
};
use serde_json::{Map, Value};

use crate::{
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility,
        reasoning_suffix::{
            GeneratedReasoningPatch, ReasoningPatchContext, generate_reasoning_patches,
        },
        runtime::route_resolver::ExecutionTarget,
    },
    schema::enum_def::{RequestPatchOperation, RequestPatchPlacement},
    service::{
        app_state::AppState,
        cache::types::{
            CacheModel, CacheProvider, CacheRequestPatchConflict, CacheResolvedRequestPatch,
            RequestPatchSource, RuntimeRequestPatchConflict, RuntimeResolvedRequestPatch,
        },
        request_patch::resolve_effective_request_patches,
    },
};
use cyder_tools::log::{debug, error};

fn patch_error(message: impl Into<String>) -> ProxyError {
    ProxyError::gateway(
        ProxyErrorCode::ServerError,
        ExecutionStage::Patch,
        ResponseVisibility::NotVisible,
        None,
        message,
    )
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeRequestPatchTrace {
    pub applied_rules: Vec<RuntimeResolvedRequestPatch>,
    pub conflicts: Vec<RuntimeRequestPatchConflict>,
    pub has_conflicts: bool,
}

impl RuntimeRequestPatchTrace {
    pub(crate) fn conflict_error(&self, model_name: &str) -> Option<ProxyError> {
        if !self.has_conflicts {
            return None;
        }

        let reasons = self
            .conflicts
            .iter()
            .map(|conflict| conflict.reason.as_str())
            .collect::<Vec<_>>()
            .join("; ");

        Some(ProxyError::gateway(
            ProxyErrorCode::RequestPatchConflictError,
            ExecutionStage::Patch,
            ResponseVisibility::NotVisible,
            None,
            format!(
                "Request patch conflicts prevent model '{}' from being used: {}",
                model_name, reasons
            ),
        ))
    }
}

fn describe_json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn parse_request_patch_value(rule: &RuntimeResolvedRequestPatch) -> Result<Value, ProxyError> {
    let raw = rule.value_json.as_ref().ok_or_else(|| {
        patch_error(format!(
            "{} is missing value_json for SET",
            rule.source_label()
        ))
    })?;

    serde_json::from_str(raw).map_err(|err| {
        patch_error(format!(
            "{} has invalid value_json: {}",
            rule.source_label(),
            err
        ))
    })
}

fn parse_json_pointer_segments(pointer: &str) -> Result<Vec<String>, ProxyError> {
    if pointer.is_empty() || !pointer.starts_with('/') {
        return Err(patch_error(format!(
            "BODY request patch target '{}' is not a valid JSON Pointer",
            pointer
        )));
    }

    pointer
        .split('/')
        .skip(1)
        .map(|segment| {
            let mut decoded = String::with_capacity(segment.len());
            let mut chars = segment.chars();
            while let Some(ch) = chars.next() {
                if ch == '~' {
                    match chars.next() {
                        Some('0') => decoded.push('~'),
                        Some('1') => decoded.push('/'),
                        _ => {
                            return Err(patch_error(format!(
                                "BODY request patch target '{}' contains an invalid JSON Pointer escape",
                                pointer
                            )));
                        }
                    }
                } else {
                    decoded.push(ch);
                }
            }
            Ok(decoded)
        })
        .collect()
}

fn parse_array_index(token: &str, pointer: &str) -> Result<usize, ProxyError> {
    token.parse::<usize>().map_err(|_| {
        patch_error(format!(
            "BODY request patch target '{}' references invalid array index '{}'",
            pointer, token
        ))
    })
}

fn set_body_pointer_value(
    current: &mut Value,
    segments: &[String],
    pointer: &str,
    value: &mut Option<Value>,
) -> Result<(), ProxyError> {
    let segment = &segments[0];

    if segments.len() == 1 {
        let final_value = value
            .take()
            .expect("request patch final value should only be consumed once");
        match current {
            Value::Object(map) => {
                map.insert(segment.clone(), final_value);
                Ok(())
            }
            Value::Array(items) => {
                let index = parse_array_index(segment, pointer)?;
                let len = items.len();
                let slot = items.get_mut(index).ok_or_else(|| {
                    patch_error(format!(
                        "BODY request patch target '{}' is out of bounds for an array of length {}",
                        pointer, len
                    ))
                })?;
                *slot = final_value;
                Ok(())
            }
            Value::Null => {
                *current = Value::Object(Map::new());
                if let Value::Object(map) = current {
                    map.insert(segment.clone(), final_value);
                }
                Ok(())
            }
            other => Err(patch_error(format!(
                "BODY request patch target '{}' cannot write through existing {}",
                pointer,
                describe_json_kind(other)
            ))),
        }
    } else {
        match current {
            Value::Object(map) => {
                let child = map.entry(segment.clone()).or_insert(Value::Null);
                set_body_pointer_value(child, &segments[1..], pointer, value)
            }
            Value::Array(items) => {
                let index = parse_array_index(segment, pointer)?;
                let len = items.len();
                let child = items.get_mut(index).ok_or_else(|| {
                    patch_error(format!(
                        "BODY request patch target '{}' is out of bounds for an array of length {}",
                        pointer, len
                    ))
                })?;
                set_body_pointer_value(child, &segments[1..], pointer, value)
            }
            Value::Null => {
                *current = Value::Object(Map::new());
                if let Value::Object(map) = current {
                    let child = map.entry(segment.clone()).or_insert(Value::Null);
                    return set_body_pointer_value(child, &segments[1..], pointer, value);
                }
                unreachable!("BODY request patch SET should have promoted null to object");
            }
            other => Err(patch_error(format!(
                "BODY request patch target '{}' cannot create children under existing {}",
                pointer,
                describe_json_kind(other)
            ))),
        }
    }
}

fn remove_body_pointer_value(
    current: &mut Value,
    segments: &[String],
    pointer: &str,
) -> Result<(), ProxyError> {
    let segment = &segments[0];

    if segments.len() == 1 {
        match current {
            Value::Object(map) => {
                map.remove(segment);
                Ok(())
            }
            Value::Array(items) => {
                let index = parse_array_index(segment, pointer)?;
                if index >= items.len() {
                    return Ok(());
                }
                Err(patch_error(format!(
                    "BODY request patch target '{}' cannot remove array elements because that rewrites message structure",
                    pointer
                )))
            }
            _ => Ok(()),
        }
    } else {
        match current {
            Value::Object(map) => match map.get_mut(segment) {
                Some(child) => remove_body_pointer_value(child, &segments[1..], pointer),
                None => Ok(()),
            },
            Value::Array(items) => {
                let index = parse_array_index(segment, pointer)?;
                match items.get_mut(index) {
                    Some(child) => remove_body_pointer_value(child, &segments[1..], pointer),
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}

fn scalar_request_patch_value(rule: &RuntimeResolvedRequestPatch) -> Result<String, ProxyError> {
    let value = parse_request_patch_value(rule)?;
    match value {
        Value::String(text) => Ok(text),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(boolean) => Ok(boolean.to_string()),
        Value::Null => Ok("null".to_string()),
        other => Err(patch_error(format!(
            "{:?} request patch target '{}' requires a scalar JSON value, got {}",
            rule.placement,
            rule.target,
            describe_json_kind(&other)
        ))),
    }
}

fn apply_query_request_patch(
    url: &mut Url,
    rule: &RuntimeResolvedRequestPatch,
) -> Result<(), ProxyError> {
    let mut existing_pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .filter(|(key, _)| key != &rule.target)
        .collect();

    if rule.operation == RequestPatchOperation::Set {
        existing_pairs.push((rule.target.clone(), scalar_request_patch_value(rule)?));
    }

    let mut query_pairs = url.query_pairs_mut();
    query_pairs.clear();
    for (key, value) in existing_pairs {
        query_pairs.append_pair(&key, &value);
    }

    Ok(())
}

fn apply_header_request_patch(
    headers: &mut HeaderMap,
    rule: &RuntimeResolvedRequestPatch,
) -> Result<(), ProxyError> {
    let header_name = HeaderName::from_bytes(rule.target.as_bytes()).map_err(|err| {
        patch_error(format!(
            "{} has invalid header target '{}': {}",
            rule.source_label(),
            rule.target,
            err
        ))
    })?;

    match rule.operation {
        RequestPatchOperation::Remove => {
            headers.remove(&header_name);
            Ok(())
        }
        RequestPatchOperation::Set => {
            let header_value = ReqwestHeaderValue::from_str(&scalar_request_patch_value(rule)?)
                .map_err(|err| {
                    patch_error(format!(
                        "{} has invalid header value for '{}': {}",
                        rule.source_label(),
                        rule.target,
                        err
                    ))
                })?;
            headers.insert(header_name, header_value);
            Ok(())
        }
    }
}

pub(crate) fn apply_request_patches(
    data: &mut Value,
    url: &mut Url,
    headers: &mut HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
) -> Result<(), ProxyError> {
    for rule in request_patches {
        debug!(
            "Applying request patch {} to {:?} '{}'",
            rule.source_label(),
            rule.placement,
            rule.target
        );
        match rule.placement {
            RequestPatchPlacement::Header => apply_header_request_patch(headers, rule)?,
            RequestPatchPlacement::Query => apply_query_request_patch(url, rule)?,
            RequestPatchPlacement::Body => {
                let segments = parse_json_pointer_segments(&rule.target)?;
                match rule.operation {
                    RequestPatchOperation::Set => {
                        let mut value = Some(parse_request_patch_value(rule)?);
                        set_body_pointer_value(data, &segments, &rule.target, &mut value)?;
                    }
                    RequestPatchOperation::Remove => {
                        remove_body_pointer_value(data, &segments, &rule.target)?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn build_runtime_request_patch_trace(
    effective_rules: Vec<CacheResolvedRequestPatch>,
    conflicts: Vec<CacheRequestPatchConflict>,
    has_conflicts: bool,
    generated_rules: Vec<RuntimeResolvedRequestPatch>,
) -> Result<RuntimeRequestPatchTrace, ProxyError> {
    let mut runtime_rules = effective_rules
        .into_iter()
        .map(RuntimeResolvedRequestPatch::from)
        .collect::<Vec<_>>();
    let mut runtime_conflicts = conflicts
        .into_iter()
        .map(RuntimeRequestPatchConflict::from)
        .collect::<Vec<_>>();

    if !has_conflicts {
        let (merged_rules, generated_conflicts) =
            merge_runtime_request_patches(runtime_rules, generated_rules);
        runtime_rules = merged_rules;
        runtime_conflicts.extend(generated_conflicts);
    }

    let has_conflicts = has_conflicts || !runtime_conflicts.is_empty();
    let applied_rules = if has_conflicts {
        Vec::new()
    } else {
        runtime_rules.clone()
    };

    Ok(RuntimeRequestPatchTrace {
        applied_rules,
        conflicts: runtime_conflicts,
        has_conflicts,
    })
}

impl From<CacheRequestPatchConflict> for RuntimeRequestPatchConflict {
    fn from(conflict: CacheRequestPatchConflict) -> Self {
        Self {
            placement: conflict.placement,
            lower_priority_source: RequestPatchSource::ProviderRule {
                rule_id: conflict.provider_rule_id,
            },
            higher_priority_source: RequestPatchSource::ModelRule {
                rule_id: conflict.model_rule_id,
            },
            lower_priority_target: conflict.provider_target,
            higher_priority_target: conflict.model_target,
            reason: conflict.reason,
        }
    }
}

fn request_patch_source_priority(source: &RequestPatchSource) -> u8 {
    match source {
        RequestPatchSource::ProviderRule { .. } => 0,
        RequestPatchSource::ModelRule { .. } => 1,
        RequestPatchSource::ReasoningPreset { .. } => 2,
    }
}

fn request_patch_placement_rank(placement: RequestPatchPlacement) -> u8 {
    match placement {
        RequestPatchPlacement::Header => 0,
        RequestPatchPlacement::Query => 1,
        RequestPatchPlacement::Body => 2,
    }
}

fn stable_sort_runtime_request_patches(rules: &mut [RuntimeResolvedRequestPatch]) {
    rules.sort_by(|left, right| {
        request_patch_placement_rank(left.placement)
            .cmp(&request_patch_placement_rank(right.placement))
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| {
                request_patch_source_priority(&left.source)
                    .cmp(&request_patch_source_priority(&right.source))
            })
            .then_with(|| left.source_label().cmp(&right.source_label()))
    });
}

fn request_patch_target_matches_body_prefix(target: &str, prefix: &str) -> bool {
    target == prefix || target.starts_with(&format!("{prefix}/"))
}

fn runtime_body_targets_conflict(left_target: &str, right_target: &str) -> bool {
    left_target != right_target
        && (request_patch_target_matches_body_prefix(left_target, right_target)
            || request_patch_target_matches_body_prefix(right_target, left_target))
}

fn is_runtime_body_conflict(
    left: &RuntimeResolvedRequestPatch,
    right: &RuntimeResolvedRequestPatch,
) -> bool {
    left.placement == RequestPatchPlacement::Body
        && right.placement == RequestPatchPlacement::Body
        && runtime_body_targets_conflict(&left.target, &right.target)
}

fn runtime_request_patch_conflict(
    left: &RuntimeResolvedRequestPatch,
    right: &RuntimeResolvedRequestPatch,
) -> RuntimeRequestPatchConflict {
    let left_priority = request_patch_source_priority(&left.source);
    let right_priority = request_patch_source_priority(&right.source);
    let (lower, higher) = if left_priority <= right_priority {
        (left, right)
    } else {
        (right, left)
    };

    RuntimeRequestPatchConflict {
        placement: RequestPatchPlacement::Body,
        lower_priority_source: lower.source.clone(),
        higher_priority_source: higher.source.clone(),
        lower_priority_target: lower.target.clone(),
        higher_priority_target: higher.target.clone(),
        reason: format!(
            "{} BODY target '{}' conflicts with higher-priority {} BODY target '{}'",
            lower.source_label(),
            lower.target,
            higher.source_label(),
            higher.target
        ),
    }
}

fn push_unique_source(sources: &mut Vec<RequestPatchSource>, source: RequestPatchSource) {
    if !sources.contains(&source) {
        sources.push(source);
    }
}

fn push_unique_rule_id(rule_ids: &mut Vec<i64>, rule_id: i64) {
    if !rule_ids.contains(&rule_id) {
        rule_ids.push(rule_id);
    }
}

fn record_overridden_runtime_patch(
    overriding_rule: &mut RuntimeResolvedRequestPatch,
    overridden_rule: &RuntimeResolvedRequestPatch,
) {
    push_unique_source(
        &mut overriding_rule.overridden_sources,
        overridden_rule.source.clone(),
    );
    if let Some(rule_id) = overridden_rule.source.rule_id() {
        push_unique_rule_id(&mut overriding_rule.overridden_rule_ids, rule_id);
    }
    for source in &overridden_rule.overridden_sources {
        push_unique_source(&mut overriding_rule.overridden_sources, source.clone());
    }
    for rule_id in &overridden_rule.overridden_rule_ids {
        push_unique_rule_id(&mut overriding_rule.overridden_rule_ids, *rule_id);
    }
}

fn merge_runtime_request_patches(
    mut base_rules: Vec<RuntimeResolvedRequestPatch>,
    generated_rules: Vec<RuntimeResolvedRequestPatch>,
) -> (
    Vec<RuntimeResolvedRequestPatch>,
    Vec<RuntimeRequestPatchConflict>,
) {
    let mut conflicts = Vec::new();

    for mut generated_rule in generated_rules {
        let mut retained_rules = Vec::with_capacity(base_rules.len());
        for existing_rule in base_rules {
            if is_runtime_body_conflict(&existing_rule, &generated_rule) {
                conflicts.push(runtime_request_patch_conflict(
                    &existing_rule,
                    &generated_rule,
                ));
                retained_rules.push(existing_rule);
                continue;
            }

            if existing_rule.placement == generated_rule.placement
                && existing_rule.target == generated_rule.target
            {
                record_overridden_runtime_patch(&mut generated_rule, &existing_rule);
                continue;
            }

            retained_rules.push(existing_rule);
        }

        retained_rules.push(generated_rule);
        base_rules = retained_rules;
    }

    stable_sort_runtime_request_patches(&mut base_rules);
    conflicts.sort_by(|left, right| {
        left.lower_priority_target
            .cmp(&right.lower_priority_target)
            .then_with(|| {
                left.higher_priority_target
                    .cmp(&right.higher_priority_target)
            })
            .then_with(|| {
                left.lower_priority_source
                    .label()
                    .cmp(&right.lower_priority_source.label())
            })
            .then_with(|| {
                left.higher_priority_source
                    .label()
                    .cmp(&right.higher_priority_source.label())
            })
    });

    (base_rules, conflicts)
}

fn generated_reasoning_patch_to_runtime(
    target: &ExecutionTarget,
    patch: GeneratedReasoningPatch,
) -> Result<RuntimeResolvedRequestPatch, ProxyError> {
    let config_id = target.reasoning_config_id.ok_or_else(|| {
        patch_error(format!(
            "target provider '{}' model '{}' is missing reasoning_config_id for generated patch",
            target.provider.provider_key, target.model.model_name
        ))
    })?;
    let config_preset_id = target.reasoning_config_preset_id.ok_or_else(|| {
        patch_error(format!(
            "target provider '{}' model '{}' is missing reasoning_config_preset_id for generated patch",
            target.provider.provider_key, target.model.model_name
        ))
    })?;
    let config_scope = target.reasoning_config_scope.ok_or_else(|| {
        patch_error(format!(
            "target provider '{}' model '{}' is missing reasoning_config_scope for generated patch",
            target.provider.provider_key, target.model.model_name
        ))
    })?;

    Ok(RuntimeResolvedRequestPatch {
        placement: patch.placement,
        target: patch.target,
        operation: patch.operation,
        value_json: patch.value_json,
        source: RequestPatchSource::ReasoningPreset {
            config_id,
            config_scope,
            config_preset_id,
            family: patch.family,
            preset: patch.preset,
            suffix: patch.suffix,
        },
        source_rule_id: None,
        source_origin: None,
        overridden_rule_ids: Vec::new(),
        overridden_sources: Vec::new(),
        description: patch.description,
    })
}

fn generate_target_reasoning_request_patches(
    target: Option<&ExecutionTarget>,
) -> Result<Vec<RuntimeResolvedRequestPatch>, ProxyError> {
    let Some(target) = target else {
        return Ok(Vec::new());
    };

    let Some(family) = target.reasoning_family else {
        return Ok(Vec::new());
    };
    let preset = target.reasoning_preset.ok_or_else(|| {
        patch_error(format!(
            "target provider '{}' model '{}' has reasoning family but no preset",
            target.provider.provider_key, target.model.model_name
        ))
    })?;

    generate_reasoning_patches(
        family,
        preset,
        ReasoningPatchContext::for_model(target.upstream_protocol, &target.model),
    )
    .map_err(|err| patch_error(err.to_string()))?
    .into_iter()
    .map(|patch| generated_reasoning_patch_to_runtime(target, patch))
    .collect()
}

pub(crate) async fn load_runtime_request_patch_trace(
    provider: &CacheProvider,
    model: Option<&CacheModel>,
    target: Option<&ExecutionTarget>,
    app_state: &Arc<AppState>,
) -> Result<RuntimeRequestPatchTrace, ProxyError> {
    let generated_rules = generate_target_reasoning_request_patches(target)?;

    if let Some(model) = model {
        let resolved = app_state
            .catalog
            .get_model_effective_request_patches(model.id)
            .await
            .map_err(|err| {
                error!(
                    "Failed to get effective request patches for model_id {}: {:?}",
                    model.id, err
                );
                patch_error(format!(
                    "Failed to retrieve effective request patches for model '{}'",
                    model.model_name
                ))
            })?
            .ok_or_else(|| {
                patch_error(format!(
                    "Effective request patch snapshot is missing for model '{}'",
                    model.model_name
                ))
            })?;

        return build_runtime_request_patch_trace(
            resolved.effective_rules.clone(),
            resolved.conflicts.clone(),
            resolved.has_conflicts,
            generated_rules,
        );
    }

    let provider_rules = app_state
        .catalog
        .get_provider_request_patch_rules(provider.id)
        .await
        .map_err(|err| {
            error!(
                "Failed to get provider request patches for provider_id {}: {:?}",
                provider.id, err
            );
            patch_error(format!(
                "Failed to retrieve request patches for provider '{}'",
                provider.name
            ))
        })?;

    let resolved = resolve_effective_request_patches(provider.id, 0, &provider_rules, &[]);
    build_runtime_request_patch_trace(
        resolved.effective_rules,
        resolved.conflicts,
        resolved.has_conflicts,
        generated_rules,
    )
}
