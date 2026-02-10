use super::auth::*;
use super::core::proxy_request;
use super::logging::RequestLogContext;
use super::prepare::*;
use super::util::*;

use crate::schema::enum_def::LlmApiType;
use crate::schema::enum_def::ProviderType;
use crate::service::app_state::AppState;
use crate::service::cache::types::{CacheModel, CacheProvider};
use crate::service::transform::transform_request_data;
use axum::{body::Body, extract::Request, response::Response};
use bytes::Bytes;
use chrono::Utc;
use cyder_tools::log::{debug, error, info, warn};
use reqwest::StatusCode;
use serde_json::Value;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

pub async fn handle_ollama_request(
    app_state: Arc<AppState>,
    addr: SocketAddr,
    query_params: HashMap<String, String>,
    request: Request<Body>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let client_ip_addr = Some(addr.ip().to_string());
    let start_time = Utc::now().timestamp_millis();
    let request_uri_path = request.uri().path().to_string();
    let original_headers = request.headers().clone();

    debug!("{} --- {:?}", &request_uri_path, query_params);

    // Step 1: Authenticate the request and retrieve API key.
    let api_key_check_result =
        authenticate_ollama_request(&original_headers, &query_params, &app_state).await?;
    let system_api_key = api_key_check_result.api_key;

    // Step 2: Parse the incoming request body.
    let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Failed to read request body: {}", e),
            )
        })?;
    let mut data: Value = serde_json::from_slice(&body_bytes).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Failed to parse request body: {}", e),
        )
    })?;
    let original_request_value = data.clone();
    let original_request_body = body_bytes;
    debug!(
        "[proxy] original request data: {}",
        serde_json::to_string(&data).unwrap_or_default()
    );

    // Step 3: Determine the provider and model.
    let pre_model_str = data
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "'model' field must be a string".to_string(),
            )
        })?;

    info!("Processing Ollama request for model: {}", pre_model_str);

    let (provider, model): (Arc<CacheProvider>, Arc<CacheModel>) =
        get_provider_and_model(&app_state, pre_model_str)
            .await
            .map_err(|e: String| {
                warn!("Failed to resolve model '{}': {}", pre_model_str, e);
                (StatusCode::BAD_REQUEST, e)
            })?;

    let target_api_type = if provider.provider_type == ProviderType::Ollama {
        LlmApiType::Ollama
    } else {
        LlmApiType::Openai
    };

    let is_stream = data.get("stream").and_then(Value::as_bool).unwrap_or(false);

    let api_type = LlmApiType::Ollama;
    data = transform_request_data(data, api_type, target_api_type, is_stream);

    let billing_plan = get_pricing_info(&model, &app_state).await;

    // Step 4: If an access policy is present, check if the request is allowed.
    if let Err(e) = check_access_control(&system_api_key, &provider, &model, &app_state).await {
        warn!("Access control check failed: {:?}", e);
        return Err(e);
    }

    // Step 5: Prepare the downstream request details (URL, headers, body).
    let (final_url, final_headers, final_body_value, provider_api_key_id) = match target_api_type {
        LlmApiType::Ollama => {
            prepare_llm_request(
                &provider,
                &model,
                data,
                &original_headers,
                &app_state,
                "api/chat",
            )
            .await
            .map_err(|e| {
                error!("Failed to prepare Ollama LLM request: {:?}", e);
                e
            })?
        }
        LlmApiType::Openai => {
            prepare_llm_request(
                &provider,
                &model,
                data,
                &original_headers,
                &app_state,
                "chat/completions",
            )
            .await
            .map_err(|e| {
                error!("Failed to prepare OpenAI LLM request: {:?}", e);
                e
            })?
        }
        _ => {
            error!("Unsupported target API type: {:?}", target_api_type);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "unsupported api type".to_string(),
            ));
        }
    };

    let final_body = Bytes::from(serde_json::to_vec(&final_body_value).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize final request body: {}", e),
        )
    })?);

    // Step 6: Create an initial log entry for the request.
    let llm_request_body_for_log = calculate_llm_request_body_for_log(
        api_type,
        target_api_type,
        &original_request_value,
        &final_body_value,
        &final_body,
    )?;

    let mut log_context = RequestLogContext::new(
        &system_api_key,
        &provider,
        &model,
        provider_api_key_id,
        start_time,
        &client_ip_addr,
    );
    log_context.llm_request_body = llm_request_body_for_log;
    log_context.user_request_body = Some(original_request_body);

    // Step 7: Execute the request against the downstream LLM service.
    let model_str = format_model_str(&provider, &model);

    proxy_request(
        log_context,
        final_url,
        final_body,
        final_headers,
        model_str,
        provider.use_proxy,
        billing_plan,
        api_type,
        target_api_type,
    )
    .await
}
