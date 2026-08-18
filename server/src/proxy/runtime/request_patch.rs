use axum::http::HeaderMap;
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue as ReqwestHeaderValue},
};
use serde_json::{Map, Value};

use crate::{
    database::request_patch::validate_reserved_target,
    proxy::{ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility},
    schema::enum_def::{RequestPatchOperation, RequestPatchPlacement, UpstreamProfileType},
    service::{
        cache::types::{
            CacheRequestPatchConflict, CacheRequestPatchExplainEntry, CacheRequestPatchVariant,
            CacheUpstreamSource, RequestPatchSource, RuntimeRequestPatchConflict,
            RuntimeResolvedRequestPatch,
        },
        request_patch::{RequestPatchEvaluation, RequestPatchLayerState},
    },
};
use cyder_tools::log::debug;

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
    pub source_id: i64,
    pub model_id: Option<i64>,
    pub profile_type: UpstreamProfileType,
    pub suffix: Option<String>,
    pub layers: Vec<RequestPatchLayerState>,
    pub explain: Vec<CacheRequestPatchExplainEntry>,
    pub applied_rules: Vec<RuntimeResolvedRequestPatch>,
    pub conflicts: Vec<RuntimeRequestPatchConflict>,
    pub has_conflicts: bool,
    pub executable: bool,
    pub failure_reason: Option<String>,
}

impl RuntimeRequestPatchTrace {
    pub(crate) fn execution_error(
        &self,
        provider_key: &str,
        model_name: &str,
    ) -> Option<ProxyError> {
        if self.executable {
            return None;
        }
        if self.has_conflicts {
            return self.conflict_error(model_name);
        }
        let reason = self
            .failure_reason
            .clone()
            .unwrap_or_else(|| "Request Patch configuration is not executable".to_string());
        Some(ProxyError::gateway(
            ProxyErrorCode::UnsupportedCapabilityError,
            ExecutionStage::Patch,
            ResponseVisibility::NotVisible,
            None,
            format!(
                "Request Patch configuration for provider '{}' model '{}'{} is unavailable: {}",
                provider_key,
                model_name,
                self.suffix
                    .as_deref()
                    .map(|suffix| format!(" suffix '{suffix}'"))
                    .unwrap_or_default(),
                reason
            ),
        ))
    }

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
        if validate_reserved_target(rule.placement, &rule.target).is_err() {
            return Err(patch_error(format!(
                "{} targets reserved {:?} path '{}'",
                rule.source_label(),
                rule.placement,
                rule.target
            )));
        }
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
    source: &CacheUpstreamSource,
    model_id: Option<i64>,
    suffix: Option<String>,
    evaluation: RequestPatchEvaluation,
) -> RuntimeRequestPatchTrace {
    let runtime_rules = evaluation
        .effective_rules
        .into_iter()
        .map(RuntimeResolvedRequestPatch::from)
        .collect::<Vec<_>>();
    let runtime_conflicts = evaluation
        .conflicts
        .into_iter()
        .map(RuntimeRequestPatchConflict::from)
        .collect::<Vec<_>>();
    let has_conflicts = evaluation.has_conflicts || !runtime_conflicts.is_empty();
    RuntimeRequestPatchTrace {
        source_id: source.id,
        model_id,
        profile_type: source.profile_type,
        suffix,
        layers: evaluation.layers,
        explain: evaluation.explain,
        applied_rules: if has_conflicts {
            Vec::new()
        } else {
            runtime_rules
        },
        conflicts: runtime_conflicts,
        has_conflicts,
        executable: evaluation.executable && !has_conflicts,
        failure_reason: evaluation.failure_reason,
    }
}

pub(crate) fn resolve_runtime_request_patch_trace(
    source: &CacheUpstreamSource,
    model_id: Option<i64>,
    suffix: Option<String>,
    variants: &[CacheRequestPatchVariant],
) -> RuntimeRequestPatchTrace {
    let evaluation = crate::service::request_patch::evaluate_request_patch_variants(
        variants,
        source.id,
        model_id,
        suffix.as_deref(),
    );
    build_runtime_request_patch_trace(source, model_id, suffix, evaluation)
}

