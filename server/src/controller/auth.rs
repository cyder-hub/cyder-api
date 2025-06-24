use axum::{middleware, routing::post, Extension}; // Router will be replaced by StateRouter
use crate::service::app_state::{create_state_router, StateRouter};
use crate::utils::{
    auth::{
        authorization_refresh_middleware, issue_access_token, issue_refresh_token, RefreshJwtResult,
    },
    HttpResult,
};

use super::error::BaseError;

use axum::Json;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct LoginRequest {
    key: String,
}

async fn login(Json(login_request): Json<LoginRequest>) -> Result<HttpResult<String>, BaseError> {
    use crate::config::CONFIG;
    if login_request.key == CONFIG.secret_key {
        let refresh_token = issue_refresh_token(0);
        Ok(HttpResult::new(refresh_token))
    } else {
        Err(BaseError::Unauthorized(Some("Invalid key".to_string())))
    }
}

async fn refresh_token(
    Extension(jwt_result): Extension<RefreshJwtResult>,
) -> Result<HttpResult<String>, BaseError> {
    Ok(HttpResult::new(issue_access_token(
        jwt_result.id,
        jwt_result.jwt_id.to_string(),
    )))
}

pub fn create_auth_router() -> StateRouter {
    let refresh_token_router = create_state_router()
        .route("/refresh_token", post(refresh_token))
        .layer(middleware::from_fn(authorization_refresh_middleware));

    create_state_router().nest(
        "/auth",
        create_state_router()
            .route("/login", post(login))
            .merge(refresh_token_router),
    )
}
