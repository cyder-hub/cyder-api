use std::{collections::HashMap, sync::Arc};

use axum::http::HeaderMap;
use reqwest::header::AUTHORIZATION;

use super::ProxyError;
use crate::{
    service::app_state::{AppState, AppStoreError},
    service::cache::types::{CacheModel, CacheProvider, CacheSystemApiKey},
    utils::limit::LIMITER,
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
    pub api_key: Arc<CacheSystemApiKey>,
    pub position: ApiKeyPosition,
}

// Authenticates an OpenAI-style request (Bearer token or query param).
pub async fn authenticate_openai_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, ProxyError> {
    debug!("Authenticating OpenAI request");
    let (system_api_key_str, position) =
        parse_token_from_request(headers, params).map_err(|err_msg| {
            warn!("OpenAI auth failed: {}", err_msg);
            ProxyError::Unauthorized(err_msg)
        })?;
    check_system_api_key(app_state, &system_api_key_str, position)
        .await
        .map_err(|err_msg| {
            warn!("OpenAI system key check failed: {}", err_msg);
            ProxyError::Unauthorized(err_msg)
        })
}

// Authenticates a Gemini-style request (X-Goog-Api-Key header or 'key' query param).
pub async fn authenticate_gemini_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, ProxyError> {
    debug!("Authenticating Gemini request");
    let (system_api_key_str, position) = match headers.get("X-Goog-Api-Key") {
        Some(header_value) => match header_value.to_str() {
            Ok(key) => (key.to_string(), ApiKeyPosition::XGoogApiKeyHeader),
            Err(_) => {
                warn!("Invalid characters in X-Goog-Api-Key header");
                return Err(ProxyError::BadRequest(
                    "Invalid characters in X-Goog-Api-Key header".to_string(),
                ));
            }
        },
        None => match params.get("key") {
            Some(key) => (key.clone(), ApiKeyPosition::KeyQuery),
            None => {
                warn!("Missing API key for Gemini request");
                return Err(ProxyError::Unauthorized(
                    "Missing API key. Provide it in 'X-Goog-Api-Key' header or 'key' query parameter.".to_string()
                ));
            }
        },
    };
    check_system_api_key(app_state, &system_api_key_str, position)
        .await
        .map_err(|err_msg| {
            warn!("Gemini system key check failed: {}", err_msg);
            ProxyError::Unauthorized(err_msg)
        })
}