impl From<CacheRequestPatchConflict> for RuntimeRequestPatchConflict {
    fn from(conflict: CacheRequestPatchConflict) -> Self {
        Self {
            placement: conflict.placement,
            lower_priority_source: RequestPatchSource::Variant {
                variant_id: conflict.lower_priority_variant_id,
                origin: conflict.lower_priority_origin,
            },
            higher_priority_source: RequestPatchSource::Variant {
                variant_id: conflict.higher_priority_variant_id,
                origin: conflict.higher_priority_origin,
            },
            lower_priority_target: conflict.lower_priority_target,
            higher_priority_target: conflict.higher_priority_target,
            reason: conflict.reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol};
    use crate::service::cache::types::{RequestPatchVariantOrigin, RuntimeResolvedRequestPatch};
    use crate::service::transform::validate_final_generation_request_for_downstream;

    fn runtime_rule(
        operation: RequestPatchOperation,
        target: &str,
        value_json: Option<&str>,
    ) -> RuntimeResolvedRequestPatch {
        RuntimeResolvedRequestPatch {
            placement: RequestPatchPlacement::Body,
            target: target.to_string(),
            operation,
            value_json: value_json.map(str::to_string),
            source: RequestPatchSource::Variant {
                variant_id: 1,
                origin: RequestPatchVariantOrigin::SourceBase,
            },
            source_rule_id: Some(1),
            source_origin: Some(RequestPatchVariantOrigin::SourceBase),
            overridden_rule_ids: Vec::new(),
            overridden_sources: Vec::new(),
            description: None,
        }
    }

    #[test]
    fn body_set_and_remove_use_json_pointer() {
        let mut body = serde_json::json!({"options": {"temperature": 0.1, "top_p": 0.9}});
        let mut url = Url::parse("https://example.test").unwrap();
        let mut headers = HeaderMap::new();
        apply_request_patches(
            &mut body,
            &mut url,
            &mut headers,
            &[
                runtime_rule(
                    RequestPatchOperation::Set,
                    "/options/temperature",
                    Some("0.2"),
                ),
                runtime_rule(RequestPatchOperation::Remove, "/options/top_p", None),
            ],
        )
        .unwrap();
        assert_eq!(body["options"]["temperature"], serde_json::json!(0.2));
        assert!(body["options"].get("top_p").is_none());
    }

    #[test]
    fn anthropic_max_tokens_patch_is_revalidated_after_synthesis() {
        let base = serde_json::json!({
            "model":"claude-real",
            "max_tokens":4096,
            "messages":[{"role":"user","content":"hello"}]
        });
        let mut url = Url::parse("https://api.anthropic.test/messages").unwrap();
        let mut headers = HeaderMap::new();

        let mut valid = base.clone();
        apply_request_patches(
            &mut valid,
            &mut url,
            &mut headers,
            &[runtime_rule(
                RequestPatchOperation::Set,
                "/max_tokens",
                Some("128"),
            )],
        )
        .expect("positive max_tokens Patch should apply");
        assert_eq!(valid["max_tokens"], 128);
        validate_final_generation_request_for_downstream(
            &valid,
            DownstreamProtocol::Openai,
            UpstreamProtocol::Anthropic,
            &UpstreamProfileType::Anthropic,
        )
        .expect("positive patched value should pass final validation");

        for (operation, value) in [
            (RequestPatchOperation::Set, Some("0")),
            (RequestPatchOperation::Set, Some("-1")),
            (RequestPatchOperation::Set, Some("1.5")),
            (RequestPatchOperation::Set, Some("4294967296")),
            (RequestPatchOperation::Remove, None),
        ] {
            let mut invalid = base.clone();
            apply_request_patches(
                &mut invalid,
                &mut url,
                &mut headers,
                &[runtime_rule(operation, "/max_tokens", value)],
            )
            .expect("syntactically valid Patch should reach final validation");
            let error = validate_final_generation_request_for_downstream(
                &invalid,
                DownstreamProtocol::Openai,
                UpstreamProtocol::Anthropic,
                &UpstreamProfileType::Anthropic,
            )
            .expect_err("invalid patched max_tokens must fail closed");
            assert_eq!(error.path, "/max_tokens");
        }
    }

    #[test]
    fn runtime_rejects_persisted_forbidden_targets_before_mutation() {
        let mut body = serde_json::json!({"model": "safe-model", "messages": []});
        let mut url = Url::parse("https://example.test?existing=safe").unwrap();
        let mut headers = HeaderMap::new();

        let forbidden_body = runtime_rule(RequestPatchOperation::Set, "/model", Some("\"other\""));
        let error = apply_request_patches(
            &mut body,
            &mut url,
            &mut headers,
            std::slice::from_ref(&forbidden_body),
        )
        .expect_err("runtime must defend against a corrupted forbidden body Patch");
        assert!(error.operator_message().contains("/model"));
        assert_eq!(body["model"], "safe-model");

        let mut forbidden_header = runtime_rule(
            RequestPatchOperation::Set,
            "authorization",
            Some("\"Bearer leaked\""),
        );
        forbidden_header.placement = RequestPatchPlacement::Header;
        let error = apply_request_patches(
            &mut body,
            &mut url,
            &mut headers,
            std::slice::from_ref(&forbidden_header),
        )
        .expect_err("runtime must defend against a corrupted credential header Patch");
        assert!(error.operator_message().contains("authorization"));
        assert!(!headers.contains_key("authorization"));
        assert_eq!(url.query(), Some("existing=safe"));

        for target in ["key", "Key", "alt", "ALT"] {
            let mut forbidden_query = runtime_rule(
                RequestPatchOperation::Set,
                target,
                Some("\"sentinel-secret\""),
            );
            forbidden_query.placement = RequestPatchPlacement::Query;
            let error = apply_request_patches(
                &mut body,
                &mut url,
                &mut headers,
                std::slice::from_ref(&forbidden_query),
            )
            .expect_err("runtime must defend against a corrupted Gemini target Query Patch");
            assert!(error.operator_message().contains("reserved"), "{target}");
            assert_eq!(url.query(), Some("existing=safe"), "{target}");
        }

        for target in ["/generationConfig/candidateCount", "/generationConfig"] {
            let original = body.clone();
            let forbidden_candidate = runtime_rule(
                RequestPatchOperation::Set,
                target,
                Some("{\"candidateCount\":2}"),
            );
            let error = apply_request_patches(
                &mut body,
                &mut url,
                &mut headers,
                std::slice::from_ref(&forbidden_candidate),
            )
            .expect_err("runtime must defend against candidate target override");
            assert!(error.operator_message().contains("reserved"), "{target}");
            assert_eq!(body, original, "{target}");
        }

        let mut forbidden_version = runtime_rule(
            RequestPatchOperation::Set,
            "anthropic-version",
            Some("\"2099-01-01\""),
        );
        forbidden_version.placement = RequestPatchPlacement::Header;
        let error = apply_request_patches(
            &mut body,
            &mut url,
            &mut headers,
            std::slice::from_ref(&forbidden_version),
        )
        .expect_err("runtime must defend against a corrupted Anthropic version Patch");
        assert!(error.operator_message().contains("anthropic-version"));
        assert!(!headers.contains_key("anthropic-version"));

        let mut allowed_beta = runtime_rule(
            RequestPatchOperation::Set,
            "anthropic-beta",
            Some("\"structured-outputs-2099-01-01\""),
        );
        allowed_beta.placement = RequestPatchPlacement::Header;
        apply_request_patches(
            &mut body,
            &mut url,
            &mut headers,
            std::slice::from_ref(&allowed_beta),
        )
        .expect("Source-bound Patch may explicitly set anthropic-beta");
        assert_eq!(
            headers
                .get("anthropic-beta")
                .and_then(|value| value.to_str().ok()),
            Some("structured-outputs-2099-01-01")
        );

        for (operation, target, value) in [
            (RequestPatchOperation::Set, "/store", Some("true")),
            (RequestPatchOperation::Remove, "/store/enabled", None),
            (
                RequestPatchOperation::Set,
                "/previous_response_id",
                Some("\"private-response-id\""),
            ),
            (RequestPatchOperation::Remove, "/conversation/id", None),
            (RequestPatchOperation::Set, "/background", Some("true")),
        ] {
            let original = body.clone();
            let forbidden = runtime_rule(operation, target, value);
            let error = apply_request_patches(
                &mut body,
                &mut url,
                &mut headers,
                std::slice::from_ref(&forbidden),
            )
            .expect_err("runtime must reject persisted stateful Responses target");
            assert!(error.operator_message().contains("reserved"), "{target}");
            assert_eq!(body, original, "{target}");
        }
    }
}
