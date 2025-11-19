use std::{collections::HashMap, sync::Arc};

use axum::http::{HeaderMap, HeaderValue};
use reqwest::{
    header::{ACCEPT_ENCODING, AUTHORIZATION, CONTENT_LENGTH, HOST},
    StatusCode, Url,
};
use serde_json::{json, Value};

use crate::{
    database::{
        custom_field::CustomFieldDefinition,
        model::Model,
        provider::Provider,
    },
    schema::enum_def::{FieldPlacement, FieldType, ProviderType},
    service::{
        app_state::{AppStoreError, AppState, GroupItemSelectionStrategy},
        vertex::get_vertex_token,
    },
    utils::process_stream_options,
};
use cyder_tools::log::{debug, error};

pub fn build_new_headers(
    pre_headers: &HeaderMap,
    api_key: &str,
) -> Result<HeaderMap, (StatusCode, String)> {
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

// Prepares all elements for the downstream LLM request including URL, headers, and body.
pub async fn prepare_llm_request(
    provider: &Provider,
    model: &Model,
    mut data: Value, // Takes ownership of data
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    path: &str,
) -> Result<(String, HeaderMap, String, i64), (StatusCode, String)> {
    debug!(
        "Preparing LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    // 1. Get provider API key
    // TODO: Make selection strategy configurable on the provider. Using Queue for now.
    let selected_provider_api_key = app_state
        .provider_api_key_store
        .get_one_by_group_id(provider.id, GroupItemSelectionStrategy::Queue)
        .map_err(|e| {
            error!(
                "Failed to get provider API key from store for provider_id {}: {:?}",
                provider.id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to retrieve API key for provider '{}'", provider.name),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("No API keys configured for provider '{}'", provider.name),
            )
        })?;

    // 2. Get provider-specific token if needed (e.g., Vertex AI)
    let request_api_key = if provider.provider_type == ProviderType::VertexOpenai {
        get_vertex_token(
            selected_provider_api_key.id,
            &selected_provider_api_key.api_key,
        )
        .await
        .map_err(|err_msg| (StatusCode::BAD_REQUEST, err_msg))?
    } else {
        selected_provider_api_key.api_key.clone()
    };

    // 3. Fetch and combine custom fields for the provider and model
    let provider_cfs = app_state
        .custom_field_link_store
        .get_definitions_by_entity_id(provider.id)
        .map_err(|e| {
            error!(
                "Failed to get custom fields for provider_id {}: {:?}",
                provider.id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to retrieve custom fields for provider".to_string(),
            )
        })?;
    let model_cfs = app_state
        .custom_field_link_store
        .get_definitions_by_entity_id(model.id)
        .map_err(|e| {
            error!(
                "Failed to get custom fields for model_id {}: {:?}",
                model.id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to retrieve custom fields for model".to_string(),
            )
        })?;

    let mut combined_cfs_map: HashMap<i64, CustomFieldDefinition> = HashMap::new();
    for cf in provider_cfs {
        combined_cfs_map.insert(cf.id, cf);
    }
    for cf in model_cfs {
        combined_cfs_map.insert(cf.id, cf);
    }
    let custom_fields: Vec<CustomFieldDefinition> = combined_cfs_map.values().cloned().collect();
    debug!(
        "Fetched {} custom fields for provider and model",
        custom_fields.len()
    );

    // 4. Prepare URL, headers, and apply custom fields
    let target_url = format!("{}/{}", provider.endpoint, path);
    let mut url = Url::parse(&target_url)
        .map_err(|_| (StatusCode::BAD_REQUEST, "failed to parse target url".to_string()))?;
    let mut headers = build_new_headers(original_headers, &request_api_key)?;

    handle_custom_fields(&mut data, &mut url, &mut headers, &custom_fields);

    // 5. Set the real model name in the request body
    let real_model_name_str = model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name);
    if let Some(obj) = data.as_object_mut() {
        obj.insert("model".to_string(), json!(real_model_name_str));
    }

    process_stream_options(&mut data);

    // 6. Serialize final body and return all parts
    let final_body = serde_json::to_string(&data).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize final request body: {}", e),
        )
    })?;

    Ok((
        url.to_string(),
        headers,
        final_body,
        selected_provider_api_key.id,
    ))
}

