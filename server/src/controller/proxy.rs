use cyder_tools::log::{debug, error, info, warn}; // Removed info as it's re-imported if needed by other macros
use std::{
    collections::HashMap,
    io::Read,
    net::SocketAddr, // For ConnectInfo
    sync::{Arc, Mutex},
};

use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, Path, Query, Request, State}, // Added Query, Added State
    http::{HeaderMap, HeaderValue},
    response::Response,
    routing::{any, get},
};
use crate::controller::llm_types::LlmApiType;
use crate::service::{
    app_state::{create_state_router, StateRouter, AppState, SystemApiKeyStore, AppStoreError, GroupItemSelectionStrategy}, // Added AppState, StateStore, AppStoreError
    vertex::get_vertex_token,
};
use crate::utils::billing::{parse_usage_info, populate_token_cost_fields, UsageInfo};
use crate::service::transform::{
    transform_request_data, transform_result, StreamTransformer,
};
use chrono::Utc;
use flate2::read::GzDecoder;
use futures::StreamExt;
use reqwest::{
    header::{
        ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HOST,
        TRANSFER_ENCODING,
    },
    Method, Proxy, StatusCode, Url,
};
use serde::{Serialize}; // Added Serialize and Deserialize
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::utils::auth::decode_api_key_jwt;
use crate::{
    database::{
        access_control::ApiAccessControlPolicy, // Added ApiAccessControlPolicy
        model::Model,
        custom_field::CustomFieldDefinition,
        provider::Provider,
        price::PriceRule,
        request_log::{NewRequestLog, RequestLog, UpdateRequestLogData},
        system_api_key::SystemApiKey,
    },
    utils::{limit::LIMITER, ID_GENERATOR}, // Added LIMITER
                                                                     // utils::id::generate_snowflake_id, // TODO: Ensure this utility exists or implement it
};
use crate::{
    config::CONFIG,
    utils::{process_stream_options, split_chunks},
};
use crate::schema::enum_def::{FieldPlacement, FieldType, ProviderType, RequestStatus};

// A guard to log a warning if the request is cancelled before completion.
struct CancellationGuard {
    log_id: i64,
    is_armed: bool,
}

impl CancellationGuard {
    fn new(log_id: i64) -> Self {
        Self {
            log_id,
            is_armed: true,
        }
    }

    fn disarm(&mut self) {
        self.is_armed = false;
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if self.is_armed {
            warn!(
                "Request for log_id {} was cancelled by the client.",
                self.log_id
            );
            let update_data = UpdateRequestLogData {
                status: Some(RequestStatus::Cancelled),
                response_sent_to_client_at: Some(Utc::now().timestamp_millis()),
                ..Default::default()
            };
            if let Err(e) = RequestLog::update_request_with_completion_details(self.log_id, &update_data) {
                error!(
                    "Failed to update request log status to CANCELLED for log_id {}: {:?}",
                    self.log_id, e
                );
            }
        }
    }
}

fn build_reqwest_client(use_proxy: bool) -> Result<reqwest::Client, (StatusCode, String)> {
    let mut client_builder = reqwest::Client::builder();
    if use_proxy {
        if let Some(proxy_url) = &CONFIG.proxy {
            let proxy = Proxy::https(proxy_url).map_err(|e| {
                error!("Invalid proxy URL '{}': {}", proxy_url, e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Invalid proxy configuration".to_string(),
                )
            })?;
            client_builder = client_builder.proxy(proxy);
        }
    }
    client_builder.build().map_err(|e| {
        error!("Failed to build reqwest client: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to build HTTP client".to_string(),
        )
    })
}

struct ApiKeyCheckResult {
    api_key: SystemApiKey,
    channel: Option<String>,
    external_id: Option<String>,
}

// Helper to serialize reqwest::header::HeaderMap to JSON String
fn serialize_reqwest_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let mut header_map_simplified = HashMap::new();
    for (name, value) in headers.iter() {
        header_map_simplified.insert(
            name.as_str().to_string(),
            value.to_str().unwrap_or("").to_string(),
        );
    }
    serde_json::to_string(&header_map_simplified).ok()
}

// Helper to serialize axum::http::HeaderMap

const IGNORED_AXUM_HEADERS: [&str; 2] = ["authorization", "cookie"];

fn _serialize_axum_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    let mut header_map_simplified = HashMap::new();
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if IGNORED_AXUM_HEADERS.contains(&name_str.as_str()) {
            continue;
        }
        header_map_simplified.insert(
            name.as_str().to_string(),
            value.to_str().unwrap_or("").to_string(),
        );
    }
    serde_json::to_string(&header_map_simplified).ok()
}

fn build_new_headers(
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

// Helper function to build and log the final update for a request
fn log_final_update(
    log_id: i64,
    context_msg: &str,
    request_url: &str,
    request_body: &str,
    llm_status: Option<StatusCode>, // LLM response status
    // How to update llm_response_body:
    // None: don't touch the field in DB
    // Some(None): set to NULL in DB
    // Some(Some("body")): set to "body" in DB
    llm_body_update: Option<Option<String>>,
    is_stream_val: bool,
    first_chunk_ts: Option<i64>,
    completion_ts: i64,
    usage_opt: Option<&UsageInfo>,
    price_rules: &[PriceRule],
    currency: Option<&str>,
    overall_status: Option<RequestStatus>,
) {
    let is_error = overall_status == Some(RequestStatus::Error);

    let mut update_data = UpdateRequestLogData {
        llm_request_uri: Some(Some(request_url.to_string())),
        llm_request_body: if is_error {
            let truncated_body: String = request_body.chars().take(2000).collect();
            Some(Some(truncated_body))
        } else {
            None
        },
        llm_response_status: llm_status.map(|s| Some(s.as_u16() as i32)),
        llm_response_body: if is_error { llm_body_update } else { None },
        is_stream: Some(is_stream_val),
        llm_response_first_chunk_at: first_chunk_ts,
        llm_response_completed_at: Some(completion_ts),
        response_sent_to_client_at: Some(completion_ts),
        status: overall_status.map(|s| s.clone()),
        ..Default::default()
    };
    populate_token_cost_fields(&mut update_data, usage_opt, price_rules, currency);

    if let Err(e) = RequestLog::update_request_with_completion_details(log_id, &update_data) {
        error!(
            "Failed to update request log ({}) for log_id {}: {:?}",
            context_msg, log_id, e
        );
    }
}

// Authenticates an OpenAI-style request (Bearer token or query param).
async fn authenticate_openai_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, (StatusCode, String)> {
    let system_api_key_str = parse_token_from_request(headers, params)
        .map_err(|err_msg| (StatusCode::UNAUTHORIZED, err_msg))?;
    check_system_api_key(&app_state.system_api_key_store, &system_api_key_str)
        .map_err(|err_msg| (StatusCode::UNAUTHORIZED, err_msg))
}

// Authenticates a Gemini-style request (X-Goog-Api-Key header or 'key' query param).
fn authenticate_gemini_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, (StatusCode, String)> {
    let system_api_key_str = match headers.get("X-Goog-Api-Key") {
        Some(header_value) => match header_value.to_str() {
            Ok(key) => key.to_string(),
            Err(_) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Invalid characters in X-Goog-Api-Key header".to_string(),
                ));
            }
        },
        None => match params.get("key") {
            Some(key) => key.clone(),
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    "Missing API key. Provide it in 'X-Goog-Api-Key' header or 'key' query parameter.".to_string()
                ));
            }
        },
    };
    check_system_api_key(&app_state.system_api_key_store, &system_api_key_str)
        .map_err(|err_msg| (StatusCode::UNAUTHORIZED, err_msg))
}

