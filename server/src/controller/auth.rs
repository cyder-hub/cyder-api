use crate::service::app_state::{StateRouter, create_state_router};
use crate::utils::{
    HttpResult,
    auth::{
        AuthError, ManagerAuthContext, authorization_access_middleware,
        extract_bearer_token_from_headers,
    },
};
use axum::{Extension, extract::State, http::HeaderMap, middleware, routing::post};
use std::sync::Arc;

use super::error::BaseError;

use axum::Json;
use serde::{Deserialize, Serialize};

use crate::service::admin::auth::AuthTokenPair;
use crate::service::app_state::AppState;

#[derive(Debug, Deserialize)]
struct LoginRequest {
    key: String,
}

#[derive(Debug, Serialize)]
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

async fn login(
    State(app_state): State<Arc<AppState>>,
    Json(login_request): Json<LoginRequest>,
) -> Result<HttpResult<AuthTokenPairResponse>, BaseError> {
    app_state
        .admin
        .auth
        .login(&login_request.key)
        .await
        .map(AuthTokenPairResponse::from)
        .map(HttpResult::new)
}

async fn refresh_token(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<HttpResult<AuthTokenPairResponse>, BaseError> {
    let refresh_token = extract_bearer_token_from_headers(&headers).map_err(auth_error_to_base)?;
    app_state
        .admin
        .auth
        .refresh(refresh_token)
        .await
        .map(AuthTokenPairResponse::from)
        .map(HttpResult::new)
}

async fn logout(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_context): Extension<ManagerAuthContext>,
) -> Result<HttpResult<()>, BaseError> {
    app_state.admin.auth.logout(&auth_context).await?;
    Ok(HttpResult::new(()))
}

fn auth_error_to_base(error: AuthError) -> BaseError {
    match error {
        AuthError::Empty => {
            BaseError::Unauthorized(Some("header Authorization is needed".to_string()))
        }
        AuthError::Invalid => BaseError::Unauthorized(Some("token invalid or expired".to_string())),
    }
}

pub fn create_auth_router() -> StateRouter {
    let logout_router = create_state_router()
        .route("/logout", post(logout))
        .layer(middleware::from_fn(authorization_access_middleware));

    create_state_router().nest(
        "/auth",
        create_state_router()
            .route("/login", post(login))
            .route("/refresh_token", post(refresh_token))
            .merge(logout_router),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode, header},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::{
        config::CONFIG,
        database::TestDbContext,
        database::manager_auth_instance::MANAGER_ID,
        service::app_state::{AppState, create_test_app_state},
        utils::auth::{
            decode_access_token, get_current_timestamp, issue_access_token_with_expiration_for_test,
        },
    };

    use super::create_auth_router;

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_auth_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("auth router should respond")
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

    fn empty_auth_request(method: Method, uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("request should build")
    }

    fn empty_auth_header_request(method: Method, uri: &str, auth_header: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, auth_header)
            .body(Body::empty())
            .expect("request should build")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

    fn token_pair(body: &Value) -> (String, String) {
        let refresh_token = body["data"]["refresh_token"]
            .as_str()
            .expect("refresh token should exist")
            .to_string();
        let access_token = body["data"]["access_token"]
            .as_str()
            .expect("access token should exist")
            .to_string();
        (refresh_token, access_token)
    }

    #[tokio::test]
    async fn auth_http_login_refresh_rotation_and_logout_contract() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-contract.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let login_response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/auth/login",
                        json!({ "key": CONFIG.secret_key.as_str() }),
                    ),
                )
                .await;
                assert_eq!(login_response.status(), StatusCode::OK);
                let login_body = response_json(login_response).await;
                let (refresh_token, _access_token) = token_pair(&login_body);

                let refresh_response = send(
                    &app_state,
                    empty_auth_request(Method::POST, "/auth/refresh_token", &refresh_token),
                )
                .await;
                assert_eq!(refresh_response.status(), StatusCode::OK);
                let refresh_body = response_json(refresh_response).await;
                let (rotated_refresh_token, rotated_access_token) = token_pair(&refresh_body);

                let stale_refresh_response = send(
                    &app_state,
                    empty_auth_request(Method::POST, "/auth/refresh_token", &refresh_token),
                )
                .await;
                assert_eq!(stale_refresh_response.status(), StatusCode::UNAUTHORIZED);

                let logout_response = send(
                    &app_state,
                    empty_auth_request(Method::POST, "/auth/logout", &rotated_access_token),
                )
                .await;
                assert_eq!(logout_response.status(), StatusCode::OK);

                let offline_access_still_decodes =
                    decode_access_token(&rotated_access_token).expect("access should decode");
                assert!(offline_access_still_decodes.expires_at > get_current_timestamp());

                let second_logout_response = send(
                    &app_state,
                    empty_auth_request(Method::POST, "/auth/logout", &rotated_access_token),
                )
                .await;
                assert_eq!(
                    second_logout_response.status(),
                    StatusCode::OK,
                    "logout does not online-revoke unexpired access tokens"
                );

                let logged_out_refresh_response = send(
                    &app_state,
                    empty_auth_request(Method::POST, "/auth/refresh_token", &rotated_refresh_token),
                )
                .await;
                assert_eq!(
                    logged_out_refresh_response.status(),
                    StatusCode::UNAUTHORIZED
                );

                let now = get_current_timestamp();
                let expired_access = issue_access_token_with_expiration_for_test(
                    MANAGER_ID,
                    offline_access_still_decodes.login_instance_id,
                    "expired-access",
                    now - 1_200,
                    now - 600,
                );
                let expired_logout_response = send(
                    &app_state,
                    empty_auth_request(Method::POST, "/auth/logout", &expired_access),
                )
                .await;
                assert_eq!(expired_logout_response.status(), StatusCode::UNAUTHORIZED);
            })
            .await;
    }

    #[tokio::test]
    async fn auth_http_rejects_bad_authorization_header_shapes() {
        let test_db_context = TestDbContext::new_sqlite("controller-auth-header.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let bad_refresh_header = send(
                    &app_state,
                    empty_auth_header_request(Method::POST, "/auth/refresh_token", "Token abc"),
                )
                .await;
                assert_eq!(bad_refresh_header.status(), StatusCode::UNAUTHORIZED);

                let bad_logout_header = send(
                    &app_state,
                    empty_auth_header_request(Method::POST, "/auth/logout", "Bearer abc extra"),
                )
                .await;
                assert_eq!(bad_logout_header.status(), StatusCode::UNAUTHORIZED);
            })
            .await;
    }
}
