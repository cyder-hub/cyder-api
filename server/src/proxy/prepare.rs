use std::{collections::HashMap, sync::Arc};

use axum::http::{HeaderMap, HeaderValue};
use reqwest::{
    Url,
    header::{ACCEPT_ENCODING, AUTHORIZATION, CONTENT_LENGTH, HOST},
};
use serde_json::{Value, json};

use super::ProxyError;
use crate::{
    schema::enum_def::{FieldPlacement, FieldType, LlmApiType, ProviderType},
    service::{
        app_state::{AppState, GroupItemSelectionStrategy},
        cache::types::{CacheCustomField, CacheModel, CacheProvider},
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
struct ProviderCredentials {
    /// The database ID of the selected provider API key.
    key_id: i64,
    /// The credential to use for the downstream request. For Vertex AI providers,
    /// this is an OAuth token; for others, it's the raw API key.
    request_key: String,
}

/// Resolves the API key and authentication credential for a provider.
///
/// This handles: selecting a provider API key via the provider's configured
/// selection strategy, and exchanging it for a Vertex AI OAuth token when the
/// provider type requires it.
async fn resolve_provider_credentials(
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

/// Fetches custom fields for both the provider and model, merging them with
/// model-level fields taking precedence over provider-level fields (by ID).
async fn fetch_combined_custom_fields(
    provider: &CacheProvider,
    model: &CacheModel,
    app_state: &Arc<AppState>,
) -> Result<Vec<Arc<CacheCustomField>>, ProxyError> {
    let provider_cfs = app_state
        .get_custom_fields_by_provider_id(provider.id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get custom fields for provider_id {}: {:?}",
                provider.id, e
            );
            ProxyError::InternalError("Failed to retrieve custom fields for provider".to_string())
        })?;
    let model_cfs = app_state
        .get_custom_fields_by_model_id(model.id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get custom fields for model_id {}: {:?}",
                model.id, e
            );
            ProxyError::InternalError("Failed to retrieve custom fields for model".to_string())
        })?;

    let mut combined_map: HashMap<i64, Arc<CacheCustomField>> = HashMap::new();
    for cf in provider_cfs {
        combined_map.insert(cf.id, cf);
    }
    for cf in model_cfs {
        combined_map.insert(cf.id, cf);
    }
    let fields: Vec<Arc<CacheCustomField>> = combined_map.into_values().collect();
    debug!(
        "Fetched {} custom fields for provider and model",
        fields.len()
    );
    Ok(fields)
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

// Prepares all elements for the downstream LLM request including URL, headers, and body.
pub async fn prepare_llm_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value, // Takes ownership of data
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    path: &str,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let creds = resolve_provider_credentials(provider, app_state).await?;
    let custom_fields = fetch_combined_custom_fields(provider, model, app_state).await?;

    // Prepare URL, headers, and apply custom fields
    let target_url = format!("{}/{}", provider.endpoint, path);
    let mut url = Url::parse(&target_url)
        .map_err(|_| ProxyError::BadRequest("failed to parse target url".to_string()))?;
    let mut headers = build_new_headers(original_headers, &creds.request_key)?;

    handle_custom_fields(&mut data, &mut url, &mut headers, &custom_fields);

    // Set the real model name in the request body
    let real_model_name_str = resolve_real_model_name(model);
    if let Some(obj) = data.as_object_mut() {
        obj.insert("model".to_string(), json!(real_model_name_str));
    }

    data = finalize_request_data(data, LlmApiType::Openai, &provider.provider_type, path);

    Ok((url.to_string(), headers, data, creds.key_id))
}