// Authenticates an Anthropic-style request (x-api-key header).
fn authenticate_anthropic_request(
    headers: &HeaderMap,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, (StatusCode, String)> {
    let system_api_key_str = match headers.get("x-api-key") {
        Some(header_value) => match header_value.to_str() {
            Ok(key) => key.to_string(),
            Err(_) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Invalid characters in x-api-key header".to_string(),
                ));
            }
        },
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Missing API key. Provide it in 'x-api-key' header.".to_string(),
            ));
        }
    };
    check_system_api_key(&app_state.system_api_key_store, &system_api_key_str)
        .map_err(|err_msg| (StatusCode::UNAUTHORIZED, err_msg))
}

// Checks if the request is allowed by the access control policy.
fn check_access_control(
    system_api_key: &SystemApiKey,
    provider: &Provider,
    model: &Model,
    app_state: &Arc<AppState>,
) -> Result<(), (StatusCode, String)> {
    if let Some(policy_id) = system_api_key.access_control_policy_id {
        match app_state.access_control_store.get_by_id(policy_id) {
            Ok(Some(policy)) => {
                if let Err(reason) = LIMITER.check_limit_strategy(&policy, provider.id, model.id) {
                    info!(
                        "Access denied by policy '{}' for SystemApiKey ID {}, Provider ID {}, Model ID {}. Reason: {}",
                        policy.name, system_api_key.id, provider.id, model.id, reason
                    );
                    return Err((
                        StatusCode::FORBIDDEN,
                        format!("Access denied by access control policy: {}", reason),
                    ));
                }
            }
            Ok(None) => {
                let err_msg = format!(
                    "Access control policy id {} configured but not found in application cache.",
                    policy_id
                );
                error!("{}, SystemApiKey ID: {}", err_msg, system_api_key.id);
                return Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg));
            }
            Err(store_err) => {
                let err_msg = format!(
                    "Error accessing application cache for access control policy id {}: {}",
                    policy_id, store_err
                );
                error!("{}, SystemApiKey ID: {}", err_msg, system_api_key.id);
                return Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg));
            }
        }
    }
    Ok(())
}

// Retrieves pricing rules and currency for a given model.
fn get_pricing_info(model: &Model, app_state: &Arc<AppState>) -> (Vec<PriceRule>, Option<String>) {
    if let Some(plan_id) = model.billing_plan_id {
        let rules = app_state
            .price_rule_store
            .list_by_group_id(plan_id)
            .unwrap_or_else(|e| {
                error!(
                    "Failed to get price rules for plan_id {}: {:?}. Cost will not be calculated.",
                    plan_id, e
                );
                Vec::new()
            });

        let plan_currency = match app_state.billing_plan_store.get_by_id(plan_id) {
            Ok(Some(plan)) => Some(plan.currency),
            Ok(None) => {
                error!("Billing plan with id {} not found in store.", plan_id);
                None
            }
            Err(e) => {
                error!("Failed to get billing plan for plan_id {}: {:?}", plan_id, e);
                None
            }
        };

        (rules, plan_currency)
    } else {
        (Vec::new(), None)
    }
}

// Creates an initial request log entry in the database.
fn create_request_log(
    system_api_key: &SystemApiKey,
    provider: &Provider,
    model: &Model,
    provider_api_key_id: i64,
    start_time: i64,
    client_ip_addr: &Option<String>,
    request_uri_path: &str,
    channel: &Option<String>,
    external_id: &Option<String>,
) -> i64 {
    let log_id = ID_GENERATOR.generate_id();
    let real_model_name = model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name);

    let initial_log_data = NewRequestLog {
        id: log_id,
        system_api_key_id: system_api_key.id,
        provider_id: provider.id,
        model_id: model.id,
        provider_api_key_id,
        model_name: model.model_name.clone(),
        real_model_name: real_model_name.to_string(),
        request_received_at: start_time,
        client_ip: client_ip_addr.clone(),
        external_request_uri: Some(request_uri_path.to_string()),
        status: RequestStatus::Pending,
        llm_request_sent_at: Utc::now().timestamp_millis(),
        created_at: start_time,
        updated_at: start_time,
        channel: channel.clone(),
        external_id: external_id.clone(),
    };

    if let Err(e) = RequestLog::create_initial_request(&initial_log_data) {
        error!(
            "Failed to create initial request log for log_id {}: {:?}",
            log_id, e
        );
    }
    log_id
}

// Parses the request body into a JSON Value.
async fn parse_request_body(request: Request<Body>) -> Result<Value, (StatusCode, String)> {
    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Failed to read body: {}", e)))?;

    serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Failed to parse JSON body: {}", e)))
}

