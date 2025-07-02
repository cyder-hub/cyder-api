use cyder_tools::log::{debug, error, info}; // Removed info as it's re-imported if needed by other macros
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
use axum::Json;
use crate::service::{
    app_state::{create_state_router, StateRouter, AppState, StateStore, SystemApiKeyStore, AppStoreError, GroupItemSelectionStrategy}, // Added AppState, StateStore, AppStoreError
    vertex::get_vertex_token,
};
use bytes::BytesMut;
use chrono::Utc;
use flate2::read::GzDecoder;
use futures::StreamExt;
use reqwest::{
    header::{
        ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HOST,
    },
    Method, Proxy, StatusCode, Url,
};
use serde::{Deserialize, Serialize}; // Added Serialize and Deserialize
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
        request_log::{NewRequestLog, RequestLog, RequestLogStatus, UpdateRequestLogData},
        system_api_key::SystemApiKey,
    },
    utils::{limit::LIMITER, ID_GENERATOR}, // Added LIMITER
                                                                     // utils::id::generate_snowflake_id, // TODO: Ensure this utility exists or implement it
};
use crate::{
    config::CONFIG,
    utils::{process_stream_options, split_chunks},
};

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

#[derive(Debug)]
struct UsageInfo {
    prompt_tokens: i32,
    completion_tokens: i32,
    reasoning_tokens: i32,
    total_tokens: i32,
}

