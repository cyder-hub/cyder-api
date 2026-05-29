use std::sync::Arc;

use axum::http::HeaderMap;
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue as ReqwestHeaderValue},
};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    proxy::{
        ProxyError,
        reasoning_suffix::{
            GeneratedReasoningPatch, ReasoningPatchContext, generate_reasoning_patches,
        },
        runtime::route_resolver::ExecutionCandidate,
    },
    schema::enum_def::{RequestPatchOperation, RequestPatchPlacement},
    service::{
        app_state::AppState,
        cache::types::{
            CacheModel, CacheProvider, CacheRequestPatchConflict, CacheRequestPatchExplainEntry,
            CacheResolvedRequestPatch, RequestPatchSource, RuntimeRequestPatchConflict,
            RuntimeResolvedRequestPatch,
        },
        request_patch::resolve_effective_request_patches,
    },
    utils::storage::RequestLogBundleQueryParam,
};
use cyder_tools::log::{debug, error};

#[derive(Debug, Clone)]
pub(crate) struct RuntimeRequestPatchTrace {
    pub applied_rules: Vec<RuntimeResolvedRequestPatch>,
    pub conflicts: Vec<RuntimeRequestPatchConflict>,
    pub has_conflicts: bool,
    pub applied_request_patch_ids_json: Option<String>,
    pub request_patch_summary_json: Option<String>,
}

#[derive(Debug, Serialize)]
struct RequestPatchTraceSummary {
    provider_id: i64,
    model_id: Option<i64>,
    effective_rules: Vec<RuntimeResolvedRequestPatch>,
    explain: Vec<CacheRequestPatchExplainEntry>,
    conflicts: Vec<RuntimeRequestPatchConflict>,
    has_conflicts: bool,
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

        Some(ProxyError::RequestPatchConflict(format!(
            "Request patch conflicts prevent model '{}' from being used: {}",
            model_name, reasons
        )))
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
        ProxyError::InternalError(format!(
            "{} is missing value_json for SET",
            rule.source_label()
        ))
    })?;

    serde_json::from_str(raw).map_err(|err| {
        ProxyError::InternalError(format!(
            "{} has invalid value_json: {}",
            rule.source_label(),
            err
        ))
    })
}