// Prepares all elements for the downstream LLM request including URL, headers, and body.
async fn prepare_llm_request(
    provider: &Provider,
    model: &Model,
    mut data: Value, // Takes ownership of data
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    path: &str,
) -> Result<(String, HeaderMap, String, i64), (StatusCode, String)> {
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
async fn prepare_simple_gemini_request(
    provider: &Provider,
    model: &Model,
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    action: &str,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, i64), (StatusCode, String)> {
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
async fn prepare_gemini_llm_request(
    provider: &Provider,
    model: &Model,
    mut data: Value,
    original_headers: &HeaderMap,
    app_state: &Arc<AppState>,
    is_stream: bool,
    params: &HashMap<String, String>,
) -> Result<(String, HeaderMap, String, i64), (StatusCode, String)> {
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

// Handles a non-streaming response from the LLM.
async fn handle_non_streaming_response(
    log_id: i64,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    data: &str,
    price_rules: Vec<PriceRule>,
    currency: Option<String>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    let status_code = response.status();
    let response_headers = response.headers().clone();
    debug!("[handle_non_streaming_response] response headers: {:?}", response_headers);
    let is_gzip = response_headers
        .get(CONTENT_ENCODING)
        .map_or(false, |value| value.to_str().unwrap_or("").contains("gzip"));

    debug!("is gzip {}", is_gzip);

    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        // Do not forward content-length, content-encoding, or transfer-encoding.
        // Axum will set the correct content-length.
        // We handle decompression, so we don't want to forward the original encoding.
        // We are not streaming chunk-by-chunk from the origin, so we don't forward transfer-encoding.
        if name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING {
            response_builder = response_builder.header(name, value);
        }
    }

    let body_bytes = match response.bytes().await {
        Ok(b) => {
            debug!("[handle_non_streaming_response] body bytes: {:?}", b);
            b
        }
        Err(e) => {
            let err_msg = format!("Failed to read LLM response body: {}", e);
            error!("[handle_non_streaming_response] {}", err_msg);
            let completed_at = Utc::now().timestamp_millis();
            log_final_update(log_id, "LLM body read error", url, data, Some(status_code), Some(Some(err_msg.clone())), false, None, completed_at, None, &price_rules, currency.as_deref(), Some(RequestStatus::Error));
            return Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg));
        }
    };

    let decompressed_body = if is_gzip {
        if body_bytes.is_empty() {
            Bytes::new()
        } else {
            let mut gz = GzDecoder::new(&body_bytes[..]);
            let mut decompressed_data = Vec::new();
            match gz.read_to_end(&mut decompressed_data) {
                Ok(_) => Bytes::from(decompressed_data),
                Err(e) => {
                    error!("Gzip decoding failed for log_id {}: {}", log_id, e);
                    body_bytes // return original if decode fails
                }
            }
        }
    } else {
        body_bytes
    };
    debug!("[handle_non_streaming_response] decompressed body: {}", String::from_utf8_lossy(&decompressed_body));
    let llm_response_completed_at = Utc::now().timestamp_millis();

    if status_code.is_success() {
        let parsed_usage_info = serde_json::from_slice::<Value>(&decompressed_body)
            .ok()
            .and_then(|val| parse_usage_info(&val, target_api_type));

        log_final_update(log_id, "Non-SSE success", url, data, Some(status_code), Some(None), false, None, llm_response_completed_at, parsed_usage_info.as_ref(), &price_rules, currency.as_deref(), Some(RequestStatus::Success));
        info!("{}: Non-SSE request completed for log_id {}.", model_str, log_id);

        let final_body = if api_type != target_api_type {
            // Transformation is needed
            let original_value: Value = match serde_json::from_slice(&decompressed_body) {
                Ok(v) => v,
                Err(e) => {
                    // If we can't parse the body, we can't transform it. Return original.
                    error!("Failed to parse LLM response for transformation: {}. Returning original body.", e);
                    return Ok(response_builder.body(Body::from(decompressed_body)).unwrap());
                }
            };

            let transformed_value = transform_result(original_value, target_api_type, api_type);

            match serde_json::to_vec(&transformed_value) {
                Ok(b) => Bytes::from(b),
                Err(e) => {
                    error!("Failed to serialize transformed response: {}. Returning original body.", e);
                    decompressed_body
                }
            }
        } else {
            // No transformation needed
            decompressed_body
        };
        
        Ok(response_builder.body(Body::from(final_body)).unwrap())
    } else {
        let error_body_str = String::from_utf8_lossy(&decompressed_body).into_owned();
        error!("LLM request failed with status {} for log_id {}: {}", status_code, log_id, &error_body_str);
        log_final_update(log_id, "LLM error status", url, data, Some(status_code), Some(Some(error_body_str.clone())), false, None, llm_response_completed_at, None, &price_rules, currency.as_deref(), Some(RequestStatus::Error));
        
        Ok(response_builder.body(Body::from(error_body_str)).unwrap())
    }
}