// Prepares a simple Gemini request for utility endpoints, without custom fields or body transformation.
pub async fn prepare_simple_gemini_request(
    provider: &Provider,
    model: &Model,
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    action: &str,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, i64), (StatusCode, String)> {
    debug!(
        "Preparing simple Gemini request for provider: {}, model: {}, action: {}",
        provider.name, model.model_name, action
    );

    // 1. Get provider API key
    let selected_provider_api_key = app_state
        .provider_api_key_store
        .get_one_by_group_id(provider.id, GroupItemSelectionStrategy::Queue)
        .map_err(|e| {
            error!(
                "Failed to get provider API key from store for provider_id {}: {:?}",
                provider.id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to retrieve API key for provider '{}'", provider.name),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("No API keys configured for provider '{}'", provider.name),
            )
        })?;

    // 2. Get provider-specific token if needed (e.g., Vertex AI)
    let request_api_key = if provider.provider_type == ProviderType::Vertex {
        get_vertex_token(
            selected_provider_api_key.id,
            &selected_provider_api_key.api_key,
        )
        .await
        .map_err(|err_msg| (StatusCode::BAD_REQUEST, err_msg))?
    } else {
        selected_provider_api_key.api_key.clone()
    };

    // 3. Prepare URL
    let real_model_name = model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name);
    let target_url_str = format!("{}/{}:{}", provider.endpoint, real_model_name, action);
    let mut url = Url::parse(&target_url_str)
        .map_err(|_| (StatusCode::BAD_REQUEST, "failed to parse target url".to_string()))?;

    // Append original query params, except 'key'
    for (k, v) in params {
        if k != "key" {
            url.query_pairs_mut().append_pair(k, v);
        }
    }

    // 4. Prepare headers
    let mut final_headers = reqwest::header::HeaderMap::new();
    for (name, value) in original_headers.iter() {
        if name != HOST
            && name != CONTENT_LENGTH
            && name != ACCEPT_ENCODING
            && name != "x-api-key"
            && name != "x-goog-api-key"
            && name != AUTHORIZATION
        {
            final_headers.insert(name.clone(), value.clone());
        }
    }

    if provider.provider_type == ProviderType::Vertex {
        let bearer_token = format!("Bearer {}", request_api_key);
        final_headers.insert(
            AUTHORIZATION,
            reqwest::header::HeaderValue::try_from(bearer_token).unwrap(),
        );
    } else {
        // For Gemini, use X-Goog-Api-Key
        final_headers.insert(
            "X-Goog-Api-Key",
            reqwest::header::HeaderValue::try_from(request_api_key).unwrap(),
        );
    }

    Ok((
        url.to_string(),
        final_headers,
        selected_provider_api_key.id,
    ))
}

