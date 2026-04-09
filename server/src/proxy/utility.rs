use std::{collections::HashMap, sync::Arc};

use axum::{
    body::{Body, Bytes},
    http::HeaderMap,
    response::Response,
};
use chrono::Utc;
use cyder_tools::log::{debug, error, info, warn};
use reqwest::{Method, header::CONTENT_ENCODING};

use super::{
    ProxyError,
    auth::check_access_control,
    cancellation::{CancellationDropGuard, ProxyCancellationContext},
    classify_reqwest_error, classify_upstream_status,
    core::{
        build_response_builder, decode_response_body, finalize_non_streaming_log_context,
        read_response_bytes_with_cancellation, send_with_first_byte_timeout,
    },
    governance::{
        ensure_provider_request_allowed, record_provider_failure, record_provider_success,
    },
    logging::{LoggedBody, RequestLogContext, get_log_manager},
    prepare::{get_provider_and_model, prepare_llm_request, prepare_simple_gemini_request},
    protocol_transform_error,
    request::ParsedProxyRequest,
    util::{
        determine_target_api_type, format_model_str, get_pricing_info, parse_utility_usage_info,
        serialize_reqwest_headers,
    },
};
use crate::{
    schema::enum_def::{LlmApiType, RequestStatus},
    service::{app_state::AppState, cache::types::CacheSystemApiKey},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UtilityProtocol {
    OpenaiCompatible,
    GeminiCompatible,
}

#[derive(Clone, Debug)]
pub(super) struct UtilityOperation {
    pub name: String,
    pub api_type: LlmApiType,
    pub protocol: UtilityProtocol,
    pub downstream_path: String,
}

pub(super) struct UtilityExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub system_api_key: Arc<CacheSystemApiKey>,
    pub operation: UtilityOperation,
    pub requested_model: String,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub parsed_request: ParsedProxyRequest,
}

fn validate_utility_target(
    operation: &UtilityOperation,
    target_api_type: LlmApiType,
) -> Result<(), ProxyError> {
    match (operation.protocol, target_api_type) {
        (UtilityProtocol::OpenaiCompatible, LlmApiType::Openai) => Ok(()),
        (UtilityProtocol::GeminiCompatible, LlmApiType::Gemini) => Ok(()),
        (UtilityProtocol::OpenaiCompatible, _) => Err(ProxyError::BadRequest(format!(
            "'{}' is only supported for OpenAI-compatible providers.",
            operation.name
        ))),
        (UtilityProtocol::GeminiCompatible, _) => Err(ProxyError::BadRequest(format!(
            "Action '{}' is only supported for Gemini-compatible providers.",
            operation.name
        ))),
    }
}