// Handles a streaming (SSE) response from the LLM.
async fn handle_streaming_response(
    log_id: i64,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    data: &str,
    price_rules: Vec<PriceRule>,
    currency: Option<String>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    let status_code = response.status();
    let response_headers = response.headers().clone();

    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        // Do not forward content-length, content-encoding, or transfer-encoding.
        // Axum will set the correct content-length for the stream.
        // We are not forwarding the original encoding.
        // We are re-streaming, so we don't forward transfer-encoding.
        if name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING {
            response_builder = response_builder.header(name, value);
        }
    }

    let mut first_chunk_received_at_proxy: i64 = 0;
    let latest_chunk_arc: Arc<Mutex<Option<Bytes>>> = Arc::new(Mutex::new(None));
    let logged_sse_success = Arc::new(Mutex::new(false));
    let (tx, mut rx) = mpsc::channel::<Result<bytes::Bytes, reqwest::Error>>(10);

    let url_owned = url.to_string();
    let data_owned = data.to_string();
    let logged_sse_success_clone = logged_sse_success.clone();

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        while let Some(chunk_result) = stream.next().await {
            if tx.send(chunk_result).await.is_err() {
                break;
            }
        }
    });

    let mut transformer = StreamTransformer::new(target_api_type, api_type);

    let monitored_stream = async_stream::stream! {
        let mut remainder = Bytes::new();
        while let Some(chunk_result) = rx.recv().await {
            match chunk_result {
                Ok(chunk) => {
                    if first_chunk_received_at_proxy == 0 {
                        first_chunk_received_at_proxy = Utc::now().timestamp_millis();
                    }

                    let current_chunk = if remainder.is_empty() {
                        chunk
                    } else {
                        Bytes::from([remainder.as_ref(), chunk.as_ref()].concat())
                    };
                    let (lines, new_remainder) = split_chunks(current_chunk);
                    remainder = new_remainder;

                    if lines.is_empty() {
                        continue;
                    }

                    let mut transformed_sub_chunks_as_strings: Vec<String> = Vec::new();
                    for sub_chunk in &lines {
                        // This part is for logging usage from the final chunk of an OpenAI stream.
                        if target_api_type == LlmApiType::OpenAI {
                            let line_str = String::from_utf8_lossy(sub_chunk);
                            if line_str.trim() == "data: [DONE]" {
                                if let Some(final_data_chunk) = latest_chunk_arc.lock().unwrap().take() {
                                    let final_line_str = String::from_utf8_lossy(&final_data_chunk);
                                    if let Some(data_json_str) = final_line_str.strip_prefix("data:").map(str::trim) {
                                        if let Ok(data_value) = serde_json::from_str::<Value>(data_json_str) {
                                            let parsed_usage_info = parse_usage_info(&data_value, target_api_type);
                                            let completed_at = Utc::now().timestamp_millis();
                                            let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };
                                            log_final_update(log_id, "SSE DONE", &url_owned, &data_owned, Some(status_code), Some(None), true, first_chunk_ts, completed_at, parsed_usage_info.as_ref(), &price_rules, currency.as_deref(), Some(RequestStatus::Success));
                                            *logged_sse_success_clone.lock().unwrap() = true;
                                            info!("{}: SSE stream completed.", model_str);
                                        }
                                    }
                                }
                            } else if line_str.starts_with("data:") {
                                *latest_chunk_arc.lock().unwrap() = Some(sub_chunk.clone());
                            }
                        }

                        let transformed_bytes_opt = transformer.transform_chunk(sub_chunk.clone());

                        if let Some(transformed_bytes) = transformed_bytes_opt {
                            let s = if transformed_bytes.is_empty() {
                                String::new()
                            } else {
                                if api_type == LlmApiType::OpenAI && target_api_type == LlmApiType::OpenAI {
                                    format!("{}\n\n", String::from_utf8_lossy(&transformed_bytes))
                                } else {
                                    String::from_utf8_lossy(&transformed_bytes).to_string()
                                }
                            };
                            transformed_sub_chunks_as_strings.push(s);
                        }
                    }

                    let transformed_chunk = Bytes::from(transformed_sub_chunks_as_strings.concat());

                    // Forward the transformed chunk to the client
                    if !transformed_chunk.is_empty() {
                        yield Ok::<_, std::io::Error>(transformed_chunk);
                    }
                }
                Err(e) => {
                    let stream_error_message = format!("LLM stream error: {}", e);
                    error!("{}", stream_error_message);
                    let completed_at = Utc::now().timestamp_millis();
                    let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };

                    log_final_update(
                        log_id, "LLM stream error", &url_owned, &data_owned, Some(status_code),
                        Some(None), true, first_chunk_ts, completed_at, None, &price_rules, currency.as_deref(), Some(RequestStatus::Error),
                    );
                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, stream_error_message));
                    break;
                }
            }
        }

        if status_code.is_success() && !remainder.is_empty() {
            let line_str = String::from_utf8_lossy(&remainder);
            if line_str.starts_with("data:") {
                *latest_chunk_arc.lock().unwrap() = Some(remainder);
            }
        }

        // After the upstream is closed, check if we need to send a [DONE] message.
        if status_code.is_success() && api_type == LlmApiType::OpenAI && target_api_type == LlmApiType::Gemini {
            // The client expects an OpenAI stream, which must end with [DONE].
            // The Gemini stream we just consumed doesn't have this, so we add it.
            debug!("[handle_streaming_response] Appending [DONE] chunk for OpenAI client.");
            let done_chunk = Bytes::from("data: [DONE]\n\n");
            yield Ok::<_, std::io::Error>(done_chunk);
        }

        let llm_response_completed_at = Utc::now().timestamp_millis();

        if status_code.is_success() && !*logged_sse_success.lock().unwrap() {
            if let Some(final_data_chunk) = latest_chunk_arc.lock().unwrap().take() {
                let final_line_str = String::from_utf8_lossy(&final_data_chunk);
                if let Some(data_json_str) = final_line_str.strip_prefix("data:").map(str::trim) {
                    if let Ok(data_value) = serde_json::from_str::<Value>(data_json_str) {
                        debug!("[proxy] final response from stream end {:?}", data_value);
                        let parsed_usage_info = parse_usage_info(&data_value, target_api_type);
                        let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };

                        log_final_update(
                            log_id, "SSE stream end", &url_owned, &data_owned, Some(status_code),
                            Some(None), true, first_chunk_ts, llm_response_completed_at,
                            parsed_usage_info.as_ref(), &price_rules, currency.as_deref(), Some(RequestStatus::Success),
                        );
                        info!("{}: SSE stream completed at stream end.", model_str);
                    } else {
                        error!("Failed to parse final SSE data JSON at stream end for log_id {}: {}", log_id, data_json_str);
                    }
                }
            } else {
                info!("{}: SSE stream completed without a final data chunk to parse.", model_str);
                let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };
                log_final_update(
                    log_id, "SSE stream end (no final chunk)", &url_owned, &data_owned, Some(status_code),
                    Some(None), true, first_chunk_ts, llm_response_completed_at,
                    None, &price_rules, currency.as_deref(), Some(RequestStatus::Success),
                );
            }
        } else if !status_code.is_success() {
            let error_body_str = "Error during stream".to_string();
            let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };
            log_final_update(
                log_id, "LLM error status", &url_owned, &data_owned, Some(status_code),
                Some(Some(error_body_str)), true, first_chunk_ts,
                llm_response_completed_at, None, &price_rules, currency.as_deref(), Some(RequestStatus::Error),
            );
        }
    };

    match response_builder.body(Body::from_stream(monitored_stream)) {
        Ok(final_response) => Ok(final_response),
        Err(e) => {
            let error_message = format!("Failed to build client response for log_id {}: {}", log_id, e);
            error!("{}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

// Dispatches to the correct response handler based on whether the response is a stream.
async fn handle_llm_response(
    log_id: i64,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    data: &str,
    price_rules: Vec<PriceRule>,
    currency: Option<String>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    let is_sse = response.headers()
        .get(CONTENT_TYPE)
        .map_or(false, |value| {
            value.to_str().unwrap_or("").contains("text/event-stream")
        });

    if let Err(e) = RequestLog::update_request_with_completion_details(
        log_id,
        &UpdateRequestLogData {
            is_stream: Some(is_sse),
            ..Default::default()
        },
    ) {
        error!("Failed to update request log (is_stream) for log_id {}: {:?}", log_id, e);
    }

    if is_sse {
        handle_streaming_response(log_id, model_str, response, url, data, price_rules, currency, api_type, target_api_type).await
    } else {
        handle_non_streaming_response(log_id, model_str, response, url, data, price_rules, currency, api_type, target_api_type).await
    }
}

// A simple proxy that sends a request and returns the response, handling streaming and gzip.
// It does not perform logging or response transformation.
async fn simple_proxy_request(
    url: String,
    data: String,
    headers: HeaderMap,
    use_proxy: bool,
) -> Result<Response<Body>, (StatusCode, String)> {
    let client = build_reqwest_client(use_proxy)?;

    debug!(
        "[simple_proxy_request] request header: {:?}",
        serialize_reqwest_headers(&headers)
    );
    debug!("[simple_proxy_request] request data: {}", &data);

    let response = match client
        .request(Method::POST, &url)
        .headers(headers)
        .body(data)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            let error_message = format!("LLM request failed: {}", e);
            error!("{}", error_message);
            return Err((StatusCode::BAD_GATEWAY, error_message));
        }
    };

    let status_code = response.status();
    let response_headers = response.headers().clone();
    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        if name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING {
            response_builder = response_builder.header(name, value);
        }
    }

    let is_sse = response_headers
        .get(CONTENT_TYPE)
        .map_or(false, |value| {
            value.to_str().unwrap_or("").contains("text/event-stream")
        });

    if is_sse {
        let body = Body::from_stream(
            response
                .bytes_stream()
                .map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))),
        );
        Ok(response_builder.body(body).unwrap())
    } else {
        let is_gzip = response_headers
            .get(CONTENT_ENCODING)
            .map_or(false, |value| value.to_str().unwrap_or("").contains("gzip"));

        let body_bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                let err_msg = format!("Failed to read LLM response body: {}", e);
                error!("[simple_proxy_request] {}", err_msg);
                return Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg));
            }
        };

        let decompressed_body = if is_gzip {
            if body_bytes.is_empty() {
                Bytes::new()
            } else {
                let mut gz = GzDecoder::new(&body_bytes[..]);
                let mut decompressed_data = Vec::new();
                match gz.read_to_end(&mut decompressed_data) {
                    Ok(_) => Bytes::from(decompressed_data),
                    Err(e) => {
                        error!("Gzip decoding failed in simple_proxy_request: {}", e);
                        body_bytes // return original if decode fails
                    }
                }
            }
        } else {
            body_bytes
        };
        Ok(response_builder.body(Body::from(decompressed_body)).unwrap())
    }
}