// Authenticates an Anthropic-style request (x-api-key header).
pub async fn authenticate_anthropic_request(
    headers: &HeaderMap,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, ProxyError> {
    debug!(
        "Authenticating Anthropic request: x-api-key={}, authorization={}",
        headers.contains_key("x-api-key"),
        headers.contains_key(AUTHORIZATION)
    );
    let (system_api_key_str, position) =
        parse_anthropic_api_key_from_headers(headers).map_err(|err| {
            warn!("Anthropic auth failed: {}", err);
            err
        })?;
    check_system_api_key(app_state, &system_api_key_str, position)
        .await
        .map_err(|err_msg| {
            warn!("Anthropic system key check failed: {}", err_msg);
            ProxyError::Unauthorized(err_msg)
        })
}

// Authenticates an Ollama-style request (Bearer token or query param, same as OpenAI).
pub async fn authenticate_ollama_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, ProxyError> {
    debug!("Authenticating Ollama request");
    let (system_api_key_str, position) =
        parse_token_from_request(headers, params).map_err(|err_msg| {
            warn!("Ollama auth failed: {}", err_msg);
            ProxyError::Unauthorized(err_msg)
        })?;
    check_system_api_key(app_state, &system_api_key_str, position)
        .await
        .map_err(|err_msg| {
            warn!("Ollama system key check failed: {}", err_msg);
            ProxyError::Unauthorized(err_msg)
        })
}

// Checks if the request is allowed by the access control policy.
pub async fn check_access_control(
    system_api_key: &CacheSystemApiKey,
    provider: &CacheProvider,
    model: &CacheModel,
    app_state: &Arc<AppState>,
) -> Result<(), ProxyError> {
    if let Some(policy_id) = system_api_key.access_control_policy_id {
        match app_state.get_access_control_policy(policy_id).await {
            Ok(Some(policy)) => {
                if let Err(reason) = LIMITER.check_limit_strategy(&policy, provider.id, model.id) {
                    info!(
                        "Access denied by policy '{}' for SystemApiKey ID {}, Provider ID {}, Model ID {}. Reason: {}",
                        policy.name, system_api_key.id, provider.id, model.id, reason
                    );
                    return Err(ProxyError::Forbidden(format!(
                        "Access denied by access control policy: {}",
                        reason,
                    )));
                }
            }
            Ok(None) => {
                let err_msg = format!(
                    "Access control policy id {} configured but not found in application cache.",
                    policy_id
                );
                error!("{}, SystemApiKey ID: {}", err_msg, system_api_key.id);
                return Err(ProxyError::InternalError(err_msg));
            }
            Err(store_err) => {
                let err_msg = format!(
                    "Error accessing application cache for access control policy id {}: {}",
                    policy_id, store_err
                );
                error!("{}, SystemApiKey ID: {}", err_msg, system_api_key.id);
                return Err(ProxyError::InternalError(err_msg));
            }
        }
    }
    Ok(())
}

const BEARER_PREFIX: &str = "Bearer ";

fn parse_anthropic_api_key_from_headers(
    headers: &HeaderMap,
) -> Result<(String, ApiKeyPosition), ProxyError> {
    if let Some(header_value) = headers.get("x-api-key") {
        return match header_value.to_str() {
            Ok(key) => Ok((key.to_string(), ApiKeyPosition::XApiKeyHeader)),
            Err(_) => Err(ProxyError::BadRequest(
                "Invalid characters in x-api-key header".to_string(),
            )),
        };
    }

    match headers.get(AUTHORIZATION) {
        Some(header_value) => match header_value.to_str() {
            Ok(auth_str) => match auth_str.strip_prefix(BEARER_PREFIX) {
                Some(token) if !token.is_empty() => {
                    Ok((token.to_string(), ApiKeyPosition::AuthorizationHeader))
                }
                _ => Err(ProxyError::Unauthorized(
                    "Invalid Authorization header. Expected 'Bearer <api-key>'.".to_string(),
                )),
            },
            Err(_) => Err(ProxyError::BadRequest(
                "Invalid characters in Authorization header".to_string(),
            )),
        },
        None => Err(ProxyError::Unauthorized(
            "Missing API key. Provide it in 'x-api-key' header or 'Authorization: Bearer <api-key>' header.".to_string(),
        )),
    }
}

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

// Checks system API key from AppState cache
pub async fn check_system_api_key(
    app_state: &AppState,
    key_str: &str,
    position: ApiKeyPosition,
) -> Result<ApiKeyCheckResult, String> {
    if key_str.starts_with("cyder-") {
        match app_state.get_system_api_key(key_str).await {
            Ok(Some(api_key)) => Ok(ApiKeyCheckResult { api_key, position }),
            Ok(None) => Err("api key invalid or not found".to_string()),
            Err(AppStoreError::LockError(e)) => {
                error!("AppState lock error: {}", e);
                Err("Internal server error while checking API key".to_string())
            }
            Err(e) => {
                error!("AppState error: {:?}", e);
                Err("Internal server error while checking API key".to_string())
            }
        }
    } else {
        Err("Invalid api key format. Must start with 'cyder-'".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiKeyPosition, ProxyError, parse_anthropic_api_key_from_headers};
    use axum::http::{HeaderMap, HeaderValue, header::AUTHORIZATION};

    #[test]
    fn anthropic_auth_prefers_x_api_key_over_authorization() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_static("cyder-from-x-api-key"),
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer cyder-from-authorization"),
        );

        let (key, position) = parse_anthropic_api_key_from_headers(&headers).unwrap();

        assert_eq!(key, "cyder-from-x-api-key");
        assert_eq!(position, ApiKeyPosition::XApiKeyHeader);
    }

    #[test]
    fn anthropic_auth_falls_back_to_authorization_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer cyder-from-authorization"),
        );

        let (key, position) = parse_anthropic_api_key_from_headers(&headers).unwrap();

        assert_eq!(key, "cyder-from-authorization");
        assert_eq!(position, ApiKeyPosition::AuthorizationHeader);
    }

    #[test]
    fn anthropic_auth_rejects_invalid_authorization_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic abc"));

        let err = parse_anthropic_api_key_from_headers(&headers).unwrap_err();

        assert!(matches!(err, ProxyError::Unauthorized(_)));
    }

    #[test]
    fn anthropic_auth_requires_header_when_none_present() {
        let headers = HeaderMap::new();

        let err = parse_anthropic_api_key_from_headers(&headers).unwrap_err();

        assert!(matches!(err, ProxyError::Unauthorized(_)));
    }
}