pub(super) async fn execute_utility_proxy(
    app_state: Arc<AppState>,
    input: UtilityExecutionInput,
) -> Result<Response<Body>, ProxyError> {
    let UtilityExecutionInput {
        cancellation,
        system_api_key,
        operation,
        requested_model,
        query_params,
        original_headers,
        client_ip_addr,
        start_time,
        parsed_request,
    } = input;
    let ParsedProxyRequest {
        data,
        original_request_body,
        ..
    } = parsed_request;

    info!(
        "Processing {:?} utility request ({}) for model: {}",
        operation.api_type, operation.name, requested_model
    );
    let mut cancellation_guard = CancellationDropGuard::new(
        cancellation.clone(),
        format!(
            "Client disconnected during utility operation '{}'.",
            operation.name
        ),
    );

    let (provider, model) = get_provider_and_model(&app_state, &requested_model)
        .await
        .map_err(|e| {
            warn!("Failed to resolve model '{}': {}", requested_model, e);
            ProxyError::BadRequest(e)
        })?;
    let target_api_type = determine_target_api_type(&provider);
    validate_utility_target(&operation, target_api_type)?;
    let billing_plan = get_pricing_info(&model, &app_state).await;

    check_access_control(&system_api_key, &provider, &model, &app_state)
        .await
        .map_err(|e| {
            warn!("Access control check failed: {:?}", e);
            e
        })?;

    let (final_url, final_headers, final_body, provider_api_key_id) = match operation.protocol {
        UtilityProtocol::OpenaiCompatible => {
            let (final_url, final_headers, final_body_value, provider_api_key_id) =
                prepare_llm_request(
                    &provider,
                    &model,
                    data,
                    &original_headers,
                    &app_state,
                    &operation.downstream_path,
                )
                .await
                .map_err(|e| {
                    error!(
                        "Failed to prepare utility request '{}': {:?}",
                        operation.name, e
                    );
                    e
                })?;
            let final_body = Bytes::from(serde_json::to_vec(&final_body_value).map_err(|e| {
                protocol_transform_error("Failed to serialize final request body", e)
            })?);
            (final_url, final_headers, final_body, provider_api_key_id)
        }
        UtilityProtocol::GeminiCompatible => {
            let (final_url, final_headers, provider_api_key_id) = prepare_simple_gemini_request(
                &provider,
                &model,
                &original_headers,
                &app_state,
                &operation.downstream_path,
                &query_params,
            )
            .await
            .map_err(|e| {
                error!(
                    "Failed to prepare utility request '{}': {:?}",
                    operation.name, e
                );
                e
            })?;
            let final_body = Bytes::from(serde_json::to_vec(&data).map_err(|e| {
                protocol_transform_error("Failed to serialize final request body", e)
            })?);
            (final_url, final_headers, final_body, provider_api_key_id)
        }
    };

    let mut log_context = RequestLogContext::new(
        &system_api_key,
        &provider,
        &model,
        provider_api_key_id,
        start_time,
        &client_ip_addr,
        operation.api_type,
        target_api_type,
    );
    log_context.user_request_body = Some(LoggedBody::from_bytes(original_request_body));
    log_context.llm_request_body = Some(LoggedBody::from_bytes(final_body.clone()));

    let model_str = format_model_str(&provider, &model);
    let client = if provider.use_proxy {
        &app_state.proxy_client
    } else {
        &app_state.client
    };
    ensure_provider_request_allowed(&app_state, provider.id, &model_str).await?;

    debug!(
        "[utility:{}] proxy request header: {:?}",
        operation.name,
        serialize_reqwest_headers(&final_headers)
    );
    debug!(
        "[utility:{}] proxy request data: {}",
        operation.name,
        String::from_utf8_lossy(&final_body)
    );

    log_context.llm_request_sent_at = Some(Utc::now().timestamp_millis());
    let response = match send_with_first_byte_timeout(
        &cancellation,
        client
            .request(Method::POST, &final_url)
            .headers(final_headers)
            .body(final_body.clone()),
        "LLM request",
    )
    .await
    {
        Ok(resp) => resp,
        Err(proxy_error) => {
            cancellation_guard.disarm();
            error!("[utility:{}] {}", operation.name, proxy_error);
            if !matches!(proxy_error, ProxyError::ClientCancelled(_)) {
                record_provider_failure(&app_state, provider.id, &model_str, &proxy_error).await;
            }
            log_context.request_url = Some(final_url.clone());
            log_context.completion_ts = Some(Utc::now().timestamp_millis());
            log_context.billing_plan = billing_plan.clone();
            log_context.overall_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
                RequestStatus::Cancelled
            } else {
                RequestStatus::Error
            };
            get_log_manager().log(log_context).await;
            return Err(proxy_error);
        }
    };

    let status_code = response.status();
    let response_headers = response.headers().clone();
    let response_builder = build_response_builder(status_code, &response_headers);
    let is_gzip = response_headers
        .get(CONTENT_ENCODING)
        .map_or(false, |value| value.to_str().unwrap_or("").contains("gzip"));

    let body_bytes = match read_response_bytes_with_cancellation(
        response,
        "Reading upstream response body",
        &cancellation,
    )
    .await
    {
        Ok(b) => b,
        Err(proxy_error) => {
            cancellation_guard.disarm();
            error!("[utility:{}] {}", operation.name, proxy_error);
            if !matches!(proxy_error, ProxyError::ClientCancelled(_)) {
                record_provider_failure(&app_state, provider.id, &model_str, &proxy_error).await;
            }
            log_context.request_url = Some(final_url);
            log_context.llm_status = Some(status_code);
            log_context.completion_ts = Some(Utc::now().timestamp_millis());
            log_context.billing_plan = billing_plan;
            log_context.overall_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
                RequestStatus::Cancelled
            } else {
                RequestStatus::Error
            };
            log_context.llm_response_body =
                Some(LoggedBody::from_bytes(Bytes::from(proxy_error.to_string())));
            get_log_manager().log(log_context).await;
            return Err(proxy_error);
        }
    };

    let decompressed_body = decode_response_body(body_bytes, is_gzip);
    let completion_ts = Utc::now().timestamp_millis();
    let parsed_usage_info = serde_json::from_slice::<serde_json::Value>(&decompressed_body)
        .ok()
        .and_then(|val| parse_utility_usage_info(&val));

    let overall_status = if status_code.is_success() {
        RequestStatus::Success
    } else {
        RequestStatus::Error
    };
    finalize_non_streaming_log_context(
        &mut log_context,
        &final_url,
        status_code,
        completion_ts,
        billing_plan.as_ref(),
        overall_status,
        parsed_usage_info,
        decompressed_body.clone(),
        decompressed_body.clone(),
    );
    get_log_manager().log(log_context.clone()).await;
    cancellation_guard.disarm();

    if status_code.is_success() {
        record_provider_success(&app_state, provider.id, &model_str).await;
        info!(
            "{}: Utility request '{}' completed for log_id {}.",
            model_str, operation.name, log_context.id
        );
        Ok(response_builder
            .body(Body::from(decompressed_body))
            .unwrap())
    } else {
        error!(
            "[utility:{}] LLM request failed with status {} for log_id {}: {}",
            operation.name,
            status_code,
            log_context.id,
            String::from_utf8_lossy(&decompressed_body)
        );
        let proxy_error = classify_upstream_status(status_code, &decompressed_body);
        record_provider_failure(&app_state, provider.id, &model_str, &proxy_error).await;
        Err(proxy_error)
    }
}

#[cfg(test)]
mod tests {
    use super::{UtilityOperation, UtilityProtocol, validate_utility_target};
    use crate::{proxy::ProxyError, schema::enum_def::LlmApiType};

    #[test]
    fn validate_utility_target_enforces_openai_compatibility() {
        let operation = UtilityOperation {
            name: "embeddings".to_string(),
            api_type: LlmApiType::Openai,
            protocol: UtilityProtocol::OpenaiCompatible,
            downstream_path: "embeddings".to_string(),
        };

        assert!(validate_utility_target(&operation, LlmApiType::Openai).is_ok());
        assert!(matches!(
            validate_utility_target(&operation, LlmApiType::Gemini),
            Err(ProxyError::BadRequest(_))
        ));
    }

    #[test]
    fn validate_utility_target_enforces_gemini_compatibility() {
        let operation = UtilityOperation {
            name: "countTokens".to_string(),
            api_type: LlmApiType::Gemini,
            protocol: UtilityProtocol::GeminiCompatible,
            downstream_path: "countTokens".to_string(),
        };

        assert!(validate_utility_target(&operation, LlmApiType::Gemini).is_ok());
        assert!(matches!(
            validate_utility_target(&operation, LlmApiType::Openai),
            Err(ProxyError::BadRequest(_))
        ));
    }
}
