use std::{collections::HashMap, sync::Arc};

use axum::http::HeaderMap;
use reqwest::header::AUTHORIZATION;

use super::{ProxyError, error::ProxyLogLevel};
use crate::{
    database::api_key::{ApiKey, hash_api_key},
    service::app_state::{
        ApiKeyConcurrencyGuard, ApiKeyGovernanceAdmissionError, AppState, AppStoreError,
    },
    service::cache::types::{CacheApiKey, CacheModel, CacheProvider},
    utils::acl::ACL_EVALUATOR,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyPosition {
    AuthorizationHeader,
    XGoogApiKeyHeader,
    XApiKeyHeader,
    KeyQuery,
}

pub struct ApiKeyCheckResult {
    pub api_key: Arc<CacheApiKey>,
    pub position: ApiKeyPosition,
}

fn log_auth_request_rejected(
    protocol: &'static str,
    source: Option<&'static str>,
    proxy_error: &ProxyError,
) {
    match proxy_error.operator_log_level() {
        ProxyLogLevel::Debug => crate::debug_event!(
            "auth.request_rejected",
            protocol = protocol,
            error_code = proxy_error.error_code(),
            source = source,
        ),
        ProxyLogLevel::Warn => crate::warn_event!(
            "auth.request_rejected",
            protocol = protocol,
            error_code = proxy_error.error_code(),
            source = source,
        ),
        ProxyLogLevel::Error => crate::error_event!(
            "auth.request_rejected",
            protocol = protocol,
            error_code = proxy_error.error_code(),
            source = source,
        ),
    }
}

// Authenticates an OpenAI-style request (Bearer token or query param).
pub async fn authenticate_openai_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, ProxyError> {
    let (system_api_key_str, position) =
        parse_token_from_request(headers, params).map_err(|err_msg| {
            let proxy_error = ProxyError::Unauthorized(err_msg);
            log_auth_request_rejected("openai", None, &proxy_error);
            proxy_error
        })?;
    let result = check_system_api_key(app_state, &system_api_key_str, position).await;
    if let Err(proxy_error) = &result {
        log_auth_request_rejected("openai", None, proxy_error);
    }
    result
}

// Authenticates a Gemini-style request (X-Goog-Api-Key header or 'key' query param).
pub async fn authenticate_gemini_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, ProxyError> {
    let (system_api_key_str, position) = match headers.get("X-Goog-Api-Key") {
        Some(header_value) => match header_value.to_str() {
            Ok(key) => (key.to_string(), ApiKeyPosition::XGoogApiKeyHeader),
            Err(_) => {
                let proxy_error = ProxyError::BadRequest(
                    "Invalid characters in X-Goog-Api-Key header".to_string(),
                );
                log_auth_request_rejected("gemini", Some("x-goog-api-key"), &proxy_error);
                return Err(proxy_error);
            }
        },
        None => match params.get("key") {
            Some(key) => (key.clone(), ApiKeyPosition::KeyQuery),
            None => {
                let proxy_error = ProxyError::Unauthorized(
                    "Missing API key. Provide it in 'X-Goog-Api-Key' header or 'key' query parameter.".to_string()
                );
                log_auth_request_rejected("gemini", Some("key_or_x-goog-api-key"), &proxy_error);
                return Err(proxy_error);
            }
        },
    };
    let result = check_system_api_key(app_state, &system_api_key_str, position).await;
    if let Err(proxy_error) = &result {
        log_auth_request_rejected("gemini", None, proxy_error);
    }
    result
}

// Authenticates an Anthropic-style request (x-api-key header).
pub async fn authenticate_anthropic_request(
    headers: &HeaderMap,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, ProxyError> {
    let (system_api_key_str, position) =
        parse_anthropic_api_key_from_headers(headers).map_err(|err| {
            log_auth_request_rejected("anthropic", None, &err);
            err
        })?;
    let result = check_system_api_key(app_state, &system_api_key_str, position).await;
    if let Err(proxy_error) = &result {
        log_auth_request_rejected("anthropic", None, proxy_error);
    }
    result
}

// Authenticates an Ollama-style request (Bearer token or query param, same as OpenAI).
pub async fn authenticate_ollama_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, ProxyError> {
    let (system_api_key_str, position) =
        parse_token_from_request(headers, params).map_err(|err_msg| {
            let proxy_error = ProxyError::Unauthorized(err_msg);
            log_auth_request_rejected("ollama", None, &proxy_error);
            proxy_error
        })?;
    let result = check_system_api_key(app_state, &system_api_key_str, position).await;
    if let Err(proxy_error) = &result {
        log_auth_request_rejected("ollama", None, proxy_error);
    }
    result
}

// Checks if the request is allowed by the API key's embedded ACL snapshot.
pub async fn check_access_control(
    system_api_key: &CacheApiKey,
    provider: &CacheProvider,
    model: &CacheModel,
    _app_state: &Arc<AppState>,
) -> Result<(), ProxyError> {
    if let Err(reason) = ACL_EVALUATOR.authorize(
        &system_api_key.name,
        &system_api_key.default_action,
        &system_api_key.acl_rules,
        provider.id,
        model.id,
    ) {
        return Err(ProxyError::Forbidden(format!(
            "Access denied by api key access control: {}",
            reason,
        )));
    }

    Ok(())
}

pub async fn admit_api_key_request(
    app_state: &Arc<AppState>,
    system_api_key: &CacheApiKey,
) -> Result<Option<ApiKeyConcurrencyGuard>, ProxyError> {
    match app_state.try_begin_api_key_request(system_api_key).await {
        Ok(guard) => Ok(guard),
        Err(ApiKeyGovernanceAdmissionError::Internal(message)) => {
            crate::error_event!(
                "auth.governance_state_error",
                api_key_id = system_api_key.id,
                error = message,
            );
            Err(ProxyError::InternalError(
                "Internal server error while evaluating API key governance".to_string(),
            ))
        }
        Err(ApiKeyGovernanceAdmissionError::RateLimited { limit, current }) => {
            Err(ProxyError::RateLimited(format!(
                "API key '{}' exceeded rate_limit_rpm={} (current_window_requests={})",
                system_api_key.name, limit, current
            )))
        }
        Err(ApiKeyGovernanceAdmissionError::ConcurrencyLimited { limit, current }) => {
            Err(ProxyError::ConcurrencyLimited(format!(
                "API key '{}' exceeded max_concurrent_requests={} (current={})",
                system_api_key.name, limit, current
            )))
        }
        Err(ApiKeyGovernanceAdmissionError::DailyRequestQuotaExceeded { limit, current }) => {
            Err(ProxyError::QuotaExhausted(format!(
                "API key '{}' exhausted daily request quota {} (current={})",
                system_api_key.name, limit, current
            )))
        }
        Err(ApiKeyGovernanceAdmissionError::DailyTokenQuotaExceeded { limit, current }) => {
            Err(ProxyError::QuotaExhausted(format!(
                "API key '{}' exhausted daily token quota {} (current={})",
                system_api_key.name, limit, current
            )))
        }
        Err(ApiKeyGovernanceAdmissionError::MonthlyTokenQuotaExceeded { limit, current }) => {
            Err(ProxyError::QuotaExhausted(format!(
                "API key '{}' exhausted monthly token quota {} (current={})",
                system_api_key.name, limit, current
            )))
        }
        Err(ApiKeyGovernanceAdmissionError::DailyBudgetExceeded {
            currency,
            limit_nanos,
            current_nanos,
        }) => Err(ProxyError::BudgetExhausted(format!(
            "API key '{}' exhausted daily budget {} {} (current={})",
            system_api_key.name, currency, limit_nanos, current_nanos
        ))),
        Err(ApiKeyGovernanceAdmissionError::MonthlyBudgetExceeded {
            currency,
            limit_nanos,
            current_nanos,
        }) => Err(ProxyError::BudgetExhausted(format!(
            "API key '{}' exhausted monthly budget {} {} (current={})",
            system_api_key.name, currency, limit_nanos, current_nanos
        ))),
    }
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
) -> Result<ApiKeyCheckResult, ProxyError> {
    if key_str.starts_with("cyder-") {
        match app_state.get_api_key(key_str).await {
            Ok(Some(api_key)) => Ok(ApiKeyCheckResult { api_key, position }),
            Ok(None) => classify_missing_active_api_key(key_str),
            Err(AppStoreError::LockError(e)) => {
                crate::error_event!("auth.app_state_lock_error", error = e);
                Err(ProxyError::InternalError(
                    "Internal server error while checking API key".to_string(),
                ))
            }
            Err(e) => {
                crate::error_event!("auth.app_state_error", error = format!("{e:?}"));
                Err(ProxyError::InternalError(
                    "Internal server error while checking API key".to_string(),
                ))
            }
        }
    } else {
        Err(ProxyError::Unauthorized(
            "Invalid api key format. Must start with 'cyder-'".to_string(),
        ))
    }
}

fn classify_missing_active_api_key(key_str: &str) -> Result<ApiKeyCheckResult, ProxyError> {
    let key_hash = hash_api_key(key_str);
    let row = match ApiKey::get_by_hash(&key_hash) {
        Ok(row) => row,
        Err(crate::controller::BaseError::NotFound(_)) => {
            return Err(ProxyError::Unauthorized(
                "api key invalid or not found".to_string(),
            ));
        }
        Err(err) => {
            crate::error_event!(
                "auth.database_classification_error",
                error = format!("{err:?}")
            );
            return Err(ProxyError::InternalError(
                "Internal server error while checking API key".to_string(),
            ));
        }
    };

    classify_inactive_api_key_row(&row)?;

    Err(ProxyError::Unauthorized(
        "api key invalid or not found".to_string(),
    ))
}

fn classify_inactive_api_key_row(row: &ApiKey) -> Result<(), ProxyError> {
    if !row.is_enabled {
        return Err(ProxyError::KeyDisabled(format!(
            "API key '{}' is disabled",
            row.name
        )));
    }

    if let Some(expires_at) = row.expires_at {
        if expires_at <= chrono::Utc::now().timestamp_millis() {
            return Err(ProxyError::KeyExpired(format!(
                "API key '{}' expired at {}",
                row.name, expires_at
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ApiKeyPosition, ProxyError, classify_inactive_api_key_row,
        parse_anthropic_api_key_from_headers,
    };
    use axum::http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
    use chrono::Utc;

    use crate::database::api_key::ApiKey;

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

    #[test]
    fn inactive_api_key_row_classifies_disabled_key() {
        let row = ApiKey {
            name: "disabled".to_string(),
            is_enabled: false,
            ..ApiKey::default()
        };

        let err = classify_inactive_api_key_row(&row).expect_err("disabled key should fail");

        assert!(matches!(err, ProxyError::KeyDisabled(_)));
    }

    #[test]
    fn inactive_api_key_row_classifies_expired_key() {
        let row = ApiKey {
            name: "expired".to_string(),
            is_enabled: true,
            expires_at: Some(Utc::now().timestamp_millis() - 1),
            ..ApiKey::default()
        };

        let err = classify_inactive_api_key_row(&row).expect_err("expired key should fail");

        assert!(matches!(err, ProxyError::KeyExpired(_)));
    }
}