fn parse_usage_info(usage_val: Option<&Value>) -> Option<UsageInfo> {
    if let Some(usage) = usage_val {
        if usage.is_null() {
            return None;
        }

        let prompt_tokens = usage
            .get("prompt_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32;
        let completion_tokens = usage
            .get("completion_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32;
        let total_tokens = usage
            .get("total_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32;

        let reasoning_tokens = usage
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_i64)
            .map(|rt| rt as i32)
            .unwrap_or_else(|| {
                let calculated_reasoning = total_tokens - prompt_tokens - completion_tokens;
                if calculated_reasoning < 0 {
                    0
                } else {
                    calculated_reasoning
                }
            });

        Some(UsageInfo {
            prompt_tokens,
            completion_tokens,
            reasoning_tokens,
            total_tokens,
        })
    } else {
        None
    }
}

/// Calculates the total cost of a request based on token usage and a set of price rules.
///
/// It finds the best-matching active price rule for each usage type ('PROMPT', 'COMPLETION', 'INVOCATION')
/// and calculates the cost. "Best" is the rule with the most recent `effective_from` date.
///
/// # Arguments
///
/// * `usage_info` - A reference to the `UsageInfo` struct containing token counts.
/// * `price_rules` - A slice of `PriceRule`s applicable to the request.
///
/// # Returns
///
/// The total calculated cost in micro-units.
fn calculate_cost(usage_info: &UsageInfo, price_rules: &[PriceRule]) -> i64 {
    debug!("[calculate_cost] Calculating cost for usage: {:?}, with price rules: {:?}", usage_info, price_rules);
    let now = Utc::now().timestamp_millis();
    let mut total_cost: i64 = 0;

    // Helper to find the best rule for a given usage type.
    // "Best" is defined as the one that is currently active and has the latest `effective_from` date.
    let find_best_rule = |usage_type: &str| -> Option<&PriceRule> {
        price_rules
            .iter()
            .filter(|rule| {
                rule.usage_type == usage_type
                    && rule.is_enabled
                    && rule.effective_from <= now
                    && rule.effective_until.map_or(true, |until| now < until)
            })
            .max_by_key(|rule| rule.effective_from)
    };

    // Calculate cost for prompt tokens
    if let Some(rule) = find_best_rule("PROMPT") {
        if usage_info.prompt_tokens > 0 {
            // Price is per 1000 tokens
            let cost = usage_info.prompt_tokens as i64 * rule.price_in_micro_units;
            total_cost += cost;
            debug!("[calculate_cost] Applied PROMPT rule: {:?}. Cost added: {}. Current total cost: {}", rule, cost, total_cost);
        }
    }

    // Calculate cost for completion tokens
    if let Some(rule) = find_best_rule("COMPLETION") {
        if usage_info.completion_tokens > 0 {
            // Price is per 1000 tokens
            let cost = usage_info.completion_tokens as i64 * rule.price_in_micro_units;
            total_cost += cost;
            debug!("[calculate_cost] Applied COMPLETION rule: {:?}. Cost added: {}. Current total cost: {}", rule, cost, total_cost);
        }
    }

    // Calculate cost for invocation (flat fee)
    if let Some(rule) = find_best_rule("INVOCATION") {
        // Invocation is a flat fee, not token-based.
        total_cost += rule.price_in_micro_units;
        debug!("[calculate_cost] Applied INVOCATION rule: {:?}. Cost added: {}. Current total cost: {}", rule, rule.price_in_micro_units, total_cost);
    }

    debug!("[calculate_cost] Final calculated cost: {}", total_cost);
    total_cost
}


// Helper function to decompress data if it's gzipped
fn decompress_if_gzipped(bytes_mut: &BytesMut, is_gzip: bool, log_id_for_error: i64) -> Bytes {
    if is_gzip {
        if bytes_mut.is_empty() {
            Bytes::new()
        } else {
            let mut gz = GzDecoder::new(&bytes_mut[..]);
            let mut decompressed_data = Vec::new();
            match gz.read_to_end(&mut decompressed_data) {
                Ok(_) => Bytes::from(decompressed_data),
                Err(e) => {
                    error!("Gzip decoding failed for log_id {}: {}", log_id_for_error, e);
                    bytes_mut.clone().freeze() // return original if decode fails
                }
            }
        }
    } else {
        bytes_mut.clone().freeze()
    }
}

// Helper function to populate token and cost fields in UpdateRequestLogData
fn populate_token_cost_fields(
    update_data: &mut UpdateRequestLogData,
    usage_info: Option<&UsageInfo>,
    price_rules: &[PriceRule],
    currency: Option<&str>,
) {
    debug!("[populate_token_cost_fields] Populating with usage_info: {:?}, price_rules: {:?}", usage_info, price_rules);
    if let Some(u) = usage_info {
        update_data.prompt_tokens = Some(u.prompt_tokens);
        update_data.completion_tokens = Some(u.completion_tokens);
        update_data.reasoning_tokens = Some(u.reasoning_tokens);
        update_data.total_tokens = Some(u.total_tokens);

        if !price_rules.is_empty() {
            let cost = calculate_cost(u, price_rules);
            update_data.calculated_cost = Some(cost);
            if cost > 0 {
                update_data.cost_currency = currency.map(|c| Some(c.to_string()));
            }
        }
    }
    debug!("[populate_token_cost_fields] Resulting update_data: {:?}", update_data);
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
    overall_status: Option<RequestLogStatus>,
) {
    let is_error = overall_status == Some(RequestLogStatus::ERROR);

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
        status: overall_status.map(|s| s.to_string()),
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

// Authenticates the request and fetches the associated access control policy.
async fn authenticate_and_authorize(
    headers: &HeaderMap,
    app_state: &Arc<AppState>,
) -> Result<(ApiKeyCheckResult, Option<ApiAccessControlPolicy>), (StatusCode, String)> {
    // 1. Get system_api_key from header and check it
    let system_api_key_str =
        parse_token_from_request(headers).map_err(|err| (StatusCode::UNAUTHORIZED, err))?;
    let api_key_check_result =
        check_system_api_key(&app_state.system_api_key_store, &system_api_key_str)
            .map_err(|err_msg| (StatusCode::UNAUTHORIZED, err_msg))?;

    // 2. Fetch Access Control Policy if ID is present on the SystemApiKey
    if let Some(policy_id) = api_key_check_result.api_key.access_control_policy_id {
        match app_state.access_control_store.get_by_id(policy_id) {
            Ok(Some(policy)) => Ok((api_key_check_result, Some(policy))),
            Ok(None) => {
                let err_msg = format!(
                    "Access control policy id {} configured but not found in application cache.",
                    policy_id
                );
                error!(
                    "{}, SystemApiKey ID: {}",
                    err_msg, api_key_check_result.api_key.id
                );
                Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg))
            }
            Err(store_err) => {
                let err_msg = format!(
                    "Error accessing application cache for access control policy id {}: {}",
                    policy_id, store_err
                );
                error!(
                    "{}, SystemApiKey ID: {}",
                    err_msg, api_key_check_result.api_key.id
                );
                Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg))
            }
        }
    } else {
        Ok((api_key_check_result, None))
    }
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
    let request_api_key = if provider.provider_type == "VERTEX_OPENAI" {
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
    let path = "chat/completions";
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

// Handles the response from the LLM, including streaming, logging, and error handling.
async fn handle_llm_response(
    log_id: i64,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    data: &str,
    price_rules: Vec<PriceRule>,
    currency: Option<String>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let response_headers = response.headers().clone();
    let is_sse = response_headers
        .get(CONTENT_TYPE)
        .map_or(false, |value| {
            value.to_str().unwrap_or("").contains("text/event-stream")
        });

    // Update the log with whether the response is a stream.
    if let Err(e) = RequestLog::update_request_with_completion_details(
        log_id,
        &UpdateRequestLogData {
            is_stream: Some(is_sse),
            ..Default::default()
        },
    ) {
        error!(
            "Failed to update request log (is_stream) for log_id {}: {:?}",
            log_id, e
        );
    }

    let is_gzip = response_headers
        .get(CONTENT_ENCODING)
        .map_or(false, |value| value.to_str().unwrap_or("").contains("gzip"));
    let status_code = response.status();

    // Build the response to the client, forwarding headers from the LLM response.
    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        if name != CONTENT_LENGTH {
            response_builder = response_builder.header(name, value);
        }
    }

    let mut first_chunk_received_at_proxy: i64 = 0;
    let mut total_bytes_mut = BytesMut::new();
    let latest_chunk_arc: Arc<Mutex<Option<Bytes>>> = Arc::new(Mutex::new(None));
    let (tx, mut rx) = mpsc::channel::<Result<bytes::Bytes, reqwest::Error>>(10);

    // Clone url and data to be moved into the async stream
    let url_owned = url.to_string();
    let data_owned = data.to_string();

    // Spawn a task to pull chunks from the LLM response stream and send them to a channel.
    // This decouples receiving from processing.
    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        while let Some(chunk_result) = stream.next().await {
            if tx.send(chunk_result).await.is_err() {
                // Receiver has been dropped, so we can stop.
                break;
            }
        }
    });

    // Create a new stream that processes chunks as they are received from the channel.
    let monitored_stream = async_stream::stream! {
        while let Some(chunk_result) = rx.recv().await {
            match chunk_result {
                Ok(chunk) => {
                    if first_chunk_received_at_proxy == 0 {
                        first_chunk_received_at_proxy = Utc::now().timestamp_millis();
                    }

                    if is_sse {
                        let multi_chunks = split_chunks(chunk.slice(..));
                        for current_sub_chunk in multi_chunks {
                            let line_str = String::from_utf8_lossy(&current_sub_chunk);
                            if line_str.trim() == "data: [DONE]" {
                                if let Some(final_data_chunk) = latest_chunk_arc.lock().unwrap().take() {
                                    let final_line_str = String::from_utf8_lossy(&final_data_chunk);
                                    if let Some(data_json_str) = final_line_str.strip_prefix("data:").map(str::trim) {
                                        if let Ok(data_value) = serde_json::from_str::<Value>(data_json_str) {
                                            debug!("[proxy] final response {:?}", data_value);
                                            let usage_json = data_value.get("usage");
                                            let parsed_usage_info = parse_usage_info(usage_json);
                                            let completed_at = Utc::now().timestamp_millis();
                                            let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };

                                            log_final_update(
                                                log_id, "SSE DONE", &url_owned, &data_owned, Some(status_code),
                                                Some(None), true, first_chunk_ts, completed_at,
                                                parsed_usage_info.as_ref(), &price_rules, currency.as_deref(), Some(RequestLogStatus::SUCCESS),
                                            );
                                            info!("{}: SSE stream completed.", model_str);
                                        } else {
                                            error!("Failed to parse final SSE data JSON for log_id {}: {}", log_id, data_json_str);
                                        }
                                    }
                                }
                            } else {
                                *latest_chunk_arc.lock().unwrap() = Some(current_sub_chunk);
                            }
                        }
                    } else { // Not SSE
                        total_bytes_mut.extend_from_slice(&chunk);
                    }
                    yield Ok::<_, std::io::Error>(chunk); // Forward the original chunk to the client
                }
                Err(e) => { // An error occurred while streaming from the LLM
                    let stream_error_message = format!("LLM stream error: {}", e);
                    error!("{}", stream_error_message);
                    let completed_at = Utc::now().timestamp_millis();
                    let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };

                    log_final_update(
                        log_id, "LLM stream error", &url_owned, &data_owned, Some(status_code),
                        Some(None), is_sse, first_chunk_ts, completed_at, None, &price_rules, currency.as_deref(), Some(RequestLogStatus::ERROR),
                    );
                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, stream_error_message));
                    break;
                }
            }
        }

        // This block executes after the stream loop finishes.
        let llm_response_completed_at = Utc::now().timestamp_millis();

        if status_code.is_success() {
            if !is_sse { // Non-SSE success, log here (SSE success is logged at "[DONE]")
                let final_body_bytes = decompress_if_gzipped(&total_bytes_mut, is_gzip, log_id);
                let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };
                let parsed_usage_info = serde_json::from_slice::<Value>(&final_body_bytes)
                    .ok()
                    .and_then(|val| parse_usage_info(val.get("usage")));

                log_final_update(
                    log_id, "Non-SSE success", &url_owned, &data_owned, Some(status_code),
                    Some(None), false, first_chunk_ts, llm_response_completed_at,
                    parsed_usage_info.as_ref(), &price_rules, currency.as_deref(), Some(RequestLogStatus::SUCCESS),
                );
                info!("{}: Non-SSE request completed for log_id {}.", model_str, log_id);
            }
        } else { // LLM returned a non-2xx status code
            let final_body_bytes = decompress_if_gzipped(&total_bytes_mut, is_gzip, log_id);
            let error_body_str = String::from_utf8_lossy(&final_body_bytes).into_owned();
            let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };

            error!("LLM request failed with status {} for log_id {}: {}", status_code, log_id, error_body_str);

            log_final_update(
                log_id, "LLM error status", &url_owned, &data_owned, Some(status_code),
                Some(Some(error_body_str)), is_sse, first_chunk_ts,
                llm_response_completed_at, None, &price_rules, currency.as_deref(), Some(RequestLogStatus::ERROR),
            );
        }
    };

    // Build the final response to the client, streaming the body from our monitored stream.
    match response_builder.body(Body::from_stream(monitored_stream)) {
        Ok(final_response) => Ok(final_response),
        Err(e) => {
            let error_message = format!("Failed to build client response for log_id {}: {}", log_id, e);
            error!("{}", error_message);
            let completed_at = Utc::now().timestamp_millis();
            let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };

            log_final_update(
                log_id, "Response build error", url, data, None, Some(None),
                is_sse, first_chunk_ts, completed_at, None, &[], None, Some(RequestLogStatus::ERROR),
            );
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
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
) -> Result<Response<Body>, (StatusCode, String)> {
    // 1. Build HTTP client, with proxy if configured
    let client = if use_proxy {
        let proxy = Proxy::https(&CONFIG.proxy.url).unwrap();
        reqwest::Client::builder().proxy(proxy).build().unwrap()
    } else {
        reqwest::Client::new()
    };

    debug!(
        "[proxy] proxy request header: {:?}",
        serialize_reqwest_headers(&headers)
    );
    debug!("[proxy] proxy request data: {:?}", &data);

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
                Some(RequestLogStatus::ERROR),
            );
            return Err((StatusCode::BAD_GATEWAY, error_message));
        }
    };

    // 3. Process the response stream
    handle_llm_response(log_id, model_str, response, &url, &data, price_rules, currency).await
}