// Builds the HTTP client, sends the request to the LLM, and passes the response to be handled.
async fn proxy_request(
    log_id: i64,
    url: String,
    data: String,
    headers: HeaderMap,
    model_str: String,
    use_proxy: bool,
    price_rules: Vec<PriceRule>,
    currency: Option<String>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    // 1. Build HTTP client, with proxy if configured
    let client = build_reqwest_client(use_proxy)?;

    let mut cancellation_guard = CancellationGuard::new(log_id);

    debug!(
        "[proxy] proxy request header: {:?}",
        serialize_reqwest_headers(&headers)
    );
    debug!("[proxy] proxy request data: {}", &data);

    // 2. Send request to LLM
    let response = match client
        .request(Method::POST, &url)
        .headers(headers)
        .body(data.clone()) // Clone here for potential retries or logging
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            cancellation_guard.disarm();
            let error_message = format!("LLM request failed: {}", e);
            error!("{}", error_message);
            let completed_at = Utc::now().timestamp_millis();
            log_final_update(
                log_id,
                "LLM send error",
                &url,
                &data,
                None,
                Some(None),
                false,
                None,
                completed_at,
                None,
                &price_rules,
                currency.as_deref(),
                Some(RequestStatus::Error),
            );
            return Err((StatusCode::BAD_GATEWAY, error_message));
        }
    };

    // 3. Process the response stream
    let result = handle_llm_response(
        log_id,
        model_str,
        response,
        &url,
        &data,
        price_rules,
        currency,
        api_type,
        target_api_type,
    )
    .await;
    cancellation_guard.disarm();
    result
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

fn handle_custom_fields(
    data: &mut Value,    // For "BODY"
    url: &mut Url,       // For "QUERY"
    headers: &mut HeaderMap, // For "HEADER" (reqwest::header::HeaderMap)
    custom_fields: &Vec<CustomFieldDefinition>,
) {
    for cf in custom_fields {
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

const BEARER_PREFIX: &str = "Bearer ";
fn parse_token_from_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
) -> Result<String, String> {
    if let Some(auth_header_value) = headers.get(AUTHORIZATION) {
        if let Ok(auth_str) = auth_header_value.to_str() {
            if let Some(token) = auth_str.strip_prefix(BEARER_PREFIX) {
                if !token.is_empty() && token != "raspberry" {
                    return Ok(token.to_string());
                }
            }
        }
    }

    // Fallback to query parameter
    params.get("key").cloned().ok_or_else(|| {
        "Missing API key. Provide it in 'Authorization' header or 'key' query parameter."
            .to_string()
    })
}

// Updated to query from the new StateStore<SystemApiKey> struct
fn check_system_api_key(
    store: &SystemApiKeyStore,
    key_str: &str,
) -> Result<ApiKeyCheckResult, String> {
    if key_str.starts_with("cyder-") {
        match store.get_by_key(key_str) {
            Ok(Some(api_key)) => Ok(ApiKeyCheckResult {
                api_key,
                channel: None,
                external_id: None,
            }),
            Ok(None) => Err("api key invalid or not found".to_string()),
            Err(AppStoreError::LockError(e)) => {
                error!("SystemApiKeyStore lock error: {}", e);
                Err("Internal server error while checking API key".to_string())
            }
            Err(e) => {
                // Catch other AppStoreError variants if any, though get_by_key primarily returns Option or LockError
                error!("SystemApiKeyStore error: {:?}", e);
                Err("Internal server error while checking API key".to_string())
            }
        }
    } else if let Some(token) = key_str.strip_prefix("jwt-") {
        let jwt_result =
            decode_api_key_jwt(token).map_err(|e| format!("Invalid JWT token: {:?}", e))?;

        match store.get_by_ref(&jwt_result.key_ref) {
            Ok(Some(api_key)) => Ok(ApiKeyCheckResult {
                api_key,
                channel: Some(jwt_result.channel),
                external_id: Some(jwt_result.sub),
            }),
            Ok(None) => Err(format!(
                "api key for ref '{}' invalid or not found",
                jwt_result.key_ref
            )),
            Err(AppStoreError::LockError(e)) => {
                error!("SystemApiKeyStore lock error: {}", e);
                Err("Internal server error while checking API key by ref".to_string())
            }
            Err(e) => {
                error!("SystemApiKeyStore error: {:?}", e);
                Err("Internal server error while checking API key by ref".to_string())
            }
        }
    } else {
        Err("Invalid api key format. Must start with 'cyder-' or 'jwt-'".to_string())
    }
}

// Updated to use ProviderStore, ModelStore, and ModelAliasStore from AppState
fn get_provider_and_model(app_state: &Arc<AppState>, pre_model_value: &str) -> Result<(Provider, Model), String> {
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

fn parse_utility_usage_info(response_body: &Value) -> Option<UsageInfo> {
    let tokens = response_body
        .get("usage")
        .and_then(|u| u.get("total_tokens"))
        .and_then(|t| t.as_i64())
        .or_else(|| {
            response_body
                .get("meta")
                .and_then(|m| m.get("tokens"))
                .and_then(|t| t.get("input_tokens"))
                .and_then(|it| it.as_i64())
        });

    tokens.map(|t| UsageInfo {
        prompt_tokens: t as i32,
        completion_tokens: 0,
        total_tokens: t as i32,
        reasoning_tokens: 0,
    })
}

// A generic handler for non-streaming OpenAI-compatible endpoints like embeddings and rerank.
async fn openai_utility_handler(
    app_state: Arc<AppState>,
    addr: SocketAddr,
    params: HashMap<String, String>,
    request: Request<Body>,
    downstream_path: &str,
) -> Result<Response<Body>, (StatusCode, String)> {
    let client_ip_addr = Some(addr.ip().to_string());
    let start_time = Utc::now().timestamp_millis();
    let request_uri_path = request.uri().path().to_string();
    let original_headers = request.headers().clone();

    // Step 1: Authenticate
    let api_key_check_result =
        authenticate_openai_request(&original_headers, &params, &app_state).await?;
    let system_api_key = api_key_check_result.api_key;
    let channel = api_key_check_result.channel;
    let external_id = api_key_check_result.external_id;

    // Step 2: Parse body
    let data = parse_request_body(request).await?;
    debug!(
        "[{}] original request data: {}",
        downstream_path,
        serde_json::to_string(&data).unwrap_or_default()
    );

    // Step 3: Get provider and model
    let pre_model_str = data.get("model").and_then(Value::as_str).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "'model' field must be a string".to_string(),
        )
    })?;
    let (provider, model) =
        get_provider_and_model(&app_state, pre_model_str).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    // Step 4: Check if provider is OpenAI compatible
    let target_api_type = if provider.provider_type == ProviderType::Vertex
        || provider.provider_type == ProviderType::Gemini
    {
        LlmApiType::Gemini
    } else {
        LlmApiType::OpenAI
    };

    if target_api_type != LlmApiType::OpenAI {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("'{}' is only supported for OpenAI-compatible providers.", downstream_path),
        ));
    }

    // Step 5: Pricing info
    let (price_rules, currency) = get_pricing_info(&model, &app_state);

    // Step 6: Access control
    check_access_control(&system_api_key, &provider, &model, &app_state)?;

    // Step 7: Prepare downstream request
    let (final_url, final_headers, final_body, provider_api_key_id) = prepare_llm_request(
        &provider,
        &model,
        data,
        &original_headers,
        &app_state,
        downstream_path,
    )
    .await?;

    // Step 8: Execute request against downstream
    let client = build_reqwest_client(provider.use_proxy)?;

    debug!(
        "[{}] proxy request header: {:?}",
        downstream_path,
        serialize_reqwest_headers(&final_headers)
    );
    debug!("[{}] proxy request data: {}", downstream_path, &final_body);

    let response = match client
        .request(Method::POST, &final_url)
        .headers(final_headers)
        .body(final_body.clone())
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            // Per user request, don't log if request fails to send.
            let error_message = format!("LLM request failed: {}", e);
            error!("[{}] {}", downstream_path, error_message);
            return Err((StatusCode::BAD_GATEWAY, error_message));
        }
    };

    // Step 9: Process response, log if successful, and forward original response

    let real_model_name = model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name);
    let model_str = if model.model_name == real_model_name {
        format!("{}/{}", &provider.provider_key, &model.model_name)
    } else {
        format!(
            "{}/{}({})",
            &provider.provider_key, &model.model_name, real_model_name
        )
    };

    // Step 10: Process response, log, and forward original response
    let status_code = response.status();
    let response_headers = response.headers().clone();

    let body_bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            let err_msg = format!("Failed to read LLM response body: {}", e);
            error!("[{}] {}", downstream_path, err_msg);
            // No logging on body read error, just return error
            return Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg));
        }
    };

    if status_code.is_success() {
        let is_gzip = response_headers
            .get(CONTENT_ENCODING)
            .map_or(false, |value| value.to_str().unwrap_or("").contains("gzip"));

        // Decompress body for parsing usage, but original body_bytes will be sent to client.
        let body_for_parsing = if is_gzip {
            if body_bytes.is_empty() {
                Bytes::new()
            } else {
                let mut gz = GzDecoder::new(&body_bytes[..]);
                let mut decompressed_data = Vec::new();
                match gz.read_to_end(&mut decompressed_data) {
                    Ok(_) => Bytes::from(decompressed_data),
                    Err(e) => {
                        error!("Gzip decoding failed for {} request: {}", downstream_path, e);
                        Bytes::new() // Can't parse if decoding fails
                    }
                }
            }
        } else {
            body_bytes.clone()
        };

        let llm_response_completed_at = Utc::now().timestamp_millis();

        let log_id = create_request_log(
            &system_api_key,
            &provider,
            &model,
            provider_api_key_id,
            start_time,
            &client_ip_addr,
            &request_uri_path,
            &channel,
            &external_id,
        );
        let parsed_usage_info = serde_json::from_slice::<Value>(&body_for_parsing)
            .ok()
            .and_then(|val| parse_utility_usage_info(&val));

        log_final_update(log_id, "Non-SSE success", &final_url, &final_body, Some(status_code), Some(None), false, None, llm_response_completed_at, parsed_usage_info.as_ref(), &price_rules, currency.as_deref(), Some(RequestStatus::Success));
        info!("{}: Non-SSE request completed for log_id {}.", model_str, log_id);
    } else {
        let error_body_str = String::from_utf8_lossy(&body_bytes).into_owned();
        error!("[{}] LLM request failed with status {}: {}", downstream_path, status_code, &error_body_str);
    }

    // Build response to client, forwarding original headers and body
    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        response_builder = response_builder.header(name, value);
    }

    Ok(response_builder.body(Body::from(body_bytes)).unwrap())
}

