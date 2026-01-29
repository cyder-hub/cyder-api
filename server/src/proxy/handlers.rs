use super::auth::*;
use super::core::build_reqwest_client;
use super::logging::{create_request_log, log_final_update};
use super::models::{
    get_accessible_models, GeminiModelInfo, GeminiModelListResponse, ModelInfo, ModelListResponse,
};
use super::prepare::*;
use super::util::*;

use crate::controller::llm_types::LlmApiType;
use crate::schema::enum_def::{ProviderType, RequestStatus};
use crate::service::app_state::AppState;
use crate::service::cache::types::{CacheProvider, CacheModel};
use axum::{
    body::{Body, Bytes},
    extract::Request,
    response::Response,
};
use chrono::Utc;
use cyder_tools::log::{debug, error, info, warn};
use flate2::read::GzDecoder;
use reqwest::{
    header::{CONTENT_ENCODING, CONTENT_TYPE},
    Method, StatusCode,
};
use serde_json::Value;
use std::{collections::HashMap, io::Read, net::SocketAddr, sync::Arc};

// A generic handler for non-streaming OpenAI-compatible endpoints like embeddings and rerank.
pub async fn openai_utility_handler(
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

    info!(
        "Processing utility request ({}) for model: {}",
        downstream_path, pre_model_str
    );

    let (provider, model): (Arc<CacheProvider>, Arc<CacheModel>) = get_provider_and_model(&app_state, pre_model_str).await.map_err(|e: String| {
        warn!("Failed to resolve model '{}': {}", pre_model_str, e);
        (StatusCode::BAD_REQUEST, e)
    })?;

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
            format!(
                "'{}' is only supported for OpenAI-compatible providers.",
                downstream_path
            ),
        ));
    }

    // Step 5: Pricing info
    let billing_plan = get_pricing_info(&model, &app_state).await;

    // Step 6: Access control
    if let Err(e) = check_access_control(&system_api_key, &provider, &model, &app_state).await {
        warn!("Access control check failed: {:?}", e);
        return Err(e);
    }

    // Step 7: Prepare downstream request
    let (final_url, final_headers, final_body, provider_api_key_id) = prepare_llm_request(
        &provider,
        &model,
        data,
        &original_headers,
        &app_state,
        downstream_path,
    )
    .await
    .map_err(|e| {
        error!("Failed to prepare LLM request: {:?}", e);
        e
    })?;

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
                        error!(
                            "Gzip decoding failed for {} request: {}",
                            downstream_path, e
                        );
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

        log_final_update(
            log_id,
            "Non-SSE success",
            &final_url,
            &final_body,
            Some(status_code),
            Some(None),
            false,
            None,
            llm_response_completed_at,
            parsed_usage_info.as_ref(),
            billing_plan.as_ref(),
            Some(RequestStatus::Success),
        );
        info!(
            "{}: Non-SSE request completed for log_id {}.",
            model_str, log_id
        );
    } else {
        let error_body_str = String::from_utf8_lossy(&body_bytes).into_owned();
        error!(
            "[{}] LLM request failed with status {}: {}",
            downstream_path, status_code, &error_body_str
        );
    }

    // Build response to client, forwarding original headers and body
    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        response_builder = response_builder.header(name, value);
    }

    Ok(response_builder.body(Body::from(body_bytes)).unwrap())
}

// --- Unified Handler for listing models ---
pub async fn list_models_handler(
    app_state: Arc<AppState>,
    params: HashMap<String, String>,
    request: Request<Body>,
    api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    info!("Listing models for api_type: {:?}", api_type);

    // 1. Authenticate based on api_type
    let original_headers = request.headers().clone();
    let api_key_check_result = match api_type {
        LlmApiType::OpenAI => {
            authenticate_openai_request(&original_headers, &params, &app_state).await?
        }
        LlmApiType::Gemini => authenticate_gemini_request(&original_headers, &params, &app_state).await?,
        LlmApiType::Anthropic => authenticate_anthropic_request(&original_headers, &app_state).await?,
        _ => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "unsupported api type".to_string(),
            ))
        }
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
        _ => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "unsupported api type".to_string(),
            ))
        }
    };
    Ok(response)
}
