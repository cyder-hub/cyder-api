use crate::utils::auth::authorization_access_middleware;
use api_key::create_api_key_router;
use auth::create_auth_router;
use limit_strategy::create_limit_strategy_router; // Add this import
use axum::{
    http::{self, header::CACHE_CONTROL, HeaderValue},
    middleware,
    response::IntoResponse,
    Router,
};
use model::create_model_router;
use model_transform::create_model_transform_router;
use provider::create_provider_router;
use proxy::create_proxy_router;
use record::create_record_router;

use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

mod api_key;
mod auth;
mod error;
mod limit_strategy; // Add this module declaration
mod model;
mod model_transform;
mod provider;
pub mod proxy;
mod record;

pub use error::BaseError;

pub fn create_router() -> Router {
    Router::new()
        .merge(create_manager_router())
        .merge(create_proxy_router())
        .fallback(handle_404)
}

fn create_manager_router() -> Router {
    let serve_dir = ServeDir::new("public");
    let serve_vendor_dir = ServeDir::new("public/vendor");

    let static_router = Router::new()
        .nest_service("/settings", serve_dir)
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        ))
        .nest_service("/settings/vendor", serve_vendor_dir);

    Router::new().nest(
        "/manager",
        Router::new()
            .merge(create_record_router())
            .merge(create_provider_router())
            .merge(create_api_key_router())
            .merge(create_model_router())
            .merge(create_model_transform_router())
            .merge(create_limit_strategy_router()) // Add this line
            .layer(middleware::from_fn(authorization_access_middleware))
            .merge(create_auth_router())
            .merge(static_router),
    )
}

async fn handle_404() -> impl IntoResponse {
    (http::StatusCode::NOT_FOUND, "not found")
}
