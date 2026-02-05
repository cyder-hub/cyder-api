use super::auth::*;
use super::core::{proxy_request, simple_proxy_request};
use super::logging::RequestLogContext;
use super::prepare::*;
use super::util::*;

use crate::schema::enum_def::LlmApiType;
use crate::schema::enum_def::ProviderType;
use crate::service::app_state::AppState;
use crate::service::transform::transform_request_data;
use axum::{body::Body, extract::Request, response::Response};
use bytes::Bytes;
use chrono::Utc;
use cyder_tools::log::{debug, error};
use reqwest::StatusCode;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

pub async fn handle_gemini_request(
    app_state: Arc<AppState>,
    addr: SocketAddr,
    path_segment: String,
    query_params: HashMap<String, String>,
    request: Request<Body>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let client_ip_addr = Some(addr.ip().to_string());
    let start_time = Utc::now().timestamp_millis();
    let request_uri_path = request.uri().path().to_string();
    let original_headers = request.headers().clone();

    debug!("{} --- {:?}", &request_uri_path, &query_params);

    // Step 1: Authenticate the request and retrieve API key.
    let api_key_check_result =
        authenticate_gemini_request(&original_headers, &query_params, &app_state).await?;
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
    let mut data: serde_json::Value = serde_json::from_slice(&body_bytes).map_err(|e| {
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
    let model_action_segment = &path_segment;
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

    const GEMINI_GENERATION_ACTIONS: [&str; 2] = ["generateContent", "streamGenerateContent"];
    const GEMINI_UTILITY_ACTIONS: [&str; 3] =
        ["countMessageTokens", "countTextTokens", "countTokens"];

    if GEMINI_UTILITY_ACTIONS.contains(&action) {
        // Handle utility actions: simple proxy, no logging
        let (provider, model) = get_provider_and_model(&app_state, model_name)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

        let target_api_type =
            if provider.provider_type == ProviderType::Vertex || provider.provider_type == ProviderType::Gemini {
                LlmApiType::Gemini
            } else {
                LlmApiType::Openai
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

        let (final_url, final_headers, _) = prepare_simple_gemini_request(
            &provider,
            &model,
            &original_headers,
            &app_state,
            action,
            &query_params,
        )
        .await?;

        let final_body = serde_json::to_string(&data).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize final request body: {}", e),
            )
        })?;

        return simple_proxy_request(final_url, final_body, final_headers, provider.use_proxy)
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
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let target_api_type =
        if provider.provider_type == ProviderType::Vertex || provider.provider_type == ProviderType::Gemini {
            LlmApiType::Gemini
        } else {
            LlmApiType::Openai
        };

    let api_type = LlmApiType::Gemini;
    let is_stream = action == "streamGenerateContent";

    data = transform_request_data(data, api_type, target_api_type, is_stream);

    let billing_plan = get_pricing_info(&model, &app_state).await;

    // Step 4: If an access policy is present, check if the request is allowed.
    check_access_control(&system_api_key, &provider, &model, &app_state).await?;

    let (final_url, final_headers, final_body_value, provider_api_key_id) = match target_api_type {
        LlmApiType::Openai => {
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
            prepare_gemini_llm_request(
                &provider,
                &model,
                data,
                &original_headers,
                &app_state,
                is_stream,
                &query_params,
            )
            .await?
        }
        _ => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "unsupported api type".to_string(),
            ))
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
    log_context.llm_request_body = Some(llm_request_body_for_log);
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
