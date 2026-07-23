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

use crate::config::CONFIG;
use crate::ingress::client_identity::ClientIdentity;
use crate::service::admin::auth::{
    AccessTokenError, AuthTokenPair, BootstrapError, BootstrapStatus, BootstrapStatusError,
    LoginError, LogoutError, RotatePasswordError,
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

#[derive(Serialize)]
struct BootstrapStatusResponse {
    state: &'static str,
}

#[derive(Serialize)]
struct AuthAccessResponse {
    access_token: String,
}

#[derive(Serialize)]
struct LogoutAllResponse {
    revoked_sessions: usize,
}

impl From<&AuthTokenPair> for AuthAccessResponse {
    fn from(value: &AuthTokenPair) -> Self {
        Self {
            access_token: value.access_token.clone(),
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
    Unavailable,
    Storage,
    InvalidRequest,
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
        let mut response = auth_error_response(status, code, message, false);
        if !matches!(self, Self::Unavailable) {
            delete_mediator_cookie(response.headers_mut());
        }
        response
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

fn mediator_cookie_policy() -> MediatorCookiePolicy {
    mediator_cookie_policy_for(
        CONFIG.manager_auth.browser_origin.as_deref(),
        &CONFIG.base_path,
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

fn mediator_cookie_value(token: &str, expires_at: i64) -> HeaderValue {
    mediator_cookie_value_for(&mediator_cookie_policy(), token, expires_at)
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

fn deleted_mediator_cookie_value() -> HeaderValue {
    deleted_mediator_cookie_value_for(&mediator_cookie_policy())
}

fn delete_mediator_cookie(headers: &mut HeaderMap) {
    headers.append(SET_COOKIE, deleted_mediator_cookie_value());
}

fn token_response(pair: AuthTokenPair) -> Response {
    let mut response = HttpResult::new(AuthAccessResponse::from(&pair)).into_response();
    response.headers_mut().append(
        SET_COOKIE,
        mediator_cookie_value(&pair.mediator_token, pair.mediator_expires_at),
    );
    response
}

fn access_response(access_token: String) -> Response {
    HttpResult::new(AuthAccessResponse { access_token }).into_response()
}

fn extract_mediator_context(
    headers: &HeaderMap,
) -> Result<ManagerMediatorContext, AccessHttpError> {
    let cookie_name = mediator_cookie_policy().name;
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

fn browser_origin_allowed(headers: &HeaderMap, client_identity: &ClientIdentity) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    if let Some(configured_origin) = CONFIG.manager_auth.browser_origin.as_deref() {
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
    Extension(client_identity): Extension<ClientIdentity>,
    request: Request,
    next: Next,
) -> Result<Response, BrowserBoundaryHttpError> {
    if request.method() != axum::http::Method::POST {
        return Ok(next.run(request).await);
    }
    if !browser_origin_allowed(request.headers(), &client_identity) {
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
        .map(token_response)
}

async fn login(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    request: Result<Json<PasswordRequest>, JsonRejection>,
) -> Result<Response, LoginHttpError> {
    let Json(request) = request.map_err(|_| LoginHttpError::InvalidRequest)?;
    app_state
        .admin
        .auth
        .login(client_identity.client_ip, &request.password)
        .await
        .map_err(LoginHttpError::from)
        .map(token_response)
}

async fn rotate_password(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
    request: Result<Json<RotatePasswordRequest>, JsonRejection>,
) -> Result<Response, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let Json(request) =
        request.map_err(|_| RotatePasswordHttpError::InvalidNewPassword.into_response())?;
    app_state
        .admin
        .auth
        .rotate_password(
            &auth_context,
            &request.current_password,
            &request.new_password,
        )
        .await
        .map_err(rotate_password_error_response)
        .map(token_response)
}

async fn access(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AccessHttpError> {
    let mediator = extract_mediator_context(&headers)?;
    let access_token = app_state
        .admin
        .auth
        .access_for_session(mediator.login_instance_id, mediator.credential_epoch)
        .await
        .map_err(AccessHttpError::from)?;
    Ok(access_response(access_token))
}

async fn logout(State(app_state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Ok(mediator) = extract_mediator_context(&headers) else {
        return logout_success_response();
    };
    match app_state
        .admin
        .auth
        .logout_session(mediator.login_instance_id, mediator.credential_epoch)
        .await
    {
        Ok(()) => logout_success_response(),
        Err(error) => {
            let mut response = LogoutHttpError::from(error).into_response();
            delete_mediator_cookie(response.headers_mut());
            response
        }
    }
}

async fn logout_all(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let (auth_context, _) = require_access_and_mediator(&app_state, &headers)?;
    let revoked_sessions = app_state
        .admin
        .auth
        .logout_all(&auth_context)
        .await
        .map_err(logout_all_error_response)?;
    let mut response = HttpResult::new(LogoutAllResponse { revoked_sessions }).into_response();
    delete_mediator_cookie(response.headers_mut());
    Ok(response)
}

fn require_access_and_mediator(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<(ManagerAuthContext, ManagerMediatorContext), Response> {
    let mediator = extract_mediator_context(headers).map_err(IntoResponse::into_response)?;
    let access = validate_access_headers(app_state, headers).map_err(|error| {
        let clear_cookie = matches!(
            error,
            AccessGuardError::CredentialMismatch | AccessGuardError::SessionInvalid
        );
        let mut response = error.into_response();
        if clear_cookie {
            delete_mediator_cookie(response.headers_mut());
        }
        response
    })?;
    if access.login_instance_id != mediator.login_instance_id
        || access.credential_epoch != mediator.credential_epoch
        || access.manager_id != mediator.manager_id
        || access.manager_subject != mediator.manager_subject
    {
        return Err(AccessHttpError::Invalid.into_response());
    }
    Ok((access, mediator))
}

fn rotate_password_error_response(error: RotatePasswordError) -> Response {
    let clear_cookie = matches!(error, RotatePasswordError::EpochConflict);
    let mut response = RotatePasswordHttpError::from(error).into_response();
    if clear_cookie {
        delete_mediator_cookie(response.headers_mut());
    }
    response
}

fn logout_success_response() -> Response {
    let mut response = HttpResult::new(()).into_response();
    delete_mediator_cookie(response.headers_mut());
    response
}

fn logout_all_error_response(error: LogoutError) -> Response {
    let clear_cookie = matches!(error, LogoutError::InvalidCredential);
    let mut response = LogoutHttpError::from(error).into_response();
    if clear_cookie {
        delete_mediator_cookie(response.headers_mut());
    }
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

impl From<LoginError> for LoginHttpError {
    fn from(error: LoginError) -> Self {
        match error {
            LoginError::Uninitialized => Self::Uninitialized,
            LoginError::InvalidPassword => Self::InvalidPassword,
            LoginError::SourceRateLimited { retry_after } => Self::SourceRateLimited(retry_after),
            LoginError::GlobalRateLimited { retry_after } => Self::GlobalRateLimited(retry_after),
            LoginError::Busy => Self::Busy,
            LoginError::Unavailable => Self::Unavailable,
            LoginError::Storage => Self::Storage,
        }
    }
}

impl From<RotatePasswordError> for RotatePasswordHttpError {
    fn from(error: RotatePasswordError) -> Self {
        match error {
            RotatePasswordError::InvalidCurrentPassword => Self::InvalidCurrentPassword,
            RotatePasswordError::PasswordPolicy(_) => Self::InvalidNewPassword,
            RotatePasswordError::SamePassword => Self::SamePassword,
            RotatePasswordError::Busy => Self::Busy,
            RotatePasswordError::EpochConflict => Self::EpochConflict,
            RotatePasswordError::Unavailable | RotatePasswordError::Storage => Self::Unavailable,
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

pub fn create_auth_router(_app_state: Arc<AppState>) -> StateRouter {
    create_state_router().nest(
        "/auth",
        create_state_router()
            .route("/bootstrap/status", get(bootstrap_status))
            .route("/bootstrap", post(bootstrap))
            .route("/login", post(login))
            .route("/access", post(access))
            .route("/password/rotate", post(rotate_password))
            .route("/logout", post(logout))
            .route("/logout_all", post(logout_all))
            .layer(middleware::from_fn(manager_auth_browser_boundary))
            .layer(DefaultBodyLimit::max(AUTH_BODY_LIMIT_BYTES)),
    )
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::{Method, Request, StatusCode, header},
        response::IntoResponse,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::{
        config::ClientIdentityConfig,
        controller::create_manager_router,
        database::{
            DbConnection, TestDbContext, get_connection,
            manager_credential::{ManagerCredential, NewManagerCredential},
        },
        ingress::client_identity::{ClientIdentity, ClientIdentityResolver, ClientIdentitySource},
        service::app_state::{AppState, create_test_app_state},
        utils::auth::{
            AccessGuardError, decode_access_token, generate_token_jti, get_current_timestamp,
            issue_access_token,
        },
    };

    use super::{
        AccessHttpError, BootstrapHttpError, LoginHttpError, LogoutHttpError,
        RotatePasswordHttpError, create_auth_router, deleted_mediator_cookie_value_for,
        mediator_cookie_policy_for, mediator_cookie_value_for,
    };
    use diesel::RunQueryDsl;

    const INITIAL_PASSWORD: &str = "correct horse battery staple";
    const ROTATED_PASSWORD: &str = "correct horse battery staple rotated";

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
                        "/auth/login",
                        json!({ "key": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_error(legacy_key, StatusCode::UNPROCESSABLE_ENTITY, 1416).await;

                let login_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login",
                        json!({ "password": INITIAL_PASSWORD }),
                    ),
                )
                .await;
                assert_eq!(login_response.status(), StatusCode::OK);
                let login_cookie = mediator_cookie(&login_response);
                let login_access = access_token(&response_json(login_response).await);

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
                        "/auth/login",
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
                        json!({}),
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
                        json!({}),
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
                        json!({}),
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
                assert_error(logout_all_failure, StatusCode::SERVICE_UNAVAILABLE, 1451).await;

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

                let mut missing_origin = json_request(
                    Method::POST,
                    "/auth/bootstrap",
                    json!({ "password": INITIAL_PASSWORD }),
                );
                missing_origin.headers_mut().remove(header::ORIGIN);
                assert_error(
                    send(&app_state, missing_origin).await,
                    StatusCode::FORBIDDEN,
                    1461,
                )
                .await;

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
                        "/auth/login",
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
                    "/auth/login",
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
                        "/auth/login",
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
                        "/manager/api/auth/login",
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
                    "/manager/api/auth/login",
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
                    "/manager/api/auth/login",
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
                    .rotate_password(&initial_context, INITIAL_PASSWORD, ROTATED_PASSWORD)
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
        assert_eq!(document["info"]["version"].as_str(), Some("1.0.0-pre.4"));
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
                "/ai/manager/api/auth/login",
                "post",
                vec![1411, 1412, 1413, 1414, 1415, 1416, 1417, 1418, 1461],
            ),
            (
                "/ai/manager/api/auth/password/rotate",
                "post",
                vec![
                    1421, 1422, 1423, 1424, 1425, 1426, 1431, 1432, 1433, 1434, 1435, 1436, 1441,
                    1461,
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
                vec![1431, 1432, 1433, 1434, 1435, 1436, 1441, 1451, 1461],
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
            vec!["access_token"]
        );
        assert!(
            document["components"]["schemas"]["AuthAccess"]["properties"]["refresh_token"]
                .is_null()
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
    }
}
