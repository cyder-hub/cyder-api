use std::{collections::HashMap, sync::Arc};

use axum::http::HeaderMap;
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
    database::reasoning_config::{
        ReasoningConfigMode, ReasoningConfigScope, ReasoningPatchFamily, ReasoningPreset,
    },
    schema::enum_def::{
        LlmApiType, ProviderApiKeyMode, ProviderType, RequestPatchOperation, RequestPatchPlacement,
    },
    service::{
        app_state::AppState,
        cache::types::{
            CacheApiKeyModelOverride, CacheModel, CacheModelRoute, CacheModelRouteCandidate,
            CacheModelsCatalog, CacheProvider, CacheReasoningConfig, CacheRequestPatchConflict,
            CacheRequestPatchExplainEntry, CacheResolvedRequestPatch, RequestPatchSource,
            RuntimeRequestPatchConflict, RuntimeResolvedRequestPatch,
        },
        request_patch::resolve_effective_request_patches,
        runtime::GroupItemSelectionStrategy,
        transform::finalize_request_data,
        vertex::get_vertex_token,
    },
    utils::storage::RequestLogBundleQueryParam,
};
use cyder_tools::log::{debug, error, warn};

use super::{
    reasoning_suffix::{
        GeneratedReasoningPatch, ReasoningPatchContext, generate_reasoning_patches,
    },
    requested_model::{
        RequestedModelParseStatus, ResolvedRequestedModelName, enabled_reasoning_suffixes,
        parse_reasoning_suffix,
    },
    util::determine_target_api_type,
};

/// Unified downstream request payload for generation operations.
pub struct PreparedGenerationRequest {
    pub final_url: String,
    pub final_headers: HeaderMap,
    pub final_body_value: Value,
    pub provider_api_key_id: i64,
}

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
                    "#{} route={:?}/{} priority={:?} provider={}/{} model={}/{} llm_api={:?} key_mode={:?} reasoning_config={:?}/{}/{} reasoning_preset_row={:?} reasoning_family={:?} reasoning_preset={:?} reasoning_suffix={:?}",
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
                    candidate.reasoning_suffix
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
        .provider_key_selector
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
            app_state.infra.proxy_client(),
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

pub(crate) fn apply_provider_request_auth_header(
    headers: &mut HeaderMap,
    provider: &CacheProvider,
    target_api_type: LlmApiType,
    request_key: &str,
) -> Result<(), ProxyError> {
    let (header_name, header_value) =
        provider_request_auth_header(provider, target_api_type, request_key)?;

    headers.remove(AUTHORIZATION);
    headers.remove("x-api-key");
    headers.remove("x-goog-api-key");
    headers.insert(header_name, header_value);

    Ok(())
}

