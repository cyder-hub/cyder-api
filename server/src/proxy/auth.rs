use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::http::HeaderMap;
use reqwest::header::AUTHORIZATION;

use super::{ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, error::ProxyLogLevel};
use crate::{
    database::api_key::{ApiKey, hash_api_key},
    service::app_state::{AppState, AppStoreError},
    service::cache::types::{CacheApiKey, CacheModel, CacheProvider},
    service::runtime::{ApiKeyGovernanceAdmissionError, ApiKeyRequestLease},
    utils::acl::ACL_EVALUATOR,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyPosition {
    AuthorizationHeader,
    XGoogApiKeyHeader,
    XApiKeyHeader,
    KeyQuery,
}

impl ApiKeyPosition {
    fn as_str(self) -> &'static str {
        match self {
            Self::AuthorizationHeader => "authorization_header",
            Self::XGoogApiKeyHeader => "x-goog-api-key_header",
            Self::XApiKeyHeader => "x-api-key_header",
            Self::KeyQuery => "key_query",
        }
    }
}

pub struct ApiKeyCheckResult {
    pub api_key: Arc<CacheApiKey>,
    pub position: ApiKeyPosition,
}

fn log_auth_request_accepted(protocol: &'static str, result: &ApiKeyCheckResult) {
    crate::debug_event!(
        "auth.request_accepted",
        protocol = protocol,
        api_key_id = result.api_key.id,
        source = result.position.as_str(),
    );
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
            error_code = proxy_error.code().as_str(),
            stage = proxy_error.stage().as_str(),
            response_visibility = proxy_error.response_visibility().as_str(),
            source = source,
        ),
        ProxyLogLevel::Error => crate::error_event!(
            "auth.request_rejected",
            protocol = protocol,
            error_code = proxy_error.code().as_str(),
            stage = proxy_error.stage().as_str(),
            response_visibility = proxy_error.response_visibility().as_str(),
            source = source,
        ),
    }
}

fn authentication_error(code: ProxyErrorCode, message: impl Into<String>) -> ProxyError {
    let message = message.into();
    ProxyError::gateway(
        code,
        ExecutionStage::Authentication,
        ResponseVisibility::NotVisible,
        Some(message.clone()),
        message,
    )
}

fn governance_error(code: ProxyErrorCode, message: impl Into<String>) -> ProxyError {
    let message = message.into();
    ProxyError::gateway(
        code,
        ExecutionStage::Governance,
        ResponseVisibility::NotVisible,
        Some(message.clone()),
        message,
    )
}

fn governance_error_with_retry_after(
    code: ProxyErrorCode,
    message: impl Into<String>,
    retry_after: Duration,
) -> ProxyError {
    governance_error(code, message).with_retry_after(retry_after)
}

fn internal_error(stage: ExecutionStage, message: impl Into<String>) -> ProxyError {
    ProxyError::gateway(
        ProxyErrorCode::ServerError,
        stage,
        ResponseVisibility::NotVisible,
        None,
        message,
    )
}