fn parse_json_pointer_segments(pointer: &str) -> Result<Vec<String>, ProxyError> {
    if pointer.is_empty() || !pointer.starts_with('/') {
        return Err(ProxyError::InternalError(format!(
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
                            return Err(ProxyError::InternalError(format!(
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
        ProxyError::InternalError(format!(
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
                    ProxyError::InternalError(format!(
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
            other => Err(ProxyError::InternalError(format!(
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
                    ProxyError::InternalError(format!(
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
            other => Err(ProxyError::InternalError(format!(
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
                Err(ProxyError::InternalError(format!(
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
        other => Err(ProxyError::InternalError(format!(
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

pub(in crate::proxy) fn rebuild_gemini_url_query_from_snapshot(
    final_url: &str,
    snapshot_query_params: &[RequestLogBundleQueryParam],
    is_stream: bool,
    request_patches: &[RuntimeResolvedRequestPatch],
) -> Result<String, ProxyError> {
    let mut url = Url::parse(final_url)
        .map_err(|_| ProxyError::BadRequest("failed to parse target url".to_string()))?;
    let mut query_params = snapshot_query_params
        .iter()
        .filter(|param| !param.name.eq_ignore_ascii_case("key"))
        .cloned()
        .collect::<Vec<_>>();

    if is_stream {
        query_params.push(RequestLogBundleQueryParam {
            name: "alt".to_string(),
            value: Some("sse".to_string()),
            value_present: true,
            encoded_name: None,
            encoded_value: None,
        });
    }

    for rule in request_patches
        .iter()
        .filter(|rule| rule.placement == RequestPatchPlacement::Query)
    {
        query_params.retain(|param| param.name != rule.target);
        if rule.operation == RequestPatchOperation::Set {
            query_params.push(RequestLogBundleQueryParam {
                name: rule.target.clone(),
                value: Some(scalar_request_patch_value(rule)?),
                value_present: true,
                encoded_name: None,
                encoded_value: None,
            });
        }
    }

    set_url_query_from_ordered_params(&mut url, &query_params);
    Ok(url.to_string())
}

fn set_url_query_from_ordered_params(url: &mut Url, query_params: &[RequestLogBundleQueryParam]) {
    if query_params.is_empty() {
        url.set_query(None);
        return;
    }

    let query = query_params
        .iter()
        .map(|param| {
            let name = param
                .encoded_name
                .as_ref()
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or_else(|| percent_encode_query_component(&param.name));
            if param.has_value() {
                let value = param.encoded_value.clone().unwrap_or_else(|| {
                    percent_encode_query_component(param.value.as_deref().unwrap_or_default())
                });
                format!("{name}={value}")
            } else {
                name
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    url.set_query(Some(&query));
}

fn percent_encode_query_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn apply_header_request_patch(
    headers: &mut HeaderMap,
    rule: &RuntimeResolvedRequestPatch,
) -> Result<(), ProxyError> {
    let header_name = HeaderName::from_bytes(rule.target.as_bytes()).map_err(|err| {
        ProxyError::InternalError(format!(
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
                    ProxyError::InternalError(format!(
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
    provider_id: i64,
    model_id: Option<i64>,
    effective_rules: Vec<CacheResolvedRequestPatch>,
    explain: Vec<CacheRequestPatchExplainEntry>,
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

    let applied_request_patch_ids_json = serde_json::to_string(
        &applied_rules
            .iter()
            .filter_map(|rule| rule.source.rule_id())
            .collect::<Vec<_>>(),
    )
    .map(Some)
    .map_err(|err| {
        ProxyError::InternalError(format!(
            "Failed to serialize applied request patch IDs: {}",
            err
        ))
    })?;

    let request_patch_summary_json = serde_json::to_string(&RequestPatchTraceSummary {
        provider_id,
        model_id,
        effective_rules: runtime_rules,
        explain,
        conflicts: runtime_conflicts.clone(),
        has_conflicts,
    })
    .map(Some)
    .map_err(|err| {
        ProxyError::InternalError(format!(
            "Failed to serialize request patch summary: {}",
            err
        ))
    })?;

    Ok(RuntimeRequestPatchTrace {
        applied_rules,
        conflicts: runtime_conflicts,
        has_conflicts,
        applied_request_patch_ids_json,
        request_patch_summary_json,
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
    candidate: &ExecutionCandidate,
    patch: GeneratedReasoningPatch,
) -> Result<RuntimeResolvedRequestPatch, ProxyError> {
    let config_id = candidate.reasoning_config_id.ok_or_else(|| {
        ProxyError::InternalError(format!(
            "candidate provider '{}' model '{}' is missing reasoning_config_id for generated patch",
            candidate.provider.provider_key, candidate.model.model_name
        ))
    })?;
    let config_preset_id = candidate.reasoning_config_preset_id.ok_or_else(|| {
        ProxyError::InternalError(format!(
            "candidate provider '{}' model '{}' is missing reasoning_config_preset_id for generated patch",
            candidate.provider.provider_key, candidate.model.model_name
        ))
    })?;
    let config_scope = candidate.reasoning_config_scope.ok_or_else(|| {
        ProxyError::InternalError(format!(
            "candidate provider '{}' model '{}' is missing reasoning_config_scope for generated patch",
            candidate.provider.provider_key, candidate.model.model_name
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

fn generate_candidate_reasoning_request_patches(
    candidate: Option<&ExecutionCandidate>,
) -> Result<Vec<RuntimeResolvedRequestPatch>, ProxyError> {
    let Some(candidate) = candidate else {
        return Ok(Vec::new());
    };

    let Some(family) = candidate.reasoning_family else {
        return Ok(Vec::new());
    };
    let preset = candidate.reasoning_preset.ok_or_else(|| {
        ProxyError::InternalError(format!(
            "candidate provider '{}' model '{}' has reasoning family but no preset",
            candidate.provider.provider_key, candidate.model.model_name
        ))
    })?;

    generate_reasoning_patches(
        family,
        preset,
        ReasoningPatchContext::for_model(candidate.llm_api_type, &candidate.model),
    )
    .map_err(|err| ProxyError::BadRequest(err.to_string()))?
    .into_iter()
    .map(|patch| generated_reasoning_patch_to_runtime(candidate, patch))
    .collect()
}

pub(crate) async fn load_runtime_request_patch_trace(
    provider: &CacheProvider,
    model: Option<&CacheModel>,
    candidate: Option<&ExecutionCandidate>,
    app_state: &Arc<AppState>,
) -> Result<RuntimeRequestPatchTrace, ProxyError> {
    let generated_rules = generate_candidate_reasoning_request_patches(candidate)?;

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
                ProxyError::InternalError(format!(
                    "Failed to retrieve effective request patches for model '{}'",
                    model.model_name
                ))
            })?
            .ok_or_else(|| {
                ProxyError::InternalError(format!(
                    "Effective request patch snapshot is missing for model '{}'",
                    model.model_name
                ))
            })?;

        return build_runtime_request_patch_trace(
            provider.id,
            Some(model.id),
            resolved.effective_rules.clone(),
            resolved.explain.clone(),
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
            ProxyError::InternalError(format!(
                "Failed to retrieve request patches for provider '{}'",
                provider.name
            ))
        })?;

    let resolved = resolve_effective_request_patches(provider.id, 0, &provider_rules, &[]);
    build_runtime_request_patch_trace(
        provider.id,
        None,
        resolved.effective_rules,
        resolved.explain,
        resolved.conflicts,
        resolved.has_conflicts,
        generated_rules,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::{HeaderMap, HeaderValue};
    use reqwest::Url;
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        database::reasoning_config::{ReasoningConfigScope, ReasoningPatchFamily, ReasoningPreset},
        proxy::runtime::route_resolver::{CandidateRuntimeFeatures, RuntimeFeatureConfigSource},
        schema::enum_def::{LlmApiType, ProviderApiKeyMode, ProviderType},
        service::cache::types::{RequestPatchRuleOrigin, RuntimeResolvedRequestPatch},
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

    fn bound_reasoning_candidate() -> ExecutionCandidate {
        ExecutionCandidate {
            candidate_position: 1,
            route_id: None,
            route_name: None,
            route_candidate_priority: None,
            provider: Arc::new(provider(1, "openai", ProviderType::Openai)),
            model: Arc::new(model_with_id(10, 1, "gpt-primary", Some("gpt-real"))),
            llm_api_type: LlmApiType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            reasoning_config_id: Some(900),
            reasoning_config_scope: Some(ReasoningConfigScope::Provider),
            reasoning_config_source: None,
            reasoning_config_preset_id: Some(9000),
            reasoning_family: Some(ReasoningPatchFamily::OpenAiChatReasoningEffort),
            reasoning_preset: Some(ReasoningPreset::High),
            reasoning_suffix: Some("high".to_string()),
            runtime_features: CandidateRuntimeFeatures {
                openai_reasoning_content_repair_enabled: false,
                openai_reasoning_content_repair_source: RuntimeFeatureConfigSource::DefaultFalse,
            },
        }
    }

    fn request_patch(
        id: i64,
        placement: RequestPatchPlacement,
        target: &str,
        operation: RequestPatchOperation,
        value: Option<Value>,
    ) -> RuntimeResolvedRequestPatch {
        RuntimeResolvedRequestPatch {
            placement,
            target: target.to_string(),
            operation,
            value_json: value.map(|item| serde_json::to_string(&item).unwrap()),
            source: RequestPatchSource::ProviderRule { rule_id: id },
            source_rule_id: Some(id),
            source_origin: Some(RequestPatchRuleOrigin::ProviderDirect),
            overridden_rule_ids: Vec::new(),
            overridden_sources: Vec::new(),
            description: None,
        }
    }

    fn cache_request_patch(
        id: i64,
        origin: RequestPatchRuleOrigin,
        placement: RequestPatchPlacement,
        target: &str,
        value: Option<Value>,
    ) -> CacheResolvedRequestPatch {
        CacheResolvedRequestPatch {
            placement,
            target: target.to_string(),
            operation: RequestPatchOperation::Set,
            value_json: value.map(|item| serde_json::to_string(&item).unwrap()),
            source_rule_id: id,
            source_origin: origin,
            overridden_rule_ids: Vec::new(),
            description: None,
        }
    }

    fn reasoning_runtime_patch(target: &str, value: Value) -> RuntimeResolvedRequestPatch {
        RuntimeResolvedRequestPatch {
            placement: RequestPatchPlacement::Body,
            target: target.to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some(serde_json::to_string(&value).unwrap()),
            source: RequestPatchSource::ReasoningPreset {
                config_id: 900,
                config_scope: ReasoningConfigScope::Provider,
                config_preset_id: 9000,
                family: ReasoningPatchFamily::OpenAiChatReasoningEffort,
                preset: ReasoningPreset::High,
                suffix: "high".to_string(),
            },
            source_rule_id: None,
            source_origin: None,
            overridden_rule_ids: Vec::new(),
            overridden_sources: Vec::new(),
            description: Some("generated test reasoning patch".to_string()),
        }
    }

    #[test]
    fn apply_request_patches_creates_missing_body_parents_and_removes_object_fields() {
        let mut data = json!({
            "generation_config": {
                "temperature": 0.2,
                "remove_me": "stale"
            }
        });
        let mut url = Url::parse("https://example.com/v1/chat").unwrap();
        let mut headers = HeaderMap::new();
        let request_patches = vec![
            request_patch(
                1,
                RequestPatchPlacement::Body,
                "/generation_config/temperature",
                RequestPatchOperation::Set,
                Some(json!(0.8)),
            ),
            request_patch(
                2,
                RequestPatchPlacement::Body,
                "/generation_config/response_schema",
                RequestPatchOperation::Set,
                Some(json!({
                    "type": "object",
                    "strict": true
                })),
            ),
            request_patch(
                3,
                RequestPatchPlacement::Body,
                "/generation_config/remove_me",
                RequestPatchOperation::Remove,
                None,
            ),
        ];

        apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)
            .expect("request patches should apply");

        assert_eq!(
            data["generation_config"]["response_schema"],
            json!({
                "type": "object",
                "strict": true
            })
        );
        assert!(data["generation_config"]["remove_me"].is_null());
        let temperature = data["generation_config"]["temperature"].as_f64().unwrap();
        assert!((temperature - 0.8).abs() < 1e-6);
    }

    #[test]
    fn apply_request_patches_rejects_body_type_conflicts_without_rewriting_structure() {
        let mut data = json!({
            "metadata": "raw"
        });
        let mut url = Url::parse("https://example.com/v1/chat").unwrap();
        let mut headers = HeaderMap::new();
        let request_patches = vec![request_patch(
            1,
            RequestPatchPlacement::Body,
            "/metadata/flags/enabled",
            RequestPatchOperation::Set,
            Some(json!(true)),
        )];

        let err = apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)
            .expect_err("scalar parent should fail closed");

        assert!(matches!(err, ProxyError::InternalError(_)));
        assert_eq!(data, json!({ "metadata": "raw" }));
    }

    #[test]
    fn apply_request_patches_replaces_query_values_and_supports_remove() {
        let mut data = Value::Null;
        let mut url =
            Url::parse("https://example.com/v1/chat?keep=1&mode=old&remove=gone").unwrap();
        let mut headers = HeaderMap::new();
        let request_patches = vec![
            request_patch(
                1,
                RequestPatchPlacement::Query,
                "mode",
                RequestPatchOperation::Set,
                Some(json!("new")),
            ),
            request_patch(
                2,
                RequestPatchPlacement::Query,
                "enabled",
                RequestPatchOperation::Set,
                Some(json!(true)),
            ),
            request_patch(
                3,
                RequestPatchPlacement::Query,
                "remove",
                RequestPatchOperation::Remove,
                None,
            ),
        ];

        apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)
            .expect("query request patches should apply");

        let params: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();

        assert_eq!(
            params,
            vec![
                ("keep".to_string(), "1".to_string()),
                ("mode".to_string(), "new".to_string()),
                ("enabled".to_string(), "true".to_string()),
            ]
        );
    }

    #[test]
    fn rebuild_gemini_url_query_from_snapshot_preserves_order_flags_empty_and_encoding() {
        let snapshot = vec![
            RequestLogBundleQueryParam {
                name: "tag".to_string(),
                value: Some("a".to_string()),
                value_present: true,
                encoded_name: Some("tag".to_string()),
                encoded_value: Some("a".to_string()),
            },
            RequestLogBundleQueryParam {
                name: "tag".to_string(),
                value: Some("b".to_string()),
                value_present: true,
                encoded_name: Some("tag".to_string()),
                encoded_value: Some("b".to_string()),
            },
            RequestLogBundleQueryParam {
                name: "flag".to_string(),
                value: None,
                value_present: false,
                encoded_name: Some("flag".to_string()),
                encoded_value: None,
            },
            RequestLogBundleQueryParam {
                name: "mode".to_string(),
                value: Some(String::new()),
                value_present: true,
                encoded_name: Some("mode".to_string()),
                encoded_value: Some(String::new()),
            },
            RequestLogBundleQueryParam {
                name: "q".to_string(),
                value: Some("a b".to_string()),
                value_present: true,
                encoded_name: Some("q".to_string()),
                encoded_value: Some("a%20b".to_string()),
            },
        ];

        let final_url = rebuild_gemini_url_query_from_snapshot(
            "https://example.com/v1beta/models/gemini:generateContent?stale=1",
            &snapshot,
            false,
            &[],
        )
        .expect("query should rebuild");

        assert_eq!(
            final_url,
            "https://example.com/v1beta/models/gemini:generateContent?tag=a&tag=b&flag&mode=&q=a%20b"
        );
    }

    #[test]
    fn rebuild_gemini_url_query_from_snapshot_preserves_original_plus_and_percent_encoding() {
        let snapshot = vec![
            RequestLogBundleQueryParam {
                name: "space".to_string(),
                value: Some("a b".to_string()),
                value_present: true,
                encoded_name: Some("space".to_string()),
                encoded_value: Some("a%20b".to_string()),
            },
            RequestLogBundleQueryParam {
                name: "plus".to_string(),
                value: Some("a b".to_string()),
                value_present: true,
                encoded_name: Some("plus".to_string()),
                encoded_value: Some("a+b".to_string()),
            },
            RequestLogBundleQueryParam {
                name: "literal".to_string(),
                value: Some("a+b".to_string()),
                value_present: true,
                encoded_name: Some("literal".to_string()),
                encoded_value: Some("a%2Bb".to_string()),
            },
        ];

        let final_url = rebuild_gemini_url_query_from_snapshot(
            "https://example.com/v1beta/models/gemini:generateContent?stale=1",
            &snapshot,
            false,
            &[],
        )
        .expect("query should rebuild");

        assert_eq!(
            final_url,
            "https://example.com/v1beta/models/gemini:generateContent?space=a%20b&plus=a+b&literal=a%2Bb"
        );
    }

    #[test]
    fn rebuild_gemini_url_query_from_snapshot_applies_query_patches_after_snapshot() {
        let snapshot = vec![
            RequestLogBundleQueryParam {
                name: "flag".to_string(),
                value: None,
                value_present: false,
                encoded_name: Some("flag".to_string()),
                encoded_value: None,
            },
            RequestLogBundleQueryParam {
                name: "mode".to_string(),
                value: Some("old".to_string()),
                value_present: true,
                encoded_name: Some("mode".to_string()),
                encoded_value: Some("old".to_string()),
            },
        ];
        let request_patches = vec![
            request_patch(
                1,
                RequestPatchPlacement::Query,
                "flag",
                RequestPatchOperation::Remove,
                None,
            ),
            request_patch(
                2,
                RequestPatchPlacement::Query,
                "mode",
                RequestPatchOperation::Set,
                Some(json!("new")),
            ),
        ];

        let final_url = rebuild_gemini_url_query_from_snapshot(
            "https://example.com/v1beta/models/gemini:streamGenerateContent?flag&mode=old",
            &snapshot,
            true,
            &request_patches,
        )
        .expect("query should rebuild");

        assert_eq!(
            final_url,
            "https://example.com/v1beta/models/gemini:streamGenerateContent?alt=sse&mode=new"
        );
    }

    #[test]
    fn apply_request_patches_updates_headers_and_supports_remove() {
        let mut data = Value::Null;
        let mut url = Url::parse("https://example.com/v1/chat").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-existing", HeaderValue::from_static("old"));
        headers.insert("x-remove", HeaderValue::from_static("remove-me"));
        let request_patches = vec![
            request_patch(
                1,
                RequestPatchPlacement::Header,
                "x-existing",
                RequestPatchOperation::Set,
                Some(json!("new")),
            ),
            request_patch(
                2,
                RequestPatchPlacement::Header,
                "x-remove",
                RequestPatchOperation::Remove,
                None,
            ),
        ];

        apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)
            .expect("header request patches should apply");

        assert_eq!(headers.get("x-existing").unwrap(), "new");
        assert!(headers.get("x-remove").is_none());
    }

    #[test]
    fn apply_request_patches_rejects_invalid_header_value() {
        let mut data = Value::Null;
        let mut url = Url::parse("https://example.com/v1/chat").unwrap();
        let mut headers = HeaderMap::new();
        let request_patches = vec![request_patch(
            1,
            RequestPatchPlacement::Header,
            "x-invalid-value",
            RequestPatchOperation::Set,
            Some(json!("bad\nvalue")),
        )];

        let err = apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)
            .expect_err("invalid header value should fail closed");

        assert!(matches!(err, ProxyError::InternalError(_)));
    }

    #[test]
    fn apply_request_patches_rejects_array_removal_that_rewrites_structure() {
        let mut data = json!({
            "messages": [
                { "role": "user", "content": "hi" }
            ]
        });
        let mut url = Url::parse("https://example.com/v1/chat").unwrap();
        let mut headers = HeaderMap::new();
        let request_patches = vec![request_patch(
            1,
            RequestPatchPlacement::Body,
            "/messages/0",
            RequestPatchOperation::Remove,
            None,
        )];

        let err = apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)
            .expect_err("array removal should fail closed");

        assert!(matches!(err, ProxyError::InternalError(_)));
    }

    #[test]
    fn runtime_trace_merges_reasoning_patch_after_provider_model_patches() {
        let base_rules = vec![cache_request_patch(
            11,
            RequestPatchRuleOrigin::ProviderDirect,
            RequestPatchPlacement::Body,
            "/reasoning_effort",
            Some(json!("low")),
        )];
        let generated_rules = vec![reasoning_runtime_patch("/reasoning_effort", json!("high"))];

        let trace = build_runtime_request_patch_trace(
            1,
            Some(10),
            base_rules,
            Vec::new(),
            Vec::new(),
            false,
            generated_rules,
        )
        .expect("runtime trace should build");

        assert!(!trace.has_conflicts);
        assert_eq!(trace.applied_rules.len(), 1);
        let applied = &trace.applied_rules[0];
        assert!(matches!(
            &applied.source,
            RequestPatchSource::ReasoningPreset {
                config_id: 900,
                config_scope: ReasoningConfigScope::Provider,
                config_preset_id: 9000,
                ..
            }
        ));
        assert_eq!(applied.source_rule_id, None);
        assert_eq!(applied.source_origin, None);
        assert_eq!(applied.overridden_rule_ids, vec![11]);
        assert!(matches!(
            applied.overridden_sources.as_slice(),
            [RequestPatchSource::ProviderRule { rule_id: 11 }]
        ));
        assert_eq!(trace.applied_request_patch_ids_json.as_deref(), Some("[]"));

        let summary: Value = serde_json::from_str(
            trace
                .request_patch_summary_json
                .as_deref()
                .expect("summary should serialize"),
        )
        .expect("summary should be json");
        assert_eq!(
            summary["effective_rules"][0]["source"]["kind"],
            "reasoning_preset"
        );
        assert_eq!(summary["effective_rules"][0]["source"]["config_id"], 900);
        assert_eq!(
            summary["effective_rules"][0]["source"]["config_scope"],
            "provider"
        );
        assert_eq!(
            summary["effective_rules"][0]["source"]["config_preset_id"],
            9000
        );
        assert_eq!(
            summary["effective_rules"][0]["source"]["profile_id"],
            Value::Null
        );
        assert_eq!(summary["effective_rules"][0]["source_rule_id"], Value::Null);
        assert_eq!(
            summary["effective_rules"][0]["overridden_sources"][0]["kind"],
            "provider_rule"
        );

        let mut data = json!({ "reasoning_effort": "client" });
        let mut url = Url::parse("https://example.com/v1/chat").unwrap();
        let mut headers = HeaderMap::new();
        apply_request_patches(&mut data, &mut url, &mut headers, &trace.applied_rules)
            .expect("merged runtime patch should apply");
        assert_eq!(data["reasoning_effort"], json!("high"));
    }

    #[test]
    fn runtime_trace_reports_reasoning_body_ancestor_conflict() {
        let base_rules = vec![cache_request_patch(
            11,
            RequestPatchRuleOrigin::ProviderDirect,
            RequestPatchPlacement::Body,
            "/reasoning",
            Some(json!({ "effort": "low" })),
        )];
        let generated_rules = vec![reasoning_runtime_patch("/reasoning/effort", json!("high"))];

        let trace = build_runtime_request_patch_trace(
            1,
            Some(10),
            base_rules,
            Vec::new(),
            Vec::new(),
            false,
            generated_rules,
        )
        .expect("runtime trace should build");

        assert!(trace.has_conflicts);
        assert!(trace.applied_rules.is_empty());
        assert_eq!(trace.applied_request_patch_ids_json.as_deref(), Some("[]"));
        assert_eq!(trace.conflicts.len(), 1);
        assert!(matches!(
            &trace.conflicts[0].lower_priority_source,
            RequestPatchSource::ProviderRule { rule_id: 11 }
        ));
        assert!(matches!(
            &trace.conflicts[0].higher_priority_source,
            RequestPatchSource::ReasoningPreset { .. }
        ));
        assert!(
            trace.conflicts[0].reason.contains("reasoning preset patch"),
            "{:?}",
            trace.conflicts[0]
        );
        assert!(
            trace.conflicts[0]
                .reason
                .contains("config=provider/900 preset_row=9000"),
            "{:?}",
            trace.conflicts[0]
        );
        let err = trace
            .conflict_error("gpt-primary")
            .expect("conflict should become proxy error");
        assert!(matches!(err, ProxyError::RequestPatchConflict(_)));
    }

    #[test]
    fn generated_reasoning_patch_uses_bound_config_fields() {
        let candidate = bound_reasoning_candidate();

        let generated_rules = generate_candidate_reasoning_request_patches(Some(&candidate))
            .expect("bound config fields should generate reasoning patch");

        assert_eq!(generated_rules.len(), 1);
        let RequestPatchSource::ReasoningPreset {
            config_id,
            config_scope,
            config_preset_id,
            family,
            preset,
            suffix,
        } = generated_rules[0].source.clone()
        else {
            panic!("expected generated reasoning preset source");
        };
        assert_eq!(config_id, 900);
        assert_eq!(config_scope, ReasoningConfigScope::Provider);
        assert_eq!(config_preset_id, 9000);
        assert_eq!(family, ReasoningPatchFamily::OpenAiChatReasoningEffort);
        assert_eq!(preset, ReasoningPreset::High);
        assert_eq!(suffix, "high");
    }
}
