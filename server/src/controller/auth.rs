use std::{
    net::IpAddr,
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use axum::{
    Extension, Json,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{
        HeaderMap, HeaderValue, StatusCode, Uri,
        header::{CONTENT_TYPE, COOKIE, ORIGIN, RETRY_AFTER, SET_COOKIE},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::ingress::client_identity::ClientIdentity;
use crate::service::admin::auth::{
    AccessTokenError, AuthTokenPair, BootstrapError, BootstrapStatus, BootstrapStatusError,
    LoginPasswordError, LoginPasswordResult, LogoutError, ManagerReauthCredential,
    ManagerSecretGovernanceReauth, ManagerTotpLifecycleResult, ManagerTotpPublicState,
    ManagerTotpSetup, ManagerTotpVerificationError, RotatePasswordError,
};
use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::utils::{
    HttpResult,
    auth::{
        AccessGuardError, ManagerAuthContext, ManagerMediatorContext, decode_mediator_token,
        validate_access_headers,
    },
};

const AUTH_BODY_LIMIT_BYTES: usize = 4 * 1024;
const MANAGER_AUTH_REQUEST_HEADER: &str = "x-cyder-manager-auth";
const MANAGER_AUTH_REQUEST_HEADER_VALUE: &str = "1";
const SEC_FETCH_SITE: &str = "sec-fetch-site";
const PRODUCTION_MEDIATOR_COOKIE: &str = "__Secure-cyder_manager_session";
const DEVELOPMENT_MEDIATOR_COOKIE: &str = "cyder_manager_session_dev";
pub(crate) const MANAGER_TOTP_CODE_HEADER: &str = "x-cyder-totp-code";
const MAX_CHALLENGE_INPUT_BYTES: usize = 36;
const MAX_RECOVERY_CODE_INPUT_BYTES: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordRequest {
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RotatePasswordRequest {
    current_password: String,
    new_password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginTotpRequest {
    login_challenge: String,
    totp_code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryStartRequest {
    password: String,
    recovery_code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryConfirmRequest {
    recovery_challenge: String,
    totp_code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentPasswordRequest {
    current_password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupConfirmRequest {
    setup_challenge: String,
    totp_code: String,
}

#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum ManagerReauthRequest {
    Password { password: String },
    Totp { totp_code: String },
}

#[derive(Serialize)]
struct BootstrapStatusResponse {
    state: &'static str,
}

#[derive(Serialize)]
struct ManagerReauthResponse {
    scope: &'static str,
    method: &'static str,
    verified_until: i64,
}

#[derive(Serialize)]
struct AuthAccessResponse {
    access_token: String,
    totp_state: &'static str,
    reauth: Option<ManagerReauthResponse>,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum LoginPasswordResponse {
    Authenticated {
        access_token: String,
        totp_state: &'static str,
        reauth: Option<ManagerReauthResponse>,
    },
    TotpRequired {
        login_challenge: String,
        expires_in: u64,
    },
}

#[derive(Serialize)]
struct ManagerTotpSetupResponse {
    setup_challenge: String,
    manual_secret: String,
    otpauth_uri: String,
    expires_in: u64,
}

#[derive(Serialize)]
struct ManagerTotpRecoverySetupResponse {
    recovery_challenge: String,
    manual_secret: String,
    otpauth_uri: String,
    expires_in: u64,
}

#[derive(Serialize)]
struct ManagerTotpLifecycleResponse {
    access_token: String,
    totp_state: &'static str,
    reauth: Option<ManagerReauthResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_codes: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ManagerTotpStatusResponse {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled_at: Option<i64>,
}

#[derive(Serialize)]
struct LogoutAllResponse {
    revoked_sessions: usize,
}

impl AuthAccessResponse {
    fn new(value: &AuthTokenPair, totp_state: ManagerTotpPublicState) -> Self {
        Self {
            access_token: value.access_token.clone(),
            totp_state: manager_totp_state_name(totp_state),
            reauth: value.reauth.map(manager_reauth_response),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootstrapStatusHttpError {
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootstrapHttpError {
    AlreadyInitialized,
    InvalidRequest,
    Busy,
    Unavailable,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginHttpError {
    Uninitialized,
    InvalidPassword,
    Busy,
    SourceRateLimited(u64),
    GlobalRateLimited(u64),
    ManagerTotpUnavailable,
    Unavailable,
    Storage,
    InvalidRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagerTotpHttpError {
    Required,
    Invalid,
    StepReplayed(u64),
    StepStale(u64),
    SourceRateLimited(u64),
    GlobalRateLimited(u64),
    ChallengeInvalidOrExpired,
    ChallengeAttemptsExhausted,
    Unavailable,
    StateConflict,
    CurrentPasswordInvalid,
    RecoveryCredentialsInvalid,
    Busy,
    Storage,
    RequestInvalid,
    ReauthRequired,
    ReauthMethodChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RotatePasswordHttpError {
    InvalidCurrentPassword,
    InvalidNewPassword,
    SamePassword,
    Busy,
    EpochConflict,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessHttpError {
    Invalid,
    Replay,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrowserBoundaryHttpError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogoutHttpError {
    InvalidCredential,
    Unavailable,
}

impl IntoResponse for BootstrapStatusHttpError {
    fn into_response(self) -> Response {
        auth_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            1404,
            "manager credential unavailable",
            false,
        )
    }
}

impl IntoResponse for BootstrapHttpError {
    fn into_response(self) -> Response {
        let (status, code, message, retry) = match self {
            Self::AlreadyInitialized => (
                StatusCode::CONFLICT,
                1401,
                "manager credential already initialized",
                false,
            ),
            Self::InvalidRequest => (
                StatusCode::UNPROCESSABLE_ENTITY,
                1402,
                "password does not satisfy policy",
                false,
            ),
            Self::Busy => (
                StatusCode::TOO_MANY_REQUESTS,
                1403,
                "password operation is busy",
                true,
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                1404,
                "manager credential unavailable",
                false,
            ),
            Self::Storage => (
                StatusCode::SERVICE_UNAVAILABLE,
                1405,
                "manager bootstrap unavailable",
                false,
            ),
        };
        auth_error_response(status, code, message, retry)
    }
}

impl IntoResponse for LoginHttpError {
    fn into_response(self) -> Response {
        let (status, code, message, retry_after) = match self {
            Self::Uninitialized => (
                StatusCode::CONFLICT,
                1411,
                "manager credential is not initialized",
                None,
            ),
            Self::InvalidPassword => (
                StatusCode::UNAUTHORIZED,
                1412,
                "invalid manager password",
                None,
            ),
            Self::Busy => (
                StatusCode::TOO_MANY_REQUESTS,
                1413,
                "manager login is temporarily limited",
                Some(1),
            ),
            Self::SourceRateLimited(retry_after) => (
                StatusCode::TOO_MANY_REQUESTS,
                1417,
                "manager login source is temporarily locked",
                Some(retry_after),
            ),
            Self::GlobalRateLimited(retry_after) => (
                StatusCode::TOO_MANY_REQUESTS,
                1418,
                "manager login anomaly protection is active",
                Some(retry_after),
            ),
            Self::ManagerTotpUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                1479,
                "manager TOTP unavailable",
                None,
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                1414,
                "manager credential unavailable",
                None,
            ),
            Self::Storage => (
                StatusCode::SERVICE_UNAVAILABLE,
                1415,
                "manager login session unavailable",
                None,
            ),
            Self::InvalidRequest => (
                StatusCode::UNPROCESSABLE_ENTITY,
                1416,
                "login request is invalid",
                None,
            ),
        };
        auth_error_response_with_retry_after(status, code, message, retry_after)
    }
}

impl IntoResponse for ManagerTotpHttpError {
    fn into_response(self) -> Response {
        let (status, code, message, retry_after) = match self {
            Self::Required => (
                StatusCode::PRECONDITION_REQUIRED,
                1471,
                "manager TOTP is required",
                None,
            ),
            Self::Invalid => (
                StatusCode::UNAUTHORIZED,
                1472,
                "manager TOTP is invalid",
                None,
            ),
            Self::StepReplayed(retry_after) => (
                StatusCode::CONFLICT,
                1473,
                "manager TOTP step was already used",
                Some(retry_after),
            ),
            Self::StepStale(retry_after) => (
                StatusCode::CONFLICT,
                1474,
                "manager TOTP step is stale",
                Some(retry_after),
            ),
            Self::SourceRateLimited(retry_after) => (
                StatusCode::TOO_MANY_REQUESTS,
                1475,
                "manager TOTP source is temporarily locked",
                Some(retry_after),
            ),
            Self::GlobalRateLimited(retry_after) => (
                StatusCode::TOO_MANY_REQUESTS,
                1476,
                "manager TOTP anomaly protection is active",
                Some(retry_after),
            ),
            Self::ChallengeInvalidOrExpired => (
                StatusCode::UNAUTHORIZED,
                1477,
                "manager TOTP challenge is invalid or expired",
                None,
            ),
            Self::ChallengeAttemptsExhausted => (
                StatusCode::TOO_MANY_REQUESTS,
                1478,
                "manager TOTP challenge attempts are exhausted",
                None,
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                1479,
                "manager TOTP unavailable",
                None,
            ),
            Self::StateConflict => (
                StatusCode::CONFLICT,
                1480,
                "manager TOTP state changed concurrently",
                None,
            ),
            Self::CurrentPasswordInvalid => (
                StatusCode::UNAUTHORIZED,
                1481,
                "current manager password is invalid",
                None,
            ),
            Self::RecoveryCredentialsInvalid => (
                StatusCode::UNAUTHORIZED,
                1482,
                "manager TOTP recovery credentials are invalid",
                None,
            ),
            Self::Busy => (
                StatusCode::TOO_MANY_REQUESTS,
                1483,
                "manager TOTP operation is busy",
                Some(1),
            ),
            Self::Storage => (
                StatusCode::SERVICE_UNAVAILABLE,
                1484,
                "manager TOTP storage unavailable",
                None,
            ),
            Self::RequestInvalid => (
                StatusCode::UNPROCESSABLE_ENTITY,
                1485,
                "manager TOTP request is invalid",
                None,
            ),
            Self::ReauthRequired => (
                StatusCode::FORBIDDEN,
                1491,
                "manager secret governance reauthentication is required",
                None,
            ),
            Self::ReauthMethodChanged => (
                StatusCode::CONFLICT,
                1492,
                "manager reauthentication method changed",
                None,
            ),
        };
        auth_error_response_with_retry_after(status, code, message, retry_after)
    }
}

impl IntoResponse for RotatePasswordHttpError {
    fn into_response(self) -> Response {
        let (status, code, message, retry) = match self {
            Self::InvalidCurrentPassword => (
                StatusCode::UNAUTHORIZED,
                1421,
                "current password is invalid",
                false,
            ),
            Self::InvalidNewPassword => (
                StatusCode::UNPROCESSABLE_ENTITY,
                1422,
                "new password does not satisfy policy",
                false,
            ),
            Self::SamePassword => (
                StatusCode::UNPROCESSABLE_ENTITY,
                1423,
                "new password must differ from current password",
                false,
            ),
            Self::Busy => (
                StatusCode::TOO_MANY_REQUESTS,
                1424,
                "password operation is busy",
                true,
            ),
            Self::EpochConflict => (
                StatusCode::CONFLICT,
                1425,
                "manager credential changed concurrently",
                false,
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                1426,
                "password rotation unavailable",
                false,
            ),
        };
        auth_error_response(status, code, message, retry)
    }
}

impl IntoResponse for AccessHttpError {
    fn into_response(self) -> Response {
        let (code, message) = match self {
            Self::Invalid => (1441, "manager browser session invalid or expired"),
            Self::Unavailable => (1443, "manager access temporarily unavailable"),
            Self::Replay => (1444, "manager session integrity failure"),
        };
        let status = if matches!(self, Self::Unavailable) {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::UNAUTHORIZED
        };
        auth_error_response(status, code, message, false)
    }
}

impl IntoResponse for BrowserBoundaryHttpError {
    fn into_response(self) -> Response {
        auth_error_response(
            StatusCode::FORBIDDEN,
            1461,
            "manager auth browser request rejected",
            false,
        )
    }
}

impl IntoResponse for LogoutHttpError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::InvalidCredential => (
                StatusCode::UNAUTHORIZED,
                1433,
                "manager credential no longer valid",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                1451,
                "manager logout unavailable",
            ),
        };
        auth_error_response(status, code, message, false)
    }
}

fn auth_error_response(
    status: StatusCode,
    code: u16,
    message: &'static str,
    retry_after: bool,
) -> Response {
    auth_error_response_with_retry_after(status, code, message, retry_after.then_some(1))
}

fn auth_error_response_with_retry_after(
    status: StatusCode,
    code: u16,
    message: &'static str,
    retry_after: Option<u64>,
) -> Response {
    let mut response = (status, Json(json!({ "code": code, "msg": message }))).into_response();
    if let Some(retry_after) = retry_after {
        if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
            response.headers_mut().insert(RETRY_AFTER, value);
        }
    }
    response
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediatorCookiePolicy {
    name: &'static str,
    path: String,
    secure: bool,
}

fn configured_origin_is_production(browser_origin: Option<&str>) -> bool {
    browser_origin
        .and_then(|origin| origin.parse::<Uri>().ok())
        .and_then(|uri| uri.host().map(str::to_string))
        .is_some_and(|host| !is_loopback_host(&host))
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn mediator_cookie_policy_for(
    browser_origin: Option<&str>,
    base_path: &str,
) -> MediatorCookiePolicy {
    let secure = configured_origin_is_production(browser_origin);
    MediatorCookiePolicy {
        name: if secure {
            PRODUCTION_MEDIATOR_COOKIE
        } else {
            DEVELOPMENT_MEDIATOR_COOKIE
        },
        path: format!("{}/manager/api/auth", base_path.trim_end_matches('/')),
        secure,
    }
}

fn mediator_cookie_policy_for_app(app_state: &AppState) -> MediatorCookiePolicy {
    mediator_cookie_policy_for(
        app_state.manager_auth_browser_origin.as_deref(),
        &app_state.base_path,
    )
}

fn mediator_cookie_value_for(
    policy: &MediatorCookiePolicy,
    token: &str,
    expires_at: i64,
) -> HeaderValue {
    let now = crate::utils::auth::get_current_timestamp();
    let max_age = expires_at.saturating_sub(now).max(0);
    let expires = UNIX_EPOCH + Duration::from_secs(expires_at.max(0) as u64);
    let secure = if policy.secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{}={token}; Path={}; Max-Age={max_age}; Expires={}; HttpOnly; SameSite=Strict{secure}",
        policy.name,
        policy.path,
        httpdate::fmt_http_date(expires),
    ))
    .expect("manager mediator cookie fields are header-safe")
}

fn mediator_cookie_value(app_state: &AppState, token: &str, expires_at: i64) -> HeaderValue {
    mediator_cookie_value_for(
        &mediator_cookie_policy_for_app(app_state),
        token,
        expires_at,
    )
}

fn deleted_mediator_cookie_value_for(policy: &MediatorCookiePolicy) -> HeaderValue {
    let secure = if policy.secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{}=; Path={}; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT; HttpOnly; SameSite=Strict{secure}",
        policy.name,
        policy.path,
    ))
    .expect("manager mediator cookie deletion fields are header-safe")
}

fn delete_mediator_cookie_for(app_state: &AppState, headers: &mut HeaderMap) {
    headers.append(
        SET_COOKIE,
        deleted_mediator_cookie_value_for(&mediator_cookie_policy_for_app(app_state)),
    );
}

fn token_response(
    app_state: &AppState,
    pair: AuthTokenPair,
    totp_state: ManagerTotpPublicState,
) -> Response {
    let mut response = HttpResult::new(AuthAccessResponse::new(&pair, totp_state)).into_response();
    response.headers_mut().append(
        SET_COOKIE,
        mediator_cookie_value(app_state, &pair.mediator_token, pair.mediator_expires_at),
    );
    response
}

fn access_response(
    access_token: String,
    totp_state: ManagerTotpPublicState,
    reauth: Option<ManagerSecretGovernanceReauth>,
) -> Response {
    HttpResult::new(AuthAccessResponse {
        access_token,
        totp_state: manager_totp_state_name(totp_state),
        reauth: reauth.map(manager_reauth_response),
    })
    .into_response()
}

fn login_password_authenticated_response(
    app_state: &AppState,
    pair: AuthTokenPair,
    totp_state: ManagerTotpPublicState,
) -> Response {
    let mut response = HttpResult::new(LoginPasswordResponse::Authenticated {
        access_token: pair.access_token.clone(),
        totp_state: manager_totp_state_name(totp_state),
        reauth: pair.reauth.map(manager_reauth_response),
    })
    .into_response();
    response.headers_mut().append(
        SET_COOKIE,
        mediator_cookie_value(app_state, &pair.mediator_token, pair.mediator_expires_at),
    );
    response
}

fn login_password_challenge_response(
    app_state: &AppState,
    challenge: crate::service::admin::auth::ManagerLoginTotpChallenge,
) -> Response {
    let mut response = HttpResult::new(LoginPasswordResponse::TotpRequired {
        login_challenge: challenge.challenge.to_string(),
        expires_in: challenge.expires_in,
    })
    .into_response();
    delete_mediator_cookie_for(app_state, response.headers_mut());
    response
}

fn manager_totp_setup_response(setup: ManagerTotpSetup) -> ManagerTotpSetupResponse {
    ManagerTotpSetupResponse {
        setup_challenge: setup.challenge.to_string(),
        manual_secret: setup.manual_secret.expose().to_string(),
        otpauth_uri: setup.otpauth_uri.expose().to_string(),
        expires_in: setup.expires_in,
    }
}

fn manager_totp_recovery_setup_response(
    setup: ManagerTotpSetup,
) -> ManagerTotpRecoverySetupResponse {
    ManagerTotpRecoverySetupResponse {
        recovery_challenge: setup.challenge.to_string(),
        manual_secret: setup.manual_secret.expose().to_string(),
        otpauth_uri: setup.otpauth_uri.expose().to_string(),
        expires_in: setup.expires_in,
    }
}

fn manager_totp_lifecycle_response(
    app_state: &AppState,
    result: ManagerTotpLifecycleResult,
) -> Response {
    let recovery_codes = result
        .recovery_codes
        .as_ref()
        .map(|codes| codes.expose().map(str::to_string).collect::<Vec<String>>());
    let mut response = HttpResult::new(ManagerTotpLifecycleResponse {
        access_token: result.tokens.access_token.clone(),
        totp_state: manager_totp_state_name(result.state),
        reauth: result.tokens.reauth.map(manager_reauth_response),
        recovery_codes,
    })
    .into_response();
    response.headers_mut().append(
        SET_COOKIE,
        mediator_cookie_value(
            app_state,
            &result.tokens.mediator_token,
            result.tokens.mediator_expires_at,
        ),
    );
    response
}

fn manager_reauth_response(reauth: ManagerSecretGovernanceReauth) -> ManagerReauthResponse {
    ManagerReauthResponse {
        scope: "secret_governance",
        method: reauth.evidence.as_str(),
        verified_until: reauth.verified_until,
    }
}

fn manager_totp_state_name(state: ManagerTotpPublicState) -> &'static str {
    match state {
        ManagerTotpPublicState::Disabled => "disabled",
        ManagerTotpPublicState::Enabled { .. } => "enabled",
        ManagerTotpPublicState::Unavailable { .. } => "unavailable",
    }
}

fn manager_totp_status_response(state: ManagerTotpPublicState) -> ManagerTotpStatusResponse {
    let enabled_at = match state {
        ManagerTotpPublicState::Disabled => None,
        ManagerTotpPublicState::Enabled { enabled_at } => Some(enabled_at),
        ManagerTotpPublicState::Unavailable { enabled_at } => enabled_at,
    };
    ManagerTotpStatusResponse {
        state: manager_totp_state_name(state),
        enabled_at,
    }
}

pub(crate) fn required_manager_totp_code(headers: &HeaderMap) -> Result<&str, Response> {
    optional_manager_totp_code(headers)?
        .ok_or_else(|| ManagerTotpHttpError::Required.into_response())
}

fn optional_manager_totp_code(headers: &HeaderMap) -> Result<Option<&str>, Response> {
    let mut values = headers.get_all(MANAGER_TOTP_CODE_HEADER).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(ManagerTotpHttpError::RequestInvalid.into_response());
    }
    value
        .to_str()
        .map(Some)
        .map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())
}

pub(crate) fn manager_totp_error_response(error: ManagerTotpVerificationError) -> Response {
    ManagerTotpHttpError::from(error).into_response()
}

pub(crate) fn authorize_secret_governance_command(
    app_state: &AppState,
    auth_context: &ManagerAuthContext,
) -> Result<(), Response> {
    app_state
        .admin
        .auth
        .authorize_secret_governance(auth_context)
        .map(|_| ())
        .map_err(manager_totp_error_response)
}

fn parse_manager_totp_challenge(value: &str) -> Result<Uuid, Response> {
    if value.len() > MAX_CHALLENGE_INPUT_BYTES {
        return Err(ManagerTotpHttpError::RequestInvalid.into_response());
    }
    Uuid::parse_str(value).map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())
}

fn extract_mediator_context(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<ManagerMediatorContext, AccessHttpError> {
    let cookie_name = mediator_cookie_policy_for_app(app_state).name;
    let mut token = None;
    for header_value in headers.get_all(COOKIE) {
        let raw = header_value
            .to_str()
            .map_err(|_| AccessHttpError::Invalid)?;
        for cookie in raw.split(';') {
            let Some((name, value)) = cookie.trim().split_once('=') else {
                continue;
            };
            if name == cookie_name {
                if token.replace(value).is_some() || value.is_empty() {
                    return Err(AccessHttpError::Invalid);
                }
            }
        }
    }
    decode_mediator_token(token.ok_or(AccessHttpError::Invalid)?)
        .map_err(|_| AccessHttpError::Invalid)
}

fn browser_origin_allowed(
    headers: &HeaderMap,
    client_identity: &ClientIdentity,
    configured_browser_origin: Option<&str>,
) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    if let Some(configured_origin) = configured_browser_origin {
        if origin != configured_origin {
            return false;
        }
        if configured_origin.starts_with("http://") && !client_identity.peer_addr.ip().is_loopback()
        {
            return false;
        }
    } else {
        let Ok(uri) = origin.parse::<Uri>() else {
            return false;
        };
        let (Some(scheme), Some(authority), Some(host)) =
            (uri.scheme_str(), uri.authority(), uri.host())
        else {
            return false;
        };
        if !matches!(scheme, "http" | "https")
            || origin != format!("{scheme}://{authority}")
            || !client_identity.peer_addr.ip().is_loopback()
            || !is_loopback_host(host)
        {
            return false;
        }
    }

    headers
        .get(SEC_FETCH_SITE)
        .and_then(|value| value.to_str().ok())
        == Some("same-origin")
        && headers
            .get(MANAGER_AUTH_REQUEST_HEADER)
            .and_then(|value| value.to_str().ok())
            == Some(MANAGER_AUTH_REQUEST_HEADER_VALUE)
        && headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|media_type| media_type.trim() == "application/json")
            })
}

async fn manager_auth_browser_boundary(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    request: Request,
    next: Next,
) -> Result<Response, BrowserBoundaryHttpError> {
    if request.method() != axum::http::Method::POST {
        return Ok(next.run(request).await);
    }
    if !browser_origin_allowed(
        request.headers(),
        &client_identity,
        app_state.manager_auth_browser_origin.as_deref(),
    ) {
        return Err(BrowserBoundaryHttpError);
    }
    Ok(next.run(request).await)
}

async fn bootstrap_status(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<BootstrapStatusResponse>, BootstrapStatusHttpError> {
    let state = app_state
        .admin
        .auth
        .bootstrap_status()
        .map_err(|BootstrapStatusError::Unavailable| BootstrapStatusHttpError::Unavailable)?;
    let state = match state {
        BootstrapStatus::Uninitialized => "uninitialized",
        BootstrapStatus::Ready => "ready",
    };
    Ok(HttpResult::new(BootstrapStatusResponse { state }))
}

async fn bootstrap(
    State(app_state): State<Arc<AppState>>,
    request: Result<Json<PasswordRequest>, JsonRejection>,
) -> Result<Response, BootstrapHttpError> {
    let Json(request) = request.map_err(|_| BootstrapHttpError::InvalidRequest)?;
    app_state
        .admin
        .auth
        .bootstrap(&request.password)
        .await
        .map_err(BootstrapHttpError::from)
        .map(|tokens| token_response(&app_state, tokens, ManagerTotpPublicState::Disabled))
}

async fn login_password(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    request: Result<Json<PasswordRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let Json(request) = request.map_err(|_| LoginHttpError::InvalidRequest.into_response())?;
    match app_state
        .admin
        .auth
        .login_password(client_identity.client_ip, &request.password)
        .await
        .map_err(|error| LoginHttpError::from(error).into_response())?
    {
        LoginPasswordResult::Authenticated(tokens) => Ok(login_password_authenticated_response(
            &app_state,
            tokens,
            ManagerTotpPublicState::Disabled,
        )),
        LoginPasswordResult::TotpRequired(challenge) => {
            Ok(login_password_challenge_response(&app_state, challenge))
        }
    }
}

async fn login_totp(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    request: Result<Json<LoginTotpRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    let login_challenge = parse_manager_totp_challenge(&request.login_challenge)?;
    let tokens = app_state
        .admin
        .auth
        .login_totp(
            client_identity.client_ip,
            login_challenge,
            &request.totp_code,
        )
        .await
        .map_err(manager_totp_error_response)?;
    let state = app_state
        .admin
        .auth
        .totp_status()
        .map_err(manager_totp_error_response)?;
    if !matches!(state, ManagerTotpPublicState::Enabled { .. }) {
        return Err(ManagerTotpHttpError::StateConflict.into_response());
    }
    Ok(token_response(&app_state, tokens, state))
}

async fn reauthenticate(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    headers: HeaderMap,
    request: Result<Json<ManagerReauthRequest>, JsonRejection>,
) -> Result<HttpResult<ManagerReauthResponse>, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    let reauth = match request {
        ManagerReauthRequest::Password { password } => {
            app_state
                .admin
                .auth
                .reauthenticate_secret_governance(
                    &auth_context,
                    client_identity.client_ip,
                    ManagerReauthCredential::Password(&password),
                )
                .await
        }
        ManagerReauthRequest::Totp { totp_code } => {
            app_state
                .admin
                .auth
                .reauthenticate_secret_governance(
                    &auth_context,
                    client_identity.client_ip,
                    ManagerReauthCredential::Totp(&totp_code),
                )
                .await
        }
    }
    .map_err(manager_totp_error_response)?;
    Ok(HttpResult::new(manager_reauth_response(reauth)))
}

async fn recovery_start(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    request: Result<Json<RecoveryStartRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    if request.recovery_code.len() > MAX_RECOVERY_CODE_INPUT_BYTES {
        return Err(ManagerTotpHttpError::RequestInvalid.into_response());
    }
    let setup = app_state
        .admin
        .auth
        .start_totp_recovery(
            client_identity.client_ip,
            &request.password,
            &request.recovery_code,
        )
        .await
        .map_err(manager_totp_error_response)?;
    let mut response = HttpResult::new(manager_totp_recovery_setup_response(setup)).into_response();
    delete_mediator_cookie_for(&app_state, response.headers_mut());
    Ok(response)
}

async fn recovery_confirm(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    request: Result<Json<RecoveryConfirmRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    let recovery_challenge = parse_manager_totp_challenge(&request.recovery_challenge)?;
    app_state
        .admin
        .auth
        .confirm_totp_recovery(
            client_identity.client_ip,
            recovery_challenge,
            &request.totp_code,
        )
        .await
        .map(|result| manager_totp_lifecycle_response(&app_state, result))
        .map_err(manager_totp_error_response)
}

async fn totp_status(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<HttpResult<ManagerTotpStatusResponse>, Response> {
    require_access_and_mediator(&app_state, &headers)?;
    app_state
        .admin
        .auth
        .totp_status()
        .map(manager_totp_status_response)
        .map(HttpResult::new)
        .map_err(manager_totp_error_response)
}

async fn totp_enroll_start(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    headers: HeaderMap,
    request: Result<Json<CurrentPasswordRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    app_state
        .admin
        .auth
        .start_totp_enrollment(
            &auth_context,
            client_identity.client_ip,
            &request.current_password,
        )
        .await
        .map(manager_totp_setup_response)
        .map(HttpResult::new)
        .map(IntoResponse::into_response)
        .map_err(manager_totp_error_response)
}

async fn totp_enroll_confirm(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    headers: HeaderMap,
    request: Result<Json<SetupConfirmRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    let setup_challenge = parse_manager_totp_challenge(&request.setup_challenge)?;
    app_state
        .admin
        .auth
        .confirm_totp_enrollment(
            &auth_context,
            client_identity.client_ip,
            setup_challenge,
            &request.totp_code,
        )
        .await
        .map(|result| manager_totp_lifecycle_response(&app_state, result))
        .map_err(manager_totp_error_response)
}

async fn totp_replace_start(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    headers: HeaderMap,
    request: Result<Json<CurrentPasswordRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    let totp_code = required_manager_totp_code(&headers)?;
    app_state
        .admin
        .auth
        .start_totp_replacement(
            &auth_context,
            client_identity.client_ip,
            &request.current_password,
            totp_code,
        )
        .await
        .map(manager_totp_setup_response)
        .map(HttpResult::new)
        .map(IntoResponse::into_response)
        .map_err(manager_totp_error_response)
}

async fn totp_replace_confirm(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    headers: HeaderMap,
    request: Result<Json<SetupConfirmRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    let setup_challenge = parse_manager_totp_challenge(&request.setup_challenge)?;
    app_state
        .admin
        .auth
        .confirm_totp_replacement(
            &auth_context,
            client_identity.client_ip,
            setup_challenge,
            &request.totp_code,
        )
        .await
        .map(|result| manager_totp_lifecycle_response(&app_state, result))
        .map_err(manager_totp_error_response)
}

async fn totp_disable(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    headers: HeaderMap,
    request: Result<Json<CurrentPasswordRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    let totp_code = required_manager_totp_code(&headers)?;
    app_state
        .admin
        .auth
        .disable_totp(
            &auth_context,
            client_identity.client_ip,
            &request.current_password,
            totp_code,
        )
        .await
        .map(|result| manager_totp_lifecycle_response(&app_state, result))
        .map_err(manager_totp_error_response)
}

async fn rotate_password(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    headers: HeaderMap,
    request: Result<Json<RotatePasswordRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let Json(request) =
        request.map_err(|_| RotatePasswordHttpError::InvalidNewPassword.into_response())?;
    let totp_code = optional_manager_totp_code(&headers)?;
    app_state
        .admin
        .auth
        .rotate_password(
            &auth_context,
            client_identity.client_ip,
            totp_code,
            &request.current_password,
            &request.new_password,
        )
        .await
        .map_err(|error| rotate_password_error_response(&app_state, error))
        .and_then(|tokens| {
            app_state
                .admin
                .auth
                .totp_status()
                .map(|state| token_response(&app_state, tokens, state))
                .map_err(manager_totp_error_response)
        })
}

async fn access(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let mediator = extract_mediator_context(&app_state, &headers)
        .map_err(|error| access_error_response(&app_state, error))?;
    let access_token = app_state
        .admin
        .auth
        .access_for_session(mediator.login_instance_id, mediator.credential_epoch)
        .await
        .map_err(AccessHttpError::from)
        .map_err(|error| access_error_response(&app_state, error))?;
    let totp_state = app_state
        .admin
        .auth
        .totp_status()
        .map_err(|_| access_error_response(&app_state, AccessHttpError::Unavailable))?;
    let reauth = if matches!(totp_state, ManagerTotpPublicState::Unavailable { .. }) {
        None
    } else {
        app_state
            .admin
            .auth
            .secret_governance_reauth_for_session(
                mediator.login_instance_id,
                mediator.credential_epoch,
            )
            .map_err(AccessHttpError::from)
            .map_err(|error| access_error_response(&app_state, error))?
    };
    Ok(access_response(access_token, totp_state, reauth))
}

async fn logout(State(app_state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Ok(mediator) = extract_mediator_context(&app_state, &headers) else {
        return logout_success_response(&app_state);
    };
    match app_state
        .admin
        .auth
        .logout_session(mediator.login_instance_id, mediator.credential_epoch)
        .await
    {
        Ok(()) => logout_success_response(&app_state),
        Err(error) => {
            let mut response = LogoutHttpError::from(error).into_response();
            delete_mediator_cookie_for(&app_state, response.headers_mut());
            response
        }
    }
}

async fn logout_all(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    headers: HeaderMap,
    request: Result<Json<CurrentPasswordRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let Json(request) =
        request.map_err(|_| ManagerTotpHttpError::RequestInvalid.into_response())?;
    let totp_code = optional_manager_totp_code(&headers)?;
    let revoked_sessions = app_state
        .admin
        .auth
        .logout_all(
            &auth_context,
            client_identity.client_ip,
            &request.current_password,
            totp_code,
        )
        .await
        .map_err(manager_totp_error_response)?;
    let mut response = HttpResult::new(LogoutAllResponse { revoked_sessions }).into_response();
    delete_mediator_cookie_for(&app_state, response.headers_mut());
    Ok(response)
}

fn require_access_and_mediator(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<(ManagerAuthContext, ManagerMediatorContext), Response> {
    let mediator = extract_mediator_context(app_state, headers)
        .map_err(|error| access_error_response(app_state, error))?;
    let access = validate_access_headers(app_state, headers).map_err(|error| {
        let clear_cookie = matches!(
            error,
            AccessGuardError::CredentialMismatch | AccessGuardError::SessionInvalid
        );
        let mut response = error.into_response();
        if clear_cookie {
            delete_mediator_cookie_for(app_state, response.headers_mut());
        }
        response
    })?;
    if access.login_instance_id != mediator.login_instance_id
        || access.credential_epoch != mediator.credential_epoch
        || access.manager_id != mediator.manager_id
        || access.manager_subject != mediator.manager_subject
    {
        return Err(access_error_response(app_state, AccessHttpError::Invalid));
    }
    Ok((access, mediator))
}

fn access_error_response(app_state: &AppState, error: AccessHttpError) -> Response {
    let clear_cookie = !matches!(error, AccessHttpError::Unavailable);
    let mut response = error.into_response();
    if clear_cookie {
        delete_mediator_cookie_for(app_state, response.headers_mut());
    }
    response
}

fn rotate_password_error_response(app_state: &AppState, error: RotatePasswordError) -> Response {
    if let RotatePasswordError::Totp(error) = error {
        return manager_totp_error_response(error);
    }
    let clear_cookie = matches!(error, RotatePasswordError::EpochConflict);
    let http_error = match error {
        RotatePasswordError::InvalidCurrentPassword => {
            RotatePasswordHttpError::InvalidCurrentPassword
        }
        RotatePasswordError::PasswordPolicy(_) => RotatePasswordHttpError::InvalidNewPassword,
        RotatePasswordError::SamePassword => RotatePasswordHttpError::SamePassword,
        RotatePasswordError::Busy => RotatePasswordHttpError::Busy,
        RotatePasswordError::EpochConflict => RotatePasswordHttpError::EpochConflict,
        RotatePasswordError::Unavailable | RotatePasswordError::Storage => {
            RotatePasswordHttpError::Unavailable
        }
        RotatePasswordError::Totp(_) => unreachable!("handled above"),
    };
    let mut response = http_error.into_response();
    if clear_cookie {
        delete_mediator_cookie_for(app_state, response.headers_mut());
    }
    response
}

fn logout_success_response(app_state: &AppState) -> Response {
    let mut response = HttpResult::new(()).into_response();
    delete_mediator_cookie_for(app_state, response.headers_mut());
    response
}

impl From<BootstrapError> for BootstrapHttpError {
    fn from(error: BootstrapError) -> Self {
        match error {
            BootstrapError::AlreadyInitialized => Self::AlreadyInitialized,
            BootstrapError::PasswordPolicy(_) => Self::InvalidRequest,
            BootstrapError::Busy => Self::Busy,
            BootstrapError::Unavailable => Self::Unavailable,
            BootstrapError::Storage => Self::Storage,
        }
    }
}

impl From<LoginPasswordError> for LoginHttpError {
    fn from(error: LoginPasswordError) -> Self {
        match error {
            LoginPasswordError::Uninitialized => Self::Uninitialized,
            LoginPasswordError::InvalidPassword => Self::InvalidPassword,
            LoginPasswordError::SourceRateLimited { retry_after } => {
                Self::SourceRateLimited(retry_after)
            }
            LoginPasswordError::GlobalRateLimited { retry_after } => {
                Self::GlobalRateLimited(retry_after)
            }
            LoginPasswordError::Busy => Self::Busy,
            LoginPasswordError::ManagerTotpUnavailable => Self::ManagerTotpUnavailable,
            LoginPasswordError::Unavailable => Self::Unavailable,
            LoginPasswordError::Storage => Self::Storage,
        }
    }
}

impl From<ManagerTotpVerificationError> for ManagerTotpHttpError {
    fn from(error: ManagerTotpVerificationError) -> Self {
        match error {
            ManagerTotpVerificationError::Required => Self::Required,
            ManagerTotpVerificationError::Invalid => Self::Invalid,
            ManagerTotpVerificationError::StepReplayed { retry_after } => {
                Self::StepReplayed(retry_after)
            }
            ManagerTotpVerificationError::StepStale { retry_after } => Self::StepStale(retry_after),
            ManagerTotpVerificationError::SourceRateLimited { retry_after } => {
                Self::SourceRateLimited(retry_after)
            }
            ManagerTotpVerificationError::GlobalRateLimited { retry_after } => {
                Self::GlobalRateLimited(retry_after)
            }
            ManagerTotpVerificationError::ChallengeInvalidOrExpired => {
                Self::ChallengeInvalidOrExpired
            }
            ManagerTotpVerificationError::ChallengeAttemptsExhausted => {
                Self::ChallengeAttemptsExhausted
            }
            ManagerTotpVerificationError::Unavailable => Self::Unavailable,
            ManagerTotpVerificationError::StateConflict => Self::StateConflict,
            ManagerTotpVerificationError::CurrentPasswordInvalid => Self::CurrentPasswordInvalid,
            ManagerTotpVerificationError::RecoveryCredentialsInvalid => {
                Self::RecoveryCredentialsInvalid
            }
            ManagerTotpVerificationError::Busy => Self::Busy,
            ManagerTotpVerificationError::Storage => Self::Storage,
            ManagerTotpVerificationError::RequestInvalid => Self::RequestInvalid,
            ManagerTotpVerificationError::ReauthRequired => Self::ReauthRequired,
            ManagerTotpVerificationError::ReauthMethodChanged => Self::ReauthMethodChanged,
        }
    }
}

impl From<AccessTokenError> for AccessHttpError {
    fn from(error: AccessTokenError) -> Self {
        match error {
            AccessTokenError::Invalid => Self::Invalid,
            AccessTokenError::Replay => Self::Replay,
            AccessTokenError::Unavailable | AccessTokenError::Storage => Self::Unavailable,
        }
    }
}

impl From<LogoutError> for LogoutHttpError {
    fn from(error: LogoutError) -> Self {
        match error {
            LogoutError::InvalidCredential => Self::InvalidCredential,
            LogoutError::Unavailable | LogoutError::Storage => Self::Unavailable,
        }
    }
}

pub fn create_auth_router(app_state: Arc<AppState>) -> StateRouter {
    create_state_router().nest(
        "/auth",
        create_state_router()
            .route("/bootstrap/status", get(bootstrap_status))
            .route("/bootstrap", post(bootstrap))
            .route("/login/password", post(login_password))
            .route("/login/totp", post(login_totp))
            .route("/reauth", post(reauthenticate))
            .route("/recovery/start", post(recovery_start))
            .route("/recovery/confirm", post(recovery_confirm))
            .route("/totp/status", get(totp_status))
            .route("/totp/enroll/start", post(totp_enroll_start))
            .route("/totp/enroll/confirm", post(totp_enroll_confirm))
            .route("/totp/replace/start", post(totp_replace_start))
            .route("/totp/replace/confirm", post(totp_replace_confirm))
            .route("/totp/disable", post(totp_disable))
            .route("/access", post(access))
            .route("/password/rotate", post(rotate_password))
            .route("/logout", post(logout))
            .route("/logout_all", post(logout_all))
            .layer(middleware::from_fn_with_state(
                app_state,
                manager_auth_browser_boundary,
            ))
            .layer(DefaultBodyLimit::max(AUTH_BODY_LIMIT_BYTES)),
    )
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    use axum::{
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::{Method, Request, StatusCode, header},
        response::IntoResponse,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::{
        config::{ClientIdentityConfig, SecretEncryptionConfig},
        controller::create_manager_router,
        database::{
            DbConnection, TestDbContext, get_connection,
            manager_auth_instance::ManagerAuthInstance,
            manager_credential::{ManagerCredential, NewManagerCredential},
            manager_totp_recovery_code::ManagerTotpRecoveryCode,
        },
        ingress::client_identity::{ClientIdentity, ClientIdentityResolver, ClientIdentitySource},
        service::{
            admin::AdminServices,
            admin::auth::{
                ManagerAuthService, SECRET_GOVERNANCE_REAUTH_TTL_SEC,
                totp::generate_manager_totp_code,
            },
            app_state::{AppState, create_test_app_state},
            secret_encryption::{SecretEncryptionService, SensitiveSecret},
        },
        utils::auth::{
            AccessGuardError, decode_access_token, generate_token_jti, get_current_timestamp,
            issue_access_token,
        },
    };

    use super::{
        AUTH_BODY_LIMIT_BYTES, AccessHttpError, BootstrapHttpError, LoginHttpError,
        LogoutHttpError, MANAGER_TOTP_CODE_HEADER, ManagerTotpHttpError, RotatePasswordHttpError,
        create_auth_router, deleted_mediator_cookie_value_for, mediator_cookie_policy_for,
        mediator_cookie_value_for,
    };
    use diesel::RunQueryDsl;

    const INITIAL_PASSWORD: &str = "correct horse battery staple";
    const ROTATED_PASSWORD: &str = "correct horse battery staple rotated";
    const TEST_SECRET_ENCRYPTION_KEY: &str =
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    struct EnrolledHttpManager {
        access_token: String,
        cookie: String,
        manual_secret: String,
        recovery_codes: Vec<String>,
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        send_from(
            app_state,
            request,
            SocketAddr::from(([127, 0, 0, 1], 31_000)),
        )
        .await
    }

    async fn send_from(
        app_state: &Arc<AppState>,
        mut request: Request<Body>,
        peer_addr: SocketAddr,
    ) -> axum::response::Response {
        request.extensions_mut().insert(ClientIdentity {
            client_ip: peer_addr.ip(),
            peer_addr,
            source: ClientIdentitySource::TcpPeer,
            trusted_proxy_hops: 0,
        });
        create_auth_router(Arc::clone(app_state))
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("auth router should respond")
    }

    async fn send_manager(
        app_state: &Arc<AppState>,
        request: Request<Body>,
    ) -> axum::response::Response {
        send_manager_with_resolver(
            app_state,
            request,
            Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig::default())),
            SocketAddr::from(([127, 0, 0, 1], 31_000)),
        )
        .await
    }

    async fn send_manager_with_resolver(
        app_state: &Arc<AppState>,
        mut request: Request<Body>,
        resolver: Arc<ClientIdentityResolver>,
        peer_addr: SocketAddr,
    ) -> axum::response::Response {
        request.extensions_mut().insert(ConnectInfo(peer_addr));
        create_manager_router(Arc::clone(app_state), resolver)
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("manager router should respond")
    }

    fn json_request(method: Method, uri: &str, payload: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:29528")
            .header("sec-fetch-site", "same-origin")
            .header("x-cyder-manager-auth", "1")
            .body(Body::from(
                serde_json::to_vec(&payload).expect("payload should serialize"),
            ))
            .expect("request should build")
    }

    fn empty_request(method: Method, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:29528")
            .header("sec-fetch-site", "same-origin")
            .header("x-cyder-manager-auth", "1")
            .body(Body::from("{}"))
            .expect("request should build")
    }

    fn auth_request(method: Method, uri: &str, token: &str, payload: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:29528")
            .header("sec-fetch-site", "same-origin")
            .header("x-cyder-manager-auth", "1")
            .body(Body::from(
                serde_json::to_vec(&payload).expect("payload should serialize"),
            ))
            .expect("request should build")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

    fn access_token(body: &Value) -> String {
        assert!(
            body["data"].get("refresh_token").is_none(),
            "browser JSON must never contain a refresh token"
        );
        body["data"]["access_token"]
            .as_str()
            .expect("access token should exist")
            .to_string()
    }

    fn mediator_cookie(response: &axum::response::Response) -> String {
        response
            .headers()
            .get(header::SET_COOKIE)
            .expect("mediator Set-Cookie should exist")
            .to_str()
            .expect("Set-Cookie should be text")
            .split(';')
            .next()
            .expect("cookie pair should exist")
            .to_string()
    }

    fn cookie_request(
        method: Method,
        uri: &str,
        cookie: &str,
        access_token: Option<&str>,
        payload: Value,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:29528")
            .header("sec-fetch-site", "same-origin")
            .header("x-cyder-manager-auth", "1")
            .header(header::COOKIE, cookie);
        if let Some(access_token) = access_token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {access_token}"));
        }
        builder
            .body(Body::from(
                serde_json::to_vec(&payload).expect("payload should serialize"),
            ))
            .expect("cookie request should build")
    }

    fn with_totp_header(mut request: Request<Body>, code: &str) -> Request<Body> {
        request.headers_mut().insert(
            MANAGER_TOTP_CODE_HEADER,
            code.parse().expect("test TOTP header should be valid"),
        );
        request
    }

    fn manager_command_request(
        method: Method,
        uri: &str,
        access_token: &str,
        payload: Option<Value>,
        totp_code: Option<&str>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {access_token}"));
        let body = if let Some(payload) = payload {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&payload).expect("payload should serialize"))
        } else {
            Body::empty()
        };
        if let Some(totp_code) = totp_code {
            builder = builder.header(MANAGER_TOTP_CODE_HEADER, totp_code);
        }
        builder
            .body(body)
            .expect("manager command request should build")
    }

    fn totp_code_at(manual_secret: &str, timestamp: i64) -> String {
        generate_manager_totp_code(&SensitiveSecret::new(manual_secret.to_string()), timestamp)
            .expect("test TOTP code should generate")
            .to_string()
    }

    async fn create_totp_test_app_state(
        test_db_context: TestDbContext,
    ) -> (Arc<AppState>, Arc<AtomicI64>) {
        let mut app_state = AppState::new_for_test(test_db_context).await;
        let now = Arc::new(AtomicI64::new(get_current_timestamp()));
        let config: SecretEncryptionConfig = serde_yaml::from_str(&format!(
            "downstream_mode: recoverable\nencryption_key: '{TEST_SECRET_ENCRYPTION_KEY}'\n"
        ))
        .expect("test secret encryption config should parse");
        let secret_encryption = Arc::new(SecretEncryptionService::from_config(&config));
        app_state.admin = Arc::new(AdminServices::new(
            Arc::clone(&app_state.catalog),
            Arc::clone(&secret_encryption),
        ));
        let service_now = Arc::clone(&now);
        Arc::get_mut(&mut app_state.admin)
            .expect("fresh test AdminServices should be uniquely owned")
            .auth = Arc::new(ManagerAuthService::new_for_test_with_secret_encryption(
            Arc::new(move || service_now.load(Ordering::SeqCst)),
            Arc::clone(&secret_encryption),
        ));
        app_state.secret_encryption = secret_encryption;
        let app_state = Arc::new(app_state);
        app_state.catalog.clear_cache().await;
        app_state.catalog.reload().await;
        (app_state, now)
    }

    fn restart_with_corrupt_totp(app_state: &Arc<AppState>, now: &Arc<AtomicI64>) -> Arc<AppState> {
        let mut connection = get_connection().expect("test connection should load");
        match &mut connection {
            DbConnection::Postgres(connection) => {
                diesel::sql_query(
                    "UPDATE manager_credential SET totp_secret_ciphertext = $1 WHERE manager_id = 0",
                )
                .bind::<diesel::sql_types::Binary, _>(vec![1_u8])
                .execute(connection)
                .expect("PostgreSQL TOTP corruption should succeed");
            }
            DbConnection::Sqlite(connection) => {
                diesel::sql_query(
                    "UPDATE manager_credential SET totp_secret_ciphertext = ? WHERE manager_id = 0",
                )
                .bind::<diesel::sql_types::Binary, _>(vec![1_u8])
                .execute(connection)
                .expect("SQLite TOTP corruption should succeed");
            }
        }
        drop(connection);

        let mut restarted = (**app_state).clone();
        let mut admin = AdminServices::new(
            Arc::clone(&restarted.catalog),
            Arc::clone(&restarted.secret_encryption),
        );
        let service_now = Arc::clone(now);
        admin.auth = Arc::new(ManagerAuthService::new_for_test_with_secret_encryption(
            Arc::new(move || service_now.load(Ordering::SeqCst)),
            Arc::clone(&restarted.secret_encryption),
        ));
        restarted.admin = Arc::new(admin);
        Arc::new(restarted)
    }

    async fn enroll_totp_over_http(
        app_state: &Arc<AppState>,
        confirmation_timestamp: i64,
    ) -> EnrolledHttpManager {
        let bootstrap = send(
            app_state,
            json_request(
                Method::POST,
                "/auth/bootstrap",
                json!({ "password": INITIAL_PASSWORD }),
            ),
        )
        .await;
        assert_eq!(bootstrap.status(), StatusCode::OK);
        let bootstrap_cookie = mediator_cookie(&bootstrap);
        let bootstrap_body = response_json(bootstrap).await;
        assert_eq!(bootstrap_body["data"]["totp_state"], "disabled");
        let bootstrap_access = access_token(&bootstrap_body);

        let start = send(
            app_state,
            cookie_request(
                Method::POST,
                "/auth/totp/enroll/start",
                &bootstrap_cookie,
                Some(&bootstrap_access),
                json!({ "current_password": INITIAL_PASSWORD }),
            ),
        )
        .await;
        assert_eq!(start.status(), StatusCode::OK);
        assert!(
            start.headers().get(header::SET_COOKIE).is_none(),
            "enrollment start must not mutate the browser session"
        );
        let start = response_json(start).await;
        let setup_challenge = start["data"]["setup_challenge"]
            .as_str()
            .expect("setup challenge should exist");
        let manual_secret = start["data"]["manual_secret"]
            .as_str()
            .expect("manual secret should exist")
            .to_string();
        assert!(
            start["data"]["otpauth_uri"]
                .as_str()
                .is_some_and(|uri| uri.contains(&manual_secret))
        );
        assert_eq!(start["data"]["expires_in"], 600);

        let confirm = send(
            app_state,
            cookie_request(
                Method::POST,
                "/auth/totp/enroll/confirm",
                &bootstrap_cookie,
                Some(&bootstrap_access),
                json!({
                    "setup_challenge": setup_challenge,
                    "totp_code": totp_code_at(&manual_secret, confirmation_timestamp),
                }),
            ),
        )
        .await;
        assert_eq!(confirm.status(), StatusCode::OK);
        let cookie = mediator_cookie(&confirm);
        let body = response_json(confirm).await;
        assert_eq!(body["data"]["totp_state"], "enabled");
        let recovery_codes = body["data"]["recovery_codes"]
            .as_array()
            .expect("enrollment should return recovery codes")
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .expect("recovery code should be text")
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(recovery_codes.len(), 10);
        EnrolledHttpManager {
            access_token: access_token(&body),
            cookie,
            manual_secret,
            recovery_codes,
        }
    }

    async fn assert_error(response: axum::response::Response, status: StatusCode, code: u64) {
        assert_eq!(response.status(), status);
        let body = response_json(response).await;
        assert_eq!(body["code"].as_u64(), Some(code));
    }

    #[tokio::test]
    async fn auth_http_bootstrap_login_access_rotate_and_legacy_refresh_removal_contract() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-contract.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let status = send(
                    &app_state,
                    empty_request(Method::GET, "/auth/bootstrap/status"),
                )
                .await;
                assert_eq!(
                    response_json(status).await["data"]["state"],
                    "uninitialized"
                );

                let bootstrap_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/bootstrap",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(bootstrap_response.status(), StatusCode::OK);
                let bootstrap_cookie = mediator_cookie(&bootstrap_response);
                let bootstrap_set_cookie = bootstrap_response
                    .headers()
                    .get(header::SET_COOKIE)
                    .expect("bootstrap should set mediator cookie")
                    .to_str()
                    .expect("cookie should be text")
                    .to_string();
                assert!(bootstrap_set_cookie.contains("Path=/ai/manager/api/auth"));
                assert!(bootstrap_set_cookie.contains("HttpOnly"));
                assert!(bootstrap_set_cookie.contains("SameSite=Strict"));
                assert!(!bootstrap_set_cookie.contains("Secure"));
                let bootstrap_body = response_json(bootstrap_response).await;
                assert_eq!(bootstrap_body["data"]["totp_state"], "disabled");
                let bootstrap_access = access_token(&bootstrap_body);

                let duplicate = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/bootstrap",
                        json!({ "password": ROTATED_PASSWORD }),
                    ),
                )
                .await;
                assert_error(duplicate, StatusCode::CONFLICT, 1401).await;

                let legacy_key = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login/password",
                        json!({ "key": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_error(legacy_key, StatusCode::UNPROCESSABLE_ENTITY, 1416).await;

                let removed_single_stage_login = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(removed_single_stage_login.status(), StatusCode::NOT_FOUND);

                let login_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login/password",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(login_response.status(), StatusCode::OK);
                let login_cookie = mediator_cookie(&login_response);
                let login_body = response_json(login_response).await;
                assert_eq!(login_body["data"]["state"], "authenticated");
                assert_eq!(login_body["data"]["totp_state"], "disabled");
                let login_access = access_token(&login_body);

                let mismatched_rotate = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/password/rotate",
                        &bootstrap_cookie,
                        Some(&login_access),
                        json!({
                            "current_password": INITIAL_PASSWORD,
                            "new_password": ROTATED_PASSWORD,
                        }),
                    ),
                )
                .await;
                assert_eq!(mismatched_rotate.status(), StatusCode::UNAUTHORIZED);
                assert!(
                    mismatched_rotate
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0")))
                );
                assert_eq!(response_json(mismatched_rotate).await["code"], json!(1441));

                let rotate_response = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/password/rotate",
                        &login_cookie,
                        Some(&login_access),
                        json!({
                            "current_password": INITIAL_PASSWORD,
                            "new_password": ROTATED_PASSWORD,
                        }),
                    ),
                )
                .await;
                assert_eq!(rotate_response.status(), StatusCode::OK);
                let rotated_cookie = mediator_cookie(&rotate_response);
                let rotated_access = access_token(&response_json(rotate_response).await);

                let stale_cookie_access = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/access",
                        &bootstrap_cookie,
                        None,
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(stale_cookie_access.status(), StatusCode::UNAUTHORIZED);
                assert!(
                    stale_cookie_access
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0")))
                );
                assert_eq!(
                    response_json(stale_cookie_access).await["code"],
                    json!(1441)
                );

                let access_response = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/access",
                        &rotated_cookie,
                        None,
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(access_response.status(), StatusCode::OK);
                assert!(
                    access_response.headers().get(header::SET_COOKIE).is_none(),
                    "ordinary access retrieval must not reissue mediator cookie"
                );
                let recovered_access = access_token(&response_json(access_response).await);
                assert_eq!(recovered_access, rotated_access);

                let removed_refresh_route = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/refresh_token",
                        &rotated_cookie,
                        None,
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(removed_refresh_route.status(), StatusCode::NOT_FOUND);

                let logout_response = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/logout",
                        &rotated_cookie,
                        None,
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(logout_response.status(), StatusCode::OK);
                assert!(
                    logout_response
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0")))
                );

                let idempotent_logout = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/logout",
                        &rotated_cookie,
                        None,
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(idempotent_logout.status(), StatusCode::OK);
                assert!(
                    idempotent_logout
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0")))
                );

                let stale_bootstrap_access = send_manager(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/manager/api/system/overview")
                        .header(header::AUTHORIZATION, format!("Bearer {bootstrap_access}"))
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await;
                assert_error(stale_bootstrap_access, StatusCode::UNAUTHORIZED, 1433).await;
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_totp_enrollment_status_and_two_stage_login_contract() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-auth-totp-login-enrollment.sqlite");
        test_db_context
            .run_async(async {
                let (app_state, now) = create_totp_test_app_state(test_db_context.clone()).await;
                let enrolled =
                    enroll_totp_over_http(&app_state, now.load(Ordering::SeqCst) - 30).await;
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    1
                );

                let status = send(
                    &app_state,
                    cookie_request(
                        Method::GET,
                        "/auth/totp/status",
                        &enrolled.cookie,
                        Some(&enrolled.access_token),
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(status.status(), StatusCode::OK);
                assert!(
                    status.headers().get(header::SET_COOKIE).is_none(),
                    "status must not mutate the mediator cookie"
                );
                let status = response_json(status).await;
                assert_eq!(status["data"]["state"], "enabled");
                assert!(status["data"]["enabled_at"].as_i64().is_some());
                assert!(status["data"].get("manual_secret").is_none());

                let password_stage = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login/password",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(password_stage.status(), StatusCode::OK);
                assert!(
                    password_stage
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0"))),
                    "TOTP-required password stage must clear residual mediator cookies"
                );
                let password_stage = response_json(password_stage).await;
                assert_eq!(password_stage["data"]["state"], "totp_required");
                assert_eq!(password_stage["data"]["expires_in"], 300);
                assert!(password_stage["data"].get("access_token").is_none());
                assert!(password_stage["data"].get("totp_state").is_none());
                let login_challenge = password_stage["data"]["login_challenge"]
                    .as_str()
                    .expect("login challenge should exist")
                    .to_string();
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    1,
                    "password stage must not create a database session"
                );

                let invalid_totp = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login/totp",
                        json!({
                            "login_challenge": login_challenge,
                            "totp_code": "abcdef",
                        }),
                    ),
                )
                .await;
                assert!(
                    invalid_totp.headers().get(header::SET_COOKIE).is_none(),
                    "failed TOTP login must not issue a cookie"
                );
                assert_error(invalid_totp, StatusCode::UNAUTHORIZED, 1472).await;

                let totp_stage = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login/totp",
                        json!({
                            "login_challenge": login_challenge,
                            "totp_code": totp_code_at(
                                &enrolled.manual_secret,
                                now.load(Ordering::SeqCst),
                            ),
                        }),
                    ),
                )
                .await;
                assert_eq!(totp_stage.status(), StatusCode::OK);
                let _login_cookie = mediator_cookie(&totp_stage);
                let totp_stage = response_json(totp_stage).await;
                assert_eq!(totp_stage["data"]["totp_state"], "enabled");
                assert!(totp_stage["data"].get("recovery_codes").is_none());
                access_token(&totp_stage);
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    2
                );

                let reused_challenge = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login/totp",
                        json!({
                            "login_challenge": login_challenge,
                            "totp_code": "000000",
                        }),
                    ),
                )
                .await;
                assert_error(reused_challenge, StatusCode::UNAUTHORIZED, 1477).await;

                for alias in [
                    "/auth/login",
                    "/auth/recovery",
                    "/auth/totp/reset",
                    "/auth/totp/recovery/start",
                ] {
                    let response =
                        send(&app_state, json_request(Method::POST, alias, json!({}))).await;
                    assert_eq!(response.status(), StatusCode::NOT_FOUND, "{alias}");
                }
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_totp_replacement_rotates_epoch_session_and_recovery_material() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-totp-replacement.sqlite");
        test_db_context
            .run_async(async {
                let (app_state, now) = create_totp_test_app_state(test_db_context.clone()).await;
                let enrolled =
                    enroll_totp_over_http(&app_state, now.load(Ordering::SeqCst) - 30).await;
                let old_recovery_rows = ManagerTotpRecoveryCode::list().unwrap();

                let missing_header = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/totp/replace/start",
                        &enrolled.cookie,
                        Some(&enrolled.access_token),
                        json!({ "current_password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_error(missing_header, StatusCode::PRECONDITION_REQUIRED, 1471).await;
                assert_eq!(ManagerTotpRecoveryCode::list().unwrap(), old_recovery_rows);

                let replace_start = send(
                    &app_state,
                    with_totp_header(
                        cookie_request(
                            Method::POST,
                            "/auth/totp/replace/start",
                            &enrolled.cookie,
                            Some(&enrolled.access_token),
                            json!({ "current_password": INITIAL_PASSWORD }),
                        ),
                        &totp_code_at(&enrolled.manual_secret, now.load(Ordering::SeqCst)),
                    ),
                )
                .await;
                assert_eq!(replace_start.status(), StatusCode::OK);
                assert!(replace_start.headers().get(header::SET_COOKIE).is_none());
                let replace_start = response_json(replace_start).await;
                let setup_challenge = replace_start["data"]["setup_challenge"]
                    .as_str()
                    .expect("replacement challenge should exist");
                let replacement_secret = replace_start["data"]["manual_secret"]
                    .as_str()
                    .expect("replacement secret should exist")
                    .to_string();
                assert_eq!(ManagerTotpRecoveryCode::list().unwrap(), old_recovery_rows);

                now.fetch_add(30, Ordering::SeqCst);
                let replace_confirm = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/totp/replace/confirm",
                        &enrolled.cookie,
                        Some(&enrolled.access_token),
                        json!({
                            "setup_challenge": setup_challenge,
                            "totp_code": totp_code_at(
                                &replacement_secret,
                                now.load(Ordering::SeqCst) + 30,
                            ),
                        }),
                    ),
                )
                .await;
                assert_eq!(replace_confirm.status(), StatusCode::OK);
                let replacement_cookie = mediator_cookie(&replace_confirm);
                let replace_confirm = response_json(replace_confirm).await;
                assert_eq!(replace_confirm["data"]["totp_state"], "enabled");
                assert_eq!(
                    replace_confirm["data"]["recovery_codes"]
                        .as_array()
                        .expect("replacement codes should exist")
                        .len(),
                    10
                );
                assert!(replace_confirm["data"].get("manual_secret").is_none());
                let replacement_access = access_token(&replace_confirm);
                assert_ne!(ManagerTotpRecoveryCode::list().unwrap(), old_recovery_rows);
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    1
                );

                let old_status = send(
                    &app_state,
                    cookie_request(
                        Method::GET,
                        "/auth/totp/status",
                        &enrolled.cookie,
                        Some(&enrolled.access_token),
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(old_status.status(), StatusCode::UNAUTHORIZED);
                let new_status = send(
                    &app_state,
                    cookie_request(
                        Method::GET,
                        "/auth/totp/status",
                        &replacement_cookie,
                        Some(&replacement_access),
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(new_status.status(), StatusCode::OK);
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_totp_disable_returns_disabled_access_and_clears_recovery_codes() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-totp-disable.sqlite");
        test_db_context
            .run_async(async {
                let (app_state, now) = create_totp_test_app_state(test_db_context.clone()).await;
                let enrolled =
                    enroll_totp_over_http(&app_state, now.load(Ordering::SeqCst) - 30).await;
                let disable = send(
                    &app_state,
                    with_totp_header(
                        cookie_request(
                            Method::POST,
                            "/auth/totp/disable",
                            &enrolled.cookie,
                            Some(&enrolled.access_token),
                            json!({ "current_password": INITIAL_PASSWORD }),
                        ),
                        &totp_code_at(&enrolled.manual_secret, now.load(Ordering::SeqCst)),
                    ),
                )
                .await;
                assert_eq!(disable.status(), StatusCode::OK);
                let disabled_cookie = mediator_cookie(&disable);
                let disable = response_json(disable).await;
                assert_eq!(disable["data"]["totp_state"], "disabled");
                assert!(disable["data"].get("recovery_codes").is_none());
                let disabled_access = access_token(&disable);
                assert_eq!(ManagerTotpRecoveryCode::list().unwrap().len(), 0);
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    1
                );

                let status = send(
                    &app_state,
                    cookie_request(
                        Method::GET,
                        "/auth/totp/status",
                        &disabled_cookie,
                        Some(&disabled_access),
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(response_json(status).await["data"]["state"], "disabled");
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_password_reauth_contract_tracks_access_state() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-password-reauth.sqlite");
        test_db_context
            .run_async(async {
                let (app_state, now) = create_totp_test_app_state(test_db_context.clone()).await;
                let bootstrap = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/bootstrap",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(bootstrap.status(), StatusCode::OK);
                let cookie = mediator_cookie(&bootstrap);
                let bootstrap = response_json(bootstrap).await;
                let access = access_token(&bootstrap);
                assert_eq!(bootstrap["data"]["reauth"]["scope"], "secret_governance");
                assert_eq!(bootstrap["data"]["reauth"]["method"], "password");

                let wrong_method = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/reauth",
                        &cookie,
                        Some(&access),
                        json!({ "method": "totp", "totp_code": "000000" }),
                    ),
                )
                .await;
                assert_error(wrong_method, StatusCode::CONFLICT, 1492).await;

                now.fetch_add(SECRET_GOVERNANCE_REAUTH_TTL_SEC, Ordering::SeqCst);
                let expired_access = send(
                    &app_state,
                    cookie_request(Method::POST, "/auth/access", &cookie, None, json!({})),
                )
                .await;
                assert_eq!(expired_access.status(), StatusCode::OK);
                assert!(response_json(expired_access).await["data"]["reauth"].is_null());

                let blocked_create = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/api_key",
                        &access,
                        Some(json!({
                            "name": "password-reauth-api-key",
                            "default_action": "ALLOW",
                        })),
                        None,
                    ),
                )
                .await;
                assert_error(blocked_create, StatusCode::FORBIDDEN, 1491).await;
                assert!(app_state.admin.api_key.list_api_keys().unwrap().is_empty());

                let invalid_password = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/reauth",
                        &cookie,
                        Some(&access),
                        json!({
                            "method": "password",
                            "password": "wrong horse battery staple",
                        }),
                    ),
                )
                .await;
                assert_error(invalid_password, StatusCode::UNAUTHORIZED, 1481).await;

                let reauthenticated = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/reauth",
                        &cookie,
                        Some(&access),
                        json!({
                            "method": "password",
                            "password": INITIAL_PASSWORD,
                        }),
                    ),
                )
                .await;
                assert_eq!(reauthenticated.status(), StatusCode::OK);
                let reauthenticated = response_json(reauthenticated).await;
                assert_eq!(reauthenticated["data"]["scope"], "secret_governance");
                assert_eq!(reauthenticated["data"]["method"], "password");
                assert_eq!(
                    reauthenticated["data"]["verified_until"].as_i64(),
                    Some(now.load(Ordering::SeqCst) + SECRET_GOVERNANCE_REAUTH_TTL_SEC)
                );

                let created = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/api_key",
                        &access,
                        Some(json!({
                            "name": "password-reauth-api-key",
                            "default_action": "ALLOW",
                        })),
                        None,
                    ),
                )
                .await;
                assert_eq!(created.status(), StatusCode::OK);
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_totp_recovery_consumes_code_revokes_sessions_and_returns_one_new_session() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-totp-recovery.sqlite");
        test_db_context
            .run_async(async {
                let (app_state, now) = create_totp_test_app_state(test_db_context.clone()).await;
                let enrolled =
                    enroll_totp_over_http(&app_state, now.load(Ordering::SeqCst) - 30).await;
                let recovery_start = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/recovery/start",
                        json!({
                            "password": INITIAL_PASSWORD,
                            "recovery_code": enrolled.recovery_codes[0],
                        }),
                    ),
                )
                .await;
                assert_eq!(recovery_start.status(), StatusCode::OK);
                assert!(
                    recovery_start
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0")))
                );
                let recovery_start = response_json(recovery_start).await;
                assert!(recovery_start["data"].get("access_token").is_none());
                assert!(recovery_start["data"].get("recovery_codes").is_none());
                let recovery_challenge = recovery_start["data"]["recovery_challenge"]
                    .as_str()
                    .expect("recovery challenge should exist");
                let recovery_secret = recovery_start["data"]["manual_secret"]
                    .as_str()
                    .expect("recovery secret should exist")
                    .to_string();
                assert_eq!(ManagerTotpRecoveryCode::list().unwrap().len(), 9);
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    0
                );

                let recovery_confirm = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/recovery/confirm",
                        json!({
                            "recovery_challenge": recovery_challenge,
                            "totp_code": totp_code_at(
                                &recovery_secret,
                                now.load(Ordering::SeqCst),
                            ),
                        }),
                    ),
                )
                .await;
                assert_eq!(recovery_confirm.status(), StatusCode::OK);
                mediator_cookie(&recovery_confirm);
                let recovery_confirm = response_json(recovery_confirm).await;
                assert_eq!(recovery_confirm["data"]["totp_state"], "enabled");
                assert_eq!(
                    recovery_confirm["data"]["recovery_codes"]
                        .as_array()
                        .expect("new recovery codes should exist")
                        .len(),
                    10
                );
                access_token(&recovery_confirm);
                assert_eq!(ManagerTotpRecoveryCode::list().unwrap().len(), 10);
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    1
                );
            })
            .await;
    }

    #[tokio::test]
    async fn secret_governance_grant_protects_api_key_commands_before_business_side_effects() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-api-key-secret-governance-reauth.sqlite");
        test_db_context
            .run_async(async {
                let (app_state, now) = create_totp_test_app_state(test_db_context.clone()).await;
                let enrolled =
                    enroll_totp_over_http(&app_state, now.load(Ordering::SeqCst) - 30).await;
                now.fetch_add(SECRET_GOVERNANCE_REAUTH_TTL_SEC, Ordering::SeqCst);
                let create_payload = json!({
                    "name": "sensitive-api-key",
                    "default_action": "ALLOW"
                });

                let missing = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/api_key",
                        &enrolled.access_token,
                        Some(create_payload.clone()),
                        None,
                    ),
                )
                .await;
                assert_error(missing, StatusCode::FORBIDDEN, 1491).await;
                assert!(app_state.admin.api_key.list_api_keys().unwrap().is_empty());

                let ignored_business_header = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/api_key",
                        &enrolled.access_token,
                        Some(create_payload.clone()),
                        Some("000000"),
                    ),
                )
                .await;
                assert_error(ignored_business_header, StatusCode::FORBIDDEN, 1491).await;
                assert!(app_state.admin.api_key.list_api_keys().unwrap().is_empty());

                let invalid_reauth = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/reauth",
                        &enrolled.cookie,
                        Some(&enrolled.access_token),
                        json!({ "method": "totp", "totp_code": "000000" }),
                    ),
                )
                .await;
                assert_error(invalid_reauth, StatusCode::UNAUTHORIZED, 1472).await;

                let reauth_code = totp_code_at(&enrolled.manual_secret, now.load(Ordering::SeqCst));
                let reauthenticated = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/reauth",
                        &enrolled.cookie,
                        Some(&enrolled.access_token),
                        json!({ "method": "totp", "totp_code": reauth_code }),
                    ),
                )
                .await;
                assert_eq!(reauthenticated.status(), StatusCode::OK);
                let reauthenticated = response_json(reauthenticated).await;
                assert_eq!(reauthenticated["data"]["scope"], "secret_governance");
                assert_eq!(reauthenticated["data"]["method"], "totp");
                assert_eq!(
                    reauthenticated["data"]["verified_until"].as_i64(),
                    Some(now.load(Ordering::SeqCst) + SECRET_GOVERNANCE_REAUTH_TTL_SEC)
                );

                let created = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/api_key",
                        &enrolled.access_token,
                        Some(create_payload),
                        None,
                    ),
                )
                .await;
                assert_eq!(created.status(), StatusCode::OK);
                let created = response_json(created).await;
                let api_key_id = created["data"]["detail"]["id"]
                    .as_i64()
                    .expect("created API key id should exist");
                let original_secret = created["data"]["reveal"]["api_key"]
                    .as_str()
                    .expect("one-time API key should exist")
                    .to_string();

                let metadata_update_path = format!("/manager/api/api_key/{api_key_id}");
                let metadata_update = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::PUT,
                        &metadata_update_path,
                        &enrolled.access_token,
                        Some(json!({ "name": "sensitive-api-key-updated" })),
                        None,
                    ),
                )
                .await;
                assert_eq!(
                    metadata_update.status(),
                    StatusCode::OK,
                    "API key metadata update must not require TOTP"
                );

                let reveal_path = format!("/manager/api/api_key/{api_key_id}/reveal");
                let missing_target = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/api_key/999999/reveal",
                        &enrolled.access_token,
                        None,
                        None,
                    ),
                )
                .await;
                assert_eq!(missing_target.status(), StatusCode::NOT_FOUND);
                let revealed = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &reveal_path,
                        &enrolled.access_token,
                        None,
                        None,
                    ),
                )
                .await;
                assert_eq!(revealed.status(), StatusCode::OK);
                assert_eq!(
                    response_json(revealed).await["data"]["api_key"],
                    original_secret
                );

                let rotate_path = format!("/manager/api/api_key/{api_key_id}/rotate");
                let rotated = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &rotate_path,
                        &enrolled.access_token,
                        None,
                        None,
                    ),
                )
                .await;
                assert_eq!(rotated.status(), StatusCode::OK);
                let rotated_secret = response_json(rotated).await["data"]["api_key"]
                    .as_str()
                    .expect("rotated API key should exist")
                    .to_string();
                assert_ne!(rotated_secret, original_secret);

                let delete_path = format!("/manager/api/api_key/{api_key_id}");
                let deleted = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::DELETE,
                        &delete_path,
                        &enrolled.access_token,
                        None,
                        None,
                    ),
                )
                .await;
                assert_eq!(deleted.status(), StatusCode::OK);
                assert!(
                    app_state
                        .admin
                        .api_key
                        .get_api_key_detail(api_key_id)
                        .is_err()
                );
            })
            .await;
    }

    #[tokio::test]
    async fn provider_reveal_uses_secret_governance_grant_while_other_mutations_do_not() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-provider-reveal-secret-governance.sqlite");
        test_db_context
            .run_async(async {
                let (app_state, now) = create_totp_test_app_state(test_db_context.clone()).await;
                let enrolled =
                    enroll_totp_over_http(&app_state, now.load(Ordering::SeqCst) - 30).await;
                now.fetch_add(SECRET_GOVERNANCE_REAUTH_TTL_SEC, Ordering::SeqCst);

                let provider = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/provider",
                        &enrolled.access_token,
                        Some(json!({
                            "name": "Sensitive Provider",
                            "key": "sensitive-provider",
                            "endpoint": "https://api.example.com/v1",
                            "use_proxy": false,
                            "provider_type": "OPENAI",
                            "provider_api_key_mode": "QUEUE"
                        })),
                        None,
                    ),
                )
                .await;
                assert_eq!(provider.status(), StatusCode::OK);
                let provider_id = response_json(provider).await["data"]["id"]
                    .as_i64()
                    .expect("provider id should exist");

                let collection = format!("/manager/api/provider/{provider_id}/provider_keys");
                let created_key = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &collection,
                        &enrolled.access_token,
                        Some(json!({
                            "api_key": "sk-provider-original",
                            "description": "primary"
                        })),
                        None,
                    ),
                )
                .await;
                assert_eq!(
                    created_key.status(),
                    StatusCode::OK,
                    "provider key creation must not require TOTP"
                );
                let key_id = response_json(created_key).await["data"]["id"]
                    .as_i64()
                    .expect("provider key id should exist");

                let replace_path =
                    format!("/manager/api/provider/{provider_id}/provider_keys/{key_id}/replace");
                let replaced = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &replace_path,
                        &enrolled.access_token,
                        Some(json!({ "api_key": "sk-provider-replaced" })),
                        None,
                    ),
                )
                .await;
                assert_eq!(
                    replaced.status(),
                    StatusCode::OK,
                    "provider key replacement must not require TOTP"
                );

                let reveal_path =
                    format!("/manager/api/provider/{provider_id}/provider_keys/{key_id}/reveal");
                let missing = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &reveal_path,
                        &enrolled.access_token,
                        None,
                        None,
                    ),
                )
                .await;
                assert_error(missing, StatusCode::FORBIDDEN, 1491).await;
                let invalid = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &reveal_path,
                        &enrolled.access_token,
                        None,
                        Some("abcdef"),
                    ),
                )
                .await;
                assert_error(invalid, StatusCode::FORBIDDEN, 1491).await;

                let reauth_code = totp_code_at(&enrolled.manual_secret, now.load(Ordering::SeqCst));
                let reauthenticated = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/reauth",
                        &enrolled.cookie,
                        Some(&enrolled.access_token),
                        json!({ "method": "totp", "totp_code": reauth_code }),
                    ),
                )
                .await;
                assert_eq!(reauthenticated.status(), StatusCode::OK);

                let missing_target = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &format!("/manager/api/provider/{provider_id}/provider_keys/999999/reveal"),
                        &enrolled.access_token,
                        None,
                        None,
                    ),
                )
                .await;
                assert_eq!(missing_target.status(), StatusCode::NOT_FOUND);
                let revealed = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &reveal_path,
                        &enrolled.access_token,
                        None,
                        None,
                    ),
                )
                .await;
                assert_eq!(revealed.status(), StatusCode::OK);
                assert_eq!(
                    response_json(revealed).await["data"]["api_key"],
                    "sk-provider-replaced"
                );

                let key_path =
                    format!("/manager/api/provider/{provider_id}/provider_keys/{key_id}");
                let updated = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::PUT,
                        &key_path,
                        &enrolled.access_token,
                        Some(json!({ "description": "updated", "is_enabled": true })),
                        None,
                    ),
                )
                .await;
                assert_eq!(
                    updated.status(),
                    StatusCode::OK,
                    "provider metadata update must not require TOTP"
                );

                let second_key = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &collection,
                        &enrolled.access_token,
                        Some(json!({
                            "api_key": "sk-provider-delete",
                            "description": null
                        })),
                        None,
                    ),
                )
                .await;
                assert_eq!(second_key.status(), StatusCode::OK);
                let second_key_id = response_json(second_key).await["data"]["id"]
                    .as_i64()
                    .expect("second provider key id should exist");
                let deleted_key = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::DELETE,
                        &format!(
                            "/manager/api/provider/{provider_id}/provider_keys/{second_key_id}"
                        ),
                        &enrolled.access_token,
                        None,
                        None,
                    ),
                )
                .await;
                assert_eq!(
                    deleted_key.status(),
                    StatusCode::OK,
                    "provider key deletion must not require TOTP"
                );

                let second_provider = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/provider",
                        &enrolled.access_token,
                        Some(json!({
                            "name": "Delete Provider",
                            "key": "delete-provider",
                            "endpoint": "https://delete.example.com/v1",
                            "use_proxy": false,
                            "provider_type": "OPENAI",
                            "provider_api_key_mode": "QUEUE"
                        })),
                        None,
                    ),
                )
                .await;
                assert_eq!(second_provider.status(), StatusCode::OK);
                let second_provider_id = response_json(second_provider).await["data"]["id"]
                    .as_i64()
                    .expect("second provider id should exist");
                let deleted_provider = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::DELETE,
                        &format!("/manager/api/provider/{second_provider_id}"),
                        &enrolled.access_token,
                        None,
                        None,
                    ),
                )
                .await;
                assert_eq!(
                    deleted_provider.status(),
                    StatusCode::OK,
                    "provider deletion must not require TOTP"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn password_rotation_and_logout_all_verify_totp_at_the_required_command_boundary() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-sensitive-auth-commands-totp.sqlite");
        test_db_context
            .run_async(async {
                let (app_state, now) = create_totp_test_app_state(test_db_context.clone()).await;
                let enrolled =
                    enroll_totp_over_http(&app_state, now.load(Ordering::SeqCst) - 30).await;

                let rotate_request = |current_password: &str, totp_code: Option<&str>| {
                    let request = cookie_request(
                        Method::POST,
                        "/auth/password/rotate",
                        &enrolled.cookie,
                        Some(&enrolled.access_token),
                        json!({
                            "current_password": current_password,
                            "new_password": ROTATED_PASSWORD,
                        }),
                    );
                    match totp_code {
                        Some(code) => with_totp_header(request, code),
                        None => request,
                    }
                };

                let missing = send(&app_state, rotate_request(INITIAL_PASSWORD, None)).await;
                assert_error(missing, StatusCode::PRECONDITION_REQUIRED, 1471).await;
                let invalid =
                    send(&app_state, rotate_request(INITIAL_PASSWORD, Some("abcdef"))).await;
                assert_error(invalid, StatusCode::UNAUTHORIZED, 1472).await;

                let current_code =
                    totp_code_at(&enrolled.manual_secret, now.load(Ordering::SeqCst));
                let wrong_password = send(
                    &app_state,
                    rotate_request("wrong horse battery staple", Some(&current_code)),
                )
                .await;
                assert_error(wrong_password, StatusCode::UNAUTHORIZED, 1421).await;

                let rotated = send(
                    &app_state,
                    rotate_request(INITIAL_PASSWORD, Some(&current_code)),
                )
                .await;
                assert_eq!(
                    rotated.status(),
                    StatusCode::OK,
                    "wrong current password must not consume the TOTP step"
                );
                let rotated_cookie = mediator_cookie(&rotated);
                let rotated = response_json(rotated).await;
                assert_eq!(rotated["data"]["totp_state"], "enabled");
                let rotated_access = access_token(&rotated);
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    1
                );

                let logout_request = |current_password: &str, totp_code: Option<&str>| {
                    let request = cookie_request(
                        Method::POST,
                        "/auth/logout_all",
                        &rotated_cookie,
                        Some(&rotated_access),
                        json!({ "current_password": current_password }),
                    );
                    match totp_code {
                        Some(code) => with_totp_header(request, code),
                        None => request,
                    }
                };
                let missing = send(&app_state, logout_request(ROTATED_PASSWORD, None)).await;
                assert_error(missing, StatusCode::PRECONDITION_REQUIRED, 1471).await;
                let invalid =
                    send(&app_state, logout_request(ROTATED_PASSWORD, Some("000000"))).await;
                assert_error(invalid, StatusCode::UNAUTHORIZED, 1472).await;
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    1,
                    "failed TOTP must not revoke sessions"
                );

                now.fetch_add(30, Ordering::SeqCst);
                let logout_code = totp_code_at(&enrolled.manual_secret, now.load(Ordering::SeqCst));
                let wrong_password = send(
                    &app_state,
                    logout_request("wrong horse battery staple", Some(&logout_code)),
                )
                .await;
                assert_error(wrong_password, StatusCode::UNAUTHORIZED, 1481).await;
                let logged_out = send(
                    &app_state,
                    logout_request(ROTATED_PASSWORD, Some(&logout_code)),
                )
                .await;
                assert_eq!(logged_out.status(), StatusCode::OK);
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    0
                );
            })
            .await;
    }

    #[tokio::test]
    async fn sensitive_command_matrix_fails_closed_when_totp_is_unavailable() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-sensitive-totp-unavailable.sqlite");
        test_db_context
            .run_async(async {
                let (app_state, now) = create_totp_test_app_state(test_db_context.clone()).await;
                let enrolled =
                    enroll_totp_over_http(&app_state, now.load(Ordering::SeqCst) - 30).await;
                let create_code =
                    totp_code_at(&enrolled.manual_secret, now.load(Ordering::SeqCst));
                let created = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/api_key",
                        &enrolled.access_token,
                        Some(json!({
                            "name": "unavailable-api-key",
                            "default_action": "ALLOW"
                        })),
                        Some(&create_code),
                    ),
                )
                .await;
                assert_eq!(created.status(), StatusCode::OK);
                let created = response_json(created).await;
                let api_key_id = created["data"]["detail"]["id"]
                    .as_i64()
                    .expect("API key id should exist");
                let api_key_secret = created["data"]["reveal"]["api_key"]
                    .as_str()
                    .expect("API key secret should exist")
                    .to_string();

                let provider = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        "/manager/api/provider",
                        &enrolled.access_token,
                        Some(json!({
                            "name": "Unavailable Provider",
                            "key": "unavailable-provider",
                            "endpoint": "https://api.example.com/v1",
                            "use_proxy": false,
                            "provider_type": "OPENAI",
                            "provider_api_key_mode": "QUEUE"
                        })),
                        None,
                    ),
                )
                .await;
                assert_eq!(provider.status(), StatusCode::OK);
                let provider_id = response_json(provider).await["data"]["id"]
                    .as_i64()
                    .expect("provider id should exist");
                let provider_key = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &format!("/manager/api/provider/{provider_id}/provider_keys"),
                        &enrolled.access_token,
                        Some(json!({
                            "api_key": "sk-unavailable-provider",
                            "description": null
                        })),
                        None,
                    ),
                )
                .await;
                assert_eq!(provider_key.status(), StatusCode::OK);
                let provider_key_id = response_json(provider_key).await["data"]["id"]
                    .as_i64()
                    .expect("provider key id should exist");

                let credential_before = ManagerCredential::load()
                    .unwrap()
                    .expect("manager credential should exist");
                let sessions_before =
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len();
                let enrolled_context = decode_access_token(&enrolled.access_token)
                    .expect("enrollment access should decode");
                let app_state = restart_with_corrupt_totp(&app_state, &now);
                let restarted_access = app_state
                    .admin
                    .auth
                    .access_for_session(
                        enrolled_context.login_instance_id,
                        enrolled_context.credential_epoch,
                    )
                    .await
                    .expect("mediator recovery should rebuild page access after restart");

                let api_commands = [
                    (
                        Method::POST,
                        "/manager/api/api_key".to_string(),
                        Some(json!({
                            "name": "must-not-create",
                            "default_action": "ALLOW"
                        })),
                    ),
                    (
                        Method::POST,
                        format!("/manager/api/api_key/{api_key_id}/reveal"),
                        None,
                    ),
                    (
                        Method::POST,
                        format!("/manager/api/api_key/{api_key_id}/rotate"),
                        None,
                    ),
                    (
                        Method::DELETE,
                        format!("/manager/api/api_key/{api_key_id}"),
                        None,
                    ),
                ];
                for (method, path, payload) in api_commands {
                    let response = send_manager(
                        &app_state,
                        manager_command_request(
                            method.clone(),
                            &path,
                            &restarted_access,
                            payload,
                            None,
                        ),
                    )
                    .await;
                    let status = response.status();
                    let body = response_json(response).await;
                    assert_eq!(
                        status,
                        StatusCode::SERVICE_UNAVAILABLE,
                        "{method} {path}: {body}"
                    );
                    assert_eq!(body["code"].as_u64(), Some(1479), "{method} {path}");
                }
                assert_eq!(app_state.admin.api_key.list_api_keys().unwrap().len(), 1);
                assert_eq!(
                    app_state
                        .admin
                        .api_key
                        .reveal_api_key(api_key_id)
                        .unwrap()
                        .api_key,
                    api_key_secret,
                    "unavailable TOTP must precede reveal, rotation, and deletion"
                );

                let provider_reveal = send_manager(
                    &app_state,
                    manager_command_request(
                        Method::POST,
                        &format!(
                            "/manager/api/provider/{provider_id}/provider_keys/{provider_key_id}/reveal"
                        ),
                        &restarted_access,
                        None,
                        None,
                    ),
                )
                .await;
                assert_error(
                    provider_reveal,
                    StatusCode::SERVICE_UNAVAILABLE,
                    1479,
                )
                .await;
                assert_eq!(
                    app_state
                        .admin
                        .provider
                        .reveal_provider_api_key(provider_id, provider_key_id)
                        .await
                        .unwrap()
                        .api_key,
                    "sk-unavailable-provider"
                );

                let rotate = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/password/rotate",
                        &enrolled.cookie,
                        Some(&restarted_access),
                        json!({
                            "current_password": INITIAL_PASSWORD,
                            "new_password": ROTATED_PASSWORD,
                        }),
                    ),
                )
                .await;
                assert_error(rotate, StatusCode::SERVICE_UNAVAILABLE, 1479).await;

                let logout_all = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/logout_all",
                        &enrolled.cookie,
                        Some(&restarted_access),
                        json!({ "current_password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_error(logout_all, StatusCode::SERVICE_UNAVAILABLE, 1479).await;
                let credential_after = ManagerCredential::load()
                    .unwrap()
                    .expect("manager credential should remain");
                assert_eq!(
                    credential_after.password_verifier,
                    credential_before.password_verifier,
                    "unavailable TOTP must precede password mutation"
                );
                assert_eq!(
                    ManagerAuthInstance::list_active_instances(now.load(Ordering::SeqCst))
                        .unwrap()
                        .len(),
                    sessions_before,
                    "unavailable TOTP must precede logout-all revocation"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_logout_all_revokes_current_and_other_sessions() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-logout-all.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let first_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/bootstrap",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(first_response.status(), StatusCode::OK);
                let first_cookie = mediator_cookie(&first_response);
                let first_access = access_token(&response_json(first_response).await);
                let second_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login/password",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(second_response.status(), StatusCode::OK);
                let second_cookie = mediator_cookie(&second_response);
                let second_access = access_token(&response_json(second_response).await);

                let mismatch = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/logout_all",
                        &first_cookie,
                        Some(&second_access),
                        json!({ "current_password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(mismatch.status(), StatusCode::UNAUTHORIZED);
                assert!(
                    mismatch
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0")))
                );
                assert_eq!(response_json(mismatch).await["code"], json!(1441));

                let logout_all_response = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/logout_all",
                        &second_cookie,
                        Some(&second_access),
                        json!({ "current_password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(logout_all_response.status(), StatusCode::OK);
                assert!(
                    logout_all_response
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0")))
                );
                assert_eq!(
                    response_json(logout_all_response).await["data"]["revoked_sessions"],
                    json!(2)
                );

                for access_token in [first_access, second_access] {
                    let response = send_manager(
                        &app_state,
                        Request::builder()
                            .method(Method::GET)
                            .uri("/manager/api/system/overview")
                            .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
                            .body(Body::empty())
                            .expect("request should build"),
                    )
                    .await;
                    assert_error(response, StatusCode::UNAUTHORIZED, 1435).await;
                }
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_sensitive_storage_failures_preserve_or_delete_browser_state_by_contract() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-auth-sensitive-storage-failures.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let bootstrap = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/bootstrap",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(bootstrap.status(), StatusCode::OK);
                let cookie = mediator_cookie(&bootstrap);
                let access = access_token(&response_json(bootstrap).await);

                let mut conn = get_connection().expect("connection should load");
                match &mut conn {
                    DbConnection::Postgres(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("session table should drop");
                    }
                    DbConnection::Sqlite(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("session table should drop");
                    }
                }
                drop(conn);

                let rotate_failure = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/password/rotate",
                        &cookie,
                        Some(&access),
                        json!({
                            "current_password": INITIAL_PASSWORD,
                            "new_password": ROTATED_PASSWORD,
                        }),
                    ),
                )
                .await;
                assert!(
                    rotate_failure.headers().get(header::SET_COOKIE).is_none(),
                    "transaction failure must preserve the caller cookie"
                );
                assert_error(rotate_failure, StatusCode::SERVICE_UNAVAILABLE, 1426).await;

                let logout_all_failure = send(
                    &app_state,
                    cookie_request(
                        Method::POST,
                        "/auth/logout_all",
                        &cookie,
                        Some(&access),
                        json!({ "current_password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert!(
                    logout_all_failure
                        .headers()
                        .get(header::SET_COOKIE)
                        .is_none(),
                    "logout-all storage failure must preserve the caller cookie"
                );
                assert_error(logout_all_failure, StatusCode::SERVICE_UNAVAILABLE, 1484).await;

                let logout_failure = send(
                    &app_state,
                    cookie_request(Method::POST, "/auth/logout", &cookie, None, json!({})),
                )
                .await;
                assert!(
                    logout_failure
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0"))),
                    "current logout storage failure must delete the caller cookie"
                );
                assert_error(logout_failure, StatusCode::SERVICE_UNAVAILABLE, 1451).await;

                let still_valid = send_manager(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/manager/api/system/overview")
                        .header(header::AUTHORIZATION, format!("Bearer {access}"))
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await;
                assert_eq!(
                    still_valid.status(),
                    StatusCode::OK,
                    "failed persistent mutations must not alter in-memory authentication state"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_rejects_bad_headers_and_oversized_or_legacy_bodies() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-rejections.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let missing_mediator =
                    send(&app_state, empty_request(Method::POST, "/auth/access")).await;
                assert_error(missing_mediator, StatusCode::UNAUTHORIZED, 1441).await;

                let bad_logout = send(
                    &app_state,
                    auth_request(Method::POST, "/auth/logout", "invalid", json!({})),
                )
                .await;
                assert_eq!(bad_logout.status(), StatusCode::OK);
                assert!(
                    bad_logout
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .any(|value| value
                            .to_str()
                            .is_ok_and(|value| value.contains("Max-Age=0")))
                );

                let oversized = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/bootstrap",
                        json!({ "password": "x".repeat(5_000) }),
                    ),
                )
                .await;
                assert_error(oversized, StatusCode::UNPROCESSABLE_ENTITY, 1402).await;
            })
            .await;
    }

    #[test]
    fn mediator_cookie_policy_uses_secure_prefix_and_matching_delete_attributes_in_production() {
        let policy = mediator_cookie_policy_for(Some("https://admin.example.com"), "/ai/");
        assert_eq!(policy.name, "__Secure-cyder_manager_session");
        assert_eq!(policy.path, "/ai/manager/api/auth");
        assert!(policy.secure);

        let expires_at = get_current_timestamp() + 3_600;
        let cookie = mediator_cookie_value_for(&policy, "mediator-token", expires_at)
            .to_str()
            .expect("cookie should be text")
            .to_string();
        assert!(cookie.starts_with("__Secure-cyder_manager_session=mediator-token;"));
        assert!(cookie.contains("Path=/ai/manager/api/auth"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("Max-Age="));
        assert!(cookie.contains("Expires="));
        assert!(!cookie.contains("Domain="));

        let deleted = deleted_mediator_cookie_value_for(&policy)
            .to_str()
            .expect("deletion cookie should be text")
            .to_string();
        assert!(deleted.starts_with("__Secure-cyder_manager_session=;"));
        assert!(deleted.contains("Path=/ai/manager/api/auth"));
        assert!(deleted.contains("Max-Age=0"));
        assert!(deleted.contains("HttpOnly"));
        assert!(deleted.contains("SameSite=Strict"));
        assert!(deleted.contains("Secure"));
        assert!(!deleted.contains("Domain="));
    }

    #[tokio::test]
    async fn auth_http_browser_boundary_rejects_unsafe_requests_before_credentials_change() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-browser-boundary.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;

                for path in [
                    "/auth/bootstrap",
                    "/auth/login/password",
                    "/auth/login/totp",
                    "/auth/reauth",
                    "/auth/recovery/start",
                    "/auth/recovery/confirm",
                    "/auth/totp/enroll/start",
                    "/auth/totp/enroll/confirm",
                    "/auth/totp/replace/start",
                    "/auth/totp/replace/confirm",
                    "/auth/totp/disable",
                    "/auth/access",
                    "/auth/password/rotate",
                    "/auth/logout",
                    "/auth/logout_all",
                ] {
                    let mut missing_origin =
                        json_request(Method::POST, path, json!({ "password": INITIAL_PASSWORD }));
                    missing_origin.headers_mut().remove(header::ORIGIN);
                    assert_error(
                        send(&app_state, missing_origin).await,
                        StatusCode::FORBIDDEN,
                        1461,
                    )
                    .await;
                }

                let mut origin_with_path = json_request(
                    Method::POST,
                    "/auth/bootstrap",
                    json!({ "password": INITIAL_PASSWORD }),
                );
                origin_with_path.headers_mut().insert(
                    header::ORIGIN,
                    "http://127.0.0.1:29528/path"
                        .parse()
                        .expect("origin should parse as a header"),
                );
                assert_error(
                    send(&app_state, origin_with_path).await,
                    StatusCode::FORBIDDEN,
                    1461,
                )
                .await;

                let mut cross_site = json_request(
                    Method::POST,
                    "/auth/bootstrap",
                    json!({ "password": INITIAL_PASSWORD }),
                );
                cross_site.headers_mut().insert(
                    "sec-fetch-site",
                    "cross-site".parse().expect("fetch site should parse"),
                );
                assert_error(
                    send(&app_state, cross_site).await,
                    StatusCode::FORBIDDEN,
                    1461,
                )
                .await;

                let mut missing_custom_header = json_request(
                    Method::POST,
                    "/auth/bootstrap",
                    json!({ "password": INITIAL_PASSWORD }),
                );
                missing_custom_header
                    .headers_mut()
                    .remove("x-cyder-manager-auth");
                assert_error(
                    send(&app_state, missing_custom_header).await,
                    StatusCode::FORBIDDEN,
                    1461,
                )
                .await;

                let mut wrong_content_type = json_request(
                    Method::POST,
                    "/auth/bootstrap",
                    json!({ "password": INITIAL_PASSWORD }),
                );
                wrong_content_type.headers_mut().insert(
                    header::CONTENT_TYPE,
                    "text/plain".parse().expect("content type should parse"),
                );
                assert_error(
                    send(&app_state, wrong_content_type).await,
                    StatusCode::FORBIDDEN,
                    1461,
                )
                .await;

                let non_loopback = send_from(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/bootstrap",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                    SocketAddr::from(([192, 0, 2, 10], 31_004)),
                )
                .await;
                assert_error(non_loopback, StatusCode::FORBIDDEN, 1461).await;

                let valid = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/bootstrap",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(valid.status(), StatusCode::OK);
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_login_rate_limit_uses_peer_ip_and_ignores_forwarded_headers() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-peer-rate-limit.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                app_state
                    .admin
                    .auth
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
                let first_peer = SocketAddr::from(([127, 0, 0, 10], 31_001));
                let second_peer = SocketAddr::from(([127, 0, 0, 11], 31_002));

                for index in 0..5 {
                    let mut request = json_request(
                        Method::POST,
                        "/auth/login/password",
                        json!({ "password": "wrong horse battery staple" }),
                    );
                    request.headers_mut().insert(
                        "x-forwarded-for",
                        format!("198.51.100.{}", index + 1)
                            .parse()
                            .expect("forwarded header should parse"),
                    );
                    request.headers_mut().insert(
                        "x-real-ip",
                        format!("203.0.113.{}", index + 1)
                            .parse()
                            .expect("real ip header should parse"),
                    );
                    let response = send_from(&app_state, request, first_peer).await;
                    assert_error(response, StatusCode::UNAUTHORIZED, 1412).await;
                }

                let mut locked_request = json_request(
                    Method::POST,
                    "/auth/login/password",
                    json!({ "password": INITIAL_PASSWORD }),
                );
                locked_request.headers_mut().insert(
                    "forwarded",
                    "for=203.0.113.200"
                        .parse()
                        .expect("forwarded header should parse"),
                );
                let locked = send_from(&app_state, locked_request, first_peer).await;
                let retry_after = locked
                    .headers()
                    .get(header::RETRY_AFTER)
                    .expect("source lock should expose Retry-After")
                    .to_str()
                    .expect("Retry-After should be text")
                    .parse::<u64>()
                    .expect("Retry-After should be numeric");
                assert!((1..=60).contains(&retry_after));
                assert_error(locked, StatusCode::TOO_MANY_REQUESTS, 1417).await;

                let independent = send_from(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login/password",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                    second_peer,
                )
                .await;
                assert_eq!(independent.status(), StatusCode::OK);
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_login_rate_limit_uses_client_identity_from_trusted_proxy() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-auth-forwarded-rate-limit.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                app_state
                    .admin
                    .auth
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
                let resolver = Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig {
                    trusted_proxy_cidrs: vec![
                        "127.0.0.9/32".parse().expect("test CIDR should parse"),
                    ],
                    max_forwarded_hops: 8,
                }));
                let trusted_peer = SocketAddr::from(([127, 0, 0, 9], 31_003));

                for _ in 0..5 {
                    let mut request = json_request(
                        Method::POST,
                        "/manager/api/auth/login/password",
                        json!({ "password": "wrong horse battery staple" }),
                    );
                    request.headers_mut().insert(
                        "forwarded",
                        "for=198.51.100.10"
                            .parse()
                            .expect("forwarded header should parse"),
                    );
                    let response = send_manager_with_resolver(
                        &app_state,
                        request,
                        Arc::clone(&resolver),
                        trusted_peer,
                    )
                    .await;
                    assert_error(response, StatusCode::UNAUTHORIZED, 1412).await;
                }

                let mut locked_request = json_request(
                    Method::POST,
                    "/manager/api/auth/login/password",
                    json!({ "password": INITIAL_PASSWORD }),
                );
                locked_request.headers_mut().insert(
                    "forwarded",
                    "for=198.51.100.10"
                        .parse()
                        .expect("forwarded header should parse"),
                );
                let locked = send_manager_with_resolver(
                    &app_state,
                    locked_request,
                    Arc::clone(&resolver),
                    trusted_peer,
                )
                .await;
                assert_error(locked, StatusCode::TOO_MANY_REQUESTS, 1417).await;

                let mut independent_request = json_request(
                    Method::POST,
                    "/manager/api/auth/login/password",
                    json!({ "password": INITIAL_PASSWORD }),
                );
                independent_request.headers_mut().insert(
                    "forwarded",
                    "for=198.51.100.11"
                        .parse()
                        .expect("forwarded header should parse"),
                );
                let independent = send_manager_with_resolver(
                    &app_state,
                    independent_request,
                    resolver,
                    trusted_peer,
                )
                .await;
                assert_eq!(independent.status(), StatusCode::OK);
            })
            .await;
    }

    #[tokio::test]
    async fn manager_router_access_guard_enforces_the_in_memory_credential_epoch() {
        let test_db_context = TestDbContext::new_sqlite("manager-access-guard.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let public_status = send_manager(
                    &app_state,
                    empty_request(Method::GET, "/manager/api/auth/bootstrap/status"),
                )
                .await;
                assert_eq!(public_status.status(), StatusCode::OK);

                let missing = send_manager(
                    &app_state,
                    empty_request(Method::GET, "/manager/api/system/overview"),
                )
                .await;
                assert_error(missing, StatusCode::UNAUTHORIZED, 1431).await;

                let invalid = send_manager(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/manager/api/system/overview")
                        .header(header::AUTHORIZATION, "Bearer invalid")
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await;
                assert_error(invalid, StatusCode::UNAUTHORIZED, 1432).await;

                let initial = app_state
                    .admin
                    .auth
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
                let initial_context = decode_access_token(&initial.access_token)
                    .expect("initial access should decode");
                let accepted = send_manager(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/manager/api/system/overview")
                        .header(
                            header::AUTHORIZATION,
                            format!("Bearer {}", initial.access_token),
                        )
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await;
                assert_eq!(accepted.status(), StatusCode::OK);

                let rotated = app_state
                    .admin
                    .auth
                    .rotate_password(
                        &initial_context,
                        SocketAddr::from(([127, 0, 0, 1], 31_000)).ip(),
                        None,
                        INITIAL_PASSWORD,
                        ROTATED_PASSWORD,
                    )
                    .await
                    .expect("rotation should succeed");
                let stale = send_manager(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/manager/api/system/overview")
                        .header(
                            header::AUTHORIZATION,
                            format!("Bearer {}", initial.access_token),
                        )
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await;
                assert_error(stale, StatusCode::UNAUTHORIZED, 1433).await;

                let accepted_after_rotation = send_manager(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/manager/api/system/overview")
                        .header(
                            header::AUTHORIZATION,
                            format!("Bearer {}", rotated.access_token),
                        )
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await;
                assert_eq!(accepted_after_rotation.status(), StatusCode::OK);

                let rotated_context = decode_access_token(&rotated.access_token)
                    .expect("rotated access should decode");
                app_state
                    .admin
                    .auth
                    .logout_session(
                        rotated_context.login_instance_id,
                        rotated_context.credential_epoch,
                    )
                    .await
                    .expect("logout should revoke the rotated session");
                let revoked = send_manager(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/manager/api/system/overview")
                        .header(
                            header::AUTHORIZATION,
                            format!("Bearer {}", rotated.access_token),
                        )
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await;
                assert_error(revoked, StatusCode::UNAUTHORIZED, 1435).await;
            })
            .await;
    }

    #[tokio::test]
    async fn manager_router_access_guard_reports_unavailable_session_registry() {
        let test_db_context =
            TestDbContext::new_sqlite("manager-access-session-unavailable.sqlite");

        test_db_context
            .run_async(async {
                let initial_state = create_test_app_state(test_db_context.clone()).await;
                let tokens = initial_state
                    .admin
                    .auth
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");

                let mut conn = get_connection().expect("connection should load");
                match &mut conn {
                    DbConnection::Postgres(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("session table should drop");
                    }
                    DbConnection::Sqlite(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("session table should drop");
                    }
                }
                drop(conn);

                let unavailable_state = create_test_app_state(test_db_context.clone()).await;
                let response = send_manager(
                    &unavailable_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/manager/api/system/overview")
                        .header(
                            header::AUTHORIZATION,
                            format!("Bearer {}", tokens.access_token),
                        )
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await;
                assert_error(response, StatusCode::SERVICE_UNAVAILABLE, 1436).await;
                assert!(unavailable_state.max_body_size > 0);
            })
            .await;
    }

    #[tokio::test]
    async fn manager_router_access_guard_reports_unavailable_corrupt_credential() {
        let test_db_context = TestDbContext::new_sqlite("manager-access-guard-corrupt.sqlite");

        test_db_context
            .run_async(async {
                ManagerCredential::insert_once(NewManagerCredential {
                    password_verifier: "not-a-phc".to_string(),
                    credential_epoch: uuid::Uuid::new_v4().to_string(),
                    now: 1,
                })
                .expect("corrupt fixture should persist");
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let now = get_current_timestamp();
                let access_token = issue_access_token(
                    crate::database::manager_credential::MANAGER_ID,
                    1,
                    &generate_token_jti(),
                    crate::database::manager_auth_instance::INITIAL_SESSION_VERSION,
                    &uuid::Uuid::new_v4(),
                    now,
                );
                let response = send_manager(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/manager/api/system/overview")
                        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await;
                assert_error(response, StatusCode::SERVICE_UNAVAILABLE, 1434).await;
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_error_enums_have_stable_codes_statuses_and_retry_headers() {
        for (response, expected_retry_after) in [
            (LoginHttpError::SourceRateLimited(37).into_response(), "37"),
            (LoginHttpError::GlobalRateLimited(23).into_response(), "23"),
            (ManagerTotpHttpError::StepReplayed(19).into_response(), "19"),
            (ManagerTotpHttpError::StepStale(17).into_response(), "17"),
            (
                ManagerTotpHttpError::SourceRateLimited(13).into_response(),
                "13",
            ),
            (
                ManagerTotpHttpError::GlobalRateLimited(11).into_response(),
                "11",
            ),
        ] {
            assert_eq!(
                response
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok()),
                Some(expected_retry_after)
            );
        }

        let cases = [
            (
                BootstrapHttpError::AlreadyInitialized.into_response(),
                409,
                1401,
                false,
            ),
            (
                BootstrapHttpError::InvalidRequest.into_response(),
                422,
                1402,
                false,
            ),
            (BootstrapHttpError::Busy.into_response(), 429, 1403, true),
            (
                BootstrapHttpError::Unavailable.into_response(),
                503,
                1404,
                false,
            ),
            (
                BootstrapHttpError::Storage.into_response(),
                503,
                1405,
                false,
            ),
            (
                LoginHttpError::Uninitialized.into_response(),
                409,
                1411,
                false,
            ),
            (
                LoginHttpError::InvalidPassword.into_response(),
                401,
                1412,
                false,
            ),
            (LoginHttpError::Busy.into_response(), 429, 1413, true),
            (
                LoginHttpError::SourceRateLimited(37).into_response(),
                429,
                1417,
                true,
            ),
            (
                LoginHttpError::GlobalRateLimited(23).into_response(),
                429,
                1418,
                true,
            ),
            (
                LoginHttpError::Unavailable.into_response(),
                503,
                1414,
                false,
            ),
            (LoginHttpError::Storage.into_response(), 503, 1415, false),
            (
                LoginHttpError::InvalidRequest.into_response(),
                422,
                1416,
                false,
            ),
            (
                LoginHttpError::ManagerTotpUnavailable.into_response(),
                503,
                1479,
                false,
            ),
            (
                ManagerTotpHttpError::Required.into_response(),
                428,
                1471,
                false,
            ),
            (
                ManagerTotpHttpError::Invalid.into_response(),
                401,
                1472,
                false,
            ),
            (
                ManagerTotpHttpError::StepReplayed(19).into_response(),
                409,
                1473,
                true,
            ),
            (
                ManagerTotpHttpError::StepStale(17).into_response(),
                409,
                1474,
                true,
            ),
            (
                ManagerTotpHttpError::SourceRateLimited(13).into_response(),
                429,
                1475,
                true,
            ),
            (
                ManagerTotpHttpError::GlobalRateLimited(11).into_response(),
                429,
                1476,
                true,
            ),
            (
                ManagerTotpHttpError::ChallengeInvalidOrExpired.into_response(),
                401,
                1477,
                false,
            ),
            (
                ManagerTotpHttpError::ChallengeAttemptsExhausted.into_response(),
                429,
                1478,
                false,
            ),
            (
                ManagerTotpHttpError::Unavailable.into_response(),
                503,
                1479,
                false,
            ),
            (
                ManagerTotpHttpError::StateConflict.into_response(),
                409,
                1480,
                false,
            ),
            (
                ManagerTotpHttpError::CurrentPasswordInvalid.into_response(),
                401,
                1481,
                false,
            ),
            (
                ManagerTotpHttpError::RecoveryCredentialsInvalid.into_response(),
                401,
                1482,
                false,
            ),
            (ManagerTotpHttpError::Busy.into_response(), 429, 1483, true),
            (
                ManagerTotpHttpError::Storage.into_response(),
                503,
                1484,
                false,
            ),
            (
                ManagerTotpHttpError::RequestInvalid.into_response(),
                422,
                1485,
                false,
            ),
            (
                RotatePasswordHttpError::InvalidCurrentPassword.into_response(),
                401,
                1421,
                false,
            ),
            (
                RotatePasswordHttpError::InvalidNewPassword.into_response(),
                422,
                1422,
                false,
            ),
            (
                RotatePasswordHttpError::SamePassword.into_response(),
                422,
                1423,
                false,
            ),
            (
                RotatePasswordHttpError::Busy.into_response(),
                429,
                1424,
                true,
            ),
            (
                RotatePasswordHttpError::EpochConflict.into_response(),
                409,
                1425,
                false,
            ),
            (
                RotatePasswordHttpError::Unavailable.into_response(),
                503,
                1426,
                false,
            ),
            (AccessHttpError::Invalid.into_response(), 401, 1441, false),
            (
                AccessHttpError::Unavailable.into_response(),
                503,
                1443,
                false,
            ),
            (AccessHttpError::Replay.into_response(), 401, 1444, false),
            (
                LogoutHttpError::InvalidCredential.into_response(),
                401,
                1433,
                false,
            ),
            (
                LogoutHttpError::Unavailable.into_response(),
                503,
                1451,
                false,
            ),
            (
                AccessGuardError::MissingAuthorization.into_response(),
                401,
                1431,
                false,
            ),
            (
                AccessGuardError::InvalidToken.into_response(),
                401,
                1432,
                false,
            ),
            (
                AccessGuardError::CredentialMismatch.into_response(),
                401,
                1433,
                false,
            ),
            (
                AccessGuardError::CredentialUnavailable.into_response(),
                503,
                1434,
                false,
            ),
            (
                AccessGuardError::SessionInvalid.into_response(),
                401,
                1435,
                false,
            ),
            (
                AccessGuardError::SessionUnavailable.into_response(),
                503,
                1436,
                false,
            ),
        ];

        for (response, expected_status, expected_code, retry) in cases {
            assert_eq!(response.status().as_u16(), expected_status);
            assert_eq!(response.headers().contains_key(header::RETRY_AFTER), retry);
            let body = response_json(response).await;
            assert_eq!(body["code"].as_u64(), Some(expected_code));
        }
    }

    #[test]
    fn manager_auth_openapi_matches_routes_dtos_and_error_codes() {
        let document: serde_yaml::Value = serde_yaml::from_str(include_str!(
            "../../../docs/openapi/manager-auth.openapi.yaml"
        ))
        .expect("manager auth OpenAPI should parse");
        assert_eq!(document["openapi"].as_str(), Some("3.1.0"));
        assert_eq!(document["info"]["version"].as_str(), Some("1.0.0-pre.6"));
        assert_eq!(
            document["x-cyder-default-cache-control"].as_str(),
            Some("no-store")
        );
        assert_eq!(
            document["x-cyder-refresh-family"]["browserVisible"].as_bool(),
            Some(false)
        );
        assert_eq!(
            document["x-cyder-mediator-cookie"]["productionName"].as_str(),
            Some("__Secure-cyder_manager_session")
        );
        assert_eq!(
            document["x-cyder-mediator-cookie"]["loopbackDevelopmentName"].as_str(),
            Some("cyder_manager_session_dev")
        );
        assert_eq!(
            document["x-cyder-access-guard-error-codes"]
                .as_sequence()
                .expect("access guard codes should be a sequence")
                .iter()
                .map(|value| value.as_u64().expect("access guard code should be numeric"))
                .collect::<Vec<_>>(),
            vec![1431, 1432, 1433, 1434, 1435, 1436]
        );

        let expected = [
            ("/ai/manager/api/auth/bootstrap/status", "get", vec![1404]),
            (
                "/ai/manager/api/auth/bootstrap",
                "post",
                vec![1401, 1402, 1403, 1404, 1405, 1461],
            ),
            (
                "/ai/manager/api/auth/login/password",
                "post",
                vec![1411, 1412, 1413, 1414, 1415, 1416, 1417, 1418, 1461, 1479],
            ),
            (
                "/ai/manager/api/auth/login/totp",
                "post",
                vec![
                    1461, 1472, 1473, 1474, 1475, 1476, 1477, 1478, 1479, 1480, 1483, 1484, 1485,
                ],
            ),
            (
                "/ai/manager/api/auth/reauth",
                "post",
                vec![
                    1431, 1432, 1433, 1434, 1435, 1436, 1441, 1461, 1472, 1473, 1474, 1475, 1476,
                    1479, 1480, 1481, 1483, 1484, 1485, 1492,
                ],
            ),
            (
                "/ai/manager/api/auth/recovery/start",
                "post",
                vec![1461, 1475, 1476, 1479, 1482, 1483, 1484, 1485],
            ),
            (
                "/ai/manager/api/auth/recovery/confirm",
                "post",
                vec![
                    1461, 1472, 1473, 1474, 1475, 1476, 1477, 1478, 1479, 1480, 1483, 1484, 1485,
                ],
            ),
            (
                "/ai/manager/api/auth/totp/status",
                "get",
                vec![1431, 1432, 1433, 1434, 1435, 1436, 1441, 1461, 1479, 1480],
            ),
            (
                "/ai/manager/api/auth/totp/enroll/start",
                "post",
                vec![
                    1431, 1432, 1433, 1434, 1435, 1436, 1441, 1461, 1475, 1476, 1479, 1480, 1481,
                    1483, 1484, 1485,
                ],
            ),
            (
                "/ai/manager/api/auth/totp/enroll/confirm",
                "post",
                vec![
                    1431, 1432, 1433, 1434, 1435, 1436, 1441, 1461, 1472, 1473, 1474, 1475, 1476,
                    1477, 1478, 1479, 1480, 1483, 1484, 1485,
                ],
            ),
            (
                "/ai/manager/api/auth/totp/replace/start",
                "post",
                vec![
                    1431, 1432, 1433, 1434, 1435, 1436, 1441, 1461, 1471, 1472, 1473, 1474, 1475,
                    1476, 1479, 1480, 1481, 1483, 1484, 1485,
                ],
            ),
            (
                "/ai/manager/api/auth/totp/replace/confirm",
                "post",
                vec![
                    1431, 1432, 1433, 1434, 1435, 1436, 1441, 1461, 1472, 1473, 1474, 1475, 1476,
                    1477, 1478, 1479, 1480, 1483, 1484, 1485,
                ],
            ),
            (
                "/ai/manager/api/auth/totp/disable",
                "post",
                vec![
                    1431, 1432, 1433, 1434, 1435, 1436, 1441, 1461, 1471, 1472, 1473, 1474, 1475,
                    1476, 1479, 1480, 1481, 1483, 1484, 1485,
                ],
            ),
            (
                "/ai/manager/api/auth/password/rotate",
                "post",
                vec![
                    1421, 1422, 1423, 1424, 1425, 1426, 1431, 1432, 1433, 1434, 1435, 1436, 1441,
                    1461, 1471, 1472, 1473, 1474, 1475, 1476, 1479, 1480, 1484, 1485,
                ],
            ),
            (
                "/ai/manager/api/auth/access",
                "post",
                vec![1441, 1443, 1444, 1461],
            ),
            ("/ai/manager/api/auth/logout", "post", vec![1451, 1461]),
            (
                "/ai/manager/api/auth/logout_all",
                "post",
                vec![
                    1431, 1432, 1433, 1434, 1435, 1436, 1441, 1461, 1471, 1472, 1473, 1474, 1475,
                    1476, 1479, 1480, 1481, 1483, 1484, 1485,
                ],
            ),
        ];
        for (path, method, error_codes) in expected {
            let operation = &document["paths"][path][method];
            assert!(
                operation.is_mapping(),
                "operation should exist for {method} {path}"
            );
            let actual = operation["x-cyder-error-codes"]
                .as_sequence()
                .expect("error code extension should be a sequence")
                .iter()
                .map(|value| value.as_u64().expect("error code should be numeric"))
                .collect::<Vec<_>>();
            assert_eq!(actual, error_codes);
        }

        assert_eq!(
            document["components"]["schemas"]["PasswordRequest"]["required"]
                .as_sequence()
                .expect("password required fields")
                .len(),
            1
        );
        assert_eq!(
            document["components"]["schemas"]["RotatePasswordRequest"]["required"]
                .as_sequence()
                .expect("rotate required fields")
                .len(),
            2
        );
        assert!(
            document["paths"]["/ai/manager/api/auth/login"].is_null(),
            "removed single-stage login route must not exist"
        );
        assert!(
            document["paths"]["/ai/manager/api/auth/refresh_token"].is_null(),
            "legacy public refresh route must not exist"
        );
        assert!(
            document["components"]["securitySchemes"]["managerRefreshToken"].is_null(),
            "legacy refresh security scheme must not exist"
        );
        assert_eq!(
            document["components"]["schemas"]["AuthAccess"]["required"]
                .as_sequence()
                .expect("AuthAccess required fields")
                .iter()
                .filter_map(serde_yaml::Value::as_str)
                .collect::<Vec<_>>(),
            vec!["access_token", "totp_state", "reauth"]
        );
        assert!(
            document["components"]["schemas"]["AuthAccess"]["properties"]["refresh_token"]
                .is_null()
        );
        assert_eq!(
            document["components"]["schemas"]["TotpCode"]["pattern"].as_str(),
            Some("^[0-9]{6}$")
        );
        assert_eq!(
            document["components"]["parameters"]["ManagerTotpCode"]["name"].as_str(),
            Some("X-Cyder-TOTP-Code")
        );
        assert_eq!(
            document["components"]["parameters"]["SensitiveManagerTotpCode"]["required"].as_bool(),
            Some(false)
        );
        assert_eq!(
            document["x-cyder-secret-governance-reauth"]["fixedLifetimeSeconds"].as_u64(),
            Some(crate::service::admin::auth::SECRET_GOVERNANCE_REAUTH_TTL_SEC as u64)
        );
        assert_eq!(
            document["components"]["schemas"]["ManagerReauthStatus"]["properties"]["scope"]
                ["const"]
                .as_str(),
            Some("secret_governance")
        );
        assert_eq!(
            document["components"]["schemas"]["ManagerReauthRequest"]["discriminator"]
                ["propertyName"]
                .as_str(),
            Some("method")
        );
        assert_eq!(
            document["x-cyder-auth-body-limit-bytes"].as_u64(),
            Some(AUTH_BODY_LIMIT_BYTES as u64)
        );
        assert_eq!(
            document["paths"]["/ai/manager/api/auth/login/password"]["post"]["responses"]["200"]
                ["$ref"]
                .as_str(),
            Some("#/components/responses/LoginPasswordResult")
        );
        assert_eq!(
            document["paths"]["/ai/manager/api/auth/recovery/start"]["post"]["responses"]["200"]
                ["$ref"]
                .as_str(),
            Some("#/components/responses/RecoverySetupWithMediatorCookieDeletion")
        );
        assert_eq!(
            document["paths"]["/ai/manager/api/auth/logout"]["post"]["responses"]["200"]["$ref"]
                .as_str(),
            Some("#/components/responses/EmptyWithMediatorCookieDeletion")
        );
        assert_eq!(
            document["paths"]["/ai/manager/api/auth/logout_all"]["post"]["responses"]["200"]["$ref"]
                .as_str(),
            Some("#/components/responses/LogoutAllWithMediatorCookieDeletion")
        );
        assert_eq!(
            document["paths"]["/ai/manager/api/auth/logout_all"]["post"]["requestBody"]["content"]
                ["application/json"]["schema"]["$ref"]
                .as_str(),
            Some("#/components/schemas/CurrentPasswordRequest")
        );
    }
}