// The new unified handler for all proxy requests.
async fn proxy_handler(
    app_state: Arc<AppState>,
    addr: SocketAddr,
    path_segment: Option<String>,
    query_params: Option<HashMap<String, String>>,
    request: Request<Body>,
    api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    let client_ip_addr = Some(addr.ip().to_string());
    let start_time = Utc::now().timestamp_millis();
    let request_uri_path = request.uri().path().to_string();
    let original_headers = request.headers().clone();

    debug!("{} --- {:?}", &request_uri_path, query_params);

    // Step 1: Authenticate the request and retrieve API key.
    let api_key_check_result = match api_type {
        LlmApiType::OpenAI => {
            let empty_params = HashMap::new();
            let params = query_params.as_ref().unwrap_or(&empty_params);
            authenticate_openai_request(&original_headers, params, &app_state).await?
        }
        LlmApiType::Gemini => {
            let empty_params = HashMap::new();
            let params = query_params.as_ref().unwrap_or(&empty_params);
            authenticate_gemini_request(&original_headers, params, &app_state)?
        }
        LlmApiType::Anthropic => authenticate_anthropic_request(&original_headers, &app_state)?,
        _ => return Err((StatusCode::INTERNAL_SERVER_ERROR, "unsupported api type".to_string()))
    };
    let system_api_key = api_key_check_result.api_key;
    let channel = api_key_check_result.channel;
    let external_id = api_key_check_result.external_id;

    // Step 2: Parse the incoming request body.
    let mut data = parse_request_body(request).await?;
    debug!("[proxy] original request data: {}", serde_json::to_string(&data).unwrap_or_default());

    // Step 3: Determine the provider and model.
    let (provider, model, action) = match api_type {
        LlmApiType::OpenAI => {
            let pre_model_str = data
                .get("model")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        "'model' field must be a string".to_string(),
                    )
                })?;
            let (provider, model) =
                get_provider_and_model(&app_state, pre_model_str).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
            (provider, model, None)
        }
        LlmApiType::Gemini => {
            let model_action_segment = path_segment.as_ref().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Missing model/action path segment".to_string(),
                )
            })?;
            let parts: Vec<&str> = model_action_segment.rsplitn(2, ':').collect();
            if parts.len() != 2 {
                let err_msg = format!(
                    "Invalid model_action_segment format: '{}'. Expected 'model_name:action'.",
                    model_action_segment
                );
                error!("[gemini_models_handler] {}", err_msg);
                return Err((StatusCode::BAD_REQUEST, err_msg));
            }
            let model_name = parts[1];
            let action = parts[0];

            const GEMINI_GENERATION_ACTIONS: [&str; 2] =
                ["generateContent", "streamGenerateContent"];
            const GEMINI_UTILITY_ACTIONS: [&str; 3] =
                ["countMessageTokens", "countTextTokens", "countTokens"];

            if GEMINI_UTILITY_ACTIONS.contains(&action) {
                // Handle utility actions: simple proxy, no logging
                let (provider, model) = get_provider_and_model(&app_state, model_name)
                    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

                let target_api_type = if provider.provider_type == ProviderType::Vertex
                    || provider.provider_type == ProviderType::Gemini
                {
                    LlmApiType::Gemini
                } else {
                    LlmApiType::OpenAI
                };

                debug!("{:?}", provider.provider_type);

                if target_api_type != LlmApiType::Gemini {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!(
                            "Action '{}' is only supported for Gemini-compatible providers.",
                            action
                        ),
                    ));
                }

                let empty_params = HashMap::new();
                let params = query_params.as_ref().unwrap_or(&empty_params);

                let (final_url, final_headers, _) = prepare_simple_gemini_request(
                    &provider,
                    &model,
                    &original_headers,
                    &app_state,
                    action,
                    params,
                )
                .await?;

                let final_body = serde_json::to_string(&data).map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to serialize final request body: {}", e),
                    )
                })?;

                return simple_proxy_request(
                    final_url,
                    final_body,
                    final_headers,
                    provider.use_proxy,
                )
                .await;
            }

            if !GEMINI_GENERATION_ACTIONS.contains(&action) {
                let err_msg = format!(
                    "Invalid action: '{}'. Must be one of 'generateContent', 'streamGenerateContent', 'countMessageTokens', 'countTextTokens', or 'countTokens'.",
                    action
                );
                error!("[gemini_models_handler] {}", err_msg);
                return Err((StatusCode::BAD_REQUEST, err_msg));
            }

            let (provider, model) = get_provider_and_model(&app_state, model_name)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

            (provider, model, Some(action.to_string()))
        }
        LlmApiType::Anthropic => {
            let pre_model_str = data
                .get("model")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        "'model' field must be a string".to_string(),
                    )
                })?;
            let (provider, model) =
                get_provider_and_model(&app_state, pre_model_str).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
            (provider, model, None)
        }
        _ => return Err((StatusCode::INTERNAL_SERVER_ERROR, "unsupported api type".to_string()))
    };

    let target_api_type = if provider.provider_type == ProviderType::Vertex
        || provider.provider_type == ProviderType::Gemini
    {
        LlmApiType::Gemini
    } else {
        LlmApiType::OpenAI
    };

    let is_stream = match api_type {
        LlmApiType::OpenAI => data.get("stream").and_then(Value::as_bool).unwrap_or(false),
        LlmApiType::Gemini => action.as_deref() == Some("streamGenerateContent"),
        LlmApiType::Anthropic => data.get("stream").and_then(Value::as_bool).unwrap_or(false),
        _ => return Err((StatusCode::INTERNAL_SERVER_ERROR, "unsupported api type".to_string()))
    };

    data = transform_request_data(data, api_type, target_api_type, is_stream);

    let (price_rules, currency) = get_pricing_info(&model, &app_state);

    // Step 4: If an access policy is present, check if the request is allowed.
    check_access_control(&system_api_key, &provider, &model, &app_state)?;

    // Step 5: Prepare the downstream request details (URL, headers, body).
    let (final_url, final_headers, final_body, provider_api_key_id) = match target_api_type {
        LlmApiType::OpenAI => {
            prepare_llm_request(
                &provider,
                &model,
                data,
                &original_headers,
                &app_state,
                "chat/completions",
            )
            .await?
        }
        LlmApiType::Gemini => {
            let empty_params = HashMap::new();
            let params = query_params.as_ref().unwrap_or(&empty_params);
            prepare_gemini_llm_request(
                &provider,
                &model,
                data,
                &original_headers,
                &app_state,
                is_stream,
                params,
            )
            .await?
        }
        _ => return Err((StatusCode::INTERNAL_SERVER_ERROR, "unsupported api type".to_string()))
    };

    // Step 6: Create an initial log entry for the request.
    let log_id = create_request_log(
        &system_api_key,
        &provider,
        &model,
        provider_api_key_id,
        start_time,
        &client_ip_addr,
        &request_uri_path,
        &channel,
        &external_id,
    );

    // Step 7: Execute the request against the downstream LLM service.
    let real_model_name = model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name);
    let model_str = if model.model_name == real_model_name {
        format!("{}/{}", &provider.provider_key, &model.model_name)
    } else {
        format!(
            "{}/{}({})",
            &provider.provider_key, &model.model_name, real_model_name
        )
    };

    proxy_request(
        log_id,
        final_url,
        final_body,
        final_headers,
        model_str,
        provider.use_proxy,
        price_rules,
        currency,
        api_type,
        target_api_type,
    )
    .await
}

