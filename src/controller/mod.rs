use api_key::create_api_key_router;
use axum::{http, middleware, response::IntoResponse, Router};
use model::create_model_router;
use provider::create_provider_router;
use proxy::create_proxy_router;
use record::create_record_router;
use auth::create_auth_router;
use crate::utils::auth::authorization_access_middleware;
use model_transform::create_model_transform_router;

use tower_http::services::ServeDir;

mod api_key;
mod error;
mod model;
mod provider;
mod proxy;
mod record;
mod auth;
mod model_transform;

pub use error::BaseError;

pub fn create_router() -> Router {
    Router::new()
        .merge(create_manager_router())
        .merge(create_proxy_router())
        .fallback(handle_404)
}

fn create_manager_router() -> Router {
    let serve_dir = ServeDir::new("public");

    Router::new().nest(
        "/manager",
        Router::new()
            .merge(create_record_router())
            .merge(create_provider_router())
            .merge(create_api_key_router())
            .merge(create_model_router())
            .merge(create_model_transform_router())
            .layer(middleware::from_fn(authorization_access_middleware))
            .merge(create_auth_router())
            .nest_service("/settings", serve_dir),
    )
}

async fn handle_404() -> impl IntoResponse {
    (http::StatusCode::NOT_FOUND, "not found")
}