// Authenticates an OpenAI-style request (Bearer token or query param).
pub async fn authenticate_openai_request(
    headers: &HeaderMap,
    params: &HashMap<String, String>,
    app_state: &Arc<AppState>,
) -> Result<ApiKeyCheckResult, ProxyError> {
    let (system_api_key_str, position) =
        parse_token_from_request(headers, params).map_err(|err_msg| {
            let proxy_error = authentication_error(ProxyErrorCode::AuthenticationError, err_msg);
            log_auth_request_rejected("openai", None, &proxy_error);
            proxy_error
        })?;
    let result = check_system_api_key(app_state, &system_api_key_str, position).await;
    match &result {
        Ok(auth) => log_auth_request_accepted("openai", auth),
        Err(proxy_error) => log_auth_request_rejected("openai", None, proxy_error),
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
                let proxy_error = authentication_error(
                    ProxyErrorCode::AuthenticationError,
                    "Invalid characters in X-Goog-Api-Key header",
                );
                log_auth_request_rejected("gemini", Some("x-goog-api-key"), &proxy_error);
                return Err(proxy_error);
            }
        },
        None => match params.get("key") {
            Some(key) => (key.clone(), ApiKeyPosition::KeyQuery),
            None => {
                let proxy_error = authentication_error(
                    ProxyErrorCode::AuthenticationError,
                    "Missing API key. Provide it in 'X-Goog-Api-Key' header or 'key' query parameter.",
                );
                log_auth_request_rejected("gemini", Some("key_or_x-goog-api-key"), &proxy_error);
                return Err(proxy_error);
            }
        },
    };
    let result = check_system_api_key(app_state, &system_api_key_str, position).await;
    match &result {
        Ok(auth) => log_auth_request_accepted("gemini", auth),
        Err(proxy_error) => log_auth_request_rejected("gemini", None, proxy_error),
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
    match &result {
        Ok(auth) => log_auth_request_accepted("anthropic", auth),
        Err(proxy_error) => log_auth_request_rejected("anthropic", None, proxy_error),
    }
    result
}

// Checks if the request is allowed by the API key's embedded ACL snapshot.
pub(crate) fn evaluate_access_control(
    api_key: &CacheApiKey,
    provider: &CacheProvider,
    model: &CacheModel,
) -> Result<(), String> {
    ACL_EVALUATOR
        .authorize(
            &api_key.name,
            &api_key.default_action,
            &api_key.acl_rules,
            provider.id,
            model.id,
        )
        .map(|_| ())
}

pub async fn check_access_control(
    api_key: &CacheApiKey,
    provider: &CacheProvider,
    model: &CacheModel,
    _app_state: &Arc<AppState>,
) -> Result<(), ProxyError> {
    if let Err(reason) = evaluate_access_control(api_key, provider, model) {
        return Err(governance_error(
            ProxyErrorCode::PermissionError,
            format!("Access denied by api key access control: {}", reason,),
        ));
    }

    Ok(())
}

pub async fn admit_api_key_request(
    app_state: &Arc<AppState>,
    api_key: &CacheApiKey,
) -> Result<Option<ApiKeyRequestLease>, ProxyError> {
    match app_state
        .api_key_governance
        .try_begin_api_key_request(api_key)
        .await
    {
        Ok(guard) => Ok(guard),
        Err(error) => Err(api_key_governance_error_to_proxy_error(api_key, error)),
    }
}