fn create_openai_router() -> StateRouter {
    create_state_router()
        .route(
            "/chat/completions",
            any(
                |State(app_state),
                 Query(query_params): Query<HashMap<String, String>>,
                 ConnectInfo(addr),
                 request: Request<Body>| async move {
                    proxy_handler(
                        app_state,
                        addr,
                        None,
                        Some(query_params),
                        request,
                        LlmApiType::OpenAI,
                    )
                    .await
                },
            ),
        )
        .route(
            "/embeddings",
            any(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 ConnectInfo(addr),
                 request: Request<Body>| async move {
                    openai_utility_handler(app_state, addr, params, request, "embeddings")
                        .await
                },
            ),
        )
        .route(
            "/rerank",
            any(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 ConnectInfo(addr),
                 request: Request<Body>| async move {
                    openai_utility_handler(app_state, addr, params, request, "rerank").await
                },
            ),
        )
        .route(
            "/models",
            get(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 request: Request<Body>| async move {
                    list_models_handler(app_state, params, request, LlmApiType::OpenAI).await
                },
            ),
        )
}

fn create_anthropic_router() -> StateRouter {
    create_state_router()
        .route(
            "/messages",
            any(
                |State(app_state), ConnectInfo(addr), request: Request<Body>| async move {
                    proxy_handler(
                        app_state,
                        addr,
                        None,
                        None,
                        request,
                        LlmApiType::Anthropic,
                    )
                    .await
                },
            ),
        )
        .route(
            "/models",
            get(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 request: Request<Body>| async move {
                    list_models_handler(app_state, params, request, LlmApiType::Anthropic).await
                },
            ),
        )
}

pub fn create_proxy_router() -> StateRouter {
    let openai_router = create_openai_router();
    let anthropic_router = create_anthropic_router();
    create_state_router()
        .nest("/openai", openai_router.clone())
        .nest("/openai/v1", openai_router)
        .nest("/anthropic", anthropic_router.clone())
        .nest("/anthropic/v1", anthropic_router)
        .route(
            "/gemini/v1beta/models", // Exact match for listing models
            get(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 request: Request<Body>| async move {
                    list_models_handler(app_state, params, request, LlmApiType::Gemini).await
                },
            ),
        )
        .route(
            "/gemini/v1beta/models/{*model_action_segment}", // Wildcard for model actions
            any(
                |Path(path_segment): Path<String>,
                 Query(query_params): Query<HashMap<String, String>>,
                 State(app_state),
                 ConnectInfo(addr),
                 request: Request<Body>| async move {
                    proxy_handler(
                        app_state,
                        addr,
                        Some(path_segment),
                        Some(query_params),
                        request,
                        LlmApiType::Gemini,
                    )
                    .await
                },
            ),
        )
}