pub async fn prepare_generation_request(
    provider: &CacheProvider,
    model: &CacheModel,
    data: Value,
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    target_api_type: LlmApiType,
    is_stream: bool,
    params: &HashMap<String, String>,
) -> Result<PreparedGenerationRequest, ProxyError> {
    let prepared = match select_generation_prepare_kind(target_api_type, is_stream)? {
        GenerationPrepareKind::Llm { path } => {
            let (final_url, final_headers, final_body_value, provider_api_key_id) =
                prepare_llm_request(provider, model, data, original_headers, app_state, path)
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
                    app_state,
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

// Prepares a simple Gemini request for utility endpoints, without custom fields or body transformation.
pub async fn prepare_simple_gemini_request(
    provider: &CacheProvider,
    model: &CacheModel,
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    action: &str,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, i64), ProxyError> {
    debug!(
        "Preparing simple Gemini request for provider: {}, model: {}, action: {}",
        provider.name, model.model_name, action
    );

    let creds = resolve_provider_credentials(provider, app_state).await?;

    let real_model_name = resolve_real_model_name(model);
    let url = build_gemini_url(provider, real_model_name, action, params, false)?;
    let headers = build_gemini_headers(original_headers, provider, &creds.request_key);

    Ok((url.to_string(), headers, creds.key_id))
}

// Prepares all elements for a downstream Gemini LLM request.
pub async fn prepare_gemini_llm_request(
    provider: &CacheProvider,
    model: &CacheModel,
    mut data: Value,
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    is_stream: bool,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, Value, i64), ProxyError> {
    debug!(
        "Preparing Gemini LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    let creds = resolve_provider_credentials(provider, app_state).await?;
    let custom_fields = fetch_combined_custom_fields(provider, model, app_state).await?;

    let real_model_name = resolve_real_model_name(model);
    let action = if is_stream {
        "streamGenerateContent"
    } else {
        "generateContent"
    };
    let mut url = build_gemini_url(provider, real_model_name, action, params, is_stream)?;
    let mut headers = build_gemini_headers(original_headers, provider, &creds.request_key);

    handle_custom_fields(&mut data, &mut url, &mut headers, &custom_fields);

    Ok((url.to_string(), headers, data, creds.key_id))
}

fn parse_provider_model(pm: &str) -> (&str, &str) {
    let mut parts = pm.splitn(2, '/');
    let provider = parts.next().unwrap_or("");
    let model_id = parts.next().unwrap_or("");
    (provider, model_id)
}

// Sets or removes a value in a nested JSON object based on a dot-separated path.
fn set_nested_value(data: &mut Value, path: &str, value_to_set: Option<Value>) {
    if path.is_empty() {
        return;
    }
    let mut parts: Vec<&str> = path.split('.').collect();
    let key = match parts.pop() {
        Some(k) => k,
        None => return, // Should not happen if path is not empty
    };

    let mut current_level = data;
    for part in parts {
        if !current_level.is_object() {
            *current_level = Value::Object(serde_json::Map::new());
        }
        let obj = current_level.as_object_mut().unwrap();
        let next_level = obj
            .entry(part.to_string())
            .or_insert(Value::Object(serde_json::Map::new()));
        if !next_level.is_object() {
            *next_level = Value::Object(serde_json::Map::new());
        }
        current_level = next_level;
    }

    if let Some(obj) = current_level.as_object_mut() {
        match value_to_set {
            Some(v) => {
                obj.insert(key.to_string(), v);
            }
            None => {
                obj.remove(key);
            }
        }
    }
}

pub fn handle_custom_fields(
    data: &mut Value,        // For "BODY"
    url: &mut Url,           // For "QUERY"
    headers: &mut HeaderMap, // For "HEADER" (reqwest::header::HeaderMap)
    custom_fields: &Vec<Arc<CacheCustomField>>,
) {
    for cf in custom_fields {
        debug!(
            "Applying custom field '{}' to {:?}",
            cf.field_name, cf.field_placement
        );
        match cf.field_placement {
            FieldPlacement::Body => {
                let value_opt: Option<Value> = match cf.field_type {
                    FieldType::Unset => {
                        set_nested_value(data, &cf.field_name, None);
                        continue;
                    }
                    FieldType::String => cf.string_value.clone().map(Value::String),
                    FieldType::Integer => cf.integer_value.map(|v| Value::Number(v.into())),
                    FieldType::Number => cf.number_value.map(|v| {
                        serde_json::Number::from_f64(v as f64)
                            .map(Value::Number)
                            .unwrap_or(Value::Null)
                    }),
                    FieldType::Boolean => cf.boolean_value.map(Value::Bool),
                    FieldType::JsonString => cf.string_value.as_ref().and_then(|s| {
                        serde_json::from_str(s)
                            .map_err(|e| {
                                error!(
                                    "Failed to parse JSON_STRING custom field '{}' for BODY: {}. Value: '{}'",
                                    cf.field_name, e, s
                                );
                            })
                            .ok()
                    }),
                };

                if let Some(value) = value_opt {
                    set_nested_value(data, &cf.field_name, Some(value));
                }
            }
            FieldPlacement::Query => {
                let field_name_key = cf.field_name.clone();
                let mut new_value_opt: Option<String> = None;

                match cf.field_type {
                    FieldType::Unset => { /* new_value_opt remains None, effectively removing */ }
                    FieldType::String => {
                        new_value_opt = cf.string_value.clone();
                    }
                    FieldType::Integer => {
                        new_value_opt = cf.integer_value.map(|v| v.to_string());
                    }
                    FieldType::Number => {
                        new_value_opt = cf.number_value.map(|v| v.to_string());
                    }
                    FieldType::Boolean => {
                        new_value_opt = cf.boolean_value.map(|v| v.to_string());
                    }
                    FieldType::JsonString => {
                        new_value_opt = cf.string_value.clone();
                    } // JSON as string for query
                }

                // Rebuild query parameters to ensure replacement
                // First, collect existing pairs to drop the immutable borrow of url.
                let existing_pairs: Vec<(String, String)> = url
                    .query_pairs()
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .filter(|(k, _)| k != &field_name_key) // Keep pairs not matching current field name
                    .collect();

                // Now, get a mutable borrow to reconstruct.
                let mut query_pairs_mut = url.query_pairs_mut();
                query_pairs_mut.clear(); // Clear existing before re-adding filtered/new ones

                for (k, v) in existing_pairs {
                    query_pairs_mut.append_pair(&k, &v);
                }

                if let Some(new_val_str) = new_value_opt {
                    query_pairs_mut.append_pair(&field_name_key, &new_val_str);
                }
                // UrlQueryMut updates the URL when it's dropped (goes out of scope)
            }
            FieldPlacement::Header => {
                match cf.field_type {
                    FieldType::Unset => {
                        headers.remove(&cf.field_name);
                    }
                    _ => {
                        // For all other types, convert to string and set header
                        let value_str_opt: Option<String> = match cf.field_type {
                            FieldType::String => cf.string_value.clone(),
                            FieldType::Integer => cf.integer_value.map(|v| v.to_string()),
                            FieldType::Number => cf.number_value.map(|v| v.to_string()),
                            FieldType::Boolean => cf.boolean_value.map(|v| v.to_string()),
                            FieldType::JsonString => cf.string_value.clone(), // JSON as string for header
                            _ => {
                                debug!(
                                    "Unknown custom field type '{:?}' for field '{}' in HEADER",
                                    cf.field_type, cf.field_name
                                );
                                None
                            }
                        };

                        if let Some(value_str) = value_str_opt {
                            match reqwest::header::HeaderName::from_bytes(cf.field_name.as_bytes())
                            {
                                Ok(header_name) => {
                                    match reqwest::header::HeaderValue::from_str(&value_str) {
                                        Ok(header_value) => {
                                            headers.insert(header_name, header_value);
                                        }
                                        Err(e) => {
                                            error!(
                                                "Invalid header value for custom field '{}': {}. Value: '{}'",
                                                cf.field_name, e, value_str
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "Invalid header name for custom field '{}': {}",
                                        cf.field_name, e
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// Fetches provider and model from AppState cache, resolving aliases first.
pub async fn get_provider_and_model(
    app_state: &Arc<AppState>,
    pre_model_value: &str,
) -> Result<(Arc<CacheProvider>, Arc<CacheModel>), String> {
    // Attempt to resolve as a model alias first
    match app_state.get_model_by_alias(pre_model_value).await {
        Ok(Some(model)) => {
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
                        "Provider ID {} for model '{}' (from alias '{}') not found in cache.",
                        model.provider_id, model.model_name, pre_model_value
                    )
                })?;

            debug!(
                "Resolved '{}' as an alias to model '{}' from provider '{}'",
                pre_model_value, model.model_name, provider.name
            );
            return Ok((provider, model));
        }
        Ok(None) => {
            debug!(
                "'{}' is not a model alias. Attempting provider/model parsing.",
                pre_model_value
            );
        }
        Err(e) => {
            error!("Error checking model alias '{}': {:?}", pre_model_value, e);
            return Err(format!(
                "Internal server error while checking model alias '{}'.",
                pre_model_value
            ));
        }
    }

    // Fallback: try parsing as provider/model
    let (provider_key_str, model_name_str) = parse_provider_model(pre_model_value);
    if provider_key_str.is_empty() || model_name_str.is_empty() {
        return Err(format!(
            "Invalid model format: '{}'. Expected 'provider/model' or a valid alias.",
            pre_model_value
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
                pre_model_value, e
            )
        })?
        .ok_or_else(|| format!("Model '{}' not found.", pre_model_value))?;

    if model.provider_id != provider.id {
        return Err(format!(
            "Model '{}' does not belong to provider '{}'.",
            model.model_name, provider.name
        ));
    }

    debug!(
        "Resolved '{}' as provider '{}' and model '{}'",
        pre_model_value, provider.name, model.model_name
    );
    Ok((provider, model))
}

#[cfg(test)]
mod tests {
    use super::{
        handle_custom_fields, parse_provider_model, resolve_real_model_name,
        select_generation_prepare_kind, set_nested_value,
    };
    use crate::{
        schema::enum_def::{FieldPlacement, FieldType, LlmApiType},
        service::cache::types::CacheCustomField,
    };
    use axum::http::{HeaderMap, HeaderValue};
    use reqwest::Url;
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn custom_field(
        field_name: &str,
        field_placement: FieldPlacement,
        field_type: FieldType,
    ) -> CacheCustomField {
        CacheCustomField {
            id: 1,
            field_name: field_name.to_string(),
            field_placement,
            field_type,
            string_value: None,
            integer_value: None,
            number_value: None,
            boolean_value: None,
        }
    }

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

    #[test]
    fn set_nested_value_creates_and_overwrites_nested_path() {
        let mut data = json!({
            "existing": "value",
            "metadata": {
                "mode": "old"
            }
        });

        set_nested_value(&mut data, "metadata.config.mode", Some(json!("strict")));
        set_nested_value(&mut data, "metadata.mode", Some(json!("new")));

        assert_eq!(
            data,
            json!({
                "existing": "value",
                "metadata": {
                    "mode": "new",
                    "config": {
                        "mode": "strict"
                    }
                }
            })
        );
    }

    #[test]
    fn set_nested_value_removes_key_without_touching_siblings() {
        let mut data = json!({
            "metadata": {
                "remove_me": "value",
                "keep_me": true
            }
        });

        set_nested_value(&mut data, "metadata.remove_me", None);

        assert_eq!(
            data,
            json!({
                "metadata": {
                    "keep_me": true
                }
            })
        );
    }

    #[test]
    fn set_nested_value_replaces_non_object_intermediate_nodes() {
        let mut data = json!({
            "metadata": "raw"
        });

        set_nested_value(&mut data, "metadata.flags.enabled", Some(json!(true)));

        assert_eq!(
            data,
            json!({
                "metadata": {
                    "flags": {
                        "enabled": true
                    }
                }
            })
        );
    }

    #[test]
    fn handle_custom_fields_updates_body_and_unsets_nested_value() {
        let mut data = json!({
            "generation_config": {
                "temperature": 0.2,
                "remove_me": "stale"
            }
        });
        let mut url = Url::parse("https://example.com/v1/chat").unwrap();
        let mut headers = HeaderMap::new();

        let mut temperature_field = custom_field(
            "generation_config.temperature",
            FieldPlacement::Body,
            FieldType::Number,
        );
        temperature_field.number_value = Some(0.8);

        let mut extra_body_field = custom_field(
            "generation_config.response_schema",
            FieldPlacement::Body,
            FieldType::JsonString,
        );
        extra_body_field.id = 2;
        extra_body_field.string_value = Some(r#"{"type":"object","strict":true}"#.to_string());

        let mut unset_field = custom_field(
            "generation_config.remove_me",
            FieldPlacement::Body,
            FieldType::Unset,
        );
        unset_field.id = 3;

        let custom_fields = vec![
            Arc::new(temperature_field),
            Arc::new(extra_body_field),
            Arc::new(unset_field),
        ];

        handle_custom_fields(&mut data, &mut url, &mut headers, &custom_fields);

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
    fn handle_custom_fields_skips_invalid_body_json_string() {
        let mut data = json!({
            "metadata": {
                "keep": "value"
            }
        });
        let mut url = Url::parse("https://example.com/v1/chat").unwrap();
        let mut headers = HeaderMap::new();

        let mut invalid_json_field = custom_field(
            "metadata.invalid",
            FieldPlacement::Body,
            FieldType::JsonString,
        );
        invalid_json_field.string_value = Some("{oops".to_string());

        let custom_fields = vec![Arc::new(invalid_json_field)];
        handle_custom_fields(&mut data, &mut url, &mut headers, &custom_fields);

        assert_eq!(
            data,
            json!({
                "metadata": {
                    "keep": "value"
                }
            })
        );
    }

    #[test]
    fn handle_custom_fields_replaces_query_values_and_supports_unset() {
        let mut data = Value::Null;
        let mut url =
            Url::parse("https://example.com/v1/chat?keep=1&mode=old&remove=gone").unwrap();
        let mut headers = HeaderMap::new();

        let mut set_mode = custom_field("mode", FieldPlacement::Query, FieldType::String);
        set_mode.string_value = Some("new".to_string());

        let mut set_enabled = custom_field("enabled", FieldPlacement::Query, FieldType::Boolean);
        set_enabled.id = 2;
        set_enabled.boolean_value = Some(true);

        let mut unset_remove = custom_field("remove", FieldPlacement::Query, FieldType::Unset);
        unset_remove.id = 3;

        let custom_fields = vec![
            Arc::new(set_mode),
            Arc::new(set_enabled),
            Arc::new(unset_remove),
        ];
        handle_custom_fields(&mut data, &mut url, &mut headers, &custom_fields);

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
    fn handle_custom_fields_updates_headers_and_ignores_invalid_entries() {
        let mut data = Value::Null;
        let mut url = Url::parse("https://example.com/v1/chat").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-existing", HeaderValue::from_static("old"));
        headers.insert("x-remove", HeaderValue::from_static("remove-me"));

        let mut replace_existing =
            custom_field("x-existing", FieldPlacement::Header, FieldType::String);
        replace_existing.string_value = Some("new".to_string());

        let mut unset_header = custom_field("x-remove", FieldPlacement::Header, FieldType::Unset);
        unset_header.id = 2;

        let mut invalid_name =
            custom_field("bad header", FieldPlacement::Header, FieldType::String);
        invalid_name.id = 3;
        invalid_name.string_value = Some("ignored".to_string());

        let mut invalid_value =
            custom_field("x-invalid-value", FieldPlacement::Header, FieldType::String);
        invalid_value.id = 4;
        invalid_value.string_value = Some("bad\nvalue".to_string());

        let custom_fields = vec![
            Arc::new(replace_existing),
            Arc::new(unset_header),
            Arc::new(invalid_name),
            Arc::new(invalid_value),
        ];

        handle_custom_fields(&mut data, &mut url, &mut headers, &custom_fields);

        assert_eq!(headers.get("x-existing").unwrap(), "new");
        assert!(headers.get("x-remove").is_none());
        assert!(headers.get("bad header").is_none());
        assert!(headers.get("x-invalid-value").is_none());
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
}