// Prepares all elements for a downstream Gemini LLM request.
pub async fn prepare_gemini_llm_request(
    provider: &Provider,
    model: &Model,
    mut data: Value,
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    is_stream: bool,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, String, i64), (StatusCode, String)> {
    debug!(
        "Preparing Gemini LLM request for provider: {}, model: {}",
        provider.name, model.model_name
    );

    // 1. Get provider API key
    let selected_provider_api_key = app_state
        .provider_api_key_store
        .get_one_by_group_id(provider.id, GroupItemSelectionStrategy::Queue)
        .map_err(|e| {
            error!(
                "Failed to get provider API key from store for provider_id {}: {:?}",
                provider.id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to retrieve API key for provider '{}'", provider.name),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("No API keys configured for provider '{}'", provider.name),
            )
        })?;

    // 2. Get provider-specific token if needed (e.g., Vertex AI)
    let request_api_key = if provider.provider_type == ProviderType::Vertex {
        get_vertex_token(
            selected_provider_api_key.id,
            &selected_provider_api_key.api_key,
        )
        .await
        .map_err(|err_msg| (StatusCode::BAD_REQUEST, err_msg))?
    } else {
        selected_provider_api_key.api_key.clone()
    };

    // 3. Prepare URL, headers, and apply custom fields
    let action = if is_stream {
        "streamGenerateContent"
    } else {
        "generateContent"
    };
    let real_model_name = model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name);
    let target_url_str = format!("{}/{}:{}", provider.endpoint, real_model_name, action);
    let mut url = Url::parse(&target_url_str)
        .map_err(|_| (StatusCode::BAD_REQUEST, "failed to parse target url".to_string()))?;

    // Append original query params, except 'key'
    for (k, v) in params {
        if k != "key" {
            url.query_pairs_mut().append_pair(k, v);
        }
    }

    if is_stream {
        url.query_pairs_mut().append_pair("alt", "sse");
    }

    let mut final_headers = reqwest::header::HeaderMap::new();
    for (name, value) in original_headers.iter() {
        if name != HOST
            && name != CONTENT_LENGTH
            && name != ACCEPT_ENCODING
            && name != "x-api-key"
            && name != "x-goog-api-key"
            && name != AUTHORIZATION
        {
            final_headers.insert(name.clone(), value.clone());
        }
    }

    if provider.provider_type == ProviderType::Vertex {
        let bearer_token = format!("Bearer {}", request_api_key);
        final_headers.insert(
            AUTHORIZATION,
            reqwest::header::HeaderValue::try_from(bearer_token).unwrap(),
        );
    } else {
        // For Gemini, use X-Goog-Api-Key
        final_headers.insert(
            "X-Goog-Api-Key",
            reqwest::header::HeaderValue::try_from(request_api_key).unwrap(),
        );
    }

    // Fetch and combine custom fields for the provider and model
    let provider_cfs = app_state
        .custom_field_link_store
        .get_definitions_by_entity_id(provider.id)
        .map_err(|e| {
            error!(
                "Failed to get custom fields for provider_id {}: {:?}",
                provider.id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to retrieve custom fields for provider".to_string(),
            )
        })?;
    let model_cfs = app_state
        .custom_field_link_store
        .get_definitions_by_entity_id(model.id)
        .map_err(|e| {
            error!(
                "Failed to get custom fields for model_id {}: {:?}",
                model.id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to retrieve custom fields for model".to_string(),
            )
        })?;

    let mut combined_cfs_map: HashMap<i64, CustomFieldDefinition> = HashMap::new();
    for cf in provider_cfs {
        combined_cfs_map.insert(cf.id, cf);
    }
    for cf in model_cfs {
        combined_cfs_map.insert(cf.id, cf);
    }
    let custom_fields: Vec<CustomFieldDefinition> = combined_cfs_map.values().cloned().collect();
    debug!(
        "Fetched {} custom fields for provider and model",
        custom_fields.len()
    );

    handle_custom_fields(&mut data, &mut url, &mut final_headers, &custom_fields);

    let final_url = url.to_string();

    // 4. Serialize final body
    let final_body = serde_json::to_string(&data).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize final request body: {}", e),
        )
    })?;

    Ok((
        final_url,
        final_headers,
        final_body,
        selected_provider_api_key.id,
    ))
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
    data: &mut Value,    // For "BODY"
    url: &mut Url,       // For "QUERY"
    headers: &mut HeaderMap, // For "HEADER" (reqwest::header::HeaderMap)
    custom_fields: &Vec<CustomFieldDefinition>,
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
                    FieldType::String => { new_value_opt = cf.string_value.clone(); }
                    FieldType::Integer => { new_value_opt = cf.integer_value.map(|v| v.to_string()); }
                    FieldType::Number => { new_value_opt = cf.number_value.map(|v| v.to_string()); }
                    FieldType::Boolean => { new_value_opt = cf.boolean_value.map(|v| v.to_string()); }
                    FieldType::JsonString => { new_value_opt = cf.string_value.clone(); } // JSON as string for query
                }

                // Rebuild query parameters to ensure replacement
                // First, collect existing pairs to drop the immutable borrow of url.
                let existing_pairs: Vec<(String, String)> = url.query_pairs()
                    .map(|(k,v)| (k.into_owned(), v.into_owned()))
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
                    _ => { // For all other types, convert to string and set header
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
                            match reqwest::header::HeaderName::from_bytes(cf.field_name.as_bytes()) {
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


// Updated to use ProviderStore, ModelStore, and ModelAliasStore from AppState
pub fn get_provider_and_model(app_state: &Arc<AppState>, pre_model_value: &str) -> Result<(Provider, Model), String> {
    // Attempt to resolve as a model alias first
    match app_state.model_alias_store.get_by_key(pre_model_value) {
        Ok(Some(model_alias)) => {
            if !model_alias.is_enabled {
                debug!("Model alias '{}' found but is not enabled. Falling back to provider/model parsing.", pre_model_value);
                // Fall through to provider/model parsing logic below
            } else {
                // Alias found and enabled, try to get model and provider from stores
                let model = app_state.model_store.get_by_id(model_alias.target_model_id)
                    .map_err(|e| format!("Error accessing model store for alias target ID {}: {:?}", model_alias.target_model_id, e))?
                    .ok_or_else(|| format!("Target model ID {} for alias '{}' not found in model store.", model_alias.target_model_id, pre_model_value))?;

                if !model.is_enabled {
                     return Err(format!("Target model '{}' for alias '{}' is not enabled.", model.model_name, pre_model_value));
                }

                let provider = app_state.provider_store.get_by_id(model.provider_id)
                    .map_err(|e| format!("Error accessing provider store for model's provider ID {}: {:?}", model.provider_id, e))?
                    .ok_or_else(|| format!("Provider ID {} for model '{}' (alias '{}') not found in provider store.", model.provider_id, model.model_name, pre_model_value))?;

                if !provider.is_enabled {
                    return Err(format!("Provider '{}' for model '{}' (alias '{}') is not enabled.", provider.name, model.model_name, pre_model_value));
                }
                return Ok((provider, model));
            }
        }
        Ok(None) => {
            // Alias not found by key, fall through to provider/model parsing logic
            debug!("Model alias '{}' not found in store. Attempting provider/model parsing.", pre_model_value);
        }
        Err(AppStoreError::LockError(e)) => {
            error!("ModelAliasStore lock error when getting alias '{}': {}", pre_model_value, e);
            return Err(format!("Internal server error while checking model alias '{}'.", pre_model_value));
        }
        Err(e) => { // Other AppStoreError variants
            error!("ModelAliasStore error when getting alias '{}': {:?}", pre_model_value, e);
            return Err(format!("Internal server error while checking model alias '{}'.", pre_model_value));
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

    let provider = app_state.provider_store.get_by_key(provider_key_str)
        .map_err(|e| format!("Error accessing provider store for key '{}': {:?}", provider_key_str, e))?
        .ok_or_else(|| format!("Provider '{}' not found in store (after alias check failed/skipped).", provider_key_str))?;

    if !provider.is_enabled {
        return Err(format!("Provider '{}' found but is not enabled.", provider.name));
    }

    let model = app_state.model_store.get_by_composite_key(&provider.provider_key, model_name_str)
        .map_err(|e| {
            format!(
                "Error accessing model store for provider '{}' model '{}': {:?}",
                provider.provider_key, model_name_str, e
            )
        })?
        .ok_or_else(|| {
            format!(
                "Model '{}' not found for provider '{}' in store (after alias check failed/skipped). No given provider/model combination was valid.",
                model_name_str, provider.provider_key
            )
        })?;

    if !model.is_enabled {
        return Err(format!("Model '{}' for provider '{}' found but is not enabled.", model.model_name, provider.name));
    }

    Ok((provider, model))
}
