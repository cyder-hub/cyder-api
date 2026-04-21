use std::{collections::HashMap, sync::Arc};

use axum::http::{HeaderMap, HeaderValue};
use reqwest::{
    Url,
    header::{
        ACCEPT_ENCODING, AUTHORIZATION, CONTENT_LENGTH, HOST, HeaderName,
        HeaderValue as ReqwestHeaderValue,
    },
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::ProxyError;
use crate::{
    schema::enum_def::{LlmApiType, ProviderType, RequestPatchOperation, RequestPatchPlacement},
    service::{
        app_state::{AppState, GroupItemSelectionStrategy},
        cache::types::{
            CacheModel, CacheModelRoute, CacheProvider, CacheRequestPatchConflict,
            CacheRequestPatchExplainEntry, CacheResolvedRequestPatch,
        },
        request_patch::resolve_effective_request_patches,
        transform::finalize_request_data,
        vertex::get_vertex_token,
    },
};
use cyder_tools::log::{debug, error};

/// Unified downstream request payload for generation operations.
pub struct PreparedGenerationRequest {
    pub final_url: String,
    pub final_headers: HeaderMap,
    pub final_body_value: Value,
    pub provider_api_key_id: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeRequestPatchTrace {
    pub applied_rules: Vec<CacheResolvedRequestPatch>,
    pub conflicts: Vec<CacheRequestPatchConflict>,
    pub has_conflicts: bool,
    pub applied_request_patch_ids_json: Option<String>,
    pub request_patch_summary_json: Option<String>,
}

#[derive(Debug, Serialize)]
struct RequestPatchTraceSummary {
    provider_id: i64,
    model_id: Option<i64>,
    effective_rules: Vec<CacheResolvedRequestPatch>,
    explain: Vec<CacheRequestPatchExplainEntry>,
    conflicts: Vec<CacheRequestPatchConflict>,
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

        Some(ProxyError::InternalError(format!(
            "Request patch conflicts prevent model '{}' from being used: {}",
            model_name, reasons
        )))
    }
}

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
pub struct ResolvedModelTarget {
    pub requested_name: String,
    pub resolved_scope: ResolvedNameScope,
    pub resolved_route_id: Option<i64>,
    pub resolved_route_name: Option<String>,
    pub candidates: Vec<i64>,
    pub provider: Arc<CacheProvider>,
    pub model: Arc<CacheModel>,
}

#[derive(Debug)]
enum GenerationPrepareKind {
    Llm { path: &'static str },
    Gemini { is_stream: bool },
}

fn select_generation_prepare_kind(
    target_api_type: LlmApiType,
    is_stream: bool,
) -> Result<GenerationPrepareKind, ProxyError> {
    match target_api_type {
        LlmApiType::Openai | LlmApiType::GeminiOpenai => Ok(GenerationPrepareKind::Llm {
            path: "chat/completions",
        }),
        LlmApiType::Ollama => Ok(GenerationPrepareKind::Llm { path: "api/chat" }),
        LlmApiType::Gemini => Ok(GenerationPrepareKind::Gemini { is_stream }),
        _ => Err(ProxyError::InternalError(format!(
            "unsupported generation target api type: {:?}",
            target_api_type
        ))),
    }
}

/// Resolved API key info for a provider, including the selected key ID and the
/// final request credential (which may be a Vertex AI OAuth token).
pub(super) struct ProviderCredentials {
    /// The database ID of the selected provider API key.
    pub key_id: i64,
    /// The credential to use for the downstream request. For Vertex AI providers,
    /// this is an OAuth token; for others, it's the raw API key.
    pub request_key: String,
}

/// Resolves the API key and authentication credential for a provider.
///
/// This handles: selecting a provider API key via the provider's configured
/// selection strategy, and exchanging it for a Vertex AI OAuth token when the
/// provider type requires it.
pub(super) async fn resolve_provider_credentials(
    provider: &CacheProvider,
    app_state: &Arc<AppState>,
) -> Result<ProviderCredentials, ProxyError> {
    let strategy = GroupItemSelectionStrategy::from(provider.provider_api_key_mode.clone());
    let selected_key = app_state
        .get_one_provider_api_key_by_provider(provider.id, strategy)
        .await
        .map_err(|e| {
            error!(
                "Failed to get provider API key from cache for provider_id {}: {:?}",
                provider.id, e
            );
            ProxyError::InternalError(format!(
                "Failed to retrieve API key for provider '{}'",
                provider.name
            ))
        })?
        .ok_or_else(|| {
            ProxyError::InternalError(format!(
                "No API keys configured for provider '{}'",
                provider.name
            ))
        })?;

    let request_key = match provider.provider_type {
        ProviderType::Vertex | ProviderType::VertexOpenai => get_vertex_token(
            &app_state.proxy_client,
            selected_key.id,
            &selected_key.api_key,
        )
        .await
        .map_err(|err_msg| ProxyError::BadRequest(err_msg))?,
        _ => selected_key.api_key.clone(),
    };

    Ok(ProviderCredentials {
        key_id: selected_key.id,
        request_key,
    })
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

fn parse_request_patch_value(rule: &CacheResolvedRequestPatch) -> Result<Value, ProxyError> {
    let raw = rule.value_json.as_ref().ok_or_else(|| {
        ProxyError::InternalError(format!(
            "request patch rule {} is missing value_json for SET",
            rule.source_rule_id
        ))
    })?;

    serde_json::from_str(raw).map_err(|err| {
        ProxyError::InternalError(format!(
            "request patch rule {} has invalid value_json: {}",
            rule.source_rule_id, err
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

fn scalar_request_patch_value(rule: &CacheResolvedRequestPatch) -> Result<String, ProxyError> {
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
    rule: &CacheResolvedRequestPatch,
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
    rule: &CacheResolvedRequestPatch,
) -> Result<(), ProxyError> {
    let header_name = HeaderName::from_bytes(rule.target.as_bytes()).map_err(|err| {
        ProxyError::InternalError(format!(
            "request patch rule {} has invalid header target '{}': {}",
            rule.source_rule_id, rule.target, err
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
                        "request patch rule {} has invalid header value for '{}': {}",
                        rule.source_rule_id, rule.target, err
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
    request_patches: &[CacheResolvedRequestPatch],
) -> Result<(), ProxyError> {
    for rule in request_patches {
        debug!(
            "Applying request patch {} to {:?} '{}'",
            rule.source_rule_id, rule.placement, rule.target
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
) -> Result<RuntimeRequestPatchTrace, ProxyError> {
    let applied_rules = if has_conflicts {
        Vec::new()
    } else {
        effective_rules.clone()
    };

    let applied_request_patch_ids_json = serde_json::to_string(
        &applied_rules
            .iter()
            .map(|rule| rule.source_rule_id)
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
        effective_rules,
        explain,
        conflicts: conflicts.clone(),
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
        conflicts,
        has_conflicts,
        applied_request_patch_ids_json,
        request_patch_summary_json,
    })
}

pub(crate) async fn load_runtime_request_patch_trace(
    provider: &CacheProvider,
    model: Option<&CacheModel>,
    app_state: &Arc<AppState>,
) -> Result<RuntimeRequestPatchTrace, ProxyError> {
    if let Some(model) = model {
        let resolved = app_state
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
        );
    }

    let provider_rules = app_state
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
    )
}

/// Builds headers for a Gemini-native request.
///
/// Filters out auth-related headers from the original request and sets the
/// appropriate auth header: `Authorization: Bearer` for Vertex AI, or
/// `X-Goog-Api-Key` for native Gemini.
fn build_gemini_headers(
    original_headers: &HeaderMap,
    provider: &CacheProvider,
    api_key: &str,
) -> HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in original_headers.iter() {
        if name != HOST
            && name != CONTENT_LENGTH
            && name != ACCEPT_ENCODING
            && name != "x-api-key"
            && name != "x-goog-api-key"
            && name != AUTHORIZATION
        {
            headers.insert(name.clone(), value.clone());
        }
    }

    if provider.provider_type == ProviderType::Vertex {
        let bearer_token = format!("Bearer {}", api_key);
        headers.insert(
            AUTHORIZATION,
            reqwest::header::HeaderValue::try_from(bearer_token).unwrap(),
        );
    } else {
        headers.insert(
            "X-Goog-Api-Key",
            reqwest::header::HeaderValue::try_from(api_key).unwrap(),
        );
    }

    headers
}

/// Builds the Gemini-style URL: `{endpoint}/{model_name}:{action}`, appending
/// original query params (excluding `key`) and optionally `alt=sse` for streaming.
fn build_gemini_url(
    provider: &CacheProvider,
    real_model_name: &str,
    action: &str,
    params: &HashMap<String, String>,
    is_stream: bool,
) -> Result<Url, ProxyError> {
    let target_url_str = format!("{}/{}:{}", provider.endpoint, real_model_name, action);
    let mut url = Url::parse(&target_url_str)
        .map_err(|_| ProxyError::BadRequest("failed to parse target url".to_string()))?;

    for (k, v) in params {
        if k != "key" {
            url.query_pairs_mut().append_pair(k, v);
        }
    }

    if is_stream {
        url.query_pairs_mut().append_pair("alt", "sse");
    }

    Ok(url)
}

pub fn build_new_headers(pre_headers: &HeaderMap, api_key: &str) -> Result<HeaderMap, ProxyError> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in pre_headers.iter() {
        if name != HOST // do not expose host to api endpoint
            && name != CONTENT_LENGTH // headers may be changed after, so content length may be changed at the same time
            && name != ACCEPT_ENCODING // some client may send br, and the result could be parsed :(
            && name != "x-api-key"
        {
            // for some client remove this header
            headers.insert(name.clone(), value.clone());
        }
    }
    let request_key = format!("Bearer {}", api_key);
    headers.insert(AUTHORIZATION, HeaderValue::try_from(request_key).unwrap());
    Ok(headers)
}

/// Resolves the real model name, preferring `real_model_name` over `model_name`.
fn resolve_real_model_name(model: &CacheModel) -> &str {
    model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name)
}

fn ensure_request_body_object(data: &mut Value) {
    if !matches!(data, Value::Object(_)) {
        *data = Value::Object(Map::new());
    }
}

// Prepares all elements for the downstream LLM request including URL, headers, and body.
pub async fn prepare_llm_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value, // Takes ownership of data
    original_headers: &HeaderMap,
    request_patches: &[CacheResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    path: &str,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let target_url = format!("{}/{}", provider.endpoint, path);
    let mut url = Url::parse(&target_url)
        .map_err(|_| ProxyError::BadRequest("failed to parse target url".to_string()))?;
    let mut headers = build_new_headers(original_headers, &provider_credentials.request_key)?;

    ensure_request_body_object(&mut data);
    if let Value::Object(obj) = &mut data {
        obj.insert("model".to_string(), json!(resolve_real_model_name(model)));
    }

    data = finalize_request_data(data, LlmApiType::Openai, &provider.provider_type, path);
    apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)?;

    Ok((url.to_string(), headers, data, provider_credentials.key_id))
}

pub async fn prepare_generation_request(
    provider: &CacheProvider,
    model: &CacheModel,
    data: Value,
    original_headers: &HeaderMap,
    request_patches: &[CacheResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    target_api_type: LlmApiType,
    is_stream: bool,
    params: &HashMap<String, String>,
) -> Result<PreparedGenerationRequest, ProxyError> {
    let prepared = match select_generation_prepare_kind(target_api_type, is_stream)? {
        GenerationPrepareKind::Llm { path } => {
            let (final_url, final_headers, final_body_value, provider_api_key_id) =
                prepare_llm_request(
                    provider,
                    model,
                    data,
                    original_headers,
                    request_patches,
                    provider_credentials,
                    path,
                )
                .await?;
            PreparedGenerationRequest {
                final_url,
                final_headers,
                final_body_value,
                provider_api_key_id,
            }
        }
        GenerationPrepareKind::Gemini { is_stream } => {
            let (final_url, final_headers, final_body_value, provider_api_key_id) =
                prepare_gemini_llm_request(
                    provider,
                    model,
                    data,
                    original_headers,
                    request_patches,
                    provider_credentials,
                    is_stream,
                    params,
                )
                .await?;
            PreparedGenerationRequest {
                final_url,
                final_headers,
                final_body_value,
                provider_api_key_id,
            }
        }
    };

    Ok(prepared)
}

// Prepares a simple Gemini request for utility endpoints with request patch application.
pub async fn prepare_simple_gemini_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    request_patches: &[CacheResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    action: &str,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing simple Gemini request for provider: {}, model: {}, action: {}",
        provider.name, model.model_name, action
    );

    let real_model_name = resolve_real_model_name(model);
    let mut url = build_gemini_url(provider, real_model_name, action, params, false)?;
    let mut headers = build_gemini_headers(
        original_headers,
        provider,
        &provider_credentials.request_key,
    );
    apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)?;

    Ok((url.to_string(), headers, data, provider_credentials.key_id))
}

// Prepares all elements for a downstream Gemini LLM request.
pub async fn prepare_gemini_llm_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    request_patches: &[CacheResolvedRequestPatch],
    provider_credentials: &ProviderCredentials,
    is_stream: bool,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing Gemini LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let real_model_name = resolve_real_model_name(model);
    let action = if is_stream {
        "streamGenerateContent"
    } else {
        "generateContent"
    };
    let mut url = build_gemini_url(provider, real_model_name, action, params, is_stream)?;
    let mut headers = build_gemini_headers(
        original_headers,
        provider,
        &provider_credentials.request_key,
    );

    apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)?;

    Ok((url.to_string(), headers, data, provider_credentials.key_id))
}

fn parse_provider_model(pm: &str) -> (&str, &str) {
    let mut parts = pm.splitn(2, '/');
    let provider = parts.next().unwrap_or("");
    let model_id = parts.next().unwrap_or("");
    (provider, model_id)
}

fn select_primary_route_candidate(route: &CacheModelRoute) -> Result<i64, String> {
    route
        .candidates
        .iter()
        .filter(|candidate| candidate.is_enabled)
        .next()
        .map(|candidate| candidate.model_id)
        .ok_or_else(|| {
            format!(
                "Model route '{}' does not have any enabled candidates.",
                route.route_name
            )
        })
}

async fn resolve_route_target(
    app_state: &Arc<AppState>,
    requested_name: &str,
    resolved_scope: ResolvedNameScope,
    route: Arc<CacheModelRoute>,
) -> Result<ResolvedModelTarget, String> {
    if !route.is_enabled {
        return Err(format!("Model route '{}' is disabled.", route.route_name));
    }

    let selected_model_id = select_primary_route_candidate(&route)?;
    let model = app_state
        .get_model_by_id(selected_model_id)
        .await
        .map_err(|e| {
            format!(
                "Error accessing cache for route candidate model {}: {:?}",
                selected_model_id, e
            )
        })?
        .ok_or_else(|| {
            format!(
                "Primary candidate model {} for route '{}' was not found.",
                selected_model_id, route.route_name
            )
        })?;
    let provider = app_state
        .get_provider_by_id(model.provider_id)
        .await
        .map_err(|e| {
            format!(
                "Error accessing cache for provider ID {}: {:?}",
                model.provider_id, e
            )
        })?
        .ok_or_else(|| {
            format!(
                "Provider ID {} for route '{}' was not found.",
                model.provider_id, route.route_name
            )
        })?;

    Ok(ResolvedModelTarget {
        requested_name: requested_name.to_string(),
        resolved_scope,
        resolved_route_id: Some(route.id),
        resolved_route_name: Some(route.route_name.clone()),
        candidates: route
            .candidates
            .iter()
            .map(|candidate| candidate.model_id)
            .collect(),
        provider,
        model,
    })
}

async fn resolve_direct_target(
    app_state: &Arc<AppState>,
    requested_name: &str,
) -> Result<ResolvedModelTarget, String> {
    let (provider_key_str, model_name_str) = parse_provider_model(requested_name);
    if provider_key_str.is_empty() || model_name_str.is_empty() {
        return Err(format!(
            "Invalid model format: '{}'. Expected a configured route or 'provider/model'.",
            requested_name
        ));
    }

    let provider = app_state
        .get_provider_by_key(provider_key_str)
        .await
        .map_err(|e| {
            format!(
                "Error accessing cache for provider key '{}': {:?}",
                provider_key_str, e
            )
        })?
        .ok_or_else(|| format!("Provider '{}' not found.", provider_key_str))?;

    let model = app_state
        .get_model_by_name(provider_key_str, model_name_str)
        .await
        .map_err(|e| {
            format!(
                "Error accessing cache for model name '{}': {:?}",
                requested_name, e
            )
        })?
        .ok_or_else(|| format!("Model '{}' not found.", requested_name))?;

    if model.provider_id != provider.id {
        return Err(format!(
            "Model '{}' does not belong to provider '{}'.",
            model.model_name, provider.name
        ));
    }

    Ok(ResolvedModelTarget {
        requested_name: requested_name.to_string(),
        resolved_scope: ResolvedNameScope::Direct,
        resolved_route_id: None,
        resolved_route_name: None,
        candidates: vec![model.id],
        provider,
        model,
    })
}

pub async fn resolve_requested_model(
    app_state: &Arc<AppState>,
    api_key_id: i64,
    requested_name: &str,
) -> Result<ResolvedModelTarget, String> {
    match app_state
        .get_api_key_override_route(api_key_id, requested_name)
        .await
    {
        Ok(Some(route)) => {
            debug!(
                "Resolved '{}' via api key override for api_key_id {} to route '{}'",
                requested_name, api_key_id, route.route_name
            );
            return resolve_route_target(
                app_state,
                requested_name,
                ResolvedNameScope::ApiKeyOverride,
                route,
            )
            .await;
        }
        Ok(None) => {}
        Err(e) => {
            error!(
                "Error checking api key override for {}:{}: {:?}",
                api_key_id, requested_name, e
            );
            return Err(format!(
                "Internal server error while checking api key overrides for '{}'.",
                requested_name
            ));
        }
    }

    match app_state.get_model_route_by_name(requested_name).await {
        Ok(Some(route)) => {
            debug!(
                "Resolved '{}' as a global model route '{}'",
                requested_name, route.route_name
            );
            return resolve_route_target(
                app_state,
                requested_name,
                ResolvedNameScope::GlobalRoute,
                route,
            )
            .await;
        }
        Ok(None) => {
            debug!(
                "'{}' is not a configured route. Attempting direct provider/model parsing.",
                requested_name
            );
        }
        Err(e) => {
            error!("Error checking model route '{}': {:?}", requested_name, e);
            return Err(format!(
                "Internal server error while checking configured routes for '{}'.",
                requested_name
            ));
        }
    }

    resolve_direct_target(app_state, requested_name).await
}

#[cfg(test)]
mod tests {
    use super::{
        ResolvedNameScope, apply_request_patches, parse_provider_model, resolve_real_model_name,
        select_generation_prepare_kind, select_primary_route_candidate,
    };
    use crate::{
        schema::enum_def::{LlmApiType, RequestPatchOperation, RequestPatchPlacement},
        service::cache::types::{
            CacheModelRoute, CacheModelRouteCandidate, CacheResolvedRequestPatch,
            RequestPatchRuleOrigin,
        },
    };
    use axum::http::{HeaderMap, HeaderValue};
    use reqwest::Url;
    use serde_json::{Value, json};

    fn model(
        model_name: &str,
        real_model_name: Option<&str>,
    ) -> crate::service::cache::types::CacheModel {
        crate::service::cache::types::CacheModel {
            id: 1,
            provider_id: 2,
            model_name: model_name.to_string(),
            real_model_name: real_model_name.map(str::to_string),
            cost_catalog_id: None,
            is_enabled: true,
        }
    }

    fn route(candidate_flags: &[bool]) -> CacheModelRoute {
        CacheModelRoute {
            id: 7,
            route_name: "manual-smoke-route".to_string(),
            description: None,
            is_enabled: true,
            expose_in_models: true,
            candidates: candidate_flags
                .iter()
                .enumerate()
                .map(|(index, is_enabled)| CacheModelRouteCandidate {
                    route_id: 7,
                    model_id: (index + 1) as i64,
                    provider_id: 2,
                    priority: index as i32,
                    is_enabled: *is_enabled,
                })
                .collect(),
        }
    }

    fn request_patch(
        id: i64,
        placement: RequestPatchPlacement,
        target: &str,
        operation: RequestPatchOperation,
        value: Option<Value>,
    ) -> CacheResolvedRequestPatch {
        CacheResolvedRequestPatch {
            placement,
            target: target.to_string(),
            operation,
            value_json: value.map(|item| serde_json::to_string(&item).unwrap()),
            source_rule_id: id,
            source_origin: RequestPatchRuleOrigin::ProviderDirect,
            overridden_rule_ids: Vec::new(),
            description: None,
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

        assert!(matches!(err, crate::proxy::ProxyError::InternalError(_)));
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

        assert!(matches!(err, crate::proxy::ProxyError::InternalError(_)));
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

        assert!(matches!(err, crate::proxy::ProxyError::InternalError(_)));
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
    fn resolve_real_model_name_prefers_non_empty_real_name() {
        let aliased = model("gpt-4.1", Some("providers/acme/models/gpt-4.1"));
        let empty_real_name = model("gpt-4.1", Some(""));
        let direct = model("gpt-4.1", None);

        assert_eq!(
            resolve_real_model_name(&aliased),
            "providers/acme/models/gpt-4.1"
        );
        assert_eq!(resolve_real_model_name(&empty_real_name), "gpt-4.1");
        assert_eq!(resolve_real_model_name(&direct), "gpt-4.1");
    }

    #[test]
    fn select_generation_prepare_kind_maps_supported_generation_targets() {
        assert!(matches!(
            select_generation_prepare_kind(LlmApiType::Openai, false),
            Ok(super::GenerationPrepareKind::Llm {
                path: "chat/completions"
            })
        ));
        assert!(matches!(
            select_generation_prepare_kind(LlmApiType::GeminiOpenai, true),
            Ok(super::GenerationPrepareKind::Llm {
                path: "chat/completions"
            })
        ));
        assert!(matches!(
            select_generation_prepare_kind(LlmApiType::Ollama, false),
            Ok(super::GenerationPrepareKind::Llm { path: "api/chat" })
        ));
        assert!(matches!(
            select_generation_prepare_kind(LlmApiType::Gemini, true),
            Ok(super::GenerationPrepareKind::Gemini { is_stream: true })
        ));
    }

    #[test]
    fn select_generation_prepare_kind_rejects_non_generation_target() {
        let err = select_generation_prepare_kind(LlmApiType::Anthropic, false).unwrap_err();
        assert!(matches!(err, crate::proxy::ProxyError::InternalError(_)));
        assert_eq!(
            err.to_string(),
            "[server_error] unsupported generation target api type: Anthropic"
        );
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
    fn select_primary_route_candidate_uses_first_enabled_candidate() {
        assert_eq!(
            select_primary_route_candidate(&route(&[true, true])).unwrap(),
            1
        );
        assert_eq!(
            select_primary_route_candidate(&route(&[false, true])).unwrap(),
            2
        );
    }

    #[test]
    fn select_primary_route_candidate_rejects_route_without_enabled_candidates() {
        let err = select_primary_route_candidate(&route(&[false, false])).unwrap_err();
        assert!(err.contains("does not have any enabled candidates"));
    }
}