fn parse_provider_model(pm: &str) -> (&str, &str) {
    let mut parts = pm.splitn(2, '/');
    let provider = parts.next().unwrap_or("");
    let model_id = parts.next().unwrap_or("");
    (provider, model_id)
}

fn handle_custom_fields(
    data: &mut Value,    // For "BODY"
    url: &mut Url,       // For "QUERY"
    headers: &mut HeaderMap, // For "HEADER" (reqwest::header::HeaderMap)
    custom_fields: &Vec<CustomFieldDefinition>,
) {
    for cf in custom_fields {
        match cf.field_placement.as_str() {
            "BODY" => {
                match cf.field_type.as_str() {
                    "UNSET" => {
                        data.as_object_mut().map(|obj| {
                            obj.remove(&cf.field_name);
                        });
                    }
                    "STRING" => {
                        if let Some(string_value) = &cf.string_value {
                            data.as_object_mut().map(|obj| {
                                obj.insert(cf.field_name.clone(), Value::String(string_value.clone()));
                            });
                        }
                    }
                    "INTEGER" => {
                        if let Some(int_value) = cf.integer_value {
                            data.as_object_mut().map(|obj| {
                                obj.insert(cf.field_name.clone(), Value::Number(int_value.into()));
                            });
                        }
                    }
                    "NUMBER" => {
                        if let Some(number_value) = cf.number_value {
                            data.as_object_mut().map(|obj| {
                                obj.insert(
                                    cf.field_name.clone(),
                                    serde_json::Number::from_f64(number_value as f64) // Removed dereference *
                                        .map(Value::Number)
                                        .unwrap_or(Value::Null),
                                );
                            });
                        }
                    }
                    "BOOLEAN" => {
                        if let Some(bool_value) = cf.boolean_value {
                            data.as_object_mut().map(|obj| {
                                obj.insert(cf.field_name.clone(), Value::Bool(bool_value)); // Removed dereference *
                            });
                        }
                    }
                    "JSON_STRING" => {
                        if let Some(json_string_value) = &cf.string_value {
                            match serde_json::from_str::<Value>(json_string_value) {
                                Ok(parsed_json_value) => {
                                    data.as_object_mut().map(|obj| {
                                        obj.insert(cf.field_name.clone(), parsed_json_value);
                                    });
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to parse JSON_STRING custom field '{}' for BODY: {}. Value: '{}'",
                                        cf.field_name, e, json_string_value
                                    );
                                }
                            }
                        }
                    }
                    _ => {
                        debug!(
                            "Unknown custom field type '{}' for field '{}' in BODY",
                            cf.field_type, cf.field_name
                        );
                    }
                }
            }
            "QUERY" => {
                let field_name_key = cf.field_name.clone();
                let mut new_value_opt: Option<String> = None;

                match cf.field_type.as_str() {
                    "UNSET" => { /* new_value_opt remains None, effectively removing */ }
                    "STRING" => { new_value_opt = cf.string_value.clone(); }
                    "INTEGER" => { new_value_opt = cf.integer_value.map(|v| v.to_string()); }
                    "NUMBER" => { new_value_opt = cf.number_value.map(|v| v.to_string()); }
                    "BOOLEAN" => { new_value_opt = cf.boolean_value.map(|v| v.to_string()); }
                    "JSON_STRING" => { new_value_opt = cf.string_value.clone(); } // JSON as string for query
                    _ => {
                        debug!(
                            "Unknown custom field type '{}' for field '{}' in QUERY",
                            cf.field_type, cf.field_name
                        );
                    }
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
            "HEADER" => {
                match cf.field_type.as_str() {
                    "UNSET" => {
                        headers.remove(&cf.field_name);
                    }
                    _ => { // For all other types, convert to string and set header
                        let value_str_opt: Option<String> = match cf.field_type.as_str() {
                            "STRING" => cf.string_value.clone(),
                            "INTEGER" => cf.integer_value.map(|v| v.to_string()),
                            "NUMBER" => cf.number_value.map(|v| v.to_string()),
                            "BOOLEAN" => cf.boolean_value.map(|v| v.to_string()),
                            "JSON_STRING" => cf.string_value.clone(), // JSON as string for header
                            _ => {
                                debug!(
                                    "Unknown custom field type '{}' for field '{}' in HEADER",
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
            _ => {
                error!(
                    "Unknown custom field placement '{}' for field '{}'",
                    cf.field_placement, cf.field_name
                );
            }
        }
    }
}

const BEARER_PREFIX: &str = "Bearer ";
fn parse_token_from_request(headers: &HeaderMap) -> Result<String, String> {
    let auth_header_value = headers
        .get(AUTHORIZATION)
        .ok_or_else(|| String::from("There is no authorization header"))?;

    let auth_str = auth_header_value
        .to_str()
        .map_err(|_| String::from("authorization invalid"))?;

    auth_str
        .strip_prefix(BEARER_PREFIX)
        .map(|token_slice| token_slice.to_string())
        .ok_or_else(|| String::from("authorization invalid"))
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

async fn proxy_all_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(app_state): State<Arc<AppState>>,
    request: Request<Body>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let client_ip_addr = Some(addr.ip().to_string());
    let start_time = Utc::now().timestamp_millis();
    let request_uri_path = request.uri().path().to_string();
    let original_headers = request.headers().clone();

    // Step 1: Authenticate the request and retrieve API key and any associated access policy.
    let (api_key_check_result, access_control_policy) =
        authenticate_and_authorize(&original_headers, &app_state).await?;
    let system_api_key = api_key_check_result.api_key;
    let channel = api_key_check_result.channel;
    let external_id = api_key_check_result.external_id;

    // Step 2: Parse the incoming request body.
    let mut data = parse_request_body(request).await?;
    debug!("[proxy] original request data: {:?}", data);

    process_stream_options(&mut data);

    // Step 3: Determine the provider and model from the 'model' field in the request.
    // This can be an alias or in 'provider/model' format.
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

    let (price_rules, currency) = if let Some(plan_id) = model.billing_plan_id {
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

        // Fetch the billing plan to get the currency.
        // This assumes `billing_plan_store` is available on `app_state`.
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
    };

    // Step 4: If an access policy is present, check if the request is allowed.
    if let Some(ref policy) = access_control_policy {
        if let Err(reason) = LIMITER.check_limit_strategy(policy, provider.id, model.id) {
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

    // Step 5: Prepare the downstream request details (URL, headers, body).
    // This includes selecting a provider API key, applying custom fields, and setting the final model name.
    let (final_url, final_headers, final_body, provider_api_key_id) =
        prepare_llm_request(&provider, &model, data, &original_headers, &app_state).await?;

    // Step 6: Create an initial log entry for the request.
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
        client_ip: client_ip_addr,
        external_request_uri: Some(request_uri_path),
        status: RequestLogStatus::PENDING.to_string(),
        llm_request_sent_at: Utc::now().timestamp_millis(),
        created_at: start_time,
        updated_at: start_time,
        channel,
        external_id,
    };

    if let Err(e) = RequestLog::create_initial_request(&initial_log_data) {
        error!(
            "Failed to create initial request log for log_id {}: {:?}",
            log_id, e
        );
        // Proceeding even if logging fails, but this could be changed to return an error.
    }

    // Step 7: Execute the request against the downstream LLM service.
    let model_str = if model.model_name == *real_model_name {
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
    )
    .await
}

pub fn create_proxy_router() -> StateRouter {
    create_state_router()
        .nest(
            "/openai",
            create_state_router()
                .route("/chat/completions", any(proxy_all_handler))
                .route("/models", get(list_available_models_handler)),
        )
        .route(
            "/gemini/v1beta/models/{:model_action_segment}", // New Gemini route
            any(gemini_models_handler), // Maps to the new handler for any method
        )
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

// --- Handler for /models endpoint ---
async fn list_available_models_handler(
    State(app_state): State<Arc<AppState>>, // Added AppState
    request: Request<Body>,
) -> Result<Json<ModelListResponse>, (StatusCode, String)> {
    // 1. Authenticate and get SystemApiKey
    let original_headers_map = request.headers().clone();
    let system_api_key_str = parse_token_from_request(&original_headers_map)
        .map_err(|err| (StatusCode::UNAUTHORIZED, err))?;
    let api_key_check_result =
        check_system_api_key(&app_state.system_api_key_store, &system_api_key_str)
            .map_err(|err_msg| (StatusCode::UNAUTHORIZED, err_msg))?;
    let system_api_key = api_key_check_result.api_key;

    // 2. Fetch Access Control Policy if ID is present
    let access_control_policy_opt: Option<ApiAccessControlPolicy> =
        if let Some(policy_id) = system_api_key.access_control_policy_id {
            match app_state.access_control_store.get_by_id(policy_id) {
                Ok(Some(policy)) => Some(policy),
                Ok(None) => {
                    error!("Access control policy with id {} not found in store (configured on SystemApiKey {}).", policy_id, system_api_key.id);
                    // If a policy_id is configured but not found, it's an internal issue.
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

    let mut available_models: Vec<ModelInfo> = Vec::new();

    // 3. Get all active providers
    let active_providers = Provider::list_all_active().map_err(|e| {
        error!("Failed to list active providers: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to retrieve provider list".to_string(),
        )
    })?;

    for provider in active_providers {
        // 4. Get all active models for this provider
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
                // 5a. Check against policy if one is loaded
                match LIMITER.check_limit_strategy(policy, provider.id, model.id) {
                    Ok(_) => {
                        allowed = true;
                        debug!(
                            "Model {}/{} allowed by policy '{}' for SystemApiKey ID {}",
                            provider.provider_key, model.model_name, policy.name, system_api_key.id
                        );
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
                        // Not allowed, do nothing
                    }
                }
            } else {
                // 5b. No policy loaded, model is allowed by default
                allowed = true;
                debug!(
                    "Model {}/{} allowed (no policy attached) for SystemApiKey ID {}",
                    provider.provider_key, model.model_name, system_api_key.id
                );
            }

            if allowed {
                available_models.push(ModelInfo {
                    id: format!("{}/{}", provider.provider_key, model.model_name), // Changed id format
                    object: "model".to_string(),
                    owned_by: provider.provider_key.clone(),
                });
            }
        }
    }

    // 6. Get all model aliases and check their accessibility
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
                            debug!(
                                "Model alias '{}' (target: {}/{}) allowed by policy '{}' for SystemApiKey ID {}",
                                alias.alias_name, provider.provider_key, model.model_name, policy.name, system_api_key.id
                            );
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
                    debug!(
                        "Model alias '{}' (target: {}/{}) allowed (no policy attached) for SystemApiKey ID {}",
                        alias.alias_name, provider.provider_key, model.model_name, system_api_key.id
                    );
                }

                if allowed {
                    available_models.push(ModelInfo {
                        id: alias.alias_name.clone(),
                        object: "model".to_string(),
                        owned_by: "cyder-api".to_string(),
                    });
                }
            }
        }
    }

    // Sort by owned_by, then by id for consistent output
    available_models.sort_by(|a, b| {
        a.owned_by.cmp(&b.owned_by)
            .then_with(|| a.id.cmp(&b.id))
    });

    Ok(Json(ModelListResponse {
        object: "list".to_string(),
        data: available_models,
    }))
}

// --- Structs and Handler for Gemini endpoint ---
#[derive(Deserialize, Debug)]
struct GeminiHandlerQueryParams {
    key: Option<String>,
}

async fn gemini_models_handler(
    Path(model_action_segment): Path<String>,
    Query(params): Query<GeminiHandlerQueryParams>,
    // axum::extract::RequestParts might be useful if more request details are needed later
) -> Result<Response<Body>, (StatusCode, String)> {
    debug!(
        "[gemini_models_handler] Received model_action_segment: {}",
        model_action_segment
    );
    debug!("[gemini_models_handler] Received query params: {:?}", params);

    let parts: Vec<&str> = model_action_segment.splitn(2, ':').collect();
    if parts.len() != 2 {
        let err_msg = format!(
            "Invalid model_action_segment format: '{}'. Expected 'model_name:action'.",
            model_action_segment
        );
        error!("[gemini_models_handler] {}", err_msg);
        return Err((StatusCode::BAD_REQUEST, err_msg));
    }
    let model_name = parts[0];
    let action = parts[1];

    if action != "generateContent" && action != "streamGenerateContent" {
        let err_msg = format!(
            "Invalid action: '{}'. Must be 'generateContent' or 'streamGenerateContent'.",
            action
        );
        error!("[gemini_models_handler] {}", err_msg);
        return Err((StatusCode::BAD_REQUEST, err_msg));
    }

    let system_api_key = match params.key {
        Some(k) => k,
        None => {
            let err_msg = "Missing 'key' query parameter for API authentication.".to_string();
            error!("[gemini_models_handler] {}", err_msg);
            return Err((StatusCode::UNAUTHORIZED, err_msg));
        }
    };

    debug!(
        "[gemini_models_handler] Model Name: '{}', Action: '{}', System API Key: '{}'",
        model_name, action, system_api_key
    );

    // Placeholder response as requested
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain")
        .body(Body::from(format!(
            "Debug Info: Model Name: '{}', Action: '{}', System API Key: '{}'",
            model_name, action, system_api_key
        )))
        .unwrap_or_else(|e| {
            error!("[gemini_models_handler] Failed to build response: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Error building response"))
                .unwrap()
        }))
}
