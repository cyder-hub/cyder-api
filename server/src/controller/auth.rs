use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header::RETRY_AFTER},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ingress::client_identity::ClientIdentity;
use crate::service::admin::auth::{
    AuthTokenPair, BootstrapError, BootstrapStatus, BootstrapStatusError, LoginError, LogoutError,
    RefreshError, RotatePasswordError,
};
use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::utils::{
    HttpResult,
    auth::{
        BearerTokenError, ManagerAuthContext, authorization_access_middleware,
        extract_bearer_token_from_headers,
    },
};

const AUTH_BODY_LIMIT_BYTES: usize = 4 * 1024;

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
struct AuthTokenPairResponse {
    refresh_token: String,
    access_token: String,
}

impl From<AuthTokenPair> for AuthTokenPairResponse {
    fn from(value: AuthTokenPair) -> Self {
        Self {
            refresh_token: value.refresh_token,
            access_token: value.access_token,
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
enum RefreshHttpError {
    Invalid,
    EpochMismatch,
    Replay,
    Unavailable,
}

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

impl IntoResponse for RefreshHttpError {
    fn into_response(self) -> Response {
        let (code, message) = match self {
            Self::Invalid => (1441, "invalid refresh token"),
            Self::EpochMismatch => (1442, "manager credential no longer valid"),
            Self::Unavailable => (1443, "manager refresh unavailable"),
            Self::Replay => (1444, "refresh token replay detected"),
        };
        let status = if matches!(self, Self::Unavailable) {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::UNAUTHORIZED
        };
        auth_error_response(status, code, message, false)
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
) -> Result<HttpResult<AuthTokenPairResponse>, BootstrapHttpError> {
    let Json(request) = request.map_err(|_| BootstrapHttpError::InvalidRequest)?;
    app_state
        .admin
        .auth
        .bootstrap(&request.password)
        .await
        .map_err(BootstrapHttpError::from)
        .map(AuthTokenPairResponse::from)
        .map(HttpResult::new)
}

async fn login(
    State(app_state): State<Arc<AppState>>,
    Extension(client_identity): Extension<ClientIdentity>,
    request: Result<Json<PasswordRequest>, JsonRejection>,
) -> Result<HttpResult<AuthTokenPairResponse>, LoginHttpError> {
    let Json(request) = request.map_err(|_| LoginHttpError::InvalidRequest)?;
    app_state
        .admin
        .auth
        .login(client_identity.client_ip, &request.password)
        .await
        .map_err(LoginHttpError::from)
        .map(AuthTokenPairResponse::from)
        .map(HttpResult::new)
}

async fn rotate_password(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_context): Extension<ManagerAuthContext>,
    request: Result<Json<RotatePasswordRequest>, JsonRejection>,
) -> Result<HttpResult<AuthTokenPairResponse>, RotatePasswordHttpError> {
    let Json(request) = request.map_err(|_| RotatePasswordHttpError::InvalidNewPassword)?;
    app_state
        .admin
        .auth
        .rotate_password(
            &auth_context,
            &request.current_password,
            &request.new_password,
        )
        .await
        .map_err(RotatePasswordHttpError::from)
        .map(AuthTokenPairResponse::from)
        .map(HttpResult::new)
}

async fn refresh_token(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<HttpResult<AuthTokenPairResponse>, RefreshHttpError> {
    let refresh_token = extract_bearer_token_from_headers(&headers)
        .map_err(|_: BearerTokenError| RefreshHttpError::Invalid)?;
    app_state
        .admin
        .auth
        .refresh(refresh_token)
        .await
        .map_err(RefreshHttpError::from)
        .map(AuthTokenPairResponse::from)
        .map(HttpResult::new)
}

async fn logout(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_context): Extension<ManagerAuthContext>,
) -> Result<HttpResult<()>, LogoutHttpError> {
    app_state
        .admin
        .auth
        .logout(&auth_context)
        .await
        .map_err(LogoutHttpError::from)?;
    Ok(HttpResult::new(()))
}

async fn logout_all(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_context): Extension<ManagerAuthContext>,
) -> Result<HttpResult<()>, LogoutHttpError> {
    app_state
        .admin
        .auth
        .logout_all(&auth_context)
        .await
        .map_err(LogoutHttpError::from)?;
    Ok(HttpResult::new(()))
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

impl From<RefreshError> for RefreshHttpError {
    fn from(error: RefreshError) -> Self {
        match error {
            RefreshError::Invalid => Self::Invalid,
            RefreshError::EpochMismatch => Self::EpochMismatch,
            RefreshError::Replay => Self::Replay,
            RefreshError::Unavailable | RefreshError::Storage => Self::Unavailable,
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
    let protected_router = create_state_router()
        .route("/password/rotate", post(rotate_password))
        .route("/logout", post(logout))
        .route("/logout_all", post(logout_all))
        .layer(middleware::from_fn_with_state(
            app_state,
            authorization_access_middleware,
        ));

    create_state_router().nest(
        "/auth",
        create_state_router()
            .route("/bootstrap/status", get(bootstrap_status))
            .route("/bootstrap", post(bootstrap))
            .route("/login", post(login))
            .route("/refresh_token", post(refresh_token))
            .merge(protected_router)
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
        BootstrapHttpError, LoginHttpError, LogoutHttpError, RefreshHttpError,
        RotatePasswordHttpError, create_auth_router,
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
            .body(Body::from(
                serde_json::to_vec(&payload).expect("payload should serialize"),
            ))
            .expect("request should build")
    }

    fn empty_request(method: Method, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request should build")
    }

    fn auth_request(method: Method, uri: &str, token: &str, payload: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
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

    fn token_pair(body: &Value) -> (String, String) {
        (
            body["data"]["refresh_token"]
                .as_str()
                .expect("refresh token should exist")
                .to_string(),
            body["data"]["access_token"]
                .as_str()
                .expect("access token should exist")
                .to_string(),
        )
    }

    async fn assert_error(response: axum::response::Response, status: StatusCode, code: u64) {
        assert_eq!(response.status(), status);
        let body = response_json(response).await;
        assert_eq!(body["code"].as_u64(), Some(code));
    }

    #[tokio::test]
    async fn auth_http_bootstrap_login_refresh_rotate_and_logout_contract() {
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
                let bootstrap_body = response_json(bootstrap_response).await;
                let (bootstrap_refresh, _) = token_pair(&bootstrap_body);

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
                let (_, login_access) = token_pair(&response_json(login_response).await);

                let rotate_response = send(
                    &app_state,
                    auth_request(
                        Method::POST,
                        "/auth/password/rotate",
                        &login_access,
                        json!({
                            "current_password": INITIAL_PASSWORD,
                            "new_password": ROTATED_PASSWORD,
                        }),
                    ),
                )
                .await;
                assert_eq!(rotate_response.status(), StatusCode::OK);
                let (rotated_refresh, rotated_access) =
                    token_pair(&response_json(rotate_response).await);

                let stale_refresh = send(
                    &app_state,
                    auth_request(
                        Method::POST,
                        "/auth/refresh_token",
                        &bootstrap_refresh,
                        json!({}),
                    ),
                )
                .await;
                assert_error(stale_refresh, StatusCode::UNAUTHORIZED, 1442).await;

                let refresh_response = send(
                    &app_state,
                    auth_request(
                        Method::POST,
                        "/auth/refresh_token",
                        &rotated_refresh,
                        json!({}),
                    ),
                )
                .await;
                assert_eq!(refresh_response.status(), StatusCode::OK);
                let (_, refreshed_access) = token_pair(&response_json(refresh_response).await);

                let replay = send(
                    &app_state,
                    auth_request(
                        Method::POST,
                        "/auth/refresh_token",
                        &rotated_refresh,
                        json!({}),
                    ),
                )
                .await;
                assert_error(replay, StatusCode::UNAUTHORIZED, 1444).await;

                let stale_access_logout = send(
                    &app_state,
                    auth_request(Method::POST, "/auth/logout", &rotated_access, json!({})),
                )
                .await;
                assert_error(stale_access_logout, StatusCode::UNAUTHORIZED, 1435).await;

                let logout_response = send(
                    &app_state,
                    auth_request(Method::POST, "/auth/logout", &refreshed_access, json!({})),
                )
                .await;
                assert_eq!(logout_response.status(), StatusCode::OK);
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_logout_all_revokes_current_and_other_sessions() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-logout-all.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let first = app_state
                    .admin
                    .auth
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
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
                let (_, second_access) = token_pair(&response_json(second_response).await);

                let logout_all_response = send(
                    &app_state,
                    auth_request(Method::POST, "/auth/logout_all", &second_access, json!({})),
                )
                .await;
                assert_eq!(logout_all_response.status(), StatusCode::OK);

                for access_token in [first.access_token, second_access] {
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
    async fn auth_http_rejects_bad_headers_and_oversized_or_legacy_bodies() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-rejections.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let missing_refresh = send(
                    &app_state,
                    empty_request(Method::POST, "/auth/refresh_token"),
                )
                .await;
                assert_error(missing_refresh, StatusCode::UNAUTHORIZED, 1441).await;

                let bad_logout = send(
                    &app_state,
                    auth_request(Method::POST, "/auth/logout", "invalid", json!({})),
                )
                .await;
                assert_error(bad_logout, StatusCode::UNAUTHORIZED, 1432).await;

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
                let first_peer = SocketAddr::from(([192, 0, 2, 10], 31_001));
                let second_peer = SocketAddr::from(([192, 0, 2, 11], 31_002));

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
                        "10.0.0.0/8".parse().expect("test CIDR should parse"),
                    ],
                    max_forwarded_hops: 8,
                }));
                let trusted_peer = SocketAddr::from(([10, 0, 0, 9], 31_003));

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
                    .logout(&rotated_context)
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
            (RefreshHttpError::Invalid.into_response(), 401, 1441, false),
            (
                RefreshHttpError::EpochMismatch.into_response(),
                401,
                1442,
                false,
            ),
            (
                RefreshHttpError::Unavailable.into_response(),
                503,
                1443,
                false,
            ),
            (RefreshHttpError::Replay.into_response(), 401, 1444, false),
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
                vec![1401, 1402, 1403, 1404, 1405],
            ),
            (
                "/ai/manager/api/auth/login",
                "post",
                vec![1411, 1412, 1413, 1414, 1415, 1416, 1417, 1418],
            ),
            (
                "/ai/manager/api/auth/password/rotate",
                "post",
                vec![
                    1421, 1422, 1423, 1424, 1425, 1426, 1431, 1432, 1433, 1434, 1435, 1436,
                ],
            ),
            (
                "/ai/manager/api/auth/refresh_token",
                "post",
                vec![1441, 1442, 1443, 1444],
            ),
            (
                "/ai/manager/api/auth/logout",
                "post",
                vec![1431, 1432, 1433, 1434, 1435, 1436, 1451],
            ),
            (
                "/ai/manager/api/auth/logout_all",
                "post",
                vec![1431, 1432, 1433, 1434, 1435, 1436, 1451],
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
    }
}
