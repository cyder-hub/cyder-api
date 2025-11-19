use super::auth::*;
use super::core::{proxy_request, simple_proxy_request};
use super::logging::create_request_log;
use super::prepare::*;
use super::util::*;

use crate::controller::llm_types::LlmApiType;
use crate::schema::enum_def::ProviderType;
use crate::service::app_state::AppState;
use crate::service::transform::transform_request_data;
use axum::{body::Body, extract::Request, response::Response};
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
        authenticate_gemini_request(&original_headers, &query_params, &app_state)?;
    let system_api_key = api_key_check_result.api_key;
    let channel = api_key_check_result.channel;
    let external_id = api_key_check_result.external_id;

    // Step 2: Parse the incoming request body.
    let mut data = parse_request_body(request).await?;
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
        let (provider, model) =
            get_provider_and_model(&app_state, model_name).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

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

        return simple_proxy_request(final_url, final_body, final_headers, provider.use_proxy).await;
    }

    if !GEMINI_GENERATION_ACTIONS.contains(&action) {
        let err_msg = format!(
            "Invalid action: '{}'. Must be one of 'generateContent', 'streamGenerateContent', 'countMessageTokens', 'countTextTokens', or 'countTokens'.",
            action
        );
        error!("[gemini_models_handler] {}", err_msg);
        return Err((StatusCode::BAD_REQUEST, err_msg));
    }

    let (provider, model) =
        get_provider_and_model(&app_state, model_name).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let target_api_type = if provider.provider_type == ProviderType::Vertex
        || provider.provider_type == ProviderType::Gemini
    {
        LlmApiType::Gemini
    } else {
        LlmApiType::OpenAI
    };

    let api_type = LlmApiType::Gemini;
    let is_stream = action == "streamGenerateContent";

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