fn api_key_governance_error_to_proxy_error(
    api_key: &CacheApiKey,
    error: ApiKeyGovernanceAdmissionError,
) -> ProxyError {
    match error {
        ApiKeyGovernanceAdmissionError::Internal(message) => {
            crate::error_event!(
                "auth.governance_state_error",
                api_key_id = api_key.id,
                error = message,
            );
            internal_error(
                ExecutionStage::Governance,
                format!("Internal server error while evaluating API key governance: {message}"),
            )
        }
        ApiKeyGovernanceAdmissionError::RateLimited {
            limit,
            current,
            retry_after,
        } => governance_error_with_retry_after(
            ProxyErrorCode::RateLimitError,
            format!(
                "API key '{}' exceeded rate_limit_rpm={} (current_window_requests={})",
                api_key.name, limit, current
            ),
            retry_after,
        ),
        ApiKeyGovernanceAdmissionError::ConcurrencyLimited { limit, current } => governance_error(
            ProxyErrorCode::ConcurrencyLimitError,
            format!(
                "API key '{}' exceeded max_concurrent_requests={} (current={})",
                api_key.name, limit, current
            ),
        ),
        ApiKeyGovernanceAdmissionError::DailyRequestQuotaExceeded {
            limit,
            current,
            retry_after,
        } => governance_error_with_retry_after(
            ProxyErrorCode::QuotaExhaustedError,
            format!(
                "API key '{}' exhausted daily request quota {} (current={})",
                api_key.name, limit, current
            ),
            retry_after,
        ),
        ApiKeyGovernanceAdmissionError::DailyTokenQuotaExceeded {
            limit,
            current,
            retry_after,
        } => governance_error_with_retry_after(
            ProxyErrorCode::QuotaExhaustedError,
            format!(
                "API key '{}' exhausted daily token quota {} (current={})",
                api_key.name, limit, current
            ),
            retry_after,
        ),
        ApiKeyGovernanceAdmissionError::MonthlyTokenQuotaExceeded {
            limit,
            current,
            retry_after,
        } => governance_error_with_retry_after(
            ProxyErrorCode::QuotaExhaustedError,
            format!(
                "API key '{}' exhausted monthly token quota {} (current={})",
                api_key.name, limit, current
            ),
            retry_after,
        ),
        ApiKeyGovernanceAdmissionError::DailyBudgetExceeded {
            currency,
            limit_nanos,
            current_nanos,
            retry_after,
        } => governance_error_with_retry_after(
            ProxyErrorCode::BudgetExhaustedError,
            format!(
                "API key '{}' exhausted daily budget {} {} (current={})",
                api_key.name, currency, limit_nanos, current_nanos
            ),
            retry_after,
        ),
        ApiKeyGovernanceAdmissionError::MonthlyBudgetExceeded {
            currency,
            limit_nanos,
            current_nanos,
            retry_after,
        } => governance_error_with_retry_after(
            ProxyErrorCode::BudgetExhaustedError,
            format!(
                "API key '{}' exhausted monthly budget {} {} (current={})",
                api_key.name, currency, limit_nanos, current_nanos
            ),
            retry_after,
        ),
    }
}

const BEARER_PREFIX: &str = "Bearer ";