#[derive(Debug)]
struct AccessibleModel {
    id: String,
    owned_by: String,
    provider_type: ProviderType,
}

async fn get_accessible_models(
    app_state: &Arc<AppState>,
    system_api_key: &SystemApiKey,
) -> Result<Vec<AccessibleModel>, (StatusCode, String)> {
    // 1. Fetch Access Control Policy if ID is present
    let access_control_policy_opt: Option<ApiAccessControlPolicy> =
        if let Some(policy_id) = system_api_key.access_control_policy_id {
            match app_state.access_control_store.get_by_id(policy_id) {
                Ok(Some(policy)) => Some(policy),
                Ok(None) => {
                    error!("Access control policy with id {} not found in store (configured on SystemApiKey {}).", policy_id, system_api_key.id);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!(
                            "Access control policy id {} configured but not found in application cache.",
                            policy_id
                        ),
                    ));
                }
                Err(store_err) => {
                    error!("Failed to fetch access control policy with id {} from store: {:?}", policy_id, store_err);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error accessing application cache for access control policy id {}: {}", policy_id, store_err),
                    ));
                }
            }
        } else {
            None
        };

    let mut available_models: Vec<AccessibleModel> = Vec::new();

    // 2. Get all active providers
    let active_providers = Provider::list_all_active().map_err(|e| {
        error!("Failed to list active providers: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to retrieve provider list".to_string(),
        )
    })?;

    for provider in active_providers {
        // 3. Get all active models for this provider
        let active_models = Model::list_active_by_provider_id(provider.id).map_err(|e| {
            error!(
                "Failed to list active models for provider {}: {:?}",
                provider.id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Failed to retrieve model list for provider {}",
                    provider.name
                ),
            )
        })?;

        for model in active_models {
            let mut allowed = false;
            if let Some(ref policy) = access_control_policy_opt {
                // 4a. Check against policy if one is loaded
                match LIMITER.check_limit_strategy(policy, provider.id, model.id) {
                    Ok(_) => {
                        allowed = true;
                    }
                    Err(reason) => {
                        debug!(
                            "Model {}/{} denied by policy '{}' for SystemApiKey ID {}. Reason: {}",
                            provider.provider_key,
                            model.model_name,
                            policy.name,
                            system_api_key.id,
                            reason
                        );
                    }
                }
            } else {
                // 4b. No policy loaded, model is allowed by default
                allowed = true;
            }

            if allowed {
                available_models.push(AccessibleModel {
                    id: format!("{}/{}", provider.provider_key, model.model_name),
                    owned_by: provider.provider_key.clone(),
                    provider_type: provider.provider_type.clone(),
                });
            }
        }
    }

    // 5. Get all model aliases and check their accessibility
    let all_aliases = app_state.model_alias_store.get_all().map_err(|e| {
        error!("Failed to get model aliases from store: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to retrieve model alias list".to_string(),
        )
    })?;

    for alias in all_aliases {
        if !alias.is_enabled {
            continue;
        }

        // Find target model and provider
        if let Ok(Some(model)) = app_state.model_store.get_by_id(alias.target_model_id) {
            if !model.is_enabled {
                continue;
            }
            if let Ok(Some(provider)) = app_state.provider_store.get_by_id(model.provider_id) {
                if !provider.is_enabled {
                    continue;
                }

                let mut allowed = false;
                if let Some(ref policy) = access_control_policy_opt {
                    // Check policy against the target model
                    match LIMITER.check_limit_strategy(policy, provider.id, model.id) {
                        Ok(_) => {
                            allowed = true;
                        }
                        Err(reason) => {
                            debug!(
                                "Model alias '{}' (target: {}/{}) denied by policy '{}' for SystemApiKey ID {}. Reason: {}",
                                alias.alias_name, provider.provider_key, model.model_name, policy.name, system_api_key.id, reason
                            );
                        }
                    }
                } else {
                    // No policy, allowed by default
                    allowed = true;
                }

                if allowed {
                    available_models.push(AccessibleModel {
                        id: alias.alias_name.clone(),
                        owned_by: "cyder-api".to_string(),
                        provider_type: provider.provider_type.clone(),
                    });
                }
            }
        }
    }

    Ok(available_models)
}

// --- Structs for /models endpoint response ---
#[derive(Serialize, Debug)]
struct ModelListResponse {
    object: String,
    data: Vec<ModelInfo>,
}

#[derive(Serialize, Debug)]
struct ModelInfo {
    id: String, // model.model_name
    object: String,
    owned_by: String, // provider.provider_key
}


// --- Structs for Gemini /models endpoint response ---
#[derive(Serialize, Debug)]
struct GeminiModelListResponse {
    models: Vec<GeminiModelInfo>,
}

#[derive(Serialize, Debug)]
struct GeminiModelInfo {
    name: String,
}

// --- Unified Handler for listing models ---
async fn list_models_handler(
    app_state: Arc<AppState>,
    params: HashMap<String, String>,
    request: Request<Body>,
    api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    // 1. Authenticate based on api_type
    let original_headers = request.headers().clone();
    let api_key_check_result = match api_type {
        LlmApiType::OpenAI => authenticate_openai_request(&original_headers, &params, &app_state).await?,
        LlmApiType::Gemini => authenticate_gemini_request(&original_headers, &params, &app_state)?,
        LlmApiType::Anthropic => authenticate_anthropic_request(&original_headers, &app_state)?,
        _ => return Err((StatusCode::INTERNAL_SERVER_ERROR, "unsupported api type".to_string()))
    };
    let system_api_key = api_key_check_result.api_key;

    // 2. Get all accessible models
    let all_accessible_models = get_accessible_models(&app_state, &system_api_key).await?;

    // 3. Format response based on api_type
    let response = match api_type {
        LlmApiType::OpenAI | LlmApiType::Anthropic => {
            let mut available_models: Vec<ModelInfo> = all_accessible_models
                .into_iter()
                .map(|m| ModelInfo {
                    id: m.id,
                    object: "model".to_string(),
                    owned_by: m.owned_by,
                })
                .collect();

            available_models
                .sort_by(|a, b| a.owned_by.cmp(&b.owned_by).then_with(|| a.id.cmp(&b.id)));

            let response_data = ModelListResponse {
                object: "list".to_string(),
                data: available_models,
            };
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&response_data).unwrap()))
                .unwrap()
        }
        LlmApiType::Gemini => {
            let mut available_models: Vec<GeminiModelInfo> = all_accessible_models
                .into_iter()
                .map(|m| GeminiModelInfo { name: m.id })
                .collect();

            available_models.sort_by(|a, b| a.name.cmp(&b.name));

            let response_data = GeminiModelListResponse {
                models: available_models,
            };
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&response_data).unwrap()))
                .unwrap()
        }
        _ => return Err((StatusCode::INTERNAL_SERVER_ERROR, "unsupported api type".to_string()))
    };
    Ok(response)
}

// --- Structs and Handler for Gemini endpoint ---