fn provider_request_auth_header(
    provider: &CacheProvider,
    target_api_type: LlmApiType,
    request_key: &str,
) -> Result<(HeaderName, ReqwestHeaderValue), ProxyError> {
    let header_name = match (&provider.provider_type, target_api_type) {
        (ProviderType::Openai, LlmApiType::Openai)
        | (ProviderType::Responses, LlmApiType::Responses)
        | (ProviderType::Vertex, LlmApiType::Gemini)
        | (ProviderType::VertexOpenai, LlmApiType::Openai)
        | (ProviderType::Ollama, LlmApiType::Ollama)
        | (ProviderType::GeminiOpenai, LlmApiType::GeminiOpenai) => AUTHORIZATION,
        (ProviderType::Gemini, LlmApiType::Gemini) => HeaderName::from_static("x-goog-api-key"),
        (ProviderType::Anthropic, LlmApiType::Anthropic) => HeaderName::from_static("x-api-key"),
        _ => {
            return Err(ProxyError::BadRequest(format!(
                "Provider '{}' with type {:?} does not support downstream protocol {:?}",
                provider.name, provider.provider_type, target_api_type
            )));
        }
    };

    let header_value = if header_name == AUTHORIZATION {
        ReqwestHeaderValue::try_from(format!("Bearer {}", request_key)).map_err(|err| {
            ProxyError::BadRequest(format!(
                "Invalid provider credential for replay/auth header '{}': {}",
                header_name, err
            ))
        })?
    } else {
        ReqwestHeaderValue::try_from(request_key).map_err(|err| {
            ProxyError::BadRequest(format!(
                "Invalid provider credential for replay/auth header '{}': {}",
                header_name, err
            ))
        })?
    };

    Ok((header_name, header_value))
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

pub(crate) fn rebuild_gemini_url_query_from_snapshot(
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

/// Builds headers for a Gemini-native request.
///
/// Filters out auth-related headers from the original request and sets the
/// appropriate auth header: `Authorization: Bearer` for Vertex AI, or
/// `X-Goog-Api-Key` for native Gemini.
fn build_gemini_headers(
    original_headers: &HeaderMap,
    provider: &CacheProvider,
    api_key: &str,
) -> Result<HeaderMap, ProxyError> {
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

    apply_provider_request_auth_header(&mut headers, provider, LlmApiType::Gemini, api_key)?;

    Ok(headers)
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

pub fn build_new_headers(
    pre_headers: &HeaderMap,
    provider: &CacheProvider,
    target_api_type: LlmApiType,
    api_key: &str,
) -> Result<HeaderMap, ProxyError> {
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
    apply_provider_request_auth_header(&mut headers, provider, target_api_type, api_key)?;
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
    request_patches: &[RuntimeResolvedRequestPatch],
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
    let target_api_type = determine_target_api_type(provider);
    let mut headers = build_new_headers(
        original_headers,
        provider,
        target_api_type,
        &provider_credentials.request_key,
    )?;

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
    request_patches: &[RuntimeResolvedRequestPatch],
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
    request_patches: &[RuntimeResolvedRequestPatch],
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
    )?;
    apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)?;

    Ok((url.to_string(), headers, data, provider_credentials.key_id))
}

// Prepares all elements for a downstream Gemini LLM request.
pub async fn prepare_gemini_llm_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    request_patches: &[RuntimeResolvedRequestPatch],
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
    )?;

    apply_request_patches(&mut data, &mut url, &mut headers, &request_patches)?;

    Ok((url.to_string(), headers, data, provider_credentials.key_id))
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
    use super::{
        ReasoningConfigSource, ResolvedNameScope, apply_request_patches,
        build_execution_plan_from_catalog, build_runtime_request_patch_trace,
        generate_candidate_reasoning_request_patches, parse_provider_model,
        rebuild_gemini_url_query_from_snapshot, resolve_real_model_name,
        route_supports_reasoning_preset, select_generation_prepare_kind,
    };
    use crate::{
        database::reasoning_config::{
            ReasoningConfigMode, ReasoningConfigScope, ReasoningPatchFamily, ReasoningPreset,
        },
        schema::enum_def::{
            LlmApiType, ProviderApiKeyMode, ProviderType, RequestPatchOperation,
            RequestPatchPlacement,
        },
        service::cache::types::{
            CacheApiKeyModelOverride, CacheModel, CacheModelRoute, CacheModelRouteCandidate,
            CacheModelsCatalog, CacheProvider, CacheReasoningConfig, CacheReasoningConfigPreset,
            CacheResolvedRequestPatch, RequestPatchRuleOrigin, RequestPatchSource,
            RuntimeResolvedRequestPatch,
        },
        utils::storage::RequestLogBundleQueryParam,
    };
    use axum::http::{HeaderMap, HeaderValue};
    use reqwest::Url;
    use serde_json::{Value, json};

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

    fn model(model_name: &str, real_model_name: Option<&str>) -> CacheModel {
        model_with_id(1, 2, model_name, real_model_name)
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
        assert!(matches!(
            err,
            crate::proxy::ProxyError::RequestPatchConflict(_)
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
    fn generated_reasoning_patch_uses_bound_config_fields() {
        let catalog = catalog_with_openai_high_reasoning();
        let plan = build_execution_plan_from_catalog(&catalog, 7, "openai/gpt-primary-high")
            .expect("reasoning suffix should bind config fields");

        let generated_rules =
            generate_candidate_reasoning_request_patches(Some(plan.primary_candidate().unwrap()))
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