fn parse_anthropic_api_key_from_headers(
    headers: &HeaderMap,
) -> Result<(String, ApiKeyPosition), ProxyError> {
    if let Some(header_value) = headers.get("x-api-key") {
        return match header_value.to_str() {
            Ok(key) => Ok((key.to_string(), ApiKeyPosition::XApiKeyHeader)),
            Err(_) => Err(authentication_error(
                ProxyErrorCode::AuthenticationError,
                "Invalid characters in x-api-key header",
            )),
        };
    }

    match headers.get(AUTHORIZATION) {
        Some(header_value) => match header_value.to_str() {
            Ok(auth_str) => match auth_str.strip_prefix(BEARER_PREFIX) {
                Some(token) if !token.is_empty() => {
                    Ok((token.to_string(), ApiKeyPosition::AuthorizationHeader))
                }
                _ => Err(authentication_error(
                    ProxyErrorCode::AuthenticationError,
                    "Invalid Authorization header. Expected 'Bearer <api-key>'.",
                )),
            },
            Err(_) => Err(authentication_error(
                ProxyErrorCode::AuthenticationError,
                "Invalid characters in Authorization header",
            )),
        },
        None => Err(authentication_error(
            ProxyErrorCode::AuthenticationError,
            "Missing API key. Provide it in 'x-api-key' header or 'Authorization: Bearer <api-key>' header.",
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
        match app_state.catalog.get_api_key(key_str).await {
            Ok(Some(api_key)) => Ok(ApiKeyCheckResult { api_key, position }),
            Ok(None) => classify_missing_active_api_key(key_str),
            Err(AppStoreError::LockError(e)) => {
                crate::error_event!("auth.app_state_lock_error", error = e);
                Err(internal_error(
                    ExecutionStage::Authentication,
                    format!("Internal server error while checking API key: {e}"),
                ))
            }
            Err(e) => {
                crate::error_event!("auth.app_state_error", error = format!("{e:?}"));
                Err(internal_error(
                    ExecutionStage::Authentication,
                    format!("Internal server error while checking API key: {e:?}"),
                ))
            }
        }
    } else {
        Err(authentication_error(
            ProxyErrorCode::AuthenticationError,
            "Invalid api key format. Must start with 'cyder-'",
        ))
    }
}

fn classify_missing_active_api_key(key_str: &str) -> Result<ApiKeyCheckResult, ProxyError> {
    let key_hash = hash_api_key(key_str);
    let row = match ApiKey::get_by_hash(&key_hash) {
        Ok(row) => row,
        Err(crate::controller::BaseError::NotFound(_)) => {
            return Err(authentication_error(
                ProxyErrorCode::AuthenticationError,
                "api key invalid or not found",
            ));
        }
        Err(err) => {
            crate::error_event!(
                "auth.database_classification_error",
                error = format!("{err:?}")
            );
            return Err(internal_error(
                ExecutionStage::Authentication,
                format!("Internal server error while checking API key: {err:?}"),
            ));
        }
    };

    classify_inactive_api_key_row(&row)?;

    Err(authentication_error(
        ProxyErrorCode::AuthenticationError,
        "api key invalid or not found",
    ))
}

fn classify_inactive_api_key_row(row: &ApiKey) -> Result<(), ProxyError> {
    if !row.is_enabled {
        return Err(authentication_error(
            ProxyErrorCode::ApiKeyDisabledError,
            format!("API key '{}' is disabled", row.name),
        ));
    }

    if let Some(expires_at) = row.expires_at {
        if expires_at <= chrono::Utc::now().timestamp_millis() {
            return Err(authentication_error(
                ProxyErrorCode::ApiKeyExpiredError,
                format!("API key '{}' expired at {}", row.name, expires_at),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ApiKeyPosition, ProxyErrorCode, api_key_governance_error_to_proxy_error,
        check_system_api_key, classify_inactive_api_key_row, governance_error,
        parse_anthropic_api_key_from_headers,
    };
    use axum::http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
    use chrono::Utc;
    use std::{sync::Arc, time::Duration};

    use crate::config::SecretEncryptionConfig;
    use crate::database::TestDbContext;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload};
    use crate::schema::enum_def::Action;
    use crate::service::admin::AdminServices;
    use crate::service::app_state::create_test_app_state;
    use crate::service::secret_encryption::SecretEncryptionService;
    use crate::service::{cache::types::CacheApiKey, runtime::ApiKeyGovernanceAdmissionError};

    fn governance_cache_api_key() -> CacheApiKey {
        CacheApiKey {
            id: 42,
            api_key_hash: "hash".to_string(),
            key_prefix: "cyder-prefix".to_string(),
            key_last4: "1234".to_string(),
            name: "governed".to_string(),
            description: None,
            default_action: Action::Allow,
            is_enabled: true,
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            acl_rules: vec![],
        }
    }

    #[test]
    fn governance_reset_facts_map_to_proxy_hints_without_guessing_unknown_times() {
        let api_key = governance_cache_api_key();
        let rate_error = api_key_governance_error_to_proxy_error(
            &api_key,
            ApiKeyGovernanceAdmissionError::RateLimited {
                limit: 1,
                current: 1,
                retry_after: Duration::from_millis(1),
            },
        );
        assert_eq!(rate_error.code(), ProxyErrorCode::RateLimitError);
        assert_eq!(
            rate_error
                .response_hints()
                .retry_after()
                .map(|value| value.get()),
            Some(1)
        );

        let concurrency_error = api_key_governance_error_to_proxy_error(
            &api_key,
            ApiKeyGovernanceAdmissionError::ConcurrencyLimited {
                limit: 1,
                current: 1,
            },
        );
        assert_eq!(
            concurrency_error.response_hints().retry_after(),
            None,
            "concurrency recovery depends on another request completing"
        );

        let acl_error = governance_error(ProxyErrorCode::PermissionError, "denied by ACL");
        assert_eq!(
            acl_error.response_hints().retry_after(),
            None,
            "ACL rejection is a configuration fact, not a recovery timer"
        );
    }

    #[test]
    fn proxy_auth_anthropic_prefers_x_api_key_over_authorization() {
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
    fn proxy_auth_anthropic_falls_back_to_authorization_header() {
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
    fn proxy_auth_anthropic_rejects_invalid_authorization_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic abc"));

        let err = parse_anthropic_api_key_from_headers(&headers).unwrap_err();

        assert_eq!(err.code(), ProxyErrorCode::AuthenticationError);
    }

    #[test]
    fn proxy_auth_anthropic_requires_header_when_none_present() {
        let headers = HeaderMap::new();

        let err = parse_anthropic_api_key_from_headers(&headers).unwrap_err();

        assert_eq!(err.code(), ProxyErrorCode::AuthenticationError);
    }

    #[test]
    fn inactive_api_key_row_classifies_disabled_key() {
        let row = ApiKey {
            name: "disabled".to_string(),
            is_enabled: false,
            ..ApiKey::default()
        };

        let err = classify_inactive_api_key_row(&row).expect_err("disabled key should fail");

        assert_eq!(err.code(), ProxyErrorCode::ApiKeyDisabledError);
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

        assert_eq!(err.code(), ProxyErrorCode::ApiKeyExpiredError);
    }

    #[tokio::test]
    async fn proxy_auth_api_key_rotation_immediately_replaces_hash_authentication() {
        let database = TestDbContext::new_sqlite("proxy-api-key-rotation.sqlite");
        database
            .run_async(async {
                let base = create_test_app_state(database.clone()).await;
                let config: SecretEncryptionConfig = serde_yaml::from_str(
                    "downstream_mode: one_time\nencryption_key: '000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'\n",
                )
                .expect("one-time config should parse");
                let encryption = Arc::new(SecretEncryptionService::from_config(&config));
    let admin = Arc::new(AdminServices::new(
        Arc::clone(&base.catalog),
        Arc::clone(&encryption),
    ));
                let mut configured = (*base).clone();
                configured.admin = admin;
                configured.secret_encryption = encryption;
                let app_state = Arc::new(configured);

                let created = app_state
                    .admin
                    .api_key
                    .create_api_key(CreateApiKeyPayload {
                        name: "proxy-rotation".to_string(),
                        description: None,
                        default_action: Some(Action::Allow),
                        is_enabled: Some(true),
                        expires_at: None,
                        rate_limit_rpm: None,
                        max_concurrent_requests: None,
                        quota_daily_requests: None,
                        quota_daily_tokens: None,
                        quota_monthly_tokens: None,
                        budget_daily_nanos: None,
                        budget_daily_currency: None,
                        budget_monthly_nanos: None,
                        budget_monthly_currency: None,
                        acl_rules: None,
                    })
                    .await
                    .expect("proxy key should create");
                let old_secret = created.reveal.api_key;
                assert!(
                    check_system_api_key(
                        &app_state,
                        &old_secret,
                        ApiKeyPosition::AuthorizationHeader,
                    )
                    .await
                    .is_ok()
                );

                let rotated = app_state
                    .admin
                    .api_key
                    .rotate_api_key(created.detail.id)
                    .await
                    .expect("proxy key should rotate");
                let error = match check_system_api_key(
                    &app_state,
                    &old_secret,
                    ApiKeyPosition::AuthorizationHeader,
                )
                .await
                {
                    Err(error) => error,
                    Ok(_) => panic!("rotated API key must stop authenticating"),
                };
                assert_eq!(error.code(), ProxyErrorCode::AuthenticationError);
                assert!(
                    check_system_api_key(
                        &app_state,
                        &rotated.api_key,
                        ApiKeyPosition::AuthorizationHeader,
                    )
                    .await
                    .is_ok()
                );
            })
            .await;
    }
}
