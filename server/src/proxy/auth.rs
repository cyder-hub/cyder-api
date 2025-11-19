use std::{collections::HashMap, sync::Arc};

use axum::http::HeaderMap;
use reqwest::{header::AUTHORIZATION, StatusCode};

use crate::{
    database::{
        model::Model, provider::Provider,
        system_api_key::SystemApiKey,
    },
    service::app_state::{AppStoreError, AppState, SystemApiKeyStore},
    utils::{auth::decode_api_key_jwt, limit::LIMITER},
};
use cyder_tools::log::{debug, error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyPosition {
    AuthorizationHeader,
    XGoogApiKeyHeader,
    XApiKeyHeader,
    KeyQuery,
}

pub struct ApiKeyCheckResult {
    pub api_key: SystemApiKey,
    pub channel: Option<String>,
    pub external_id: Option<String>,
    pub position: ApiKeyPosition,
}

// Authenticates an OpenAI-style request (Bearer token or query param).
pub async fn authenticate_openai_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, (StatusCode, String)> {
    debug!("Authenticating OpenAI request");
    let (system_api_key_str, position) = parse_token_from_request(headers, params)
        .map_err(|err_msg| {
            warn!("OpenAI auth failed: {}", err_msg);
            (StatusCode::UNAUTHORIZED, err_msg)
        })?;
    check_system_api_key(&app_state.system_api_key_store, &system_api_key_str, position)
        .map_err(|err_msg| {
            warn!("OpenAI system key check failed: {}", err_msg);
            (StatusCode::UNAUTHORIZED, err_msg)
        })
}

// Authenticates a Gemini-style request (X-Goog-Api-Key header or 'key' query param).
pub fn authenticate_gemini_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, (StatusCode, String)> {
    debug!("Authenticating Gemini request");
    let (system_api_key_str, position) = match headers.get("X-Goog-Api-Key") {
        Some(header_value) => match header_value.to_str() {
            Ok(key) => (key.to_string(), ApiKeyPosition::XGoogApiKeyHeader),
            Err(_) => {
                warn!("Invalid characters in X-Goog-Api-Key header");
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Invalid characters in X-Goog-Api-Key header".to_string(),
                ));
            }
        },
        None => match params.get("key") {
            Some(key) => (key.clone(), ApiKeyPosition::KeyQuery),
            None => {
                warn!("Missing API key for Gemini request");
                return Err((
                    StatusCode::UNAUTHORIZED,
                    "Missing API key. Provide it in 'X-Goog-Api-Key' header or 'key' query parameter.".to_string()
                ));
            }
        },
    };
    check_system_api_key(&app_state.system_api_key_store, &system_api_key_str, position)
        .map_err(|err_msg| {
            warn!("Gemini system key check failed: {}", err_msg);
            (StatusCode::UNAUTHORIZED, err_msg)
        })
}

// Authenticates an Anthropic-style request (x-api-key header).
pub fn authenticate_anthropic_request(
    headers: &HeaderMap,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, (StatusCode, String)> {
    debug!("Authenticating Anthropic request");
    let system_api_key_str = match headers.get("x-api-key") {
        Some(header_value) => match header_value.to_str() {
            Ok(key) => key.to_string(),
            Err(_) => {
                warn!("Invalid characters in x-api-key header");
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Invalid characters in x-api-key header".to_string(),
                ));
            }
        },
        None => {
            warn!("Missing API key for Anthropic request");
            return Err((
                StatusCode::UNAUTHORIZED,
                "Missing API key. Provide it in 'x-api-key' header.".to_string(),
            ));
        }
    };
    check_system_api_key(
        &app_state.system_api_key_store,
        &system_api_key_str,
        ApiKeyPosition::XApiKeyHeader,
    )
    .map_err(|err_msg| {
        warn!("Anthropic system key check failed: {}", err_msg);
        (StatusCode::UNAUTHORIZED, err_msg)
    })
}

// Checks if the request is allowed by the access control policy.
pub fn check_access_control(
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

const BEARER_PREFIX: &str = "Bearer ";
pub fn parse_token_from_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
) -> Result<(String, ApiKeyPosition), String> {
    if let Some(auth_header_value) = headers.get(AUTHORIZATION) {
        if let Ok(auth_str) = auth_header_value.to_str() {
            if let Some(token) = auth_str.strip_prefix(BEARER_PREFIX) {
                if !token.is_empty() && token != "raspberry" {
                    return Ok((token.to_string(), ApiKeyPosition::AuthorizationHeader));
                }
            }
        }
    }

    // Fallback to query parameter
    params
        .get("key")
        .cloned()
        .map(|key| (key, ApiKeyPosition::KeyQuery))
        .ok_or_else(|| {
            "Missing API key. Provide it in 'Authorization' header or 'key' query parameter."
                .to_string()
        })
}

// Updated to query from the new StateStore<SystemApiKey> struct
pub fn check_system_api_key(
    store: &SystemApiKeyStore,
    key_str: &str,
    position: ApiKeyPosition,
) -> Result<ApiKeyCheckResult, String> {
    if key_str.starts_with("cyder-") {
        match store.get_by_key(key_str) {
            Ok(Some(api_key)) => Ok(ApiKeyCheckResult {
                api_key,
                channel: None,
                external_id: None,
                position,
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
                position,
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
